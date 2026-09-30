# 設計: 対話面の作法 — planner が user に向けて出す文の形は設計 doc 1 本が持ち、生成文は pointer 付きの行で運ぶ

- 要件: [FR30](../../design-intent/spec/srs.html#FR30) 便の配送構造（planner が唯一の対話面）/ [FR44](../../design-intent/spec/srs.html#FR44) 席間の連絡（不変）。「対話面の作法」を名指す FR は要件書の改訂（user の手番）で足す。制約: CON2（PUBLIC・user の逐語を tracked file に書かない）
- 憲法: [C7](../../design-intent/spec/constitution.html#c7) 対話面は 1 つ / [C10](../../design-intent/spec/constitution.html#c10) 宣言・実測・導出を型で分ける（信頼度の語の根）/ [C14](../../design-intent/spec/constitution.html#c14) 規律は文書と manifest の 2 面 / [N2](../../design-intent/spec/constitution.html#n2) prose だけの規則は規則でない / [A1](../../design-intent/spec/constitution.html#a1) 3 クラスの承認
- 決定: [ADR-0032](../../design-intent/decisions/ADR-0032-dialogue-surface-rules-live-in-one-design-doc-and-planner-brief.html)（本 doc の置き場・9 則の形・5 slot・global の行き先）/ [ADR-0031](../../design-intent/decisions/ADR-0031-working-memory-is-held-by-the-vessel-directives-status-and-hooks.html) §2.2 §2.6（brief の材料 = 器の DATA・**ADR-0045 §2 (2) が超過**）/ [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html) §2.4（雛形の行の規律）
- 土台: [seat-roles.md](./seat-roles.md) §5（注入・雛形の行は穴か pointer 付き・xtask の検査）。器の DATA（現在地・直命の表）を土台に敷いていた面は **超過した**（ADR-0045 §2 (2)・`s2-07l.479.2`・[working-memory.md](./working-memory.md)）。
- この設計から出る契約: §7（3 便）。

## 1. 何を解くか

planner は user と話す唯一の席（R-C7-1）。その席が user に向けて出す文の形は、記録時点では user の global CLAUDE.md（host の file・口座が共有）が持ち、器の生成文は持たない。復元の brief は 5 節 30 行超で user が読まない。本 doc が「文の形」の正本になり、planner の雛形は pointer 付きの行で本 doc を指す。規則の本文は雛形にも skill にも rules 行にも置かない。

やさしく言うと: 「user への言い方」の決まりをこの 1 枚に集め、planner の起動時の指示文は「言い方はここ」と矢印で指すだけにする。

## 2. 作法（9 則・宣言順・上位が優先）

| # | 則 | 形 | 出所（融合元） |
|---|---|---|---|
| 1 | **信頼度** | 事実・結論の各文に `verified`（実行して確かめた）/ `deduced`（型・文書・記録から導いた）/ `inferred`（推測）/ `uncertain`（分からない）のいずれかを負わせる。hedge の副詞（perhaps / might）はこの 4 語に置き換え、削って断定に化けさせない。**時間の見積は書かない**（AI の見積は inferred にしかならない）＝便の段（Spawned / Implemented / Gated / Landed）と残り件数（母集団付き）で言う | global「回答の方針」信頼度 4 段 / i-have-adhd 則 6 を置換 / Pre-send check の hedge |
| 2 | **先頭は次の 1 手** | 1 行目 = user が今できること（承認・裁定・入力・直命の確認候補）。無ければ「planner が次にすること」。承認要求は冒頭に材料付き〔やりたいこと / 理由 / 代替とトレードオフ / コスト・リスク / 推奨〕+ 質問形。承認不要の報告に承認の形を使わない | global「承認要求 front-load」/ i-have-adhd 則 1 |
| 3 | **手順は番号** | 2 段以上の手順は番号付き・1 項目 1 動作 | i-have-adhd 則 2 |
| 4 | **末尾は 1 手** | 最終行 = 次に起きること 1 つ（planner の次の行為 か user の手番）。締めの挨拶・要約の反復を持たない。queue が非空なら「指示があれば」型で park しない | i-have-adhd 則 3 |
| 5 | **脇道は分けて末尾に** | 本筋の後に「別件:」で 1 行ずつ。**列挙義務（乖離・orphan・危険・件数と母集団）は脇道ではない**＝本筋の slot に載せ省略しない | i-have-adhd 則 4（条件付き） |
| 6 | **状態を言い直す** | 毎 turn 現在地を 1〜2 行。材料は planner が自分で測る（器がまとめて出す DATA は `s2-07l.479.2` で超過した）。user に「覚えておいて」を頼まない | i-have-adhd 則 5 / ADR-0045 §2 (2) |
| 7 | **成果を見せる** | 前 session からの Landed を id と sha で。作文で膨らませない | i-have-adhd 則 7 |
| 8 | **error は事実だけ** | 原因・修正・出所（bead / run id / file:line）。感嘆・謝罪・「問題があるようです」を持たない | i-have-adhd 則 8 / global「推測で答えず」 |
| 9 | **一覧は 5 件・前置きと締めなし** | 表示は 5 件まで・母集団の件数を併記・全件は file へ落として path を返す。前置き（「〜します」の宣言）・要約の反復・締めの挨拶を持たない。「詳細版で」と言われたら本文の長さの上限を外す（形は保つ）。平易に書く＝提示層だけ易しく、思考・解・code の技術水準は下げない | i-have-adhd 則 9 / 則 10 / global「平易」「ガードレール」 |

外した 2 則と理由: **時間見積**（則 1 に吸収）・**「不確かなら適用」**（生成文が固定するので自己判断の余地を持たない・断定の温床）。

採った例外（When to break）: 破壊的操作は A1 の 3 クラスが上位 / **debug spiral** = 3 周「まだ壊れている」なら手を止め、疑う前提を 1 つ名指して 1 問聞く / 曖昧 = 1 論点 1 質問・推奨 1 つ（順序や是認だけを求める問いは出さない）/ 規則と課題が衝突したら課題が勝ち形は保つ（「選択肢は？」には 2〜4 案を推奨先頭で）。

pushback: user の訂正を即座に受け入れず根拠を検討し、誤っていれば則 1 の語を付けて反論する。解釈が分岐するなら明示的に提示し、黙って 1 つを選ばない（ADR-0032 §2.2・C7）。

## 3. 復元の brief（user 面 5 slot・宣言順・各 1〜2 行）

| slot | 中身 | 材料 |
|---|---|---|
| 1 `next` | user の手番（承認・裁定・入力）。無ければ planner の次の行為 | planner の実測 |
| 2 `wins` | 前 session からの Landed（id・sha） | planner の実測 |
| 3 `status` | main の sha と同期・走行中の便（id・段・口座）・席の状態 | planner の実測 |
| 4 `plan` | 進行中の計画の上位 3 件（bead id）+ 母集団（open / in_progress / blocked） | 席の指示文の `{ledger}` + planner の実測 |
| 5 `risks` | 乖離・危険。無ければ「なし」・判定不能はその理由 | planner の実測 |

**材料の出所**: 器がこの 5 slot をまとめて出す DATA は `s2-07l.479.2` で超過した（ADR-0045 §2 (2)）。器が持つのは席の指示文の `{ledger}`（台帳の現在値）だけで、残りは planner が自分で測る。**器に新しい出力面を足すのはこの doc の射程外**（足す便は別の契約）。

## 4. planner の雛形に足す行（pointer 付き・穴なし・規範文 0）

雛形の行は「作法の名 → SSOT」の形で、本文を書かない（[seat-roles.md](./seat-roles.md) §5 の規律・xtask が pointer の無い行 0 を検査）。足す行は 4 本:

```
user に向けて出す文の形は対話面の作法（信頼度が上位・次の 1 手が先頭）に従う → SSOT: docs/design/dialogue-surface.md §2 / ADR-0032 §2.2
事実と結論は verified / deduced / inferred / uncertain のいずれかを負う → SSOT: docs/design/dialogue-surface.md §2 / 憲法 C10
復元の brief の user 面は 5 slot（next / wins / status / plan / risks） → SSOT: docs/design/dialogue-surface.md §3 / ADR-0032 §2.3
並列に出す agent は model と token 予算を明示し、出力は file へ落として path を返す → SSOT: docs/design/dialogue-surface.md §5 / ADR-0032 §2.4
```

admin の雛形には足さない（対話面でない・ADR-0032 §2.5）。

5 本目（契約表の行 h・`s2-07l.386`・[ADR-0037](../../design-intent/decisions/ADR-0037-rulings-without-a-run-are-approval-events.html)・[fleet-event-log.md](./fleet-event-log.md) §9 の口が Landed の後）:

```
user の裁定を受けた turn の中で対話面の席の口（seat ruling add・逐語・bead / rules 行 id）を撃ち、裁定 id はその event の ts とする → SSOT: docs/design/fleet-event-log.md §9 / ADR-0037 / 憲法 C7.2
```

## 5. global CLAUDE.md の行き先（ADR-0032 §2.4）

| global の節 | 行き先 | 本 doc / 器の側 |
|---|---|---|
| 言語・伝え方（日本語・平易・ガードレール） | **器の作法へ** | §2 則 9。HTML / tailnet の提示面は host 固有＝global に残す |
| 回答の方針（信頼度・pushback・承認 front-load・バナー様式・v1 の merge-gate pointer） | **器の作法へ** | §2 則 1 / 則 2 / pushback。v1 docs への pointer は撤去（A1 の 3 クラスが SSOT）。バナー様式は則 2 に簡素化 |
| タスク開始時（git fetch / status） | **hook が代替** | SessionStart の hook（DATA の面は `s2-07l.479.2` で超過） |
| ファイル編集後（commit → push・worker cell 例外 5 面同文） | **縮小**（git skill の 1 行） | worker cell は前の版の遺物。scribe2 は 1 bead = 1 PR・pipeline |
| multi-agent 実行（v1 骨格の不使用・model / budget 明示・file 出力） | **器の作法へ** | §4 の 4 行目（fan-out の 3 条件）。v1 骨格・cld-spawn の記述は撤去 |
| 破壊的操作の禁止（tmux / git の hook block） | **残す** | host の hook が SSOT。器の guard へ移すのは別件（N1） |
| 記憶（auto-memory のみ・旧 MCP の廃止） | **縮小**（1 行） | 経緯は撤去。知見の carrier は `design-intent/`（ADR / research）と設計 doc |
| ホスト・コンテナ（編集 = host / test = container） | **残す** | host 固有 |

見込み（deduced）: global 52 行 → host 固有 3 節 + git skill の 1 行。器は consumer の repo にも global にも書かない（ADR-0022 §2.4）＝痩身は global を持つ repo の便。

## 6. 歯（`crates/<NAME>/tests/e2e/hook.rs` の `hook_brief_` / xtask の `check`・名前の列は現物が SSOT）

- 雛形の行: pointer の無い行 0・穴 ⊆ 定義済み（既存の xtask の検査・行が増えても検査は不変）。生成文の外形 snapshot（`hook_brief_planner`）が 4 行分動く（C12.5）。
- 作法の遵守そのものは歯にしない（機械が測れない・ADR-0032 §4）。brief の 5 slot は skill の手順で、snapshot も歯も持たない。

## 7. 契約（1 便・(h)(i) は超過）

- **(g)** planner の雛形に §4 の 4 行を足す + 外形 snapshot（S・docs-adjacent・base で RED = snapshot の 4 行不在を名指す歯 1 本）。write-set = `seat/brief/planner.txt` + `tests/e2e/snapshots/e2e__hook__hook_brief_planner.snap` + 名指す歯の file。依存: ADR-0032 land。
- **(h)(i)** 判断層の skill 2 本の縮小と global の配備替えは**超過した**: skill も退避 / 復元の口も `s2-07l.479.2` で消えた（ADR-0045 §2 (2)・[working-memory.md](./working-memory.md)）。

## 8. 却下案（ADR-0032 §5 の写しは持たない・設計固有のもの）

- 5 slot を器が全文生成する: slot 1 と 4 は計画弧（AI の判断）を要し、器は事実しか持たない（ADR-0018 §2.1 の線）。器は材料（DATA）を出し、組むのは skill の手順。
- 作法の遵守を rubric（Correctness / Autonomy / …）で lens に採点させる: 採点の値が新しい閾値になり、機械が enforce できない値を規則の表に入れる圧力になる（ADR-0032 §5 (D)）。作法は生成文の pointer で運び、違反は user の訂正で戻す。
- 雛形の 4 行を admin にも足す: admin は user と話さない（ADR-0016 §2.1・relay のみ）。

## 9. 後続

- 「対話面の作法」を名指す FR と AC（user の /folio-architect）。
- global の痩身の後、host 固有の残り（破壊的操作の hook・提示面）を器の guard / report の口へ移すか（別の ADR）。
- runner / lens の出力の形（headless）は本 doc の対象外＝要るなら別 doc。

## 10. 発話の仕分けの口 — utterance sort が要望（開いた memo へ）と会話を仕分けの event 1 件で記帳し、utterance show が ts で 1 件の逐語を返し、仕分け済みかを 1 本の純関数が決める（契約表の行 i・ADR-0087・FR88 / AC58）

やさしく言うと: user の発言 1 つ 1 つに「これは頼みごと（memo へ）」「これは問いへの答え」「これはただの会話」の札を付ける口を作る。頼みごとと会話は、記録に札を 1 枚残すだけで台帳は書き換えない。どの発言にまだ札が無いかは、1 つの関数だけが決める。turn の終わりの止めも局面の出力も、その関数に同じ答えを出させる。

- 何が起きているか（main 3908279b・verified）:
  - `UtteranceSorted`（actor machine・`Case::Sorted { utterance, sorting }`・request の行だけ bead を持つ）の読み書きは行 f で着地済みで、書き手は 0 本。
  - 最上位の口の一覧（`crates/scribe2-boundary/src/main.rs` の `render_usage` と match）に `utterance` は無い。
  - 境界の crate は R-C4-5（316 行）の中に在り、口を 1 つ足すと match の 1 行と使い方の 1 語だけ伸びる。
  - `help.rs` は口ごとの表を持ち、`help_table_` の歯が表の FORM を live の使い方と照らす。
  - memo の判定は `ledger/form.rs` の `is_memo` と `MEMO_LABEL`。台帳の bead 1 本の読みは行 h が `ledger/mod.rs` に足す。
- 約束（番号は done と 1:1）:
  1. **置き場**: core に最上位の module を 1 つ足す（行 i の write-set の `+` の file 2 つ: 本体と cli）。`lib.rs` に 1 行、境界の `main.rs` の match と使い方に `utterance` を 1 つ。
  2. **仕分け済みかの純関数（1 本だけ）**:
     - 入力: 発話の ts と、その ts を指す `UtteranceSorted` と `RulingReceived` の列。
     - 出力: 閉じた 3 値（未仕分け・会話だけ・結びあり）。結びありは memo の id の列と裁定 id の列を持つ。
     - 規則: request か答えが 1 つでも在れば「結びあり」とし、会話の札は数えない（会話の後の要望と答えで会話が外れる）。会話だけなら「会話だけ」。どれも無ければ「未仕分け」。
     - IO を持たない。turn の終わりの判定（行 lc-e1b）と局面の出力（W4）はこの関数だけを呼ぶ。
  3. **`utterance sort --repo R --state-dir S --ts TS --as request --memo ID [--bd B]`**: 名指した memo が開いた memo のとき、`UtteranceSorted`（request・bead）を 1 件書く。台帳は書かない。同じ ts と同じ memo の request が在れば、何も書かず rc 0 で `already` を出す。1 つの発話を複数の memo へ仕分けられる。
  4. **`utterance sort --state-dir S --ts TS --as chat`**: 台帳を読まずに `UtteranceSorted`（chat）を 1 件書く。会話の札がすでに在れば `already`。
  5. **断り（閉じた 4 語・const slice）**: 当たった周は何も書かずに rc 1 で `utterance: refused reason=<語> ts=<ts>` を出す。
     - `no-utterance`: その ts の発話 event が無い。
     - `linked`: 要望か答えを持つ発話へ会話を付けようとした。
     - `not-memo`: 名指しが開いた memo でない（無い・閉じた・label intake:memo が無い）。
     - `ledger-unreadable`: 台帳を読めない。
  6. **`utterance show --state-dir S --ts TS`**:
     - 1 件の逐語だけを stdout に返す（末尾に改行 1 つ）。
     - 無い ts は rc 1 で `no-utterance`。
     - 逐語を返すのはこの口だけで、席が名指したときに限る。起動行・圧縮の前の 1 枠・合図・通知へは運ばない（FR65）。
  7. **読みの範囲**: どちらの口も event log を `read_all` で読む（口は人が撃つ 1 回なので NFR5 の外）。turn の終わりの読みの範囲は行 lc-e1b が決める。
  8. **使い方と help**: `utterance <sort …|show …>` を最上位の使い方に足し、`help.rs` に utterance の表（sort・show の 2 行）を足す。
- 歯（e2e は既存の `tests/e2e/seat/ruling.rs` に足す。発話と対話面の歯を 1 か所に置き、新しい e2e の file は作らない。lib は本体の file の歯の区間）:
  - e2e `utterance_sort_`（偽の bd と、event の fixture で書いた発話）:
    - (a) 要望と会話がそれぞれ仕分けの event を 1 件書き、偽の bd の書きは 0 回。
    - (b) 1 つの発話を 2 つの memo へ仕分けられる。
    - (c) 断り 3 形（無い ts・答えを持つ発話への会話・開いた memo でない名指し）で event log が不変。
    - (d) 同じ秒の 2 つの発話を ts で別々に仕分けられる。
    - (e) `utterance show` が逐語を 1 byte も違わずに返す。
  - lib `utterance_sorted_of_`:
    - (f) 3 つの値の表（無し・会話だけ・要望・答え・会話の後の要望・会話の後の答え・承認に使った発話を会話にした形）。
    - (g) 同じ入力を 2 回渡すと同じ結果になる。
  - AC58 の「承認の発話と回答の発話が会話で仕分け済み」は (f) の fixture で表す。器は承認 event へ結ばない（仕分けの口は承認 event を読まない）。
  - base で RED の理由: 機能不在（`utterance` が最上位の使い方の誤りで rc 2・lib は新しい file で該当 0 本）。
- 触らない:
  - 発話の記帳（行 g）・bind（行 h）・答えの口（行 j）。
  - turn の終わりの止め（lc-e1b）・doctor の未仕分けの行（lc-e6c）・計測の 3 欄（lc-e22）・局面の出力（W4）。
  - 行 f の event の型。
- 限界:
  - `sort` は発話と memo の組を 1 件ずつ書く。まとめて書く口は持たない。
  - `show` は event log を全部読む。長い log では遅くなりうるが、人が撃つ口なので許す。
- 却下:
  - 要望で memo の notes に発端を書く（ADR-0087: 発端の結びの正本は仕分けの event・台帳を書かない）。
  - 仕分けを `seat` の下の口にする（ADR-0087 が口の名を `utterance sort` / `utterance show` と決めた）。
  - 会話の札を event の削除で外す（log は追記だけ・外しは判定の関数で表す）。

## 11. 裁定面の答えの口 — seat ruling answer が問いの id と標準入力の逐語を受けて、経路 gui の発話 event と、bind と同じ書き（裁定 id・5 欄の行・close・裁定 event）を 1 周で行い、裁定 id を 1 行で返す。席の道具の呼び出しからの撃ちは hook の入口が断る（契約表の行 j・ADR-0087・FR82 / AC52 / AC58）

やさしく言うと: user が画面（裁定面）で問いに答えたとき、その字を器に渡す入口を 1 つだけ作る。字は command の引数でなく標準入力で受ける。席（AI）がこの入口を自分で叩くと、user が打っていない字を答えにできてしまう。そこで、席の道具の呼び出しからの撃ちは器の hook が実行の前に止める。

- 何が起きているか（main 3908279b・verified）:
  - 答えの口は 0 件。行 h の後は、逐語を受ける器の口が 0 本になる（`seat ruling add` は消える）。
  - PreToolUse の門は、`pre_tool_use`（`hook/mod.rs`）の最初に choice の門（`hook/choice_question.rs`）が在り、その後に write-set・起票・台帳の形・anchor・merge・権能・走行中の行の門が続く。
    - choice の門は子 module 1 つ・閉じた 2 値の判定・`WHAT`・`POLARITY`（in-loop・fail-closed）の形で、極性一覧（`polarity.rs` の `Guard`）に 1 行を持つ。
  - command の語の割りは `hook/ledger_guard.rs` の `segments`（pub・引用の外の `;` `&` `|` 改行で割り、引用を解いた語の列を返す）。
  - 権能の表（`CAPABILITY_COMMANDS`）は役割の権能に写すだけで、pane の無い session では撃たれない。
  - 承認 event を書くのは `pipe approve` だけ。
- 約束（番号は done と 1:1）:
  1. **口の形**: `seat ruling answer --repo R --state-dir S --question ID [--bd B]`。逐語は標準入力の全部で、末尾の改行も 1 byte も変えずに持つ。使い方の字は `< WORDS` で標準入力を示す。
  2. **断り（何も書かない・rc 1）**: 次の順で調べ、`seat ruling: refused reason=<語> question=<id>` を出す。語は閉じた集合で、行 h の const slice に足す。
     - `words-empty`: 逐語が空白だけ。
     - `ledger-unreadable`: 台帳を読めない。
     - `closed`: 問いが閉じている。
     - `not-question`: 台帳の問いでない。
     - 断りの周は、発話 event も書かない。
  3. **通る周**: 次の順に書く。
     - (a) 経路 gui の `UtteranceReceived` を 1 件書く（session 無し・逐語の detail）。行 g の store の 1 本で、一意の ms の ts を振る。
     - (b) 行 h の結びの 1 関数を、その ts と経路 gui で呼ぶ（裁定 id・5 欄の行・close・裁定 event）。
     - (b) が落ちた周は rc 1 で `partial utterance=<ts>` を出す。発話は残るので、`seat ruling bind` で同じ ts を結び直せる。
  4. **返す 1 行**: rc 0 で stdout に裁定 id だけを 1 行。逐語は載せない。
  5. **承認でない**: `ApprovalReceived` を書かない。承認の口と権能には触れない（C7）。
  6. **hook の門**: hook の子 module 1 つ（行 j の write-set の `+` の file）。`pre_tool_use` で choice の門の直後に撃ち、Bash の周だけ command を `segments` で読む。次のどちらかで deny する（rc 2・stderr 1 行・inject.jsonl に what が `answer-mouth-deny` の 1 行）。
     - (a) ある segment に、引用の外の語として `seat` `ruling` `answer` がこの順で並ぶ。器の binary の名・変数・`cargo run --` の前置きに依らない。
     - (b) segment の頭の語（前の `VAR=…` を除く）が shell（sh・bash・zsh・dash）か `eval` で、どれかの語が `seat ruling answer` を含む。
     - 役割・pane・rules・台帳を読まない。runner の session でも止める。
     - 断りの 1 行は、答えは結びの口で問いへ結ぶ（`seat ruling bind`）ことを告げる。
  7. **極性一覧**: `Guard` に 1 行を足す（語 `answer-mouth-deny`・in-loop・fail-closed）。位置は choice-question の直後（宣言の順を実行の順に揃える）。
  8. **使い方と help**:
     - seat の使い方に `ruling answer …` を足し、`help.rs` の seat の表に 1 行足す。
     - `fleet/cli.rs` の記帳の口の列の注を、結びの口と答えの口に書き換える。
- 歯（e2e は既存の `tests/e2e/seat/ruling.rs` と `tests/e2e/hook/guards.rs`。極性は既存の `tests/e2e/polarity.rs` と外形の snapshot）:
  - e2e `seat_ruling_answer_`（偽の bd）:
    - (a) 通る周: 経路 gui の発話 event と 5 欄の行（経路 gui）と close と裁定 event が 1 件ずつ書かれ、stdout が裁定 id の 1 行、承認 event は 0 件。
    - (b) 空白だけ・閉じた問い・問いでない bead の 3 形で、event log も偽の bd の書きも不変。
    - (c) 全部の使い方の行（数を母集団として出す）のうち、`WORDS` で逐語を受ける行が答えの口の 1 行だけ。
  - e2e `hook_answer_mouth_`:
    - 止まる 5 形: 素の撃ち・変数の binary・`cd … &&` の連鎖・`sh -c '…'`・`bash -lc "…"`。どれも rc 2 で、実行されない（偽の binary の印の file が無い）。
    - 通る 3 形: `grep -rn "seat ruling answer" docs`・`… seat ruling bind …`・`… seat ruling ls …`。
    - pane の無い session（runner の形）でも止まる。
  - 極性の数と外形の snapshot を書き直す（動く pin は実装が数え、retroactive の札を付ける）。
  - base で RED の理由: 機能不在（answer が使い方の誤りで rc 2・門が無く素の撃ちが通る）。
- 触らない:
  - bind の断りと書き（行 h・呼ぶだけ）。
  - 発話の記帳の hook（行 g）。
  - choice の門。
  - 権能の表と役割の行。
  - 席の notes の書きの門（lc-a6）。
  - 4 欄の既存の裁定の行の読み（引用の数えの行・W3）。
  - 計測の経路ごとの数え（lc-e22）。
- 限界:
  - 門は道具の呼び出しの command の字だけを見る。次の撃ちは門の外で、器は経路 gui として数えるだけ（ADR-0087 の欠点のまま）。
    - 席が起こした子 process（script の中・常駐の server）からの撃ち。
    - 器の plugin を積まない session からの撃ち。
    - 仕えない repo の session からの撃ち。
  - (a) は `echo seat ruling answer` のような無害な並びも止める（止める側へ倒す）。
- 却下:
  - 権能の表に行を足す（権能を持つ役割は通り、pane の無い session では撃たれない。FR82 は役割に依らず止める）。
  - 逐語を `--words` で受ける（command の字面に逐語が残り、道具の呼び出しの記録に写る）。
  - 起票の門（`ledger_guard.rs`）に同居させる（別の約束で、一覧で見えなくなる）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "g"
title = "planner の雛形に §4 の 4 行を足す（pointer 付き・規範文 0）+ 外形 snapshot"
req = ["FR30", "FR67"]
section = "4"
also = ["crates/scribe2/src/seat/brief/planner.txt", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__hook__hook_brief_planner.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_brief_planner_"]
size = "S"
done = "planner の生成文に dialogue-surface.md §2 を指す行 2 本と §3 / §5 を指す行 1 本ずつが → SSOT: 付きで在り、admin の生成文には無い"

[[contract]]
id = "h"
title = "planner の雛形に §4 の 5 本目（裁定を受けた turn で seat ruling add を撃つ・pointer 付き・規範文 0）を足す + 外形 snapshot"
req = ["FR41", "FR67"]
section = "4"
depends = ["g"]
also = ["crates/scribe2/src/seat/brief/planner.txt", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__hook__hook_brief_planner.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_brief_planner_"]
size = "S"
done = "planner の生成文に fleet-event-log.md §9 と ADR-0037 を指す裁定の行が → SSOT: 付きで 1 本増え、admin の生成文には無く、既存の 4 行は不変"

[[contract]]
id = "i"
title = "発話の仕分けの口 — utterance sort が要望（開いた memo へ）と会話を仕分けの event 1 件で記帳し（台帳を書かない）、utterance show が ts で 1 件の逐語を返し、仕分け済みかを 1 本の純関数が決める（ADR-0087）"
req = ["FR88", "FR65"]
section = "10"
write-set = ["+crates/scribe2/src/utterance.rs", "+crates/scribe2/src/utterance/cli.rs", "crates/scribe2/src/lib.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail utterance_sort_", "cargo nextest run -p scribe2 --lib --no-tests=fail utterance_sorted_of_"]
size = "M"
growth = ["crates/scribe2/src/utterance.rs:210", "crates/scribe2/src/utterance/cli.rs:150", "crates/scribe2/src/lib.rs:1", "crates/scribe2-boundary/src/main.rs:2", "crates/scribe2/src/help.rs:12"]
done = "(1) core に最上位の module 1 つと cli を足し、lib.rs と境界の main.rs に utterance を 1 つずつ (2) 仕分け済みかは IO の無い純関数 1 本が未仕分け・会話だけ・結びありの 3 値で決め、要望か答えが在れば会話を数えない (3) sort --as request --memo が開いた memo の周に UtteranceSorted request を 1 件書き、台帳を書かず、同じ組は already (4) sort --as chat が台帳を読まずに 1 件書く (5) no-utterance・linked・not-memo・ledger-unreadable を何も書かずに rc 1 で断る (6) show が 1 件の逐語だけを返し、無い ts は no-utterance (7) 両口は read_all で読む (8) 最上位の使い方と help の表に utterance の sort と show 歯: utterance_sort_ が要望と会話の 1 件ずつと台帳の不変、1 発話 2 memo、断り 3 形の不変、同じ秒の 2 発話、show の逐語の一致を、utterance_sorted_of_ が 3 値の表と会話の外しと同じ入力の同じ結果を測る。base は utterance が使い方の誤りで RED"

[[contract]]
id = "j"
title = "裁定面の答えの口 seat ruling answer — 問いの id と標準入力の逐語を受け、経路 gui の発話 event と bind と同じ書きを 1 周で行って裁定 id を 1 行で返し（承認 event は書かない）、席の道具の呼び出しからの撃ちは hook の入口が実行の前に断る（ADR-0087）"
req = ["FR82", "FR88"]
section = "11"
depends = ["i"]
write-set = ["+crates/scribe2/src/hook/answer_mouth.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/src/seat/ruling.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/help.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_answer_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_answer_mouth_"]
size = "M"
growth = ["crates/scribe2/src/hook/answer_mouth.rs:150", "crates/scribe2/src/hook/mod.rs:4", "crates/scribe2/src/polarity.rs:12", "crates/scribe2/src/seat/ruling.rs:90", "crates/scribe2/src/seat/cli.rs:25", "crates/scribe2/src/help.rs:2", "crates/scribe2/src/fleet/cli.rs:1"]
done = "(1) seat ruling answer --repo --state-dir --question [--bd] が標準入力の逐語を 1 byte も変えずに受け、使い方は < WORDS で示す (2) 空白だけ・台帳を読めない・閉じた問い・問いでない bead を、この順に何も書かず（発話 event も）rc 1 で断る (3) 通る周は経路 gui の発話 event を一意の ms の ts で書いてから行 h の結びの 1 関数を呼び、後半が落ちた周は partial utterance=<ts> で rc 1 (4) stdout は裁定 id の 1 行だけ (5) 承認 event を書かない (6) hook の子 module が choice の門の直後に Bash の command を segments で読み、引用の外の seat ruling answer の並びと、shell か eval の語の中の seat ruling answer を、役割と pane に依らず rc 2 で断る (7) 極性一覧に answer-mouth-deny（in-loop・fail-closed）を choice-question の直後に 1 行 (8) 使い方と help の表に ruling answer を足し、fleet/cli.rs の注を直す 歯: seat_ruling_answer_ が通る周の 4 つの書きと裁定 id の 1 行と承認 0 件、断り 3 形の不変、WORDS で逐語を受ける使い方が 1 行だけ（母集団は全部の使い方の行）を、hook_answer_mouth_ が止まる 5 形の不実行と通る 3 形と pane の無い session の断りを測る。base は answer が使い方の誤りで、素の撃ちが門を通って RED"
<!-- contracts:end -->
