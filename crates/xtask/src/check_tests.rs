//! `check` の歯。**本体は `check.rs`** で、ここには test だけが在る。
//!
//! 分けたのは憲法 C4（1 file の上限）である——`check.rs` が 1499 / 1500 行で満杯になり、
//! 同じ file へ行を足す契約を受けられなくなった（`s2-07l.257`）。`#[path]` で `check` の
//! 子 module として取り込むので、module path は `check::tests` のまま＝歯の名前は 1 つも
//! 変わらない。fixture helper（`check_fixture` / `write_at` 等）は `seat_brief::tests` も
//! 使うので `pub(crate)` で公開する。
//!
//! この file は `#[cfg(test)] mod` の形を持たないが、`crates/*/src/**/*_tests.rs` は
//! **名前で test file と見なして丸ごと写す**（s2-07l.34 の (6)）ので、ここへ足した歯は
//! base へ写り flip を検査される。

// 純粋な移動（`check.rs` の test 区間から歯を足さずに写した・s2-07l.257）。
// flip-check: moved s2-07l.257

use super::{check, inspect, shape, summary, Layout, RULES_REL};
use crate::genmanifest;
use crate::limits::{Limits, ALLOWED_DEPS, REQUIRED_LINTS};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 擬似 workspace の core crate 名。実 NAME の字面を xtask の .rs へ
/// 持ち込まないための別名である（name-literal の不変条件）。
const FIXTURE_CORE: &str = "demo";
/// 擬似 workspace の version。
const FIXTURE_VERSION: &str = "0.1.0";
/// 擬似 workspace の toolchain channel（版番号の字面）。
const FIXTURE_CHANNEL: &str = "1.98.1";

/// 同一 process 内での dir 名衝突を避ける連番。
static SEQ: AtomicU32 = AtomicU32::new(0);

/// repo の外に一意な tmp dir を作る（`rules_wired::tests` も manifest の fixture を置くのに使う）。
pub(crate) fn make_tmp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("xtask-check-{}-{nanos}-{seq}", std::process::id()));
        if fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    panic!("擬似 workspace 用の tmp dir を作れない");
}

/// root Cargo.toml の本文。`skip` に与えた lint 名だけを落とす。
fn root_manifest(skip: Option<&str>) -> String {
    let mut text = String::from("[workspace]\nresolver = \"2\"\n");
    text.push_str(&format!(
        "members = [\"crates/{FIXTURE_CORE}\", \"crates/xtask\"]\n"
    ));
    for section in ["rust", "clippy"] {
        text.push_str(&format!("\n[workspace.lints.{section}]\n"));
        for (owner, lint, level) in REQUIRED_LINTS {
            if *owner != section || skip == Some(*lint) {
                continue;
            }
            text.push_str(&format!("{lint} = \"{level}\"\n"));
        }
    }
    text.push_str("\n[profile.dev]\ndebug = \"line-tables-only\"\n");
    text
}

/// member crate の Cargo.toml。`optin` が false なら `[lints]` を落とす。
fn member_manifest(name: &str, optin: bool) -> String {
    let mut text = format!(
        "[package]\nname = \"{name}\"\nversion = \"{FIXTURE_VERSION}\"\nedition = \"2021\"\n"
    );
    if optin {
        text.push_str("\n[lints]\nworkspace = true\n");
    }
    text.push_str("\n[dependencies]\n\n[dev-dependencies]\n");
    text
}

/// `rel` へ本文を書く（親 dir は作る）。
pub(crate) fn write_at(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("擬似 workspace の dir を作れる");
    }
    fs::write(&path, body).expect("擬似 workspace の file を書ける");
}

/// `rust-toolchain.toml` の本文を channel から組み立てる。
fn toolchain_file(channel: &str) -> String {
    format!("[toolchain]\nchannel = \"{channel}\"\ncomponents = [\"clippy\", \"rustfmt\"]\n")
}

/// 全 check 項目を満たす擬似 workspace を書く。
fn write_healthy(dir: &Path) {
    write_at(dir, "Cargo.toml", &root_manifest(None));
    write_at(dir, "rust-toolchain.toml", &toolchain_file(FIXTURE_CHANNEL));
    write_at(
        dir,
        &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
        &member_manifest(FIXTURE_CORE, true),
    );
    write_at(
        dir,
        &format!("crates/{FIXTURE_CORE}/src/name.rs"),
        &format!("pub const NAME: &str = \"{FIXTURE_CORE}\";\n"),
    );
    write_at(dir, "crates/xtask/Cargo.toml", &member_manifest("xtask", true));
    write_at(dir, "crates/xtask/src/main.rs", "fn main() {}\n");
    write_at(
        dir,
        genmanifest::MANIFEST_REL,
        &genmanifest::render(FIXTURE_CORE, FIXTURE_VERSION),
    );
    // **実 repo が持つものは fixture も持つ**。rules manifest が無い tree を「測れない」
    // 側へ倒す measure（non-rust-exec）が在るので、無いままだと fixture 全体が赤くなる。
    write_at(dir, RULES_REL, &rules_manifest(&[]));
    // clippy の閾値 file も同じ（clippy-thresholds は不在と key 欠落を違反に倒す・`s2-07l.163`）。
    write_at(dir, "clippy.toml", &clippy_toml());
    // nextest の test-group の写しと e2e の木も同じ（nextest-tmux-group は不在・key 欠落・group の外の歯を違反に
    // 倒す・`s2-07l.360`）。歯は `seat::seat_x` の 1 本で、filter はそれを固定形で列挙する。
    write_at(dir, crate::check_facts::NEXTEST_REL, &crate::check_facts::nextest_fixture(real_limits().tmux_test_threads, &["seat::seat_x"]));
    for (rel, body) in crate::check_facts::e2e_fixture() {
        write_at(dir, &format!("crates/{FIXTURE_CORE}/{rel}"), body);
    }
    // claude の構築点も同じ（claude-spawn-points は見失った形を違反に倒す・`s2-07l.101`）。
    write_at(
        dir,
        &format!("crates/{FIXTURE_CORE}/src/headless/mod.rs"),
        "pub fn build(claude: &str) -> std::process::Command {\n    let mut cmd = std::process::Command::new(claude);\n    cmd.arg(\"--setting-sources\").arg(\"\").arg(\"--strict-mcp-config\");\n    cmd\n}\n",
    );
    // 極性一覧の snapshot も同じ（polarity は不在を違反に倒す・`s2-07l.25`）。
    write_at(
        dir,
        &format!("crates/{FIXTURE_CORE}/{}", crate::polarity::SNAPSHOT_REL),
        "---\nsource: x\nexpression: form\n---\nguard=a timing=in-loop on-failure=fail-closed boundary=m::A\npolarity: guards=1 in-loop=1 post-hoc=0 fail-open=0\n",
    );
    // 設計 doc も同じ（prose-gate は対象 0 本を違反に倒す）。印を持つ文は pointer 付きで適合。
    write_at(dir, PROSE_DOC_REL, "# 設計\n\n器は失敗を記録しなければならない（C1）。\n");
    // 席の指示文の雛形も同じ（seat-brief は雛形 0 枚と行の無い役割を違反に倒す・`s2-07l.248`）。
    write_at(dir, &brief_rel(), "{role} {target} {anchor} → SSOT: ADR-0022 §2.4\n{capabilities}\n");
    // 契約表の欄の 2 面も同じ（contracts-schema は不在を違反に倒す・`s2-07l.208`）。
    crate::check_facts::contracts_fixture(FIXTURE_CORE).iter().for_each(|(rel, body)| write_at(dir, rel, body));
}

/// fixture の `clippy.toml`（3 閾値は現物の manifest と同じ値）。
fn clippy_toml() -> String {
    let limits = real_limits();
    format!(
        "too-many-arguments-threshold = {}\ntoo-many-lines-threshold = {}\ncognitive-complexity-threshold = {}\n",
        limits.fn_args, limits.fn_lines, limits.fn_complexity
    )
}

/// fixture の設計 doc の相対 path。
const PROSE_DOC_REL: &str = "docs/design/probe-7q.md";

/// fixture の雛形の相対 path（rules manifest の `role.planner` の行と対）。
pub(crate) fn brief_rel() -> String {
    format!("crates/{FIXTURE_CORE}/src/seat/brief/planner.txt")
}

/// 現物の rules manifest から読んだ閾値（fixture の期待値と閾値行はここから機械的に作る＝
/// magic number を書かない・`s2-07l.163`）。
fn real_limits() -> Limits {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(RULES_REL);
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()));
    Limits::read(&text).unwrap_or_else(|reason| panic!("{reason}"))
}

/// fixture の rules manifest。`allow` に与えた path が例外行に載る。役割の行は planner 1 つ（権能 2 つ）。
/// 閾値の 11 行（R-C4-* / R-C4.line-width / R-C13-1〔.per-pr / .check-delta-ms〕/ gate.tmux_test_threads）は
/// 現物と同じ値で持つ（`Limits::read` が無い行を拒むので、fixture も実 repo が持つものを持つ）。
fn rules_manifest(allow: &[&str]) -> String {
    let items = allow
        .iter()
        .map(|path| format!("\"{path}\""))
        .collect::<Vec<String>>()
        .join(", ");
    let limits = real_limits();
    let rows = [
        ("R-C4-1", "CoreLines", limits.core_lines),
        ("R-C4-2", "ModuleLines", limits.file_lines),
        ("R-C4-3", "TestSrcRatioPct", limits.test_src_ratio_pct),
        ("R-C4-4.fn-lines", "FnLines", limits.fn_lines),
        ("R-C4-4.complexity", "FnComplexity", limits.fn_complexity),
        ("R-C4-4.args", "FnArgs", limits.fn_args),
        ("R-C4.line-width", "LineWidth", limits.line_width),
        ("R-C13-1", "DepBudget", limits.dep_budget),
        ("R-C13-1.per-pr", "DepPerPr", limits.dep_per_pr),
        ("R-C13-1.check-delta-ms", "CheckDeltaMs", limits.check_delta_ms),
        ("gate.tmux_test_threads", "GateTmuxTestThreads", limits.tmux_test_threads),
    ];
    let mut text = String::from("schema = 1\n");
    for (id, kind, value) in rows {
        text.push_str(&format!(
            "\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\n\
             ruling = \"fixture\"\nruled_at = \"2026-09-14\"\n"
        ));
    }
    text.push_str(&format!(
        "\n[[rule]]\nid = \"repo.non_rust_exec_allow\"\nkind = \"RepoNonRustExecAllow\"\n\
         value = [{items}]\nenabled = true\nruling = \"fixture\"\nruled_at = \"2026-09-11\"\n\n\
         [[rule]]\nid = \"role.planner\"\nkind = \"RoleCapabilities\"\nvalue = [\"answer\", \"relay\"]\n\
         enabled = true\nruling = \"fixture\"\nruled_at = \"2026-09-14\"\n"
    ));
    text
}

/// 健全な擬似 workspace を作り `mutate` で 1 項目だけ壊してから check を回す。
/// 後始末は assert より前に済ませる。
pub(crate) fn check_fixture(mutate: impl FnOnce(&Path)) -> Vec<String> {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    mutate(&dir);
    git_track_all(&dir);
    let violations = check(&dir);
    let _ = fs::remove_dir_all(&dir);
    violations
}

/// fixture を git 化して index を埋める（paths-clean の母集団は index である）。
fn git_track_all(dir: &Path) {
    assert!(git_fixture(dir, &["init", "-q"]), "fixture で git init できる");
    let shown = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .expect("git rev-parse を起動できる");
    let top = String::from_utf8_lossy(&shown.stdout).trim().to_owned();
    assert_eq!(
        fs::canonicalize(Path::new(&top)).expect("toplevel を canonicalize できる"),
        fs::canonicalize(dir).expect("fixture を canonicalize できる"),
        "fixture 自身が repo root のはず"
    );
    assert!(git_fixture(dir, &["add", "-A"]), "fixture で git add -A できる");
}

/// fixture 内で git を撃つ（identity と署名を明示し外の設定に依存しない）。
fn git_fixture(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// member manifest の `section` へ `dep = value` を 1 本足した本文。
fn with_dep(name: &str, section: &str, dep: &str, value: &str) -> String {
    let mut text = member_manifest(name, true);
    let anchor = format!("[{section}]\n");
    let line = format!("{dep} = {value}\n");
    match text.rfind(&anchor) {
        Some(at) => {
            text.insert_str(at + anchor.len(), &line);
            text
        }
        None => format!("{text}\n[{section}]\n{line}"),
    }
}

/// 違反が `tag` ちょうど 1 件であることを表明する。
pub(crate) fn assert_single(violations: &[String], tag: &str) {
    assert_eq!(violations.len(), 1, "違反は 1 件のはず: {violations:?}");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(
        head.starts_with(&format!("{tag}: ")),
        "tag {tag} の違反のはず: {head}"
    );
}

/// `count` 行の埋め草 .rs（NAME literal を含まない）。
fn filler_rs(count: usize) -> String {
    (0..count).map(|_| "// filler\n").collect()
}

/// 自 workspace は違反 0 で通る。
#[test]
fn check_passes_on_workspace() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let violations = check(&root);
    assert!(violations.is_empty(), "自 workspace で違反 0 のはず: {violations:?}");
}

/// 判定行の外形の pin（repo root で撃ったときの形）。値は [`shape`] で伏せてある。
const SUMMARY_PIN: &str = "xtask check: ok core-lines=<v>/<v> file-lines=<v>/<v> \
    test-src-ratio=<v>/<v> name-literal=<v> manifest-name=<v> manifest-version=<v>.<v>.<v> \
    lints-set=<v> lints-optin=<v>/<v> deps-empty=<v> clippy-thresholds=<v> \
    nextest-tmux-group=<v> tests=<v> files=<v> dep-budget=<v>/<v> \
    toolchain-pin=<v>.<v>.<v> \
    paths-clean=<v> private-clean=<v> non-rust-exec=<v>/<v> allow=<v> ci-shell-lines=<v> \
    claude-md-constitution=<v> claude-md-done=<v> claude-md-prose=<v>/<v> enum-slices=<v> claude-spawn-points=<v> env-reads=<v>/<v> polarity=<v>/<v> \
    polarity-sites=<v>/<v>/<v> \
    prose-gate=<v>/<v> seat-brief=<v> contracts-schema=<v> rules-wired=<v>/<v> \
    rules-parity=<v>/<v> doc-only=<v> manifest-only=<v> population=<v>/<v>";

/// git を要する measure の fact（`.git` の無い木では測れない形になり、副 field も出ない）。
fn is_git_fact(token: &str) -> bool {
    ["paths-clean=", "private-clean=", "non-rust-exec=", "allow=", "prose-gate="]
        .iter()
        .any(|prefix| token.starts_with(prefix))
}

/// 副 field `ids=`（id の列）を持つ token の頭（判定行での並び順）。`rules-wired` の読み手の無い行
/// （`s2-07l.160`）と、`rules-parity` の片側だけの id 2 列 `doc-only` / `manifest-only`（`s2-07l.164`）。
const IDS_OWNERS: &[&str] = &["rules-wired=", "doc-only=", "manifest-only="];

/// 副 field `ids=`（id の列）。値の**中身**が形を決める（`R-C7-1` の `-` や `hook.budget_ms` の `.` `_` は
/// [`shape`] が残す区切り）ので、wire の便が列を縮めるたびに形が変わる。検出線の値であって判定行の形では
/// ないので、[`SUMMARY_PIN`] との突合からはこの token だけを外す（本数と、[`IDS_OWNERS`] の直後に来ることは
/// 別に見る・`s2-07l.160` / `s2-07l.164`）。
fn is_wired_ids(token: &str) -> bool {
    token.starts_with("ids=")
}

/// `ids=` の副 field を除いた並び。
fn without_wired_ids(shaped: &str) -> String {
    shaped.split(' ').filter(|token| !is_wired_ids(token)).collect::<Vec<&str>>().join(" ")
}

/// 判定行の**名前・並び・値の書式**を外形として pin する（ADR-0013 §2.1・`s2-07l.87`）。
///
/// 値は環境で動くので [`shape`] で伏せる。measure を 1 つ落とす／2 つ並べ替える／値の
/// 書式を変える、のどれでも落ちる。「measure が N 本」は数えない（判定行は自己区切りで
/// なく、`allow=` は non-rust-exec の副 field）。pin の単位は token の並びである。
///
/// 分岐は**測定対象と独立な判別子**（`.git` の有無）で行う（先例
/// [`check_paths_clean_scans_noncanonical_root`]）: flip-check の base 健全性前段は
/// `git archive` で展開した `.git` の無い木で全 suite を撃つので、そこで repo root の形を
/// 求めると base が恒久に赤くなり**以後の全 PR の flip-check が止まる**（lens-87 HIGH-1・
/// 展開木で実測）。`.git` の無い木では git を要する 2 つの measure だけが測れない形
/// （`n/a(not-a-repo-root)` か `?`）になるので、その fact を除いた並びが同じことと、
/// 2 つが数でないことを見る。
#[test]
fn check_summary_shape_pins_names_order_and_value_forms() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    // fact の不変条件: 空白で割った token はすべて `k=v` 形（副 field も含む）。空白入りの値は
    // 割れて環境の値が pin へ素通りするので、書式を決める本 bead でここに立てる（lens-87 MEDIUM-1）。
    let facts = line
        .strip_prefix(super::SUMMARY_PREFIX)
        .unwrap_or_else(|| panic!("判定行は接頭辞で始まるはず: {line}"));
    for token in facts.split(' ').filter(|token| !token.is_empty()) {
        assert!(token.contains('='), "fact の token は k=v 形のはず: {token} in {line}");
    }
    // 副 field `ids=` は [`IDS_OWNERS`] の本数だけ・**それぞれの直後**に在る（列の中身は pin しない・
    // [`is_wired_ids`]）。token ごと消える実装と、別の場所へ動く実装はここで落ちる。
    let tokens: Vec<&str> = facts.split(' ').collect();
    let owners: Vec<&str> = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| is_wired_ids(token))
        .map(|(at, _)| at.checked_sub(1).and_then(|before| tokens.get(before)).copied().unwrap_or_default())
        .collect();
    assert_eq!(owners.len(), IDS_OWNERS.len(), "ids= は {} つ: {line}", IDS_OWNERS.len());
    for (owner, head) in owners.iter().zip(IDS_OWNERS) {
        assert!(owner.starts_with(head), "ids= は {head} の直後: {line}");
    }
    if root.join(".git").exists() {
        assert_eq!(without_wired_ids(&shape(&line)), SUMMARY_PIN, "判定行の現物: {line}");
    } else {
        let without_git = |shaped: &str| {
            without_wired_ids(shaped)
                .split(' ')
                .filter(|token| !is_git_fact(token))
                .collect::<Vec<&str>>()
                .join(" ")
        };
        assert_eq!(
            without_git(&shape(&line)),
            without_git(SUMMARY_PIN),
            ".git の無い木でも git を要しない fact の並びは同じはず: {line}"
        );
        assert!(paths_clean_unnumbered(&line), ".git の無い木では数が出ないはず: {line}");
        // private-clean は paths-clean と同じ母集団を持つ git 依存 fact＝同じ 2 形のどちらかで
        // **必ず載る**（token ごと消える実装はここで落ちる・`s2-07l.32`）。
        assert!(
            line.contains("private-clean=n/a(") || line.contains("private-clean=?"),
            ".git の無い木では private-clean も測れない形で載るはず: {line}"
        );
        assert!(
            line.contains("non-rust-exec=n/a(") || line.contains("non-rust-exec=?"),
            ".git の無い木では non-rust-exec も測れない形のはず: {line}"
        );
    }
}

/// 擬似 workspace の core crate に enum と const slice の対を 1 つ置く。
fn write_enum_slice(dir: &Path, enum_body: &str, slice_body: &str) {
    write_at(
        dir,
        &format!("crates/{FIXTURE_CORE}/src/kinds.rs"),
        &format!(
            "/// 閉じた enum。\npub enum Kind {{\n{enum_body}}}\n\n\
             /// 全 variant。\npub const KINDS: &[Kind] = &[\n{slice_body}];\n"
        ),
    );
}

/// 擬似 workspace を `mutate` で変えてから判定行（値入り）を取る。後始末は assert より前。
pub(crate) fn summary_fixture(mutate: impl FnOnce(&Path)) -> String {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    mutate(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    line
}

/// enum の末尾に足した variant が const slice に無い木は `enum-slices` で落ちる
/// （ADR-0013 §2.3 が「どの面も受けていない」と記録した穴・`s2-07l.88`）。集合で見るので
/// 重複と余りも落ちる（数の一致では入れ忘れと余りが相殺して「一致」に化ける・lens-88 MEDIUM-4）。
#[test]
fn enum_slices_reports_variant_missing_from_slice() {
    let violations = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    /// 1。\n    Alpha,\n\n    /// 2。\n    Beta,\n    /// 末尾に足した。\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n",
        );
    });
    assert_single(&violations, "enum-slices");
    let line = violations.first().map(String::as_str).unwrap_or_default();
    assert!(line.contains("Kind::Gamma"), "欠けた variant を名指す: {line}");
    // 揃っている木は通る（空行と doc は形の一部ではない・対は 1 つ数える）。
    let ok = summary_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n\n    Beta,\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Gamma,\n",
        );
    });
    assert!(ok.contains(" enum-slices=1"), "揃った対を 1 つ数える: {ok}");
    // 重複（同じ variant を 2 回）と余り（enum に無い名前）も落ちる。
    let duplicated = check_fixture(|dir| {
        write_enum_slice(dir, "    Alpha,\n    Beta,\n", "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Beta,\n");
    });
    assert_single(&duplicated, "enum-slices");
    let extra = check_fixture(|dir| {
        write_enum_slice(dir, "    Alpha,\n", "    Kind::Alpha,\n    Kind::Ghost,\n");
    });
    assert_single(&extra, "enum-slices");
    assert!(
        extra.first().is_some_and(|line| line.contains("Kind::Ghost")),
        "余りの要素を名指す: {extra:?}"
    );
}

/// 型の側も黙って母集団から落とさない（lens-88 MEDIUM-1）: `&'static [Enum]` と字下げされた
/// const は対に数え、`&[&Enum]` は読めない型として違反、struct の slice は対象外（0 対）。
#[test]
fn enum_slices_covers_type_forms_instead_of_dropping_them() {
    let core_src = format!("crates/{FIXTURE_CORE}/src/kinds.rs");
    let static_form = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n    Beta,\n}\n\npub const KINDS: &'static [Kind] = &[Kind::Alpha];\n",
        );
    });
    assert_single(&static_form, "enum-slices");
    let indented = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n    Beta,\n}\n\nimpl Kind {\n    pub const ALL: &[Kind] = &[Kind::Alpha];\n}\n",
        );
    });
    assert_single(&indented, "enum-slices");
    let by_ref = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n}\n\npub const KINDS: &[&Kind] = &[&Kind::Alpha];\n",
        );
    });
    assert_single(&by_ref, "enum-slices");
    // ライフタイム付きの参照と、`:` の後に空白が無い形も逃がさない（lens-88 再確認）。
    let by_static_ref = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n}\n\npub const KINDS: &'static [&'static Kind] = &[&Kind::Alpha];\n",
        );
    });
    assert_single(&by_static_ref, "enum-slices");
    let no_space = check_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub enum Kind {\n    Alpha,\n    Beta,\n}\n\npub const KINDS:&[Kind] = &[Kind::Alpha];\n",
        );
    });
    assert_single(&no_space, "enum-slices");
    let of_struct = summary_fixture(|dir| {
        write_at(
            dir,
            &core_src,
            "pub struct Kind;\n\npub const KINDS: &[Kind] = &[Kind, Kind];\n",
        );
    });
    assert!(of_struct.contains(" enum-slices=0"), "struct の slice は対象外（0 対）: {of_struct}");
    // 自 workspace には ADR-0013 §2.3 が記録した 5 面が在る。黙って母集団から消えれば減る。
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let own = summary(&root);
    let pairs: usize = own
        .split(' ')
        .find_map(|token| token.strip_prefix("enum-slices="))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("自 workspace の判定行に enum-slices の数が出るはず: {own}"));
    assert!(pairs >= 5, "自 workspace の対は 5 面以上のはず（黙って消えた）: {own}");
}

/// 読めない形（payload 付き variant・属性行・`Enum::` でない要素）は**違反に倒す**
/// （fail-closed・「読めなかった」を「一致していた」に化けさせない）。
#[test]
fn enum_slices_refuses_unrecognized_forms_instead_of_counting() {
    // (a) payload 付き variant: 数えれば 3 == 3 で「一致」に化ける形。
    let payload = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta(u8),\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Gamma,\n",
        );
    });
    assert_single(&payload, "enum-slices");
    // (b) 属性行が混じる形。
    let attribute = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    #[default]\n    Beta,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n",
        );
    });
    assert_single(&attribute, "enum-slices");
    // (c) slice の要素が `Kind::<Variant>` の形でない。
    let element = check_fixture(|dir| {
        write_enum_slice(dir, "    Alpha,\n", "    Kind::Alpha, OTHER,\n");
    });
    assert_single(&element, "enum-slices");
}

/// 集合が揃っていても**宣言順**とずれた slice は落ち、ずれた最初の添字を名指す
/// （憲法 C2「宣言順」・`s2-07l.177`）。集合だけの一致は「全部並んでいるが順序は無関係」を通す。
#[test]
fn enum_slices_order_names_the_first_mismatched_index() {
    // 逆順（添字 0 から食い違う）。
    let reversed = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Gamma,\n    Kind::Beta,\n    Kind::Alpha,\n",
        );
    });
    assert_single(&reversed, "enum-slices");
    let head = reversed.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("order:KINDS"), "順序の違反として名指す: {head}");
    assert!(head.contains("expected=Kind::Alpha"), "宣言順の名を出す: {head}");
    assert!(head.contains("at=0"), "ずれた添字を出す: {head}");
    // 途中の入れ替え（添字 0 は一致・**最初の**ずれだけを 1 件出す）。
    let swapped = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Gamma,\n    Kind::Beta,\n",
        );
    });
    assert_single(&swapped, "enum-slices");
    let swapped_head = swapped.first().map(String::as_str).unwrap_or_default();
    assert!(swapped_head.contains("expected=Kind::Beta"), "添字 1 の期待を出す: {swapped_head}");
    assert!(swapped_head.contains("at=1"), "最初のずれの添字: {swapped_head}");
}

/// 宣言順に並ぶ slice は違反 0 で、対は 1 つ数える（順序の面を足しても fact の形は変わらない）。
/// 欠けが在る周は添字を出さない——1 つの入れ忘れで以降の添字が丸ごとずれ、同じずれを 2 面で
/// 数えることになるからである（集合が揃うまで順序は見ない）。
#[test]
fn enum_slices_order_passes_when_slice_follows_declaration() {
    let ordered = summary_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Alpha,\n    Kind::Beta,\n    Kind::Gamma,\n",
        );
    });
    assert!(ordered.contains(" enum-slices=1"), "宣言順の対を 1 つ数える: {ordered}");
    let missing = check_fixture(|dir| {
        write_enum_slice(
            dir,
            "    Alpha,\n    Beta,\n    Gamma,\n",
            "    Kind::Gamma,\n    Kind::Alpha,\n",
        );
    });
    assert_single(&missing, "enum-slices");
    let head = missing.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("Kind::Beta"), "欠けを名指す: {head}");
    assert!(!head.contains("order:"), "集合が揃うまで添字は出さない: {head}");
}

/// 上限 +1 行の .rs は file-lines だけで落ち、上限ちょうどは通る（上限は manifest の R-C4-2）。
#[test]
fn check_fails_on_oversized_file() {
    let max_file_lines = usize::try_from(real_limits().file_lines).unwrap_or(usize::MAX);
    let over = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/src/big.rs"),
            &filler_rs(max_file_lines + 1),
        );
    });
    assert_single(&over, "file-lines");

    let at_limit = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/src/big.rs"),
            &filler_rs(max_file_lines),
        );
    });
    assert!(
        at_limit.is_empty(),
        "上限ちょうどは違反 0 のはず: {at_limit:?}"
    );
}

/// 必須 lint を 1 本落とすと lints-set だけで落ちる。
#[test]
fn check_fails_on_missing_lint() {
    let dropped = REQUIRED_LINTS
        .first()
        .map(|(_, lint, _)| *lint)
        .unwrap_or_default();
    let violations = check_fixture(|dir| {
        write_at(dir, "Cargo.toml", &root_manifest(Some(dropped)));
    });
    assert_single(&violations, "lints-set");
}

/// member の `[lints] workspace = true` を落とすと lints-optin だけで落ちる。
#[test]
fn check_fails_on_missing_lints_optin() {
    let violations = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
            &member_manifest(FIXTURE_CORE, false),
        );
    });
    assert_single(&violations, "lints-optin");
}

/// plugin.json の name を変えると manifest-name だけで落ちる。
#[test]
fn check_fails_on_manifest_mismatch() {
    let violations = check_fixture(|dir| {
        let other = format!("{FIXTURE_CORE}-x");
        write_at(
            dir,
            genmanifest::MANIFEST_REL,
            &genmanifest::render(&other, FIXTURE_VERSION),
        );
    });
    assert_single(&violations, "manifest-name");
}

/// 浮動解決する `1.98` は toolchain-pin だけで落ちる（負の語検査だけの実装なら
/// 素通りする形＝この tag の非空虚性を示す）。
#[test]
fn check_fails_on_floating_toolchain() {
    let violations = check_fixture(|dir| {
        write_at(dir, "rust-toolchain.toml", &toolchain_file("1.98"));
    });
    assert_single(&violations, "toolchain-pin");
}

/// tmp の root に e2e の木（`check_facts::e2e_fixture` + `extra`・core crate の dir からの相対）と nextest の設定（`None`
/// なら置かない）を置いて `nextest-tmux-group` を測る（閾値は現物の manifest・`s2-07l.360`・設計 gate-cost.md §3.1）。
fn tmux_fixture(config: Option<&str>, extra: &[(&str, &str)]) -> super::Measured {
    let root = make_tmp_dir();
    let core = root.join("crates").join(FIXTURE_CORE);
    for (rel, body) in crate::check_facts::e2e_fixture().into_iter().chain(extra.iter().copied()) {
        write_at(&core, rel, body);
    }
    if let Some(text) = config {
        write_at(&root, crate::check_facts::NEXTEST_REL, text);
    }
    let layout = Layout { root: root.clone(), core_dir: core, member_dirs: Vec::new(), name: FIXTURE_CORE.to_owned() };
    let got = crate::check_facts::measure_nextest_tmux_group(&layout, &real_limits());
    let _ = fs::remove_dir_all(&root);
    got
}

/// nextest の設定の fixture（`threads` と固定形の filter に載せる名の列）。
fn nextest_toml(threads: u64, names: &[&str]) -> String {
    crate::check_facts::nextest_fixture(threads, names)
}

/// (a) 行あり + 同値の写し + 固定形の filter で fact `nextest-tmux-group=ok`（母集団 = 歯 1 本・file 1 つ）。現物の
/// workspace も ok（母集団は pin しない・数は notes に写す）。
#[test]
fn nextest_tmux_group_passes_when_max_threads_matches_the_manifest() {
    let limits = real_limits();
    let same = tmux_fixture(Some(&nextest_toml(limits.tmux_test_threads, &["seat::seat_x"])), &[]);
    assert_eq!(same.violations, Vec::<String>::new());
    assert_eq!(same.fact, "nextest-tmux-group=ok tests=1 files=1");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let layout = Layout::discover(&root).unwrap_or_else(|reason| panic!("{reason}"));
    let real = crate::check_facts::measure_nextest_tmux_group(&layout, &limits);
    assert_eq!(real.violations, Vec::<String>::new(), "現物の .config/nextest.toml は manifest と e2e の木に一致");
    assert!(real.fact.starts_with("nextest-tmux-group=ok tests="), "{}", real.fact);
}

/// (b) 値違い / file 無し / key 無しの 3 fixture が各 1 件の違反を名指す（読めなかったを一致に化けさせない）。
#[test]
fn nextest_tmux_group_names_drift_missing_file_and_missing_key() {
    let want = real_limits().tmux_test_threads;
    let drift = tmux_fixture(Some(&nextest_toml(want.saturating_add(1), &["seat::seat_x"])), &[]);
    assert_eq!(drift.violations.len(), 1, "{:?}", drift.violations);
    let line = drift.violations.first().map(String::as_str).unwrap_or_default();
    assert!(line.starts_with("nextest-tmux-group: test-groups.tmux.max-threads = "), "{line}");
    assert!(line.ends_with(&format!(" ≠ gate.tmux_test_threads = {want}")), "manifest の値を名指す: {line}");
    assert_eq!(drift.fact, "nextest-tmux-group=drift tests=1 files=1");
    let absent = tmux_fixture(None, &[]);
    assert_eq!(absent.violations.len(), 1, "{:?}", absent.violations);
    assert_eq!(absent.fact, "nextest-tmux-group=?", "file の無い周は測れない形");
    let without = nextest_toml(want, &["seat::seat_x"]).replace("max-threads", "max-thre4ds");
    let missing = tmux_fixture(Some(&without), &[]);
    assert_eq!(missing.violations.len(), 1, "{:?}", missing.violations);
    assert!(missing.violations.first().is_some_and(|line| line.contains("max-threads が無い")), "{:?}", missing.violations);
}

/// (c) fixture の e2e file（`start_seat(` を名指す `#[test] fn pipe_x()`・helper 越しの `pipe_y()`）で「group の外」を
/// module 付きの fn 名で名指し、歯に無い名は「幽霊」として名指す（両向き）。種に届かない歯は母集団の外。
#[test]
fn nextest_tmux_group_names_a_tmux_test_outside_the_group() {
    let pipe = "use crate::seat::start_seat;\n\nfn via_helper() {\n    let _guard = start_seat(\"h\");\n}\n\n\
                #[test]\nfn pipe_x() {\n    let _guard = start_seat(\"x\");\n}\n\n#[test]\nfn pipe_y() {\n    via_helper();\n}\n\n\
                #[test]\nfn pipe_plain() {\n    assert!(true);\n}\n";
    let extra = [("tests/e2e/pipe.rs", pipe)];
    let want = real_limits().tmux_test_threads;
    let outside = tmux_fixture(Some(&nextest_toml(want, &["seat::seat_x", "seat::ghost"])), &extra);
    assert_eq!(outside.fact, "nextest-tmux-group=drift tests=3 files=2", "母集団は固定点で決まる歯の本数");
    let lines: Vec<&str> = outside.violations.iter().map(String::as_str).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("nextest-tmux-group: pipe::pipe_x は") && line.contains("group の外")), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("nextest-tmux-group: pipe::pipe_y は") && line.contains("group の外")), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with("nextest-tmux-group: seat::ghost は") && line.contains("幽霊")), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("pipe_plain")), "種に届かない歯は母集団の外: {lines:?}");
    let listed = tmux_fixture(Some(&nextest_toml(want, &["seat::seat_x", "pipe::pipe_x", "pipe::pipe_y"])), &extra);
    assert_eq!(listed.violations, Vec::<String>::new(), "3 本を列挙すれば一致");
}

/// (d) 固定形でない filter（接頭辞の regex・空の名・override 無し）は typed に断る（0 件の集合に化けない）。
#[test]
fn nextest_tmux_group_refuses_a_filter_outside_the_fixed_form() {
    let want = real_limits().tmux_test_threads;
    let prefixed = nextest_toml(want, &["seat::seat_x"]).replace("test(/^(seat::seat_x)$/)", "test(/^seat::/)");
    let refused = tmux_fixture(Some(&prefixed), &[]);
    assert_eq!(refused.violations.len(), 1, "{:?}", refused.violations);
    let line = refused.violations.first().map(String::as_str).unwrap_or_default();
    assert!(line.contains("固定形") && line.contains("test(/^seat::/)"), "形と現物を名指す: {line}");
    assert!(!line.contains("group の外"), "0 件の列挙には化けない: {line}");
    let empty_name = tmux_fixture(Some(&nextest_toml(want, &["seat::seat_x", ""])), &[]);
    assert!(empty_name.violations.iter().any(|line| line.contains("固定形")), "{:?}", empty_name.violations);
    let no_override = tmux_fixture(Some(&format!("[test-groups.tmux]\nmax-threads = {want}\n")), &[]);
    assert_eq!(no_override.violations.len(), 1, "{:?}", no_override.violations);
    assert!(no_override.violations.first().is_some_and(|line| line.contains("[[profile.default.overrides]] が無い")));
}

/// in-module の `#[cfg(test)]` 以降も test 行として数える（tests/ dir だけを
/// 数える実装では本 leg で恒真 GREEN になる）。
#[test]
fn ratio_counts_in_module_tests() {
    let violations = check_fixture(|dir| {
        let mut body = String::from("pub fn tiny() {}\n#[cfg(test)]\n");
        body.push_str(&filler_rs(64));
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/src/heavy.rs"),
            &body,
        );
    });
    assert_single(&violations, "test-src-ratio");
}

/// tracked file の本文に private path 形が在れば paths-clean だけで落ち、
/// 違反本文に相対 path と行番号が載る。
#[test]
fn check_fails_on_home_path() {
    let violations = check_fixture(|dir| {
        let mark = format!("{}home{}", "/", "/");
        write_at(dir, "docs/note.md", &format!("# note\nsee {mark}someone/x\n"));
    });
    assert_single(&violations, "paths-clean");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(
        head.contains("docs/note.md:2"),
        "違反本文に相対 path と行番号が載るはず: {head}"
    );
}

/// allowlist に在る dev-dep は通り、allowlist 外の依存は deps-empty で落ちる。
#[test]
fn check_allows_listed_dev_dep() {
    let (section, dep) = ALLOWED_DEPS.first().copied().unwrap_or(("", ""));
    assert!(!section.is_empty(), "allowlist は 1 本以上のはず");
    let allowed = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
            &with_dep(FIXTURE_CORE, section, dep, "\"1\""),
        );
    });
    assert!(
        allowed.is_empty(),
        "allowlist の dev-dep は違反 0 のはず: {allowed:?}"
    );
    let outside = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
            &with_dep(FIXTURE_CORE, section, "not-in-allowlist", "\"1\""),
        );
    });
    assert_single(&outside, "deps-empty");
}

/// section 名の完全一致では拾えない dep 宣言形も deps-empty で落ちる。
///
/// 完全一致だけの実装はこの 4 形を 1 本も数えず、allowlist を素通りさせる。
#[test]
fn check_counts_nested_and_scoped_dep_sections() {
    let member_forms = [
        "[dependencies.not-in-allowlist]\nversion = \"1\"\n",
        "[build-dependencies]\nnot-in-allowlist = \"1\"\n",
        "[target.'cfg(unix)'.dependencies]\nnot-in-allowlist = \"1\"\n",
    ];
    for form in member_forms {
        let violations = check_fixture(|dir| {
            write_at(
                dir,
                &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
                &format!("{}\n{form}", member_manifest(FIXTURE_CORE, true)),
            );
        });
        assert_single(&violations, "deps-empty");
    }
    let root_form = check_fixture(|dir| {
        write_at(
            dir,
            "Cargo.toml",
            &format!(
                "{}\n[workspace.dependencies]\nnot-in-allowlist = \"1\"\n",
                root_manifest(None)
            ),
        );
    });
    assert_single(&root_form, "deps-empty");
}

/// allowlist の key 名を借りた `package =` 改名は deps-empty で落ちる。
///
/// key 名だけで照合する実装は別 crate の持ち込みを素通りさせる。
#[test]
fn check_rejects_renamed_allowlisted_dep() {
    let (section, dep) = ALLOWED_DEPS.first().copied().unwrap_or(("", ""));
    assert!(!section.is_empty(), "allowlist は 1 本以上のはず");
    let inline = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
            &with_dep(
                FIXTURE_CORE,
                section,
                dep,
                "{ package = \"not-in-allowlist\", version = \"1\" }",
            ),
        );
    });
    assert_single(&inline, "deps-empty");
    let table = check_fixture(|dir| {
        write_at(
            dir,
            &format!("crates/{FIXTURE_CORE}/Cargo.toml"),
            &format!(
                "{}\n[{section}.{dep}]\npackage = \"not-in-allowlist\"\nversion = \"1\"\n",
                member_manifest(FIXTURE_CORE, true)
            ),
        );
    });
    assert_single(&table, "deps-empty");
}

/// 非 UTF-8 byte を混ぜた tracked file でも private path 形を見逃さない。
///
/// `read_to_string` で読む実装は無言で skip するので needle が素通りする。
#[test]
fn check_scans_non_utf8_tracked_file() {
    let violations = check_fixture(|dir| {
        let mark = format!("{}home{}", "/", "/");
        let mut body = format!("# note\nsee {mark}someone/x").into_bytes();
        body.push(0xFF);
        body.push(b'\n');
        let path = dir.join("docs/bin.md");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("擬似 workspace の dir を作れる");
        }
        fs::write(&path, &body).expect("非 UTF-8 の file を書ける");
    });
    assert_single(&violations, "paths-clean");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(
        head.contains("docs/bin.md:2"),
        "違反本文に相対 path と行番号が載るはず: {head}"
    );
}

/// tracked symlink の **link target 文字列**を母集団に入れる。
///
/// 作業木から読むと symlink の中身は追跡先の file になるので、target に private path
/// 形が在っても素通りする。index の blob（mode 120000）を読めば target そのものが出る。
#[test]
fn paths_clean_reads_tracked_symlink_target() {
    let violations = check_fixture(|dir| {
        let mark = format!("{}home{}", "/", "/");
        fs::create_dir_all(dir.join("docs")).expect("fixture に docs を作れる");
        std::os::unix::fs::symlink(format!("{mark}x/secret"), dir.join("docs/link"))
            .expect("fixture に symlink を張れる");
    });
    assert_single(&violations, "paths-clean");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    // **行番号まで見る**。dangling symlink は作業木から読めないので、blob を見ない
    // 実装でも「読めない tracked file は違反」で `docs/link` を含む 1 件が出てしまい、
    // file 名だけの assert は素通りする（review 2026-09-10 critical・実測で再現した）。
    // 行番号が付くのは **本文を走査できた**ときだけである。
    assert!(
        head.contains("docs/link:1"),
        "link target の 1 行目を走査した違反のはず: {head}"
    );
}

/// 免除 file の免除は **コメント行だけ**である（全文免除にすると、その file の中では
/// private path を書き放題という穴になる）。
#[test]
fn paths_clean_skip_exempts_only_comment_lines_of_beads_config() {
    let mark = format!("{}home{}", "/", "/");
    let commented = format!("# example: {mark}someone/repo\nprefix: s2\n");
    let ok = check_fixture(|dir| write_at(dir, super::PATHS_CLEAN_SKIP, &commented));
    assert!(
        !ok.iter().any(|line| line.starts_with("paths-clean")),
        "コメント行だけなら免除される: {ok:?}"
    );
    // 行末コメントを持つ**設定行**は免除されない（行のどこかに `#` が在れば免除、と
    // する実装はここで落ちる＝免除の fail-open）。字下げコメントは免除される
    // （行頭空白を読み飛ばさない実装はここで落ちる）。
    let live = format!("  # 字下げ: {mark}a\ndb-path: {mark}someone/db  # 行末 note\n");
    let bad = check_fixture(|dir| write_at(dir, super::PATHS_CLEAN_SKIP, &live));
    assert_single(&bad, "paths-clean");
    let head = bad.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains(":2"), "非コメント行だけを名指すはず: {head}");
}

/// `../..` 形の root（`git rev-parse --show-toplevel` の出力と字面では一致しない）
/// でも paths-clean が走査本数を数える。素の `==` 比較の実装はここで落ちる。
///
/// 分岐は**測定対象と独立な判別子**（`.git` の有無）で行う。paths-clean の値そのもので
/// 分岐すると「数が出ないこと」を常に許してしまい、素の `==` 比較の回帰を取り逃がす。
/// `.git` は main checkout では dir、worktree では file、flip-check が `git archive` で
/// 展開した base tree では不在である。
#[test]
fn check_paths_clean_scans_noncanonical_root() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    if root.join(".git").exists() {
        let scanned = paths_clean_scanned(&line)
            .unwrap_or_else(|| panic!("repo の中では走査本数が出るはず: {line}"));
        assert!(scanned >= 1, "走査した tracked file 数は 1 以上のはず: {line}");
    } else {
        assert!(
            paths_clean_unnumbered(&line),
            ".git の無い木では数が出ないはず: {line}"
        );
        assert_eq!(paths_clean_scanned(&line), None, "数として読めない: {line}");
    }
}

/// git は引けるが root が toplevel でないときの値（flip-check の base tree はこれ。
/// `target/` は repo の working tree の内側なので git 自体は成功する）。
const PATHS_CLEAN_NA: &str = "paths-clean=n/a(not-a-repo-root)";

/// git そのものが引けないときの値（repo の外の tmp 木はこれ）。
const PATHS_CLEAN_UNMEASURED: &str = "paths-clean=?";

/// paths-clean が数でない 2 形のどちらかか。
fn paths_clean_unnumbered(line: &str) -> bool {
    line.contains(PATHS_CLEAN_NA) || line.contains(PATHS_CLEAN_UNMEASURED)
}

/// `paths-clean=` の直後の 10 進整数。数字で始まらなければ `None`。
///
/// 非 repo root では `n/a(not-a-repo-root)` が入るので、数として読めないことは
/// 欠陥ではない。ここで `None` に倒しておかないと、flip-check が base を
/// `git archive` で展開した木（`.git` 無し）で撃つときに parse が panic する。
fn paths_clean_scanned(line: &str) -> Option<usize> {
    let tail = line.split_once("paths-clean=").map(|(_, rest)| rest)?;
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// git repo でない木では paths-clean は数を返さず、走査数は数えられない。
///
/// flip-check の base 健全性前段は、`git archive` で展開した `.git` の無い木で
/// 全 test を撃つ。その形を fixture で再現する（`git_track_all` を呼ばない）。
#[test]
fn entrance_paths_clean_is_unnumbered_outside_repo() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(paths_clean_unnumbered(&line), "非 repo root の値: {line}");
    assert_eq!(paths_clean_scanned(&line), None, "走査数は数えられない: {line}");
}

/// `rules-parity`（`s2-07l.164`・設計 rules-manifest.md §4）の fixture の憲法 HTML。§3 の行 3 つ
/// （`r-c1-1` / `r-c2-1` / `r-c3-1`）に加え、**読んではいけない 2 形**を置く: comment の中の `<tr>` と、
/// `<` を含む script 本文（現物の憲法も持つ形・`claude_md.rs` の読み手を共有しない実装はここで落ちる）。
const PARITY_HTML: &str = concat!(
    "<!DOCTYPE html>\n<html><head><meta charset=\"UTF-8\">\n",
    "<script>if (a < b) { probe(); }</script>\n",
    "<!-- <tr id=\"r-c9-9\"><td>OMITTED</td></tr> -->\n",
    "</head><body>\n<table>\n",
    "<tr id=\"r-c1-1\"><td>R-C1-1</td><td>C1</td></tr>\n",
    "<tr id=\"r-c2-1\"><td>R-C2-1</td><td>C2</td></tr>\n",
    "<tr id=\"r-c3-1\"><td>R-C3-1</td><td>C3</td></tr>\n",
    "<tr data-delta-id=\"r-c8-8\"><td>id の無い行（改訂 marker は id でない）</td></tr>\n",
    "</table>\n</body></html>\n",
);

/// 同じ fixture の憲法から `r-c3-1` の行だけを除いた HTML（(c) の両側一致の周）。
const PARITY_HTML_TWO: &str = concat!(
    "<!DOCTYPE html>\n<html><body>\n<table>\n",
    "<tr id=\"r-c1-1\"><td>R-C1-1</td></tr>\n",
    "<tr id=\"r-c2-1\"><td>R-C2-1</td></tr>\n",
    "</table>\n</body></html>\n",
);

/// fixture の manifest の 1 行（`rules_diff::rows` が要る 4 key を持つ）。
fn parity_row(id: &str) -> String {
    format!(
        "\n[[rule]]\nid = \"{id}\"\nkind = \"Probe\"\nvalue = 1\nenabled = true\n\
         ruling = \"fixture\"\nruled_at = \"2026-09-16\"\n"
    )
}

/// `ids` の行を持つ fixture の manifest。
fn parity_manifest(ids: &[&str]) -> String {
    let mut text = String::from("schema = 1\n");
    for id in ids {
        text.push_str(&parity_row(id));
    }
    text
}

/// (a) の manifest: 畳む compound 行・形に当たるが文書に無い行・形を持たない `R-` 行・運用行。
const PARITY_ROWS: &[&str] = &["R-C1-1", "R-C2-1.fn-lines", "R-C4.line", "R-C9-9", "gate.x"];

/// tmp の root に fixture の憲法（`None` なら置かない）と manifest を置いて `rules-parity` を測る。
fn parity_fixture(html: Option<&str>, manifest: &str) -> super::Measured {
    let root = make_tmp_dir();
    if let Some(text) = html {
        write_at(&root, crate::claude_md::SOURCE_REL, text);
    }
    write_at(&root, RULES_REL, manifest);
    let layout = Layout {
        root: root.clone(),
        core_dir: root.join("crates").join(FIXTURE_CORE),
        member_dirs: Vec::new(),
        name: FIXTURE_CORE.to_owned(),
    };
    let got = crate::rules_parity::measure(&layout);
    let _ = fs::remove_dir_all(&root);
    got
}

/// fact から `<tag>=<n> ids=<列>` の列を取る（`-` は 0 本）。
fn parity_ids<'a>(fact: &'a str, tag: &str) -> Vec<&'a str> {
    let listed = fact
        .split_once(&format!(" {tag}="))
        .and_then(|(_, rest)| rest.split_once(" ids="))
        .map(|(_, rest)| rest.split(' ').next().unwrap_or_default())
        .unwrap_or_else(|| panic!("fact に {tag}= と ids= が在るはず: {fact}"));
    if listed == "-" {
        Vec::new()
    } else {
        listed.split(',').collect()
    }
}

/// (a) compound 行は §3 の行 id へ畳み、形に当たるが文書に無い行と形を持たない `R-` 行は manifest-only、
/// 文書だけの行は doc-only、運用行は母集団外（母集団 = 文書側 3 / manifest 側 R-* 4 を同じ行に）。
#[test]
fn rules_parity_folds_compound_rows_to_the_section_id() {
    let got = parity_fixture(Some(PARITY_HTML), &parity_manifest(PARITY_ROWS));
    assert!(got.violations.is_empty(), "検出線は違反を立てない: {:?}", got.violations);
    assert_eq!(
        got.fact,
        "rules-parity=1/2 doc-only=1 ids=R-C3-1 manifest-only=2 ids=R-C4.line,R-C9-9 population=3/4",
        "畳んだ 2 面の差と母集団"
    );
}

/// (b) `R-C4.line`（`R-<条>-<番号>` の形を持たない）は `R-C4` に**畳まない**（run 1 の誤畳みの退行 pin）:
/// manifest-only の列に `R-C4.line` が自身の id のまま残り、`R-C4` は現れない。
#[test]
fn rules_parity_does_not_fold_non_section_ids_to_the_article() {
    let got = parity_fixture(Some(PARITY_HTML), &parity_manifest(PARITY_ROWS));
    let manifest_only = parity_ids(&got.fact, "manifest-only");
    assert_eq!(manifest_only, ["R-C4.line", "R-C9-9"], "形を持たない行は自身の id のまま: {}", got.fact);
    assert!(!manifest_only.contains(&"R-C4"), "条だけへ畳んだ id が現れた: {}", got.fact);
    assert_eq!(parity_ids(&got.fact, "doc-only"), ["R-C3-1"], "文書だけの行: {}", got.fact);
    // 条に小数点を持つ行 id（`R-C1.2-3`）は形に当たり、その compound 行は畳む。`R-C4-`（番号無し）は畳まない。
    let dotted = parity_fixture(
        Some("<table><tr id=\"r-c1.2-3\"></tr></table>"),
        &parity_manifest(&["R-C1.2-3.args", "R-C4-"]),
    );
    assert_eq!(
        dotted.fact,
        "rules-parity=0/1 doc-only=0 ids=- manifest-only=1 ids=R-C4- population=1/2",
        "小数点付きの条は畳み、番号の無い id は畳まない"
    );
}

/// (c) 両側が一致する周は `0` と母集団を同じ行に出す（0 と「測れない」を融合しない）: 憲法の無い木は
/// `n/a`・行に分けられない manifest は `?` + 違反 1 件。
#[test]
fn rules_parity_reports_zero_with_population_when_both_sides_match() {
    let got = parity_fixture(Some(PARITY_HTML_TWO), &parity_manifest(&["R-C1-1", "R-C2-1.fn-lines", "gate.x"]));
    assert!(got.violations.is_empty(), "検出線は違反を立てない: {:?}", got.violations);
    assert_eq!(
        got.fact,
        "rules-parity=0/0 doc-only=0 ids=- manifest-only=0 ids=- population=2/2",
        "0 本でも母集団は出る"
    );
    let absent = parity_fixture(None, &parity_manifest(&["R-C1-1"]));
    assert!(absent.violations.is_empty(), "憲法の無い木は測る対象が無い: {:?}", absent.violations);
    assert_eq!(absent.fact, "rules-parity=n/a(no-constitution)", "n/a は 0 と別の字面");
    let broken = parity_fixture(Some(PARITY_HTML_TWO), "schema = 1\n\n[[rule]]\nid = \"R-C1-1\"\n");
    assert_eq!(broken.fact, "rules-parity=?", "行に分けられない manifest は ?");
    assert_eq!(broken.violations.len(), 1, "測れない周だけ違反: {:?}", broken.violations);
}

/// (d) 判定行の形（[`SUMMARY_PIN`]）に token `rules-parity=<doc-only>/<manifest-only>` が在り、現物の repo で
/// 撃った判定行がその形に一致する。`ids=` の 2 列は [`without_wired_ids`] で突合から外し、本数と対で見る。
#[test]
fn rules_parity_token_is_in_summary_pin() {
    assert!(
        SUMMARY_PIN.contains(" rules-parity=<v>/<v> doc-only=<v> manifest-only=<v> population=<v>/<v>"),
        "pin に rules-parity の token が在る: {SUMMARY_PIN}"
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    let tokens: Vec<&str> = line.split(' ').collect();
    let at = tokens
        .iter()
        .position(|token| token.starts_with("rules-parity="))
        .unwrap_or_else(|| panic!("判定行に rules-parity= が在るはず: {line}"));
    let heads: Vec<String> = tokens
        .iter()
        .skip(at)
        .take(6)
        .map(|token| token.split_once('=').map(|(head, _)| head).unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        heads,
        ["rules-parity", "doc-only", "ids", "manifest-only", "ids", "population"],
        "現物の判定行の token の並び: {line}"
    );
    let shaped = without_wired_ids(&shape(&line));
    assert!(
        shaped.contains(" rules-parity=<v>/<v> doc-only=<v> manifest-only=<v> population=<v>/<v>"),
        "現物の判定行が pin の形に一致する: {shaped}"
    );
    // 本数と列は対（`doc-only=<n>` の n と `ids=` の要素数）。現物の値は pin しない（Landed 後に admin が実測）。
    for tag in ["doc-only", "manifest-only"] {
        let count: usize = tokens
            .iter()
            .find_map(|token| token.strip_prefix(&format!("{tag}=")))
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{tag}= は数のはず: {line}"));
        assert_eq!(parity_ids(&line, tag).len(), count, "{tag} の本数と列は対: {line}");
    }
}

// 非 Rust 実行物と散文の門の歯は主題ごとの子 module（`check_<主題>_tests.rs`・`#[path]` で
// 取り込む）に在り、この file は共有の helper と残りの歯を持つ（s2-07l.370・純粋な移動・C4 R-C4-2）。
// flip-check: moved s2-07l.370

#[path = "check_nonrust_tests.rs"]
mod nonrust;

#[path = "check_prose_tests.rs"]
mod prose;
