//! 境界 crate（設計 core-boundary.md §3・ADR-0033）。子 process を起こす面（tmux / git / claude / curl / cargo /
//! systemd / `sh -c`）の置き場で、binary `<NAME>` もこの crate が持つ（`main.rs`）。
//!
//! 依存は一方向: 境界 → core（lib `vessel`）。関数は「引数 → 子 process の起動 → 生の結果」だけを返し、解釈は core の
//! 純関数が持つ（同 §4）。module は純移動の便（同 §6 段 2）が core から運ぶ。
//!
//! 起動の実物（core の起動の記述を std の Command へ写して撃つ差し替え口の実装）は [`spawner`] の 1 file
//! だけが持つ（同 §9・ADR-0062）。

pub mod spawner;
