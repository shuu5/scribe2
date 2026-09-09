//! `cargo xtask check` が測る閾値と lint 集合の唯一の置き場。
//!
//! 数値を .rs の各所へ散らさないための const 置き場である。rules manifest への
//! 移設は leg 2 の所管なので、本 leg ではここが SSOT でよい。test 側の期待値も
//! ここを参照して機械的に作る（fixture に magic number を書かない）。

/// core crate の `src` 配下 `.rs` の総行数の上限。
pub const MAX_CORE_LINES: usize = 20_000;

/// `crates/*/src` 配下 `.rs` 1 file あたりの物理行数の上限。
pub const MAX_FILE_LINES: usize = 1_500;

/// workspace root の `Cargo.toml` が持つべき lint の 3 つ組
/// `(section, lint 名, level)`。section は `workspace.lints.<section>` の後半。
///
/// 個数は契約に書かない（この配列が唯一の SSOT である）。`print_stdout` /
/// `print_stderr` を deny に据え置くのは、出力層に `#[expect]` を使えなくなる
/// forbid を避けるためである。逆に粒度 lint 3 本は `#[allow]` / `#[expect]`
/// による黙殺を compile error にしたいので forbid とする。
pub const REQUIRED_LINTS: &[(&str, &str, &str)] = &[
    ("rust", "unsafe_code", "forbid"),
    ("rust", "unused_must_use", "deny"),
    ("clippy", "unwrap_used", "deny"),
    ("clippy", "expect_used", "deny"),
    ("clippy", "panic", "deny"),
    ("clippy", "todo", "deny"),
    ("clippy", "unimplemented", "deny"),
    ("clippy", "unreachable", "deny"),
    ("clippy", "exit", "deny"),
    ("clippy", "indexing_slicing", "deny"),
    ("clippy", "dbg_macro", "deny"),
    ("clippy", "print_stdout", "deny"),
    ("clippy", "print_stderr", "deny"),
    ("clippy", "too_many_arguments", "forbid"),
    ("clippy", "too_many_lines", "forbid"),
    ("clippy", "cognitive_complexity", "forbid"),
    ("clippy", "allow_attributes", "deny"),
];

/// tracked file の本文に残してはならない private path 形の needle 集合。
///
/// 集合は 2 形ちょうどである（絶対 home dir の接頭形と、home dir の短縮展開記号 +
/// 区切りの 2 byte 形）。字面をこの file に置くと paths-clean が自分自身を撃つので
/// **実行時に組み立てる**（`concat!` は compile 時に連結するため source の byte 列に
/// needle が現れない）。行頭錨ではなく行中のどこに現れても違反である。
pub const PRIVATE_PATH_MARKS: &[&str] = &[concat!("/", "home", "/"), concat!("~", "/")];

/// `[dependencies]` / `[dev-dependencies]` に在ってよい依存の `(section, dep 名)`。
///
/// crate 名の字面を持たない 2 つ組であるため、同名の dep をどの crate が宣言しても
/// 区別できない（xtask に同名 dev-dep を足しても検出できないのは既知の限界であり、
/// crate 粒度の回復は leg 2 の所管である）。
pub const ALLOWED_DEPS: &[(&str, &str)] = &[("dev-dependencies", "insta")];

#[cfg(test)]
mod tests {
    use super::{MAX_CORE_LINES, MAX_FILE_LINES};
    use crate::toml_lite::{quoted, sections};
    use std::path::PathBuf;

    /// `[[rule]]` の section header を [`sections`] が返す字面。
    ///
    /// `sections` は `[` を 1 つだけ剥がすので、array-of-tables は `[rule` になる。
    const RULE_HEADER: &str = "[rule";

    /// manifest の本文（workspace root は この crate の 2 つ上）。
    fn manifest_text() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("manifest.toml");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()))
    }

    /// 行 id を持つ `[[rule]]` の `value` を整数で引く。
    fn int_value(text: &str, id: &str) -> Option<u64> {
        for (header, pairs) in sections(text) {
            if header != RULE_HEADER {
                continue;
            }
            let found = pairs
                .iter()
                .find(|(key, _)| *key == "id")
                .and_then(|(_, value)| quoted(value));
            if found.as_deref() != Some(id) {
                continue;
            }
            return pairs
                .iter()
                .find(|(key, _)| *key == "value")
                .and_then(|(_, value)| value.trim().parse::<u64>().ok());
        }
        None
    }

    /// `limits.rs` の const と manifest の行が同じ値である（憲法 C14.2 の最小形）。
    ///
    /// const の manifest 移設は後続の便なので、いまは 2 面の写しの一致を歯で守る。
    #[test]
    fn limits_match_rules_manifest() {
        let text = manifest_text();
        assert_eq!(
            int_value(&text, "R-C4-1"),
            Some(MAX_CORE_LINES as u64),
            "R-C4-1 と MAX_CORE_LINES"
        );
        assert_eq!(
            int_value(&text, "R-C4-2"),
            Some(MAX_FILE_LINES as u64),
            "R-C4-2 と MAX_FILE_LINES"
        );
    }
}
