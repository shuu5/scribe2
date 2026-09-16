# 設計: 台帳の棚卸し（memo の判定点と齢）を rebrief の DATA に載せる — 埋没を席の記憶でなく器の marker で塞ぐ

- 要件: [FR23](../../design-intent/spec/srs.html#FR23) 作業記憶の作法（rebrief の DATA）/ [FR51](../../design-intent/spec/srs.html#FR51) 台帳の lint / [AC8](../../design-intent/spec/srs.html#AC8)
- 憲法: [C15](../../design-intent/spec/constitution.html#c15) 台帳は task と裁定だけ（本 doc は台帳に規律を置かない・読むだけ）/ [C10](../../design-intent/spec/constitution.html#c10) 齢・件数は実測値で出所付き / [C1](../../design-intent/spec/constitution.html#c1) 閾値は rules 行 / [C5](../../design-intent/spec/constitution.html#c5) 行の追加は裁定 id 付き / [N2](../../design-intent/spec/constitution.html#n2) 順序と判定点を散文（WM）に置かない
- 決定: [ADR-0018](../../design-intent/decisions/ADR-0018-working-memory-is-a-typed-carrier-with-pointer-provenance.html)（rebrief の DATA は typed 行・器が事実を並べ skill が読む）/ [ADR-0023](../../design-intent/decisions/ADR-0023-contract-source-is-the-design-document-table.html) §2.2（台帳の bead は id / status / 裁定 / notes・契約の本文は設計 doc）
- この設計から出る契約: §9 の (a)（marker 2 種 + rules 行）→ (b)（`bd` の依存 field の読み手）。

## 1. 何を解くか

2026-09-13 の実測（planner 席）: open 65 本のうち memo（label `intake:memo`）が 53 本。user 直命（2026-09-12 03:05Z）から出た memo `s2-07l.142` は契約 (a)(b) の land（2026-09-13 07:57Z）の後 4.5 時間、契約 (c)(d) が切られないまま残り、user が問い詰めて初めて起票された。順序の判断は退避物（WM）の散文にだけあり、台帳にも rebrief の DATA にも「この memo はいつ判定するか」が無かった。同じ周に、憲法監査の memo 19 本の priority field が題の P より 1 段低く沈み、`bd ready` の並びで契約に混ざって見えなくなっていた。

memo は「確定度が低いものを低いまま台帳に置く型」として要る（契約の形に押し込むと嘘の契約になる）。無くすのでなく、**memo の状態遷移（いつ判定するか・判定を過ぎているか）を器が測って marker で出す**。判定は席（planner）と user の手番のまま、見落としだけを塞ぐ。

## 2. 判定点は bd の blocks 依存で表す（新しい field を作らない）

- memo の「判定点」= その memo が **blocks 依存で待つ bead**（`bd dep add <memo> <待つ bead>`・既存の口・種別は `dependency_type = blocks`）。「.209 の Landed ∧ folio2 要件書の承認」のように台帳の外の条件が混ざる周は、台帳側の bead だけを依存に張り、外の条件は description に書く（器は台帳の依存だけを測る・C15）。
- **判定を過ぎた memo（due）** = blocks 依存を 1 つ以上持ち、その全部が `closed` の memo。器は `bd --readonly list --limit 0 --json` の `dependencies[]`（`id` / `status` / `dependency_type`）だけを読む（bead の本文は読まない）。
- **齢を過ぎた memo（stale）** = blocks 依存を 1 つも持たず、priority field が rules 行 `ledger.memo_stale_priority`（閾値・以下）で、`updated_at` から rules 行 `ledger.memo_stale_days`（日・以上）が経った memo。判定点を持たない memo は「N 日ごとに席が見る」のが既定＝据え置きも判定であり、据え置いたら notes を 1 行足す（`updated_at` が動く）。
- parent-child 依存（epic への所属）は判定点に数えない。閉じた memo は母集団に入れない（`list` は open だけを返す・closed は「台帳に無い」でなく「閉じている」＝rebrief の `[DIFF] unknown` と同じ読み）。

## 3. rebrief の DATA に marker を 2 種足す（[working-memory.md](./working-memory.md) §5.2 の隣）

- `[MEMO-DUE] <id> p=<P> blocks=<閉じた依存 id の列> updated=<ts>`（1 memo 1 行・id の数字順）と件数行 `[MEMO-DUE-COUNT] n=<件> of=<memo 母集団>`。
- `[MEMO-STALE] <id> p=<P> age_days=<実測> updated=<ts>` と `[MEMO-STALE-COUNT] n=<件> of=<memo 母集団>`。
- 0 件は `[MEMO-DUE-NONE]` / `[MEMO-STALE-NONE]`（確認した上で 0）。台帳を読めない周は既存の `ledger-unreadable`（rc 2）のまま＝marker を出さない（`-EMPTY` と `-NONE` を融合しない・既存の読み方）。
- memo の弁別 = label `intake:memo`（[PRIME.md](../../.beads/PRIME.md) R3）だけ。題の `[memo]` は読まない（字面の語彙で判定しない）。
- 出力順は既存の `[BD-COUNT]` の直後（marker の宣言順 = `Marker` enum の宣言順・C2）。
- **skill 側の読み方（本 repo の外・形だけ）**: `[MEMO-DUE]` は「次のアクション」に必ず 1 行載る（契約化 / 統合 / 据え置き〔notes 1 行〕/ 閉じる のどれかを planner が決めて台帳に書く）。`[MEMO-STALE]` は「hygiene」に列挙。器は決めない。

## 4. rules 行（C1・値は user 裁定・C5）

| id | kind | shape | 値 | 意味 |
| --- | --- | --- | --- | --- |
| `ledger.memo_stale_days` | `MemoStaleDays`（新 variant・末尾） | Int | 裁定待ち（§9 (a)） | 判定点を持たない memo を stale と数える `updated_at` からの日数（以上） |
| `ledger.memo_stale_priority` | `MemoStalePriority`（新 variant・末尾） | Int | 裁定待ち（§9 (a)） | stale を数える priority field の上限（以下・P0 = 0 … P4 = 4） |

読み手 = `seat rebrief`（§3）。値の正本は manifest・本 doc は写さない。行の追加は rules の外形 snapshot（kind +2・行 +2）と kind 件数の歯を動かす（write-set は構造の連鎖で §9 に書く）。

## 5. priority field と題の P の drift（本便の外・記録）

題の `[P1]` と priority field が食い違う memo が 20 本あった（2026-09-13 実測・field を題に揃えて訂正済み）。字面（題）は判定入力にしない（C3.3 の型と同じ）ので、器は field だけを読む。題に P を書く作法は PRIME.md の側（台帳の作法）で、本 doc は扱わない。

## 6. 極性

marker は判定行（inject / noop の判定と同じ「出すだけ」）で guard ではない＝極性一覧に載せない（[polarity.md](./polarity.md)）。台帳が読めない周は既存の `ledger-unreadable` で rc 2（FailClosed のまま・件数を 0 に潰さない）。

## 7. 歯（`crates/<NAME>/tests/e2e/seat.rs`・接頭辞 `seat_rebrief_memo_`・fixture の bd は既存の偽 bd〔JSON を返す script 相当の fixture・現物の名は SSOT〕）

- due: blocks 依存が全部 closed の memo が `[MEMO-DUE]` に出る・1 つでも open なら出ない・parent-child だけの memo は出ない・件数行の `of=` が memo の母集団と一致。
- stale: 依存なし ∧ priority ≤ 閾値 ∧ 齢 ≥ 閾値 の memo が `[MEMO-STALE]` に出る・齢が閾値未満は出ない・priority が閾値超は出ない・`intake:memo` の無い bead は母集団に入らない。
- 0 件は `-NONE`・台帳が読めない周は marker なし・rc 2（既存の歯が緑のまま）。
- 齢の境界は壁時計の等号で pin しない（fixture は閾値 ± 1 日・[wall-clock の flaky](./seat-state.md) と同型）。
- 外形 snapshot（rebrief の marker 一覧・rules の外形）の更新。

## 8. 却下案

- memo に「判定点」の専用 field を持たせる（bd に無い・台帳に規律を置く形・C15）→ blocks 依存で表す。
- 器が memo を自動で契約化 / close する → 判定は席と user の手番（A1 の「消す」に当たる close を器が撃たない）。marker で出すまで。
- 題の `[memo]` / `[P1]` を読む → 字面の語彙で判定しない。label と field だけ。
- 齢の基準を `created_at` にする → 据え置きの判定（notes 1 行）で更新される `updated_at` の方が「席が最後に見た時刻」を表す。

## 9. 契約（2 便・この順）

- **(a) marker と rules 行**（S）: `Marker` に 6 variant（DUE / DUE-COUNT / DUE-NONE / STALE / STALE-COUNT / STALE-NONE）・`RuleKind` に 2 variant・manifest に 2 行（値と裁定 id は user 裁定）・`Issue` に `priority` / `labels` / `dependencies`（`id` / `status` / `dependency_type`）を足す・判定の純関数（入力は `Issue` の列と閾値と now・I/O なし・in-file の歯 + property）・`ledger_lines` の隣に出力。write-set = seat/rebrief.rs・rules/mod.rs・rules/manifest.toml・tests/e2e/seat.rs・tests/e2e/rules.rs（kind 件数）・snapshot（rebrief の外形・rules の外形）。依存: なし（seat/rebrief.rs を触る便が live なら直列）。
- **(b) skill 側の読み方**（運用・便ではない）: rebrief skill §3 の「次のアクション」と「hygiene」に marker の読み方を 1 行ずつ足す（本 repo の外・(a) の land 後・user の手番＝skill は user の設定 file）。

## 10. 後続

- 同じ設計 doc を指す memo が N 本以上になったら「設計 doc 1 本に畳む候補」を出す（束ねの検出線・memo の本文の pointer を読む必要があり本 doc の「本文は読まない」の外）。
- 契約表（[contract-source.md](./contract-source.md)）が正本になった後、memo の「契約化」は設計 doc に行を足すことになる＝(a) の marker はそのまま、読み方だけ skill 側で変わる。

## 11. memo の列 — rebrief の DATA に `[MEMO-QUEUE]`（着手可能な memo を priority → 齢 → id の順）を足す（契約表の行 a・`s2-07l.369`）

- 何が起きているか: user 指摘 2026-09-15 22:3xZ（要旨: 溜まった memo を処理する planner の構造的な仕組みは在るか・逐語は台帳 `s2-07l.369`）。§3 の 2 種は「判定点を過ぎた」と「齢を過ぎた」の 2 面で、**いま契約化に着手できる memo をどの順で**は出さない（実測 2026-09-15: memo 26 本・`[MEMO-DUE]` 0・`[MEMO-STALE]` 0＝planner が rebrief のたびに台帳の一覧を手で数える）。契約 34 本の側は [dispatcher.md](./dispatcher.md) §6（`[DISPATCH]`・列外の理由）が出す。現物（verified・main a620600）: `seat/rebrief.rs` の `triage`（純関数・I/O なし）は `Triage`（due / stale / memo_total / unreadable）を返し `memo_lines` が marker に描く。閾値は rules 行 `ledger.memo_stale_days` / `ledger.memo_stale_priority`。
- 形: (1) `Triage` に **queue** の列を足す = open ∧ label `intake:memo` ∧ **着手可能**（blocks 依存を持たないか、持つなら全部 closed）∧ priority ≤ `ledger.memo_stale_priority`（閾値は既存の行を共用・新しい行を足さない・C1 / C5）の memo。順序 = priority の昇順 → 齢（`updated_at` からの経過）の降順 → id の数字順（同点の周・既存の `id_key`）。due / stale と重なってよい（面が違う: due = 判定点、stale = 齢、queue = 着手順）。(2) marker 3 種（`Marker` の宣言順の末尾・`[MEMO-STALE-NONE]` の後・`as_str` / `ALL` / 網羅 match）: `[MEMO-QUEUE] <id> p=<P> age_days=<実測|-> blocks=<閉じた依存の列|-> updated=<ts>`（1 memo 1 行・上の順）/ `[MEMO-QUEUE-COUNT] n=<件> of=<memo 母集団> unreadable=<updated_at を読めず齢を測れなかった件数>`（齢が測れない memo は列の末尾に `age_days=-` で載せる＝落とさない・C10）/ `[MEMO-QUEUE-NONE]`（確認した上で 0）。台帳を読めない周は既存の rc 2（marker なし）。(3) skill 側の読み方（本 repo の外・形だけ）: brief の「次のアクション」の memo の項は `[MEMO-QUEUE]` の先頭から取る（器は決めない）。
- 触らない: due / stale の判定と marker の字面・rules 行の値・`[DISPATCH]`（契約の側）・台帳の write（列は読むだけ・起票も label も動かさない）・`Issue` の field（既存の priority / labels / dependencies / updated で足りる）。
- 歯（`seat_rebrief_memo_queue_` 接頭辞・`tests/e2e/seat/wm.rs`・偽 bd は既存の fixture〔JSON を返す script〕）: 依存なし P1 と blocks 全 closed P0 と blocks に open を持つ P0 の 3 memo → 列は P0（closed の依存）→ P1 の 2 本で open の依存を持つ memo は出ない／priority 閾値超の memo は出ない／同 priority は齢の降順／`updated_at` の壊れた memo は末尾に `age_days=-` で出て `unreadable=1`／0 件は `-NONE`・`of=` は母集団／property: queue ⊆ memo 母集団 ∧ 列は priority で非減少／snapshot の marker 一覧に 3 行増える。
- 却下案: 新しい rules 行で queue の閾値を持つ（裁定 id が要り既存の行で足りる）／件数の上限で切る（切った分が見えない・列は全部出し読み手が先頭を取る）／queue を due / stale と排他にする（面が違う・排他にすると due の memo が列から消える）／`[DISPATCH]` と 1 つの marker に畳む（契約と memo は列の入力も順序も別・dispatcher.md §2）／齢の測れない memo を落とす（測れないを 0 件に化けさせる・C10）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "memo の列 — rebrief の DATA に [MEMO-QUEUE]（着手可能 ∧ priority ≤ ledger.memo_stale_priority の memo を priority → 齢 → id の順）と件数行・NONE を足す"
req = ["FR23"]
section = "11"
touches = ["crate::seat::rebrief::Marker"]
tests = ["crates/scribe2/tests/e2e/seat/wm.rs"]
also = ["crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_rebrief_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_rebrief_memo_queue_"]
size = "S"
done = "偽 bd の memo 3 本で [MEMO-QUEUE] が着手可能な 2 本を priority 順に出し、open の依存を持つ memo は出ず、齢の測れない memo は末尾に age_days=- で載り、件数行の of= が母集団と一致し、0 件は -NONE"
<!-- contracts:end -->
