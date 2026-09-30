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
    /// 台帳の問い（label `intake:question`）。
    question: bool,
    pointer: Option<&'a str>,
    description: &'a str,
    notes: &'a str,
    from: Option<&'a str>,
    /// parent-child の辺の先（順のまま・最初の 1 本が親）。
    parents: &'a [&'a str],
}

impl<'a> Bead<'a> {
    /// open の task（label・pointer・本文・edge 無し）。
    fn task(id: &'a str) -> Self {
        let (description, notes) = ("", "");
        Self { id, status: "open", kind: "task", memo: false, question: false, pointer: None, description, notes, from: None, parents: &[] }
    }

    /// `status` と `kind` を持ち `parents` の下に付く bead（台帳のグラフの fixture）。
    fn node(id: &'a str, status: &'a str, kind: &'a str, parents: &'a [&'a str]) -> Self {
        Self { status, kind, parents, ..Self::task(id) }
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
        let labels = match (self.memo, self.question) {
            (true, _) => "[\"intake:memo\",\"doc:toy\"]",
            (false, true) => "[\"intake:question\",\"doc:toy\"]",
            (false, false) => "[\"doc:toy\"]",
        };
        let acceptance = self.pointer.map_or_else(String::new, |found| format!("design = docs/design/toy.md#{found}"));
        let edge = |on: &str, kind: &str| {
            format!("{{\"issue_id\":{},\"depends_on_id\":{},\"type\":{}}}", quoted(self.id), quoted(on), quoted(kind))
        };
        let mut deps: Vec<String> = self.from.iter().map(|memo| edge(memo, "discovered-from")).collect();
        deps.extend(self.parents.iter().map(|parent| edge(parent, "parent-child")));
        let deps = deps.join(",");
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

/// `doctor --repo`（`rules` が在れば `--rules` も）を `path` の PATH で撃ち、rc 0 を確かめて stdout を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn doctor(place: &Place, path: &str, rules: Option<&Path>) -> String {
    let mut args = vec!["doctor".to_owned(), "--repo".to_owned(), place.repo.display().to_string()];
    args.extend(rules.map(|found| ["--rules".to_owned(), found.display().to_string()]).into_iter().flatten());
    let out = Command::new(env!("CARGO_BIN_EXE_scribe2")).args(&args).env("PATH", path).output().expect("binary を起動できる");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "doctor は判定しない（rc 0）: {stdout}");
    stdout
}

/// `doctor --repo` を `path` の PATH で撃ち、rc 0 を確かめて `ledger-form:` の行（ちょうど 1 本）を返す。行は引用の行（`ruling-cite:`・
/// key の無い toy repo は `check=off`）の直前で、引用の行が doctor の末尾（設計 dispatcher.md §36 約束 7）。
// flip-check: retroactive s2-07l.738.37.3
fn line_with(place: &Place, path: &str) -> String {
    let stdout = doctor(place, path, None);
    let lines: Vec<&str> = stdout.lines().filter(|line| line.starts_with("ledger-form:")).collect();
    assert_eq!(lines.len(), 1, "台帳の形の行はちょうど 1 本: {stdout}");
    let all: Vec<&str> = stdout.lines().collect();
    assert_eq!(all.last().copied(), Some("ruling-cite: check=off"), "引用の行が doctor の末尾: {stdout}");
    let before_last = all.len().checked_sub(2).and_then(|at| all.get(at)).copied();
    assert_eq!(before_last, lines.first().copied(), "台帳の形の行は引用の行の直前: {stdout}");
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

// ─── 台帳のグラフの形（設計 ledger-form.md §10・契約表の行 f・接頭辞 `ledger_graph_`） ───

/// 埋め込みの manifest の字面（`--rules` に渡す写しの元）。
const EMBEDDED: &str = include_str!("../../../../rules/manifest.toml");

/// 埋め込みの manifest の上限の行（写しで値を替える・行ごと除く）。
const MAX_ROW: &str = "[[rule]]\nid = \"ledger.open_children_max\"\nkind = \"LedgerOpenChildrenMax\"\nvalue = 15\nenabled = true\n\
ruling = \"user 2026-09-27T17:33Z 項 2-3\"\nruled_at = \"2026-09-27\"\n";

/// 違反が 1 つ以上の周に行の末尾へ 1 回付く直す形。
const FIX: &str = " — top は bdw update <top> --parent <epic> か --type epic・親 2 つと親の輪は bdw update <子> --parent <epic> \
で 1 本に置き換える・溢れは bdw create <題> --type epic --parent <親> の子 epic へ付け替える・close-eligible は bdw close <epic> --reason 完了";

/// 埋め込みの manifest の写しの上限の行を `row` に替えて置き場に書き、その path を返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn rules_with(place: &Place, row: &str) -> PathBuf {
    let text = EMBEDDED.replace(MAX_ROW, row);
    assert_ne!(text, EMBEDDED, "写しは上限の行を替えた（字面が manifest と揃っている）");
    let path = place.dir.join("rules.toml");
    fs::write(&path, text).expect("rules の写しを書ける");
    path
}

/// 偽の client を先頭に積んだ PATH で `doctor --repo`（`rules` が在れば `--rules` も）を撃ち、台帳のグラフの行（ちょうど
/// 1 本）を返す。行は台帳 lint の行の直後・台帳の形の行の直前に在り、台帳の形の行の後ろに引用の行が 1 本だけ続いて doctor の末尾になる。
fn graph_line(place: &Place, rules: Option<&Path>) -> String {
    let stdout = doctor(place, &format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default()), rules);
    let lines: Vec<&str> = stdout.lines().collect();
    let found: Vec<usize> = (0..lines.len()).filter(|at| lines.get(*at).is_some_and(|l| l.starts_with("ledger-graph:"))).collect();
    assert_eq!(found.len(), 1, "台帳のグラフの行はちょうど 1 本: {stdout}");
    let at = found.first().copied().unwrap_or_default();
    assert!(lines.get(at.wrapping_sub(1)).is_some_and(|l| l.starts_with("ledger:")), "台帳 lint の行の直後: {stdout}");
    assert!(lines.get(at + 1).is_some_and(|l| l.starts_with("ledger-form:")), "台帳の形の行の直前: {stdout}");
    assert!(lines.get(at + 2).is_some_and(|l| l.starts_with("ruling-cite:")), "引用の行が台帳の形の行の直後: {stdout}");
    assert_eq!(at + 3, lines.len(), "引用の行が doctor の末尾: {stdout}");
    lines.get(at).map(|line| (*line).to_owned()).unwrap_or_default()
}

/// (a)(e) 根の epic 1・その子 3（open 2・closed 1）・親を持たない open の task 1 と closed の task 1・feature の top の下の
/// open 2・親 2 つの task 1・親の輪 2 本・子が全部 closed の open な epic 1 で、各欄の件数と id と末尾の直す形が出て、
/// 台帳は `--readonly` で 1 回だけ読まれる（台帳の形の行は doctor の末尾のまま）。
#[test]
fn ledger_graph_counts_each_break_with_ids_and_the_fix() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let beads = [
        Bead::node("s2-g.r", "open", "epic", &[]),
        Bead::node("s2-g.r1", "open", "task", &["s2-g.r"]),
        Bead::node("s2-g.r2", "open", "task", &["s2-g.r"]),
        Bead::node("s2-g.r3", "closed", "task", &["s2-g.r"]),
        Bead::node("s2-g.t1", "open", "task", &[]),
        Bead::node("s2-g.t2", "closed", "task", &[]),
        Bead::node("s2-g.f", "open", "feature", &[]),
        Bead::node("s2-g.f1", "open", "task", &["s2-g.f"]),
        Bead::node("s2-g.f2", "open", "task", &["s2-g.f"]),
        Bead::node("s2-g.d", "open", "task", &["s2-g.r", "s2-g.f"]),
        Bead::node("s2-g.l1", "open", "task", &["s2-g.l2"]),
        Bead::node("s2-g.l2", "open", "task", &["s2-g.l1"]),
        Bead::node("s2-g.e", "open", "epic", &["s2-g.r"]),
        Bead::node("s2-g.e1", "closed", "task", &["s2-g.e"]),
    ];
    serve(&place, &beads);
    let want = format!(
        "ledger-graph: beads=14 open=11 max=15 unrooted=6 unrooted-closed=1 tops=2:s2-g.f,s2-g.t1 tops-closed=1 \
         two-parents=1:s2-g.d parent-loops=2:s2-g.l1,s2-g.l2 over=0 close-eligible=1:s2-g.e{FIX}"
    );
    assert_eq!(graph_line(&place, None), want, "各欄の件数と id と直す形");
    assert_eq!(calls(&place), ["--readonly list --all --limit 0 --json"], "台帳は readonly で 1 回だけ読む");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 上限 2 の写しで、直下の open の子 3 本の親だけが over に `<id>/3` で出て、2 本の親（closed と pinned の子は数えない）は
/// 出ない。
#[test]
fn ledger_graph_over_names_only_the_parent_past_the_max() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let rules = rules_with(&place, &MAX_ROW.replace("value = 15", "value = 2"));
    serve(&place, &over_beads());
    let line = graph_line(&place, Some(&rules));
    assert!(line.contains(" max=2 "), "上限は写しの値: {line}");
    assert!(line.contains(" over=1:s2-o.p/3 "), "N+1 本の親だけ: {line}");
    assert!(line.ends_with(FIX), "違反が在る周は直す形: {line}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (b)(f) の fixture: 根の epic `s2-o.r` の下に open の子 3 本の親 `s2-o.p` と、open の子 2 本（ほかに closed と pinned が
/// 1 本ずつ）の親 `s2-o.q`。
fn over_beads() -> [Bead<'static>; 10] {
    [
        Bead::node("s2-o.r", "open", "epic", &[]),
        Bead::node("s2-o.p", "open", "epic", &["s2-o.r"]),
        Bead::node("s2-o.p1", "open", "task", &["s2-o.p"]),
        Bead::node("s2-o.p2", "open", "task", &["s2-o.p"]),
        Bead::node("s2-o.p3", "in_progress", "task", &["s2-o.p"]),
        Bead::node("s2-o.q", "open", "epic", &["s2-o.r"]),
        Bead::node("s2-o.q1", "open", "task", &["s2-o.q"]),
        Bead::node("s2-o.q2", "open", "task", &["s2-o.q"]),
        Bead::node("s2-o.q3", "closed", "task", &["s2-o.q"]),
        Bead::node("s2-o.q4", "pinned", "task", &["s2-o.q"]),
    ]
}

/// (f) 上限の値だけを 0 にした写しでは over を数えず `-` を出し、N+1 本の子を持つ親も over に載らない。
#[test]
fn ledger_graph_zero_max_prints_a_dash_for_over() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let rules = rules_with(&place, &MAX_ROW.replace("value = 15", "value = 0"));
    serve(&place, &over_beads());
    let line = graph_line(&place, Some(&rules));
    assert!(line.contains(" max=0 ") && line.contains(" over=- "), "over は数えない: {line}");
    assert!(!line.contains("s2-o.p/"), "溢れた親も載らない: {line}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) 違反 0 の周も行が出て、末尾の直す形が無い。
#[test]
fn ledger_graph_clean_ledger_keeps_the_line_without_the_fix() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    serve(&place, &[Bead::node("s2-c.r", "open", "epic", &[]), Bead::node("s2-c.r1", "open", "task", &["s2-c.r"])]);
    let want = "ledger-graph: beads=2 open=2 max=15 unrooted=0 unrooted-closed=0 tops=0 tops-closed=0 two-parents=0 \
                parent-loops=0 over=0 close-eligible=0";
    assert_eq!(graph_line(&place, None), want, "0 の欄だけで直す形が無い");
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) 台帳を読めない周（rc ≠ 0・壊れた出力）は unreadable reason=ledger-unreadable で件数を 1 つも出さない。
#[test]
fn ledger_graph_unreadable_ledger_is_not_zero() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    serve(&place, &[Bead::node("s2-u.r", "open", "epic", &[])]);
    assert!(graph_line(&place, None).contains(" beads=1 "), "同じ置き場で読める周は数える（否定の枝の対照）");
    write_client(&place, "cat /dev/null\nexit 3");
    let refused = graph_line(&place, None);
    write_client(&place, "printf '[{\"id\":'");
    let broken = graph_line(&place, None);
    for (case, found) in [("rc ≠ 0", &refused), ("壊れた出力", &broken)] {
        assert_eq!(found, "ledger-graph: unreadable reason=ledger-unreadable", "{case}");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) 上限の行を除いた写しでは台帳のグラフの行が unreadable reason=no-rule で件数を 1 つも出さない（台帳は読める）。
#[test]
fn ledger_graph_missing_rule_is_unreadable_no_rule() {
    let place = place().unwrap_or_else(|| panic!("置き場を作れる"));
    let rules = rules_with(&place, "");
    serve(&place, &over_beads());
    assert!(graph_line(&place, None).contains(" beads=10 "), "埋め込みの rules では数える（否定の枝の対照）");
    assert_eq!(graph_line(&place, Some(&rules)), "ledger-graph: unreadable reason=no-rule", "行の無い rules");
    fs::remove_dir_all(&place.dir).ok();
}

// ─── 裁定 id の引用の行（設計 dispatcher.md §36 約束 7・契約表の行 ak・接頭辞 `ledger_ruling_doctor_`） ───

/// 宣言の本文（必須 key の 3 行の後ろに `extra` を足す）。歯の中の `(path, 字)` の組に `&str` のまま置けるよう、字は process の終わりまで残す。
fn declaration(extra: &str) -> &'static str {
    Box::leak(format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n{extra}").into_boxed_str())
}

/// 引用の行の歯の toy repo（`branch` の名で git init・台帳の接頭辞 `s2` は追跡しない `.beads/config.yaml` が持つ）。
fn cite_place(branch: &str) -> Option<Place> {
    let dir = make_tmp_dir()?.canonical()?;
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join(".beads")).ok()?;
    fs::write(repo.join(".beads/config.yaml"), "issue-prefix: s2\n").ok()?;
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).ok()?;
    let record = dir.join("bd-args");
    let setup: [&[&str]; 4] =
        [&["init", "-q", "-b", branch], &["config", "user.name", "t"], &["config", "user.email", "t@example.invalid"], &["config", "commit.gpgsign", "false"]];
    setup.iter().all(|args| git(&repo, args)).then_some(())?;
    Some(Place { dir, repo, bin, record })
}

/// `files`（path と字）を書いて 1 commit にする（`.beads` は追跡しない）。
fn commit_files(place: &Place, files: &[(&str, &str)]) -> Option<()> {
    for (name, text) in files {
        let path = place.repo.join(name);
        fs::create_dir_all(path.parent()?).ok()?;
        fs::write(&path, text).ok()?;
    }
    (git(&place.repo, &["add", "-A", "--", ".", ":!.beads"]) && git(&place.repo, &["commit", "-q", "-m", "c"])).then_some(())
}

/// 履歴を積む（1 commit ずつ `files` を書く）。
fn history(place: &Place, commits: &[&[(&str, &str)]]) {
    for files in commits {
        assert!(commit_files(place, files).is_some(), "commit を積める: {files:?}");
    }
}

/// 偽の client を先頭に積んだ PATH で `doctor --repo` を撃ち、引用の行（ちょうど 1 本・doctor の末尾・台帳の形の行の直後）を返す。
fn cite_line(place: &Place) -> String {
    let stdout = doctor(place, &format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default()), None);
    let lines: Vec<&str> = stdout.lines().collect();
    let found: Vec<&str> = lines.iter().copied().filter(|line| line.starts_with("ruling-cite:")).collect();
    assert_eq!(found.len(), 1, "引用の行はちょうど 1 本: {stdout}");
    assert_eq!(lines.last().copied(), found.first().copied(), "引用の行は doctor の末尾（4 行目）: {stdout}");
    let before = lines.len().checked_sub(2).and_then(|at| lines.get(at)).copied();
    assert!(before.is_some_and(|line| line.starts_with("ledger-form:")), "台帳の形の行の直後: {stdout}");
    found.first().map(|line| (*line).to_owned()).unwrap_or_default()
}

/// 閉じた問い `s2-q.1` の notes: 5 欄の行（逐語の欄にだけ別の問い id `s2-q.3…` を持つ）・束の欄に `batch:b7`・逐語に `batch:v9` を持つ
/// 4 欄の行・最後の欄に `batch:v8` を持つ 4 欄の行・裁定 id の欄が `policy:batch:x` の行・`policy:p1` の行。
const RULING_NOTES: &str = "s2-q.1:20260930T0000Z-1 | s2-q.1 | 2026-09-30T00:00Z | chat | 逐語に s2-q.3:20260930T0200Z-1 を引く\n\
batch:m1 | s2-q.1 | batch:b7 | 逐語 batch:v9\n\
batch:m2 | s2-q.1 | 2026-09-30T00:00Z | batch:v8\n\
policy:batch:x | s2-q.1 | 2026-09-30T00:00Z | chat | 逐語\n\
policy:p1 | s2-q.1 | 2026-09-30T00:00Z | chat | 逐語";

/// 開いた問い `s2-q.2` の notes（裁定の行の形だが問いが開いている）。
const OPEN_NOTES: &str = "s2-q.2:20260930T0100Z-1 | s2-q.2 | 2026-09-30T01:00Z | chat | 開いた問い";

/// 台帳: 閉じた問い 1 本と開いた問い 1 本。
fn ruling_ledger(place: &Place) {
    let closed = Bead { status: "closed", question: true, notes: RULING_NOTES, ..Bead::task("s2-q.1") };
    serve(place, &[closed, Bead { question: true, notes: OPEN_NOTES, ..Bead::task("s2-q.2") }]);
}

/// 固定の toy repo の file 3 本（線の commit の木に在る）。a.md は解ける 6 形（問い id・batch: 3 形〔裁定 id の欄 2・束の欄 1〕・
/// 欄の全体が `policy:batch:x`・`policy:p1`）と解けない 6 形（未知の問い id・開いた問い・逐語の欄にだけ在る問い id・逐語の欄の `batch:v9`・
/// 最後の欄の `batch:v8`・未知の `batch:zz`）・接頭辞違い 1・見本の 2 字面・線の前の時刻の形 1。
const A_MD: &str = "# a\n\
- 解ける: s2-q.1:20260930T0000Z-1 と batch:m1 と batch:m2 と batch:b7 と policy:batch:x と policy:p1\n\
- 解けない: s2-q.9:20260930T0000Z-1 と s2-q.2:20260930T0100Z-1 と s2-q.3:20260930T0200Z-1 と batch:v9 と batch:v8 と batch:zz\n\
- 接頭辞違い: tz-1:20260930T0000Z-1\n\
- 見本: batch:fix と policy:fix\n\
- 線の前: user 2026-09-29T12:00Z\n";

/// b.md: 線の前の時刻の形 1（c.md へ写す字面）と、a.md と同じ解ける字面 1。
const B_MD: &str = "# b\n- 線の前: user 2026-09-29T13:00Z と batch:m1\n";

/// c.md（線の後の commit）: b.md から写した時刻の形（別の file なので線の前に数えない）・線の後の時刻の形・見本の時刻の形。
const C_MD: &str = "# c\n- 写した: user 2026-09-29T13:00Z\n- 線の後: user 2026-09-30T05:00Z\n- 見本の時刻: user 2026-09-30T00:0xZ\n";

/// 固定の toy repo の 1 行（file 4 本〔.vessel.toml と md 3 本〕・数えた引用 17 件・解けない 8 字面・線の前 2 字面）。
const FIXED_LINE: &str = "ruling-cite: check=on files=4 cited=17 \
unresolved=8:batch:v8,batch:v9,batch:zz,s2-q.2:20260930T0100Z-1,s2-q.3:20260930T0200Z-1,s2-q.9:20260930T0000Z-1,user 2026-09-29T13:00Z,user 2026-09-30T05:00Z \
before-line=2:user 2026-09-29T12:00Z,user 2026-09-29T13:00Z";

/// 4 行目が check=on で母集団と件数と字面を出す。解ける 4 形（問い id・裁定 id の欄の batch:・束の欄の batch:・欄の全体が policy:batch:x）は
/// unresolved に載らず、接頭辞違いは数えず、見本の 2 字面と線の後の見本の時刻の形は cited に入らない。ほかの欄（逐語の欄・開いた問いの行）
/// だけの一致・逐語の欄の中の batch:・最後の欄の batch: は unresolved に載る。線の commit の木で別の file に在った時刻の形を後の
/// commit で別の file に写した引用は before-line でなく unresolved に数え、同じ file の同じ字面は before-line に数える。台帳は 1 回だけ読む。
#[test]
fn ledger_ruling_doctor_counts_the_fixed_toy_repo_with_its_population() {
    let place = cite_place("main").unwrap_or_else(|| panic!("置き場を作れる"));
    let first = declaration("ruling-check = true\nruling-fixtures = [\"batch:fix\", \"policy:fix\"]\n");
    let later = declaration("ruling-check = true\nruling-fixtures = [\"batch:fix\", \"policy:fix\", \"user 2026-09-30T00:0xZ\"]\n");
    history(&place, &[&[(".vessel.toml", first), ("docs/a.md", A_MD), ("docs/b.md", B_MD)], &[("docs/c.md", C_MD)], &[(".vessel.toml", later)]]);
    ruling_ledger(&place);
    assert_eq!(cite_line(&place), FIXED_LINE);
    assert_eq!(calls(&place), ["--readonly list --all --limit 0 --json"], "台帳は readonly で 1 回だけ読む");
    fs::remove_dir_all(&place.dir).ok();
}

/// key を持たない repo（宣言 file が無い・key が無い・`ruling-check = false`）は 3 形とも `check=off` の 1 行だけで、数えも断りもしない
/// （引用を持つ file が在っても・台帳を読めなくても件数を出さない）。
#[test]
fn ledger_ruling_doctor_without_the_key_is_off() {
    let place = cite_place("main").unwrap_or_else(|| panic!("置き場を作れる"));
    ruling_ledger(&place);
    history(&place, &[&[("docs/a.md", A_MD)]]);
    assert_eq!(cite_line(&place), "ruling-cite: check=off", "宣言 file が無い");
    history(&place, &[&[(".vessel.toml", declaration(""))]]);
    assert_eq!(cite_line(&place), "ruling-cite: check=off", "key が無い");
    history(&place, &[&[(".vessel.toml", declaration("ruling-check = false\nruling-fixtures = []\n"))]]);
    write_client(&place, "cat /dev/null\nexit 3");
    assert_eq!(cite_line(&place), "ruling-cite: check=off", "false・台帳を読めない周でも数えも断りもしない");
    history(&place, &[&[(".vessel.toml", declaration("ruling-check = true\n"))]]);
    assert_eq!(cite_line(&place), "ruling-cite: unreadable reason=ledger", "対照: true にすると台帳を読めない周は測れない");
    fs::remove_dir_all(&place.dir).ok();
}

/// 線は最初に true と読める commit: true → false → true と変わる履歴では、false の間に足した引用は線の後（before-line でなく unresolved）。
#[test]
fn ledger_ruling_doctor_line_is_the_first_true_commit_across_flips() {
    let place = cite_place("main").unwrap_or_else(|| panic!("置き場を作れる"));
    let (on, off) = (declaration("ruling-check = true\n"), declaration("ruling-check = false\n"));
    history(
        &place,
        &[
            &[(".vessel.toml", on), ("docs/a.md", "- user 2026-09-29T10:00Z\n")],
            &[(".vessel.toml", off), ("docs/p.md", "- user 2026-09-29T11:00Z\n")],
            &[(".vessel.toml", on)],
        ],
    );
    serve(&place, &[]);
    let want = "ruling-cite: check=on files=3 cited=2 unresolved=1:user 2026-09-29T11:00Z before-line=1:user 2026-09-29T10:00Z";
    assert_eq!(cite_line(&place), want, "線は最初の true の commit（最後の true の commit ではない）");
    fs::remove_dir_all(&place.dir).ok();
}

/// 壊れた宣言のまま `ruling-check = true` を足した commit と時刻の形 X を足した commit の後に、ruling-check の行を変えず別の行だけを直した
/// commit を置き、その後に Y を足した履歴では、線は直した commit で before-line は X の 1 件だけ（`-G ruling-check` で候補を絞ると線を取り逃がす）。
#[test]
fn ledger_ruling_doctor_line_is_the_commit_that_fixed_another_row() {
    let place = cite_place("main").unwrap_or_else(|| panic!("置き場を作れる"));
    let (broken, fixed) = (declaration("ruling-check = true\nclose-check = \"yes\"\n"), declaration("ruling-check = true\nclose-check = true\n"));
    history(
        &place,
        &[&[(".vessel.toml", broken)], &[("docs/x.md", "- user 2026-09-29T15:00Z\n")], &[(".vessel.toml", fixed)], &[("docs/y.md", "- user 2026-09-29T16:00Z\n")]],
    );
    serve(&place, &[]);
    let want = "ruling-cite: check=on files=3 cited=2 unresolved=1:user 2026-09-29T16:00Z before-line=1:user 2026-09-29T15:00Z";
    assert_eq!(cite_line(&place), want, "線は直した commit・X は線の前・Y は線の後");
    fs::remove_dir_all(&place.dir).ok();
}

/// 読めない 3 形は閉じた 3 語で、件数を 1 つも出さない（4 行目・逐語一致）。宣言 → 台帳 → git の順に最初に読めなかった 1 つ:
/// 壊した宣言は declaration（台帳も落ちていても）・落ちる台帳は ledger（main の ref が無くても）・main の ref を持たない repo は git。
#[test]
fn ledger_ruling_doctor_unreadable_names_the_first_unreadable_face() {
    let broken = cite_place("main").unwrap_or_else(|| panic!("置き場を作れる"));
    history(&broken, &[&[(".vessel.toml", declaration("ruling-check = \"yes\"\n")), ("docs/a.md", A_MD)]]);
    ruling_ledger(&broken);
    let want = "ruling-cite: unreadable reason=declaration";
    assert_eq!(cite_line(&broken), want, "壊した宣言");
    write_client(&broken, "cat /dev/null\nexit 3");
    assert_eq!(cite_line(&broken), want, "台帳も落ちていても宣言が先");
    let trunk = cite_place("trunk").unwrap_or_else(|| panic!("置き場を作れる"));
    history(&trunk, &[&[(".vessel.toml", declaration("ruling-check = true\n")), ("docs/a.md", A_MD)]]);
    ruling_ledger(&trunk);
    assert_eq!(cite_line(&trunk), "ruling-cite: unreadable reason=git", "main の ref を持たない repo");
    write_client(&trunk, "printf '[{\"id\":'");
    let torn = cite_line(&trunk);
    assert_eq!(torn, "ruling-cite: unreadable reason=ledger", "台帳が壊れていれば git より先");
    for found in [want, torn.as_str()] {
        assert!(!found.contains("files=") && !found.contains("cited="), "件数を出さない: {found}");
    }
    fs::remove_dir_all(&broken.dir).ok();
    fs::remove_dir_all(&trunk.dir).ok();
}
