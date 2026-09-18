# 設計: working memory — 超過（機構ごと削除済み）

> **本 doc の設計は [ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (2) が超過した。**
> 退避物（WM doc）の書込・復元の DATA・消費（move）・直命の表・判断層の skill 2 つは `s2-07l.479.2` で
> code ごと削除された（rules 行 `seat.wm_directive_cap` と棚卸しの 2 行、極性の 2 行も同じ便で消えた）。
> 本文と契約表は git の履歴に在る（最後の版は本 doc の削除 commit の親）。台帳の bead（`s2-07l.479` とその子）が経緯を持つ。

- 残ったもの（別 doc が持つ）: 出所 pointer の形と解決は [seat-roles.md](./seat-roles.md) §5 の席の指示文が持ち（`seat/brief/pointer.rs`）、
  台帳の現在値の読みは同 §5 の `{ledger}`（`seat/ledger.rs`）である。
- session を跨ぐ記憶は台帳の構造（metadata / status / 依存）、文脈の圧縮は Claude Code の `/compact` である（ADR-0045 §2 (2)）。
