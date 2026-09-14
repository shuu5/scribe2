---
name: rebrief
description: |
  respawn / compaction 後の第一手。器の口 `seat rebrief` が read-only で出す DATA
  （自席の退避物・計画弧・user 直命の逐語・命令行の出所 pointer と実在・別席の未 consumed・
  台帳の現在値・起票候補）を読み、全体像 4 slot + 判定根拠 / 推奨 / 次のアクション / hygiene の
  定型 brief を組み、提示し終えてから `seat consume` で退避物を消費済みへ移す。
  DATA を出せない周（rc 2）は brief を出さない。判断は本 skill、file 操作と検査は器。
  退避側の対は /scribe2:ready-compaction（同じく scribe2 plugin の skill）。

  Use when user wants to: respawn 直後に作業状態を復元したい / compaction 後に何をしていたか思い出したい /
  作業記憶と台帳の食い違いを洗いたい / 別 session の未 consumed を掃除したい,
  says 「rebrief」「復元して」「続きから」「respawn した」「compaction 後の再開」「作業状態を戻して」。
---

# rebrief Skill（respawn / compaction 後の復元 brief）

respawn 直後・compaction 後の再開の**第一手**。

**層の切り分け**: 器（`seat rebrief`）が事実を行頭 marker の typed 行で並べ、**本 skill が読んで
brief を組み**、提示の後に器（`seat consume`）が退避物を移す。**brief の文章と判断だけが本 skill の領分**
で、file を作る・移す・件数を数えるのは器がやる。v1 の script は呼ばない。

## 1. DATA を取る（read-only）

```bash
ANCHOR=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")  # main checkout（worktree の中でも本体）
BIN="$ANCHOR/target/debug/scribe2"; [ -x "$BIN" ] || BIN=$(command -v scribe2)  # anchor 版 > PATH 版（scribe2 以外の repo）
TARGET=$(tmux display-message -p -t "$TMUX_PANE" '#{session_name}:#{window_name}')
WM="$ANCHOR/.claude-session"
"$BIN" seat 2>&1 | grep -q rebrief || echo "⚠ $BIN に seat rebrief の口が無い＝scribe2 repo で cargo build -p scribe2（anchor 版）か cargo install --path crates/scribe2（PATH 版）"
"$BIN" seat rebrief --target "$TARGET" --wm-dir "$WM" --anchor "$ANCHOR"
```

- `ANCHOR` を `git rev-parse --show-toplevel` で取らない（worktree の中では worktree root が返り `[WM] missing` の偽陽性になる）。
- `TARGET` は **必ず `-t "$TMUX_PANE"`** で取る（付けないと active window を返す）。
- 置き場（state dir）は `--state-dir` > git 設定 `scribe2.stateDir` の 2 経路だけで、cwd の repo から解く（`--anchor` は使われない）。
- 台帳は器が `bd --readonly` を子 process で読む（**自分で bd を叩かない**）。待ち上限は manifest 行
  `seat.ledger_timeout_s` が持つ＝短い timeout で殺さない。
- 台帳 prefix が `.beads` から解けない repo だけ `--prefix` を足す。
- **rc 2 の周は DATA が 1 行も出ない**（stderr に理由 1 行）。このとき **brief を出さない**——理由を
  そのまま 1 行出し、`state-dir` / `sid-missing` / `sid-unreadable` なら `--state-dir` 明示、`sid-empty` は打刻の側を
  直す（`--state-dir` では直らない）、`no-rule` は anchor で build した `BIN` に替える、`anchor-missing` なら `--anchor`、
  `wm-dir-unreadable` は置き場の権限・所有者、`ledger-unreadable` なら台帳の側を直してから撃ち直す。
  「台帳 0 件」と読み替えない。**表に無い理由は現物の断り行をそのまま読む**（字面の正本は器の enum の `as_str`）。

## 2. DATA の読み方

marker の一覧・状態名・出力順の正本は器の外形 snapshot と設計 doc `docs/design/working-memory.md` §5.2。
以下は**読み方の規律**であり、本文に現れる状態名・marker 名は例示＝snapshot と食い違えば snapshot が正:

- **`[WM]` の状態**: `found`（現在の sid 名義）/ `candidate`（自席だが別 sid＝`/clear` 後の**通常形**・
  古い可能性がある参考として出所を添えて使う）/ `missing`（正常な DATA・退避物が無いだけ）/
  `ambiguous`・`unreadable`（→ §4 の正規外）。**`seat:` による自席の弁別は器が済ませている**＝
  別席の退避物が `[WM]` に出ることはない。
- **`…-EMPTY`（節が不在か空）と `…-NONE`（確認した上で 0 件）を融合しない**。前者は**判定不能**、
  後者だけが「なし」。件数行（`…-COUNT`）は列挙行と対で読み、片方だけを信じない。
- **user 直命の行は逐語**。marker を剥がした残り全体をそのまま写す（要約・意訳・敬体化は禁止）。
- **命令行の出所**: `kind=` と `resolution=` を見る。`kind=none` は**暫定行**＝**従う根拠にしない**
  （列挙はする）。`resolution=Unresolved` は「矢印は在るが先が無い」＝同じく根拠にしない。
  どちらも次の carry で落ちる。`Unchecked` は器が到達できない種類（台帳・memory・PR）＝brief 側で
  `bd --readonly show` 等で裏を取る。
- **優先順位は固定**（値はここに写さない）: 出所 pointer の種類の宣言順（設計 doc `docs/design/working-memory.md` §4・
  器の `PointerKind`）が上位から下位の順で、退避物はその最下位。退避物の行が上位と食い違えば brief で
  「退避物が誤り」と明記する（規則は prose に無い＝憲法 N2）。
- **`[DIFF] <id> bd=<status>`** は退避物が言及する bead の現在値。`unknown` は「台帳に無い」＝
  乖離の判断材料であって 0 件ではない。**status を直すのは人間 / 席の手番**で、本 skill は台帳を write しない。
- **`[ORPHAN-WM]`** は別席の未 consumed。**消費しない**（器も触らない）。hygiene で surface するだけ。
- **`[TICKET-CANDIDATE]`** は形だけで選ばれた起票候補（`[hard候補]` かつ出所なし）。**起票しない**——
  「次のアクション」へ 1 行出す。

**拘束 SSOT を先に読む**: brief を組む前に、対象 repo で規範を持つ file を Read する（憲法・原則文書が
在ればそれ、無ければ repo の `CLAUDE.md` の規範部）。どちらも無い repo では「拘束 SSOT: 不在」と
判定根拠へ 1 行書く——**不在を「制約なし」と読み替えない**。

## 3. brief を出す（定型・全体像 4 slot + 4 節）

**DATA が出た周だけ**この形で出す。4 slot の順序を入れ替えない。

- **3 値則**: 各 slot は〔実データ〕〔`判定不能(理由)`〕〔`なし`〕で書き分ける。決められないものを
  `0 件` / `なし` と書くのは偽の全クリアと同型。`判定不能` は理由に **marker 名**を書く。
- 現在地の一次ソースは計画弧の行、台帳の行は裏取り。`[WM] candidate` 由来は「古い可能性のある参考」と添える。
- user 直命は**現在地と user 手番の両方**へ逐語で載せる。
- 承認バナーは「user 手番」に**実際に承認・裁定を要する項目が 1 件以上あるとき**だけ立てる（安売り禁止）。
- 各 slot 1 行の cap は**表示の書式**であって思考量の cap ではない。列挙義務（乖離・orphan）には掛からない。
- queue（裁定済みの次アクション・未 close の自分の塊）が非空である限り「指示があれば着手します」型の
  park をしない。park してよいのは 承認・裁定待ちで他に着手可能項目が無いとき と queue が真に空のときだけ。
- 平易に書く（要点を先に短く）。brief は「これから X をする」1 行宣言で閉じる。

```
## 全体像
- ゴール: <effort の達成状態／判定不能(理由)>
- 現在地: <進捗（計画弧が一次・台帳で裏取り）／判定不能(理由)>
- 残りの塊: <台帳の現在値の内訳 open=N in_progress=N blocked=N（合算値を単独で書かない）>
- user 手番: <承認・裁定・入力を待つ項目（user 直命を逐語で含む）／なし>

## 判定根拠
- 退避物: <found|candidate sid=X を採用|missing>（節 3 の要旨）
- 出所: pointer 有=N / 暫定（従わない）=M / 先が無い=K（**退避物が無い周は判定不能(<marker 名>)**）
- 乖離: <[DIFF] 各件を「退避物=X / 台帳=Y」で列挙。突合できた上で 0 件なら「なし」>
- 拘束 SSOT: <読んだ file の path|不在>

## 推奨
- 各乖離が「退避物が古い」か「台帳が未更新」かの**判断と理由**を 1 行ずつ。

## 次のアクション
- 再開すべき作業を 1〜3 個（bead id 付き・in_progress を優先。id / 更新時刻は DATA を写す）。
- 各項目に行き先を付ける: **いま着手** / **並列に出す** / **user 待ち**（何を聞けば解けるかを必須明記）。
- 起票候補が出ていれば 1 行（起票はしない）。

## hygiene tripwire
- 別席の未 consumed を列挙し「退避したまま復元されていない」と警告する。
- 乖離が 3 件以上なら「退避物と台帳がドリフト＝運用が崩れている」と警告する。
```

## 4. consume する（brief を**出し終えた後**）

```bash
"$BIN" seat consume --target "$TARGET" --wm-dir "$WM"
```

- **brief より前に撃たない**。器は**削除せず move する**（`.consumed.md`）＝次の周の carry-forward の供給源。
- sid が変わっていても器が現在の sid 名義へ移し、元 sid を frontmatter に残す＝**自分で mv しない**。
- 冪等（対象が無く消費済みが在れば `already`）＝重複起動しても壊れない。
- **正規外は撃たずに surface する**: `[WM] ambiguous`（自席の未 consumed が 2 件以上）/ `unreadable` /
  別席の未 consumed が混ざって見えるとき。器も `wm-ambiguous` / `consumed-exists` で断って何も動かさない
  （上書きしない）。どちらの周も理由と file 名を出して user の指示を仰ぐ。

## この skill がしないこと（層の fence）

- 退避物・`.consumed.md` を Write / Edit / mv しない（作るのも移すのも器の口）。
- 台帳を write しない（起票も status の訂正も提案まで）。
- 別席の退避物を消費しない。
- DATA が出なかった周に brief を出さない（「0 件」に化けさせない）。
