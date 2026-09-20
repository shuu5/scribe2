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
- **終端の便を列外にするのは無限再起動を塞ぐためである**（`s2-07l.366`）: 便が終端に着くと live でなくなって交差が消えるが、bead は台帳で `open` のまま（器は台帳に書かない・C15）なので、終端が来るたびに同じ契約が起こし直される（着地から close までの間ずっと）。契約が改訂されて sha が動けば列に戻る＝「直すまで起こさない」で `Landed`（済んでいる）・`Failed`（契約を直すまで再試行しない）・`Stopped`（人が止めた）のどれも筋が通る。契約の字が正しいのに器の側の理由で終端に着いた便（実装役の起動の失敗など）を、字を変えずに列へ戻す口は §12（`release` の印）。台帳の読みは席の指示文の `{ledger}` と同じ子 process と同じ関数（`read_ledger`・`bd --readonly list --limit 0 --json`・待ち上限は rules 行 `seat.ledger_timeout_s`・置き場は `seat/ledger.rs`）を共用し、読めない周は列を空と読まず `unmeasured` で止まる（NFR4・C10）。
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

`<NAME> pipe dispatch first <bead>` / `hold <bead>` / `release <bead>`。印は state dir の列の記録（event log の 1 kind `DispatchMark { bead, mark: First | Hold | Release }`＝`run` を持たない行・`mark` は `Event` の typed な field で、自由文の `detail` を判定入力にしない〔C3.3〕）で、`dispatch ls` に出る（退避物の復元の DATA に出す案は §6 のとおり超過した）。**打ち手の名（`by`）は持たない**（実装 `s2-07l.345`）: 口は権能なし（§5・ADR-0045 §2 (1)）で誰が撃っても同じ印になり、器は env も HOME も読まない（C2.2）ので、正直に書ける出所が無い。user の直命「最優先」の対は `first`（planner が打つ・逐語は台帳に残る）。印は台帳の priority を書き換えない（台帳は task と裁定・憲法 C15）。`release` は `hold` を外すだけでなく、終端に着いた便の列外（§2 の `Settled`）も 1 回だけ外す（§12）。

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
- 列へ戻す印（行 (h)・`pipe_dispatch_release_requeues_` 接頭辞）: `Failed` で終端した便の bead は `Settled` で列外だが、その後の `release` で同じ sha のまま列に戻り（`dispatch ls` の reason が `-`）、起こし直した便が同じ sha でまた終端に着くと再び `Settled`（印は 1 回しか効かない）・終端より**前**の `release` は効かない・`Landed` の便と審査 FAIL の便は `release` の後も `Settled` のまま・`Stopped` の便と gate の判定で終端になった便は戻る（母集団 = 終端の段の種類）。
- 関門が開いた待ちの便（行 (j)・`pipe_dispatch_waiting_gate_` 接頭辞）: 回答済みの `Questioned` の便（driver の札なし）が手動の 1 周で `--drive` 付きの resume で起こされて先の段へ進み（`resumed:1`）、未回答の `Questioned` の便と、古い質問に回答が在っても最新の質問が未回答の便は起こされず（`resumed:0`）、承認済みの `Blocked` の便も同じく起こされ、札の 4 値（無い・所有者が死んでいる → 起こす／所有者が生きている・在るのに読めない → 触らない）がそれぞれ測られ、道具を渡した `pipe answer` と `pipe approve` の記帳の直後に同じ 1 周が撃たれて便が進み、道具を渡さない `pipe answer` は記帳だけで rc 0 のまま、回答・承認の stdout は記帳の 1 行だけで、1 周が失敗しても回答の rc は変わらず、候補の選別（pure・in-file）は段の前進の 3 値のそれぞれを測り（**段を前へ進めた driver の周は関門の候補をそのまま起こす**／同じ段のまま・段が戻った driver の周は関門の候補を 0 本にする／driver でない周は絞らない）、段を前へ進めた driver の終端の 1 周が別の回答済みの便を起こす（e2e・`resumed:1`）、関門の判定は resume の入口と列が同じ述語 1 本を呼び、待ちの段でない便の起こし直しの規則と未承認の `Blocked` を外す既存の歯（`pipe_dispatch_driver_` の歯）と、質問と回答の歯（`pipe_question_`）と承認の歯（`pipe_approval_`）は測っている約束を変えずに緑のまま（行の verify がこの 3 つの接頭辞も撃つ）。
- 起動の失敗の理由と repo の名指し（行 (i)・`pipe_spawn_runner_stderr_` と `pipe_repo_relative_` 接頭辞・置き場は行 (i) の write-set の `+` の file）: stderr に 1 行書いて rc 2 で落ちる偽 runner の便が `Failed` に着いた後、run dir の stderr の log にその 1 行が見出し付きで残り、呼び手の stderr にも同じ行が出る・stderr が空の周は file を作らない・stderr に書いても rc 0 ∧ commit 1 の偽 runner は `Implemented` に着く（段は stderr の中身で変わらない）・`--repo` を相対 path で渡した `pipe run` が絶対 path で渡した周と同じ worktree の場所と同じ段に着く。

## 9. 契約（8 便・(a) → (b) → (d) → (g) → (e)、その後に (h) → (j) と (i)。(r) と (c) は超過）

- **(a)** `pipe dispatch` の本体: 列の導出（台帳の list + acceptance の設計 pointer + 審査 FAIL の列外）・順序の 1 関数・起動条件（intake の判定関数の再利用・可視性の変更）・`first / hold / release` の印（event kind 1 つ `DispatchMark` + `Event` の typed な field）・`dispatch ls`。依存: s2-07l.209 Landed。
- **(r)** 審査の時点（契約が出来た直後の審査・`ContractReviewed`）は**超過した**（ADR-0045 §2 の後の形・§2「審査の時点」・`s2-07l.368` は close）。審査は `pipe run` の Reviewed の段のまま。
- **(b)** 契機: 便の終端の直後 + `pipe dispatch` の手動 1 周 + 印の直後 + 1 周の材料の読みは 1 回（§5）。依存: (a)。
- **(c)** 観測（rebrief の DATA に `[DISPATCH]` の行）は**超過した**（ADR-0045 §2 (2)・`s2-07l.479.2`）。`dispatch ls` の 1 口が観測の面である。
- **(d)** driver の死亡（§5・s2-07l.352）: 札の書き・消し（`pipe run` / `resume` の入口と終端）・turn 関数の起こし直し・record token・`base_of_run` の typed 化。依存: (a)・(b)。
- **(g)** 歯の診断と結合の切り離し（§8・s2-07l.487・歯だけ）: assert の文に rc / stdout / stderr・印の直後の 1 周は子 process を起こさない台帳で測る。依存: (b)。
- **(e)** 便の自走（§5・s2-07l.485）: `pipe run` / `pipe resume` の flag `--drive`・turn の Input に呼び手の便・渡す周の判定（前進 ∧ 待ちでない ∧ 終端でない）と理由の record token・列と起こし直しが起こす便への `--drive` の写し。依存: (d)・(g)・`s2-07l.486`（Landed）。
- **(h)** 列へ戻す印（§12・s2-07l.495）: `Settled` の判定が、その便の最後の記帳より後の `release` を見て 1 回だけ列外を外す（`Landed` と審査 FAIL は外さない）。依存: (b)。
- **(i)** 起動の失敗の理由と repo の名指し（§12・s2-07l.495）: 実装役の stderr を run dir に残し、`--repo` の値を口で絶対 path に直す。依存: なし。
- **(j)** 関門が開いた待ちの便の再開（§13・s2-07l.495）: 起こし直しの候補に「回答済みの `Questioned` / 承認済みの `Blocked` で、driver が居ないと測れた便（札の 4 値）」を足し、段を進めなかった driver の周は関門の候補を起こさず、`pipe answer` / `pipe approve` の記帳の直後にも黙って 1 周を撃つ。(h) と列の module を共に触る＝直列（(h) が先）。

## 10. 却下案（ADR-0034 §5 の写しは持たない・設計固有のもの）

- 列を台帳の label（`dispatch:ready` 等）で持つ: 台帳に規律と状態を置く（C15）・label の継承で親から漏れる（PRIME R2）。列は器の記録。
- 介入を台帳の priority の書き換えで表す: priority は契約の性質、介入は一時の順序。混ぜると「なぜこの順か」が記録から消える。
- 起動条件を dispatcher が独自に再実装する: 交差と受付の判定が 2 か所になる（C2 違反・.303 の QUESTION の型）。既存の intake の関数を呼ぶ。
- 審査を起動の瞬間（`pipe run` の中・.241 の位置のまま）に撃ち、列の入力に審査を持たない: 契約の不備が起動が回ってきた時まで見えず、planner の待ち時間が捨てられ、FAIL の便が run N+1 まで列を塞ぐ（user 裁定 2026-09-15 13:4xZ で却下・planner の初案）。

## 11. 後続

- SRS FR30 の response と FR49 の condition の改訂・AC38 / AC39 の追加は SRS v0.14 で反映済み（FR68 の起動の形・lock・glossary の 受付 / priority / Reviewed は v0.15）。
- QUESTION と Gated FAIL の裁定（planner の手番）を速くする形は別設計（契約の改訂を器の口で持つ .133 の系）。
- ADR-0045 §2 (6) の SRS 改稿（FR30 の担い手・FR68 の契機を tick から便の終端と手動の 1 周へ・FR49 の condition は「便が intake を通ったとき」のまま）は user の /folio-architect の周。
- 居座る便を席から外す口: `pipe stop` は起動の権能で、ADR-0045 §2 (1) の後はどの席の行にも無い。「止める」だけを席の権能に足すかは rules 行の変更＝user の裁定が先（`s2-07l.495` の notes）。→ 裁定は出た: [ADR-0048](../../design-intent/decisions/ADR-0048-stopping-a-run-is-a-separate-capability-of-the-orchestrator.html)（proposed）が「止める」だけを権能 `stop` に分けて `role.orchestrator` の行に足し、席が撃てるのは便 1 本を名指す形だけ（全部を止める形は launch のまま）と決めた。発効は権能と行の値と guard の照合が land した版。
- 着地の終端の bead の close が台帳の子 process の cwd を名指さない件（§14 の口 (2)）は本設計の行に入れられない: 呼び手の file（着地の口）の上限の余地が base で 84 行しか無く、いちばん小さい見積でも受付が `cap-headroom` で断る（§14 の実測 2026-09-20）。その file を割る便の後に別の行で直す。
- 終端の便の worktree（退役先に寄せたものと `Failed` で残ったもの）の掃除は別設計（消す操作＝憲法 A1 の裁定が先）。[FR68](../../design-intent/spec/srs.html#FR68) の「release で戻し」の対象に終端の便を含める字の改訂は user の /folio-architect の周（§12 は hold と同じ印の同じ向きの拡張で、要件の向きは変えない）。

## 12. 器の側の理由で終端に着いた便（起動の失敗・`s2-07l.495`）

やさしく言うと: 契約は正しいのに、実装役が立ち上がれずに便が落ちることがある。今はその理由がどこにも残らず、契約の字を書き換えない限り二度と起きない。理由を残し、席が「もう一度」の印を 1 つ打てば同じ契約のまま起き直るようにする。

実測（consumer の最初の便・2026-09-19）: 審査 PASS の直後に実装役が rc 2・commit 0 で落ちて `Failed` に着いた。(1) 列が起こした driver は端末を持たないので、継承した stderr に出た理由の 1 行は読める場所に残らない（C10）。(2) §2 の列外は段を区別しないので、この便は契約の字を変えるまで列に戻らない——字は正しいので変える理由が無い。席は起動の権能を持たない（ADR-0045 §2 (1)）ので、正規に起こし直す口が無い。(3) `--repo` を相対 path で渡すと worktree の場所も相対になり、cwd を worktree に移した子から見て解けない。

- **列へ戻す印は `release`**（行 (h)）: `Settled` の判定は、同じ契約 file を持つ直前の便を見つけた後、**その便の最後の記帳より後に同じ bead への `release` が在る**周は列外にしない。判定の材料は既存の event log の並びだけである（新しい event kind も field も足さない・C17.1）。起こし直した便は新しい run id を持ち、その記帳は `release` より後に並ぶので、同じ sha でまた終端に着けば再び列外になる＝**印 1 回で起き直るのは 1 回**（§2 の無限再起動を開け直さない）。
- **戻さない段が 2 つ在る**: `Landed`（済んでいる。起こし直すと同じ変更をもう一度作る）と審査 FAIL（`Reviewed` で終端・[FR49](../../design-intent/spec/srs.html#FR49) が「中身が変わるまで列に入らない」と定める）。この 2 つは `release` の後も `Settled` のままで、`dispatch ls` の理由も変わらない。`Failed` / `Stopped` / gate の判定で終端になった段は戻す——gate の FAIL には flaky な歯で落ちた周が含まれ、契約の字を変えずに測り直す口が他に無い。戻すかどうかの段の弁別は段の型の**網羅の match 1 本**で持つ（新しい段が増えた便は compile が止めて、その段を戻すかを決めさせる）。この match で列の module は段の型の閉包に入る＝段の型を `touches` に持つ既存の行（[contract-source.md](./contract-source.md) の行 c）の write-set に列の module を足した（1 周目の実測 2026-09-20: 足さないまま実装した便は「現物の契約表は違反 0」の歯で gate が赤になった）。
- **器が自分で戻すことはしない**: 「commit 0 ∧ 起動の失敗」を器が見分けて自動で起こし直す形は、見分けを誤った周に §2 が塞いだ無限再起動へ戻る。戻すかは理由（下の stderr の log）を読んだ席が決める。口は権能なしの既存の `release`（§4）で、印の直後の 1 周（§5）がそのまま起こす。
- **実装役の stderr を run dir に残す**（行 (i)）: spawn は stdout と同じ形で stderr も捕らえ、run dir に stdout の log と並べて 1 本置く（名前は stdout の log の `stdout` を `stderr` に替えたもの・見出し行 `## <ts> rc=<rc>` 付きの append・空の周は書かない・機械は読まない診断 file＝[pipeline.md](./pipeline.md) §5.2 の 7 と同じ規律）。捕らえた stderr は呼び手の stderr にもそのまま流す（手で撃った周の見え方を変えない）。段の判定は今までどおり rc と commit の数だけで決め、stderr の中身を判定入力にしない（C3.3）。
- **`--repo` の値は口で絶対 path に直す**（行 (i)）: `--repo` を読む口（pipe の引数の読み手）は値を `std::path::absolute` で絶対にしてから使う（標準 library・symlink も存在も見ない＝席の打刻の絶対化と同じ関数）。読み手が複数在る現状は 1 本に畳む（C2）。相対を断る形にしないのは、絶対にできない入力が空文字しか無く、断りの variant を 1 つ足すより直す 1 行が小さいためである（C17.4）。
- **`Failed` の便の worktree の片付け**は本節では足さない: 畳む口（retire）は起動の権能の側に在り、席から撃てない。起こし直しは新しい worktree を切るので、残った worktree は次の便を塞がない。溜まった worktree の掃除は消す操作（憲法 A1）を含むので、別の設計で user の裁定を取る（§11）。

## 13. 関門が開いた待ちの便を列が再開する（回答・承認の後・`s2-07l.495`）

やさしく言うと: 実装役の質問に席が答えても、その便をもう一度動かす人が居ない。答えた（か承認した）直後に、器が自分でその便を再開する。

実測（2026-09-20・verified）: 質問で止まった便（`Questioned`）に席が `pipe answer` で回答した後、手動の 1 周を撃っても `resumed:0` のまま便は動かなかった。根は 2 つ重なっている。(1) 起こし直しの候補は待ちの段（`Blocked` / `Questioned`）を**段で**外す（§5）が、回答も承認も段を動かさない（resume が進める）ので、関門が開いた後も段は待ちのままで候補に戻らない。(2) 質問で止まった driver は正常に抜けるので札を外す＝起こし直しの条件「札が残っていて所有者が死んでいる」にも当たらない。`pipe resume` は起動の権能で、ADR-0045 §2 (1) の後はどの席の行にも無い＝回答済みの便は user が手で撃つまで止まる。

- **起こし直しの候補に「関門が開いた待ちの便」を足す**（行 (j)）: live ∧ 段が待ちの段 ∧ 関門が開いている ∧ **driver が居ないと測れた**便を、§5 の起こし直しと同じ構築点（`--drive` 付きの resume・道具は列と同じ 1 本）で起こす。関門が閉じたままの待ちの便は今までどおり候補にしない（空撃ちを作らない・既存の歯 `pipe_dispatch_driver_blocked_run_is_excluded_by_stage_and_keeps_its_ticket` が測る側＝変えない）。
- **§5 の「札が無い便は触らない」をこの候補にだけ緩める**（消すものの名指し・C17.2）: §5 の起こし直しは「札が残っていて所有者が死んでいる」便だけを候補にし、札の無い便は触らない。待ちの段で止まった driver は正常に抜けて札を外すので、関門が開いた待ちの便は**札が無い**（file が無いと読めた）周も候補にする。札の状態は 4 値で読む: 無い → 候補／所有者が死んでいる → 候補／所有者が生きている → 触らない／**在るのに読めない → 触らない**（測れないを「居ない」に読み替えない・fail-closed は保つ）。待ちの段でない便の §5 の規則は 1 字も変えない。
- **関門の判定は resume の入口と同じ述語 1 本**（C2）: `Questioned` は**最新の**質問に回答が在ること（古い質問への回答が在っても、その後の新しい質問が未回答なら閉じている）、`Blocked` は replay の承認の導出値。いま resume の入口に式で書かれている 2 つの判定を pipe の module の述語 1 本に畳み、resume と列が同じ 1 本を呼ぶ（site を 2 つにしない）。
- **段を進めなかった driver の終端の 1 周は、関門の候補を 1 本も起こさない**（空撃ちの連鎖を塞ぐ）: §5 の起こし直しが暴走しないのは、resume が抜けると札が消えて候補から落ちるからである。札の無い便を候補にするとこの止め金が効かない——段を 1 つも進められずに抜けた resume（受付・worktree・口座で落ちる周）が、自分の終端の 1 周で同じ便をまた起こし、待ちの便が 2 本在れば互いを起こし合う。そこで、**driver が撃つ終端の 1 周**（呼び手の便を持つ周・行 (e)）は、その driver が**段を前へ進めた周だけ**関門の候補を起こす（段の前進は行 (e) の pure fn の 3 値をそのまま読む・前進以外は 0 本）。連鎖は段の前進を 1 回ずつ要るので有限で、進められない便は driver でない契機（手動の 1 周・印の直後・回答や承認の記帳の直後）でだけ再び候補になる。§5 の「札の所有者が死んだ便」の起こし直しはこの絞りの外である（今までどおり）。
- **二重の再開は既存の札の排他が断る**: resume は入口で driver の札を握り、握れない周は駆動しない（§5）。契機が重なって同じ便へ resume が 2 本撃たれても、駆動するのは 1 本である（既存の挙動・本行は歯を足さない）。
- **契機に回答と承認の記帳の直後を足す**（行 (j)）: `pipe answer` / `pipe approve` の**記帳が成った周だけ**、その直後に同じ 1 周を撃つ（渡された引数の道具だけを使う・便が live で無くなりうる subcommand の列には足さない）。**stdout には 1 行も足さない**（終端の 1 周と同じ黙る形＝回答・承認の stdout は今の記帳の 1 行だけ・置き場や repo を渡さない既存の呼び方の出力は 1 字も変わらない）。道具（`--runner`）を渡さない回答・承認は今までどおり記帳だけで終わり、その便は次の契機で再開される。1 周が失敗しても回答・承認の rc は変えない（§5 の終端の 1 周と同じ）。
- **write-set の外の歯の走査**（未実測・実装の周に測る）: 回答や承認を撃つ既存の歯は e2e の他の file にも在る（字面の在る file = pipe の spawn / gate / ratelimit / land の歯と、fleet と極性の歯）。それらは道具を渡さずに回答・承認を撃ち、続けて手で resume を撃つ形なので新しい契機は発火しない見込みだが、「関門が開いた待ちの便が置き場に残ったまま、別の便の道具付きの終端が走る」歯が在れば段が動いて落ちる。行 (j) の write-set は回答・承認の字面の在る歯の file を含める（落ちた歯だけを、測っている約束を変えずに直す）。
- 触らない: 回答・承認の記帳の形・段の遷移・待ちの段の集合・札の形・待ちの段でない便の起こし直しの規則。`pipe stop` を席から撃てない件（居座る便を外す口）は権能の行の変更で、user の裁定が先（§11）。

## 14. 台帳の子 process を名指された repo の中で撃つ（契約表の行 k・`s2-07l.495`）

やさしく言うと: 「どの repo の契約を起こすか」は `--repo` で名指すのに、台帳（bead の一覧）だけは器が居る場所から読んでいる。別の repo から撃つと、列に他所の bead が並ぶ。

- 何が起きているか（現物 = 本行の base・verified）: `crates/scribe2/src/seat/ledger.rs` の `read_text` は `Command::new(bd)` に引数と 3 つの標準の口だけを付けて `spawn` し、**cwd を名指さない**＝子は親 process の cwd を継ぐ。台帳 client は cwd から台帳を解くので、読む台帳は cwd 側になる。列（`crates/scribe2/src/pipe/dispatch.rs` の `turn`）はこの 1 本を `ledger::read_ledger(input.bd, timeout)` で撃つが、`Input` は `repo` を持っている（契約の生成・sha の読み・起こす便の `--repo` には渡っている）のに台帳の読みには渡していない。よって `pipe dispatch --repo X` を別の repo の cwd から撃つと、**設計 doc と契約表は X・台帳は cwd 側**という食い違った 1 周になる（`dispatch ls` も同じ 1 本を撃つので同じ列が見える）。
- 同じ根の口は器に 3 本在る（実測・母集団 = 台帳 client を子 process で撃つ site 2 本とその呼び手 3 本）: (1) 列の読み（上）。(2) `crates/scribe2/src/ledger/mod.rs` の `close` も `Command::new(bd)` に引数だけを付けて撃ち cwd を名指さない——呼び手は `crates/scribe2/src/pipe/land.rs` の着地の終端で、そこは着地した便の repo を持っている。(3) `crates/scribe2/src/hook/mod.rs` の SessionStart は `read_text` を撃つが、**その周の cwd は session の repo** なので出所は今も正しい。読み手が cwd を引数で取ると (3) も渡す側になる＝同じ便で直す（構造の連鎖）。
- **本行が直すのは読み手の 2 本（(1) と (3)）だけである**: (2) を同じ便に入れると呼び手の file（着地の口）を write-set に要るが、その file の上限の余地は base で 84 行しか無く、いちばん小さい見積（`size = "S"`）にも足りない＝受付が `cap-headroom` で断る（本行の事前審査の実測 2026-09-20）。`close` の cwd は着地の口の file を割る便の後に別の行で直す（§11 の後続に足した）。読み手の 2 本を先に直しても `close` の向きは変わらない（列の 1 周と着地の終端は別 process の別の口で、片方だけ直しても食い違いは増えない）。
- 形: 台帳の**読み**の子 process の cwd を**呼び手が名指す**。`read_text` / `read_ledger`（`seat/ledger.rs`）は cwd を引数で取り、標準 library の子 process の起動に作業 dir として渡す（新しい依存は増えない・C17.3）。器は cwd を推さない（process の cwd を判定の入力にしない・C2.2 の seam と同じ向き）。列は `Input` の `repo` を、SessionStart は payload の cwd（無ければ process の cwd・`hook/mod.rs` の既存の 1 本）を渡す。
- 渡した値が cwd に出来ない周（dir でない・無い・読めない）は、子の `spawn` が落ちて**既存の断りがそのまま受ける**: 列は `LedgerError::Unreadable` → `dispatch=unmeasured reason=ledger` で 1 本も起こさない（C10・0 件と融合しない）。SessionStart は今までどおり件数を書けない周の字面になる。新しい断りの variant も rules 行も足さない（C17.1）。
- 触らない: 台帳 client の引数（`BD_ARGS`）・待ち上限の rules 行 `seat.ledger_timeout_s`・`LedgerError` の 2 値・件数の 1 行（`counts_of`）の字面・SessionStart の指示文の本文・`--bd` の既定（`DEFAULT_BD`）・列の入力の条件（§2）・起こす便へ渡す道具（§5）・`close` と着地の終端（上）。
- 却下案: 列が台帳を読む前に process の cwd を `--repo` へ移す（process 全体の cwd を動かすと同じ周の他の相対 path の読みが静かにずれ、並行する子とも噛み合わない）／台帳 client に台帳の場所を渡す flag を足す（client の引数は client の契約で、器が決める線ではない・C5）／`--repo` と cwd が違う周を断る（別 repo から撃てる性質を失う＝列は「この置き場の便」と「この repo の契約」を突き合わせる口である・§5）。
- 歯（`pipe_dispatch_ledger_cwd_` 接頭辞・置き場は列の歯の file）: (a) cwd を書き出してから台帳の JSON を吐く偽の台帳 client を `--bd` で渡し、process の cwd を別の dir にしたまま `pipe dispatch ls --repo <toy>` を撃つと、子の見た cwd が toy repo である（process の cwd でない）／(b) 同じ偽の台帳 client で、`--repo` を相対 path で渡した周も子の見た cwd が同じ絶対 path になる（口が値を絶対にする §12 の形と噛み合う pin）／(c) 無い dir を `--repo` に渡した周は列が `[DISPATCH-UNMEASURED` の行で 0 本（`[DISPATCH-NONE]` と融合しない）／(d) SessionStart の `{ledger}` の行は 1 字も変わらない（既存の歯が測る側・行の verify がその歯を撃つ）。

## 15. 席が測り直して PASS になった Gated の便を列が起こし直す（契約表の行 l・`s2-07l.495`）

やさしく言うと: 関門で「判定できず」に終わった便を席が測り直して「通った」にしても、その便を着地まで運ぶ人が居ない。通っていると分かった便は、器が自分で次の段へ進める。

- 何が起きているか（実測 2026-09-20・verified）: 審査役の出力の形式不備で INCONCLUSIVE になった便を席が `pipe gate` で測り直して verdict を PASS にした後、手動の 1 周（`pipe dispatch`）を撃っても `resumed:0` のまま便は動かなかった。
- 現物（本行の base・verified）: 起こし直しの候補は `crates/scribe2/src/pipe/dispatch.rs` の `revivals` の 1 本が決める。live 便を待ちの段（`WAITING` = `Blocked` / `Questioned`）とそれ以外に分け、それ以外は `super::driver_is_dead`（札の所有者が死んでいる便だけ）で絞る。`Gated` は `WAITING` に無いので後者に落ちるが、INCONCLUSIVE で正常に抜けた driver は `Drop` で自分の札を外す＝札は `Ticket::Absent` で `driver_is_dead` は偽になり、候補にならない。段の生死（`crates/scribe2/src/pipe/cli/state.rs` の `live`）は `Stage::Gated` を「verdict が FAIL でない」で判じるので、**verdict が PASS の Gated 便は live** である（`revivals` の最初の絞りは通っている）。
- 形: `revivals` の絞りに **1 枝だけ**足す。`Stage::Gated` の live 便は、今までの「札の所有者が死んでいる」に加えて**「verdict が PASS ∧ 札が `Absent` か `Dead`」**でも候補にする。**足す側だけで既存の枝は 1 字も変えない**＝gate の途中で driver が死んだ便は verdict に依らず今までどおり候補である。verdict の読みは着地の段が持つ既存の 1 本をそのまま呼ぶ（site を 2 つにしない・C2）。実測: その読み手は `crates/scribe2/src/pipe/land.rs` の `verdict_of`（引数は置き場と run id・戻り値は 3 値の `Option`・読めない周は `None`）で、**可視性は既に crate 全体**である（`cli/state.rs` の `live` と `gated_is` が module をまたいで呼んでいるのと同じ口）＝列の file（`crates/scribe2/src/pipe/dispatch.rs`）から呼ぶのに `land.rs` も `cli/state.rs` も 1 字も変えない。だから write-set は列の file と列の歯の file の 2 つで閉じる。
- **要件との対応**: [FR68](../../design-intent/spec/srs.html#FR68) は、所有者の印（本 doc の札）を持たなくても再開を 1 回起こす便を 3 種に閉じている——回答済みの Questioned・承認済みの Blocked・**verdict が PASS の Gated**。本行が足す枝はその 3 種目そのもので、要件の外の例外を足さない。同じ要件の「verdict が PASS でない Gated の便と verdict を読めない Gated の便は起こさない」「この便のための契機は足さず既存の契機の周で拾う」も本行の約束と同じである（下の 2 項と、歯が手動の 1 周で測ること）。
- **PASS 以外は候補にしない**: verdict が INCONCLUSIVE の Gated 便を候補にすると `pipe resume` が再 gate へ倒す（`crates/scribe2/src/pipe/cli/resume.rs` の既存の分岐）＝器が勝手に 1 周ぶんの費用（[gate-cost.md](./gate-cost.md) §2）を払い直す。verdict を読めない周も候補にしない（測れないを「通った」に読み替えない・fail-closed・NFR4）。札が `Live` / `Unreadable` の便は触らない（§13 の 4 値の読みをそのまま使う）。
- **§13 の絞りをそのまま受ける**（空撃ちの連鎖を塞ぐ）: この候補の札は起こす前も後も `Absent` なので、§5 の止め金（resume が抜けると札が消えて候補から落ちる）が効かない。よって driver が撃つ終端の 1 周は、§13 の関門の候補と同じく**段を前へ進めた周だけ**この候補を起こす（段の前進の 3 値をそのまま読む・前進以外は 0 本）。driver でない契機（手動の 1 周・印の直後・回答や承認の記帳の直後）は今までどおり絞らない。
- 触らない: 待ちの段の集合（`WAITING`）と §13 の候補の規則・§5 の「待ちの段でない便は札の所有者が死んだものだけ」の規則（`Gated` 以外の段）・札の形と 4 値・段の生死の match・起こし直しの構築点（`--drive` 付きの `pipe resume`・道具は列と同じ 1 本）・`dispatch ls` の行の字面・`dispatch=` の record token の形。
- 却下案: 席に `resume` の権能を足す（権能の行の変更＝user の裁定が先・§11）／測り直しの口（`pipe gate`）が PASS を書いた周に自分で次の段を撃つ（関門の口が起動の口を兼ねる＝1 つの口が 2 つの権能を持つ・ADR-0045 §2 (1)）／`Gated` を `WAITING` に入れる（待ちの段は「人の手を待つ」意味で、承認と回答の関門の判定がそのまま当たらない）。
- 歯（`pipe_dispatch_gated_pass_` 接頭辞・置き場は列の歯の file）: (a) verdict PASS ∧ 札の無い `Gated` の便が手動の 1 周で `--drive` 付きの resume で起こされ（`resumed:1`）先の段へ進む／(b) verdict INCONCLUSIVE ∧ 札の無い `Gated` の便は起こされない（`resumed:0`）／(c) verdict を読めない `Gated` の便も起こされない（`resumed:0`）／(d) 札の所有者が生きている便と札が在るのに読めない便は触らない（母集団 = 札の 4 値）／(e) 札の所有者が死んでいる `Gated` の便は verdict に依らず起こされる（既存の規則を PASS と INCONCLUSIVE の両方で測る）／(f) 段を前へ進めなかった driver の終端の 1 周はこの候補を 1 本も起こさない／(g) 待ちの段の候補の規則と、待ちの段でない `Gated` 以外の便の規則は変わらない（既存の歯が測る側・行の verify がその 2 接頭辞も撃つ）。

## 16. 列外の鍵に審査役へ渡る材料を含める（契約表の行 m・`s2-07l.495`）

やさしく言うと: 契約の審査で「判定できず」に終わった便は、設計の節を書き直しても列に戻らない。審査役が読むのは節の本文なのに、器は契約 file の字しか見ていないからである。

- 何が起きているか（実測 2026-09-20・verified）: 審査が INCONCLUSIVE で終端した `Reviewed` の便は、設計 § を直しても列へ戻らなかった。回避は行の `done` の字を動かして契約 file の中身を変えることだった（`s2-07l.496`）。
- 現物（本行の base・verified）: 列外の鍵は `crates/scribe2/src/pipe/dispatch.rs` の `settled` が組む。直前の便の run dir の契約の写しを読み、**いま行から生成した契約 file の本文と同じか**だけを突き合わせる。一方、審査役が受け取る材料は 3 つである（`crates/scribe2/src/pipe/review.rs` の `keep`）: 審査の材料の dir に置かれる契約の写しと、行の `section` が指す § の本文（base から読む）と、`req` の要件文。**§ の本文は既に便ごとに run dir へ写っている**が、鍵には入っていない。
- `release` の印（§12）も効かない: 戻す段の match は `Stage::Reviewed` を戻さない側に置く（[FR49](../../design-intent/spec/srs.html#FR49)「中身が変わるまで列に入らない」）。**審査が読む中身が変わったのに鍵が動かない**、というのが穴の形である。
- 形（裁定: orchestrator 2026-09-20・**同じ材料 → 同じ判定**が鍵の意味）: `settled` の突き合わせに § の本文を足す。直前の便の材料の dir に在る § の写しの本文と、いま base から読んだ § の本文が違う周は列外にしない。§ の読みは**審査と同じ 1 本**（`review` が持つ § の読み手を pipe の中に開いて列が呼ぶ・site を 2 つにしない・C2）で、材料を書く側と同じ形に揃えてから突き合わせる（本文を作る読み手は 1 本・末尾の整え方は材料を書く 1 本に合わせる）。
- **§ を鍵に入れるのは `Reviewed` の段だけである**: § はその段で審査役が読んだ材料であって、`Landed`（済んでいる・起こし直すと同じ変更をもう一度作る）とも、`release` が戻す段（`Failed` / `Stopped` / `Gated`・§12）とも関係が無い。段の弁別は `release` の戻す段と同じ**段の型の網羅の match 1 本**で持つ（段が増えた便は compile が止めて、その段の鍵に § が要るかを決めさせる）。
- **材料の写しが無い周は契約 file だけの鍵に倒す**（今の挙動のまま）: 審査へ届かずに終端した便は § の写しを持たない。無い周を「違う」と読むと、審査へ届かないまま終端する便が終端のたびに起こし直され、§2 が塞いだ無限再起動が開く。**「無い」と「違う」を畳まない**（C10・fail-closed）。写しが在るのに読めない周も「無い」と同じ扱いで、今までどおり列外に留まる。
- 互換（置き場に既に在る記録）: § の写しは審査の段を通った便が必ず持つ（材料を書く 1 本が毎回書く）ので、**本行より前に終端した `Reviewed` の便も新しい鍵でそのまま読める**＝移行のための書き足しも、旧い記録の読み替えも要らない。新しい file も新しい field も足さないので、on-disk の形は 1 byte も変わらない（ADR は要らない）。
- FAIL も同じ鍵でよい（裁定）: § を直した契約は再審査に値する。FAIL と INCONCLUSIVE の弁別は鍵には要らない——どちらも「この材料では通らなかった」であって、材料が変われば測り直す側である。
- 触らない: `dispatch ls` の理由の字面（sha は契約 file の名札のまま・観測の面を増やさない）・event kind と field（足さない・C17.1）・`release` の印と戻す段の match・§2 の列の入力の条件・審査の材料の書き方と判定の記録の形・`req` の要件文（鍵に入れない——要件面の改訂は SRS の周であって、契約 1 本を起こし直す契機ではない）。
- 却下案: `release` の印で `Reviewed` も戻す（印は器の側の理由で落ちた便を戻す口で、中身が変わっていない便を審査へ送り直す＝FR49 の「中身が変わるまで」を印で破る）／鍵に要件文も入れる（SRS の 1 字の改訂で、その要件を指す契約が一斉に列へ戻る）／審査の判定を § の sha に紐づけて記録する（新しい on-disk の面を足す＝超過した旧案（§2「審査の時点」）と同じ型・C17.2）／設計 doc を直す便で契約 file の字も必ず動かす運用にする（手書きの規範文を増やす・C1 / N2）。
- 歯（`pipe_dispatch_section_key_` 接頭辞・置き場は列の歯の file）: (a) 審査 INCONCLUSIVE で終端した `Reviewed` の便の契約が、§ の本文を直した後の 1 周で列に戻る（`dispatch ls` の理由が値なしの欄になる）／(b) § も契約 file も変わっていない周は列外のまま（無限に起こし直さない）／(c) 審査 FAIL で終端した便も § を直せば戻る／(d) § の写しを持たない便と、写しが在るのに読めない便は契約 file だけの鍵で今までどおり列外（母集団 = 写しの 3 値: 在って読める / 在るが読めない / 無い）／(e) `Landed` の便は § を直しても戻らない（母集団 = 終端の段の種類）／(f) § の本文を 1 文字だけ変えた周も戻る（列が突き合わせる本文が審査の材料と同じ 1 本から出ている pin）／(g) `release` の印の既存の規則は変わらない（既存の歯が測る側・行の verify がその接頭辞も撃つ）。

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

[[contract]]
id = "h"
title = "列へ戻す印 — Settled の判定が、同じ契約 file を持つ直前の便の最後の記帳より後の release を見て 1 回だけ列外を外す（Landed と審査 FAIL は外さない・新しい event kind は足さない）"
req = ["FR68", "FR49", "NFR4"]
section = "12"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_release_requeues_"]
size = "S"
done = "偽の台帳と run dir の fixture で、Failed で終端した便の bead が Settled で列外に居り、その後の release で同じ sha のまま列に戻って dispatch ls の reason が - になり、起こし直した便が同じ sha でまた終端に着くと再び Settled になり、終端より前の release は効かず、Landed の便と審査 FAIL の便は release の後も Settled のままで、Stopped の便と gate の判定で終端になった便は戻り（母集団 = 終端の段の種類）、戻すかどうかの段の弁別は段の型の網羅の match 1 本で持つ"

[[contract]]
id = "i"
title = "起動の失敗の理由と repo の名指し — spawn が実装役の stderr を捕らえて run dir の log に残し呼び手の stderr にも流す・pipe の --repo の読み手を 1 本に畳んで値を std::path::absolute で絶対にする"
req = ["FR30", "NFR4"]
section = "12"
write-set = ["crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/tests/e2e/pipe/launch_failure.rs", "crates/scribe2/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_spawn_runner_stderr_", "cargo nextest run -p scribe2 --no-tests=fail pipe_repo_relative_"]
size = "S"
done = "stderr に 1 行書いて rc 2 で落ちる偽 runner の便が Failed detail=runner-rc:2,commits:0 に着いた後、run dir の stderr の log にその 1 行が見出し行付きで残り、呼び手の stderr にも同じ行が出て、stderr が空の周は file が作られず、stderr に書いても rc 0 で commit 1 の偽 runner は Implemented に着き、--repo を相対 path で渡した pipe run が絶対 path で渡した周と同じ worktree の場所と同じ段に着く"

[[contract]]
id = "j"
title = "関門が開いた待ちの便の再開 — 起こし直しの候補に回答済みの Questioned と承認済みの Blocked（生きている driver の居ない便）を足し、pipe answer / pipe approve の記帳の直後にも同じ 1 周を撃つ（関門の判定は resume の入口と同じ 1 本・新しい段も event kind も足さない）"
req = ["FR68", "FR32", "FR16"]
section = "13"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/polarity.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_waiting_gate_", "cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_driver_", "cargo nextest run -p scribe2 --no-tests=fail pipe_question_", "cargo nextest run -p scribe2 --no-tests=fail pipe_approval_"]
size = "M"
done = "偽の台帳と偽 runner の toy repo で、回答済みの Questioned の便（driver の札なし）が手動の 1 周で --drive 付きの resume で起こされて先の段へ進み（resumed:1）、未回答の Questioned の便と古い質問に回答が在っても最新の質問が未回答の便は起こされず（resumed:0）、承認済みの Blocked の便も同じく起こされ、札の 4 値（無い・所有者が死んでいる便は起こす／所有者が生きている・在るのに読めない便は触らない）がそれぞれ測られ、道具を渡した pipe answer と pipe approve の記帳の直後に同じ 1 周が撃たれて便が進み、道具を渡さない pipe answer は記帳だけで rc 0 のまま、回答と承認の stdout は記帳の 1 行だけで、1 周が失敗しても回答の rc は変わらず、候補の選別の pure な fn が段の前進の 3 値のそれぞれで測られ（段を前へ進めた driver の周は関門の候補をそのまま起こし、同じ段のままと段が戻った driver の周は 0 本にし、driver でない周は絞らない・in-file の歯）、段を前へ進めた driver の終端の 1 周が別の回答済みの便を起こし（resumed:1）、関門の判定は resume の入口と列が同じ述語 1 本を呼び、待ちの段でない便の起こし直しの規則と未承認の Blocked を外す既存の歯（pipe_dispatch_driver_ の歯）と、質問と回答の歯（pipe_question_）と承認の歯（pipe_approval_）は測っている約束を変えずに緑のまま"

[[contract]]
id = "k"
title = "台帳の読みの子 process を名指された repo の中で撃つ — 台帳の読みが cwd を引数で取り、列は --repo の値を・SessionStart は payload の cwd を渡す（器は cwd を推さない・close は上限の余地が足りず別の行・新しい断りも rules 行も足さない）"
req = ["FR68", "FR30", "NFR4"]
section = "14"
write-set = ["crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/hook.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_ledger_cwd_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail hook_brief_ledger_is_unknown_when_the_client_is_unreadable"]
size = "S"
done = "cwd を書き出してから台帳の JSON を吐く偽の台帳 client を --bd で渡し process の cwd を別の dir にしたまま pipe dispatch ls --repo <toy> を撃つと子の見た cwd が toy repo になり（process の cwd でない）、--repo を相対 path で渡した周も子の見た cwd が同じ絶対 path になり、無い dir を --repo に渡した周は列が DISPATCH-UNMEASURED の行で 0 本になり（DISPATCH-NONE と融合しない）、SessionStart の {ledger} の行は 1 字も変わらず、台帳 client の引数と待ち上限の rules 行と LedgerError の 2 値と件数の 1 行の字面は変わらない"

[[contract]]
id = "l"
title = "席が測り直して PASS になった Gated の便を列が起こし直す — 起こし直しの候補に「Gated ∧ verdict が PASS ∧ 札が無いか所有者が死んでいる」を 1 枝足す（既存の枝は不変・PASS 以外と読めない verdict は候補にしない・段を前へ進めた driver の周だけ起こす）"
req = ["FR68", "FR14", "NFR4"]
section = "15"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_gated_pass_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_driver_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_waiting_gate_"]
size = "S"
done = "verdict の読みは既存の読み手（land.rs の verdict_of・可視性は不変）を列から呼ぶだけで足り、偽の台帳と偽 runner の toy repo で、verdict が PASS ∧ 札の無い Gated の便が手動の 1 周で --drive 付きの resume で起こされて先の段へ進み（resumed:1）、verdict が INCONCLUSIVE の便と verdict を読めない便は起こされず（resumed:0）、札の所有者が生きている便と札が在るのに読めない便は触らず（母集団 = 札の 4 値）、札の所有者が死んでいる Gated の便は verdict が PASS の周も INCONCLUSIVE の周も今までどおり起こされ、段を前へ進めなかった driver の終端の 1 周はこの候補を 1 本も起こさず、待ちの段の候補の規則と待ちの段でない Gated 以外の便の規則を測る既存の歯（pipe_dispatch_driver_ と pipe_dispatch_waiting_gate_）は測っている約束を変えずに緑のまま"

[[contract]]
id = "m"
title = "列外の鍵に審査役へ渡る材料を含める — Reviewed で終端した便の鍵に、行の section が指す § の本文（審査の材料の dir の写し）を足す（§ の読みは審査と同じ 1 本・写しが無い周は契約 file だけの鍵に倒す・Reviewed 以外の段の鍵は不変・新しい file も field も足さない）"
req = ["FR68", "FR49", "NFR4"]
section = "16"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_section_key_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_release_requeues_"]
size = "M"
done = "偽の台帳と run dir の fixture で、審査 INCONCLUSIVE で終端した Reviewed の便の契約が § の本文を直した後の 1 周で列に戻って dispatch ls の理由が値なしの欄になり、§ も契約 file も変わっていない周は列外のままで、審査 FAIL で終端した便も § を直せば戻り、§ の写しを持たない便と写しが在るのに読めない便は契約 file だけの鍵で今までどおり列外になり（母集団 = 写しの 3 値）、Landed の便は § を直しても戻らず（母集団 = 終端の段の種類）、§ の本文を 1 文字だけ変えた周も戻り、Reviewed 以外の段の鍵と dispatch ls の理由の字面と event kind は変わらず、release の印の既存の規則を測る歯（pipe_dispatch_release_requeues_）は測っている約束を変えずに緑のまま"
<!-- contracts:end -->
