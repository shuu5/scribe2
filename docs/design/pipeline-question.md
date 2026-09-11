# 設計: pipeline の QUESTION の口 — runner は契約の不足を typed な質問 record で返して止まり、席が planner へ中継し、回答の記帳で便が再開する

- 要件: [FR5](../../design-intent/spec/srs.html#FR5) runner / lens は claude -p の 1 形（stdout に構造化 1 行）/ [FR4](../../design-intent/spec/srs.html#FR4) 契約 1 本に worktree 1 つ / [FR15](../../design-intent/spec/srs.html#FR15) 3 クラスは spawn の手前で止まる / [FR16](../../design-intent/spec/srs.html#FR16) 逐語付きの記帳で resume / [FR22](../../design-intent/spec/srs.html#FR22) 人由来の event は approval 以外 0 件（**本設計は FR22 を変えない**）。[FR30](../../design-intent/spec/srs.html#FR30) 配送構造 / [FR31](../../design-intent/spec/srs.html#FR31) 質問の停止 / [FR32](../../design-intent/spec/srs.html#FR32) 質問の再開 / [AC10](../../design-intent/spec/srs.html#AC10)（SRS v0.4・2026-09-12 反映）。
- 憲法: [C2](../../design-intent/spec/constitution.html#c2) 新しい理由は新 variant 1 つ・適用順は宣言順 / [C3](../../design-intent/spec/constitution.html#c3) C3.3 席の状態は typed・C3.4 完了条件は 1 enum（**`Completion` は増やさない**）/ [C7](../../design-intent/spec/constitution.html#c7) 承認は 1 つの対話面・承認 event に逐語 / [C11](../../design-intent/spec/constitution.html#c11) C11.2 境界ごとの enum が極性を運ぶ / [C16](../../design-intent/spec/constitution.html#c16) 逸脱は edit time で止める。
- 決定: [ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html)（既定の配送構造・質問 record・回答の権限・accepted）/ [ADR-0004 §2.1 / §2.5](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html#s2-5-schema-version)（追記だけの event log・schema 版の規則）/ [ADR-0012](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html)（record は専用種別・未知は claude の rc へ）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)（極性一覧）。
- 探索: [RESEARCH-delivery-structure](../../design-intent/research/RESEARCH-delivery-structure.html)（席並列 / pipeline 並列 / 混成の比較・2026-09-11 の実測）。
- 前提の設計: [pipeline.md](./pipeline.md)（stage・event・subcommand・runner / lens の起動形）/ [seat-autonomy.md](./seat-autonomy.md)（席の tick・inject）/ [polarity.md](./polarity.md)（Guard の足し方）。
- **契約の起票**: SRS v0.4（FR30 / FR31 / FR32 / AC10）が反映済みなので planner が起票する（本 doc の便では切らない）。契約の割り: (a) core の段・event・`pipe answer`・`resume` の分岐・spawn の runner stdout 捕捉（fake runner で 質問 → Questioned → answer → resume → Implemented を歯で通す）/ (b) runner の包みの質問 record の読みと rc・prompt template の改訂 / (c) 実 run 1 本（toy repo・AC10 候補）。席の中継（§6）は core の契約ではなく席の手順。

## 1. 何を解くか

runner（claude -p）は契約と worktree の中身だけで判断し、聞き返さない（`runner.txt`「対話しない」）。契約に穴があると、pipeline では gate の FAIL か黙った推測として現れる。席が自分の手で実装した 2026-09-11 の便では、同じ穴が planner への QUESTION 9 件（うち契約側の誤り 6 件）として上がり、契約を直してから進めた（research §2）。

**解くこと**: runner が「契約のここが足りない」を **typed な record** で返して止まり、席が planner へ中継し、planner が契約を直す（または回答を逐語で記帳する）と、同じ run が再開する口を作る。3 クラスの承認（`Blocked` → `pipe approve` → `resume`・FR15 / FR16）と**同じ形**で作る＝新しい機構を増やさず、段と event を足す（C2）。

やさしく言うと: 実装役が「この指示では作れない」と思ったら、途中で勝手に決めるのではなく、決まった形の 1 行で質問を書いて手を止める。係の席がそれを計画役へ届け、計画役が指示を直すと、同じ便がそこから続く。

## 2. 全体の形

```
契約 ──intake──▶ Intake ──spawn──▶ Spawned ──runner──▶ Implemented ──gate──▶ Gated ──land──▶ Landed
                                       │
                                       │ runner（包み）が rc RC_QUESTION で終わり、stdout の最終行が質問 record
                                       ▼
                                  Questioned（QuestionRaised・detail = 質問の逐語）
                                       │ 席が planner へ pointer 1 行を中継（seat inject・席の手順）
                                       │ planner: 契約を直す ／ 回答を記帳する ／ A 系なら契約に classes / 裁定 id を足して承認関門へ
                                       ▼
                              pipe answer --run <id> --words "<回答の逐語>"（QuestionAnswered・actor = machine）
                                       │
                                       └─ pipe resume ──▶ Spawned（同じ run・契約は再読・回答は prompt の「回答」節へ）
```

- **段は 1 つ**（`Stage::Questioned`）、**event は 2 つ**（`EventKind::QuestionRaised` / `QuestionAnswered`）を足す。`Blocked` / `ApprovalRequested` / `ApprovalReceived` と同型で、遷移は `resume` の 1 関数が持つ（[pipeline.md §4](./pipeline.md)）。
- **宣言順の位置**（C2・ADR-0013 §2.2 の宣言順 pin が歯で在る）: `Stage` は `Spawned` の直後（`Intake, Blocked, Spawned, Questioned, Implemented, …`＝流れの順）、`EventKind` は末尾（`…, ApprovalReceived, QuestionRaised, QuestionAnswered`）。const slice と網羅 match が導く。
- **schema 版は 1 のまま**: ADR-0004 §2.5（D-5）が版上げを求めるのは「既存の行が読めなくなる変更」で、kind / stage の値を足しても既存の行は読める。古い読み手が新しい行（`Questioned` / `QuestionRaised`）を `Err` にするのは fail-closed のとおり（跨版の読み手は自分の版の値集合しか受けない）。ADR-0004 §2.1 の「閉じた 8 variant」は記録時点の件数であって上限ではない。
- **質問は行為を止める判定**: 包みの質問 record の読みは guard の定義（行為を止めうる判定・[polarity.md §2](./polarity.md)）に当たる＝runner の便を `Questioned` に倒す。極性は §7。
- **回答の権限**は契約の所有者（planner 席）にある。管理席は中継だけ（[ADR-0016 §2.3](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html#s2-3-answer-authority)）。**user の言葉は承認 event でしか記帳しない**（C7.2・FR22 不変）＝3 クラス・A2・A3・C5 に触れる質問は「回答」で解かず、planner が契約に `classes` や裁定 id を足してから `resume` し、既存の承認関門（`Blocked` → `pipe approve --words`）を通す。

## 3. 質問 record と包みの rc

lens の verdict（最終行の JSON・`{"verdict":…,"evidence":…}`）と**同じ置き場**（claude の最終 `result` の text の最終行）と**同じ parse 関数**を使う。**読む条件は違う**: gate は lens の rc が 0 の周だけ出力を読むが、質問 record は包みが rc `RC_QUESTION` で終わった周にこそ読む（§9 (d)）。

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `question` | string | 必須 | 何が足りないか **1 行**（改行なし・非空）。逐語で `QuestionRaised.detail` に写す |
| `about` | string | 任意 | 契約のどの key に関する質問か（`verify` / `write-set` / `done` / `goal` / `design` の 1 語） |

- runner の prompt template は「対話しない」を「**質問は最終行の record で返して止まる**（それ以外の形で人へ問わない）」に改める。record を出した周は **commit を作らない**（作った周は質問ではなく実装の失敗として扱う＝`Failed`・runner の rc を写す）。
- 包み（`<NAME> runner`）は claude が **正常に終わった後**に最終行を読み、`question` key を持つ JSON 1 つなら **rc `RC_QUESTION` = 76** で終え、同じ record を自分の stdout の最終行に**そのまま**書く（pipeline が読む面）。rc 76 は「包みが終了後に付ける typed な名札」であり、上限の rc 75（`launch` が途中で kill する**実行の中断**）とは意味が違う。claude 自身が 76 で終わる周と衝突しうる弱点は rc 75 と同じで、包みは「record が最終行に在る ∧ 自分が付けた」ときだけ 76 を返す（record が無い・読めない周は claude の rc を写す＝ADR-0012 §2.1 と同じ「未知は claude の rc へ落ちる」）。
- **pipeline の読み**: `spawn` は runner cmd を `sh -c` で起こし stdout を捕らえていない（記録時点は rc だけを `settle` へ渡す）。本設計で runner の **stdout を `ask_lens` と同じ形（piped・`wait_with_output`）で捕らえ**、rc が 76 の周だけ最終行を parse して `QuestionRaised(detail)` を記帳する。rc が 76 で最終行が record でない周は `Failed`（理由 `question-record-missing`・fail-closed）。rc が 76 でない周は最終行を読まない。
- record の細部（複数の質問・選択肢付きの質問・優先度）は本設計の射程外。1 便 1 質問で始め、足りなければ ADR を supersede する。

## 4. 段と event

| stage | 入る event | 出る条件 |
|---|---|---|
| `Spawned` | （既存） | runner rc = 76 ∧ 最終行が record → `SeatStopped` + `QuestionRaised(detail=逐語)` + `RunStage(Questioned)` |
| `Questioned` | `QuestionRaised` + `RunStage` | `QuestionAnswered`（`detail` 非空）が在れば `resume` → spawn（`Spawned`・同じ run）。無ければ `resume` は rc 3 で何も書かない（`Blocked` と同じ・何回聞いたかを積まない） |
| `Failed` | （既存） | 質問 record と commit が同時に在る周・rc 76 で record が無い周 |

- `Questioned` の run に Live 席は無い（runner は畳まれている）＝`pipe stop --all` の母集団（Live 席）に入らず、stop は冪等のまま。
- `fleet export` / `report` は `QuestionRaised` / `QuestionAnswered` を既存の event と同じ行の形で出す（schema 1・key は既存の `KNOWN_KEYS` の内側・`detail` に逐語）。`report` の「人由来の event」の数え方（`ApprovalReceived` 以外を除く）は変えない＝回答は machine 由来。

## 5. subcommand

- `<NAME> pipe answer --run <id> --words "<回答の逐語>"`: `Questioned` の run にだけ受理（それ以外は rc 3・stderr 1 行）。`QuestionAnswered` を `detail=<逐語>` で 1 行 append。**actor は `EventKind::default_actor` が返す `machine` のまま**（`Emit` に actor の seam を足さない・「actor は kind の関数」の不変を保つ・FR22 不変）。**逐語が空なら書かない**（`approve` と同じ）。
- `<NAME> pipe resume --run <id> --runner <cmd>`: `Questioned` からは `QuestionAnswered` が在る周だけ spawn（[pipeline.md §5](./pipeline.md) の `Blocked` 分岐と同型）。runner の prompt には契約（再読・planner が直していれば新しい本文）に加えて「**回答**」節（`QuestionRaised.detail` と `QuestionAnswered.detail` の対）を付ける。
- `<NAME> pipe run`（席が撃つ 1 本）の判定行に `question=<id>` を出す（席の中継の入力・§6）。CLI の外形 snapshot（`e2e__pipe__pipe_external_form.snap`・usage 行）は `answer` の分だけ変わる。
- **A 系の質問**（3 クラス・A2 の閾値・A3 の依存・C5 の裁定 id に触れる）: planner は `answer` で解かず、契約に `classes` を名乗らせる／裁定 id を書く→ `resume` → 既存の承認関門が `Blocked` に倒す → user の逐語は `pipe approve --words` で承認 event に載る（C7.2）。`answer` は契約の読み方・write-set・verify 行の**機械由来**の補足に限る。

## 6. 席の役割（既定の配送構造・ADR-0016 §2.1）

- **planner ×1**: 契約・裁定・merge の go。QUESTION には契約を直すか回答を記帳するかで応え、A 系は §5 の経路で user へ上げる。
- **管理席 ×N**（各 1 口座）: dispatch された bead を `pipe run` で流す・判定行に `question=` が出たら **pointer 1 行**（run id と `about`）を planner へ `seat inject` で送る・go の後に merge する。**自分の手で実装するのは pipeline が回せない便だけ**（器の穴・design-intent・host 作業）。
- **中継は席の手順であって core の結合ではない**: `seat` の module は `pipe` を呼ばず、`pipe` も席へ書かない（記録時点どおり）。席が `pipe run` の判定行（typed な token）を読んで `seat inject --target <planner>` を撃つ。歯は「判定行に `question=` が出る」（契約 (a)）と既存の inject の歯で足りる。planner の入力欄が塞がっている周は inject が `Refused` になる（既存の極性・[ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)）＝**届かなかった事実は判定行に残る**。
- runner / lens は claude -p のまま（FR5・ADR-0009 / 0011）。teammate（Agent tool の subagent）は席の中の review に限る。

## 7. 極性（C11.2・polarity.md §3）

- 新しい Guard variant **1 つ**: `RunnerQuestion`（boundary = runner の包みの record 読み）。`timing = InLoop`（record を読んだその場で便を倒す）/ `on_failure = FailOpen`（record が無い・読めない周は claude の rc へ落ちる。ADR-0012 の上限 record と同じ向き。一覧はこれを隠さない）。
- `pipe answer` は行為を止めない（記帳するだけ）＝guard ではない。`resume` の `Questioned` 分岐は既存の `Blocked` 分岐と同じ「記帳が無ければ進まない」で、既存の承認関門の極性の内側。
- write-set は構造の連鎖で書く: 極性一覧の snapshot（`e2e__polarity__polarity_external_form.snap`）+ `polarity.rs` + CLI 外形 snapshot（`e2e__pipe__pipe_external_form.snap`）+ `spawn.rs`（stdout 捕捉）+ `fleet/mod.rs`（段・event・宣言順）+ `pipe/cli.rs`（`answer`・`resume`）+ `headless/runner.rs`・`runner.txt`（包み・template）。

## 8. 歯（契約ごと・`tests/e2e/pipe.rs` module・fake runner は `sh -c` 1 行）

- (a) core: fake runner が stdout の最終行に `{"question":"…"}` を書いて rc 76 で終える → event が `SeatStopped` + `QuestionRaised(detail)` + `RunStage(Questioned)` の順で並ぶ / rc 76 で最終行が record でない → `Failed`（`question-record-missing`）/ `resume` は rc 3 で何も書かない / `answer --words ""` は書かない / `answer` 後の `resume` が `Spawned` へ進み prompt に回答節が在る（fake runner が stdin を写す）/ `Questioned` 以外の run への `answer` は rc 3 / 質問 record と commit が同時の周は `Failed` / `report` の人由来 event 数が `answer` の後も 0。**base で RED**: `Questioned` の段名が無い・`answer` subcommand が無い（rc 4 か unknown subcommand）。宣言順 pin（`fleet.rs` の `is_declaration_order`）は位置を足しても緑のまま＝変化なし（数えない）。
- (b) runner の包み: fake claude が最終 result に record を書く → rc 76・包みの stdout 最終行に同じ record / key 無し・壊れた JSON・入れ子の引用は claude の rc を写す（3 形）/ prompt template に「回答」節が入る（stdin の写し）。
- (c) 実 run: toy repo に「verify 行が矛盾する契約」を流す → `Questioned` → planner 席の回答 → `resume` → `Landed`。人由来 event 0。AC 候補（SRS 改訂案 AC10）。

## 9. 却下案

- (a) **管理席がその場で答える**: 契約の SSOT は planner にあり、席が答えると契約と実装が別の言葉を持つ（N2 の向き・research §5 Q3）。中継に限る。
- (b) **runner を teammate にして親に text で聞き返す**: 口座分離・settings 不読・無 session・typed な判定入力の 4 点を失う（research §4）。判定入力が自由文になる（C3.3）。
- (c) **質問を gate の FAIL で代用する**: 契約の穴が「失敗」として記録され、直す向き（契約側か実装側か）が judgment に残る。
- (d) **run dir の file を質問 record の置き場にする**: 包みが file を書き pipeline が file を読む経路が 1 本増える。最終行の JSON（claude → 包み → pipeline の 2 段とも同じ形）に揃える方が parse 関数を共有できる。読む条件（rc 76 の周だけ）は gate の lens 読み（rc 0 の周だけ）と違うが、置き場と parse は同じ。
- (e) **質問を `Blocked` に相乗りさせる**: 承認（3 クラス・人由来）と質問（契約の不足・機械由来）は意味も actor も違う。同じ段に載せると FR22 の計測が混ざる。
- (f) **user が答えた質問を `QuestionAnswered(actor=human)` で記帳する**（本設計の初稿）: 承認を承認 event の外に置くことになり C7.2 に反する。`classes` が空の契約は spawn の承認関門を通らないので、A3 の質問に user が答えて `resume` する経路が関門を素通りする。§5 の経路（契約に classes / 裁定 id を足して承認関門へ）に改めた。

## 10. 後続

- 複数の質問・選択肢付きの質問・質問の優先度（1 便 1 質問で始める・ADR を supersede）。
- 契約の write-set の交差を intake で検査する（席並列の衝突を構造で消す・research §3）。
- 席の dashboard（`Questioned` の run を一覧で見る口・v3）。
