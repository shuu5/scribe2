//! 台帳 lint（設計 docs/design/contract-source.md §6・契約表の行 e・接頭辞 `ledger_lint_`）。
//!
//! toy repo（契約表を持つ設計 doc 1 本・行 `a` だけ）に対して実 binary の `doctor --repo` を撃ち、台帳は PATH の先頭に
//! 置いた偽の client（引数を記録して fixture の JSON を返す shim）が答える。

use super::{make_tmp_dir, TmpDir};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 設計 doc（契約表の行 `a` だけ）。
const DOC: &str = "# toy\n\n## 1. one\n\n本文。\n\n\
<!-- contracts:begin -->\nschema = 1\n\n\
[[contract]]\nid = \"a\"\ntitle = \"a\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"seed\"]\nverify = [\"cargo nextest run -p x --no-tests=fail a_\"]\nsize = \"S\"\ndone = \"a\"\n\
<!-- contracts:end -->\n";

/// 解ける pointer。
const RESOLVED: &str = "design = docs/design/toy.md#a";

/// 歯の置き場（toy repo・偽の client の dir・argv の記録）。
struct Place {
    dir: TmpDir,
    repo: PathBuf,
    bin: PathBuf,
    record: PathBuf,
}

/// git を 1 回撃つ（rc 0 か）。
fn git(repo: &Path, args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(repo).args(args).output().is_ok_and(|out| out.status.success())
}

/// toy repo を作る（設計 doc と `seed` を index に載せる）。
fn place() -> Option<Place> {
    let dir = make_tmp_dir()?.canonical()?;
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join("docs/design")).ok()?;
    fs::write(repo.join("docs/design/toy.md"), DOC).ok()?;
    fs::write(repo.join("seed"), "seed\n").ok()?;
    (git(&repo, &["init", "-q"]) && git(&repo, &["add", "-A"])).then_some(())?;
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).ok()?;
    let record = dir.join("bd-args");
    Some(Place { dir, repo, bin, record })
}

/// JSON の文字列（`"` と `\` と改行を escape）。
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

/// 台帳の 1 件（`bd list --json` の要素の形）。
struct Bead<'a> {
    id: &'a str,
    status: &'a str,
    memo: bool,
    acceptance: &'a str,
    description: &'a str,
    notes: &'a str,
}

impl<'a> Bead<'a> {
    /// acceptance を持つ open の契約。
    fn contract(id: &'a str, acceptance: &'a str) -> Self {
        Self { id, status: "open", memo: false, acceptance, description: "", notes: "" }
    }

    /// 本文を持つ open の memo。
    fn memo(id: &'a str, description: &'a str) -> Self {
        Self { id, status: "open", memo: true, acceptance: "", description, notes: "" }
    }

    /// JSON の 1 要素。
    fn json(&self) -> String {
        let labels = if self.memo { "[\"intake:memo\",\"doc:toy\"]" } else { "[\"doc:toy\"]" };
        format!(
            "{{\"id\":{},\"title\":\"t\",\"status\":{},\"issue_type\":\"task\",\"labels\":{labels},\"acceptance_criteria\":{},\"description\":{},\"notes\":{},\"dependencies\":[]}}",
            quoted(self.id),
            quoted(self.status),
            quoted(self.acceptance),
            quoted(self.description),
            quoted(self.notes),
        )
    }
}

/// 偽の client を書く（argv を 1 行で記録し、`body` の shell 本文を実行する）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_client(place: &Place, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let shim = place.bin.join("bd");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n{body}\n", place.record.display());
    fs::write(&shim, script).expect("偽の client を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("実行権を付ける");
}

/// 偽の client が `beads` の JSON を返す形にする。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn serve(place: &Place, beads: &[Bead<'_>]) {
    let items: Vec<String> = beads.iter().map(Bead::json).collect();
    let json = place.dir.join("ledger.json");
    fs::write(&json, format!("[{}]\n", items.join(","))).expect("fixture を書ける");
    write_client(place, &format!("cat '{}'", json.display()));
}

/// `doctor <args>` を `path` の PATH で撃ち、rc 0 を確かめて stdout を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn doctor_run(args: &[&str], path: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_scribe2")).arg("doctor").args(args).env("PATH", path).output().expect("binary を起動できる");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "doctor は判定しない（rc 0）: {stdout}");
    stdout
}

/// `doctor --repo` を `path` の PATH で撃ち、rc 0 を確かめて `ledger:` の行（ちょうど 1 本）を返す。
fn line_with(place: &Place, path: &str) -> String {
    line_args(place, &[], path)
}

/// `doctor --repo <extra>` を `path` の PATH で撃ち、`ledger:` の行（ちょうど 1 本）を返す。
fn line_args(place: &Place, extra: &[&str], path: &str) -> String {
    let mut args = vec!["--repo", place.repo.to_str().unwrap_or_default()];
    args.extend(extra);
    let stdout = doctor_run(&args, path);
    let lines: Vec<&str> = stdout.lines().filter(|line| line.starts_with("ledger:")).collect();
    assert_eq!(lines.len(), 1, "台帳 lint の行はちょうど 1 本: {stdout}");
    lines.first().map(|line| (*line).to_owned()).unwrap_or_default()
}

/// 偽の client を先頭に積んだ PATH で撃つ。
fn line(place: &Place) -> String {
    line_with(place, &format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default()))
}

/// 偽の client の起動の記録（1 起動 1 行）。
fn calls(place: &Place) -> Vec<String> {
    fs::read_to_string(&place.record).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// (a) 3 つの欠陥を**件数を違えて**持つ fixture（pointer の解けない契約 1・本文を持つ契約 2・pointer 無しの memo 3）で
/// 件数と母集団が同じ行に出て、id が 6 つとも欠陥ごとに名指される。台帳は doctor の 2 行で readonly の 1 回だけ読む。
#[test]
fn ledger_lint_names_each_defect_with_its_count_and_population() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let bodied = format!("{RESOLVED}\n本文の行");
    let beads = [
        Bead::contract("s2-l.u1", "design = docs/design/toy.md#z"),
        Bead::contract("s2-l.b1", &bodied),
        Bead::contract("s2-l.b2", &bodied),
        Bead::contract("s2-l.ok", RESOLVED),
        Bead::memo("s2-l.m1", "## memo\n本文"),
        Bead::memo("s2-l.m2", "## memo\n本文"),
        Bead::memo("s2-l.m3", "## memo\n本文"),
        Bead { status: "closed", ..Bead::contract("s2-l.k1", "design = docs/design/toy.md#z") },
    ];
    serve(&place, &beads);
    let want = "ledger: open=7 contracts=4 unresolved=1 bodied=2 memos=3 unpointed=3 \
                oversized=0 unresolved:s2-l.u1 bodied:s2-l.b1,s2-l.b2 unpointed:s2-l.m1,s2-l.m2,s2-l.m3";
    assert_eq!(line(&place), want, "3 つの欠陥の件数と母集団と id");
    assert_eq!(calls(&place), ["--readonly list --all --limit 0 --json"], "台帳は readonly で 1 回だけ読む");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 欠陥 0 の周も 3 つの数が 0 で母集団が出て、行は消えない（id の列は無い）。
#[test]
fn ledger_lint_zero_defects_keep_the_line_with_its_population() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let beads = [
        Bead::contract("s2-z.c1", RESOLVED),
        Bead::memo("s2-z.m1", "## memo\ndesign = docs/design/toy.md#a"),
        Bead::memo("s2-z.m2", "## memo\nresearch = docs/research/x.md"),
    ];
    serve(&place, &beads);
    assert_eq!(line(&place), "ledger: open=3 contracts=1 unresolved=0 bodied=0 memos=2 unpointed=0 oversized=0", "0 と母集団");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) client が起動できない・rc ≠ 0・出力が壊れた周は件数 0 に倒れず測れていない形の行（`=0` の欄を 1 つも持たない）。
#[test]
fn ledger_lint_unreadable_ledger_is_not_zero() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let empty = place.dir.join("empty-path");
    fs::create_dir_all(&empty).ok();
    let unlaunchable = line_with(&place, &empty.display().to_string());
    serve(&place, &[Bead::memo("s2-u.1", "## memo")]);
    assert!(line(&place).contains("memos=1"), "同じ置き場で読める周は数える（否定の枝の対照）");
    write_client(&place, "cat /dev/null\nexit 3");
    let refused = line(&place);
    write_client(&place, "printf '[{\"id\":'");
    let broken = line(&place);
    for (case, found) in [("起動できない", &unlaunchable), ("rc ≠ 0", &refused), ("壊れた出力", &broken)] {
        assert_eq!(found, "ledger: unreadable reason=ledger-unreadable", "{case}");
        assert!(!found.contains("=0"), "{case}: 件数 0 に倒さない: {found}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) pointer の**解ける**契約と `## memo` の見出しを持たない memo は数に入らない（偽陽性の pin）。対照に、doc の
/// 無い pointer と区間に無い id の契約は数に入る。
#[test]
fn ledger_lint_resolved_contracts_and_headless_memos_are_not_counted() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let beads = [
        Bead::contract("s2-d.ok", RESOLVED),
        Bead::memo("s2-d.m1", "見出しの無い memo の本文"),
        Bead::memo("s2-d.m2", "### memo\n## memo の字面は行の途中"),
    ];
    serve(&place, &beads);
    assert_eq!(line(&place), "ledger: open=3 contracts=1 unresolved=0 bodied=0 memos=2 unpointed=0 oversized=0", "偽陽性 0");
    let beads = [
        Bead::contract("s2-d.ok", RESOLVED),
        Bead::contract("s2-d.gone", "design = docs/design/gone.md#a"),
        Bead::contract("s2-d.row", "design = docs/design/toy.md#b"),
    ];
    serve(&place, &beads);
    assert_eq!(
        line(&place),
        "ledger: open=3 contracts=3 unresolved=2 bodied=0 memos=0 unpointed=0 oversized=0 unresolved:s2-d.gone,s2-d.row",
        "doc の無い pointer と行の無い pointer は解けない"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// 偽の client を先頭に積んだ PATH。
fn shimmed(place: &Place) -> String {
    format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default())
}

/// notes の上限（rules 行 `memo.notes_max_bytes`・既定 8192）を byte で越える notes。
fn notes_of(bytes: usize) -> String {
    "x".repeat(bytes)
}

/// (f) notes が 8193 byte の open の memo は名指され、8192 byte の memo は名指されない（境の対・門は止めず rc 0）。
#[test]
fn ledger_lint_oversized_names_the_memo_past_the_limit_only() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (over, edge) = (notes_of(8193), notes_of(8192));
    let beads = [
        Bead { notes: &over, ..Bead::memo("s2-o.over", "## memo\ndesign = docs/design/toy.md#a") },
        Bead { notes: &edge, ..Bead::memo("s2-o.edge", "## memo\ndesign = docs/design/toy.md#a") },
    ];
    serve(&place, &beads);
    assert_eq!(
        line(&place),
        "ledger: open=2 contracts=0 unresolved=0 bodied=0 memos=2 unpointed=0 oversized=1 oversized:s2-o.over",
        "8193 byte だけが数と id に出る"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) 上限の行の無い manifest は `oversized=no-rule`（0 に畳まない・id の列も無い）。同じ歯の行の在る manifest は数える。
#[test]
fn ledger_lint_oversized_is_no_rule_without_the_row_and_counts_with_it() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let over = notes_of(9000);
    serve(&place, &[Bead { notes: &over, ..Bead::memo("s2-g.over", "## memo\ndesign = docs/design/toy.md#a") }]);
    let head = "schema = 1\n\n[[rule]]\nid = \"seat.ledger_timeout_s\"\nkind = \"LedgerTimeoutS\"\nvalue = 60\nenabled = true\nruling = \"user 2026-09-12T02:01Z\"\nruled_at = \"2026-09-12\"\n";
    let row = "\n[[rule]]\nid = \"memo.notes_max_bytes\"\nkind = \"MemoNotesMaxBytes\"\nvalue = 8192\nenabled = true\nruling = \"user 2026-09-30T04:25Z 項 memo\"\nruled_at = \"2026-09-30\"\n";
    let (without, with) = (place.dir.join("without.toml"), place.dir.join("with.toml"));
    fs::write(&without, head).unwrap_or_else(|err| panic!("manifest を書ける: {err}"));
    fs::write(&with, format!("{head}{row}")).unwrap_or_else(|err| panic!("manifest を書ける: {err}"));
    let path = shimmed(&place);
    let unruled = line_args(&place, &["--rules", &without.display().to_string()], &path);
    assert_eq!(unruled, "ledger: open=1 contracts=0 unresolved=0 bodied=0 memos=1 unpointed=0 oversized=no-rule", "行の無い周は no-rule");
    let ruled = line_args(&place, &["--rules", &with.display().to_string()], &path);
    assert_eq!(ruled, "ledger: open=1 contracts=0 unresolved=0 bodied=0 memos=1 unpointed=0 oversized=1 oversized:s2-g.over", "行の在る周は数える");
    fs::remove_dir_all(&place.dir).ok();
}

/// (h) closed の memo は大きな notes でも数えない（対に、同じ notes の open の memo は数える）。
#[test]
fn ledger_lint_oversized_ignores_closed_memos() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let over = notes_of(9000);
    let closed = Bead { status: "closed", notes: &over, ..Bead::memo("s2-h.closed", "## memo\ndesign = docs/design/toy.md#a") };
    serve(&place, &[closed]);
    assert_eq!(line(&place), "ledger: open=0 contracts=0 unresolved=0 bodied=0 memos=0 unpointed=0 oversized=0", "closed は数えない");
    let open = Bead { notes: &over, ..Bead::memo("s2-h.open", "## memo\ndesign = docs/design/toy.md#a") };
    serve(&place, &[open]);
    assert!(line(&place).ends_with("oversized=1 oversized:s2-h.open"), "open は数える");
    fs::remove_dir_all(&place.dir).ok();
}

/// 局面の出力の歯の置き場（`pipe` の toy repo に台帳の files の形と origin/main の ref を足し、偽の台帳 client を PATH に積む）。
struct Site {
    repo: PathBuf,
    state: PathBuf,
    shim: PathBuf,
}

/// 発話の最初の ts（切り替えの線の後ろに置く・遠い未来で線の ts より後）。
const UTTERANCE: &str = "2999-01-01T00:00:01Z";

impl Site {
    /// 台帳は memo 3 本（期日が過ぎて引き金が満ちた memo・昇格の行の契約が取り下げで閉じた memo・待ちの memo）と取り下げで閉じた契約。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn new() -> Self {
        let (repo, state) = super::pipe::repo_with_state();
        super::pipe::write_contract(&repo, &[], &[]);
        let bin = state.join("bin");
        fs::create_dir_all(&bin).expect("bin を作れる");
        let shim = bin.join("bd");
        fs::write(&shim, format!("#!/bin/sh\ncat '{}'\n", state.join("ledger.json").display())).expect("偽の client を書ける");
        fs::set_permissions(&shim, std::os::unix::fs::PermissionsExt::from_mode(0o755)).expect("実行権を付ける");
        fs::create_dir_all(repo.join(".beads")).expect(".beads を作れる");
        fs::write(repo.join(".beads/config.yaml"), "issue-prefix: toy\n").expect("config を書ける");
        fs::write(repo.join(".beads/issues.jsonl"), "[]\n").expect("台帳の file を書ける");
        let exclude = repo.join(".git/info/exclude");
        let body = fs::read_to_string(&exclude).unwrap_or_default();
        fs::write(&exclude, format!("{body}.beads/\n")).expect("exclude を書ける");
        let main = super::pipe::git(&repo, &["rev-parse", "refs/heads/main"]);
        super::pipe::git(&repo, &["update-ref", "refs/remotes/origin/main", &main]);
        let site = Self { repo, state, shim };
        let head = "### 出所\n### 観測\n### 候補\n### 昇格条件\n";
        let memo = |id: &str, trigger: &str, notes: &str| {
            let description = format!("{head}- 引き金: {trigger}\n");
            format!("{{\"id\":\"{id}\",\"status\":\"open\",\"priority\":2,\"issue_type\":\"task\",\"labels\":[\"intake:memo\"],\"description\":{},\"notes\":{},\"dependencies\":[]}}", quoted(&description), quoted(notes))
        };
        let withdrawn = format!(
            "{{\"id\":\"toy-wc\",\"status\":\"closed\",\"priority\":2,\"issue_type\":\"task\",\"labels\":[],\"acceptance_criteria\":\"design = {}\",\"close_reason\":\"取り下げ 不要\",\"closed_at\":\"2026-09-28T00:00:00Z\",\"dependencies\":[{{\"depends_on_id\":\"toy-mb\",\"type\":\"discovered-from\"}}]}}",
            super::pipe::design_pointer()
        );
        let beads = [memo("toy-ma", "期日 2026-09-25T00:00Z", ""), memo("toy-mb", "再発 5", "昇格: 全部 toy-wc"), memo("toy-mc", "再発 5", ""), withdrawn];
        fs::write(site.state.join("ledger.json"), format!("[{}]\n", beads.join(","))).expect("台帳の fixture を書ける");
        site
    }

    fn path(&self) -> String {
        format!("{}:{}", self.shim.parent().unwrap_or(&self.state).display(), std::env::var("PATH").unwrap_or_default())
    }

    fn json_path(&self) -> PathBuf {
        self.state.join("fleet").join("lifecycle.json")
    }

    /// `fleet lifecycle write`（rc 0 を確かめる）。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn write(&self) {
        let out = Command::new(env!("CARGO_BIN_EXE_scribe2"))
            .args(["fleet", "lifecycle", "write", "--state-dir", &self.state.display().to_string(), "--repo", &self.repo.display().to_string(), "--bd", &self.shim.display().to_string()])
            .output()
            .expect("binary を起動できる");
        assert_eq!(out.status.code(), Some(0), "write は rc 0: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// event log に行を足す。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn append(&self, lines: &[String]) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new().append(true).open(self.state.join("fleet").join("events.jsonl")).expect("event log を開ける");
        for line in lines {
            writeln!(file, "{line}").expect("行を足せる");
        }
    }

    /// 偽の台帳と置き場から出力を作る: 切り替えの線を引く 1 周の後に出力を消し、線の後ろの発話（未仕分け 1・会話だけ 1・memo 2 本へ
    /// 要望の 1 発話）を足して書き直す（出力の入力の印が今の置き場と同じになる）。出力の `generated_at` は固定の時刻へ置き換える。
    #[expect(
        clippy::expect_used,
        reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
    )]
    fn built(&self) {
        self.write();
        fs::remove_file(self.json_path()).expect("1 周目の出力を消せる");
        let received = |ts: &str| format!("{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"UtteranceReceived\",\"channel\":\"gui\",\"host\":\"h\",\"actor\":\"human\",\"detail\":\"x\"}}");
        let sorted = |ts: &str, utterance: &str, tail: &str| {
            format!("{{\"schema\":1,\"ts\":\"{ts}\",\"kind\":\"UtteranceSorted\",\"utterance\":\"{utterance}\",\"sorting\":\"{tail}\",\"host\":\"h\",\"actor\":\"machine\"}}")
        };
        let request = |ts: &str, utterance: &str, memo: &str| sorted(ts, utterance, "request").replace("\"host\"", &format!("\"bead\":\"{memo}\",\"host\""));
        self.append(&[
            received(UTTERANCE),
            received("2999-01-01T00:00:02Z"),
            sorted("2999-01-01T00:00:03Z", "2999-01-01T00:00:02Z", "chat"),
            received("2999-01-01T00:00:04Z"),
            request("2999-01-01T00:00:05Z", "2999-01-01T00:00:04Z", "toy-ma"),
            request("2999-01-01T00:00:06Z", "2999-01-01T00:00:04Z", "toy-mc"),
        ]);
        self.write();
        let text = fs::read_to_string(self.json_path()).expect("出力を読める");
        let tree = vessel::fleet::json_tree::parse(&text).expect("出力は JSON");
        let generated = tree.get("generated_at").and_then(|found| found.as_str()).expect("generated_at が在る");
        // 線の ts と同じ秒に当たりうるので、`generated_at` の欄だけを置き換える。
        let field = |value: &str| format!("\"generated_at\": \"{value}\"");
        assert!(text.contains(&field(generated)), "generated_at の欄の字");
        fs::write(self.json_path(), text.replace(&field(generated), &field("2026-10-01T00:00:00Z"))).expect("出力を書き戻せる");
    }

    /// `doctor --state-dir S --repo R` の `lifecycle-` の行（頭ごとに 1 本ずつ・頭の順）。
    fn lines(&self) -> Vec<String> {
        let (state, repo) = (self.state.display().to_string(), self.repo.display().to_string());
        let stdout = doctor_run(&["--state-dir", &state, "--repo", &repo], &self.path());
        stdout.lines().filter(|line| line.starts_with("lifecycle-")).map(str::to_owned).collect()
    }
}

/// (a) 3 行の件数と最古: 未仕分け 1・request 1（2 つの memo へ行き先を持つ 1 発話を 1 と数える）・chat 1、open な memo 3 本のうち、引き金が満ちて
/// keep の無い memo と、昇格の行の契約が全部取り下げで閉じた memo の 2 本が actionable（最古は期日の 6 日前＝144 時間）、席の手番の閾値越えは 1 本。
#[test]
fn ledger_doctor_lifecycle_prints_the_three_lines_with_counts_and_oldest() {
    let site = Site::new();
    site.built();
    assert_eq!(
        site.lines(),
        [
            format!("lifecycle-utterance: unsorted=1 oldest={UTTERANCE} request=1 chat=1"),
            "lifecycle-memo: open=3 actionable=2 oldest=toy-ma:144h".to_owned(),
            "lifecycle-owned: count=1 oldest=memo:toy-ma:memo-actionable:144h".to_owned(),
        ],
        "3 行の件数と最古（古くない周は stale= を持たない）"
    );
    super::pipe::clean(&[&site.repo, &site.state]);
}

/// (b) 古さの印を置いた周は各行の末尾が `stale=ledger-gate`（同じ歯の印の無い周は `stale=` が無い）。
#[test]
fn ledger_doctor_lifecycle_marks_a_stale_mark_with_its_kind() {
    use vessel::fleet::lifecycle_mark::{add_mark, Added, Kind, Ledger, Mark, Value};
    use vessel::fleet::store::LockPolicy;
    let site = Site::new();
    site.built();
    assert!(site.lines().iter().all(|line| !line.contains("stale=")), "印の無い周は stale= が無い: {:?}", site.lines());
    let mark = Mark { kind: Kind::LedgerGate, at: "2026-10-01T00:00:00Z".to_owned(), value: Value::Ledger(Ledger::Files { len: 1, mtime_ns: 1 }) };
    assert_eq!(add_mark(&site.state, &mark, LockPolicy { retry_ms: 500, stale_ms: 600_000 }), Added::Added, "印を足せる");
    let lines = site.lines();
    assert_eq!(lines.len(), 3, "3 行: {lines:?}");
    assert!(lines.iter().all(|line| line.ends_with(" stale=ledger-gate")), "各行の末尾に印の種類: {lines:?}");
    super::pipe::clean(&[&site.repo, &site.state]);
}

/// (c) 印の無い台帳の変化（台帳の印が出力の入力の印と違う）は各行の末尾が `stale=ledger`。
#[test]
fn ledger_doctor_lifecycle_marks_a_ledger_change_without_a_mark() {
    let site = Site::new();
    site.built();
    assert!(site.lines().iter().all(|line| !line.contains("stale=")), "変化の前は stale= が無い: {:?}", site.lines());
    fs::write(site.repo.join(".beads/issues.jsonl"), "[]\n\n").unwrap_or_else(|err| panic!("台帳の file を進められる: {err}"));
    let lines = site.lines();
    assert_eq!(lines.len(), 3, "3 行: {lines:?}");
    assert!(lines.iter().all(|line| line.ends_with(" stale=ledger")), "各行の末尾に ledger: {lines:?}");
    super::pipe::clean(&[&site.repo, &site.state]);
}

/// (d) 出力の無い置き場は 3 行とも `unreadable reason=absent`（件数を出さない）。json が壊れた置き場は `reason=unparsed`。
#[test]
fn ledger_doctor_lifecycle_names_an_absent_or_unparsed_output_without_counts() {
    let site = Site::new();
    let want = |reason: &str| ["lifecycle-utterance:", "lifecycle-memo:", "lifecycle-owned:"].map(|head| format!("{head} unreadable reason={reason}")).to_vec();
    assert_eq!(site.lines(), want("absent"), "出力の無い置き場");
    fs::create_dir_all(site.state.join("fleet")).unwrap_or_else(|err| panic!("fleet を作れる: {err}"));
    fs::write(site.json_path(), "{\"version\":9}\n").unwrap_or_else(|err| panic!("壊れた出力を書ける: {err}"));
    assert_eq!(site.lines(), want("unparsed"), "読めない出力");
    super::pipe::clean(&[&site.repo, &site.state]);
}

/// (e) 3 行は `--state-dir` と `--repo` の両方を渡した周だけ在る（片方だけの周は無い）。
#[test]
fn ledger_doctor_lifecycle_needs_both_the_state_dir_and_the_repo() {
    let site = Site::new();
    let (state, repo) = (site.state.display().to_string(), site.repo.display().to_string());
    let count = |args: &[&str]| doctor_run(args, &site.path()).lines().filter(|line| line.starts_with("lifecycle-")).count();
    assert_eq!(count(&["--repo", &repo]), 0, "--state-dir の無い周は無い");
    assert_eq!(count(&["--state-dir", &state]), 0, "--repo の無い周は無い");
    assert_eq!(count(&["--state-dir", &state, "--repo", &repo]), 3, "両方を渡す周は在る");
    super::pipe::clean(&[&site.repo, &site.state]);
}
