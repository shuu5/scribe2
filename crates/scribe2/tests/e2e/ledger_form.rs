//! 台帳の形の lint（設計 docs/design/ledger-form.md §3 の 4・§6 行 a・接頭辞 `ledger_form_`）。
//!
//! toy repo（契約表を持つ設計 doc 1 本・tracked の集合は index）に対して実 binary の `doctor --repo` を撃ち、
//! 台帳は PATH の先頭に置いた偽の client（引数を記録して fixture の JSON を返す shim）が答える。

use super::{make_tmp_dir, TmpDir};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 設計 doc（§ 1 は memo `s2-f.7` を名指し、§ 2 は名指さない）と契約表 4 行: `a`（§1・未着地）・`b`（§2・未着地）・
/// `c`（§2・`+seed` が tracked＝着地済み）・`d`（`+` の無い行＝未着地に数えない）。
const DOC: &str = "# toy\n\n## 1. one\n\ns2-f.7 の観測から起こした。\n\n## 2. two\n\n名指しなし。\n\n\
<!-- contracts:begin -->\nschema = 1\n\n\
[[contract]]\nid = \"a\"\ntitle = \"a\"\nreq = [\"FR1\"]\nsection = \"1\"\nwrite-set = [\"+src/new_a.rs\"]\nverify = [\"cargo nextest run -p x --no-tests=fail a_\"]\nsize = \"S\"\ndone = \"a\"\n\n\
[[contract]]\nid = \"b\"\ntitle = \"b\"\nreq = [\"FR1\"]\nsection = \"2\"\nwrite-set = [\"+src/new_b.rs\"]\nverify = [\"cargo nextest run -p x --no-tests=fail b_\"]\nsize = \"S\"\ndone = \"b\"\n\n\
[[contract]]\nid = \"c\"\ntitle = \"c\"\nreq = [\"FR1\"]\nsection = \"2\"\nwrite-set = [\"+seed\"]\nverify = [\"cargo nextest run -p x --no-tests=fail c_\"]\nsize = \"S\"\ndone = \"c\"\n\n\
[[contract]]\nid = \"d\"\ntitle = \"d\"\nreq = [\"FR1\"]\nsection = \"2\"\nwrite-set = [\"seed\"]\nverify = [\"cargo nextest run -p x --no-tests=fail d_\"]\nsize = \"S\"\ndone = \"d\"\n\
<!-- contracts:end -->\n";

/// memo の 4 節が揃った本文。
const FULL: &str = "## memo\n### 出所\nrun\n### 観測\n1/2\n### 候補\nなし\n### 昇格条件\n要 ADR\n";

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

/// toy repo を作る（設計 doc と `seed` を index に載せる・`src/new_*.rs` は無い）。
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
    kind: &'a str,
    memo: bool,
    pointer: Option<&'a str>,
    description: &'a str,
    notes: &'a str,
    from: Option<&'a str>,
}

impl<'a> Bead<'a> {
    /// open の task（label・pointer・本文・edge 無し）。
    fn task(id: &'a str) -> Self {
        Self { id, status: "open", kind: "task", memo: false, pointer: None, description: "", notes: "", from: None }
    }

    /// 4 節を `description` で持つ open の memo。
    fn memo(id: &'a str, description: &'a str) -> Self {
        Self { memo: true, description, ..Self::task(id) }
    }

    /// 設計 pointer を持つ open の契約。
    fn contract(id: &'a str, pointer: &'a str) -> Self {
        Self { pointer: Some(pointer), ..Self::task(id) }
    }

    /// JSON の 1 要素。
    fn json(&self) -> String {
        let labels = if self.memo { "[\"intake:memo\",\"doc:toy\"]" } else { "[\"doc:toy\"]" };
        let acceptance = self.pointer.map_or_else(String::new, |found| format!("design = docs/design/toy.md#{found}"));
        let deps = self.from.map_or_else(String::new, |memo| {
            format!("{{\"issue_id\":{},\"depends_on_id\":{},\"type\":\"discovered-from\"}}", quoted(self.id), quoted(memo))
        });
        format!(
            "{{\"id\":{},\"title\":\"t\",\"status\":{},\"issue_type\":{},\"labels\":{labels},\"acceptance_criteria\":{},\"description\":{},\"notes\":{},\"dependencies\":[{deps}]}}",
            quoted(self.id),
            quoted(self.status),
            quoted(self.kind),
            quoted(&acceptance),
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

/// `doctor --repo` を `path` の PATH で撃ち、rc 0 を確かめて `ledger-form:` の行（ちょうど 1 本）を返す。
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
    let lines: Vec<&str> = stdout.lines().filter(|line| line.starts_with("ledger-form:")).collect();
    assert_eq!(lines.len(), 1, "台帳の形の行はちょうど 1 本: {stdout}");
    assert_eq!(stdout.lines().last(), lines.first().copied(), "台帳の形の行は doctor の末尾: {stdout}");
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

/// (a)(b)(c)(f)(viii) 5 つの欠陥を**件数を違えて**持つ fixture: 4 節の欠けは 出所 1・観測 2・候補 0・昇格条件 3、
/// 4 象限は両方 1・どちらも無し 2（epic と裁定は母集団の外）、名指しの edge 無しは § の本文から 1・本文から 1
/// （edge の在る名指し・`s2-f.70` の字面・名指しの無い契約は数えない）、drift は未着地 2 行のうち 1、辿れる契約が
/// 全部 closed の memo は 1（open の契約が残る memo・契約の無い memo は数えない）。台帳は `--readonly` で 1 回だけ読む。
#[test]
fn ledger_form_names_each_defect_with_its_count_and_population() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let no_source = FULL.replace("### 出所\n", "");
    let no_observation = FULL.replace("### 観測\n", "");
    let no_promotion = FULL.replace("### 昇格条件\n", "");
    let (head, tail) = FULL.split_at(FULL.find("### 候補").unwrap_or_default());
    let beads = [
        Bead::memo("s2-f.1", &no_source),
        Bead::memo("s2-f.2", &no_observation),
        Bead::memo("s2-f.3", &no_observation),
        Bead::memo("s2-f.4", &no_promotion),
        Bead::memo("s2-f.5", &no_promotion),
        Bead::memo("s2-f.6", &no_promotion),
        Bead { notes: tail, ..Bead::memo("s2-f.7", head) },
        Bead::memo("s2-f.8", FULL),
        Bead::memo("s2-f.9", FULL),
        Bead { pointer: Some("c"), ..Bead::memo("s2-f.b1", FULL) },
        Bead::task("s2-f.n1"),
        Bead::task("s2-f.n2"),
        Bead { kind: "epic", ..Bead::task("s2-f.e1") },
        Bead { kind: "decision", ..Bead::task("s2-f.d1") },
        Bead { description: "s2-f.7 の観測から", ..Bead::contract("s2-f.c1", "c") },
        Bead::contract("s2-f.c2", "a"),
        Bead { notes: "s2-f.7 から", from: Some("s2-f.7"), ..Bead::contract("s2-f.c3", "c") },
        Bead { description: "s2-f.70 と s2-f.n1 を見る", ..Bead::contract("s2-f.c4", "c") },
        Bead { status: "closed", from: Some("s2-f.8"), ..Bead::contract("s2-f.k1", "b") },
    ];
    serve(&place, &beads);
    let want = "ledger-form: open=18 memos=10 no-source=1:s2-f.1 no-observation=2:s2-f.2,s2-f.3 no-candidate=0 \
                no-promotion=3:s2-f.4,s2-f.5,s2-f.6 shaped=16 both=1:s2-f.b1 neither=2:s2-f.n1,s2-f.n2 contracts=4 \
                undiscovered=2:s2-f.c1,s2-f.c2 unlanded=2 drift=1:toy#b settled=1:s2-f.8";
    assert_eq!(line(&place), want, "5 つの欠陥の件数と母集団と id");
    assert_eq!(calls(&place), ["--readonly list --all --limit 0 --json"], "台帳は readonly で 1 回だけ読む");
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) 欠陥 0 の周も 0 と母集団が出て行が消えない（揃った memo・edge を張った契約・着地済みの行だけ）。
#[test]
fn ledger_form_zero_defects_keep_the_line_with_its_population() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    fs::write(place.repo.join("docs/design/toy.md"), DOC.replace("+src/new_a.rs", "+seed").replace("+src/new_b.rs", "+seed"))
        .ok();
    let beads = [
        Bead::memo("s2-z.1", FULL),
        Bead { description: "s2-z.1 から", from: Some("s2-z.1"), ..Bead::contract("s2-z.c1", "a") },
        Bead { kind: "epic", ..Bead::task("s2-z.e1") },
    ];
    serve(&place, &beads);
    let want = "ledger-form: open=3 memos=1 no-source=0 no-observation=0 no-candidate=0 no-promotion=0 shaped=2 \
                both=0 neither=0 contracts=1 undiscovered=0 unlanded=0 drift=0 settled=0";
    assert_eq!(line(&place), want, "0 と母集団");
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) client が起動できない・rc ≠ 0・出力が壊れた周は件数 0 に倒れず測れていない形の行（`=0` の欄を 1 つも持たない）。
#[test]
fn ledger_form_unreadable_ledger_is_not_zero() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let empty = place.dir.join("empty-path");
    fs::create_dir_all(&empty).ok();
    let unlaunchable = line_with(&place, &empty.display().to_string());
    serve(&place, &[Bead::memo("s2-u.1", FULL)]);
    assert!(line(&place).contains("memos=1"), "同じ置き場で読める周は数える（否定の枝の対照）");
    write_client(&place, "cat /dev/null\nexit 3");
    let refused = line(&place);
    write_client(&place, "printf '[{\"id\":'");
    let broken = line(&place);
    for (case, found) in [("起動できない", &unlaunchable), ("rc ≠ 0", &refused), ("壊れた出力", &broken)] {
        assert_eq!(found, "ledger-form: unreadable reason=ledger-unreadable", "{case}");
        assert!(!found.contains("=0"), "{case}: 件数 0 に倒さない: {found}");
    }
    fs::remove_dir_all(&place.dir).ok();
}
