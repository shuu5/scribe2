//! `cargo xtask check` の本体。憲法 C4 の歯（行数 / 比 / manifest parity / lints）を測る。
//!
//! 違反は 1 件 1 行 `<tag>: <本文>` で返し、1 件以上なら CLI 面が rc 1 を返す。
//! 違反 0 のときだけ各 tag の実測値入りサマリを 1 行出す（失敗時にサマリは出さない）。
//! 測る tag は core-lines / file-lines / test-src-ratio / manifest-name /
//! manifest-version / lints-set / lints-optin / name-literal / deps-empty /
//! toolchain-pin / paths-clean の 11 本である。

use crate::genmanifest::MANIFEST_REL;
use crate::limits::{ALLOWED_DEPS, MAX_CORE_LINES, MAX_FILE_LINES, REQUIRED_LINTS};
use crate::toml_lite::{entries_in, lint_level, quoted, sections, string_array};
use std::collections::BTreeSet;
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
/// test-src-ratio がそこから下を丸ごと test 区間と数えるからである（実測: 1414/7933
/// → 2165/7024）。
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
pub(crate) struct Measured {
    /// サマリ行に載せる `tag=値` の断片。
    pub(crate) fact: String,
    /// 違反行（合格なら空）。
    pub(crate) violations: Vec<String>,
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
    measured.push(crate::paths_clean::measure(&layout));
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
pub(crate) fn failed(tag: &str, reason: &str) -> Measured {
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
///
/// section 名の完全一致では足りない。`[dependencies.<name>]` の入れ子形、
/// `[build-dependencies]`、`[target.'cfg(unix)'.dependencies]`、
/// `[workspace.dependencies]` のいずれも直接依存を 1 本増やすからである。
fn declared_deps(manifest: &str, path: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for (header, pairs) in sections(manifest) {
        let Some((section, nested)) = dep_section(header) else {
            continue;
        };
        match nested {
            Some(dep) => {
                let renamed = pairs.iter().any(|(key, _)| *key == "package");
                found.extend(dep_violation(path, header, section, dep, renamed));
            }
            None => {
                for (key, value) in pairs {
                    found.extend(dep_violation(path, header, section, key, renames_package(value)));
                }
            }
        }
    }
    found
}

/// 直接依存 1 本を測り、allowlist の外なら違反行を返す。
///
/// `package =` で別 crate へ改名した entry は key 名が allowlist に在っても違反である
/// （さもないと `insta = { package = "other" }` が allowlist を素通りする）。
fn dep_violation(
    path: &Path,
    header: &str,
    section: &str,
    dep: &str,
    renamed: bool,
) -> Option<String> {
    if !renamed && is_allowed_dep(section, dep) {
        return None;
    }
    let reason = if renamed {
        "package = で別 crate へ改名している（allowlist は key 名では通さない）"
    } else {
        "allowlist 外の直接依存"
    };
    Some(format!(
        "deps-empty: {} の [{header}] に {dep} が在る（{reason}）",
        path.display()
    ))
}

/// 直接依存を宣言しうる section の base 名。`workspace.` / `target.<spec>.` の前置は
/// 剥がしてから照合する。
const DEP_SECTION_BASES: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

/// section header が依存 section なら `(base 名, `[<base>.<name>]` 形の dep 名)`。
fn dep_section(header: &str) -> Option<(&'static str, Option<&str>)> {
    let scoped = strip_target_scope(header.strip_prefix("workspace.").unwrap_or(header));
    for base in DEP_SECTION_BASES {
        if scoped == *base {
            return Some((base, None));
        }
        let nested = scoped
            .strip_prefix(*base)
            .and_then(|rest| rest.strip_prefix('.'))
            .filter(|dep| !dep.is_empty() && !dep.contains('.'));
        if let Some(dep) = nested {
            return Some((base, Some(dep)));
        }
    }
    None
}

/// `target.<spec>.` の前置を剥がす。`<spec>` は quote 内に `.` を含みうるので、
/// dot 分割ではなく base 名の直前の `.` を探して切る。
fn strip_target_scope(header: &str) -> &str {
    let Some(after) = header.strip_prefix("target.") else {
        return header;
    };
    for base in DEP_SECTION_BASES {
        if let Some(at) = after.find(&format!(".{base}")) {
            return after.get(at + 1..).unwrap_or(after);
        }
    }
    after
}

/// inline table 形の dep 値が `package = ` による改名を持つか。
fn renames_package(value: &str) -> bool {
    let Some(inner) = value
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return false;
    };
    inner
        .split(',')
        .filter_map(|part| part.split_once('='))
        .any(|(key, _)| key.trim() == "package")
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
}
