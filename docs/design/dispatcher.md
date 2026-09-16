# 設計: dispatcher — 審査を通った契約を器が自動で起こす（列・介入・観測）

- 出所: user 裁定 2026-09-15 12:5xZ（逐語は台帳 s2-07l notes）→ [ADR-0034](../../design-intent/decisions/ADR-0034-dispatcher-starts-reviewed-contracts-automatically.html)。
- 要件: [FR30](../../design-intent/spec/srs.html#FR30) 配送構造（担い手の移動・SRS の反映は user の /folio-architect の周）/ [FR39](../../design-intent/spec/srs.html#FR39) intake の排他 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR36](../../design-intent/spec/srs.html#FR36) 口座選定 / [NFR4](../../design-intent/spec/srs.html#NFR4)。
- 前提: 審査の段（[contract-source.md](./contract-source.md) §4・契約 (c) = s2-07l.241）と、設計 doc の契約表の行から intake が契約 file を生成する口（同 §2・契約 (b) = s2-07l.209）が Landed していること。後者が無いと契約 file は管理席の手順（bead の acceptance を写す script）でしか作れず、器は起動を組めない。
- この設計から出る契約: §9（4 便・(a) → (r) → (b)・(c) は (a) の後に並列）。裁定: user 2026-09-15 13:4xZ（審査は契約が出来た直後・逐語は台帳）/ 同 13:5xZ（順序は first → priority → 起票順・印は 3 つ・契機は tick と land・FR30 の改訂は /folio-architect の周）。

## 1. 何を解くか

便の起動が planner → 管理席 → launcher の中継に乗っている（[ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html) §2.1）。起動に要る判定＝台帳の依存・live 便との write-set の交差（intake の 1 関数）・受付（余地と host の memory）・審査の verdict は全部器が持っていて、席はそれを読み直して launcher を撃つだけだった。2026-09-15 の実測: 着地から次の起動まで便ごとに数分〜十数分の待ち、優先便の後回し（11:47Z）、器に無い「同時 3 便」の散文（11:5xZ）。本設計は起動を器の 1 関数に置き、席の産物を「契約・priority・介入」に絞る。

やさしく言うと: 「次はどれを起こすか」を人（席）が考えるのをやめ、器が既に持っている判定でそのまま起こす。planner は急ぐ便に印を付けるだけ。

## 2. 列の入力と順序

- **入力** = 台帳の open な bead のうち「依存が全部 closed ∧ acceptance が非空 ∧ `intake:memo` の label が無い ∧ acceptance に設計 pointer の行 `design = docs/design/<題>.md#<id>` が在る ∧ **現在の契約の sha に対する審査の verdict が PASS**」もの（.209 の `--design` と同じ字面・pointer の無い便は理由 `NoDesignPointer`・verdict の無い便は `NotReviewed { sha }`・FAIL / INCONCLUSIVE の便は `ReviewFailed { sha }`。列外の便も `dispatch ls` には理由付きで出す＝planner が直すべき契約が見える）。台帳の読みは rebrief と同じ子 process と同じ関数（`seat/rebrief.rs` の `read_ledger`・`bd --readonly list --limit 0 --json`・待ち上限は rules 行 `seat.ledger_timeout_s`）を共用し、読めない周は列を空と読まず `unmeasured` で止まる（NFR4・C10）。
- **審査の時点 = 契約が出来た直後**（user 裁定 2026-09-15 13:4xZ「planner が作ったらその直後に lens は審査すべき」・逐語は台帳 s2-07l notes）。審査の段（[contract-source.md](./contract-source.md) §4・契約 (c) = s2-07l.241 の `Stage::Reviewed`・同じ lens・同じ雛形 `headless/lens-contract.txt`・同じ観点 3 つ）を起動の瞬間でなく、契約 file が出来た直後に 1 回撃つ。verdict は契約 file の sha に紐づく event log の 1 kind `ContractReviewed { bead, sha, verdict }`（append-only・replay で bead ごとの最新 sha の verdict を導く・C6.3 と同じ store）。契機は 2 つ: (1) .209 の生成の口が契約 file を書いた直後（planner の焼き直しで acceptance の sha が変われば生成が走り直し、審査も走り直す）(2) dispatcher の 1 周が「現 sha に verdict の無い便」を見つけた時（取りこぼしを次の tick で埋める）。`pipe run` は intake の直後、同じ sha の PASS が在れば Reviewed をその記録で埋めて lens を撃ち直さず、sha が違えば .241 のとおり撃つ（C2 の 1 実装・審査を飛ばす flag は作らない・C16）。理由: 契約の不備は planner の手空きのうちに返す（起動の瞬間まで見えないと planner の待ち時間が捨てられ、FAIL が列を塞ぐ）。
- **順序** = 1 関数 `order(rows) -> Vec<Candidate>`: (1) 介入 `first` の便 (2) 台帳の `priority`（P0 → P4）(3) 起票順（id の数字）。同順は起票順。**散文の順序を持たない**（憲法 C2）。
- **hold** の便は列に載るが起こさない（理由 = `Hold`）。

## 3. 起動条件（器の判定の再利用・1 実装）

| 条件 | 器の既存の判定 | 落ちたときの理由（閉じた型 `WaitReason`） |
|---|---|---|
| 台帳の依存が閉じている | `bd --readonly ready` | `Dependency { on: Vec<bead> }` |
| live 便と write-set が交差しない | intake の排他（[pipeline-conflict.md](./pipeline-conflict.md) §2・FR39・`pipe/cli/intake.rs` の同じ関数） | `Overlap { with: run, files: n }` |
| 受付（余地・host の memory）を通る | intake の余地（[contract-source.md](./contract-source.md) §3）と受付札（[gate-cost.md](./gate-cost.md) §3.2） | `Admission { reason }` |
| 介入 hold が無い | 列の状態 | `Hold { since }` |
| 現 sha の審査が PASS（§2「審査の時点」） | event log の `ContractReviewed` の最新（replay）・.241 の Reviewed と同じ判定 | `NotReviewed { sha }` / `ReviewFailed { sha }` |
| 設計 pointer が在る | acceptance の `design = …` 行 | `NoDesignPointer` |

- 判定は intake の既存の関数（`pipe/cli/intake.rs` の `exclude_overlap` / `exclude_cap_shortfall`・`pipe/admission.rs` の `has_room`）を**記帳せずに**呼ぶ（可視性を `pub(super)` にする以外は不変・C2 の 1 実装）。通る便だけ既存の `pipe run --design <pointer> --bead <id> --repo <anchor>`（intake → Reviewed〔同じ sha の PASS を再利用・§2〕→ spawn）を撃つ。**dispatcher は起動の時機だけを決め、口座の選定（FR36）・受付の記帳・審査は従来の段がそのまま行う**。
- **数値の並列上限を持たない**（[ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)）。同じ契機で複数の便が条件を満たせば全部起こす（受付札が memory で縮退させる）。
- 1 周で起こした便は次の候補の交差の相手に入る（列を上から順に評価し、起こした便の write-set を live に足して次を評価する）。
- **anchor の作業木の汚れは起動条件に無い**（`s2-07l.367`・admin の実測 2026-09-15 21:51Z で 2 例目: planner の未 commit の design-intent 編集が launcher の手順 0「porcelain 0」で便を止めた）。便は base の sha から worktree を切る（[pipeline.md](./pipeline.md) §5.2）ので作業木の汚れは便に載らず、照合するのは **base の sha が `origin/main` の先端と一致するか**だけ（一致しない周は列の `WaitReason` でなく intake の stale base の断り＝既存の判定）。binary は base の sha で build した写しを使う（anchor の作業木で build した binary を便に渡さない）。

## 4. 介入の口（planner の typed な印）

`<NAME> pipe dispatch first <bead>` / `hold <bead>` / `release <bead>`。印は state dir の列の記録（event log の 1 kind `DispatchMark { bead, mark: First | Hold | Release, by: target }`）で、rebrief の DATA と `dispatch ls` に出る。user の直命「最優先」の対は `first`（planner が打つ・逐語は直命の表 [working-memory.md](./working-memory.md) §12.1 に残る）。印は台帳の priority を書き換えない（台帳は task と裁定・憲法 C15）。

## 5. 契機（tick と land の直後）

- **tick**: `seat/tick.rs` の `judge` の列に dispatch の段を 1 つ足す（s2-07l.304 の「hook 集合の軸」と同じ形・置き場は新 module `seat/tick/dispatch.rs`・宣言順の末尾＝席の判定の後）。席の状態と無関係に列を 1 周評価して起こせる便を起こし、`TickDecision` の variant は増やさず record 行に token `dispatch=started:<n>,waiting:<m>` か `dispatch=unmeasured` を 1 つ足す（`account=` の隣・C10）。どの席の tick が撃っても同じ関数で冪等（起こした便は live になり次の周の交差の相手になる）。
- **land の直後**: `pipe land` が Landed を記帳した直後に同じ関数を 1 周撃つ（着地で交差が解けた便を待たせない）。
- どちらも同じ 1 関数（dispatch module の turn 関数）を撃つ（C2）。lock は着地の列と同じ store の lock（fleet の store が持つ acquire）を使い、二重起動を防ぐ。
- **driver の死亡**（`s2-07l.352`・契約 (d)・C9 の便版の driver 側）: `pipe run` / `pipe resume` の process（driver）は入口で `<state_dir>/pipe/<run>/driver` に受付札と同じ本文（pid + 起動時刻・[gate-cost.md](./gate-cost.md) §3.2）を書き、終端で消す。turn 関数は live 便のうち札の所有者が死んでいる便（lock の所有者と同じ probe・`Owner::Dead`）を (a) と同じ引数の `pipe resume` で起こし直し、record token に `resumed:<m>` を足す（`dispatch=started:<n>,resumed:<m>,waiting:<k>`）。札が無い / 読めない便は触らない（測れないを「死んだ」に読み替えない・fail-closed）。schema を広げた便の Landed で古い binary の driver が typed に死ぬ周（NFR4・.160 の座礁 2026-09-15）も次の周に現在の binary で続く＝写し binary の refresh は要らない（走行中の process の code は変わらないので refresh は座礁を防がない）。`base_of_run` の読めなさは typed に呼び手へ返す（C10・「base が無い」と分ける）。

## 6. 観測

- `<NAME> pipe dispatch ls --state-dir S`: 列の各便を `[DISPATCH] bead=<id> prio=<p> mark=<first|hold|->` + `reason=<WaitReason>` で 1 行ずつ・`[DISPATCH-COUNT] total=<n> ready=<k>`・0 件は `[DISPATCH-NONE]`・台帳が読めない周は `[DISPATCH-UNMEASURED reason=…]`（0 件と融合しない・C10）。
- rebrief の DATA に同じ行を載せる（`Marker` の variant を宣言順で足す・[working-memory.md](./working-memory.md) §12.2 の現在地 DATA の隣）。planner の brief の slot `plan` の材料。

## 7. 極性

dispatcher は「起こす」側で行為を止める判定を持たない（起こせない便は理由付きで待つだけ）＝[ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1 の guard ではなく極性一覧に載せない（受付札と同じ扱い・[gate-cost.md](./gate-cost.md) §3.2）。台帳が読めない周は `unmeasured` で 1 本も起こさない（fail-closed 側に倒す・NFR4）。

## 8. 歯（`crates/<NAME>/tests/e2e/pipe/dispatch.rs`・`pipe_dispatch_` 接頭辞・名前の列は現物が SSOT）

- 順序: first → priority → 起票順の 1 関数（pure・in-file）。
- 起動条件: 偽の台帳（ready の出力 fixture）+ 偽の live 便（event log）で、交差する便は `Overlap` で待ち、交差しない便だけ `pipe run` の構築点が呼ばれる。
- 台帳が読めない周: `[DISPATCH-UNMEASURED]` で起動 0（0 件と区別）。
- 介入: `first` が priority より先に来る・`hold` は起こさない・`release` で戻る（event log の往復）。
- 契機: tick の軸が `decision=dispatch` を記録する・land の直後に 1 周撃たれる（偽 remote の toy repo）。
- 審査の時点（行 (r)・`pipe_review_contract_` 接頭辞）: 契約 file が出来た直後に偽 lens が 1 回撃たれ `ContractReviewed { sha }` が記録される・同じ sha で `pipe run` を撃つと lens の呼出 0 で Reviewed が PASS になる・sha が変わると撃ち直す・verdict の無い便は列外で `NotReviewed`。

## 9. 契約（4 便・(a) の後に (r)(b)(c)・(b) は (r) の後）

- **(a)** `pipe dispatch` の本体: 列の導出（台帳の list + acceptance の設計 pointer + `ContractReviewed` の最新）・順序の 1 関数・起動条件（intake の判定関数の再利用・可視性の変更）・`first / hold / release` の印と `ContractReviewed`（event kind 2 つ + `Event` の typed な field）・`dispatch ls`。依存: s2-07l.241 と s2-07l.209 Landed。
- **(r)** 審査の時点: 契約 file が出来た直後に審査を撃つ口（.209 の生成の直後 + dispatcher の 1 周）・`ContractReviewed` の記録・`pipe run` の同 sha 再利用。依存: (a)。**行 (r) は契約表に s2-07l.241 Landed 後に足す**（write-set が .241 の新設 file `pipe/review.rs` を編集するため、tracked になる前は行が解けない＝s2-07l.346 の罠）。
- **(b)** 契機: tick の軸 `dispatch` + `pipe land` の直後の 1 周。依存: (a)・(r)（列に PASS の便が無いと契機が空回りするだけなので (r) の Landed を待つ・行の `depends` は行 (r) と同時に足す）。
- **(c)** 観測: rebrief の DATA に `[DISPATCH]` の行（`Marker` の variant）。依存: (a)・s2-07l.326.1（現在地の DATA）Landed。
- **(d)** driver の死亡（§5・s2-07l.352）: 札の書き・消し（`pipe run` / `resume` の入口と終端）・turn 関数の起こし直し・record token・`base_of_run` の typed 化。依存: (a)・(b)。

## 10. 却下案（ADR-0034 §5 の写しは持たない・設計固有のもの）

- 列を台帳の label（`dispatch:ready` 等）で持つ: 台帳に規律と状態を置く（C15）・label の継承で親から漏れる（PRIME R2）。列は器の記録。
- 介入を台帳の priority の書き換えで表す: priority は契約の性質、介入は一時の順序。混ぜると「なぜこの順か」が記録から消える。
- 起動条件を dispatcher が独自に再実装する: 交差と受付の判定が 2 か所になる（C2 違反・.303 の QUESTION の型）。既存の intake の関数を呼ぶ。
- 審査を起動の瞬間（`pipe run` の中・.241 の位置のまま）に撃ち、列の入力に審査を持たない: 契約の不備が起動が回ってきた時まで見えず、planner の待ち時間が捨てられ、FAIL の便が run N+1 まで列を塞ぐ（user 裁定 2026-09-15 13:4xZ で却下・planner の初案）。

## 11. 後続

- SRS FR30 の response と FR49 の condition の改訂・AC38 / AC39 の追加は SRS v0.14 で反映済み（FR68 の起動の形・lock・glossary の 受付 / priority / Reviewed は v0.15）。
- QUESTION と Gated FAIL の裁定（planner の手番）を速くする形は別設計（契約の改訂を器の口で持つ .133 の系）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "pipe dispatch の本体 — 列の導出（台帳 + ContractReviewed の最新）・順序の 1 関数（first → priority → 起票順）・起動条件は intake の判定を再利用・first / hold / release の印と ContractReviewed の event kind・dispatch ls"
req = ["FR30", "FR39", "FR49"]
section = "3"
touches = ["crate::fleet::EventKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/seat/rebrief.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/seat/state.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_"]
size = "M"
done = "偽の台帳と偽の live 便で、交差する便は Overlap で待ち交差しない便だけが起動の構築点に届き、first が priority より先に来て hold は起こさず、現 sha の verdict の無い便は NotReviewed で列外、台帳が読めない周は UNMEASURED で 0 本"

[[contract]]
id = "b"
title = "契機 — tick の軸 dispatch と pipe land の直後の 1 周（同じ dispatch::turn を撃つ）"
req = ["FR30"]
section = "5"
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/seat/tick.rs", "+crates/scribe2/src/seat/tick/dispatch.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/seat/tick.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_tick_dispatch_", "cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_dispatch_"]
size = "S"
done = "tick が decision=dispatch を記録し、偽 remote の toy repo で land の直後に列が 1 周撃たれる"
depends = ["a", "r"]

[[contract]]
id = "c"
title = "観測 — rebrief の DATA に [DISPATCH] の行（Marker の variant・現在地 DATA の隣）"
req = ["FR23"]
section = "6"
touches = ["crate::seat::rebrief::Marker"]
also = ["crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_rebrief_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_wm_rebrief_"]
size = "S"
done = "偽の列で rebrief が [DISPATCH] を件数付きで出し、読めない周は [DISPATCH-UNMEASURED]"
depends = ["a"]

[[contract]]
id = "r"
title = "審査の時点 — 契約 file が出来た直後に審査を撃ち ContractReviewed を記録し、pipe run は同じ sha の PASS を再利用する"
req = ["FR49", "FR68"]
section = "2"
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_review_contract_"]
size = "S"
done = "契約 file が出来た直後に偽 lens が 1 回撃たれ verdict が sha に紐づいて記帳され、同じ sha の pipe run は lens の呼出 0 で Reviewed が埋まり、sha が変わると撃ち直す"
depends = ["a"]

[[contract]]
id = "d"
title = "driver の死亡 — 札の書き・消し、turn 関数の起こし直し（pipe resume）、record token resumed:<m>、base_of_run の typed 化"
req = ["FR68", "FR14", "FR50"]
section = "5"
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/seat/tick/dispatch.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/land.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_driver_"]
size = "M"
done = "driver を殺した便に dispatch の 1 周を撃つと pipe resume が 1 回起きて Landed まで通り record に resumed:1、札の無い live 便は起こし直さない"
depends = ["a", "b"]
<!-- contracts:end -->
