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
- **flip check の区間置換規則**: `cargo xtask flip-check --base <ref>` は変更 `.rs` ごとに「base の src 区間 + HEAD の test 区間」を base tree へ重ねて runner を撃ち、test が base で RED になることを確かめる（test 区間 = 直後に `mod` が続く行頭 `#[cfg(test)]` 以降・`crates/*/tests/*.rs` と `crates/*/src/**/*_tests.rs`〔`tests.rs` を含む〕は**全体**＝`#[path]` で外出しした test module は `#[cfg(test)] mod` の形を持たず、名前で見なければ区間判定に見えない）。**flip した file が 2 本以上のときは 1 本ずつ単独で overlay して撃ち、全部が単独で RED のときだけ通す**（まとめて 1 回だと、赤い file に隠れて緑の新しい test が通る）。測れなかった便は**測れなかったと言う**: 新規 module の in-file 歯は base に `mod` 宣言ごと無く compile されないので、**その便に測れる flip が 1 本も無いとき**は `FAIL reason=not-flippable files=<rel>`（逃がし方を stderr に 1 行。測れる flip が在る便はそちらで判定し、写せなかった file は stderr の `not-copied` 行に出る）／**HEAD の test 区間の行列が base の行列の部分列**（順序を保った行の削除だけで得られる）な file は flip に数えず `not-flipped reason=tests-removed-only <rel>`（純粋な module 分割で恒久 FAIL しない・fn 名では数えない＝名前が同じまま本文を書き換えた歯を免除しないため）／既に在る挙動へ**後から足す歯**は test 区間に `// flip-check: retroactive <bead-id>` を 1 行置くと RED を要求せず判定行に `retroactive=N` が載る（review の対象・変異 proof が要る）。**効くのはその便で足した札だけ**である＝札の bead id が HEAD の test 区間に在り base の test 区間に無いときに限る（札は file に残るので、在るだけで数えると一度貼った札がその file の test 区間を触る以後のすべての便を免除する）。持ち越した札**しか**無い file で **test 区間が動いた便**には免除を与えず、stderr に `stale-marker <rel>` を 1 行出す（削除するか新しい bead id で置き直す。免除を求めていない便＝src だけ触った便には出さない）。**限界**: 札の bead id が実在の便を指すかは照合しない（bd を見ない）ので、**新規 file へ古い id の札を置く**形は通る——`retroactive=N` は判定行に必ず出るので review が拾う。git / tar / cargo の spawn 失敗と rc≠0 は `reason=infra-error` で rc 1。
- **道具 3 本**: `cargo install --locked cargo-deny@0.20.2` / `cargo install --locked cargo-insta@1.48.0`（dev-dep の `insta` も 1.48.0 に pin）/ `cargo install --locked cargo-mutants@27.1.0`（週次 job と同じ版）。
- **CI 6 job**（standing teeth の SSOT は CI 1 本＝手元でだけ撃つ歯を残さない）:
  - `nextest`: `cargo nextest run --workspace --no-tests=fail`
  - `clippy`: `cargo clippy --workspace --all-targets -- -D warnings`
  - `xtask-check`: `cargo xtask check`
  - `flip-check`: `cargo xtask flip-check --base <PR の base sha>`（**PR のときだけ**・push(main) には比較する base が無い）
  - `deny`: `cargo deny check`（全面形＝advisories 込み・個別に黙らせない）
  - `insta`: `cargo insta test --test-runner nextest --unreferenced reject --check --workspace`（未 accept の差分と orphan snapshot の両方で赤）
- **週次 mutants**: `.github/workflows/mutants.yml` が週 1（と `workflow_dispatch`）で `cargo mutants --workspace --no-shuffle --copy-vcs true` を回し、撃墜率（`total_mutants` / `caught` / `missed`）を **artifact でなく job summary の 1 行**に残す。生存が在っても job は赤くしない（測定結果であって失敗ではない）——読むのは人。
