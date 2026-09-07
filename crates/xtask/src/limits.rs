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
