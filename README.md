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
- **flip check**: `cargo xtask flip-check --base <ref>` が、変更した `.rs` の test 区間だけを base へ重ねて撃ち、歯が base で RED になることを確かめる。判定クラスの語彙は **その判定行（stdout の `flip-check: …` 1 行）が SSOT** で、各クラスの意味と免除の札の使い方は `docs/design/pipeline.md` §7 を参照。
- **道具 3 本**: `cargo install --locked cargo-deny@0.20.2` / `cargo install --locked cargo-insta@1.48.0`（dev-dep の `insta` も 1.48.0 に pin）/ `cargo install --locked cargo-mutants@27.1.0`（週次 job と同じ版）。
- **CI 6 job**（standing teeth の SSOT は CI 1 本＝手元でだけ撃つ歯を残さない）:
  - `nextest`: `cargo nextest run --workspace --no-tests=fail`
  - `clippy`: `cargo clippy --workspace --all-targets -- -D warnings`
  - `xtask-check`: `cargo xtask check`
  - `flip-check`: `cargo xtask flip-check --base <PR の base sha>`（**PR のときだけ**・push(main) には比較する base が無い）
  - `deny`: `cargo deny check`（全面形＝advisories 込み・個別に黙らせない）
  - `insta`: `cargo insta test --test-runner nextest --unreferenced reject --check --workspace`（未 accept の差分と orphan snapshot の両方で赤）
- **週次 mutants**: `.github/workflows/mutants.yml` が週 1（と `workflow_dispatch`）で `cargo mutants --workspace --no-shuffle --copy-vcs true` を回し、撃墜率（`total_mutants` / `caught` / `missed`）を **artifact でなく job summary の 1 行**に残す。生存が在っても job は赤くしない（測定結果であって失敗ではない）——読むのは人。
