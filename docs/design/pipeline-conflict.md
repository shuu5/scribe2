# 設計: 並列の便の受付と衝突の解消 — intake の write-set 排他と、land の rebase 衝突を runner が解く形

- 要件: [FR4](../../design-intent/spec/srs.html#FR4) 契約 1 本 = worktree 1 + runner 1 / [FR10](../../design-intent/spec/srs.html#FR10) [FR11](../../design-intent/spec/srs.html#FR11) land の前提と CAS（FR10 は改訂案あり・ADR-0019 §3）/ [FR14](../../design-intent/spec/srs.html#FR14) resume / [FR30](../../design-intent/spec/srs.html#FR30) 配送構造 / [FR31](../../design-intent/spec/srs.html#FR31) 質問の停止 / [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed・読めない store は rc 2
- 憲法: [C2](../../design-intent/spec/constitution.html#c2) 除外は 1 enum の 1 関数 / [C3](../../design-intent/spec/constitution.html#c3) 状態は 1 つの store（C3.3 free text を判定入力にしない） / [C5](../../design-intent/spec/constitution.html#c5) rules 行は裁定 id / [C6](../../design-intent/spec/constitution.html#c6) 起動口は 1 つ / [C11](../../design-intent/spec/constitution.html#c11) C11.2 極性 / [C14](../../design-intent/spec/constitution.html#c14) C14.2 文書と manifest の drift / [C16](../../design-intent/spec/constitution.html#c16) edit time で止める / [N1](../../design-intent/spec/constitution.html#n1) N1.2 退役は可逆 move
- 決定: [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html) / [ADR-0009](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html) §2.1 / §2.2 / §2.4 / [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1 / [ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html) §2.2 / §2.3
- 土台: [pipeline.md](./pipeline.md) §3（契約 file）/ §4（段）/ §5.4（land の追随・retire）/ §5.6（stop）/ §5.7（resume）・[pipeline-question.md](./pipeline-question.md)（質問 record・回答節・再 spawn）。本 doc が土台の記述を上書きする箇所は土台側に pointer を置く（§4 / §5.4 / §5.7 / §11）。
- この設計から出る契約: §8（2 便）。並列 2 便の実地試験（台帳 s2-07l.143 の観測）は本 doc の契約より先に走る。

## 1. 何を解くか

便を何本か同時に流したときの 2 つの穴を塞ぐ。(1) 同じ file を触る契約が同時に走ると、後着の便が land の追随（rebase）で衝突して Failed になり、worktree が畳めない。(2) 衝突を誰がどう解くかが決まっておらず、admin に集まると並列にした意味が無くなる。

やさしく言うと: 入口で「まだ終わっていない便と同じ file を触る契約」は断る。それでも衝突したら、便を失敗にせず実装役をもう 1 回起こして「main を取り込んで直してから続けて」と頼む。人は実装役が質問したときだけ答える。

## 2. 入口の排他（ADR-0019 §2.1）

- **live な便** = **終端でない run**。終端 = replay で段が `Landed` / `Failed` / `Stopped` のいずれか、または `Gated` で verdict が FAIL（`verdict_of` の 1 本で読む・pipeline.md §4「FAIL は終端」）。live = `Intake` / `Blocked` / `Spawned` / `Questioned` / `Implemented` / `Gated(PASS または INCONCLUSIVE)`。判定は `Stage` の**網羅 match** で書く（段が増えたら compile で気付く・if-not の列挙にしない）。
- 各 live run の write-set = `<state_dir>/pipe/<run>/contract.toml`（契約の写し・pipeline.md §3）の `write-set`。写しが無い / parse 不能 → その run は「読めない」= 受け付けない側（fail-closed・理由 `Refuse::WriteSetUnreadable`・**rc 2**＝壊れた store・NFR4）。
- **交差の判定** = 正規化した path の集合の共通部分が空でない。正規化は write-set guard（`hook/guard.rs` の `relative_to`）と同じ規則: 先頭の `./` を落とす・連続する `/` を 1 つに・`..` を畳む・末尾 `/` は dir の印として残す。dir `a/` は `a/…` を含む（`a/` と `a/b.rs` は交差・`a/` と `ab/` は非交差・`a` と `a/` は交差・`src/../src/x.rs` と `src/x.rs` は交差）。symlink は解かない・存在は見ない（契約の字面だけ）。**解かない側の穴**: 実体が同じ file を別名で持つ 2 契約は入口で見逃す（偽陰性）。編集時の guard が実体名で塞ぎ、Bash の git 経路は ADR-0009 §2.1 の既知の穴のまま。
- **契約単位の拒否は enum 1 つの関数 1 本**（C2・主語は「契約 file が読めた後の契約単位の判定」＝引数の不足〔rc 1〕と契約 file 自体の parse error〔`ContractError`・rc 2〕は従来の口のまま外）: 新 module `pipe/refuse.rs` に閉じた enum `Refuse` を置き、現在 intake が散文で返している理由（git repo でない・同じ秒の再 intake）を variant に集めた上で、宣言順の末尾に `WriteSetOverlap { run, path }`（先頭の 1 組を持つ・全組は stderr に列挙）と `WriteSetUnreadable { run }` を足す。`Unfit`（verify 行 1 本の理由・`declaration.rs`・private・payload 持ち・判別子順の pin は**無い**）とは境界が違うので触らない。判別子順の pin（名前の slice + as_str・ADR-0013 §2.1 の形）は `Refuse` に新設する。
- **極性**: `Refuse` は新しい guard 境界（行為 = 便の起動を止めうる判定）。`polarity.rs` の `Guard` に variant 1 つ（名 `intake-refuse`・境界 `pipe::refuse::Refuse`・InLoop / FailClosed）を足し、極性一覧に **1 行増える**（既存 `intake-unfit` の行は不変）。
- rc: 交差 = `RC_REFUSED`（1・既存の拒否と同じ）/ 読めない = `RC_BROKEN`（2・契約の写しが無い / parse 不能・`Gated` run の verdict.json が読めない周も同じ「読めない」側）。いずれも run を作らず event を書かない。
- **同じ bead の 2 本目**: owner が live run と同じ契約も交差扱い（同じ write-set になるので自然に掛かる・特別扱いしない）。終端した run（Failed / Gated FAIL / Stopped）の同 bead は交差しない＝契約を改訂して流し直す経路（本番の .129 / .131 の型）は塞がらない。
- **放置された便を外す口**: `pipe stop --run <id>`（新 flag・pipeline.md §5.6）。終端でない run 1 本に `RunStopped` を書く（席が Live なら先に止める・pid が無い Live 席も対象）。終端の run には event を増やさず rc 1（書込は冪等・rc は冪等でない＝`--all` の「対象なし rc 0」とは語が違う）。`--all` の意味（Live な席を持つ run だけ）は変えない。
- 回答で write-set を広げる周（.133）: `pipe answer` が受ける回答の中に write-set の追加が在るなら、追加後の集合で同じ関数を撃ち、交差すれば answer を受け付けない（rc・理由は同じ variant）。write-set の追加をどう表すかは .133 の設計で決める（本 doc は照合の口＝`pub(crate)` の関数 1 本だけを約束する）。

## 3. 衝突の機械解消（ADR-0019 §2.2）

land の追随（pipeline.md §5.4・`follow_main`）で `git rebase <main>` が衝突した周:

1. `git rebase --abort` で木を戻す（現状どおり・戻せなければ `Failed detail=rebase-abort-failed` で終端・stderr に理由）。
2. `RunStage stage=Implemented detail=rebase-conflict:<base>..<main>` を記帳（**終端にしない**・段は Implemented に戻る）。
3. 回数の判定（§5 の guard）: **起こし直しの回数** = 同じ run の `detail` が `rebase-conflict:` で始まる `RunStage` の行数 − 1（いま記帳した分を除く・replay の導出値・別の状態を持たない）。この回数が rules 行 `pipe.follow_retries`（kind `FollowRetries`・Int・裁定 id 付き）**未満なら起こし直し、以上なら** `Failed detail=rebase-conflict`（終端・fail-closed）。値 N = 最大 N 回起こし直す（N+1 回目の衝突で Failed）。回数を読めない周（replay 不能）は起こし直さず `Failed detail=follow-unmeasured` + **rc 2** で終端する（FailClosed・上限到達の `rebase-conflict` + rc 1 とは分ける＝pipeline.md §5.4 の `main-unmeasured` と同じ極性・NFR4）。
4. **runner を起こし直す**（C6 の 1 つの spawn 口・Budget は Precheck から・同じ worktree・記録済みの base・同じ契約）。stdin = 契約の写し + 「回答」節（回答済みの質問が在れば）+ 「追随」節（順序: 契約 → 回答 → 追随）。起こし直しに `--runner` が要る: `pipe run` は自分の runner を渡す。`pipe land` / `resume` で `--runner` が無い周は rc 1 で「起こし直しに --runner が要る」を名乗り**以後の event を書かない**（手順 2 の `rebase-conflict:` 行は既に記帳済み・段は Implemented detail=rebase-conflict のまま・resume で続けられる）。
5. **runner の終了後、器が base を進める**: worktree の HEAD と main の merge-base を実測し、記録済みの base より進んでいれば `RunStage stage=Implemented detail=rebase:<old>..<merge-base>` を記帳する（**書く値は実測した merge-base**。turn の間に main がさらに進んでいても現在の main を書かない＝2 点 diff に main の新しい commit の逆向きが載る穴を塞ぐ。`base_of_run` は `rebase:` の新しい側だけを読む既存の 1 本＝gate の `git diff <base>..HEAD` が新しい base で測られ、先着便の file を write-set の外と誤らない）。その後は通常の spawn 完了と同じ（`SeatStopped` → `Implemented` / `Failed` / `Questioned`）→ gate → land。land の追随で再び rebase が走る（解けていれば衝突しない）。
6. 質問で止まる周: 「追随」節は「解けなければ `git rebase --abort` で木を戻し（commit を作らず clean のまま）質問 record で止まれ」と指示する。判定は **turn 開始時の tip を基準**にする（ADR-0019 §2.6 = ADR-0016 §2.2 の読み替え）: `settle_question` の「便の commit が 0 本」（base 基準）では起こし直しの run は常に commit ≥ 1 で Failed になるので、spawn の時点の worktree HEAD を記録し「turn で commit が増えていない」で `Questioned` へ倒す（初回の turn では tip = base ゆえ同値）。木が rebase の途中（`.git/rebase-merge` 在り）で runner が終わった周は `Failed detail=rebase-dirty` で終端（clean 前提を守る・fail-closed）。

「追随」節（`headless/runner.txt` に節の雛形を足す・字面は現物が正本・要旨だけ書く）: main が `<sha>` へ進んだ・`git rebase <sha>` を実行し衝突を解いて `git rebase --continue` で終える・解いてよいのは write-set の中の file だけ・解けなければ `git rebase --abort` で戻して質問 record で止まる（commit を作らない）・追随の commit は便の commit に含める。追随 turn は Bash 主体ゆえ編集時の guard は効かず、write-set の fence は gate の照合（新しい base）が担う（ADR-0009 §2.1 / §2.4）。

- `resume`: 最後の段が `Implemented` で最後の `RunStage` の detail が `rebase-conflict:` で始まり、かつ runner が起きていない（`SeatSpawned` の後に `SeatStopped` が在る）周は、`resume` も同じ起こし直しを撃つ（`Questioned` / `Blocked` からの再 spawn と同じ関数・allowed に `Implemented` を足す）。detail が `rebase-conflict:` でない `Implemented` は従来どおり gate。再 spawn の契機は ADR-0016 §2.2 の「回答済みの質問」と本項の 2 つ（ADR-0019 §2.6 の読み替え）。
- Budget: 起こし直しごとに Precheck を消費する（C6・上限 record や枠の判断は .142）。

## 4. 役割（ADR-0019 §2.3）

- admin = 流す・中継する・land する（ADR-0016 §2.1 の 3 つ）。衝突した worktree を席が編集する経路は無い（guard の外・C16）。放置された便を `pipe stop --run` で外すのは管理席の操作。
- planner = 契約の write-set を「構造の連鎖」で設計する（ADR-0019 §2.3・根拠は台帳 s2-07l.46 / .59 / .81 の notes）・質問に答える・排他で断られた契約を出し直す時機を決める・上限到達で Failed になった便の契約を切り直して再 intake する。
- user = 3 クラスの承認だけ（本 doc の経路には無い）。

## 5. 上限・guard・retire（ADR-0019 §2.4）

- rules 行 `pipe.follow_retries`（kind `FollowRetries`・Int・enabled・裁定 id）。本 doc は値を写さない。manifest に行が載るまでは ADR の**予定行**（C14.2 の相互参照は行が在って成立・ADR-0018 §4 と同じ）。
- **回数判定は guard**（ADR-0014 §2.1「起動を止めうる判定」）: 新 module `pipe/follow.rs`（land.rs は 977 行ゆえ R-C4-2 の内側に収める分割）に閉じた enum `FollowCheck { Retry, Exhausted, Unreadable }` と極性定数（InLoop / FailClosed）。終端形は variant ごとに固定: `Exhausted` → `Failed detail=rebase-conflict` + rc 1 / `Unreadable` → `Failed detail=follow-unmeasured` + rc 2。`polarity.rs` の `Guard` に variant 1 つ（名 `follow-retry`・境界 `pipe::follow::FollowCheck`）。
- `pipe retire` の前提を広げる: `Landed` ∨ (`Failed` ∧ 最後の `RunStage` の detail が `rebase-empty` **または `rebase-conflict`**) ∨ (`Gated` ∧ verdict が FAIL) ∧ worktree が在る ∧ clean。move だけ（N1.2）。.132 の memo（gate / lens FAIL の worktree）はここで吸収する。
- 上限到達後: Failed の便を retire で畳み、planner が契約の write-set を切り直して再 intake（同 bead の再 intake は終端した run とは交差しない・§2）。

## 6. 極性（一覧は 16 → 18 行）

- 排他 = 新 guard `intake-refuse`（InLoop / FailClosed・§2）。既存 `intake-unfit` は不変。
- 起こし直しの回数判定 = 新 guard `follow-retry`（InLoop / FailClosed・§5）。起こし直しの spawn 自体は既存 `spawn-budget` の口。
- 極性一覧 snapshot は 2 行増え、件数の歯（`polarity_` の母集団）は 16 → 契約 (a) で 17 → 契約 (b) で 18 に更新する。

## 7. 歯（契約ごとの「base で RED」・`crates/<NAME>/tests/e2e/pipe.rs` に `pipe_refuse_` / `pipe_follow_` 接頭辞・偽 runner は headless の歯と同じ stub 実行 file）

何を測るか（契約 (a) 入口の排他・stop --run）: live run（fixture の events + contract.toml）と交差する契約の intake が rc 1・run を作らず events.jsonl が byte 不変・stderr に run と path／dir と file の交差の表（`a/` × `a/b.rs` 交差・`a/` × `ab/` 非交差・`./a/b.rs` × `a/b.rs` 交差・`a` × `a/` 交差・`src/../src/x.rs` × `src/x.rs` 交差）／Landed / Failed / Stopped / Gated(FAIL) の run とは交差しない（Gated(PASS) / INCONCLUSIVE とは交差する）／写しが無い live run が在れば `WriteSetUnreadable` で rc 2／同 bead の 2 本目は交差／交差 2 組以上で stderr に全組／`stop --run` が非終端 run に `RunStopped` を書き、その後の同 write-set の intake が通る・終端 run には何も書かず rc 1・2 回撃っても 2 件目を書かない／property（`prop_refuse_`）: 交差判定は対称・非空集合は自分自身と交差・正規化に対して不変（`./` 前置・`//` 重複・`a/../a`）・共通 prefix を持たない集合は非交差／`Refuse` の判別子順 pin（新設）／極性一覧 snapshot に `intake-refuse` の 1 行・件数 17。

何を測るか（契約 (b) 衝突の起こし直し + rules 行 + retire）: 偽 runner（1 回目: file X を書いて commit・main を別 commit で同じ X を書き換えておく）で land の追随が衝突 → `Implemented detail=rebase-conflict:…` が記帳され Failed でない・main 不変・worktree は rebase 途中でない → 偽 runner が 2 回目に起こされ stdin に「追随」節と main の sha が在り、順序が 契約 → 回答 → 追随（stub が stdin を file に写す）→ 2 回目で解いて commit → 器が `Implemented detail=rebase:<old>..<merge-base>` を記帳し（turn 中に main をもう 1 commit 進めた fixture で、書かれる値が現在の main でなく merge-base であること）gate が新しい base で PASS → Landed／回数上限（fixture の rules で `follow_retries` = 1 ＝起こし直し 1 回まで）: 2 回目の衝突で `Failed detail=rebase-conflict`・runner は 3 回目に起こされない（呼出回数 file）／`rebase-conflict` と Gated(FAIL) の run を `pipe retire` が畳める（move・元 dir 不在・retired/ に在る）・`rebase-empty` の既存経路は不変／`resume` が同じ段から起こし直す（`--runner` 無しは rc 1 で events 不変・detail が `rebase-conflict:` でない Implemented は従来どおり gate）／stub が `rebase --abort` の後に質問 record で止まれば `Questioned`（turn で commit が増えていない・便が base から持つ commit は在る・clean）・起こし直しの turn で commit を作ってから質問 record を出せば `Failed detail=runner-rc:…`・rebase 途中で終われば `Failed detail=rebase-dirty`／回数を読めない fixture（`rebase-conflict:` の RunStage 行が壊れた JSON）で `Failed detail=follow-unmeasured` + rc 2・runner は起こされない／回数は replay の導出（events.jsonl に `rebase-conflict:` の行を手で書いた fixture・state dir に新 file が増えない）／rules 行 `pipe.follow_retries` を manifest から読む（kind 件数 + rules 外形 snapshot・行の欠落は RuleError）／極性一覧 snapshot に `follow-retry` の 1 行・件数 18／起こし直しの spawn も Precheck を通る（`pipe_spawn_measures_repo_before_launching` と同型の観測）。

## 8. 契約（2 便・この順）

- **(a)** intake の write-set 排他（`Refuse` enum + 関数 1 本・正規化・contract.toml の読み・`stop --run`・guard `intake-refuse`・歯 + property）。S〜M。FR10 に触れないので SRS 改訂の前に流せる。
- **(b)** land の衝突起こし直し（`rebase-conflict` の段と回数・「追随」節・runner 終了後の base 記帳・resume の経路・rules 行 `pipe.follow_retries`〔裁定 id が先・C5〕・guard `follow-retry`・retire の前提拡張〔rebase-conflict と Gated(FAIL)〕）。M。(a) と独立だが同時には流さない（pipe/ を両方が触る）。**SRS FR10 の改訂（ADR-0019 §3・user の folio-architect 手番）の land 後に流す。**
- 実地: (a)(b) の前に **.137 ‖ .139 の並列 2 便**で現行の追随（衝突なし）と枠の減りを観測（台帳 s2-07l.143 notes）。(b) の後に、意図的に交差する 2 便（write-set を同じ file にした toy 契約）で衝突 → 起こし直し → Landed を実地で 1 回通す（T の歯とは別に D の記録）。

## 9. 却下案（ADR-0019 §5 の写しは持たない・設計固有のもの）

- 交差判定で symlink や実 file の存在を見る（契約の字面で閉じないと intake が repo の状態に依存する。guard は編集時に実体名で当てるので同じ穴は編集経路で塞がる＝入口は保守的な字面判定で足りる）。
- 衝突時に器が `git merge` / 3-way tool で自動解決（意味の衝突を解けない・道具が「解けた」と言う周を器は疑えない）。
- 回数の状態を別 file に持つ（replay から導ける・C3 の 1 file）。
- 排他を land の時点で行う（衝突を早く知る方が安い・入口で止めるのが C16 の向き）。
- `Unfit` に契約単位の理由を足す（`Unfit` は verify 行 1 本の理由で入力型が違う・境界を混ぜると極性一覧の 1 行が 2 種の判定を背負う）。
- `Implemented` の下位状態を `Stage` の variant で持つ。跨版コストは小さい（ADR-0016 §2.2 が `Questioned` を schema 1 のまま足した前例）が、この下位状態は「次に撃つ段 = gate」を変えず resume の起こし方だけを変えるので、段を増やすと gate / land / retire の全 match に「Implemented と同じ」腕が増える。既存の `rebase-empty`（Failed の理由）と同じ detail prefix の形を採り、C2 / C3.3 との緊張はコストとして受け入れる（ADR-0019 §4）。

## 10. 後続

- 同時に流す本数の上限と口座の枠との連動（.142 の口座の自律制御）。
- 排他で断られた契約の待ち行列（tick か管理席の運用・別途）。
- 放置された非終端 run の自動掃除（`stop --run` は管理席の操作・自動化は別途）。
