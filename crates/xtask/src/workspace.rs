//! workspace の走査。root manifest から member を発見し、`crates/*/src` 配下の `.rs` を読む。
//!
//! `check.rs` から**挙動不変で切り出した**（`s2-07l.257`・憲法 C4）。[`Layout`] / [`SourceFile`]
//! の定義もここに在り、外の呼び手が使う `crate::check::{Layout, SourceFile, read_text,
//! json_string_field}` の path は `check` の再 export で保つ（measure の各 module は触らない）。

use crate::toml_lite::{entries_in, quoted, string_array};
use std::fs;
use std::path::{Path, PathBuf};

/// task runner 自身の package 名。core crate はこれ以外の member として発見する。
const RUNNER_PACKAGE: &str = "xtask";

/// core crate の名前定数を宣言する行の前置き。
const NAME_CONST_PREFIX: &str = "pub const NAME: &str =";

/// in-module test の始まりを示す行頭の印（■C の測定定義）。
const TEST_MOD_MARK: &str = "#[cfg(test)]";

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

/// file を読む。読めない理由はそのまま違反本文に出せる形にする。
pub(crate) fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{} を読めない: {err}", path.display()))
}

/// `crates/*/src` 配下の `.rs` を path 順に読む。
pub(crate) fn collect_rs_files(root: &Path) -> Result<Vec<SourceFile>, String> {
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
pub(crate) fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
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
