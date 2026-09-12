# 設計: 並列の便の受付と衝突の解消 — intake の write-set 排他と、land の rebase 衝突を runner が解く形

- 要件: [FR4](../../design-intent/spec/srs.html#FR4) 契約 1 本 = worktree 1 + runner 1 / [FR10](../../design-intent/spec/srs.html#FR10) [FR11](../../design-intent/spec/srs.html#FR11) land の前提と CAS / [FR14](../../design-intent/spec/srs.html#FR14) resume / [FR30](../../design-intent/spec/srs.html#FR30) 配送構造 / [FR31](../../design-intent/spec/srs.html#FR31) 質問の停止 / [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed
- 憲法: [C2](../../design-intent/spec/constitution.html#c2) 除外は 1 enum の 1 関数 / [C5](../../design-intent/spec/constitution.html#c5) rules 行は裁定 id / [C6](../../design-intent/spec/constitution.html#c6) 起動口は 1 つ / [C11](../../design-intent/spec/constitution.html#c11) C11.2 極性 / [C16](../../design-intent/spec/constitution.html#c16) edit time で止める / [N1](../../design-intent/spec/constitution.html#n1) N1.2 退役は可逆 move
- 決定: [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html) / [ADR-0009](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html) §2.1 / §2.2 / [ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html) §2.2 / §2.3
- 土台: [pipeline.md](./pipeline.md) §3（契約 file）/ §4（段）/ §5.4（land の追随・retire）・[pipeline-question.md](./pipeline-question.md)（質問 record・回答節・再 spawn）。
- この設計から出る契約: §7（2 便）。並列 2 便の実地試験（.137 ‖ .139）は本 doc の契約より先に走り、観測は bd .143 notes。

## 1. 何を解くか

便を何本か同時に流したときの 2 つの穴を塞ぐ。(1) 同じ file を触る契約が同時に走ると、後着の便が land の追随（rebase）で衝突して Failed になり、worktree が畳めない。(2) 衝突を誰がどう解くかが決まっておらず、admin に集まると並列にした意味が無くなる。

やさしく言うと: 入口で「いま走っている便と同じ file を触る契約」は断る。それでも衝突したら、便を失敗にせず実装役をもう 1 回起こして「main を取り込んで直してから続けて」と頼む。人は実装役が質問したときだけ答える。

## 2. 入口の排他（ADR-0019 §2.1）

- **live な便** = replay で段が `Landed` / `Stopped` / `Failed` のいずれでもない run（`Intake` / `Blocked` / `Spawned` / `Questioned` / `Implemented` / `Gated`）。
- 各 live run の write-set = `<state_dir>/pipe/<run>/contract.toml`（契約の写し・§3）の `write-set`。写しが無い / parse 不能 → その run は「読めない」= 受け付けない側（fail-closed・理由 `Unfit::WriteSetUnreadable`）。
- **交差の判定** = 正規化した path の集合の共通部分が空でない。正規化: 先頭の `./` を落とす・連続する `/` を 1 つに・末尾 `/` は dir の印として残す。dir `a/` は `a/…` を含む（`a/` と `a/b.rs` は交差・`a/` と `ab/` は非交差・`a` と `a/` は交差）。symlink は解かない・存在は見ない（契約の字面だけ）。
- **判定は enum 1 つの関数 1 本**: 既存の intake の除外（`pipe/declaration.rs` の `Unfit`）に variant `WriteSetOverlap { run, path }`（先頭の 1 組を持つ・全組は stderr に列挙）と `WriteSetUnreadable { run }` を宣言順の末尾に足す。rc は既存の拒否と同じ（RC_REFUSED）・run を作らず event を書かない。極性一覧は既存 guard `intake-unfit`（InLoop / FailClosed）の内側なので行は増えない。
- **同じ bead の 2 本目**: owner が live run と同じ契約も交差扱い（同じ write-set になるので自然に掛かる・特別扱いしない）。
- 回答で write-set を広げる周（.133）: `pipe answer` が受ける回答の中に write-set の追加が在るなら、追加後の集合で同じ関数を撃ち、交差すれば answer を受け付けない（rc・理由は同じ variant）。write-set の追加をどう表すかは .133 の設計で決める（本 doc は照合の口だけを約束する）。

## 3. 衝突の機械解消（ADR-0019 §2.2）

land の追随（pipeline.md §5.4・`follow_main`）で `git rebase <main>` が衝突した周:

1. `git rebase --abort` で木を戻す（現状どおり・戻せなければ `Failed detail=rebase-abort-failed` で終端・stderr に理由）。
2. `RunStage stage=Implemented detail=rebase-conflict:<base>..<main>` を記帳（**終端にしない**・段は Implemented に戻る）。
3. 回数の判定: 同じ run の `detail` が `rebase-conflict:` で始まる `RunStage` の行数（replay の導出値・別の状態を持たない）が rules 行 `pipe.follow_retries`（新 kind `FollowRetries`・Int・裁定 id 付き）以上なら `Failed detail=rebase-conflict`（終端・fail-closed）。
4. 上限未満なら **runner を起こし直す**（C6 の 1 つの spawn 口・Budget は Precheck から・同じ worktree・同じ契約）。stdin = 契約の写し + 「追随」節 + 回答済みの質問が在れば「回答」節（順序: 契約 → 回答 → 追随）。
5. runner の終了後は通常の spawn 完了と同じ（`SeatStopped` → `Implemented` / `Failed` / `Questioned`）→ gate → land。land の追随で再び rebase が走る（解けていれば衝突しない）。

「追随」節（`headless/runner.txt` に節の雛形を足す・字面は現物が正本・要旨だけ書く）: main が `<sha>` へ進んだ・`git rebase <sha>` を実行し衝突を解いて `git rebase --continue` で終える・解いてよいのは write-set の中の file だけ・解けなければ質問 record で止まる（commit を作らない）・追随の commit は便の commit に含める。

- `resume`: 最後の段が `Implemented` で最後の `RunStage` の detail が `rebase-conflict:` かつ runner が起きていない（`SeatSpawned` の後に `SeatStopped` が在る）周は、`resume` も同じ起こし直しを撃つ（`Questioned` / `Blocked` からの再 spawn と同じ関数）。
- Budget: 起こし直しごとに Precheck を消費する（C6・上限 record や枠の判断は .142）。

## 4. 役割（ADR-0019 §2.3）

- admin = 流す・中継する・land する（ADR-0016 §2.1 の 3 つ）。衝突した worktree を席が編集する経路は無い（guard の外・C16）。
- planner = 契約の write-set を「構造の連鎖」で設計する（memory: write-set-must-follow-structural-chains）・質問に答える・排他で断られた契約を出し直す時機を決める。
- user = 3 クラスの承認だけ（本 doc の経路には無い）。

## 5. 上限と retire（ADR-0019 §2.4）

- rules 行 `pipe.follow_retries`（kind `FollowRetries`・Int・enabled・裁定 id）。本 doc は値を写さない。
- `pipe retire` の前提を広げる: `Landed` ∨ (`Failed` ∧ 最後の `RunStage` の detail が `rebase-empty` **または `rebase-conflict`**) ∧ worktree が在る ∧ clean（.132 の gate / lens FAIL は本 doc の対象外＝別途）。move だけ（N1.2）。

## 6. 極性

- 排他 = 既存 guard `intake-unfit` の variant 追加（一覧の行は増えない）。
- 起こし直し = spawn の口（既存 `spawn-budget` guard）。衝突の終端判定（回数上限）は `land` の境界 enum（`Follow` / `MainCheck` 系）の variant 1 つ・FailClosed。極性一覧 snapshot は変わらないはず（変わったら理由を報告）。

## 7. 歯（契約ごとの「base で RED」・`crates/<NAME>/tests/e2e/pipe.rs` に `pipe_overlap_` / `pipe_follow_` 接頭辞・偽 runner は headless の歯と同じ stub 実行 file）

何を測るか（契約 (a) 入口の排他）: live run（fixture の events + contract.toml）と交差する契約の intake が rc RC_REFUSED・run を作らず events.jsonl が不変・stderr に run と path／dir と file の交差の表（`a/` × `a/b.rs` 交差・`a/` × `ab/` 非交差・`./a/b.rs` × `a/b.rs` 交差）／Landed / Failed / Stopped の run とは交差しない／写しが無い live run が在れば `WriteSetUnreadable` で受け付けない／property（`prop_overlap_`）: 交差判定は対称・自分自身と交差・正規化に対して不変／`Unfit::ALL` の判別子順 pin（既存の pin が +2 で通る）。

何を測るか（契約 (b) 衝突の起こし直し + rules 行 + retire）: 偽 runner（1 回目: file X を書いて commit・main を別 commit で同じ X を書き換えておく）で land の追随が衝突 → `Implemented detail=rebase-conflict:…` が記帳され Failed でない → 偽 runner が 2 回目に起こされ stdin に「追随」節が在る（stub が stdin を file に写す）→ 2 回目で解いて commit → gate → Landed／回数上限（fixture の rules で 1）到達で `Failed detail=rebase-conflict`／その run を `pipe retire` が畳める（move・元 dir 不在・retired/ に在る）／`resume` が同じ段から起こし直す／stub が質問 record で止まれば `Questioned`／極性一覧 snapshot 不変／rules 行 `pipe.follow_retries` を manifest から読む（kind 件数 + rules 外形 snapshot）。

## 8. 契約（2 便・この順）

- **(a)** intake の write-set 排他（`Unfit` 2 variant・交差関数 1 本・contract.toml の読み・歯 + property）。S。
- **(b)** land の衝突起こし直し（`rebase-conflict` の段と回数・「追随」節・resume の経路・rules 行 `pipe.follow_retries`〔裁定 id が先・C5〕・retire の前提拡張）。M。(a) と独立だが同時には流さない（pipe/ を両方が触る）。
- 実地: (a)(b) の前に **.137 ‖ .139 の並列 2 便**で現行の追随（衝突なし）と枠の減りを観測（dispatch 102・bd .143 notes）。(b) の後に、意図的に交差する 2 便（write-set を同じ file にした toy 契約）で衝突 → 起こし直し → Landed を実地で 1 回通す（T の歯とは別に D の記録）。

## 9. 却下案（ADR-0019 §5 の写しは持たない・設計固有のもの）

- 交差判定で symlink や実 file の存在を見る（契約の字面で閉じないと intake が repo の状態に依存する・保守的な字面判定で足りる）。
- 衝突時に器が `git merge` / 3-way tool で自動解決（意味の衝突を解けない・誤 merge が gate を通る）。
- 回数の状態を別 file に持つ（replay から導ける・C3 の 1 file）。
- 排他を land の時点で行う（衝突を早く知る方が安い・入口で止めるのが C16 の向き）。

## 10. 後続

- 同時に流す本数の上限と口座の枠との連動（.142 の口座の自律制御）。
- 排他で断られた契約の待ち行列（tick か管理席の運用・別途）。
- gate / lens FAIL の worktree の retire（.132 の残り）。
