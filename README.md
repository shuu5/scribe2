# scribe2

**codename `scribe2`** — 正式名は未定（user 裁定の手番）。

scribe v1（先行する scribe plugin repo）の I1〜I7 要件を新しい器で満たすための repo。

## 名前について（器 SPEC §7）

**名前は 1 定数 `NAME` に集約する。** repo 名・plugin 名・marker の中身・CLI 名は `NAME` から導出し、名前の字面をコードへ散らさない。正式名が決まったときに変えるのは `NAME` の 1 行だけ、という状態を保つ。

## 状態

P2 骨格。**開発は素の Claude Code session 1 つ + beads（bd）+ repo tracked の design-intent（folio 形式）だけ**で行う（2026-09-09 から。前の器 scribe v1 の plugin・役割席・gate 席・QA 席は使わない）。作業の流れ・done の定義・review の形は `CLAUDE.md`、折れない線は `design-intent/spec/constitution.html`、MVP のゴールは `design-intent/spec/srs.html`（起草中）。repo root の marker `.vessel` は自己ホストの便で置く（器 SPEC §7 / §12）。

- 憲法 v0.3（Always C1〜C13 / Ask-first A1〜A4 / Never N1〜N4）= user 承認済み（2026-09-07・P1 exit）。C14〜C16 の追随と v1 放棄に伴う C7 / A1 / C8 / A4 / N4 の改訂 ADR は起草中
- 台帳 prefix = `s2-`

## cargo workspace（P2 leg 0 / leg 1a）

- **NAME 1 定数**: 名前の字面を持つ `.rs` は `crates/scribe2/src/name.rs` ただ 1 本で、`xtask check` の `name-literal` が機械で守る。
- **`cargo xtask check`**: core 行数 / 1 file 行数 / test:src 比 / plugin manifest parity / lints 集合と opt-in / 直接依存は allowlist（dev-dep の insta 1 本）/ toolchain pin / 全木の path 衛生（paths-clean）を測り、違反 1 件 1 行で rc 1 を返す。
- **flip check の区間置換規則**: `cargo xtask flip-check --base <ref>` は変更 `.rs` ごとに「base の src 区間 + HEAD の test 区間」を base tree へ重ねて runner を撃ち、test が base で RED になることを確かめる（test 区間 = 直後に `mod` が続く行頭 `#[cfg(test)]` 以降・`crates/*/tests/*.rs` は全体）。git / tar / cargo の spawn 失敗と rc≠0 は `reason=infra-error` で rc 1。
- **道具 2 本**: `cargo install --locked cargo-deny@0.20.2` / `cargo install --locked cargo-insta@1.48.0`（dev-dep の `insta` も 1.48.0 に pin）。
- **CI 3 job**: `nextest`（`cargo nextest run --workspace --no-tests=fail`）→ `clippy`（`cargo clippy --workspace --all-targets -- -D warnings`）→ `xtask-check`（`cargo xtask check`）。flip-check / deny / insta の job は leg 1b で足す。
