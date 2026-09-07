//! この器の名前を 1 箇所に集約する（器 SPEC §7）。
//!
//! repo 内で名前の字面を持つ `.rs` はこの file **ただ 1 本**である。
//! 正式名が決まったときに変えるのはここの 1 行だけでよい、という状態を保つ。
//! この不変条件は `cargo xtask check` の `name-literal` が機械で守る。

/// この器の名前。plugin 名・CLI 名・marker の中身はすべてここから導出する。
pub const NAME: &str = "scribe2";
