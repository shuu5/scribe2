//! この器の library 面。bin は薄い dispatch と出力層だけを持ち、実体はここに置く。
//!
//! `[lib] name` は器の名前の字面を含まない別名にしてある。`cargo xtask check` の
//! `name-literal` が「NAME の字面を持つ `.rs` は `name.rs` ただ 1 本」を測るため、
//! bin から `use <NAME>::…` と書けないからである（設計 docs/design/rules-manifest.md §2）。

pub mod cli_outcome;
pub mod fleet;
pub mod headless;
pub mod hook;
pub mod name;
pub mod order;
pub mod pipe;
pub mod polarity;
pub mod rules;
pub mod seat;
