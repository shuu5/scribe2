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

/// core crate の plugin の生成 dir の定数を宣言する行の前置き（設計 consumer-sync.md §17 形 1）。
const PLUGIN_DIR_CONST_PREFIX: &str = "pub const PLUGIN_DIR: &str =";

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
        let name = read_str_const(&core_dir.join("src").join("name.rs"), NAME_CONST_PREFIX, "NAME")?;
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

    /// plugin の実体の生成 dir（root 相対・core crate の `name.rs` の `PLUGIN_DIR` が正本・設計 consumer-sync.md §17 形 1）。
    ///
    /// 生成 dir を含む path を結ぶのは呼び手である（`genmanifest` の 3 定数は root 相対の末尾の字面のまま）。field に
    /// 持たず都度読むのは、`Layout` を literal で組む measure の歯を書き換えないため（[`SourceFile::lines`] と同じ理由）。
    /// 定数の無い core は `Err` で断る（推測で埋めない）。
    pub fn plugin_dir(&self) -> Result<String, String> {
        read_str_const(&self.core_dir.join("src").join("name.rs"), PLUGIN_DIR_CONST_PREFIX, "PLUGIN_DIR")
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
    /// 幅 `width` で正規化した行数（[`weighted_lines`]・末尾改行の有無で差を出さない）。
    ///
    /// 幅は field に持たず引数で受ける: `SourceFile` は measure の歯が literal で組むので（env_reads /
    /// spawn_points）、field を足すと write-set の外の歯まで書き換わる（`s2-07l.254`）。
    pub(crate) fn lines(&self, width: usize) -> usize {
        weighted_lines(&self.text, width)
    }

    /// 最初に現れる行頭 `#[cfg(test)]` から file 末尾までを test 行、残りを src 行と数える（幅で正規化）。
    pub(crate) fn split_test_src(&self, width: usize) -> (usize, usize) {
        let lines: Vec<&str> = self.text.lines().collect();
        let at = lines.iter().position(|line| line.starts_with(TEST_MOD_MARK)).unwrap_or(lines.len());
        let (src, test) = lines.split_at(at);
        (sum_weights(test, width), sum_weights(src, width))
    }
}

/// 行の数え方（設計 rules-manifest.md §4・`R-C4.line-width`）: 各行を `max(1, ceil(文字数 ÷ width))` と数えた合計。
///
/// 1 行に詰め込んでも行数が下がらない形。文字数は `chars` の数・末尾改行の有無で差を出さない・`width = 0` は 1 行 1
/// と数える（0 除算の縮退）。core の `pipe::closure::weighted_lines` と同じ式で、同じ fixture の歯が一致を守る。
pub(crate) fn weighted_lines(text: &str, width: usize) -> usize {
    sum_weights(&text.lines().collect::<Vec<&str>>(), width)
}

/// 行の列の重みの合計（[`weighted_lines`] の式）。
fn sum_weights(lines: &[&str], width: usize) -> usize {
    lines
        .iter()
        .map(|line| match width {
            0 => 1,
            _ => line.chars().count().div_ceil(width).max(1),
        })
        .sum()
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

/// core crate の `name.rs` から `prefix` で宣言された文字列 const（`label`）の値を取り出す。
fn read_str_const(path: &Path, prefix: &str, label: &str) -> Result<String, String> {
    let src = read_text(path)?;
    src.lines()
        .filter_map(|line| line.trim().strip_prefix(prefix))
        .find_map(|rest| quoted(rest.trim()))
        .ok_or_else(|| format!("{} に {label} const が無い", path.display()))
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
    use super::{weighted_lines, SourceFile};
    use std::path::PathBuf;

    /// 幅 10 の fixture と期待値。**core の `pipe::closure` の歯と同じ字面・同じ値**（2 crate の式の一致を守る）。
    const WIDTH_FIXTURES: &[(&str, usize)] = &[
        ("ab\ncd\nef\n", 3),
        ("abcdefghijklmnopqrstuvwxy\n", 3),
        ("0123456789\n", 1),
        ("\n", 1),
        ("ab\ncd\nef", 3),
        ("abcdefghijklmnopqrstuvwxy", 3),
        ("あいうえおかきくけこさ\n", 2),
    ];

    /// 幅 10: 短い 3 行 = 3・25 字の 1 行 = 3・10 字ちょうど = 1・空行 = 1・末尾改行の有無で同値・文字数は byte でなく
    /// 文字で数える（11 字の和文 = 2）。
    #[test]
    fn sizes_width_weighs_each_line_by_ceil_of_chars_over_width() {
        for (text, want) in WIDTH_FIXTURES {
            assert_eq!(weighted_lines(text, 10), *want, "{text:?}");
        }
        assert_eq!(weighted_lines("ab\ncd\nef", 10), weighted_lines("ab\ncd\nef\n", 10), "末尾改行の有無で差を出さない");
    }

    /// 幅 0 は 1 行 1 と数える（0 除算の縮退）・幅が行より広ければ改行の数と同じ。
    #[test]
    fn sizes_width_zero_and_wide_width_count_newlines() {
        assert_eq!(weighted_lines("abcdefghijklmnopqrstuvwxy\nab\n", 0), 2, "幅 0");
        assert_eq!(weighted_lines("abcdefghijklmnopqrstuvwxy\nab\n", 120), 2, "幅 120");
    }

    /// `SourceFile` の 2 つの数え方も同じ式: 詰め込んだ src 行は src 側に、詰め込んだ test 行は test 側に重く載る。
    #[test]
    fn sizes_width_split_weighs_both_sides() {
        let file = SourceFile {
            path: PathBuf::from("x.rs"),
            text: "fn a() {}\nabcdefghijklmnopqrstuvwxy\n#[cfg(test)]\nmod t {}\nabcdefghijklmnopqrstuvwxy\n".to_owned(),
        };
        // 12 字の `#[cfg(test)]` 自身も幅 10 では 2 行に数える。
        assert_eq!(file.split_test_src(10), (6, 4), "(test, src) = (2 + 1 + 3, 1 + 3)");
        assert_eq!(file.split_test_src(120), (3, 2), "幅が広ければ改行の数");
        assert_eq!(file.lines(10), 10, "file の行数は 2 側の和");
        let bare = SourceFile { path: PathBuf::from("y.rs"), text: "abcdefghijklmnopqrstuvwxy\n".to_owned() };
        assert_eq!(bare.split_test_src(10), (0, 3), "test 区間の無い file は全部 src");
    }
}
