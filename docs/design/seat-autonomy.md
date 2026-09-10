# 設計: seat autonomy — 開発 session（planner / 管理席）の context 計測・cap guard・管理 tick・cycle を器に入れる

- 要件: [FR25](../../design-intent/spec/srs.html#FR25) 開発 session の context 計測 / [FR26](../../design-intent/spec/srs.html#FR26) cap guard / [FR27](../../design-intent/spec/srs.html#FR27) 管理 tick / [FR28](../../design-intent/spec/srs.html#FR28) cycle / [AC9](../../design-intent/spec/srs.html#AC9)・既存 [FR21](../../design-intent/spec/srs.html#FR21) / [FR23](../../design-intent/spec/srs.html#FR23)
- 憲法: [C3](../../design-intent/spec/constitution.html#c3) 開発 session の状態 / [C10](../../design-intent/spec/constitution.html#c10) 実測値の型 / [A1](../../design-intent/spec/constitution.html#a1) 3 クラス（v1 script の停止は「消す」）
- この設計から出る契約: SRS 便（本 doc と同 PR）→ meter + inject → cap guard → tick + cycle → 併走 1 日の切替便（後続は末尾）。
- 位置づけ: **器（runner）の機能ではなく、器の開発 session の機能**。runner は便ごとに headless claude を起動して終わる（CON6）ので不要。要るのは長生きする対話 session = planner / admin の 2 つの開発 sessionだけ。

## 1. v1 が担っているもの（実読 2026-09-10・置き換え対象の実体）
| 機能 | v1 の実体 | 判定 / 動作 |
|---|---|---|
| (a) context 計測 | `session-context-meter.py`（read-only） | tmux statusline の「NN% XXXk/YM」を parse（pane）／transcript jsonl の末尾 10MiB から算出（jsonl）。出力 1 行 `used_pct= used_tokens= window_tokens= source= sid=` |
| (b) 閾値通知 + work-tool deny | tick prompt (C) の `[fleet-cap]` 注入・`hooks/context-cap-guard.py`（PreToolUse） | 60% 超で退避を促し、編集 tool を deny（退避 skill だけ通す） |
| (c) 周期 tick | `scribe-tick-wake.timer`（systemd user・5 分）→ `scribe-tick-wake.py` | AND 4 条件（heartbeat stale ∧ pane idle ∧ 未 consumed WM 0 ∧ cycle marker 不在 or residue）で tick pointer 1 行を inject |
| (d) cycle 実行 | `scribe-cycle-kick.py` → `scribe-self-cycle-inject.py`（detach） | 退避後に lock を取り `/clear` → `/rebrief` を打つ。二重起動は `<marker>.lock` で排他 |
| (e) 退避 / 復元 | global skill ready-compaction / rebrief（実体は v1 の凍結 script） | WM = `working-memory.<sid>.md`・消費は `.consumed.md` へ mv（= SRS FR23 が既に固定） |
| 注入の口 | `scribe-inject.sh`（tmux send-keys + 送達確認） | rc 0 / 4 / 7 の 3 値（4 / 7 は偽陰性） |

v2 に既に在るもの: FR23（WM の規則）・FR21（`<state_dir>/inject.jsonl` = hook の消費記録）・C3（seat 状態は typed enum・fleet DB）。**無いもの: tmux を触る口（capture / send-keys）が 1 つも無い**（実測 grep 0）。

## 2. 要件（SRS v0.2 が正本・ここには写さない）
- FR25 開発 session の context 計測（event 型・N-3）/ FR26 cap guard（unwanted 型・N-1 / N-2）/ FR27 管理 tick（state 型・N-1 / N-3）/ FR28 cycle（event 型・N-1 / N-3）/ AC9（2 つの開発 session で 1 日同じ判定 + cycle 完走 1 回）。文面は `design-intent/spec/srs.html` の当該行を読む。
- 本設計はその HOW（subcommand・置き場・条件の評価順・lock）だけを決める。

## 3. 設計（subcommand `scribe2 seat …`・std のみ・env を読まない C2.2）
- `seat meter --target <tmux target> [--sid S]` … (a) の port。pane → jsonl の順で fallback。Measured 型で返す（C10）。
- `seat guard` … hook `pre-tool-use` の内側に組み込む（新 hook は増やさない）。cap は manifest 行 `seat.context_cap_pct`（初期値 60・裁定 id 付き）。deny の極性は既存 guard と同じ fail-closed。**計測不能は deny しない**（context が読めないだけで編集を止めると開発 session が詰む＝理由を inject.jsonl に記録して allow）。
- `seat tick --target T` … (c) の 4 条件を同じ順序で評価し、成立時だけ注入。heartbeat は `<state_dir>/seat/<target>/heartbeat`（anchor 配下の `.claude-session/` へ書かない＝v1 と場所を分けて併走可能にする）。
- `seat cycle --target T` … (d)。lock は `<state_dir>/seat/<target>/cycle.lock`（`O_EXCL`）。手順 = WM が未 consumed で在ることを確認 → `/clear` 注入 → 復元 command（`/rebrief`）注入 → lock 解除。
- `seat inject --target T --text …` … tmux `send-keys` + 送達確認（pane の末尾に text が現れたか）。rc は 0 / 1 の 2 値にし、v1 の偽陰性（4 / 7）を作らない。
- 駆動: systemd user timer は **repo に入れない**（起動コマンドを repo に置かない・CLAUDE.md）。unit の雛形は docs に書き、user の host で有効化する。
- 記録: 判定と注入は `<state_dir>/seat/<target>/tick.jsonl` に 1 行ずつ（FR21 と同じ schema）。
- 計測できない理由の弁別（便 2 の実装で決めた読み）: statusline の候補が無い周は、pane 本文が空なら `no-source`・本文が在れば `pane-no-statusline` に分ける（どちらも transcript が明示されていれば先に jsonl へ落ちる）。健全性を外れた候補は `pane-out-of-bound` で**不成立のまま**とし、別の出所で塗り直さない（壊れた面を他の値で隠さない）。

## 4. 憲法・制約との整合
- R-E12（常駐席は event 駆動で周期起動を張らない）: 管理 tick は席の**外**（host の timer）で回し、席には event（注入）としてしか届かない＝両立。R-G20（管理系の席の watchdog と別口座 respawn）は本設計の tick + cycle が受け皿。
- C3.3（状態は型で運び端末の見た目を判定に使わない）: 判定対象は seat state ではなく context 使用量で、出所（pane / transcript）は Provenance として Measured 型で運ぶ（C10）。端末描画は出所の 1 つであって seat state の判定入力ではない。
- CON4（v1 の役割席を持ち込まない）: 開発 session の**役割**は増やさない。planner / admin は既存の 2 つの開発 session で、v2 が持つのは「開発 session を生かす機械」だけ。
- CON5（不可逆の口を持たない）: `/clear` は session を作り直す＝退避済み WM が在るときだけ撃つ（FR23 の「DATA が無い周は brief を出さない」と対）。lock 無しでは撃たない。
- N（依存）: tmux は外部 process 呼出しで、crate 依存は増えない（A3 非該当）。
- C4（行数）: seat は新 module 1 本 + hook に guard 1 段。

## 5. 便の割り（契約は planner・実装は admin）
1. SRS 便（FR25〜27 + AC・user が /folio-architect を打つ）+ 本設計 doc の land。
2. `seat meter` + `seat inject`（歯: fixture の statusline 文字列 / jsonl から used_pct・send-keys は tmux 独立 socket で実測）。
3. `seat guard`（hook 内・manifest 行の追加は裁定 id 付き）。
4. `seat tick` + `seat cycle`（歯: 4 条件の真理値表・lock の排他・WM 不在では /clear を撃たない）。
5. 切替便: planner / admin の 2 つの開発 session で v1 timer と**併走 1 日**→ 判定一致を tick.jsonl で示す → user 裁定で v1 timer を stop（消す = A1・user 手番）。

## 6. 却下案
- (i) in-session の CronCreate だけで tick を回す: respawn / 口座切替で死に、死んだことを誰も検知しない（v1 の case A 裁定と同じ理由）。
- (ii) v1 script をそのまま plugin として積む: user 直命（v1 を開発に積まない）に反し、python 実体の保守が続く。
- (iii) context 上限を契約の大きさだけで受ける（CON6 の拡大解釈）: runner には正しいが開発 session には効かない。

## 7. 裁定済みの論点
- 計測の一次ソース = pane（statusline）一次・transcript は fallback（v1 と同じ・速い・NFR5 に収まる）。
- cap の初期値 = 60（user 裁定 2026-09-10・manifest 行 `seat.context_cap_pct` に裁定 id 付きで置く・C4 / C13 の閾値ではないので A2 非該当）。
- 切替の判定 = 併走 1 日 + cycle 完走 1 回（AC9）。v1 の timer を止めるのは user 手番（A1「消す」）。
