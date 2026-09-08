# scribe2

**codename `scribe2`** — 正式名は未定（user 裁定の手番）。

scribe v1（先行する scribe plugin repo）の I1〜I7 要件を新しい器で満たすための repo。

## 名前について（器 SPEC §7）

**名前は 1 定数 `NAME` に集約する。** repo 名・plugin 名・marker の中身・CLI 名は `NAME` から導出し、名前の字面をコードへ散らさない。正式名が決まったときに変えるのは `NAME` の 1 行だけ、という状態を保つ。

## 状態

P2 骨格の bootstrap 段 1。v2 は当面 **v1 の pipeline（scribe v1 の器）で作る**ので、この repo に marker `.vessel` は置かない（器 SPEC §12.1: P2 の間 v2 は v1 が仕える project である）。

- 憲法 v0.3（Always C1〜C13 / Ask-first A1〜A4 / Never N1〜N4）= user 承認済み（2026-09-07・P1 exit）
- 台帳 prefix = `s2-`

## cargo workspace（P2 leg 0）

- **NAME 1 定数**: 名前の字面を持つ `.rs` は `crates/scribe2/src/name.rs` ただ 1 本で、`xtask check` の `name-literal` が機械で守る。
- **`cargo xtask check`**: core 行数 / 1 file 行数 / test:src 比 / plugin manifest parity / lints 集合と opt-in / 直接依存 0 本 / toolchain pin を測り、違反 1 件 1 行で rc 1 を返す。
- **CI 3 job**: `nextest`（`cargo nextest run --workspace`）→ `clippy`（`cargo clippy --workspace --all-targets -- -D warnings`）→ `xtask-check`（`cargo xtask check`）。
