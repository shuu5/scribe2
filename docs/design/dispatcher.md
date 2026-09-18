# 設計: dispatcher — 審査を通った契約を器が自動で起こす（列・介入・観測）

- 出所: user 裁定 2026-09-15 12:5xZ（逐語は台帳 s2-07l notes）→ [ADR-0034](../../design-intent/decisions/ADR-0034-dispatcher-starts-reviewed-contracts-automatically.html)。
- 要件: [FR30](../../design-intent/spec/srs.html#FR30) 配送構造（担い手の移動・SRS の反映は user の /folio-architect の周）/ [FR39](../../design-intent/spec/srs.html#FR39) intake の排他 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR36](../../design-intent/spec/srs.html#FR36) 口座選定 / [NFR4](../../design-intent/spec/srs.html#NFR4)。
- 前提: 審査の段（[contract-source.md](./contract-source.md) §4・契約 (c) = s2-07l.241・Landed）と、設計 doc の契約表の行から intake が契約 file を生成する口（同 §2・契約 (b) = s2-07l.209）が Landed していること。後者が無いと契約 file は手書きでしか作れず、器は起動を組めない。
- この設計から出る契約: §9（3 便・(a) → (b) → (d)）。裁定: user 2026-09-15 13:5xZ（順序は first → priority → 起票順・印は 3 つ・FR30 の改訂は /folio-architect の周）。**[ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 の後の形**（user 裁定 2026-09-18・逐語は台帳）: 席は orchestrator 1 つで管理席は無く、便の起動・着地・merge は器の dispatcher が行う。管理 tick は消えた（`s2-07l.479.1`）ので契機は便の終端と手動の 1 周（§5）。「契約が出来た直後の審査」（旧 (r)・`ContractReviewed`）は持たず、審査は `pipe run` の Reviewed の段のまま（§2）。本 doc の「planner」は orchestrator を指す（改名は ADR-0045・`s2-07l.478`）。

## 1. 何を解くか

便の起動が planner → 管理席 → launcher の中継に乗っている（[ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html) §2.1）。起動に要る判定＝台帳の依存・live 便との write-set の交差（intake の 1 関数）・受付（余地と host の memory）・審査の verdict は全部器が持っていて、席はそれを読み直して launcher を撃つだけだった。2026-09-15 の実測: 着地から次の起動まで便ごとに数分〜十数分の待ち、優先便の後回し（11:47Z）、器に無い「同時 3 便」の散文（11:5xZ）。本設計は起動を器の 1 関数に置き、席の産物を「契約・priority・介入」に絞る。

やさしく言うと: 「次はどれを起こすか」を人（席）が考えるのをやめ、器が既に持っている判定でそのまま起こす。planner は急ぐ便に印を付けるだけ。

## 2. 列の入力と順序

- **入力** = 台帳の open な bead のうち「依存が全部 closed ∧ acceptance が非空 ∧ `intake:memo` の label が無い ∧ acceptance に設計 pointer の行 `design = docs/design/<題>.md#<id>` が在る ∧ **同じ契約で審査 FAIL に終わった便が無い**」もの（.209 の `--design` と同じ字面・pointer の無い便は理由 `NoDesignPointer`・直前の便が `Reviewed` FAIL / INCONCLUSIVE で終わり契約 file の sha が変わっていない便は `ReviewFailed { sha }`＝既存の event log（`RunStage` の Reviewed の verdict）と run dir の契約 file の sha から導く・新しい event kind は足さない。列外の便も `dispatch ls` には理由付きで出す＝planner が直すべき契約が見える）。台帳の読みは席の指示文の `{ledger}` と同じ子 process と同じ関数（`read_ledger`・`bd --readonly list --limit 0 --json`・待ち上限は rules 行 `seat.ledger_timeout_s`・置き場は `seat/ledger.rs`）を共用し、読めない周は列を空と読まず `unmeasured` で止まる（NFR4・C10）。
- **審査の時点 = `pipe run` の Reviewed の段のまま**（[contract-source.md](./contract-source.md) §4・契約 (c) = s2-07l.241）。「契約が出来た直後に審査し verdict を sha に紐づける」旧案（user 裁定 2026-09-15 13:4xZ・旧 (r)・`ContractReviewed`）は ADR-0045 §2 の後は持たない: 契約は設計 doc の行 1 つになり（[contract-source.md](./contract-source.md) §2「台帳の bead」・`s2-07l.209`）本文の不備は CI の `contracts check` が先に落とすので、審査の材料は起動の時点で揃う。FAIL の便が列を塞ぐ形は「同じ sha で審査 FAIL に終わった便は列外」（上の入力の条件）で塞ぎ、契約の改訂（設計 doc の PR）で sha が変わればまた列に入る。新しい event kind・審査を飛ばす flag は作らない（C2・C16・C17.2）。
- **順序** = 1 関数 `order`（行の列 → 候補 `Candidate` の列）: (1) 介入 `first` の便 (2) 台帳の `priority`（P0 → P4）(3) 起票順（id の数字）。同順は起票順。**散文の順序を持たない**（憲法 C2）。
- **hold** の便は列に載るが起こさない（理由 = `Hold`）。

## 3. 起動条件（器の判定の再利用・1 実装）

| 条件 | 器の既存の判定 | 落ちたときの理由（閉じた型 `WaitReason`） |
|---|---|---|
| 台帳の依存が閉じている | `bd --readonly ready` | `Dependency { on: Vec<bead> }` |
| live 便と write-set が交差しない | intake の排他（[pipeline-conflict.md](./pipeline-conflict.md) §2・FR39・`pipe/cli/intake.rs` の同じ関数） | `Overlap { with: run, files: n }` |
| 受付（余地・host の memory）を通る | intake の余地（[contract-source.md](./contract-source.md) §3）と受付札（[gate-cost.md](./gate-cost.md) §3.2） | `Admission { reason }` |
| 介入 hold が無い | 列の状態 | `Hold { since }` |
| 同じ契約で審査 FAIL に終わった便が無い（§2） | event log の `RunStage`（Reviewed の verdict・replay）と run dir の契約 file の sha | `ReviewFailed { sha }` |
| 設計 pointer が在る | acceptance の `design = …` 行 | `NoDesignPointer` |

- 判定は intake の既存の関数（`pipe/cli/intake.rs` の `exclude_overlap` / `exclude_cap_shortfall`・`pipe/admission.rs` の `has_room`）を**記帳せずに**呼ぶ（可視性を `pub(super)` にする以外は不変・C2 の 1 実装）。通る便だけ既存の `pipe run --design <pointer> --bead <id> --repo <anchor>`（intake → Reviewed → spawn・審査は .241 のまま）を撃つ。**dispatcher は起動の時機だけを決め、口座の選定（FR36）・受付の記帳・審査は従来の段がそのまま行う**。
- **並列の上限は write-set の交差・直列依存・受付の枠と、host の同時走行の最大値 1 つ**（[ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)・[ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)）。最大値は rules 行 `pipe.max_live` が持ち、受付が live な便を数えて断る（[gate-cost.md](./gate-cost.md) §24）。同じ契機で複数の便が条件を満たせば最大値まで全部起こす（受付札が memory で縮退させる）。最大値で断られた契約は列に留まり、その理由の variant は行 a の Landed 後に足す（gate-cost.md §24 (4)）。
- 1 周で起こした便は次の候補の交差の相手に入る（列を上から順に評価し、起こした便の write-set を live に足して次を評価する）。
- **anchor の作業木の汚れは起動条件に無い**（`s2-07l.367`・admin の実測 2026-09-15 21:51Z で 2 例目: planner の未 commit の design-intent 編集が launcher の手順 0「porcelain 0」で便を止めた）。便は base の sha から worktree を切る（[pipeline.md](./pipeline.md) §5.2）ので作業木の汚れは便に載らず、照合するのは **base の sha が `origin/main` の先端と一致するか**だけ（一致しない周は列の `WaitReason` でなく intake の stale base の断り＝既存の判定）。binary は base の sha で build した写しを使う（anchor の作業木で build した binary を便に渡さない）。

## 4. 介入の口（planner の typed な印）

`<NAME> pipe dispatch first <bead>` / `hold <bead>` / `release <bead>`。印は state dir の列の記録（event log の 1 kind `DispatchMark { bead, mark: First | Hold | Release, by: target }`）で、`dispatch ls` に出る（退避物の復元の DATA に出す案は §6 のとおり超過した）。user の直命「最優先」の対は `first`（planner が打つ・逐語は台帳に残る）。印は台帳の priority を書き換えない（台帳は task と裁定・憲法 C15）。

## 5. 契機（便の終端と手動の 1 周・tick は無い）

- **便の終端**: 便が live で無くなる瞬間（`Landed` / `Failed` / `Stopped` / retire・`pipe run` `pipe resume` `pipe land` `pipe stop` `pipe retire` の終端の記帳の直後）に同じ関数を 1 周撃つ。着地や停止で write-set の交差が解けた便を待たせない。
- **手動の 1 周**: `<NAME> pipe dispatch --state-dir S --repo R` は権能なしの口（誰が撃っても同じ 1 周・起動の権能は turn 関数の中の器の判定であって席の権能ではない・ADR-0045 §2 (1)）。host の再起動や driver の死亡（下）の後に人が撃つ。
- **印の直後**: `first` / `release` の記録の直後にも 1 周撃つ（印を付けた便を次の終端まで待たせない）。
- **管理 tick の契機は無い**（tick は `s2-07l.479.1` で消えた・ADR-0045 §2 (2)）。時計で撃つ契機を足さない（C17.2: 足すなら先に消すものを名指す）。
- 全部同じ 1 関数（dispatch module の turn 関数）を撃つ（C2）。lock は着地の列と同じ store の lock（fleet の store が持つ acquire）を使い、二重起動を防ぐ。終端の中から撃つ 1 周は終端の記帳の後・lock の外で行い、失敗しても終端の rc を変えない（起こせなかった便は次の契機で拾う・観測は §6）。
- **driver の死亡**（`s2-07l.352`・契約 (d)・C9 の便版の driver 側）: `pipe run` / `pipe resume` の process（driver）は入口で `<state_dir>/pipe/<run>/driver` に受付札と同じ本文（pid + 起動時刻・[gate-cost.md](./gate-cost.md) §3.2）を書き、終端で消す。turn 関数は live 便のうち札の所有者が死んでいる便（lock の所有者と同じ probe・`Owner::Dead`）を (a) と同じ引数の `pipe resume` で起こし直し、record token に `resumed:<m>` を足す（`dispatch=started:<n>,resumed:<m>,waiting:<k>`）。札が無い / 読めない便は触らない（測れないを「死んだ」に読み替えない・fail-closed）。schema を広げた便の Landed で古い binary の driver が typed に死ぬ周（NFR4・.160 の座礁 2026-09-15）も次の契機（他便の終端か手動の 1 周）に現在の binary で続く＝写し binary の refresh は要らない（走行中の process の code は変わらないので refresh は座礁を防がない）。`base_of_run` の読めなさは typed に呼び手へ返す（C10・「base が無い」と分ける）。

## 6. 観測

- `<NAME> pipe dispatch ls --state-dir S`: 列の各便を `[DISPATCH] bead=<id> prio=<p> mark=<first|hold|->` + `reason=<WaitReason>` で 1 行ずつ・`[DISPATCH-COUNT] total=<n> ready=<k>`・0 件は `[DISPATCH-NONE]`・台帳が読めない周は `[DISPATCH-UNMEASURED reason=…]`（0 件と融合しない・C10）。
- 観測の面は上の 1 口だけである。退避物の復元（rebrief）の DATA に同じ行を載せる案は、その DATA ごと超過した（ADR-0045 §2 (2)・`s2-07l.479.2`・[working-memory.md](./working-memory.md)）。

## 7. 極性

dispatcher は「起こす」側で行為を止める判定を持たない（起こせない便は理由付きで待つだけ）＝[ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1 の guard ではなく極性一覧に載せない（受付札と同じ扱い・[gate-cost.md](./gate-cost.md) §3.2）。台帳が読めない周は `unmeasured` で 1 本も起こさない（fail-closed 側に倒す・NFR4）。

## 8. 歯（`crates/<NAME>/tests/e2e/pipe/dispatch.rs`・`pipe_dispatch_` 接頭辞・名前の列は現物が SSOT）

- 順序: first → priority → 起票順の 1 関数（pure・in-file）。
- 起動条件: 偽の台帳（ready の出力 fixture）+ 偽の live 便（event log）で、交差する便は `Overlap` で待ち、交差しない便だけ `pipe run` の構築点が呼ばれる。
- 台帳が読めない周: `[DISPATCH-UNMEASURED]` で起動 0（0 件と区別）。
- 介入: `first` が priority より先に来る・`hold` は起こさない・`release` で戻る（event log の往復）。
- 契機（行 (b)・`pipe_terminal_dispatch_` 接頭辞）: land / stop / retire の終端の直後に 1 周撃たれ、交差が解けた便が起動の構築点に届く（偽 remote の toy repo）・`pipe dispatch` の手動 1 周が同じ関数を撃つ・`first` の記録の直後に 1 周撃たれる・終端の中の 1 周が失敗しても終端の rc は変わらない。
- 審査 FAIL の列外: 直前の便が Reviewed FAIL で終わった契約は同じ sha では `ReviewFailed` で列外、契約 file の sha が変わると列に戻る（偽 lens の verdict と run dir の fixture）。

## 9. 契約（3 便・(a) → (b) → (d)。(r) と (c) は超過）

- **(a)** `pipe dispatch` の本体: 列の導出（台帳の list + acceptance の設計 pointer + 審査 FAIL の列外）・順序の 1 関数・起動条件（intake の判定関数の再利用・可視性の変更）・`first / hold / release` の印（event kind 1 つ `DispatchMark` + `Event` の typed な field）・`dispatch ls`。依存: s2-07l.209 Landed。
- **(r)** 審査の時点（契約が出来た直後の審査・`ContractReviewed`）は**超過した**（ADR-0045 §2 の後の形・§2「審査の時点」・`s2-07l.368` は close）。審査は `pipe run` の Reviewed の段のまま。
- **(b)** 契機: 便の終端の直後 + `pipe dispatch` の手動 1 周 + 印の直後（§5）。依存: (a)。
- **(c)** 観測（rebrief の DATA に `[DISPATCH]` の行）は**超過した**（ADR-0045 §2 (2)・`s2-07l.479.2`）。`dispatch ls` の 1 口が観測の面である。
- **(d)** driver の死亡（§5・s2-07l.352）: 札の書き・消し（`pipe run` / `resume` の入口と終端）・turn 関数の起こし直し・record token・`base_of_run` の typed 化。依存: (a)・(b)。

## 10. 却下案（ADR-0034 §5 の写しは持たない・設計固有のもの）

- 列を台帳の label（`dispatch:ready` 等）で持つ: 台帳に規律と状態を置く（C15）・label の継承で親から漏れる（PRIME R2）。列は器の記録。
- 介入を台帳の priority の書き換えで表す: priority は契約の性質、介入は一時の順序。混ぜると「なぜこの順か」が記録から消える。
- 起動条件を dispatcher が独自に再実装する: 交差と受付の判定が 2 か所になる（C2 違反・.303 の QUESTION の型）。既存の intake の関数を呼ぶ。
- 審査を起動の瞬間（`pipe run` の中・.241 の位置のまま）に撃ち、列の入力に審査を持たない: 契約の不備が起動が回ってきた時まで見えず、planner の待ち時間が捨てられ、FAIL の便が run N+1 まで列を塞ぐ（user 裁定 2026-09-15 13:4xZ で却下・planner の初案）。

## 11. 後続

- SRS FR30 の response と FR49 の condition の改訂・AC38 / AC39 の追加は SRS v0.14 で反映済み（FR68 の起動の形・lock・glossary の 受付 / priority / Reviewed は v0.15）。
- QUESTION と Gated FAIL の裁定（planner の手番）を速くする形は別設計（契約の改訂を器の口で持つ .133 の系）。
- ADR-0045 §2 (6) の SRS 改稿（FR30 の担い手・FR68 の契機を tick から便の終端と手動の 1 周へ・FR49 の condition は「便が intake を通ったとき」のまま）は user の /folio-architect の周。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "pipe dispatch の本体 — 列の導出（台帳 + 設計 pointer + 審査 FAIL の列外）・順序の 1 関数（first → priority → 起票順）・起動条件は intake の判定を再利用・first / hold / release の印の event kind 1 つ・dispatch ls"
req = ["FR30", "FR39", "FR49"]
section = "3"
touches = ["crate::fleet::EventKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", ".config/nextest.toml", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_"]
size = "M"
done = "偽の台帳と偽の live 便で、交差する便は Overlap で待ち交差しない便だけが起動の構築点に届き、first が priority より先に来て hold は起こさず、直前の便が Reviewed FAIL の契約は同じ sha では ReviewFailed で列外、台帳が読めない周は UNMEASURED で 0 本"

[[contract]]
id = "b"
title = "契機 — 便の終端（land / stop / retire・run と resume の終端）の直後・pipe dispatch の手動 1 周・first / release の記録の直後に同じ dispatch::turn を撃つ（tick は無い）"
req = ["FR30", "FR68"]
section = "5"
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/retire.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", ".config/nextest.toml", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_dispatch_"]
size = "S"
done = "偽 remote の toy repo で land と stop の終端の直後に列が 1 周撃たれて交差の解けた便が起動の構築点に届き、pipe dispatch の手動 1 周と first の記録の直後も同じ関数を撃ち、終端の中の 1 周が失敗しても終端の rc は変わらない"
depends = ["a"]

[[contract]]
id = "d"
title = "driver の死亡 — 札の書き・消し、turn 関数の起こし直し（pipe resume）、record token resumed:<m>、base_of_run の typed 化"
req = ["FR68", "FR14", "FR50"]
section = "5"
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/land.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_driver_"]
size = "M"
done = "driver を殺した便に dispatch の 1 周を撃つと pipe resume が 1 回起きて Landed まで通り record に resumed:1、札の無い live 便は起こし直さない"
depends = ["a", "b"]
<!-- contracts:end -->
