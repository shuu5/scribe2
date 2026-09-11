//! `cargo xtask check` の本体。憲法 C4 の歯（行数 / 比 / manifest parity / lints）を測る。
//!
//! 違反は 1 件 1 行 `<tag>: <本文>` で返し、1 件以上なら CLI 面が rc 1 を返す。
//! 違反 0 のときだけ各 tag の実測値入りサマリを 1 行出す（失敗時にサマリは出さない）。
//! **測る tag の列挙はここに書かない**——列挙の SSOT は判定行を組み立てる実装
//! （[`summary`]）と、それが出す判定行そのものである（ADR-0013 §2.1）。doc へ写した列挙は
//! 腐るので、増減のたびに doc を直す形を採らない。

use crate::toml_lite::{entries_in, quoted, string_array};
use std::fs;
use std::path::{Path, PathBuf};

/// task runner 自身の package 名。core crate はこれ以外の member として発見する。
const RUNNER_PACKAGE: &str = "xtask";

/// core crate の名前定数を宣言する行の前置き。
const NAME_CONST_PREFIX: &str = "pub const NAME: &str =";

/// in-module test の始まりを示す行頭の印（■C の測定定義）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// paths-clean の母集団から外す tracked file。**ただ 1 本で固定**である
/// （bd が生成し private path 形の例をコメントに持つため）。
///
/// 測定一式は [`crate::paths_clean`] へ移したが、この名前だけは本 file に残す——
/// 免除を測る歯が本 file の test 区間に在り、`#[cfg(test)]` を src 区間へ置くと
/// test-src-ratio がその行から下（本 file の残り全部）を test 区間として数えるからである。
/// 実測値は本 file の行数そのものに依存するのでここへは焼かない（bead s2-07l.33 の notes）。
pub(crate) const PATHS_CLEAN_SKIP: &str = ".beads/config.yaml";

/// 検査対象 workspace の骨組み。root から 1 度だけ組み立てる。
pub struct Layout {
    /// workspace root（全 path はここからの相対で解決し cwd を直読みしない）。
    pub root: PathBuf,
    /// core crate の dir（member のうち task runner 以外）。
    pub core_dir: PathBuf,
    /// 全 member の dir。
    pub member_dirs: Vec<PathBuf>,
    /// core crate の `name.rs` が持つ NAME の値。
    pub name: String,
}

impl Layout {
    /// workspace root から crate の配置と NAME を読み取る。
    ///
    /// members は 1 行の配列を前提とする（骨格の root manifest はその形である）。
    pub fn discover(root: &Path) -> Result<Self, String> {
        let manifest = read_text(&root.join("Cargo.toml"))?;
        let members = workspace_members(&manifest);
        if members.is_empty() {
            return Err(format!("{} に workspace members が無い", root.display()));
        }
        let member_dirs: Vec<PathBuf> = members.iter().map(|rel| root.join(rel)).collect();
        let core_dir = find_core_dir(&member_dirs)?;
        let name = read_name_const(&core_dir.join("src").join("name.rs"))?;
        Ok(Self {
            root: root.to_path_buf(),
            core_dir,
            member_dirs,
            name,
        })
    }

    /// core crate の `[package] version`。
    pub fn core_version(&self) -> Result<String, String> {
        self.core_package_field("version")
    }

    /// core crate の `[package]` から 1 key を読む。
    pub(crate) fn core_package_field(&self, key: &str) -> Result<String, String> {
        let manifest = read_text(&self.core_dir.join("Cargo.toml"))?;
        package_field(&manifest, key)
            .ok_or_else(|| format!("{} の [package] に {key} が無い", self.core_dir.display()))
    }
}

/// check の結果。違反行の列と、違反 0 のときに出す 1 行サマリを持つ。
pub struct Report {
    /// `<tag>: <本文>` 形の違反行。
    pub violations: Vec<String>,
    /// 各 tag の実測値を載せた 1 行サマリ。
    pub summary: String,
}

/// 1 tag の測定結果。
pub(crate) struct Measured {
    /// サマリ行に載せる `tag=値` の断片。
    pub(crate) fact: String,
    /// 違反行（合格なら空）。
    pub(crate) violations: Vec<String>,
}

/// 判定行の接頭辞。この右に各 measure の fact が空白区切りで並ぶ。
const SUMMARY_PREFIX: &str = "xtask check: ok";

/// 読み込んだ `.rs` 1 本。
pub(crate) struct SourceFile {
    /// 絶対 path。
    pub(crate) path: PathBuf,
    /// 本文。
    pub(crate) text: String,
}

impl SourceFile {
    /// 物理行数（末尾改行の有無で差を出さない）。
    pub(crate) fn lines(&self) -> usize {
        self.text.lines().count()
    }

    /// 最初に現れる行頭 `#[cfg(test)]` から file 末尾までを test 行、残りを src 行と数える。
    pub(crate) fn split_test_src(&self) -> (usize, usize) {
        let lines: Vec<&str> = self.text.lines().collect();
        match lines.iter().position(|line| line.starts_with(TEST_MOD_MARK)) {
            Some(at) => (lines.len().saturating_sub(at), at),
            None => (0, lines.len()),
        }
    }
}

/// workspace を測って [`Report`] を返す。
pub fn inspect(root: &Path) -> Report {
    let layout = match Layout::discover(root) {
        Ok(found) => found,
        Err(reason) => return blocked(&reason),
    };
    let files = match collect_rs_files(&layout.root) {
        Ok(found) => found,
        Err(reason) => return blocked(&reason),
    };
    let mut measured = vec![
        crate::check_sizes::measure_core_lines(&layout, &files),
        crate::check_sizes::measure_file_lines(&files),
        crate::check_sizes::measure_test_src_ratio(&files),
        crate::check_sizes::measure_name_literal(&layout, &files),
    ];
    measured.extend(crate::check_facts::measure_manifests(&layout));
    measured.extend(crate::check_facts::measure_lints(&layout));
    measured.push(crate::check_facts::measure_deps_empty(&layout));
    measured.push(crate::check_facts::measure_toolchain_pin(&layout));
    measured.push(crate::paths_clean::measure(&layout));
    measured.push(crate::non_rust_exec::measure(&layout));
    measured.push(crate::non_rust_exec::ci_shell_lines(&layout));
    measured.push(crate::claude_md::measure(&layout));
    measured.push(crate::enum_slices::measure(&files));
    measured.push(crate::spawn_points::measure(&layout, &files));
    measured.push(crate::polarity::measure(&layout));
    fold(measured)
}

/// 純関数面（■D1）。`root` 配下を測り違反行の列を返す。`process::exit` はしない。
pub fn check(root: &Path) -> Vec<String> {
    inspect(root).violations
}

/// 違反 0 のときに出す 1 行サマリ。
pub fn summary(root: &Path) -> String {
    inspect(root).summary
}

/// 判定行を**値を伏せた形**へ写す（tag の名前・並び・値の書式だけを残す）。
///
/// [`SUMMARY_PREFIX`] は字面のまま残し、その右の各 token は `=` より右の英数字の連なりを
/// `<v>` に置き、区切り（`/` `.` `(` 等）はそのまま残す: `a=12/300 b=0.1.0` →
/// `a=<v>/<v> b=<v>.<v>.<v>`。tag は触らない。`=` を持たない token（fact の値が空白を含んで
/// 割れた片割れ）は丸ごと伏せる＝環境で動く値が素通りしない（lens-87 MEDIUM-1）。英数字は
/// Unicode で見る（非 ASCII の値も伏せる）。値は環境で動く（file-lines / paths-clean 等）ので、
/// 外形として pin できるのはこの形までである。ADR-0013 §2.1 が SSOT と定めた判定行を、集合と
/// 順序で測る歯の材料（bd `s2-07l.87`）。
///
/// 呼ぶのは test 区間の pin だけで、runtime に判定行を消費する口は作らない（外形を増やさない）。
/// それでも src に置くのは、base に test 区間だけを写した木で compile error＝flip-check の RED
/// を構造で作るためである。非 test build では未使用になるので、憲法 C11 の口（理由付き expect）
/// で dead_code だけを除く。
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "test 区間の pin だけが呼ぶ（bd s2-07l.87・flip-check の RED を src 配置で作る）")
)]
pub fn shape(summary: &str) -> String {
    let (prefix, facts) = match summary.strip_prefix(SUMMARY_PREFIX) {
        Some(rest) => (SUMMARY_PREFIX, rest),
        None => ("", summary),
    };
    let veiled = facts
        .split(' ')
        .map(|token| match token.split_once('=') {
            Some((tag, value)) => format!("{tag}={}", veil(value)),
            None => veil(token),
        })
        .collect::<Vec<String>>()
        .join(" ");
    format!("{prefix}{veiled}")
}

/// 英数字の連なりを 1 つの `<v>` に畳む（区切り文字は残す）。
fn veil(value: &str) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            if !in_run {
                out.push_str("<v>");
            }
            in_run = true;
        } else {
            out.push(ch);
            in_run = false;
        }
    }
    out
}

/// workspace の形そのものが読めないときの Report。
fn blocked(reason: &str) -> Report {
    Report {
        violations: vec![format!("layout: {reason}")],
        summary: String::new(),
    }
}

/// tag ごとの測定結果を 1 つの Report へ畳む。
fn fold(measured: Vec<Measured>) -> Report {
    let mut violations = Vec::new();
    let mut facts = Vec::new();
    for item in measured {
        violations.extend(item.violations);
        facts.push(item.fact);
    }
    Report {
        summary: format!("{SUMMARY_PREFIX} {}", facts.join(" ")),
        violations,
    }
}

/// 測れなかった tag を違反として立てる。
pub(crate) fn failed(tag: &str, reason: &str) -> Measured {
    Measured {
        fact: format!("{tag}=?"),
        violations: vec![format!("{tag}: {reason}")],
    }
}

/// file を読む。読めない理由はそのまま違反本文に出せる形にする。
pub(crate) fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{} を読めない: {err}", path.display()))
}

/// `crates/*/src` 配下の `.rs` を path 順に読む。
fn collect_rs_files(root: &Path) -> Result<Vec<SourceFile>, String> {
    let mut files = Vec::new();
    for entry in read_dir_sorted(&root.join("crates"))? {
        let src = entry.join("src");
        if src.is_dir() {
            collect_rs_under(&src, &mut files)?;
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// dir を再帰して `.rs` を集める。
fn collect_rs_under(dir: &Path, out: &mut Vec<SourceFile>) -> Result<(), String> {
    for entry in read_dir_sorted(dir)? {
        if entry.is_dir() {
            collect_rs_under(&entry, out)?;
        } else if entry.extension().is_some_and(|ext| ext == "rs") {
            out.push(SourceFile {
                text: read_text(&entry)?,
                path: entry,
            });
        }
    }
    Ok(())
}

/// dir の直下 entry を path 順で返す。
fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let listing =
        fs::read_dir(dir).map_err(|err| format!("{} を読めない: {err}", dir.display()))?;
    let mut paths = Vec::new();
    for entry in listing {
        let found = entry.map_err(|err| format!("{} の entry を読めない: {err}", dir.display()))?;
        paths.push(found.path());
    }
    paths.sort();
    Ok(paths)
}

/// root manifest の `[workspace] members`。
fn workspace_members(manifest: &str) -> Vec<String> {
    entries_in(manifest, "workspace")
        .into_iter()
        .find(|(key, _)| *key == "members")
        .map(|(_, value)| string_array(value))
        .unwrap_or_default()
}

/// `[package]` から 1 key を読む。
fn package_field(manifest: &str, key: &str) -> Option<String> {
    entries_in(manifest, "package")
        .into_iter()
        .find(|(found, _)| *found == key)
        .and_then(|(_, value)| quoted(value))
}

/// member のうち task runner 以外を core crate として 1 本だけ選ぶ。
fn find_core_dir(member_dirs: &[PathBuf]) -> Result<PathBuf, String> {
    let mut cores = Vec::new();
    for dir in member_dirs {
        let manifest = read_text(&dir.join("Cargo.toml"))?;
        let Some(name) = package_field(&manifest, "name") else {
            continue;
        };
        if name != RUNNER_PACKAGE {
            cores.push(dir.clone());
        }
    }
    let found = cores.len();
    cores
        .into_iter()
        .next()
        .filter(|_| found == 1)
        .ok_or_else(|| format!("core crate は 1 本のはずが {found} 本"))
}

/// core crate の `name.rs` から NAME の値を取り出す。
fn read_name_const(path: &Path) -> Result<String, String> {
    let src = read_text(path)?;
    src.lines()
        .filter_map(|line| line.trim().strip_prefix(NAME_CONST_PREFIX))
        .find_map(|rest| quoted(rest.trim()))
        .ok_or_else(|| format!("{} に NAME const が無い", path.display()))
}

/// JSON から `"<key>": "<値>"` の値を std だけで抜く（骨格に JSON crate を足さない）。
pub(crate) fn json_string_field(src: &str, key: &str) -> Option<String> {
    let after_key = src.split_once(&format!("\"{key}\""))?.1;
    let after_colon = after_key.split_once(':')?.1;
    let after_open = after_colon.split_once('"')?.1;
    after_open
        .split_once('"')
        .map(|(value, _)| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{check, shape, summary};
    use crate::genmanifest;
    use crate::limits::{ALLOWED_DEPS, MAX_FILE_LINES, REQUIRED_LINTS};
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
    fn write_at(dir: &Path, rel: &str, body: &str) {
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
    }

    /// rules manifest の相対 path。
    const RULES_REL: &str = "rules/manifest.toml";

    /// fixture の rules manifest。`allow` に与えた path が例外行に載る。
    fn rules_manifest(allow: &[&str]) -> String {
        let items = allow
            .iter()
            .map(|path| format!("\"{path}\""))
            .collect::<Vec<String>>()
            .join(", ");
        format!(
            "[[rule]]\nid = \"repo.non_rust_exec_allow\"\nkind = \"RepoNonRustExecAllow\"\n\
             value = [{items}]\nruling = \"fixture\"\nruled_at = \"2026-09-11\"\n"
        )
    }

    /// 健全な擬似 workspace を作り `mutate` で 1 項目だけ壊してから check を回す。
    /// 後始末は assert より前に済ませる。
    fn check_fixture(mutate: impl FnOnce(&Path)) -> Vec<String> {
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
    fn assert_single(violations: &[String], tag: &str) {
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
        assert!(
            violations.is_empty(),
            "自 workspace で違反 0 のはず: {violations:?}"
        );
    }

    /// 判定行の外形の pin（repo root で撃ったときの形）。値は [`shape`] で伏せてある。
    const SUMMARY_PIN: &str = "xtask check: ok core-lines=<v>/<v> file-lines=<v>/<v> \
        test-src-ratio=<v>/<v> name-literal=<v> manifest-name=<v> manifest-version=<v>.<v>.<v> \
        lints-set=<v> lints-optin=<v>/<v> deps-empty=<v> toolchain-pin=<v>.<v>.<v> \
        paths-clean=<v> non-rust-exec=<v>/<v> allow=<v> ci-shell-lines=<v> \
        claude-md-constitution=<v> enum-slices=<v> claude-spawn-points=<v> polarity=<v>/<v>";

    /// git を要する measure の fact（`.git` の無い木では測れない形になり、副 field も出ない）。
    fn is_git_fact(token: &str) -> bool {
        ["paths-clean=", "non-rust-exec=", "allow="]
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
    fn summary_fixture(mutate: impl FnOnce(&Path)) -> String {
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

    /// 上限 +1 行の .rs は file-lines だけで落ち、上限ちょうどは通る。
    #[test]
    fn check_fails_on_oversized_file() {
        let over = check_fixture(|dir| {
            write_at(
                dir,
                &format!("crates/{FIXTURE_CORE}/src/big.rs"),
                &filler_rs(MAX_FILE_LINES + 1),
            );
        });
        assert_single(&over, "file-lines");

        let at_limit = check_fixture(|dir| {
            write_at(
                dir,
                &format!("crates/{FIXTURE_CORE}/src/big.rs"),
                &filler_rs(MAX_FILE_LINES),
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
    #[test]
    fn non_rust_exec_is_unmeasured_when_the_manifest_is_missing() {
        let violations = check_fixture(|dir| {
            let _ = fs::remove_file(dir.join(RULES_REL));
        });
        assert!(
            violations
                .iter()
                .any(|line| line.starts_with("non-rust-exec:") && line.contains("manifest")),
            "manifest を読めない周は違反として名乗る: {violations:?}"
        );
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

}
