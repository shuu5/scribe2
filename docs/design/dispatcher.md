# 設計: dispatcher — 審査を通った契約を器が自動で起こす（列・介入・観測）

- 出所: user 裁定 2026-09-15 12:5xZ（逐語は台帳 s2-07l notes）→ [ADR-0034](../../design-intent/decisions/ADR-0034-dispatcher-starts-reviewed-contracts-automatically.html)。
- 要件: [FR30](../../design-intent/spec/srs.html#FR30) 配送構造（担い手の移動・SRS の反映は user の /folio-architect の周）/ [FR39](../../design-intent/spec/srs.html#FR39) intake の排他 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR36](../../design-intent/spec/srs.html#FR36) 口座選定 / [NFR4](../../design-intent/spec/srs.html#NFR4)。
- 前提: 審査の段（[contract-source.md](./contract-source.md) §4・契約 (c) = s2-07l.241・Landed）と、設計 doc の契約表の行から intake が契約 file を生成する口（同 §2・契約 (b) = s2-07l.209）が Landed していること。後者が無いと契約 file は手書きでしか作れず、器は起動を組めない。
- この設計から出る契約: §9（3 便・(a) → (b) → (d)）。裁定: user 2026-09-15 13:5xZ（順序は first → priority → 起票順・印は 3 つ・FR30 の改訂は /folio-architect の周）。**[ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 の後の形**（user 裁定 2026-09-18・逐語は台帳）: 席は orchestrator 1 つで管理席は無く、便の起動・着地・merge は器の dispatcher が行う。管理 tick は消えた（`s2-07l.479.1`）ので契機は便の終端と手動の 1 周（§5）。「契約が出来た直後の審査」（旧 (r)・`ContractReviewed`）は持たず、審査は `pipe run` の Reviewed の段のまま（§2）。本 doc の「planner」は orchestrator を指す（改名は ADR-0045・`s2-07l.478`）。

## 1. 何を解くか

便の起動が planner → 管理席 → launcher の中継に乗っている（[ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html) §2.1）。起動に要る判定＝台帳の依存・live 便との write-set の交差（intake の 1 関数）・受付（余地と host の memory）・審査の verdict は全部器が持っていて、席はそれを読み直して launcher を撃つだけだった。2026-09-15 の実測: 着地から次の起動まで便ごとに数分〜十数分の待ち、優先便の後回し（11:47Z）、器に無い「同時 3 便」の散文（11:5xZ）。本設計は起動を器の 1 関数に置き、席の産物を「契約・priority・介入」に絞る。

やさしく言うと: 「次はどれを起こすか」を人（席）が考えるのをやめ、器が既に持っている判定でそのまま起こす。planner は急ぐ便に印を付けるだけ。

## 2. 列の入力と順序

- **入力** = 台帳の open な bead のうち「依存が全部 closed ∧ acceptance が非空 ∧ `intake:memo` の label が無い ∧ acceptance に設計 pointer の行 `design = docs/design/<題>.md#<id>` が在る ∧ **同じ契約 file の sha で終端に着いた便が無い**」もの（.209 の `--design` と同じ字面・pointer の無い便は理由 `NoDesignPointer`・直前の便が終端〔`Landed` / `Failed` / `Stopped`、および審査や gate の判定で終端になった段〕で終わり契約 file の sha が変わっていない便は `Settled { sha, stage }`＝既存の event log（replay の段）と run dir の契約 file の sha から導く・終端かの判定は受付と同じ 1 本・新しい event kind は足さない。列外の便も `dispatch ls` には理由付きで出す＝planner が直すべき契約が見える）。
- **終端の便を列外にするのは無限再起動を塞ぐためである**（`s2-07l.366`）: 便が終端に着くと live でなくなって交差が消えるが、bead は台帳で `open` のまま（器は台帳に書かない・C15）なので、終端が来るたびに同じ契約が起こし直される（着地から close までの間ずっと）。契約が改訂されて sha が動けば列に戻る＝「直すまで起こさない」で `Landed`（済んでいる）・`Failed`（契約を直すまで再試行しない）・`Stopped`（人が止めた）のどれも筋が通る。台帳の読みは席の指示文の `{ledger}` と同じ子 process と同じ関数（`read_ledger`・`bd --readonly list --limit 0 --json`・待ち上限は rules 行 `seat.ledger_timeout_s`・置き場は `seat/ledger.rs`）を共用し、読めない周は列を空と読まず `unmeasured` で止まる（NFR4・C10）。
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
| 同じ契約 file の sha で終端に着いた便が無い（§2） | replay の段と受付と同じ終端の判定（`cli::live`）と run dir の契約 file の sha | `Settled { sha, stage }` |
| 設計 pointer が在る | acceptance の `design = …` 行 | `NoDesignPointer` |

- 判定は intake の既存の関数（`pipe/cli/intake.rs` の `exclude_overlap` / `exclude_cap_shortfall`・`pipe/admission.rs` の `has_room`）を**記帳せずに**呼ぶ（可視性を上げる以外は不変・C2 の 1 実装）。列は `pipe::cli` の兄弟なので可視性は `pub(in crate::pipe)` で、交差は断りでなく事実を返す 1 本（`crossings`）に割って受付と共用する（受付の断りの形は不変）。余地は受付の判定 1 本（`judge`）を置き場なしで撃つ（交差は上で測り済み・同じ周に store を 2 度読まない）。
- **§3 の表に無い断り**（宣言が上限に外れる・行の欄が不備・base を読めない）は `Admission { reason: <断りの名> }` に寄せる（実装 `s2-07l.345`）: variant を増やさず、名は受付の断りの名（`Refuse::as_str`）そのままで、`dispatch ls` から planner が直すべき契約が見える。通る便だけ既存の `pipe run --design <pointer> --bead <id> --repo <anchor>`（intake → Reviewed → spawn・審査は .241 のまま）を撃つ。**dispatcher は起動の時機だけを決め、口座の選定（FR36）・受付の記帳・審査は従来の段がそのまま行う**。
- **並列の上限は write-set の交差・直列依存・受付の枠と、host の同時走行の最大値 1 つ**（[ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)・[ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)）。最大値は rules 行 `pipe.max_live` が持ち、受付が live な便を数えて断る（[gate-cost.md](./gate-cost.md) §24）。同じ契機で複数の便が条件を満たせば最大値まで全部起こす（受付札が memory で縮退させる）。最大値で断られた契約は列に留まり、その理由の variant は行 a の Landed 後に足す（gate-cost.md §24 (4)）。
- 1 周で起こした便は次の候補の交差の相手に入る（列を上から順に評価し、起こした便の write-set を live に足して次を評価する）。
- **anchor の作業木の汚れは起動条件に無い**（`s2-07l.367`・admin の実測 2026-09-15 21:51Z で 2 例目: planner の未 commit の design-intent 編集が launcher の手順 0「porcelain 0」で便を止めた）。便は base の sha から worktree を切る（[pipeline.md](./pipeline.md) §5.2）ので作業木の汚れは便に載らず、照合するのは **base の sha が `origin/main` の先端と一致するか**だけ（一致しない周は列の `WaitReason` でなく intake の stale base の断り＝既存の判定）。binary は base の sha で build した写しを使う（anchor の作業木で build した binary を便に渡さない）。

## 4. 介入の口（planner の typed な印）

`<NAME> pipe dispatch first <bead>` / `hold <bead>` / `release <bead>`。印は state dir の列の記録（event log の 1 kind `DispatchMark { bead, mark: First | Hold | Release }`＝`run` を持たない行・`mark` は `Event` の typed な field で、自由文の `detail` を判定入力にしない〔C3.3〕）で、`dispatch ls` に出る（退避物の復元の DATA に出す案は §6 のとおり超過した）。**打ち手の名（`by`）は持たない**（実装 `s2-07l.345`）: 口は権能なし（§5・ADR-0045 §2 (1)）で誰が撃っても同じ印になり、器は env も HOME も読まない（C2.2）ので、正直に書ける出所が無い。user の直命「最優先」の対は `first`（planner が打つ・逐語は台帳に残る）。印は台帳の priority を書き換えない（台帳は task と裁定・憲法 C15）。

## 5. 契機（便の終端と手動の 1 周・tick は無い）

- **便の終端**: 便が live で無くなる瞬間（`Landed` / `Failed` / `Stopped` / retire・`pipe run` `pipe resume` `pipe land` `pipe stop` `pipe retire` の終端の記帳の直後）に同じ関数を 1 周撃つ。着地や停止で write-set の交差が解けた便を待たせない。
- **手動の 1 周**: `<NAME> pipe dispatch --state-dir S --repo R` は権能なしの口（誰が撃っても同じ 1 周・起動の権能は turn 関数の中の器の判定であって席の権能ではない・ADR-0045 §2 (1)）。host の再起動や driver の死亡（下）の後に人が撃つ。
- **印の直後**: `first` / `release` の記録の直後にも 1 周撃つ（印を付けた便を次の終端まで待たせない）。
- **1 周の材料の読み**: turn 関数は repo の材料（tracked の一覧・sources・snapshots・契約表の facts）を 1 周につき 1 回だけ読み、候補ごとに `generated` と `judge` で読み直さない（候補の数に比例して 1 周が伸びるのを止める・(a) の実測は台帳 `s2-07l.345` の notes・行 (b)）。判定の関数は不変で、材料を受け取る引数を足すだけ（C2）。
- **起こす口と見る口を分ける**: 契機（終端・手動の 1 周・印の直後）は**起こす側**を撃ち、観測の `dispatch ls` は**同じ判定を撃って起こさない**（見るだけで便が起きない・§6）。判定は 1 本のままで、違いは起こすかどうかだけである（C2）。
- **列の材料は引数で名指された周だけ解く**: 置き場と repo のどちらかでも引数に無い周は 1 周を撃たない（cwd へ落ちない・`s2-07l.366` の実測 2026-09-19: cwd へ落ちた終端が、別の repo の契約を toy の置き場へ起こした）。列は「この置き場の便」と「この repo の契約」を突き合わせる口なので、片方を推すと起こす先がずれる（fail-closed・NFR4）。
- **起こす便へ渡す道具**: 列に渡された `--rules` / `--lens` / `--runner` は起こす便へそのまま渡す（列と便が同じ道具で動く）。**`--runner` が無い周は 1 本も起こさない**（`pipe run` は実装役の口を必須にし、器は既定を持たない〔宣言にも rules 行にも無い・`s2-07l.366` の実測〕。列が既定を作ると「何を起こすか」が契約の外で決まる・C5 / C1）。起こせないと分かっている周は**台帳も読まない**——読んでも起こせず、便の終端ごとに子 process の読みが 1 回乗る（実測: e2e 全体が 21 秒 → 111 秒）。1 本も起こさなかった事実は `unmeasured` の理由で名乗り、`0 件`とは言わない（C10）。列を見る口は `dispatch ls` の側である。
- **`pipe run` / `pipe resume` の終端の 1 周は、その process 自身の道具を使う**（driver は自分の `--rules` / `--lens` / `--runner` を知っている）。`land` / `stop` / `retire` の終端と手動の 1 周は、渡された引数の道具だけを使う。
- **管理 tick の契機は無い**（tick は `s2-07l.479.1` で消えた・ADR-0045 §2 (2)）。時計で撃つ契機を足さない（C17.2: 足すなら先に消すものを名指す）。
- 全部同じ 1 関数（dispatch module の turn 関数）を撃つ（C2）。終端の中から撃つ 1 周は終端の記帳の後・lock の外で行い、失敗しても終端の rc を変えない（起こせなかった便は次の契機で拾う・観測は §6）。
- **二重起動を止めるのは受付の入口の排他である**（ADR-0019 §2.1「入口で排他する」・`s2-07l.366`）: 1 周は lock を取らない——子を起こす間じゅう着地の列と同じ lock を握ることになるうえ、**取っても効かない**（1 周は子を待たないので、lock を離した時点ではその子はまだ受付を通っていない＝次の 1 周からは live な便が見えない）。不変条件は**記帳する 1 か所**に置く: 受付は `judge` → `create` を入口の lock（event log の lock とは別の 1 file）の内側で atomic に通し、同時に来た 2 便の 2 本目は run id の衝突（`DuplicateRun`）か write-set の交差（`WriteSetOverlap`）で必ず落ちる。**起こす前の判定（列）と受付の判定は同じ 1 本**で、列が記帳しない側・受付が記帳する側である（二重に守る・planner 裁定 2026-09-19）。入口を閉じない形は実測で破れる（契機を同時に 2 回撃つと 20 回に 1 回、同じ bead の便が 2 本できた）。
- **driver の死亡**（`s2-07l.352`・契約 (d)・C9 の便版の driver 側）: `pipe run` / `pipe resume` の process（driver）は入口で `<state_dir>/pipe/<run>/driver` に所有者の pid を書く（生死の判定は lock の所有者と**同じ 1 本**・[gate-cost.md](./gate-cost.md) §3.2 の受付札と同じ読み方）。turn 関数は live 便のうち札の所有者が**もう駆動していない**便を (a) と同じ道具の `pipe resume` で起こし直し、record token に `resumed:<m>` を足す（`dispatch=started:<n>,resumed:<m>,waiting:<k>`）。札が無い / 読めない便は触らない（測れないを「死んだ」に読み替えない・fail-closed）。
- **札が残るのは driver が死んだ周だけである**: 正常に抜けた process は `Drop` で自分の札を外す（消すのは自分の pid を持つ札だけで、同じ便に別の driver が後から入っていればその札は落とさない）。残った札の所有者が居なければ、その便は駆動する者を失っている。
- **便の自走は起こす側の引数で選ぶ**（行 (e)・`s2-07l.485`）: `pipe run` / `pipe resume` に typed な flag `--drive` を足し、**これを持つ driver だけ**が終端の 1 周で自分の便を次の driver に渡す。列が起こす便（起こす側・起こし直す側・継ぎの子＝`spawn_self` の argv）には**常に** `--drive` を付ける——道具の pass-through（値を持つ flag の対の配列・全部か皆無か）とは別の定数で、列の判断で起きた便は自走する。値なし flag の読み手は src に 2 箇所（`stop.rs` の `--all`・`fleet/usage.rs` の `--show`）在り `--drive` は 3 本目なので、pipe の中の 2 本を `args.rs` の 1 本に畳む（`usage.rs` は別の module 木で `args.rs` は pipe の中にしか見えない・args.rs の doc の規律「4 本目が要るときは 1 本へ畳む」・C17。`size.rs` の 2 つの照合は test の偽 git が git の引数を読む行で、CLI の flag reader ではない＝admin の実測 2026-09-19）。`resume` の段の前進は同じ process の中で進む（`relaunch` → `launch`）ので、run.rs / resume.rs は flag を読む側として触るだけである（admin の実測 2026-09-19）。flag の無い run / resume は今までどおり**1 段だけ**進めて抜ける＝段を手で 1 つずつ進める既存の歯（終端の口を撃つ歯 205 本／母集団 786 本・`gate_once` 79 箇所・`land_once` 51 箇所・`implemented` 52 箇所・admin の実測 2026-09-19）は 1 本も触らない。自走を段で見分ける形（(d) で試した）は、自走の周が手動の段の間に割り込み、触っていない歯まで落とす（`s2-07l.482` の実測: 全体の歯が 3 周で 1 周しか緑にならず、5 便を land する歯が `land` の CAS 外れで落ちた）。`--repo` を渡さずに 1 周を止める形は `land` が repo を要るので使えない。列（起こす側と起こし直す側）が起こす便は**必ず `--drive` を持つ**＝(d) の起こし直しは 1 回で終わらず Landed まで続く（`s2-07l.482` の実測で `Implemented` で止まった件の直し・AC38「再開が 1 回だけ起きて Landed まで通る」）。C17.2 で消すもの: (d) の「起こし直しは 1 回・続きは人か次の契機が起こす」の但し書きと、段ごとに人が撃つ手動の 1 周。
- **渡す周と渡さない周**（行 (e)）: 渡すのは「自分が段を 1 つ進めた ∧ 進めた先が待ちの段（`WAITING`）でない ∧ 終端でない」周だけ。**前進なし**（入口で読んだ段と終端で読み直した段が同じ）の周は渡さない——渡すと同じ段を空撃ちする子が無限に連なる。段の前進は pure fn 1 本（入口の段と終端の段の 2 値・in-file の歯）で判じ、渡さなかった周は理由（`waiting` / `settled` / `no-progress`）を record token に載せる（C10・黙って止まらない）。終端の判定は `Settled` が、待ちの段は `WAITING` が既に持つ（新しい段も rules 行も足さない）。
- **渡し方は 1 周の関数 1 本のまま**（C2）: turn の Input に「呼び手が渡す自分の便」（run id・`--drive` で段を進めた周だけ `Some`）を足し、その便は札の所有者（＝呼び手）が生きていても起こし直しの候補に入れる（(d) の候補の規則は不変・足すのは呼び手の 1 便だけ）。札の寿命は変えない（`Drop` で外す・継ぎの子は親が抜けるまで待って札を取る〔下の `DeadOnly` の項〕）。
- **1 段進めた driver は終端の 1 周で自分の便を次の driver に渡す**（自分の札は継ぎの対象である・`--drive` の周）。終端の直後に撃つ 1 周は、その便を駆動していた process（＝自分）が仕事を終えて抜ける直前に走る。自分の札を「生きている」と読むと、1 段進めて抜ける driver の後を誰も継がない（`s2-07l.482` の実測: 起こし直した便が `Implemented` で止まった）。親がまだ抜けきる前に子が同じ便を起こしても害は無い——親は既に自分の段を記帳し終えていて以後 1 件も記帳しないので、重なるのは「読む」側だけである。
- **札は器の唯一の lock 実装（`create_new`）で取る**（`s2-07l.482`）: **生きている driver が握っている札は取れない**ので、走っている driver の隣にもう 1 本は立たない。排他は `s2-07l.366` の受付の入口と同じく**記帳する側**（札を握る側）に置く——1 周の側で閉じても効かない（1 周は lock を取らず、起こし直した子は別 process だからである）。
- **死んだ所有者の札の回収は原子的でない**（`s2-07l.482` の lens の指摘・実測: 死んだ札を読んだ 2 本の起こし直しが同じ便に runner を 2 本起こした〔起動試行 2 回で 3 回中 2 回〕）。回収が `remove_file` → `create_new` の 2 手なので、同じ死んだ札を読んだ 2 本が両方外して両方取れる（後の外しが先の書いた新しい札を消す）。起こし直しの経路は定義上いつも死んだ所有者なので、**この面はまだ閉じていない**。器の唯一の lock 実装の面であり、受付の入口（`s2-07l.366`）と受付札（gate-cost.md §3.2）にも同じ穴が在るので、**別便（[fleet-event-log.md](./fleet-event-log.md) §4「回収は 1 手」・行 c・`s2-07l.486`・Landed）で lock の回収そのものを直した**（`rename` で置き換える案は塞がらない——後の 1 本が path で先の新しい札を動かすため・実測で確認した）。
- **回収は死んだ所有者の札だけである**（`Reclaim::DeadOnly`）。追記の lock は握る時間が短いので古さで剥がしてよいが、**driver の札は数分〜数十分握られる**ので、同じ扱いにすると走っている driver の札を奪う。生きている所有者は待つ側に倒し、**継ぎの子は親が抜けるまで待って取れる**（自分の札を継ぎの対象にする形と噛み合う）。
- **測れなかった周は起こすのも起こし直すのも動かさない**（`s2-07l.482`）。起こし直しは台帳を読まないが、列を 1 周として成立させられない周に片方だけ動かすと `dispatch=unmeasured` の行が「何もしなかった」を意味しなくなる（C10・fail-closed）。
- **人の手を待つ段（`Blocked` / `Questioned`）は起こし直しの候補から段で外す**。`pipe resume` はこの 2 段で何もしないので起こし直すと空撃ちになる。起こし直しても何も進まない周を数えないためである（札は driver が死んだ周にだけ残るので、承認や回答を待つ便の札は既に外れている＝そもそも候補にならないが、`pipe run` が死んだ後に承認された便では段で外す側が効く）。schema を広げた便の Landed で古い binary の driver が typed に死ぬ周（NFR4・.160 の座礁 2026-09-15）も次の契機（他便の終端か手動の 1 周）に現在の binary で続く＝写し binary の refresh は要らない（走行中の process の code は変わらないので refresh は座礁を防がない）。`base_of_run` の読めなさは typed に呼び手へ返す（C10・「base が無い」と分ける）。

## 6. 観測

- `<NAME> pipe dispatch ls --state-dir S`: 列の各便を `[DISPATCH] bead=<id> prio=<p> mark=<first|hold|->` + `reason=<WaitReason>` で 1 行ずつ・`[DISPATCH-COUNT] total=<n> ready=<k>`・0 件は `[DISPATCH-NONE]`・台帳が読めない周は `[DISPATCH-UNMEASURED reason=…]`（0 件と融合しない・C10）。
- 観測の面は上の 1 口だけである。退避物の復元（rebrief）の DATA に同じ行を載せる案は、その DATA ごと超過した（ADR-0045 §2 (2)・`s2-07l.479.2`・[working-memory.md](./working-memory.md)）。

## 7. 極性

dispatcher は「起こす」側で行為を止める判定を持たない（起こせない便は理由付きで待つだけ）＝[ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1 の guard ではなく極性一覧に載せない（受付札と同じ扱い・[gate-cost.md](./gate-cost.md) §3.2）。台帳が読めない周は `unmeasured` で 1 本も起こさない（fail-closed 側に倒す・NFR4）。

## 8. 歯（`crates/<NAME>/tests/e2e/pipe/dispatch.rs`・`pipe_dispatch_` 接頭辞・名前の列は現物が SSOT）

- 順序: first → priority → 起票順の 1 関数（pure・in-file）。
- 起動条件: 偽の台帳（ready の出力 fixture）+ 偽の live 便（event log）で、交差する便は `Overlap` で待ち、交差しない便だけが起こせる側に立つ（`dispatch ls` は**見るだけで起こさない**ので、(a) の歯は `ready=` の数で測る）。
- 台帳が読めない周: `[DISPATCH-UNMEASURED]` で起動 0（0 件と区別）。
- 介入: `first` が priority より先に来る・`hold` は起こさない・`release` で戻る（event log の往復）。
- driver の死亡（行 (d)・`pipe_dispatch_driver_` 接頭辞）: 札は lock（握られている札は取れず・死んだ所有者の札は回収し・生きている所有者の札は stale を超えても奪わない・in-file の歯で決定的に測る——同じ便に 2 本同時に撃つ e2e の歯は全体の歯の負荷下で不安定〔単独 10/10・全体 1/3〕なので置かない。死んだ所有者の回収の競合は [fleet-event-log.md](./fleet-event-log.md) §4「回収は 1 手」・行 c・`s2-07l.486` の面）・殺した driver の便が 1 周で起こし直されて `Landed` まで通る（行 (e) の後の形・`resumed:1`・`SeatStopped detail=runner-dead` 1 件。行 (e) の前は 1 段進むだけを測っていた）・札の無い live 便は触らない・`Blocked` の便は段で外れる・正常に抜けた driver は自分の札を外す。
- 契機（行 (b)・`pipe_terminal_dispatch_` 接頭辞）: 受付の入口は同時に 1 つしか通さない（in-file・握っている間は取れず外せば取れる）・契機を同時に 2 回撃っても便は 1 本（起動試行 2 回）・land と stop の終端の直後に 1 周撃たれ、交差が解けた便が**起こされる**（`RunCreated` が増える・toy repo）・着地した bead は起こし直されず行を改訂して sha が動くと列に戻る・`pipe run` の終端の 1 周は自分の道具で起こす・列に渡した台帳 client が起こした子にも渡る（偽の台帳が列と子で 2 回呼ばれる）・`pipe dispatch` の手動 1 周と `first` / `release` の記録の直後が同じ関数を撃つ・実装役の口が無い周は `unmeasured`・終端の中の 1 周が失敗しても終端の rc は変わらない・候補 N 件の 1 周で repo の材料の読みが 1 回（読みの回数を数える fixture・母集団 = 候補数）。
- 便の自走（行 (e)・`pipe_dispatch_drive_` 接頭辞）: `--drive` を持つ `pipe run` は `run_all` の 1 process の連鎖（intake → 審査 → spawn → gate → land）で `Landed` に着き record は `drive=settled`（`resumed:0`）・`--drive` を持つ `pipe resume` は段ごとの終端の 1 周が子を 1 本ずつ継いで人の手なしに `Landed` まで通る（`Intake → Reviewed → Spawned → Implemented → Gated → Landed` の実測・母集団 = 段の遷移の数・渡さない理由は閉じた値〔待ち / 終端 / 前進なし / 測れない〕で record に載る＝測れなかった周を終端に読み替えない・C10）・`--drive` の無い run / resume は 1 段で止まる（既存の歯は 1 本も変えない＝母集団 205 本の diff 0）・`Blocked` の便と段の動かなかった周は渡さず理由が record に載る・列の起こし直しが起こす resume は `--drive` を持ち、殺した driver の便が `Landed` まで通る・段の前進の pure fn（in-file・前進 / 同じ段 / 後退の 3 形）・usage の外形 snapshot が更新される。
- 診断と結合（行 (g)・`s2-07l.487`・歯だけの便）: dispatch の歯の assert の文は落ちた周の rc・stdout・stderr を写す（`s2-07l.486` の merge 後の main の CI で「印の直後の 1 周」の行が無い落ち方をし、rc も stderr も写っておらず原因が測れなかった・同じ sha の再走は緑・A/B は `.486` の前後とも 0/20）。印の直後の 1 周は、手動の 1 周で起こした子 process の状態に依存しない台帳（起こせる候補が 0 の列・`RunCreated` 0・子 process 0）で測り、起こした効果は別の歯が測る（子が走っている最中に印を打つ形は、子の記帳との lock の競合で印の記帳が `fleet.lock_retry_ms` を超えうる＝歯が器の待ち時間に依存する）。
- 終端の便の列外: 直前の便が終端で終わった契約は同じ sha では `Settled` で列外、契約 file の sha が変わると列に戻る（偽 lens の verdict と run dir の fixture・着地した便を起こし直さない側も同じ 1 本で測る）。

## 9. 契約（5 便・(a) → (b) → (d) → (g) → (e)。(r) と (c) は超過）

- **(a)** `pipe dispatch` の本体: 列の導出（台帳の list + acceptance の設計 pointer + 審査 FAIL の列外）・順序の 1 関数・起動条件（intake の判定関数の再利用・可視性の変更）・`first / hold / release` の印（event kind 1 つ `DispatchMark` + `Event` の typed な field）・`dispatch ls`。依存: s2-07l.209 Landed。
- **(r)** 審査の時点（契約が出来た直後の審査・`ContractReviewed`）は**超過した**（ADR-0045 §2 の後の形・§2「審査の時点」・`s2-07l.368` は close）。審査は `pipe run` の Reviewed の段のまま。
- **(b)** 契機: 便の終端の直後 + `pipe dispatch` の手動 1 周 + 印の直後 + 1 周の材料の読みは 1 回（§5）。依存: (a)。
- **(c)** 観測（rebrief の DATA に `[DISPATCH]` の行）は**超過した**（ADR-0045 §2 (2)・`s2-07l.479.2`）。`dispatch ls` の 1 口が観測の面である。
- **(d)** driver の死亡（§5・s2-07l.352）: 札の書き・消し（`pipe run` / `resume` の入口と終端）・turn 関数の起こし直し・record token・`base_of_run` の typed 化。依存: (a)・(b)。
- **(g)** 歯の診断と結合の切り離し（§8・s2-07l.487・歯だけ）: assert の文に rc / stdout / stderr・印の直後の 1 周は子 process を起こさない台帳で測る。依存: (b)。
- **(e)** 便の自走（§5・s2-07l.485）: `pipe run` / `pipe resume` の flag `--drive`・turn の Input に呼び手の便・渡す周の判定（前進 ∧ 待ちでない ∧ 終端でない）と理由の record token・列と起こし直しが起こす便への `--drive` の写し。依存: (d)・(g)・`s2-07l.486`（Landed）。

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
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/fleet/cli.rs", "+crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/prop.rs", ".config/nextest.toml", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_"]
size = "M"
done = "偽の台帳と偽の live 便で、交差する便は Overlap で待ち交差しない便だけが起動の構築点に届き、first が priority より先に来て hold は起こさず、直前の便が Reviewed FAIL の契約は同じ sha では ReviewFailed で列外、台帳が読めない周は UNMEASURED で 0 本"

[[contract]]
id = "b"
title = "契機 — 便の終端（land / stop / retire・run と resume の終端）の直後・pipe dispatch の手動 1 周・first / release の記録の直後に同じ dispatch::turn を撃つ（tick は無い）・1 周の repo の材料の読みは 1 回（候補ごとに読み直さない）"
req = ["FR30", "FR68"]
section = "5"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/retire.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", ".config/nextest.toml", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_dispatch_"]
size = "S"
done = "偽 remote の toy repo で land と stop の終端の直後に列が 1 周撃たれて交差の解けた便が起こされ（RunCreated が増える）、契機が重なっても受付の入口の排他で便は 1 本に留まり、着地した bead は起こし直されず契約の行を改訂して sha が動くと列に戻り、run の終端の 1 周は自分の道具で便を起こし、pipe dispatch の手動 1 周と first / release の記録の直後も同じ関数を撃ち、dispatch ls は 1 本も起こさず、終端の中の 1 周が失敗しても終端の rc は変わらず、候補 N 件の 1 周で repo の材料の読みが 1 回（母集団 = 候補数）"
depends = ["a"]

[[contract]]
id = "d"
title = "driver の死亡 — 札の書き・消し、turn 関数の起こし直し（pipe resume）、record token resumed:<m>、base_of_run の typed 化"
req = ["FR68", "FR14", "FR50"]
section = "5"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/src/fleet/store.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_driver_"]
size = "M"
done = "driver を殺した便に dispatch の 1 周を撃つと pipe resume が 1 回起きて record に resumed:1・その便が 1 段進み、札の無い live 便と Blocked の便は起こし直さず、生きている driver が握っている札は取れず・古さでも奪われず、正常に抜けた driver は自分の札を外す"
depends = ["a", "b"]

[[contract]]
id = "g"
title = "dispatch の歯の診断と結合の切り離し — assert の文に rc / stdout / stderr を写し、印の直後の 1 周は子 process を起こさない台帳で測る（歯だけ・src/ は触らない）"
req = ["FR68", "NFR4"]
section = "8"
write-set = ["crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_dispatch_marks_fire_without_children"]
size = "S"
done = "印の直後の 1 周の歯が便を 1 本も起こさずに（RunCreated 0・子 process 0）release の直後の dispatch= 行を測り、既存の手動の 1 周の歯は起こした効果だけを測り、dispatch の歯の assert の文が落ちた周の rc と stdout と stderr を写し、src/ は 1 行も変わらない"
depends = ["b"]

[[contract]]
id = "e"
title = "便の自走 — pipe run / pipe resume の flag --drive を持つ driver だけが終端の 1 周で自分の便を次の driver に渡し（前進 ∧ 待ちでない ∧ 終端でない周だけ・渡さない理由は record token）、列と起こし直しが起こす便は --drive を持つ"
req = ["FR68", "FR14", "FR50"]
section = "5"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/size.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_drive_"]
size = "M"
done = "--drive を持つ pipe run が toy repo の契約 1 本を偽 runner と偽 lens で人の手なしに Landed まで通し（run_all の 1 process・record は drive=settled）、--drive を持つ pipe resume が段ごとに次の driver へ継いで Landed まで通り、--drive の無い run / resume は 1 段で止まり既存の歯は 1 本も変わらず、Blocked の便と段の動かなかった周は渡さず理由が record に載り、列の起こし直しが起こす resume は --drive を持ち殺した driver の便が Landed まで通り、usage の外形 snapshot が更新される"
depends = ["d", "g"]
<!-- contracts:end -->
