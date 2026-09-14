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

use super::{check, shape, summary, Layout, RULES_REL};
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

/// repo の外に一意な tmp dir を作る。
fn make_tmp_dir() -> PathBuf {
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
/// 閾値の 7 行（R-C4-* / R-C13-1）は現物と同じ値で持つ（`Limits::read` が無い行を拒むので、fixture も
/// 実 repo が持つものを持つ）。
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
        ("R-C13-1", "DepBudget", limits.dep_budget),
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
    lints-set=<v> lints-optin=<v>/<v> deps-empty=<v> clippy-thresholds=<v> dep-budget=<v>/<v> \
    toolchain-pin=<v>.<v>.<v> \
    paths-clean=<v> private-clean=<v> non-rust-exec=<v>/<v> allow=<v> ci-shell-lines=<v> \
    claude-md-constitution=<v> enum-slices=<v> claude-spawn-points=<v> env-reads=<v>/<v> polarity=<v>/<v> \
    prose-gate=<v>/<v> seat-brief=<v> contracts-schema=<v>";

/// git を要する measure の fact（`.git` の無い木では測れない形になり、副 field も出ない）。
fn is_git_fact(token: &str) -> bool {
    ["paths-clean=", "private-clean=", "non-rust-exec=", "allow=", "prose-gate="]
        .iter()
        .any(|prefix| token.starts_with(prefix))
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
    if root.join(".git").exists() {
        assert_eq!(shape(&line), SUMMARY_PIN, "判定行の現物: {line}");
    } else {
        let without_git = |shaped: &str| {
            shaped
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

/// 非 Rust 実行物は **file 種別**で捕まえる（shebang / 実行 bit / 拡張子）。
///
/// 憲法 C12 の「歯は Rust 1 framework」は、歯以外の実行物から静かに崩れる。3 つの
/// 経路を**別々の file** で置くのは、1 つだけ見る実装（例: 拡張子しか見ない）が
/// 緑にならないようにするためである。
#[test]
fn non_rust_exec_denies_shebang_exec_bit_and_extension() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/from-shebang", "#!/bin/sh\necho hi\n");
        write_at(dir, "tools/from-mode", "echo hi\n");
        write_at(dir, "tools/from-ext.py", "print('hi')\n");
        let mode = fs::metadata(dir.join("tools/from-mode"))
            .expect("fixture の file を読める")
            .permissions();
        let mut mode = mode;
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
        fs::set_permissions(dir.join("tools/from-mode"), mode).expect("実行 bit を立てられる");
    });
    for rel in ["tools/from-shebang", "tools/from-mode", "tools/from-ext.py"] {
        assert!(
            violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains(rel)),
            "{rel} を非 Rust 実行物として名指す: {violations:?}"
        );
    }
}

/// 例外は **manifest の 1 面**（`repo.non_rust_exec_allow`）だけが持つ（憲法 C1）。
#[test]
fn non_rust_exec_allows_paths_listed_in_the_manifest_row() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/from-ext.py", "print('hi')\n");
        write_at(dir, RULES_REL, &rules_manifest(&["tools/from-ext.py"]));
    });
    assert!(
        !violations.iter().any(|line| line.starts_with("non-rust-exec:")),
        "例外行に載る path は通る: {violations:?}"
    );
    // **完全一致**である（部分一致で通ると、例外 1 本が dir ごと素通しになる）。
    let partial = check_fixture(|dir| {
        write_at(dir, "tools/from-ext.py", "print('hi')\n");
        write_at(dir, RULES_REL, &rules_manifest(&["tools/from-ext"]));
    });
    assert!(
        partial.iter().any(|line| line.starts_with("non-rust-exec:")),
        "前方一致では通さない: {partial:?}"
    );
}

/// **母集団 0 は「測れなかった」**（`paths-clean` と同じ極性・0 件を緑にしない）。
#[test]
fn non_rust_exec_is_unmeasured_when_the_population_is_empty() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    git_track_all(&dir);
    // index を空にする（file は作業木に残るので「見に行けたが 0 件」の形になる）。
    assert!(git_fixture(&dir, &["rm", "-r", "-q", "--cached", "."]), "index を空にできる");
    let violations = check(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        violations
            .iter()
            .any(|line| line.starts_with("non-rust-exec:") && line.contains("0 件")),
        "母集団 0 は違反として名乗る: {violations:?}"
    );
}

/// CI の shell 行は **`run:` の 1 行形と block scalar の継続行**を数える（検出線）。
#[test]
fn ci_shell_lines_counts_run_lines_including_continuations() {
    let yml = "jobs:\n  a:\n    steps:\n      - run: cargo test\n      - run: |\n          echo one\n          echo two\n\n      - uses: actions/checkout@v4\n      - run: echo tail\n";
    assert_eq!(
        crate::non_rust_exec::count_run_lines(yml),
        4,
        "1 行形 2 本 + 継続行 2 本"
    );
    // 継続行の終わりは**字下げが浅くなった行**で決まる（次の step を数え込まない）。
    assert_eq!(
        crate::non_rust_exec::count_run_lines("      - run: |\n          echo one\n      - uses: x\n"),
        1,
        "block を抜けた行は数えない"
    );
}

/// 例外行の **`[` 〜 `]` の中だけ**を読む（末尾コメントの引用符を拾わない）。
///
/// `value = [...] # 旧 "x" は外した` の**注記が例外を 1 件増やす**形は、例外を減らす
/// 意図の文が静かに例外を足す＝憲法 C1 の「値の面は 1 つ」が崩れる（lens 2026-09-11 H2）。
#[test]
fn non_rust_exec_ignores_quotes_in_trailing_comments() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/ghosted.py", "print('hi')\n");
        let row = rules_manifest(&[]);
        let commented = row.replace(
            "value = []",
            "value = [] # 旧 \"tools/ghosted.py\" は外した",
        );
        write_at(dir, RULES_REL, &commented);
    });
    assert!(
        violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains("tools/ghosted.py")),
        "コメント内の引用符は例外にならない: {violations:?}"
    );
}

/// **腐った例外行を落とす**（憲法 C10.3: 未配線の設定は CI が落とす）。
///
/// 母集団に居ない path を例外に置けるままだと、file を land する前に例外だけ先に通せる。
#[test]
fn non_rust_exec_denies_allow_entries_that_match_nothing() {
    let violations = check_fixture(|dir| {
        write_at(dir, RULES_REL, &rules_manifest(&["tools/never-existed.sh"]));
    });
    assert!(
        violations
            .iter()
            .any(|line| line.starts_with("non-rust-exec:") && line.contains("never-existed")),
        "母集団に居ない例外は違反: {violations:?}"
    );
}

/// rules manifest を読めない tree は **fail-closed**（`paths-clean` と同じ極性）。
///
/// `check` 全体は閾値を読めない時点で止まる（`layout:` の 1 件・`s2-07l.163`）ので、measure 単体の
/// 極性は `Layout` を組んで直に撃って測る（check 経由では届かない）。
#[test]
fn non_rust_exec_is_unmeasured_when_the_manifest_is_missing() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    let _ = fs::remove_file(dir.join(RULES_REL));
    git_track_all(&dir);
    let whole = check(&dir);
    let layout = Layout::discover(&dir);
    let single = layout.as_ref().ok().map(crate::non_rust_exec::measure);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        whole.iter().any(|line| line.starts_with("layout: ") && line.contains("manifest.toml")),
        "閾値を読めない周は check 全体が止まる: {whole:?}"
    );
    let single = single.unwrap_or_else(|| panic!("fixture の Layout を組める: {}", layout.err().unwrap_or_default()));
    assert!(
        single
            .violations
            .iter()
            .any(|line| line.starts_with("non-rust-exec:") && line.contains("manifest")),
        "manifest を読めない周は違反として名乗る: {:?}",
        single.violations
    );
}

/// 閾値の行を 1 本欠いた manifest は `check` 全体を止め、欠いた行 id を名指す（SRS FR18）。
#[test]
fn check_is_blocked_when_a_threshold_row_is_missing() {
    let violations = check_fixture(|dir| {
        let dropped = rules_manifest(&[]).replace("id = \"R-C4-3\"", "id = \"R-C4-9\"");
        write_at(dir, RULES_REL, &dropped);
    });
    assert_single(&violations, "layout");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("R-C4-3"), "欠いた行 id を名指す: {head}");
}

/// 判定行に clippy-thresholds と dep-budget の fact が並び、`clippy.toml` を緩めると違反が
/// key と両値を名指す（写しの実効値を manifest が縛る・憲法 C14.2）。
#[test]
fn check_fails_when_clippy_toml_loosens_a_manifest_threshold() {
    let line = summary_fixture(|_| {});
    assert!(line.contains(" clippy-thresholds=ok "), "{line}");
    assert!(
        line.contains(&format!(" dep-budget={}/{} ", ALLOWED_DEPS.len(), real_limits().dep_budget)),
        "{line}"
    );
    let violations = check_fixture(|dir| {
        let loosened = clippy_toml().replace(
            &format!("too-many-lines-threshold = {}", real_limits().fn_lines),
            "too-many-lines-threshold = 600",
        );
        write_at(dir, "clippy.toml", &loosened);
    });
    assert_single(&violations, "clippy-thresholds");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("too-many-lines-threshold = 600") && head.contains("R-C4-4.fn-lines"), "{head}");
}

/// 分類器の拡張子は**閉じた列**で、`js` を外さない（契約が名指しで禁じた形）。
///
/// asset は**例外行に載せて**通す。定義を緩めると次の asset が黙って通る。
#[test]
fn non_rust_exec_pins_the_extension_list() {
    let sample = |rel: &str| crate::paths_clean::TrackedFile {
        rel: rel.to_owned(),
        mode: "100644".to_owned(),
        oid: String::new(),
    };
    for ext in ["sh", "bash", "zsh", "py", "bats", "pl", "rb", "js", "ts", "mjs"] {
        assert!(
            crate::non_rust_exec::is_non_rust_exec(&sample(&format!("a/b.{ext}")), None),
            "{ext} は分類器の内側"
        );
    }
    assert!(
        !crate::non_rust_exec::is_non_rust_exec(&sample("a/b.rs"), None),
        "Rust は非 Rust 実行物ではない"
    );
}

/// 判定行の**値**まで測る（検出線が恒久 0 になっても緑、を防ぐ）。
#[test]
fn non_rust_exec_and_ci_shell_lines_report_counts_in_the_fact_line() {
    let dir = make_tmp_dir();
    write_healthy(&dir);
    write_at(&dir, "tools/one.py", "print('hi')\n");
    write_at(&dir, RULES_REL, &rules_manifest(&["tools/one.py"]));
    write_at(&dir, ".github/workflows/ci.yml", "jobs:\n  a:\n    steps:\n      - run: cargo test\n");
    git_track_all(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        line.contains("non-rust-exec=1/") && line.contains("allow=1"),
        "該当 / 母集団 / 例外数を判定行へ出す: {line}"
    );
    assert!(line.contains("ci-shell-lines=1"), "検出線の値を判定行へ出す: {line}");
}

/// 拡張子の照合は **大文字小文字を区別しない**（`.PY` は「拡張子 py の file」である）。
///
/// 線引きの問題ではなく**同じ signal の取り落とし**（lens 2026-09-11・planner 裁定で本便の射程）。
#[test]
fn non_rust_exec_matches_extensions_case_insensitively() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/shouty.PY", "print('hi')\n");
    });
    assert!(
        violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains("shouty.PY")),
        "大文字の拡張子も分類器の内側: {violations:?}"
    );
}

/// **BOM 付きの shebang も shebang である**（先頭 3 byte の BOM で見落とさない）。
#[test]
fn non_rust_exec_sees_shebang_behind_a_bom() {
    let violations = check_fixture(|dir| {
        write_at(dir, "tools/bom-script", "\u{feff}#!/bin/sh\necho hi\n");
    });
    assert!(
        violations.iter().any(|line| line.starts_with("non-rust-exec:") && line.contains("bom-script")),
        "BOM の後ろの shebang も見る: {violations:?}"
    );
}

/// 判定行に散文の門の fact が載る（設計 contract-source.md §12・`s2-07l.202`）。
#[test]
fn prose_gate_fact_is_in_summary() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    assert!(line.contains(" prose-gate="), "判定行に prose-gate の fact が在るはず: {line}");
}

/// fact は `prose-gate=<違反数>/<母集団>` の形で、現物の `docs/design` は違反 0（母集団は空でない）。
///
/// 分岐は `.git` の有無（flip-check の展開木では git を要する他の fact と同じく測れない形）。
#[test]
fn prose_gate_fact_counts_zero_violations_on_workspace() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let line = summary(&root);
    let value = line
        .split(' ')
        .find_map(|token| token.strip_prefix("prose-gate="))
        .unwrap_or_default();
    if root.join(".git").exists() {
        let counts: Vec<usize> = value.split('/').filter_map(|part| part.parse().ok()).collect();
        assert_eq!(counts.len(), 2, "<n>/<m> の形のはず: {line}");
        assert_eq!(counts.first(), Some(&0), "現物の設計 doc は違反 0 のはず: {line}");
        assert!(counts.get(1).is_some_and(|marked| *marked >= 1), "母集団は空でないはず: {line}");
    } else {
        assert!(value.starts_with("n/a(") || value == "?", ".git の無い木では測れない形のはず: {line}");
    }
}

/// 印を持つ文が pointer を失った設計 doc は prose-gate だけで落ち、file:line と理由を名指す。
#[test]
fn prose_gate_names_violating_design_doc_in_check() {
    let violations = check_fixture(|dir| {
        write_at(dir, "docs/design/probe-8w.md", "# 設計\n席は lock を確保しなければならない。\n");
    });
    assert_single(&violations, "prose-gate");
    let head = violations.first().map(String::as_str).unwrap_or_default();
    assert!(head.contains("docs/design/probe-8w.md:2: no-pointer"), "{head}");
}

/// tracked な設計 doc が 0 本の木は違反（0 本の緑にしない）・健全な木の fact は `0/<母集団>`。
#[test]
fn prose_gate_fails_closed_without_design_docs() {
    let missing = check_fixture(|dir| {
        let _ = fs::remove_file(dir.join(PROSE_DOC_REL));
    });
    assert_single(&missing, "prose-gate");
    let dir = make_tmp_dir();
    write_healthy(&dir);
    git_track_all(&dir);
    let line = summary(&dir);
    let _ = fs::remove_dir_all(&dir);
    assert!(line.contains(" prose-gate=0/1"), "健全な木は違反 0 / 母集団 1: {line}");
}
