# 設計: seat autonomy — 超過（機構ごと削除済み）

> **本 doc の設計は [ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (2) が超過した。**
> 席の context 計測・cap guard・管理 tick・作り直しの cycle・合図の back-off・窓ごとの強制 cycle・heartbeat・statusline は
> `s2-07l.479.1` で code ごと削除された（rules 行 7 本と極性の 2 行も同じ便で消えた）。本文と契約表は git の履歴に在る
> （最後の版は本 doc の削除 commit の親）。台帳の bead（`s2-07l.479` とその子）が経緯を持つ。

- 残ったもの（別 doc が持つ）: 席の登録と起動は [seat-roles.md](./seat-roles.md) §2 / [account-lifecycle.md](./account-lifecycle.md) §4、
  便用の口座選定は [account-autonomy.md](./account-autonomy.md) §3、SessionStart の打刻は [seat-state.md](./seat-state.md) §2。
- 文脈の圧縮は Claude Code の `/compact`、session を跨ぐ記憶は台帳の構造（metadata / status / 依存）である（ADR-0045 §2 (2)）。
