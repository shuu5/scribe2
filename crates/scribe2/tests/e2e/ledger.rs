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
}

impl<'a> Bead<'a> {
    /// acceptance を持つ open の契約。
    fn contract(id: &'a str, acceptance: &'a str) -> Self {
        Self { id, status: "open", memo: false, acceptance, description: "" }
    }

    /// 本文を持つ open の memo。
    fn memo(id: &'a str, description: &'a str) -> Self {
        Self { id, status: "open", memo: true, acceptance: "", description }
    }

    /// JSON の 1 要素。
    fn json(&self) -> String {
        let labels = if self.memo { "[\"intake:memo\",\"doc:toy\"]" } else { "[\"doc:toy\"]" };
        format!(
            "{{\"id\":{},\"title\":\"t\",\"status\":{},\"issue_type\":\"task\",\"labels\":{labels},\"acceptance_criteria\":{},\"description\":{},\"notes\":\"\",\"dependencies\":[]}}",
            quoted(self.id),
            quoted(self.status),
            quoted(self.acceptance),
            quoted(self.description),
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

/// `doctor --repo` を `path` の PATH で撃ち、rc 0 を確かめて `ledger:` の行（ちょうど 1 本）を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn line_with(place: &Place, path: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_scribe2"))
        .args(["doctor", "--repo", &place.repo.display().to_string()])
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "doctor は判定しない（rc 0）: {stdout}");
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
                unresolved:s2-l.u1 bodied:s2-l.b1,s2-l.b2 unpointed:s2-l.m1,s2-l.m2,s2-l.m3";
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
    assert_eq!(line(&place), "ledger: open=3 contracts=1 unresolved=0 bodied=0 memos=2 unpointed=0", "0 と母集団");
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
    assert_eq!(line(&place), "ledger: open=3 contracts=1 unresolved=0 bodied=0 memos=2 unpointed=0", "偽陽性 0");
    let beads = [
        Bead::contract("s2-d.ok", RESOLVED),
        Bead::contract("s2-d.gone", "design = docs/design/gone.md#a"),
        Bead::contract("s2-d.row", "design = docs/design/toy.md#b"),
    ];
    serve(&place, &beads);
    assert_eq!(
        line(&place),
        "ledger: open=3 contracts=3 unresolved=2 bodied=0 memos=0 unpointed=0 unresolved:s2-d.gone,s2-d.row",
        "doc の無い pointer と行の無い pointer は解けない"
    );
    fs::remove_dir_all(&place.dir).ok();
}
