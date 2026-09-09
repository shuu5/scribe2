//! `cargo xtask check` の本体。憲法 C4 の歯（行数 / 比 / manifest parity / lints）を測る。
//!
//! 違反は 1 件 1 行 `<tag>: <本文>` で返し、1 件以上なら CLI 面が rc 1 を返す。
//! 違反 0 のときだけ各 tag の実測値入りサマリを 1 行出す（失敗時にサマリは出さない）。
//! 測る tag は core-lines / file-lines / test-src-ratio / manifest-name /
//! manifest-version / lints-set / lints-optin / name-literal / deps-empty /
//! toolchain-pin / paths-clean の 11 本である。

use crate::genmanifest::MANIFEST_REL;
use crate::limits::{ALLOWED_DEPS, MAX_CORE_LINES, MAX_FILE_LINES, PRIVATE_PATH_MARKS, REQUIRED_LINTS};
use crate::toml_lite::{entries_in, lint_level, quoted, string_array};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// task runner 自身の package 名。core crate はこれ以外の member として発見する。
const RUNNER_PACKAGE: &str = "xtask";

/// core crate の名前定数を宣言する行の前置き。
const NAME_CONST_PREFIX: &str = "pub const NAME: &str =";

/// in-module test の始まりを示す行頭の印（■C の測定定義）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

/// paths-clean の母集団から外す tracked file。**ただ 1 本で固定**である
/// （bd が生成し private path 形の例をコメントに持つため）。
const PATHS_CLEAN_SKIP: &str = ".beads/config.yaml";

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
    fn core_package_field(&self, key: &str) -> Result<String, String> {
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
struct Measured {
    /// サマリ行に載せる `tag=値` の断片。
    fact: String,
    /// 違反行（合格なら空）。
    violations: Vec<String>,
}

/// 読み込んだ `.rs` 1 本。
struct SourceFile {
    /// 絶対 path。
    path: PathBuf,
    /// 本文。
    text: String,
}

impl SourceFile {
    /// 物理行数（末尾改行の有無で差を出さない）。
    fn lines(&self) -> usize {
        self.text.lines().count()
    }

    /// 最初に現れる行頭 `#[cfg(test)]` から file 末尾までを test 行、残りを src 行と数える。
    fn split_test_src(&self) -> (usize, usize) {
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
        measure_core_lines(&layout, &files),
        measure_file_lines(&files),
        measure_test_src_ratio(&files),
        measure_name_literal(&layout, &files),
    ];
    measured.extend(measure_manifests(&layout));
    measured.extend(measure_lints(&layout));
    measured.push(measure_deps_empty(&layout));
    measured.push(measure_toolchain_pin(&layout));
    measured.push(measure_paths_clean(&layout));
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
        summary: format!("xtask check: ok {}", facts.join(" ")),
        violations,
    }
}

/// 測れなかった tag を違反として立てる。
fn failed(tag: &str, reason: &str) -> Measured {
    Measured {
        fact: format!("{tag}=?"),
        violations: vec![format!("{tag}: {reason}")],
    }
}

/// core crate の `src` 配下の総行数（core-lines）。
fn measure_core_lines(layout: &Layout, files: &[SourceFile]) -> Measured {
    let core_src = layout.core_dir.join("src");
    let total: usize = files
        .iter()
        .filter(|file| file.path.starts_with(&core_src))
        .map(SourceFile::lines)
        .sum();
    let mut violations = Vec::new();
    if total > MAX_CORE_LINES {
        violations.push(format!(
            "core-lines: core crate の src が {total} 行で上限 {MAX_CORE_LINES} 行を超える"
        ));
    }
    Measured {
        fact: format!("core-lines={total}/{MAX_CORE_LINES}"),
        violations,
    }
}

/// `crates/*/src` 配下 `.rs` の 1 file 行数（file-lines）。
fn measure_file_lines(files: &[SourceFile]) -> Measured {
    let mut violations = Vec::new();
    let mut worst = 0;
    for file in files {
        let lines = file.lines();
        worst = worst.max(lines);
        if lines > MAX_FILE_LINES {
            violations.push(format!(
                "file-lines: {} が {lines} 行で上限 {MAX_FILE_LINES} 行を超える",
                file.path.display()
            ));
        }
    }
    Measured {
        fact: format!("file-lines={worst}/{MAX_FILE_LINES}"),
        violations,
    }
}

/// test 行と src 行の比（test-src-ratio）。整数比較で `Σtest <= Σsrc` を見る。
fn measure_test_src_ratio(files: &[SourceFile]) -> Measured {
    let mut test_total = 0;
    let mut src_total = 0;
    for file in files {
        let (test, src) = file.split_test_src();
        test_total += test;
        src_total += src;
    }
    let mut violations = Vec::new();
    if src_total > 0 && test_total > src_total {
        violations.push(format!(
            "test-src-ratio: test {test_total} 行 > src {src_total} 行（比の上限は 1.0）"
        ));
    }
    Measured {
        fact: format!("test-src-ratio={test_total}/{src_total}"),
        violations,
    }
}

/// NAME の字面を持つ `.rs` が core crate の `name.rs` ただ 1 本であること（name-literal）。
fn measure_name_literal(layout: &Layout, files: &[SourceFile]) -> Measured {
    let needle = format!("\"{}\"", layout.name);
    let allowed = layout.core_dir.join("src").join("name.rs");
    let holders: Vec<&PathBuf> = files
        .iter()
        .filter(|file| file.text.contains(&needle))
        .map(|file| &file.path)
        .collect();
    let mut violations = Vec::new();
    for path in &holders {
        if **path != allowed {
            violations.push(format!(
                "name-literal: {} が NAME の字面を持つ（name.rs だけが持てる）",
                path.display()
            ));
        }
    }
    if !holders.iter().any(|path| **path == allowed) {
        violations.push(format!(
            "name-literal: {} に NAME の字面が無い",
            allowed.display()
        ));
    }
    Measured {
        fact: format!("name-literal={}", holders.len()),
        violations,
    }
}

/// plugin manifest と core crate の突き合わせ（manifest-name / manifest-version）。
fn measure_manifests(layout: &Layout) -> Vec<Measured> {
    let plugin = match read_text(&layout.root.join(MANIFEST_REL)) {
        Ok(text) => text,
        Err(reason) => {
            return vec![
                failed("manifest-name", &reason),
                failed("manifest-version", &reason),
            ]
        }
    };
    vec![
        agreement(
            "manifest-name",
            &[
                ("plugin.json", json_string_field(&plugin, "name")),
                ("Cargo.toml", layout.core_package_field("name").ok()),
                ("name.rs", Some(layout.name.clone())),
            ],
        ),
        agreement(
            "manifest-version",
            &[
                ("plugin.json", json_string_field(&plugin, "version")),
                ("Cargo.toml", layout.core_version().ok()),
            ],
        ),
    ]
}

/// 与えた出所の値がすべて同一であることを測る。
fn agreement(tag: &str, sources: &[(&str, Option<String>)]) -> Measured {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut missing = Vec::new();
    for (label, value) in sources {
        match value {
            Some(found) => {
                seen.insert(found.clone());
            }
            None => missing.push(*label),
        }
    }
    let listed: Vec<String> = seen.iter().cloned().collect();
    let fact = format!("{tag}={}", listed.join("|"));
    if !missing.is_empty() {
        let reason = format!("{} から値を読めない", missing.join(", "));
        return Measured {
            fact,
            violations: vec![format!("{tag}: {reason}")],
        };
    }
    if listed.len() > 1 {
        return Measured {
            fact,
            violations: vec![format!("{tag}: {}", listed.join(" != "))],
        };
    }
    Measured {
        fact,
        violations: Vec::new(),
    }
}

/// root manifest の lint 集合（lints-set）と member 側 opt-in（lints-optin）。
fn measure_lints(layout: &Layout) -> Vec<Measured> {
    let root_manifest = match read_text(&layout.root.join("Cargo.toml")) {
        Ok(text) => text,
        Err(reason) => {
            return vec![failed("lints-set", &reason), failed("lints-optin", &reason)]
        }
    };
    vec![measure_lints_set(&root_manifest), measure_lints_optin(layout)]
}

/// root manifest が宣言している `(section, lint, level)` の 3 つ組集合。
fn declared_lints(manifest: &str) -> BTreeSet<(String, String, String)> {
    let mut found = BTreeSet::new();
    for section in ["rust", "clippy"] {
        for (key, value) in entries_in(manifest, &format!("workspace.lints.{section}")) {
            if let Some(level) = lint_level(value) {
                found.insert((section.to_owned(), key.to_owned(), level));
            }
        }
    }
    found
}

/// root manifest の lint 集合が [`REQUIRED_LINTS`] と一致すること（lints-set）。
fn measure_lints_set(manifest: &str) -> Measured {
    let declared = declared_lints(manifest);
    let required: BTreeSet<(String, String, String)> = REQUIRED_LINTS
        .iter()
        .map(|(section, lint, level)| {
            ((*section).to_owned(), (*lint).to_owned(), (*level).to_owned())
        })
        .collect();
    let mut violations = Vec::new();
    for (section, lint, level) in required.difference(&declared) {
        violations.push(format!(
            "lints-set: {section}.{lint} = \"{level}\" が root Cargo.toml に無い"
        ));
    }
    for (section, lint, level) in declared.difference(&required) {
        violations.push(format!(
            "lints-set: {section}.{lint} = \"{level}\" は REQUIRED_LINTS に無い"
        ));
    }
    Measured {
        fact: format!("lints-set={}", declared.len()),
        violations,
    }
}

/// 各 member が `[lints] workspace = true` を持つこと（lints-optin）。
///
/// これが無いと `[workspace.lints]` は member に一切適用されず、clippy も
/// nextest も全部緑のまま歯が死ぬ。
fn measure_lints_optin(layout: &Layout) -> Measured {
    let mut violations = Vec::new();
    let mut opted = 0;
    for dir in &layout.member_dirs {
        let path = dir.join("Cargo.toml");
        match read_text(&path) {
            Ok(text) if has_workspace_lints(&text) => opted += 1,
            Ok(_) => violations.push(format!(
                "lints-optin: {} に [lints] workspace = true が無い（workspace.lints が不活性になる）",
                path.display()
            )),
            Err(reason) => violations.push(format!("lints-optin: {reason}")),
        }
    }
    Measured {
        fact: format!("lints-optin={opted}/{}", layout.member_dirs.len()),
        violations,
    }
}

/// member manifest が `[lints] workspace = true` を持つか。
fn has_workspace_lints(manifest: &str) -> bool {
    entries_in(manifest, "lints")
        .iter()
        .any(|(key, value)| *key == "workspace" && *value == "true")
}

/// root と全 member の直接依存が [`ALLOWED_DEPS`] の内側であること（deps-empty）。
///
/// 中身は allowlist だが measure tag の名は ADR-0002 §2.4 が凍結しているので
/// `deps-empty` に据え置く。
fn measure_deps_empty(layout: &Layout) -> Measured {
    let mut manifests = vec![layout.root.join("Cargo.toml")];
    manifests.extend(layout.member_dirs.iter().map(|dir| dir.join("Cargo.toml")));
    let mut violations = Vec::new();
    for path in &manifests {
        match read_text(path) {
            Ok(text) => violations.extend(declared_deps(&text, path)),
            Err(reason) => violations.push(format!("deps-empty: {reason}")),
        }
    }
    Measured {
        fact: format!("deps-empty={}", manifests.len()),
        violations,
    }
}

/// 1 つの manifest が宣言している allowlist 外の直接依存を違反行に写す。
fn declared_deps(manifest: &str, path: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for section in ["dependencies", "dev-dependencies"] {
        for (key, _) in entries_in(manifest, section) {
            if is_allowed_dep(section, key) {
                continue;
            }
            found.push(format!(
                "deps-empty: {} の [{section}] に {key} が在る（allowlist 外の直接依存）",
                path.display()
            ));
        }
    }
    found
}

/// `(section, dep 名)` が [`ALLOWED_DEPS`] に在るか。
fn is_allowed_dep(section: &str, dep: &str) -> bool {
    ALLOWED_DEPS
        .iter()
        .any(|(allowed_section, allowed_dep)| *allowed_section == section && *allowed_dep == dep)
}

/// channel が浮動 channel の語を含むならその語を返す。
fn floating_word(channel: &str) -> Option<&'static str> {
    ["stable", "beta", "nightly"]
        .into_iter()
        .find(|word| channel.contains(word))
}

/// channel が版番号の字面（`major.minor.patch`・任意で `-<target-triple>` 付き）か。
///
/// 3 要素を要求するのは、`1.98` のような短い形が rustup では 1.98.x の最新へ
/// **浮動解決**され、■H1 が要求する「host に既に導入済みの toolchain 名」に
/// ならないためである。`my-custom` のような custom toolchain 名もここで落ちる。
fn is_version_literal(channel: &str) -> bool {
    let core = channel.split_once('-').map_or(channel, |(head, _)| head);
    let mut parts = 0;
    for part in core.split('.') {
        if part.is_empty() || !part.chars().all(|digit| digit.is_ascii_digit()) {
            return false;
        }
        parts += 1;
    }
    parts == 3
}

/// `rust-toolchain.toml` の channel が版番号の字面であり、かつ
/// stable / beta / nightly の語を含まないこと（toolchain-pin）。
///
/// 負の語検査だけだと `1.98` や `my-custom` が素通りするので、正の字面検査
/// （[`is_version_literal`]）と合接で測る。違反は多くとも 1 件に畳む。
fn measure_toolchain_pin(layout: &Layout) -> Measured {
    let path = layout.root.join("rust-toolchain.toml");
    let channel = read_text(&path).ok().and_then(|text| {
        entries_in(&text, "toolchain")
            .into_iter()
            .find(|(key, _)| *key == "channel")
            .and_then(|(_, value)| quoted(value))
    });
    let Some(channel) = channel else {
        return failed(
            "toolchain-pin",
            &format!("{} の channel を読めない", path.display()),
        );
    };
    let fact = format!("toolchain-pin={channel}");
    let violation = match floating_word(&channel) {
        Some(word) => Some(format!(
            "toolchain-pin: channel \"{channel}\" が {word} を含む（版番号で固定する）"
        )),
        None if !is_version_literal(&channel) => Some(format!(
            "toolchain-pin: channel \"{channel}\" が版番号の字面でない（major.minor.patch で固定する）"
        )),
        None => None,
    };
    Measured {
        fact,
        violations: violation.into_iter().collect(),
    }
}

/// paths-clean の母集団（tracked file の repo 相対 path）と root 一致判定の結果。
enum Tracked {
    /// `<root>` が repo root であり tracked file を列挙できた。
    Listed(Vec<String>),
    /// `<root>` が repo root でない（flip-check の base tree はこの枝に落ちる）。
    NotRepoRoot,
    /// 測れなかった（fail-closed で `n/a` へ落とさない）。
    Unmeasurable(String),
}

/// `git -C <dir> <args...>` を撃ち rc 0 のときだけ stdout を返す。
fn git_stdout(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let shown = args.join(" ");
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|err| format!("git {shown} を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {shown} が rc≠0: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// path を canonicalize する。失敗は path と io error を逐語で載せて fail-closed。
fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|err| format!("{} を canonicalize できない: {err}", path.display()))
}

/// `<root>` が git repo の toplevel そのものか。
///
/// 素の [`PathBuf`] 比較では `CARGO_MANIFEST_DIR/../..` 形の root が字面で一致せず
/// paths-clean が恒久 `n/a` に化けるので、両側を canonicalize して比べる。
fn root_is_repo_root(root: &Path) -> Result<bool, String> {
    let raw = git_stdout(root, &["rev-parse", "--show-toplevel"])?;
    let shown = String::from_utf8_lossy(&raw).trim().to_owned();
    if shown.is_empty() {
        return Err("git rev-parse --show-toplevel が空を返した".to_owned());
    }
    Ok(canonical(Path::new(&shown))? == canonical(root)?)
}

/// NUL 区切りの出力を path の列へ分ける。
fn split_nul(raw: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(raw)
        .split('\0')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// paths-clean の母集団を data 化して固定する（cwd を直読みしない）。
fn tracked_files(root: &Path) -> Tracked {
    match root_is_repo_root(root) {
        Err(reason) => Tracked::Unmeasurable(reason),
        Ok(false) => Tracked::NotRepoRoot,
        Ok(true) => match git_stdout(root, &["ls-files", "-z"]) {
            Err(reason) => Tracked::Unmeasurable(format!("tracked file を列挙できない: {reason}")),
            Ok(raw) => Tracked::Listed(split_nul(&raw)),
        },
    }
}

/// tracked file の本文に private path 形が残っていないこと（paths-clean）。
fn measure_paths_clean(layout: &Layout) -> Measured {
    match tracked_files(&layout.root) {
        Tracked::Unmeasurable(reason) => failed("paths-clean", &reason),
        Tracked::NotRepoRoot => Measured {
            fact: "paths-clean=n/a(not-a-repo-root)".to_owned(),
            violations: Vec::new(),
        },
        Tracked::Listed(rels) if rels.is_empty() => {
            failed("paths-clean", "tracked file が 0 件である")
        }
        Tracked::Listed(rels) => scan_private_paths(&layout.root, &rels),
    }
}

/// tracked file を 1 本ずつ走査する。読めない file（UTF-8 でない・作業木から消えている
/// 等）は skip して `scanned` にも数えない。母集団は index だが本文は作業木から読むので、
/// index にしか在らない blob（tracked symlink の link target を含む）はこの面では測らない。
fn scan_private_paths(root: &Path, rels: &[String]) -> Measured {
    let mut violations = Vec::new();
    let mut scanned = 0;
    for rel in rels {
        if rel == PATHS_CLEAN_SKIP {
            continue;
        }
        let Ok(text) = fs::read_to_string(root.join(rel)) else {
            continue;
        };
        scanned += 1;
        violations.extend(private_path_hits(rel, &text));
    }
    Measured {
        fact: format!("paths-clean={scanned}"),
        violations,
    }
}

/// 1 file の本文から private path 形の在る行を違反行へ写す（行中のどこでも違反）。
fn private_path_hits(rel: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| PRIVATE_PATH_MARKS.iter().any(|mark| line.contains(mark)))
        .map(|(index, _)| format!("paths-clean: {rel}:{} に private path 形が在る", index + 1))
        .collect()
}

/// file を読む。読めない理由はそのまま違反本文に出せる形にする。
fn read_text(path: &Path) -> Result<String, String> {
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
fn json_string_field(src: &str, key: &str) -> Option<String> {
    let after_key = src.split_once(&format!("\"{key}\""))?.1;
    let after_colon = after_key.split_once(':')?.1;
    let after_open = after_colon.split_once('"')?.1;
    after_open
        .split_once('"')
        .map(|(value, _)| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{check, summary};
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

    /// member manifest の `section` へ依存 1 本を足した本文。
    fn with_dep(name: &str, section: &str, dep: &str) -> String {
        let mut text = member_manifest(name, true);
        let anchor = format!("[{section}]\n");
        match text.rfind(&anchor) {
            Some(at) => {
                text.insert_str(at + anchor.len(), &format!("{dep} = \"1\"\n"));
                text
            }
            None => format!("{text}\n[{section}]\n{dep} = \"1\"\n"),
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
                &with_dep(FIXTURE_CORE, section, dep),
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
                &with_dep(FIXTURE_CORE, section, "not-in-allowlist"),
            );
        });
        assert_single(&outside, "deps-empty");
    }

    /// `../..` 形の root（`git rev-parse --show-toplevel` の出力と字面では一致しない）
    /// でも paths-clean が走査本数を数える。素の `==` 比較の実装はここで落ちる。
    #[test]
    fn check_paths_clean_scans_noncanonical_root() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let line = summary(&root);
        let tail = line
            .split_once("paths-clean=")
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        let scanned: usize = digits
            .parse()
            .unwrap_or_else(|err| panic!("paths-clean が 10 進整数でない（{err}）: {line}"));
        assert!(scanned >= 1, "走査した tracked file 数は 1 以上のはず: {line}");
    }
}
