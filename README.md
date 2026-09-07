# scribe2

**codename `scribe2`** — 正式名は未定（user 裁定の手番）。

scribe v1（`~/projects/local-projects/scribe`）の I1〜I7 要件を新しい器で満たすための repo。

## 名前について（器 SPEC §7）

**名前は 1 定数 `NAME` に集約する。** repo 名・plugin 名・marker の中身・CLI 名は `NAME` から導出し、名前の字面をコードへ散らさない。正式名が決まったときに変えるのは `NAME` の 1 行だけ、という状態を保つ。

## 状態

P2 骨格の bootstrap 段 1。v2 は当面 **v1 の pipeline（scribe v1 の器）で作る**ので、この repo に marker `.vessel` は置かない（器 SPEC §12.1: P2 の間 v2 は v1 が仕える project である）。

- 憲法 v0.3（Always C1〜C13 / Ask-first A1〜A4 / Never N1〜N4）= user 承認済み（2026-09-07・P1 exit）
- 台帳 prefix = `s2-`
