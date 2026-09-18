# 設計: 対話面の作法 — planner が user に向けて出す文の形は設計 doc 1 本が持ち、生成文は pointer 付きの行で運ぶ

- 要件: [FR30](../../design-intent/spec/srs.html#FR30) 便の配送構造（planner が唯一の対話面）/ [FR44](../../design-intent/spec/srs.html#FR44) 席間の連絡（不変）。「対話面の作法」を名指す FR は要件書の改訂（user の手番）で足す。制約: CON2（PUBLIC・user の逐語を tracked file に書かない）
- 憲法: [C7](../../design-intent/spec/constitution.html#c7) 対話面は 1 つ / [C10](../../design-intent/spec/constitution.html#c10) 宣言・実測・導出を型で分ける（信頼度の語の根）/ [C14](../../design-intent/spec/constitution.html#c14) 規律は文書と manifest の 2 面 / [N2](../../design-intent/spec/constitution.html#n2) prose だけの規則は規則でない / [A1](../../design-intent/spec/constitution.html#a1) 3 クラスの承認
- 決定: [ADR-0032](../../design-intent/decisions/ADR-0032-dialogue-surface-rules-live-in-one-design-doc-and-planner-brief.html)（本 doc の置き場・9 則の形・5 slot・global の行き先）/ [ADR-0031](../../design-intent/decisions/ADR-0031-working-memory-is-held-by-the-vessel-directives-status-and-hooks.html) §2.2 §2.6（brief の材料 = 器の DATA）/ [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html) §2.4（雛形の行の規律）
- 土台: [seat-roles.md](./seat-roles.md) §5（注入・雛形の行は穴か pointer 付き・xtask の検査）/ [working-memory.md](./working-memory.md) §12（現在地の DATA・直命の表）
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
| 6 | **状態を言い直す** | 毎 turn 現在地を 1〜2 行（器の `[MAIN]` `[RUN]` `[SEAT]` 由来）。user に「覚えておいて」を頼まない（直命の表が持つ） | i-have-adhd 則 5 / ADR-0031 §2.1 §2.2 |
| 7 | **成果を見せる** | 前 session からの Landed を id と sha で（器の `[WIN]`）。作文で膨らませない | i-have-adhd 則 7 |
| 8 | **error は事実だけ** | 原因・修正・出所（bead / run id / file:line）。感嘆・謝罪・「問題があるようです」を持たない | i-have-adhd 則 8 / global「推測で答えず」 |
| 9 | **一覧は 5 件・前置きと締めなし** | 表示は 5 件まで・母集団の件数を併記・全件は file へ落として path を返す。前置き（「〜します」の宣言）・要約の反復・締めの挨拶を持たない。「詳細版で」と言われたら本文の長さの上限を外す（形は保つ）。平易に書く＝提示層だけ易しく、思考・解・code の技術水準は下げない | i-have-adhd 則 9 / 則 10 / global「平易」「ガードレール」 |

外した 2 則と理由: **時間見積**（則 1 に吸収）・**「不確かなら適用」**（生成文が固定するので自己判断の余地を持たない・断定の温床）。

採った例外（When to break）: 破壊的操作は A1 の 3 クラスが上位 / **debug spiral** = 3 周「まだ壊れている」なら手を止め、疑う前提を 1 つ名指して 1 問聞く / 曖昧 = 1 論点 1 質問・推奨 1 つ（順序や是認だけを求める問いは出さない）/ 規則と課題が衝突したら課題が勝ち形は保つ（「選択肢は？」には 2〜4 案を推奨先頭で）。

pushback: user の訂正を即座に受け入れず根拠を検討し、誤っていれば則 1 の語を付けて反論する。解釈が分岐するなら明示的に提示し、黙って 1 つを選ばない（ADR-0032 §2.2・C7）。

## 3. 復元の brief（user 面 5 slot・宣言順・各 1〜2 行）

| slot | 中身 | 材料 |
|---|---|---|
| 1 `next` | user の手番（承認・裁定・入力・`[DIRECTIVE-REVIEW]` の確認候補）。無ければ planner の次の行為 | DATA + 計画弧 |
| 2 `wins` | 前 session からの Landed（id・sha） | `[WIN]` |
| 3 `status` | main の sha と同期・走行中の便（id・段・口座）・席の状態 | `[MAIN]` `[RUN]` `[SEAT]` |
| 4 `plan` | 進行中の計画の上位 3 件（bead id）+ 母集団（open / in_progress / blocked） | 計画弧 + `[BD-COUNT]` |
| 5 `risks` | 乖離・orphan・危険。無ければ「なし」・判定不能はその理由（marker 名） | `[DIFF]` `[ORPHAN-WM]` `[WM]` |

AI 面（pointer の kind / resolution・3 値則・consume の可否・暫定行の扱い）は context に在って表示しない。brief の後の consume は不変（ADR-0018 §2.1）。

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
| タスク開始時（git fetch / status） | **hook / DATA が代替** | SessionStart の hook と `[MAIN]`（ADR-0031 §2.2） |
| ファイル編集後（commit → push・worker cell 例外 5 面同文） | **縮小**（git skill の 1 行） | worker cell は前の版の遺物。scribe2 は 1 bead = 1 PR・pipeline |
| multi-agent 実行（v1 骨格の不使用・model / budget 明示・file 出力） | **器の作法へ** | §4 の 4 行目（fan-out の 3 条件）。v1 骨格・cld-spawn の記述は撤去 |
| 破壊的操作の禁止（tmux / git の hook block） | **残す** | host の hook が SSOT。器の guard へ移すのは別件（N1） |
| 記憶（auto-memory のみ・旧 MCP の廃止） | **縮小**（1 行） | 経緯は撤去。教訓の carrier は [working-memory.md](./working-memory.md) §12 |
| ホスト・コンテナ（編集 = host / test = container） | **残す** | host 固有 |

見込み（deduced）: global 52 行 → host 固有 3 節 + git skill の 1 行。器は consumer の repo にも global にも書かない（ADR-0022 §2.4）＝痩身は global を持つ repo の便。

## 6. 歯（`crates/<NAME>/tests/e2e/hook.rs` の `hook_brief_` / xtask の `check`・名前の列は現物が SSOT）

- 雛形の行: pointer の無い行 0・穴 ⊆ 定義済み（既存の xtask の検査・行が増えても検査は不変）。生成文の外形 snapshot（`hook_brief_planner`）が 4 行分動く（C12.5）。
- 作法の遵守そのものは歯にしない（機械が測れない・ADR-0032 §4）。brief の 5 slot は skill の手順で、snapshot も歯も持たない。

## 7. 契約（3 便）

- **(g)** planner の雛形に §4 の 4 行を足す + 外形 snapshot（S・docs-adjacent・base で RED = snapshot の 4 行不在を名指す歯 1 本）。write-set = `seat/brief/planner.txt` + `tests/e2e/snapshots/e2e__hook__hook_brief_planner.snap` + 名指す歯の file。依存: ADR-0032 land。
- **(h)(i)** 判断層の skill 2 本の縮小と global の配備替えは**超過した**: skill も退避 / 復元の口も `s2-07l.479.2` で消えた（ADR-0045 §2 (2)・[working-memory.md](./working-memory.md)）。

## 8. 却下案（ADR-0032 §5 の写しは持たない・設計固有のもの）

- 5 slot を器が全文生成する: slot 1 と 4 は計画弧（AI の判断）を要し、器は事実しか持たない（ADR-0018 §2.1 の線）。器は材料（DATA）を出し、組むのは skill の手順。
- 作法の遵守を rubric（Correctness / Autonomy / …）で lens に採点させる: 採点の値が新しい閾値になり、機械が enforce できない値を規則の表に入れる圧力になる（ADR-0032 §5 (D)）。作法は生成文の pointer で運び、違反は user の訂正（直命の表）で戻す。
- 雛形の 4 行を admin にも足す: admin は user と話さない（ADR-0016 §2.1・relay のみ）。

## 9. 後続

- 「対話面の作法」を名指す FR と AC（user の /folio-architect）。
- global の痩身の後、host 固有の残り（破壊的操作の hook・提示面）を器の guard / report の口へ移すか（別の ADR）。
- runner / lens の出力の形（headless）は本 doc の対象外＝要るなら別 doc。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "g"
title = "planner の雛形に §4 の 4 行を足す（pointer 付き・規範文 0）+ 外形 snapshot"
req = ["FR30", "FR67"]
section = "4"
also = ["crates/scribe2/src/seat/brief/planner.txt", "crates/scribe2/tests/e2e/snapshots/e2e__hook__hook_brief_planner.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_brief_planner_"]
size = "S"
done = "planner の生成文に dialogue-surface.md §2 を指す行 2 本と §3 / §5 を指す行 1 本ずつが → SSOT: 付きで在り、admin の生成文には無い"

[[contract]]
id = "h"
title = "planner の雛形に §4 の 5 本目（裁定を受けた turn で seat ruling add を撃つ・pointer 付き・規範文 0）を足す + 外形 snapshot"
req = ["FR41", "FR67"]
section = "4"
depends = ["g"]
also = ["crates/scribe2/src/seat/brief/planner.txt", "crates/scribe2/tests/e2e/snapshots/e2e__hook__hook_brief_planner.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_brief_planner_"]
size = "S"
done = "planner の生成文に fleet-event-log.md §9 と ADR-0037 を指す裁定の行が → SSOT: 付きで 1 本増え、admin の生成文には無く、既存の 4 行は不変"
<!-- contracts:end -->
