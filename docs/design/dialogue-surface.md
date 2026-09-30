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
  - memo の判定は `ledger/form.rs` の `is_memo` と `MEMO_LABEL`。台帳の bead 1 本の読みは行 h が `ledger/mod.rs` に足す（本行は行 h の着地の後に走り、その読みを呼ぶだけで `ledger/mod.rs` は書かない）。
  - 仕分けの値は `fleet/mod.rs` の `Sorting`（`Request`・`Chat`）。答えの結びは行 h が `RulingReceived` に足す発話の ts の key で読む。
- 約束（番号は done と 1:1）:
  1. **置き場**: core に最上位の module を 1 つ足す（行 i の write-set の `+` の file 2 つ: 本体と cli）。`lib.rs` に 1 行、境界の `main.rs` の match と使い方に `utterance` を 1 つ。
  2. **仕分け済みかの純関数（1 本だけ）**:
     - 入力: 発話の ts と、その ts を指す `UtteranceSorted` と `RulingReceived` の列。
     - 出力: 閉じた 3 値（未仕分け・会話だけ・結びあり）。結びありは memo の id の列と裁定 id の列を持つ。
     - 規則: request か答えが 1 つでも在れば「結びあり」とし、会話の札は数えない（会話の後の要望と答えで会話が外れる）。会話だけなら「会話だけ」。どれも無ければ「未仕分け」。
     - IO を持たない。turn の終わりの判定（行 lc-e1b）と局面の出力（W4）はこの関数だけを呼ぶ。
  3. **`utterance sort --repo R --state-dir S --ts TS --as request --memo ID [--bd B]`**: 名指した memo が開いた memo のとき、`UtteranceSorted`（request・bead）を 1 件書く。台帳は書かない。同じ ts と同じ memo の request が在れば、何も書かず rc 0 で `already` を出す。1 つの発話を複数の memo へ仕分けられる。
  4. **`utterance sort --state-dir S --ts TS --as chat`**: 台帳を読まずに `UtteranceSorted`（chat）を 1 件書く。会話の札がすでに在れば `already`。
  5. **断り（閉じた 4 語・const slice）**: 当たった周は何も書かずに rc 1・stdout 0 byte で、stderr に `utterance: refused reason=<語> ts=<ts>` の 1 行を出す。調べる順は no-utterance → linked → ledger-unreadable → not-memo（event log の判定を台帳の読みより先に行い、台帳を読むのは request の周だけ）。
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
    - (a) 要望と会話がそれぞれ仕分けの event を 1 件書き、偽の bd の書きは 0 回。会話は、偽の bd の show が読めない JSON を返す置き場でも rc 0 で 1 件書き、偽の bd の呼び出しの記録が 0 行（約束 4 の台帳を読まない）。
    - (b) 1 つの発話を 2 つの memo へ仕分けられる。
    - (c) 断りの 7 形（無い ts の no-utterance を `--as request` と `--as chat` の 2 形・答えを持つ発話への会話と要望を持つ発話への会話の linked 2 形・名指しが無い bead / 閉じた memo / label intake:memo の無い bead の not-memo 3 形）が、どれも rc 1・stdout 0 byte・stderr が `utterance: refused reason=<語> ts=<ts>` の 1 行と逐語で一致し、event log が不変。ledger-unreadable は (e3)。会話の口で発話の在否を見ない実装と、答えだけを linked に数える実装を落とす（FR88 は要望か答えの仕分けを持つ発話への会話を断る）。
    - (c2) 断りの順: 無い ts ∧ 偽の bd の読めない JSON の request と、無い ts ∧ 開いた memo でない名指しの request が、どちらも no-utterance だけを出す（event log の判定が台帳より先）。
    - (d) 同じ秒の 2 つの発話を ts で別々に仕分けられる。
    - (e) `utterance show` が逐語を 1 byte も違わずに返す。3 つの発話を持つ log で真ん中の ts の show が真ん中の逐語だけを返す。無い ts は rc 1 で `no-utterance` を出す。
    - (e2) 同じ ts と同じ memo の request と、会話の札が在る発話への chat が、どちらも rc 0・stdout が `already` の 1 行で、event log が不変。
    - (e3) 偽の bd の show が読めない JSON を返す周の request が rc 1・stdout 0 byte・stderr の 1 行が `reason=ledger-unreadable` で、event log が不変。
    - (e4) 読みの範囲（約束 7）: 対象の発話の後に発話でない event を合わせて 2 MB 続けた log で、会話の sort と show が通る（末尾の窓だけを読む実装を落とす）。
  - e2e の既存の `cli_help_`（`crates/scribe2-boundary/tests/e2e/main.rs`・約束 8）: 最上位の語が 1 つ増えるので、次を直す。直した歯は base で落ちるので retroactive の札は付けない。
    - `cli_help_overview_names_every_top_word_with_a_purpose` と `cli_help_pages_carry_the_headings_in_order` の頂点の語の数（15 → 16）。
    - `cli_help_pages_match_the_live_form_and_every_subcommand` の突き合わせる口の数（9 → 10・utterance は免除の 6 口に入れない）。
    - `cli_help_bare_and_unknown_stay_one_usage_line` の最上位の使い方の行の字（`utterance` を 1 語足す）。
  - 最上位の使い方の行を写す既存の外形 snapshot 5 本を書き直す: `tests/e2e/snapshots/e2e__rules__rules_external_form.snap` と、境界の crate の `src/snapshots/` の doctor の 4 本（`doctor_external_form`・`ledger_form_doctor_external_form`・`ledger_lint_doctor_external_form`・`ledger_graph_doctor_external_form`）。
  - lib `utterance_sorted_of_`:
    - (f) 3 つの値の表（無し・会話だけ・要望・答え・会話の後の要望・会話の後の答え・承認に使った発話を会話にした形・要望の後の会話）。要望の後の会話（口は linked で断るが、入力の列としては在りうる並び）も結びありで、札の並びに依らない（最後の札で決める実装を落とす）。
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
  2. **断り（何も書かない・rc 1）**: 次の順で調べ、`seat ruling: refused reason=<語> question=<id>` を出す。語は閉じた 4 語。closed と not-question は行 h の結びの断りの語、ledger-unreadable は行 h の台帳を読めない周の断りの語と同じ字で、words-empty だけが答えの口の新しい語。行 h の結びの閉じた 4 語の断り（no-utterance・bound・closed・not-question の const slice）は変えない（結びは words-empty を返さない）。
     - `words-empty`: 逐語が空白だけ。
     - `ledger-unreadable`: 台帳を読めない。
     - `closed`: 問いが閉じている。
     - `not-question`: 台帳の問いでない。
     - 断りの周は、発話 event も書かない。
  3. **通る周**: 次の順に書く。
     - (a) 経路 gui の `UtteranceReceived` を 1 件書く（session 無し・逐語の detail）。行 g が `crates/scribe2/src/fleet/store.rs` に足す追記の 1 本（lock の中で一意の ms の ts を振る・fleet-event-log §13 約束 5）を呼ぶ。
     - (b) 行 h の結びの 1 関数を、その ts で呼ぶ（裁定 id・5 欄の行・close・裁定 event）。経路は結びが発話 event の gui を読む（結びの関数は経路を引数に持たない）。
     - (b) が落ちた周（結びの断り・台帳を読めない・notes の追記の失敗・close か裁定 event の途中の止まりのどれでも）は rc 1 で `partial utterance=<ts>` を出す。発話は残るので、`seat ruling bind` で同じ ts を結び直せる。
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
    - (a) 通る周: 経路 gui の発話 event と 5 欄の行（経路 gui）と close と裁定 event が 1 件ずつ書かれ、stdout が裁定 id の 1 行、承認 event は 0 件。標準入力は前後に空白を持ち末尾に改行 2 つを持つ字で、発話 event の detail と、5 欄の行の最後の欄を JSON の文字列として戻した字が、標準入力の byte と一致する（約束 1）。
    - (b) 断りの 4 語の全部: 空白だけの逐語（`words-empty`）・偽の bd の show が読めない JSON を返す（`ledger-unreadable`）・閉じた問い（`closed`）・問いでない bead（`not-question`）。どれも rc 1 で `reason=<語>` の 1 行を出し、event log（発話 event を含む）も偽の bd の書き（append-notes と close）も撃つ前と同じ。
    - (c) 全部の使い方の行（数を母集団として出す）のうち、`WORDS` で逐語を受ける行が答えの口の 1 行だけ。
    - (d) 断りの順: 2 つの断りに同時に当たる入力 3 形で、先の語だけが出る。空白だけの逐語 + 読めない台帳 → `words-empty`・空白だけの逐語 + 閉じた問い → `words-empty`・閉じていて問いでない bead → `closed`。
    - (e) 書きの途中の失敗: 偽の bd が close の撃ちの中で event log の file を脇へ移し、同じ path に dir を置く（fleet-event-log §14 行 h の歯 (h) と同じ撃ち方）。答えの口は rc 1 で、出力に `partial utterance=<ts>` の 1 行が在り、その ts は脇へ移した log の経路 gui の発話 event の ts と一致し、偽の bd の append-notes と close の書きは残る。log を戻して同じ問いと ts で `seat ruling bind` を撃つと rc 0 で、log の裁定 event が 1 件になり、発話 event は 1 件のまま（発話が残り、結び直せる証拠）。
    - (e2) notes の追記の失敗: 偽の bd の append-notes が rc 1 を返す周も、答えの口は rc 1 で `partial utterance=<ts>` の 1 行を出し、発話 event は 1 件残り、close は撃たれない（close の途中の止まりだけを partial にする実装を落とす）。
  - e2e `hook_answer_mouth_`:
    - 止まる 8 形: 素の撃ち・変数の binary・`cd … &&` の連鎖・`sh -c '…'`・`bash -lc "…"`・`eval "… seat ruling answer …"`・`X=1 bash -c '…'`（前の代入を除いた頭の語）・`cargo run -- seat ruling answer …`。どれも rc 2 で、実行されない（偽の binary の印の file が無い）。stderr は 1 行で `seat ruling bind` を含み、inject.jsonl に what が `answer-mouth-deny` の行が 1 行増える（撃った形の数を母集団として出す）。
    - 通る 3 形: `grep -rn "seat ruling answer" docs`・`… seat ruling bind …`・`… seat ruling ls …`。
    - pane の無い session（runner の形）でも止まる。
  - e2e `polarity_lists_answer_mouth_deny_`（`tests/e2e/polarity.rs`）: 一覧に `guard=answer-mouth-deny timing=in-loop on-failure=fail-closed` で始まる行が 1 行在り、位置は `guard=choice-question-deny` の直後・`guard=write-set-guard` の直前。
  - 極性の既存の歯を直す（直した歯は base で落ちるので retroactive の札は付けない）:
    - `polarity_lists_choice_question_deny_first_right_before_write_set_guard`: 2 つ目を `guard=answer-mouth-deny`、3 つ目を `guard=write-set-guard` にする（先頭は choice-question-deny のまま）。
    - `tests/e2e/polarity.rs` の数の pin を持つ既存の歯 4 本（`runner_question_guard_is_in_loop_fail_open` の in-loop と fail-open・`polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed` の in-loop と guards・`polarity_lists_contract_table_as_a_post_hoc_fail_closed_guard` の guards と post-hoc・`polarity_lists_command_guard_as_in_loop_fail_closed_right_after_cap_guard` の guards と in-loop）: 本行を撃つ時の main の値から in-loop と guards を 1 ずつ増やす。4 本目は便 s2-07l.738.36-20260930T151823Z の契約の審査が名指した（数を pin する歯を `guards=` と `in-loop=` の字で全部数えて足した・2026-09-30 の main で e2e の polarity.rs の 4 本と外形の snapshot）。
    - 外形の snapshot（`polarity_external_form`）。
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

## 12. turn の終わりの止め — 未告の未仕分けの発話を持つ session の turn の終わりを 1 度だけ止め、ts を全部並べて仕分けの口を告げる（契約表の行 k・FR88・NFR5・AC58・ADR-0087）

やさしく言うと: user の発話を席が仕分けないまま turn を終えようとしたら、器が 1 度だけ引き止めて「まだ仕分けていない発話の番号」と仕分けの口を伝える。同じ番号では 2 度止めない。止めた周は席を「空いた」と記録しない。記録を読めないときは止めずに通し、読めなかったことを残す。

- 何が起きているか（main b028af03・verified）:
  - hook の `Stop` の分岐（`crates/scribe2/src/hook/mod.rs` の `dispatch`）は打刻だけ（`stamp.rs`・Idle・stdout 0 byte・rc 0）。`stamp.rs` は `stop_hook_active` が真の再入を打刻しない。`SessionStart` の分岐は名乗り → 打刻 → 読み込み元の記録の順。plugin の hooks.json の Stop の行は timeout 10 で、rc はそのまま Claude Code へ届く。
  - Claude Code の Stop hook は rc 2 で止まり stderr を model へ渡し、payload に `session_id` と `stop_hook_active` を持つ（inferred・Claude Code の hooks の文書。本物の session での扱いは ADR-0087 が未実測と記録）。
  - event の kind（fleet-event-log §12・行 f 着地済み）: `UtteranceReceived`（chat の行は session を持つ）・`UtteranceSorted`・`TurnEndUnjudged`（key は session と reason・reason の語の一覧は本行が決める）。
  - 告げ済みの控えも session の開始の位置も main に無い。ADR-0087 は両方を state dir の session ごとの置き場に置き、跨版でないと決めた。
  - 発話の書き手は fleet-event-log の行 g（UserPromptSubmit・runner と lens と marker の外の session は書かない）、仕分け済みかの純関数は本 doc の行 i が置く。
  - 再入の沈黙を pin する歯: `tests/e2e/hook.rs` の `seat_state_hook_stays_silent_when_it_cannot_stamp`。打刻 3 event の歯 `seat_state_hook_stamps_three_events_into_state_jsonl` は prompt の key を持たない payload で撃つ。
- 約束（約束 2〜9 が done の (1)〜(8) と 1:1・約束 1 は done に載せない・done (9) は触らないの `EventKind` の arm）:
  1. **置き場と呼び出し**: 判定は行 k の write-set の `+` の file（hook の子 module）が持つ。Stop の分岐と SessionStart の分岐はそれぞれその file の関数を 1 回呼ぶ（hook の呼び出しは今の 1 回のまま・NFR5）。
     - 置き場と関数の呼び方は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。hook の呼び出しの回数は write-set の外の hooks.json が決め、runner の guard が測るので done にしない（器の門が測る）。
  2. **開始の位置**: SessionStart の分岐は、打刻の後に、その session の置き場（state dir の session ごとの置き場・ADR-0087）へ event log の今の長さ（byte）を書く。置き場に開始の位置が既に在る session（/compact と resume の SessionStart）は上書きしない（圧縮の前の未仕分けを止めから漏らさない）。payload に session_id が無い周は書かない。
  3. **判定**: 台帳を読まず、開始の位置から event log の末尾までを読み（`fleet/store.rs` の `events_path` の file を開始の位置から読み、行ごとに `Event::from_line` で読む・store.rs は触らない）、その session の `UtteranceReceived` のうち、本 doc の行 i の仕分け済みかの純関数が未仕分けと判じたものを未仕分けとする（判定の 2 つ目を書かない・C2）。発話の記帳の対象の外の session（runner・lens・marker の外）は行 g が発話を書かないので未仕分けが 0 で、今のまま打刻だけ（event も控えも書かない）。
  4. **控え**: 開始の位置と同じ session ごとの置き場の 1 file（1 行 1 記録: `told <ts>`・`block`・`released`・`unjudged <reason>`・跨版でない）。
  5. **止め**: 未仕分けのうち控えに無い（未告の）ものが 1 つ以上在る周は、控えに告げた ts と `block` を足してから rc 2・stdout 0 byte・stderr 1 行（その session の未仕分けの ts を全部・`<NAME> utterance sort <ts> --as request --memo <id>` / `--as chat`・答えは `<NAME> seat ruling bind`・中身は `<NAME> utterance show <ts>`・逐語を運ばない）。打刻しない（Busy のまま）。差し込みの記録を書かない。控えを書けない周は止めない（約束 8 の told-unwritable）。
  6. **未告 0**: 告げ済みだけか未仕分け 0 の周は今のまま打刻・rc 0・0 byte。
  7. **再入**: `stop_hook_active` が真の周は止めない。控えの最後の記録が `block` の周だけ Idle を打って `released` を足す（器の止めの続きの終わりで席を Idle に戻す・`stamp.rs` の再入の沈黙はこの周だけ外す）。ほかは今のまま黙る。
  8. **読めない周（fail-open）**: `TurnEndUnjudged`（session・reason）を記帳して今のまま打刻・rc 0。reason は閉じた 5 語: no-session（payload に session_id が無いか path に使えない字を持つ）・no-start（開始の位置が無い・本行の着地の前に始まった session）・log-unreadable・told-unreadable・told-unwritable。同じ session の同じ語は控えで 1 度だけ記帳する。控えを持てない・読めない・書けない 3 語（no-session・told-unreadable・told-unwritable）は控えで重ねを判じられないので毎回記帳する（記帳を落とすより重ねる側へ倒す）。
  9. **極性一覧**: 極性一覧の末尾に 1 行（guard=turn-end-block・in-loop・fail-open）。集計の pin と snapshot は、本行を撃つ時の main の値から in-loop と fail-open を 1 ずつ増やす（本 doc の行 j も一覧に 1 行を足すので、字の値を本 § に書かない）。
- 歯（接頭辞 hook_unsorted_stop_ と polarity_lists_turn_end_block_・`git grep -c` はどちらも 0 件。素の turn_end_ は seat/tick.rs の歯の名の部分文字列なので使わない）:
  - e2e（`crates/scribe2-boundary/tests/e2e/hook/session.rs`・module doc の接頭辞の列に 1 つ足す・新しい e2e の file は作らない。発話は UserPromptSubmit（行 g）、仕分けは `utterance sort`（行 i）、開始の位置は session-start で作る）:
    - (a) 未仕分け 2 つの session の Stop が rc 2・stdout 0 byte・stderr 1 行に 2 つの ts と口の名・逐語の一意の字が無い。2 度目の Stop（再入でない）は rc 0・0 byte。3 つ目の発話を足すと止まって 3 つの ts を並べる。同じ log の別 session の未仕分け 2 つでは止まらない。
    - (b) request・chat・答え（裁定 event の発話の ts の key）で仕分けた発話だけの session は止まらない。
    - (c) 独立 socket の登録された席で、止めた周の state.jsonl の最終行が busy・差し込みの記録が 0 行増、続く再入の Stop で idle、止めていない再入は黙る。
    - (d) 5 語の各 fixture で rc 0・`TurnEndUnjudged` 1 件（reason が一致）。同じ fixture で Stop をもう 1 度撃つと、控えを持てる 2 語（no-start・log-unreadable）は増えず、控えを持てない・読めない・書けない 3 語（no-session・told-unreadable・told-unwritable）は 1 件増える（約束 8）。
    - (e) session の前に 10 MB と 20 MB の埋め草の event を置いた 2 つの log で、どちらも止まり、`sh -c` で起こした子の読みの byte（親の /proc の io の rchar・wait の後に子の分が親へ積まれる）が等しく、`--bd` の偽の client の記録が 0 行（台帳の読み 0）。
    - (f) marker の無い repo と、発話の無い session（runner の形の payload）は、止めず event も書かず、その session の置き場に控えの file が無い（約束 3 の控えも書かない・発話の無い session に控えを作る実装を落とす）。同じ歯で対象の session が止まることも確かめる。
    - (g) 1 度目の SessionStart の後に未仕分けの発話を 1 つ書き、2 度目の SessionStart（resume の形）を撃つ。開始の位置は上書きされず、次の Stop がその発話で止まる（上書きする実装は 2 度目の位置から読んで止めない）。
    - (h) session_id の無い SessionStart の後、state dir の session の置き場の下に file が 0 本（約束 2 の書かない）。同じ歯で session_id を持つ SessionStart が開始の位置を 1 つ書くことも確かめる。
  - lib（行 k の `+` の file の in-file・接頭辞 hook_unsorted_stop_）: 控えの読み書き・再入の表・reason の語の閉じた列。
  - e2e（`crates/scribe2-boundary/tests/e2e/polarity.rs`・接頭辞 polarity_lists_turn_end_block_）: 一覧に `guard=turn-end-block timing=in-loop on-failure=fail-open` の 1 行が末尾に載る。
  - 極性の既存の歯を直す（直した歯は base で落ちるので retroactive の札は付けない）: `tests/e2e/polarity.rs` の数の pin を持つ既存の歯 3 本（`runner_question_guard_is_in_loop_fail_open` の in-loop と fail-open・`polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed` の in-loop と guards・`polarity_lists_contract_table_as_a_post_hoc_fail_closed_guard` の guards と post-hoc） を、本行を撃つ時の main の値から in-loop と fail-open と guards を 1 ずつ増やし、外形の snapshot（`polarity_external_form`）を 1 行分直す。集計の歯 `polarity_summary_counts_match_lines` は行を数えるだけで数の字を持たないので直さず、緑のまま。
  - 既存: `seat_state_hook_stamps_three_events_into_state_jsonl`・`seat_state_hook_stays_silent_when_it_cannot_stamp` が緑のまま。
- base で RED の理由: base の Stop は判定を持たず rc 0 で打刻する（機能不在）。極性の歯は行が無い（機能不在）。各歯は base で赤の断言を 1 つ以上持つ（10 / 20 MB の歯は止まることも断言する）。
- 触らない: hooks.json・UserPromptSubmit の分岐（行 g）・仕分けの判定（行 i）・doctor の未仕分けの行と tick の alarm（束 E の後の行）・局面の出力（束 E の W4）・inject の記録。`EventKind` と `Case` の match の arm を `+` の file に書かない（== と構築だけ）。
  - `EventKind` は dispatcher.md 行 a・行 ap の touches に在り、本行の `+` の file が match の arm で名指すとその行の閉包に入って、現物の契約表の閉包の検査（gate の共通の verify の歯 `contract_closure_ext_real_table_has_zero_findings` と同じ `contracts check`）が赤になる。器の閉包の検査が測るので verify に置く: 便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (9) の歯にする（歯の e2e を verify に置くと歯の file を write-set に載せて行どうしが交差で直列になるので、command の 1 行にする・dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた）。`Case` は touches に無いので done に載せない。
- 限界: 長い session ほど読む量が伸びる（開始の位置から末尾まで・NFR5 は log の大きさに依らない読みだけを求める）。判定の wall time は記録しない。再入の周は止めない（続きの間の新しい発話は次の turn の終わりで告げる）。他の plugin の Stop hook が同じ周に止めると、器の再入の Idle が早すぎうる。ts が多いと stderr の 1 行が長い（FR88 は全部を求める）。
- 却下: stdout の JSON の decision で止める（rc 2 と stderr で足り、PreToolUse の deny と同じ形）／止めの行を入力欄へ差し込む（AC58 の差し込み 0）／周ごとの読みの続きの位置を控えに持つ（状態が 2 つになる・開始の位置で NFR5 を満たす）／再入でも止める（Claude Code の続きの輪の防ぎと逆）／再入の打刻を今のまま黙らせる（止めた席が次の prompt まで Busy に残り、管理 tick が合図を送れない）／対象の判定を行 g の判定の関数に問う（発話の有無で同じ集合になり、読みが 1 つ減る）。
- 自分を締め出す順: 行 i（仕分けの口）と行 j の着地と binary の入れ替えの後（depends と台帳の依存）。先に着くと止められた席が仕分けできない。
- 着地の後: binary を入れ替えた後、orchestrator の席で 1 度、未仕分けの発話を残して turn を終え、止めの行が model に届くこと・差し込みと数えられないこと・仕分けの後の turn の終わりで Idle に戻ることを実物で確かめ、bead の notes に残す（ADR-0087 が設計 doc に置いた確かめ）。

## 13. 問いの起票が発話より後の結びを doctor の 1 行で数える — asked の値ごとの件数と裁定 id と、asked の無い結び（契約表の行 l・FR89・AC59・ADR-0087）

やさしく言うと: 席が user の答えを受けた後で「問い」を台帳に立てて結んだ件数を、問いのきっかけ（席が聞いた・user の指示を受けた）ごとに数えて見せる。席がチャットだけで聞いて後から問いを立てる癖を数で見えるようにするためで、止めはしない。

- 何が起きているか（main b028af03・verified）:
  - doctor の裁定の行は `crates/scribe2/src/seat/ruling.rs` の `doctor_lines`（pub・境界の crate の doctor が呼ぶ）で、今は run 無しの裁定 event と rules 行の ruling の分の突合 `rulings=<n> rule-rulings=<m>/<of> …` の 1 行。数えるものが無い周は行を出さず、log を読めない周は `unreadable` を名乗る。
  - fleet-event-log の行 h（bind）は `seat ruling add` を消し、突合の行は古い行を読むだけで残し、裁定 event（`RulingReceived`）に key ruling・utterance（発話の ts）・channel・question_ts（問いの起票の時刻）・asked（metadata に在る周だけ・seat か user）を足す。本 doc の行 j（答えの口）の裁定 event も同じ key を持つ。
  - 時刻の読みは `fleet::epoch_of`（pub の再輸出・秒の形だけを読む・秒の UNIX 時刻）。発話の ts はミリ秒の形（fleet-event-log §13 約束 4）で epoch_of では読めず、行 g が `crates/scribe2/src/fleet/wait.rs` の epoch_of の隣にミリ秒の形だけを読む 1 本を足して fleet から再輸出する（fleet-event-log §13 の限界）。question_ts は台帳の created_at の秒の形の字のまま。
  - 境界の crate（R-C4-5 の上限 316）は doctor の行を library の関数から組むだけ。
- 約束（約束 1〜4 が done と 1:1・約束 5 は arm を書かないことだけを done (5) に載せる）:
  1. **母集団**: key utterance を持つ裁定 event（bind と答えの口の結び）。utterance を持たない古い裁定 event は数えない。
  2. **後の結び**: question_ts が発話の ts より新しい結び。question_ts は `fleet::epoch_of` で、発話の ts は行 g のミリ秒の形の読みで読み、発話を秒へ切り捨てて秒の時刻として比べる（字面で比べない）。同じ秒は後に数えない。
  3. **1 行の形**: `binds=<N> after-seat=<n> after-seat-ids=<id,…|-> after-user=<n> after-user-ids=<id,…|-> asked-none=<n> asked-none-ids=<id,…|->`。asked-none は asked の無い結びを発話との前後に依らず全部数える（FR89）。id は裁定 id を log の順に並べる。
  4. **出さない周と読めない周**: 母集団 0 の周は行を出さない（doctor の外形を動かさない）。log を読めない周は `binds=unreadable` の 1 行。question_ts か utterance を読めない結びは母集団（binds）と asked-none には数え、`after-*` に入れず、行の末尾に `unreadable=<n>` を足す（0 の周は出さない）。
  5. **置き場**: 数えは `seat/ruling.rs` の純関数 1 本で、行は同じ `doctor_lines` から出す（境界の crate を触らない）。`EventKind` の match の arm を書かない（== で比べる）。
     - 数えの置き場は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。境界の crate は行 l の write-set の外で、触る diff は runner の guard が止めるので done にしない（器の門が測る）。
     - `EventKind` は dispatcher.md 行 a・行 ap の touches に在り、`seat/ruling.rs` が match の arm で名指すとその行の閉包に入って、現物の契約表の閉包の検査（gate の共通の verify の歯 `contract_closure_ext_real_table_has_zero_findings` と同じ `contracts check`）が赤になる。器の閉包の検査が測るので verify に置く: 便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (5) の歯にする（歯の e2e を verify に置くと歯の file を write-set に載せて行どうしが交差で直列になるので、command の 1 行にする・dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた）。
- 歯（接頭辞 doctor_asked_after_・`git grep -c` は 0 件）:
  - e2e（`crates/scribe2-boundary/tests/e2e/seat/ruling.rs`・裁定 event を log へ直に書いた fixture）:
    - (a) asked が seat と user でそれぞれ発話の前と後に起こした問いの 4 結び・asked の無い後の結び 1・asked の無い前の結び 1・utterance を持たない古い裁定 1 で、doctor の行が `binds=6 after-seat=1 after-seat-ids=<id> after-user=1 after-user-ids=<id> asked-none=2 asked-none-ids=<id>,<id>` と一致する。asked-none の 2 つの結びは、log の順が裁定 id の字の順と逆になるように置く（字の順に並べる実装を落とす・約束 3 の log の順）。発話の ts は全てミリ秒の形（行 g の形）で置く（発話の ts を epoch_of で読む実装は全部を unreadable に落として一致しない）。
    - (b) 母集団 0 の置き場（古い裁定だけを含む）は行を出さない。
    - (c) log が読めない置き場は `binds=unreadable`。
    - (d) asked が seat で question_ts が時刻として読めない結びを 1 つ足すと、binds が 1 増え、`unreadable=1` が末尾に付き、after-* と asked-none の数は変わらない。utterance が時刻として読めない結びを 1 つ足した形でも同じく `unreadable=1`。
    - (e) asked が user の後の結びを持たない置き場（seat の後の結び 1・asked の無い結び 1）で、行が `after-user=0 after-user-ids=-` を持つ（約束 3 の無い列の `-`）。
  - lib（`seat/ruling.rs` の in-file・接頭辞 doctor_asked_after_）: 秒より下の桁を持つ発話の ts と秒までの question_ts の比べ・同じ秒は後に数えない・読めない時刻の unreadable。
- base で RED の理由: base の doctor に `binds=` の行が無い（機能不在）。lib は該当 0 本。
- 触らない: bind と答えの口の書き（fleet-event-log の行 h・本 doc の行 j）・起票の門の asked の検査（ledger-form §14）・境界の crate・pipe report。
- 限界: id の列は log の全期間の結びを並べ、長くなりうる（窓で絞らない・FR89 は件数と id を求める）。asked の値の正しさは判じない（ADR-0087）。答えの口の結びは問いが先に在るので後の結びに入らない。
- 却下: 発話の ts と question_ts を字面で比べる（秒より下の桁と桁の数で順が崩れる）／境界の crate に行を組む（R-C4-5）／行を常に出す（数えるものの無い置き場の doctor の外形が動く・既存の先例と逆）。
- 自分を締め出す順: fleet-event-log の行 h と本 doc の行 j の着地の後に走る（depends と台帳の依存）。
- 跨版の面: doctor の text の新しい key 7 つ（binds・after-seat・after-seat-ids・after-user・after-user-ids・asked-none・asked-none-ids）と unreadable=。着地の前に doctor の key を読む消費側の面へ 1 行知らせる。

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
write-set = ["+crates/scribe2/src/utterance.rs", "+crates/scribe2/src/utterance/cli.rs", "crates/scribe2/src/lib.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_graph_doctor_external_form.snap", "=crates/scribe2-boundary/tests/e2e/rules.rs", "=crates/scribe2-boundary/tests/e2e/seat.rs", "=crates/scribe2/src/ledger/mod.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail utterance_sort_", "cargo nextest run -p scribe2 --lib --no-tests=fail utterance_sorted_of_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --bin scribe2 --no-tests=fail doctor_external_form"]
size = "M"
growth = ["crates/scribe2/src/utterance.rs:210", "crates/scribe2/src/utterance/cli.rs:150", "crates/scribe2/src/lib.rs:1", "crates/scribe2-boundary/src/main.rs:2", "crates/scribe2/src/help.rs:12"]
done = "(1) core に最上位の module 1 つと cli を足し、lib.rs と境界の main.rs に utterance を 1 つずつ (2) 仕分け済みかは IO の無い純関数 1 本が未仕分け・会話だけ・結びありの 3 値で決め、要望か答えが在れば会話を数えない (3) sort --as request --memo が開いた memo の周に UtteranceSorted request を 1 件書き、台帳を書かず、同じ組は already (4) sort --as chat が台帳を読まずに 1 件書く (5) no-utterance・linked・ledger-unreadable・not-memo をこの順に何も書かずに rc 1・stdout 0 byte・stderr 1 行で断る（no-utterance は request と chat の両方・linked は要望か答えを持つ発話への会話） (6) show が 1 件の逐語だけを返し、無い ts は no-utterance (7) 両口は read_all で読む (8) 最上位の使い方と help の表に utterance の sort と show（最上位の使い方を写す既存の cli_help_ の 4 本と外形 snapshot 5 本を直す） 歯: utterance_sort_ が要望と会話の 1 件ずつと台帳の不変・読めない台帳でも会話が書けて bd の呼び出し 0、1 発話 2 memo、断り 7 形（no-utterance は request と chat の 2 形・linked は答えと要望の 2 形）の rc 1 と stderr の逐語と不変、2 つの断りに当たる 2 形の no-utterance、同じ秒の 2 発話、show の逐語の一致と 3 発話の真ん中の弁別と no-utterance、already の rc 0 と stdout の 1 行と不変、ledger-unreadable の rc 1 と不変、2 MB の後ろの発話の sort と show を、utterance_sorted_of_ が 3 値の表と会話の外しと札の並びに依らない結びと同じ入力の同じ結果を、cli_help_ が頂点の語 16 と突き合わせ 10 と使い方の行を、rules_external_form と doctor_external_form が使い方の行を写す外形を測る。base は utterance が使い方の誤りで RED"

[[contract]]
id = "j"
title = "裁定面の答えの口 seat ruling answer — 問いの id と標準入力の逐語を受け、経路 gui の発話 event と bind と同じ書きを 1 周で行って裁定 id を 1 行で返し（承認 event は書かない）、席の道具の呼び出しからの撃ちは hook の入口が実行の前に断る（ADR-0087）"
req = ["FR82", "FR88"]
section = "11"
depends = ["i"]
write-set = ["+crates/scribe2/src/hook/answer_mouth.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/src/seat/ruling.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/help.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "=crates/scribe2-boundary/tests/e2e/seat.rs", "=crates/scribe2-boundary/tests/e2e/main.rs", "=crates/scribe2/src/fleet/store.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_answer_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_answer_mouth_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_answer_mouth_deny_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_choice_question_deny_first_right_before_write_set_guard", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_question_guard_is_in_loop_fail_open", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_contract_table_as_a_post_hoc_fail_closed_guard", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_command_guard_as_in_loop_fail_closed_right_after_cap_guard", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_pages_match_the_live_form_and_every_subcommand"]
size = "M"
growth = ["crates/scribe2/src/hook/answer_mouth.rs:150", "crates/scribe2/src/hook/mod.rs:4", "crates/scribe2/src/polarity.rs:12", "crates/scribe2/src/seat/ruling.rs:90", "crates/scribe2/src/seat/cli.rs:25", "crates/scribe2/src/help.rs:2", "crates/scribe2/src/fleet/cli.rs:1"]
done = "(1) seat ruling answer --repo --state-dir --question [--bd] が標準入力の逐語を 1 byte も変えずに受け、使い方は < WORDS で示す (2) 空白だけ・台帳を読めない・閉じた問い・問いでない bead を、この順に何も書かず（発話 event も）rc 1 で断り、新しい語は words-empty だけ（行 h の結びの閉じた 4 語の列は xtask check の enum-slices が測る） (3) 通る周は経路 gui の発話 event を行 g の store の追記の 1 本で一意の ms の ts に書いてから、その ts で行 h の結びの 1 関数を呼び（経路は結びが発話 event から読む）、後半のどの失敗の周も partial utterance=<ts> で rc 1 (4) stdout は裁定 id の 1 行だけ (5) 承認 event を書かない (6) hook の子 module が choice の門の直後に Bash の command を segments で読み、引用の外の seat ruling answer の並びと、shell か eval の語の中の seat ruling answer を、役割と pane に依らず rc 2 で断る (7) 極性一覧に answer-mouth-deny（in-loop・fail-closed）を choice-question の直後に 1 行 (8) 使い方と help の表に ruling answer を足す 歯: seat_ruling_answer_ が通る周の 4 つの書きと裁定 id の 1 行と承認 0 件と標準入力の byte の一致、断りの 4 語の全部（読めない台帳は偽の bd の壊れた JSON）で何も書かないこと、2 つの断りに同時に当たる 3 形で先の語だけが出る順、close の中で event log が塞がれた周の partial utterance=<ts> の rc 1 と残る notes と close と同じ ts の bind での結び直し、append-notes の失敗の周の partial utterance=<ts> と残る発話と撃たれない close、WORDS で逐語を受ける使い方が 1 行だけ（母集団は全部の使い方の行）を、hook_answer_mouth_ が止まる 8 形（eval・前の代入・cargo run の前置きを含む）の不実行と stderr の bind の案内と inject.jsonl の 1 行と、通る 3 形と pane の無い session の断りを、polarity_lists_answer_mouth_deny_ が一覧の 1 行と choice-question-deny の直後の位置を、直した極性の歯 5 本と外形 snapshot が数と並びを、seat_usage_external_form と cli_help_pages_match_the_live_form_and_every_subcommand が使い方と help の表を測る。base は answer が使い方の誤りで、素の撃ちが門を通って RED"

[[contract]]
id = "k"
title = "turn の終わりの止め — SessionStart で session の開始の位置を 1 度だけ残し、そこから読んだ未仕分けの発話のうち未告のものが在る Stop を 1 度だけ rc 2 で止めて ts を全部並べ、Idle を打たず、再入で Idle に戻し、読めない周は TurnEndUnjudged を 1 度記帳して通す（hook の子 module・極性一覧に 1 行・§12）"
req = ["FR88", "NFR5"]
section = "12"
depends = ["i", "j"]
write-set = ["+crates/scribe2/src/hook/turn_end.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/hook/stamp.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2-boundary/tests/e2e/hook/session.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "=crates/scribe2-boundary/tests/e2e/hook.rs", "=crates/scribe2/src/fleet/store.rs", "=crates/scribe2/src/fleet/event.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_unsorted_stop_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_unsorted_stop_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_turn_end_block_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_state_hook_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_summary_counts_match_lines", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_question_guard_is_in_loop_fail_open", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_contract_table_as_a_post_hoc_fail_closed_guard", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_external_form", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
growth = ["crates/scribe2/src/hook/turn_end.rs:280", "crates/scribe2/src/hook/mod.rs:8", "crates/scribe2/src/hook/stamp.rs:15", "crates/scribe2/src/polarity.rs:8", "crates/scribe2-boundary/tests/e2e/hook/session.rs:380", "crates/scribe2-boundary/tests/e2e/polarity.rs:30"]
size = "L"
done = "(1) SessionStart が session の置き場に event log の長さを開始の位置として書き、既に在れば上書きせず、session_id の無い周は書かない (2) 判定は台帳を読まず開始の位置から log の末尾を読み、その session の発話を行 i の純関数で未仕分けと判じ、発話の無い session は打刻だけで event も控えも書かない (3) 控えは session の置き場の 1 file で told・block・released・unjudged の 4 形 (4) 未告が在る周は控えを書いてから rc 2・stdout 0 byte・stderr 1 行（未仕分けの ts を全部・仕分けの口・bind・show の名・逐語なし）で、打刻も差し込みの記録もせず、控えを書けない周は止めない (5) 未告 0 の周は今のまま打刻・rc 0・0 byte (6) 再入は止めず、控えの最後が block の周だけ Idle を打って released を足す (7) 読めない周は TurnEndUnjudged を reason の 5 語（no-session・no-start・log-unreadable・told-unreadable・told-unwritable）で記帳して打刻・rc 0 で、控えを持てる 2 語（no-start・log-unreadable）は session ごとに 1 度、控えを持てない・読めない・書けない 3 語は毎回 (8) 極性一覧の末尾に turn-end-block（in-loop・fail-open）が 1 行で、数の pin を持つ既存の極性の歯 3 本と snapshot を 1 行分直す 歯: hook_unsorted_stop_ の e2e が止めと stdout 0 byte と 2 度目の通過と 3 つ目での再度の止め・別 session の不止め・仕分け済みの不止め・busy の保持と差し込み 0 と再入の idle・5 語の記帳と、2 度目の Stop で控えを持てる 2 語は増えず 3 語は 1 件増えること・10 MB と 20 MB での止めと rchar の一致と台帳の読み 0・marker の無い repo と発話の無い session の不止めと控えの file の不在・2 度目の SessionStart の不上書き（1 度目と 2 度目の間の未仕分けで止まる）・session_id の無い SessionStart の置き場の file 0 本を、lib が控えの読み書きと再入の表と語の列を、polarity_lists_turn_end_block_ が一覧の末尾の 1 行を、直した極性の歯 3 本と polarity_external_form が数と外形を測る。既存の seat_state_hook_ と polarity_summary_counts_match_lines が緑。base は Stop が判定を持たず rc 0 なので RED (9) hook の子 module turn_end.rs は EventKind の match の arm を書かず、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "l"
title = "doctor の 1 行 — utterance を持つ裁定 event を母集団に、問いの起票が発話より後の結びを asked の値（seat・user）ごとの件数と裁定 id で、asked の無い結びを別に数える（seat/ruling.rs の純関数・境界の crate は不変・§13）"
req = ["FR89"]
section = "13"
depends = ["j"]
write-set = ["crates/scribe2/src/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "=crates/scribe2/src/fleet/wait.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail doctor_asked_after_", "cargo nextest run -p scribe2 --lib --no-tests=fail doctor_asked_after_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
growth = ["crates/scribe2/src/seat/ruling.rs:110", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs:130"]
size = "S"
done = "(1) 母集団は utterance を持つ裁定 event で、古い裁定 event を数えない (2) 後の結びは question_ts が発話の ts より新しい結びで、question_ts を fleet::epoch_of・発話の ts を行 g のミリ秒の形の読みで読み、秒へ切り捨てて比べ同じ秒は数えない (3) doctor の行が binds・after-seat・after-seat-ids・after-user・after-user-ids・asked-none・asked-none-ids の順の 1 行で、asked-none は前後に依らず数え、id は log の順・無ければ - (4) 母集団 0 の周は行を出さず、log を読めない周は binds=unreadable、時刻を読めない結びは binds に数えて after-* に入れず unreadable=<n> を足す 歯: doctor_asked_after_ の e2e が 7 件の fixture の行の一致（asked-none の id を字の順と逆の log の順に置いた形）・母集団 0 の不出力・読めない log の unreadable・読めない question_ts と読めない utterance の unreadable=1・無い id の列の - を、lib が秒より下の桁の比べと同じ秒の不算入と読めない時刻を測る。base は binds= の行が無いので RED (5) seat/ruling.rs は EventKind の match の arm を書かず（== で比べる）、verify の最終行の contracts check が便の木で findings 0"
<!-- contracts:end -->
