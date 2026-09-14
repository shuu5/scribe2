---
name: ready-compaction
description: |
  context cycle（/clear・respawn）の前に、失うと困る「命令・状態」を carrier 別に振り分け、
  effort 一時層（作業記憶）だけを器の口 `seat externalize` で退避する skill。
  退避物の schema・carry-forward・命令行の文法・上限・暫定行の扱いは**器**が持ち、
  本 skill は「どれを何処へ」の振り分けと入力 3 file の作成、口を 1 回撃つこと、報告だけを行う。
  恒久命令は bd 起票を提案し、教訓は repo tracked な carrier へ回す（本 skill は書かない）。
  復元側の対は /scribe2:rebrief（同じく scribe2 plugin の skill）。

  Use when user wants to: prepare for context cycle, externalize knowledge,
  says 「cycle 前に退避」「/clear する前に」「respawn 前に」「context cycle」
  says 「compaction の準備」「知識を保存して」「ready-compaction」
  says 「コンテキストが限界」「作業状態を退避」
---

# ready-compaction Skill（cycle 前の退避）

context cycle（`/clear`・respawn）は文脈を**丸ごと**捨てる。捨てる前に、失うと困る項目を carrier へ
振り分け、**effort の間だけ真で他に置き場が無い状態**だけを作業記憶へ退避する。

**層の切り分け**（これを崩さない）:

- **器**（`seat externalize`）= file を作る側。3 節 schema・frontmatter・前の周からの carry-forward・
  節 3 の文法検査・上限・出所 pointer の分類と実在検査。**規則はここと設計 doc に在る**。
- **本 skill（LLM 層）** = 何を残すか決める側。振り分け・計画弧の文章・逐語の転記・報告。

退避物を **Write / Edit で直接書かない**（口は器だけ）。v1 の script は呼ばない。

## 0. 場所を解く

```bash
ANCHOR=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")  # main checkout（worktree の中でも本体）
BIN="$ANCHOR/target/debug/scribe2"; [ -x "$BIN" ] || BIN=$(command -v scribe2)  # anchor 版 > PATH 版（scribe2 以外の repo）
TARGET=$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}:#{window_name}')
WM="$ANCHOR/.claude-session"             # 退避物の置き場
"$BIN" seat 2>&1 | grep -q externalize || echo "⚠ $BIN に seat externalize の口が無い＝scribe2 repo で cargo build -p scribe2（anchor 版）か cargo install --path crates/scribe2（PATH 版）"
```

- `ANCHOR` を `git rev-parse --show-toplevel` で取らない（worktree の中では worktree root が返り、`WM` が空 dir に
  なって `[WM] missing` の偽陽性・2 席の `--wm-dir` が別々になり `[ORPHAN-WM]` が原理的に出ない）。
- `TARGET` は **必ず `-t "$TMUX_PANE"`** で取る（付けないと active window を返し、他席の名を名乗る）。
- 置き場（state dir）は `--state-dir` > git 設定 `scribe2.stateDir` の 2 経路だけで、**cwd の repo から**解く
  （`--anchor` は置き場の解決に使われない）。git 設定は repo-local に限らず、死んだ dir を指したまま rc 0 で
  別 dir へ書く型がある。断りの理由が `state-dir` / `sid-*` の周は `--state-dir` を明示して撃ち直す。
- `mkdir -p "$WM"` は済ませておく（`.gitignore` は勝手に触らない）。

## 1. 振り分け（carrier router）

会話から「失うと困る項目」を挙げ、**事実か命令か**・**いつ効くか**で行き先を決める。

| 種類 | 行き先 | 本 skill の役割 |
|---|---|---|
| 恒久命令（この repo で常に真） | **bd 起票** | 「起票しては?」と**提案のみ**。notes に 誰に効くか / 毎 boot 要るか / 根拠 を書く |
| 教訓・知見（事故・実測から得たもの） | repo tracked な carrier（ADR / research / 設計 doc） | **提案のみ**（本 skill は書かない） |
| その repo × その host の作業文脈 | auto-memory | 提案のみ。machine-local ゆえ**横断 carrier に当てにしない** |
| 跨 session の discrete なタスク | **beads** | `bd create` を促す。計画弧には **id 参照だけ**置き内容を重複させない |
| **user 直命**（cycle 中に user 本人が言ったこと） | 節 1（`--user`） | **逐語**で転記（要約・言い換え・敬体化は禁止） |
| effort 命令・制約（この作業の間だけ） | 節 3（`--directives`） | 出所 pointer を添えて 1 行にする |
| 作業状態・次の一手 | 節 2（`--plan`） | bd id と現在値で書く |

- 判断に迷う項目は**節 3 に落とさず**行き先を 1 行 user へ出す（作業記憶は一時層＝恒久の避難所にしない）。
- global CLAUDE.md への追記は自動提案・自動編集しない（口頭で促すだけ）。

## 2. 現在値の突合（書く前・MUST）

計画弧を**会話の信念だけで書かない**。退避物は次の周へ無検証で運ばれるので、stale な主張が焼き付く。

```bash
"$BIN" seat rebrief --target "$TARGET" --wm-dir "$WM" --anchor "$ANCHOR"   # read-only の DATA
git -C "$ANCHOR" status --porcelain; git -C "$ANCHOR" log --oneline -3
```

`[BD-COUNT]` / `[BD-INPROGRESS]` を現在値として読み、これから書く計画弧と突き合わせる。
rc が 0 でない周は突合が**成立していない**＝報告で「未突合（理由）」と書く（✓ を印字しない）。

## 3. 入力 3 file を書く（scratchpad へ）

器へ渡す平文を作る。**退避物そのものではない**（器が 3 節へ組む）。

- `--plan`（節 2・毎周上書き）: 現在地と次の一手を項目行で。bd id を添える。
- `--user`（節 1・任意）: user 発言 1 行 = 1 項目。**時刻は実測だけ**を使い、取れなければ `[時刻不明]`
  と書く（推定しない）。状態欄（未着手 / 着手中 / 完了 / user 撤回）以外は書き換えない。
- `--directives`（節 3・新規行だけ）: 見本 1 行 —
  `- [auto] [P1] since=2026-09-13 <本文> → SSOT: <出所>`
  形の SSOT は設計 doc `docs/design/working-memory.md` §3、出所 pointer の種類と実在検査は §4。
  **検査は器が行う**（落ちた行は行番号付きで全件返る）ので、ここで自前の検査表を作らない。

> **出所 pointer の無い行は暫定行**として退避物に入り、次の carry-forward で落ちる（設計 §4）。
> 「落ちてほしくない」なら本文を直すのではなく、憲法条・ADR・rules 行・bead id のどれかを添える。
> 道具が塞がれている旨の申し送りは規則ではない＝bead の有無を確かめ、無ければ起票候補として報告に出す。

## 4. 器の口を 1 回撃つ

```bash
"$BIN" seat externalize --target "$TARGET" --wm-dir "$WM" --anchor "$ANCHOR" \
  --plan "$PLAN" --directives "$DIRECTIVES" [--user "$USER_FILE"] [--role planner] [--trigger manual]
```

成功は stdout 1 行（`file=` / `carried=` / `dropped_provisional=` / `dropped_unresolved=` / `directives=`）。
**この値をそのまま報告へ写す**（会話の記憶で件数を書かない）。

断られた周（rc ≠ 0）は理由を読んで**直してから撃ち直す**。器は退避物を作っていない（fail-closed）。

| 理由 | 手当て |
|---|---|
| `wm-exists` | 自席の未 consumed が既に在る＝先に `/scribe2:rebrief` を回して consume する |
| `directive-grammar` | 返った行番号の行を直す（全件が 1 行ずつ出る） |
| `directive-cap` | **刈り込む**。上限の値は manifest 行 `seat.wm_directive_cap` が持ち、変更は裁定が要る（憲法 C5）＝上限を上げて回避しない |
| `no-rule` | 撃った binary が `seat.*` の manifest 行を持たない（古い build）＝anchor で `cargo build -p scribe2` し `BIN` を anchor 版にする |
| `sid-missing` / `sid-unreadable` / `sid-invalid` | 打刻が無い・読めない席＝tmux の外か hook 未積載。`--state-dir` を明示して撃ち直す |
| `sid-empty` | 打刻の sid が空＝`--state-dir` 明示では直らない。打刻の側（席の起動 hook）を直してから撃ち直す |
| `wm-unreadable` / `input-unreadable` | 置き場か入力 file が読めない＝権限・所有者・path を直す |
| `anchor-missing` | `--anchor` が repo の本体を指していない |
| `state-dir` | 置き場を解けない＝`--state-dir` を明示する（§0） |

**表に無い理由は現物の断り行をそのまま読む**（字面の正本は器の enum の `as_str`・表は網羅ではない）。

## 5. 報告と cycle 案内

```
✓ 退避 <file>（carried=<n> 暫定で落ちた=<k> 先の無い矢印で落ちた=<u> 新規の命令=<m>）
✓ 突合: bd open=<n> in_progress=<n> blocked=<n> / git <branch> 未 commit <n> 件
   （rc≠0 の周は「⚠ 未突合（<理由>）——計画弧の完了主張は未検証」に置き換える）
✓ 恒久命令 → bd 起票を提案 <n> 件 / 教訓 → repo carrier を提案 <n> 件（本 skill は書いていない）
>>> cycle の実行形は **この席への `/clear` 打鍵**（user が打つ・skill からは起動できない）。
    `/clear` の後の第一手は `/scribe2:rebrief`。自動復元の hook は無い＝打鍵と `/scribe2:rebrief` を省かない。
```

- `/clear` と respawn で background の Monitor / poll の生死が違う（respawn は全て失う・`/clear` は
  生き延びうる）。`/clear` で回した周は再武装の前に旧 monitor の生存を確かめる。

## しないこと

- 退避物・`.consumed.md` を **Write / Edit / mv しない**（作るのも移すのも器の口）。
- 節 1 を要約・言い換えない。user が言っていないことを user の依頼として書かない。
- 上限超過のまま cycle 案内を出さない（刈り込む・上限は上げない）。
- 別席の退避物を触らない（自席の弁別は frontmatter の `seat:`＝器が見る）。
- 外部化が未完了のまま「完了」と報告しない。
- `/compact` の自動実行を試みない（built-in ゆえ skill から起動できない）。
- `bd remember` / `bd recall` / `bd memories` を使わない。
