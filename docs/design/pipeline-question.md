# 設計: pipeline の QUESTION の口 — runner は契約の不足を typed な質問 record で返して止まり、席が planner へ中継し、回答の記帳で便が再開する

- 要件: [FR5](../../design-intent/spec/srs.html#FR5) runner / lens は claude -p の 1 形 / [FR4](../../design-intent/spec/srs.html#FR4) 契約 1 本に worktree 1 つ / [FR16](../../design-intent/spec/srs.html#FR16) 逐語付きの記帳で resume / [FR22](../../design-intent/spec/srs.html#FR22) 人由来の event を数える。**席の役割と QUESTION の口そのものは記録時点の SRS に無い**＝改訂案は [ADR-0016 §3](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html#s3-srs-proposal)（反映は user の裁定）。
- 憲法: [C2](../../design-intent/spec/constitution.html#c2) 新しい理由は新 variant 1 つ / [C3](../../design-intent/spec/constitution.html#c3) C3.3 席の状態は typed・C3.4 完了条件は 1 enum / [C7](../../design-intent/spec/constitution.html#c7) 承認は 1 つの対話面・逐語で記帳 / [C11](../../design-intent/spec/constitution.html#c11) C11.2 境界ごとの enum が極性を運ぶ / [C16](../../design-intent/spec/constitution.html#c16) 逸脱は edit time で止める。
- 決定: [ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html)（既定の配送構造・質問 record・回答の権限）/ [ADR-0004 §2.1](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html#s2-1-persistence)（追記だけの event log）/ [ADR-0012](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html)（record は専用種別・構造化 field で読む）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)（極性一覧）。
- 探索: [RESEARCH-delivery-structure](../../design-intent/research/RESEARCH-delivery-structure.html)（席並列 / pipeline 並列 / 混成の比較・2026-09-11 の実測）。
- 前提の設計: [pipeline.md](./pipeline.md)（stage・event・subcommand・runner / lens の起動形）/ [seat-autonomy.md](./seat-autonomy.md)（席の tick・inject）/ [polarity.md](./polarity.md)（Guard の足し方）。
- この設計から出る契約（**本 doc の便では切らない**・ADR-0016 の user 裁定の後に planner が起票する）: (a) core の段と event と `pipe answer`（fake runner で質問 → Questioned → answer → resume → Implemented を歯で通す）/ (b) runner の包みの質問 record の読みと rc・prompt template の改訂 / (c) 席の中継（tick / 判定行から pointer 1 行を planner へ）/ (d) 実 run 1 本（toy repo・AC 候補）。

## 1. 何を解くか

runner（claude -p）は契約と worktree の中身だけで判断し、聞き返さない（`runner.txt`「対話しない」）。契約に穴があると、pipeline では gate の FAIL か黙った推測として現れる。席が自分の手で実装した 2026-09-11 の便では、同じ穴が planner への QUESTION 9 件（うち契約側の誤り 6 件）として上がり、契約を直してから進めた（research §2）。

**解くこと**: runner が「契約のここが足りない」を **typed な record** で返して止まり、席が planner へ中継し、planner が契約を直す（または回答を逐語で記帳する）と、同じ run が再開する口を作る。3 クラスの承認（`Blocked` → `pipe approve` → `resume`・FR15 / FR16）と**同じ形**で作る＝新しい機構を増やさず、段と event を 1 つずつ足す（C2）。

やさしく言うと: 実装役が「この指示では作れない」と思ったら、途中で勝手に決めるのではなく、決まった形の 1 行で質問を書いて手を止める。係の席がそれを計画役へ届け、計画役が指示を直すと、同じ便がそこから続く。

## 2. 全体の形

```
契約 ──intake──▶ Intake ──spawn──▶ Spawned ──runner──▶ Implemented ──gate──▶ Gated ──land──▶ Landed
                                       │
                                       │ runner の最終行が質問 record
                                       ▼
                                  Questioned（QuestionRaised・逐語）
                                       │ 席が planner へ pointer 1 行を中継
                                       │ planner: 契約を直す / 回答を書く / user へ上げる
                                       ▼
                              pipe answer --run <id> --words "<回答の逐語>"（QuestionAnswered）
                                       │
                                       └─ pipe resume ──▶ Spawned（同じ run・契約は再読・回答は prompt の「回答」節へ）
```

- **段は 1 つ**（`Stage::Questioned`）、**event は 2 つ**（`EventKind::QuestionRaised` / `QuestionAnswered`）を足す。`Blocked` / `ApprovalRequested` / `ApprovalReceived` と同型で、遷移は `resume` の 1 関数が持つ（[pipeline.md §4](./pipeline.md)）。
- **質問は行為ではない**ので runner の質問 record を読む判定は guard の定義（行為を止めうる判定・[polarity.md §2](./polarity.md)）に当たる: runner の process を止め、便を `Questioned` に倒す。極性は §7。
- **回答の権限**は契約の所有者（planner 席）にある。管理席は中継だけ（[ADR-0016 §2.3](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html#s2-3-answer-authority)）。3 クラス・裁定 id・閾値に触れる質問は planner が user へ上げ、user が答えた周だけ `actor=human` で記帳する（FR22 の計測面: 人由来は承認と回答だけ）。

## 3. 質問 record（runner の最終行・JSON 1 つ）

lens の verdict（最終行の JSON・`{"verdict":…,"evidence":…}`）と同じ置き場と読み方を使う。

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `question` | string | 必須 | 何が足りないか **1 行**（改行なし・非空）。逐語で `QuestionRaised.detail` に写す |
| `about` | string | 任意 | 契約のどの key に関する質問か（`verify` / `write-set` / `done` / `goal` / `design` の 1 語） |

- runner の prompt template は「対話しない」を「**質問は最終行の record で返して止まる**（それ以外の形で人へ問わない）」に改める。record を出した周は **commit を作らない**（作った周は質問ではなく実装の失敗として扱う＝`Failed`・runner の rc を写す）。
- 包み（`<NAME> runner`）は claude の stream の最終 `result` の text の最終行を読み、`question` key を持つ JSON 1 つなら **rc `RC_QUESTION`**（新しい rc・上限の rc 75 と同じく「判定の名札」）で終える。**読めない・key が無い周は質問ではない**（claude の rc をそのまま写す・ADR-0012 §2.1 と同じ「未知は claude の rc へ落ちる」）。
- record の細部（複数の質問・選択肢付きの質問・優先度）は本設計の射程外。1 便 1 質問で始め、足りなければ ADR を supersede する。

## 4. 段と event

| stage | 入る event | 出る条件 |
|---|---|---|
| `Spawned` | （既存） | runner rc = `RC_QUESTION` → `SeatStopped` + `QuestionRaised(detail=逐語)` + `RunStage(Questioned)` |
| `Questioned` | `QuestionRaised` + `RunStage` | `QuestionAnswered`（`detail` 非空）が在れば `resume` → spawn（`Spawned`・同じ run）。無ければ `resume` は rc 3 で何も書かない（`Blocked` と同じ・何回聞いたかを積まない） |
| `Failed` | （既存） | 質問 record と commit が同時に在る周（質問の形をした実装の失敗） |

- `Questioned` の run は `pipe stop --all` の対象（席は畳まれているので pid は無い・`Live` 席 0 で冪等）。
- `fleet export` / `report` は `QuestionRaised` / `QuestionAnswered` を既存の event と同じ行の形で出す（schema 版は上げない・key は既存の `KNOWN_KEYS` の内側・`detail` に逐語）。

## 5. subcommand

- `<NAME> pipe answer --run <id> --words "<回答の逐語>" [--human]`: `Questioned` の run にだけ受理（それ以外は rc 3・stderr 1 行）。`QuestionAnswered` を `detail=<逐語>` で 1 行 append。`--human` を渡した周だけ `actor=human`（user が直接答えた周＝planner が user の逐語を写す）。既定は `actor=machine`（planner 席の回答）。**逐語が空なら書かない**（`approve` と同じ）。
- `<NAME> pipe resume --run <id> --runner <cmd>`: `Questioned` からは `QuestionAnswered` が在る周だけ spawn（[pipeline.md §5](./pipeline.md) の `Blocked` 分岐と同型）。runner の prompt には契約（再読・planner が直していれば新しい本文）に加えて「**回答**」節（`QuestionRaised.detail` と `QuestionAnswered.detail` の対）を付ける。
- `<NAME> pipe run`（席が撃つ 1 本）の判定行に `question=<id>` を出す（席の中継の入力・§6）。

## 6. 席の役割（既定の配送構造・ADR-0016 §2.1）

- **planner ×1**: 契約・裁定・merge の go。QUESTION には契約を直すか回答を記帳するかで応え、3 クラス・裁定 id・閾値の質問は user へ上げる。
- **管理席 ×N**（各 1 口座）: dispatch された bead を `pipe run` で流す・`question=` が出たら **pointer 1 行**（run id と `about`）を planner へ inject する・go の後に merge する。**自分の手で実装するのは pipeline が回せない便だけ**（器の穴・design-intent・host 作業）。
- runner / lens は claude -p のまま（FR5・ADR-0009 / 0011）。teammate（Agent tool の subagent）は席の中の review に限る。
- 中継は席の tick / run の判定行を入力にする（pane の字面を読まない・C3.3）。planner の入力欄が塞がっている周は inject が `Refused` になる（既存の極性・[ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)）＝**届かなかった事実は判定行に残る**。

## 7. 極性（C11.2・polarity.md §3）

- 新しい Guard variant **1 つ**: `RunnerQuestion`（boundary = runner の包みの record 読み）。`timing = InLoop`（record を読んだその場で runner を止め便を倒す）/ `on_failure = FailOpen`（record が無い・読めない周は止めない＝claude の rc へ落ちる。ADR-0012 の上限 record と同じ向き。一覧はこれを隠さない）。
- `pipe answer` は行為を止めない（記帳するだけ）＝guard ではない。`resume` の `Questioned` 分岐は既存の `Blocked` 分岐と同じ「記帳が無ければ進まない」で、既存の承認関門の極性の内側。
- 極性一覧の snapshot（`e2e__polarity__polarity_external_form.snap`）に 1 行増える＝契約の write-set に snapshot と `polarity.rs` を入れる（構造の連鎖）。

## 8. 歯（契約ごと・`tests/e2e/pipe.rs` module・fake runner は `sh -c` 1 行）

- (a) core: fake runner が最終行に `{"question":"…"}` を書いて rc `RC_QUESTION` で終える → event が `SeatStopped` + `QuestionRaised(detail)` + `RunStage(Questioned)` の順で並ぶ / `resume` は rc 3 で何も書かない / `answer --words ""` は書かない / `answer` 後の `resume` が `Spawned` へ進み prompt に回答節が在る（fake runner が stdin を写す）/ `Questioned` 以外の run への `answer` は rc 3 / 質問 record と commit が同時の周は `Failed`。**base で RED**: `Questioned` の段名が無い・`answer` subcommand が無い（rc 4 か unknown subcommand）。
- (b) runner の包み: fake claude が最終 result に record を書く → rc `RC_QUESTION`・stdout の判定行に `question=` / key 無し・壊れた JSON・入れ子の引用は rc を写す（3 形）/ prompt template に「回答」節が入る（stdin の写し）。
- (c) 席: `pipe run` の判定行に `question=<id>` が在る周だけ planner 宛の pointer 1 行を送る（fake tmux の送信記録）/ 入力欄が非空なら `Refused`（既存の歯に 1 case）。
- (d) 実 run: toy repo に「verify 行が矛盾する契約」を流す → `Questioned` → planner 席の回答 → `resume` → `Landed`。人由来 event 0（`--human` なし）。AC 候補（SRS 改訂案 AC10）。

## 9. 却下案

- (a) **管理席がその場で答える**: 契約の SSOT は planner にあり、席が答えると契約と実装が別の言葉を持つ（N2 の向き・research §5 Q3）。中継に限る。
- (b) **runner を teammate にして親に text で聞き返す**: 口座分離・settings 不読・無 session・typed な判定入力の 4 点を失う（research §4）。判定入力が自由文になる（C3.3）。
- (c) **質問を gate の FAIL で代用する**: 契約の穴が「失敗」として記録され、直す向き（契約側か実装側か）が judgment に残る。FR22 の計測でも人の関与が見えない。
- (d) **run dir の file を質問 record の置き場にする**: 包みが file を読む経路が 1 つ増える。lens の verdict と同じ「最終行の JSON」に揃える方が読み手が 1 つで済む（C2）。
- (e) **質問を `Blocked` に相乗りさせる**: 承認（3 クラス・人由来）と質問（契約の不足・機械由来が既定）は意味も actor も違う。同じ段に載せると FR22 の計測が混ざる。

## 10. 後続

- 複数の質問・選択肢付きの質問・質問の優先度（1 便 1 質問で始める・ADR を supersede）。
- 契約の write-set の交差を intake で検査する（席並列の衝突を構造で消す・research §3）。
- 席の dashboard（`Questioned` の run を一覧で見る口・v3）。
