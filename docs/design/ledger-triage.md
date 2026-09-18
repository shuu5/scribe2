# 設計: ledger triage — 超過（出す面ごと削除済み）

> **本 doc の設計は [ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (2) が超過した。**
> 棚卸しの marker（`[MEMO-DUE]` / `[MEMO-STALE]` / `[MEMO-QUEUE]`）は復元の DATA に出す形だったが、その DATA が
> `s2-07l.479.2` で削除され、閾値の rules 行 2 本（`ledger.memo_stale_days` / `ledger.memo_stale_priority`）も
> 読み手を失って同じ便で消えた。本文と契約表は git の履歴に在る（最後の版は本 doc の削除 commit の親）。
> 台帳の bead（`s2-07l.479` とその子）が経緯を持つ。

- 残ったもの: 判定点を bd の `blocks` 依存で表す規約（§2）は台帳の使い方として残り、`.beads/PRIME.md` が持つ。
- 契約の列の観測は [dispatcher.md](./dispatcher.md) §6 の 1 口（`pipe dispatch ls`）である。
