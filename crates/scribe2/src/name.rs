//! この器の名前を 1 箇所に集約する（器 SPEC §7）。
//!
//! repo 内で名前の字面を持つ `.rs` はこの file **ただ 1 本**である。
//! 正式名が決まったときに変えるのはここの 1 行だけでよい、という状態を保つ。
//! この不変条件は `cargo xtask check` の `name-literal` が機械で守る。

/// この器の名前。plugin 名・CLI 名・marker の中身はすべてここから導出する。
pub const NAME: &str = "scribe2";

/// plugin の実体の生成 dir（repo root 相対・設計 consumer-sync.md §17・ADR-0038 OPT1）。marketplace の `source`・席の起動行の
/// 1 つ目の `--plugin-dir`・便の worktree への写し・導入先の読み込み元はすべてここから解く（xtask は tracked のこの行を読む）。
pub const PLUGIN_DIR: &str = "plugin";

/// build 元 commit（`<sha12>` / `<sha12>+dirty` / `unknown`・設計 consumer-sync.md §2・carry-prep.md §2 の 3）。
/// `build.rs` が cargo の出力 dir の 1 file に書いた値を compile 時に読む（env の名を持たない）。version の行・
/// binary の世代の記録・consumer の drift はすべてこの const を読む。
pub const BUILD_COMMIT: &str = include_str!(concat!(env!("OUT_DIR"), "/build_commit"));

#[cfg(test)]
mod tests {
    use super::BUILD_COMMIT;

    /// 小文字の 16 進 12 桁か（build.rs の `is_sha12` と同じ形）。
    fn is_sha12(text: &str) -> bool {
        text.len() == 12 && text.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
    }

    /// 焼いた値は `<sha12>` / `<sha12>+dirty` / `unknown` のどれか（空・改行つき・他の形は無い）。
    #[test]
    fn build_commit_is_one_of_the_three_forms() {
        let sha = BUILD_COMMIT.strip_suffix("+dirty").unwrap_or(BUILD_COMMIT);
        assert!(BUILD_COMMIT == "unknown" || is_sha12(sha), "3 形のどれでもない: {BUILD_COMMIT:?}");
    }
}
