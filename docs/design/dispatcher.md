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
- **要件との対応**: [FR68](../../design-intent/spec/srs.html#FR68) は、所有者の印（本 doc の札）を持たなくても再開を 1 回起こす便を 4 種に閉じている——回答済みの Questioned・承認済みの Blocked・**verdict が PASS の Gated**・regate で `Implemented` へ戻された便（§23）。本行が足す枝はその 3 種目そのもので、要件の外の例外を足さない。同じ要件の「verdict が PASS でない Gated の便と verdict を読めない Gated の便は起こさない」「この便のための契機は足さず既存の契機の周で拾う」も本行の約束と同じである（下の 2 項と、歯が手動の 1 周で測ること）。
- **PASS 以外は候補にしない**: verdict が INCONCLUSIVE の Gated 便を候補にすると `pipe resume` が再 gate へ倒す（`crates/scribe2/src/pipe/cli/resume.rs` の既存の分岐）＝器が勝手に 1 周ぶんの費用（[gate-cost.md](./gate-cost.md) §2）を払い直す。verdict を読めない周も候補にしない（測れないを「通った」に読み替えない・fail-closed・NFR4）。札が `Live` / `Unreadable` の便は触らない（§13 の 4 値の読みをそのまま使う）。
- **§13 の絞りをそのまま受ける**（空撃ちの連鎖を塞ぐ）: この候補の札は起こす前も後も `Absent` なので、§5 の止め金（resume が抜けると札が消えて候補から落ちる）が効かない。よって driver が撃つ終端の 1 周は、§13 の関門の候補と同じく**段を前へ進めた周だけ**この候補を起こす（段の前進の 3 値をそのまま読む・前進以外は 0 本）。driver でない契機（手動の 1 周・印の直後・回答や承認の記帳の直後）は今までどおり絞らない。
- **flag の無い driver は自分の便をこの候補にしない**（実装役の質問 2026-09-20 への裁定・§5「flag の無い resume は 1 段だけ」を保つ）: 実測——`--drive` を持たない `pipe run` / `pipe resume` も終端の 1 周を撃つが、`crates/scribe2/src/pipe/cli.rs` の `dispatch` は自分が段を進めた便（`Driven`）を **flag の在る周にだけ**列の入力（`Input` の `driving`）へ渡すので、flag の無い周は列から見て手動の 1 周と同じ形になる。このまま枝を足すと、flag の無い resume が Implemented → Gated（PASS）で抜けた直後の自分の 1 周が、札の外れた自分の便を拾って Landed まで運び、§5 の歯 `pipe_dispatch_drive_resume_hands_off_only_with_the_flag` が赤になる。形: `dispatch` は flag の有無に依らず自分が段を進めた便の id を列の入力に渡し（`Input` に項目 1 つ・`driving` の意味は変えない）、`revivals` の**新しい枝だけ**がその id の便を候補から外す。flag の在る driver の自分の便は今までどおり handoff の経路（§5）が運ぶ。別の契機（手動の 1 周・他の便の driver の終端の 1 周）は、flag の無い driver が置いていった PASS の Gated の便を拾う——これは [FR68](../../design-intent/spec/srs.html#FR68) の 3 種目どおりで、「1 段だけ」は**その process が**進める段の数の約束である。
- 触らない: 待ちの段の集合（`WAITING`）と §13 の候補の規則・§5 の「待ちの段でない便は札の所有者が死んだものだけ」の規則（`Gated` 以外の段）・札の形と 4 値・段の生死の match・起こし直しの構築点（`--drive` 付きの `pipe resume`・道具は列と同じ 1 本）・`dispatch ls` の行の字面・`dispatch=` の record token の形。
- 却下案: 席に `resume` の権能を足す（権能の行の変更＝user の裁定が先・§11）／測り直しの口（`pipe gate`）が PASS を書いた周に自分で次の段を撃つ（関門の口が起動の口を兼ねる＝1 つの口が 2 つの権能を持つ・ADR-0045 §2 (1)）／`Gated` を `WAITING` に入れる（待ちの段は「人の手を待つ」意味で、承認と回答の関門の判定がそのまま当たらない）。
- 歯（`pipe_dispatch_gated_pass_` 接頭辞・置き場は列の歯の file）: (a) verdict PASS ∧ 札の無い `Gated` の便が手動の 1 周で `--drive` 付きの resume で起こされ（`resumed:1`）先の段へ進む／(b) verdict INCONCLUSIVE ∧ 札の無い `Gated` の便は起こされない（`resumed:0`）／(c) verdict を読めない `Gated` の便も起こされない（`resumed:0`）／(d) 札の所有者が生きている便と札が在るのに読めない便は触らない（母集団 = 札の 4 値）／(e) 札の所有者が死んでいる `Gated` の便は verdict に依らず起こされる（既存の規則を PASS と INCONCLUSIVE の両方で測る）／(f) 段を前へ進めなかった driver の終端の 1 周はこの候補を 1 本も起こさない／(g) 待ちの段の候補の規則と、待ちの段でない `Gated` 以外の便の規則は変わらない（既存の歯が測る側・行の verify がその 2 接頭辞も撃つ）／(h) flag の無い resume が Implemented → `Gated`（PASS）で抜けた直後の自分の終端の 1 周は自分の便を起こさず（段は `Gated` のまま）、その後の手動の 1 周は同じ便を起こす（正負の対）。§5 の既存の歯 `pipe_dispatch_drive_resume_hands_off_only_with_the_flag` は 1 字も変えずに緑のまま（行の verify が完全名で撃つ）。

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

## 17. 起こした便が受付に届かない周は同じ bead を起こし直さない（契約表の行 n・`s2-07l.509`・約束の行の形）

やさしく言うと: 列は「起こした」ことを覚えていないので、受付で落ちた便を毎周起こし直し、落ち続けると台帳を詰まらせて自分で自分を止められなくなる。起こした事実を記録に残し、受付に届くまで同じ bead を起こさない。

- 出所: memo `s2-07l.509`（隣の repo の便で 73 本常駐・hold で収束・2026-09-21）。user 裁定 2026-09-21（分解表 G4 を推奨どおり・逐語は台帳 `s2-07l.505` の notes）。
- 現物（verified・main）: `crates/scribe2/src/pipe/dispatch.rs` の `start` → `spawn_self` は子を `pipe intake …` で起こして `spawn` の成否だけを返し、stdout / stderr は `Stdio::null()`。列（`turn` / `fire`）は「起こした便が `RunCreated` に届いたか」を見ない。起こした事実は event に無い（`RunCreated` は intake が書く）。intake が台帳 timeout（環境）で落ちた周は run dir も event も無く、次の周が同じ bead をまた起こす。
- 形（印の 1 値と候補の条件 1 つ・新しい file も rules 行も足さない）:
  1. **起こす前に印を書く**: `fleet/mod.rs` の `Mark` に 4 値目 **`Launched`** を足し、`start` の直前に既存の `mark(` の口で bead 名義の `DispatchMark`（mark = Launched・detail = 起こした argv の subcommand 1 語）を書く。書けない周は起こさない（fail-closed・記帳できない起動を数えない）。書けない周の理由は既存の `WaitReason::Admission` に閉じた語を 1 つ足して出す（`SLOT` / `SPAWN` と同じ `'static` の語・新語 `MARK` = `mark`・`dispatch ls` は `admission:mark`＝測れない側）。`Mark` の網羅 match は `fleet/mod.rs` の `as_str` / `parse` と `dispatch.rs` の `marks_of` の 3 か所で、宣言順の slice `MARKS`（`enum-slices` の母集団・`parse` が読む）にも 4 値目を足す（`marks_of` は `Launched` を hold と同じ側に畳まず、独立の値として最新の 1 つを持つ）。`fleet/wait.rs` の private な `struct Mark`（file の印・別の型）は同名なだけで本行は触らない（閉包が名で拾うので write-set に載るが diff は 0 行）。
  2. **候補の条件**: その bead の最新の `Launched` より後に、その bead の `RunCreated` も `Release` の印も無い周は起こさない（`WaitReason` に 1 値 **`Launched`**・`dispatch ls` の理由は `launched:<ts>`。`WaitReason` は `dispatch.rs` の 1 file に閉じ、網羅 match は同 file の `as_str` と `render` の 2 か所・宣言順の slice `WAIT_REASONS` に `launched` を足す・他 module に match は無い）。`RunCreated` が来れば従来の live / 終端の判定に戻る。intake が受付で断った便（run dir 0）は `Release` の印まで起きない＝落ち続ける便が台帳を詰まらせる正帰還が閉じる。`hold` / `first` の印の意味は不変。
  3. **子の stderr を残す**: `spawn_self` の stderr を `<state_dir>/pipe/launch.log` に append する（stdout は null のまま・file は書けなければ起こさない側に倒さず null に落とす＝起動を記録の失敗で止めない）。読み手は席（C10・死因が観測できなかった .509 の穴）。
- 触らない: `hold` / `release` / `first` の印の意味・§2 の列外の鍵・受付の判定（intake は変えない）・`Turn` の形（`launches` は印を書けた分だけ）・event の schema（`DispatchMark` の欄は既存のまま・`mark` の値が 1 つ増えるだけ）。
- 却下: 時間の冷却（rules 行 `pipe.launch_cooldown_s` を足す・値の裁定が要り、台帳 timeout の周は何秒待っても同じ理由で落ちる）／N 周で自動 hold（周の間隔が契機依存で N の意味が定まらない・`Launched` 1 回で止める方が読みやすい）／state dir の印 file（記帳と別の状態・C3）。

## 18. 列が起こす前に器の健康の遮断器を通し、受付で止まったまま運転手の居ない便を live に数えない（契約表の行 o・`s2-07l.509`・約束の行の形）

やさしく言うと: 台帳や host が詰まっている周に新しい便を起こしても落ちるだけなので、gate と同じ遮断器を列にも通す。受付の途中で死んだ便の亡骸が「走行中」と数えられて次の便を塞ぐので、札の無い受付中の便は走行中と読まない。

- 出所: memo `s2-07l.509`（候補 1 の後半 = 健康の遮断器と Intake で止まった run の畳み）。user 裁定 2026-09-21（G4）。
- 現物（verified・main）: 遮断器は `crates/scribe2/src/pipe/health.rs`（`now(per_core)` → `Health` の 3 値・`act` が `Action` と `Mark` を返す）で、呼び手は gate の `crates/scribe2/src/pipe/gate/verify.rs`（`health::pass(checks.host)`・`Breaker` は `gate.rs` の `breaker()` が manifest の 2 行から組む）だけ。列は通していない。live の判定は `crates/scribe2/src/pipe/cli/state.rs` の `live(state_dir, id, stage)` で、`Stage::Intake` は無条件に `Some(true)`＝受付の途中で運転手が死んだ便（`RunCreated` の後に event が無く、札も無い）が永遠に live のままで、同じ write-set の便を `overlap:<亡骸>` で塞ぐ（隣の repo で実測・席から stop が撃てない置き場では持ち主の手が要った）。
- 形:
  1. **列の遮断器**: `turn` / `fire` が候補を起こす前に `health::now(per_core)` を 1 回読み、`act` の `Action::Wait` の周は 1 本も起こさない（`WaitReason` に 1 値 **`HostBusy`**・`dispatch ls` の理由は `host-busy`）。`Unmeasured` は `act` のとおり起こす側（gate と同じ 1 実装・C2）。`per_core` は gate と同じ 2 行（`host.runnable_per_core` / `host.blocked_per_core`）を同じ読み手で読む: 読み手は `crates/scribe2/src/pipe/cli/step.rs` の private な `limits_of`（`int_row` 8 行で `Limits` を組む）なので、これを `crates/scribe2/src/pipe/gate.rs` の `Limits` に `pub(crate)` の関連関数として**純移動**し（step.rs はそれを呼ぶだけ・本文は 1 字も変えない）、列は `Input` が既に持つ `manifest`（`dispatch.rs` の `Input.manifest`）から `Limits` を組んで `.breaker().per_core` を `health::now(per_core)` に渡す。**`Limits::breaker` は動かさない**（呼び手 `crates/scribe2/src/pipe/gate/record.rs` と `health::pass` の呼び手 `crates/scribe2/src/pipe/gate/verify.rs` は不変＝write-set の外）。閉じた型の追加が閉じる面（実測・main）: `WaitReason` の宣言と `WAIT_REASONS`（kebab の名の列）と網羅 match 2 つ（`as_str` / `render`）と全 variant を列挙する in-file の歯（`pipe_dispatch_wait_reasons_render_the_name_and_the_value`）は**全部 `crates/scribe2/src/pipe/dispatch.rs`** に在り、`dispatch ls` の理由の字面は `render` 経由（同 file・分岐なし）、`pipe/notify.rs` と `pipe/dispatch/candidates.rs` は `render` / 構築を呼ぶだけで match を持たない＝1 値の追加は dispatch.rs の中で 1 周閉じる（in-file の歯の列挙に `HostBusy` を足す）。
  2. **受付で止まった便は live でない**: `live` の `Stage::Intake` の枝を「札（`<state_dir>/pipe/<run>/driver`・`pipe/mod.rs` の `Ticket` の 4 値）が `Live` なら `Some(true)`・`Dead` / `Absent` なら `Some(false)`・`Unreadable` なら `None`」にする。他の段の枝は不変。読み手は既存の `Ticket`（`fleet/store.rs` の `Owner` を写す）で、新しい probe は足さない。
- 触らない: 遮断器の閾値と 3 値・gate の呼び方・`Ticket` の 4 値・`Intake` 以外の段の live・overlap の式（live の集合が変わるだけ）。
- 却下: 列だけの別の閾値（rules 行が増える・gate と違う判断になる）／Intake の便を時間で畳む（時間の裁定が要る・札で読める）／dispatcher が亡骸の run を `RunStopped` で終端にする（列が記帳する面を増やす・stop の口は席にある）／`Limits::breaker` を `health.rs` へ移す（呼び手の `gate/record.rs` と `gate/verify.rs` が write-set に入り交差が増える・値の読み手を寄せれば足りる）。

## 19. 便の終端と「起こす便 0 ∧ 候補あり」を登録 row の席の pane へ 1 行で知らせる（契約表の行 p・`s2-07l.507`・約束の行の形）

やさしく言うと: 便が落ちても席に誰も知らせないので、席は user に聞かれるまで気づかなかった（6 時間）。運転手が自分の終端で席の pane に 1 行送る。

- 出所: memo `s2-07l.507`（user 指摘 2026-09-21・6 時間の放置）。user 裁定 2026-09-21（分解表 G2 を候補 1 で）。
- 現物（verified・main）: 運転手の終端の 1 周は `crates/scribe2/src/pipe/cli.rs` の `TERMINALS`（run / resume / land / stop / retire）の後に `queue::fire` を撃つ（`driving` の周だけ stdout に 1 行）。席の pane への送達は `crates/scribe2/src/seat/inject.rs` の `deliver_within(Request, window)`（`Request` は target / socket / payload / state_dir・結果は `Delivery` の 3 値）が既に在り、登録 row は `crates/scribe2/src/fleet/replay.rs` の `State.registrations`（`(Role, anchor)` → 最新の `Registration`・`target` を持つ）から読める。列から席へ知らせる口は無い。
- 形（送るのは運転手・1 行・送れない周は理由を残す）:
  1. **契機は 2 つ**: (a) 運転手の終端の周で、自分の便の最後の段が `Reviewed` の FAIL / INCONCLUSIVE・`Gated` の FAIL / INCONCLUSIVE・`Failed`・`Questioned`・`Stopped` のとき。(b) 同じ周の列の結果が「起こした便 0 ∧ 候補 1 本以上」のとき（席が dispatch の周を撃つ契機）。`Landed` と PASS は送らない（静かな正常）。列の結果は `queue::fire`（`crates/scribe2/src/pipe/dispatch.rs` の `fire`・`cli.rs` が終端の周で既に撃つ）の返り値 `Turn` から**読むだけ**で組む: 候補の本数 = `candidates` の長さ・起こした便 = `launches` の長さ・先頭の候補の理由 = `candidates` の先頭の `reason` の `render`（`WaitReason` の既存の字面・`hold` 等）。`Turn` / `Candidate` / queue 側は 1 字も触らない（欄は既に在る）。
  2. **宛先**: fleet の replay の `State.registrations` から `(Role::Orchestrator, anchor = 便の repo)` の最新 row の `target`（tmux の pane）。row が無い周は送らず、理由 `no-seat`。
  3. **本文は 1 行**: `scribe2 pipe: <bead> <run> <段>=<verdict か kind か detail の 1 語> — 次の 1 手は pipe dispatch ls`（(a)）／`scribe2 pipe: idle ready=<候補の本数> launched=0 reason=<先頭の候補の理由>`（(b)）。対話面の作法（次の 1 手が先頭・dialogue-surface.md §2）に合わせて 1 行に閉じ、逐語も path も載せない（PUBLIC 面ではない state dir だが、pane は人が見る）。
  4. **送達は既存の 1 関数**: `deliver_within` を `pipe.stop_grace_ms` と同じ桁の窓で 1 回撃ち、結果を運転手の stdout に `notify=<delivered|refused:<理由>|unconfirmed|no-seat>` の 1 行で残す（C10）。送達の失敗で便の rc は変えない（通知は副作用・便の終端は既に記帳済み）。
  5. **閉じた型の variant を新設の module に書かない**: 行 p の `+` の src は `Stage` / `EventKind` の variant を `match` の腕や `Type::Variant` の字面で名指さない——名指すと他 doc の行の `touches` の閉包（contract-source.md 行 c の `crate::fleet::Stage`）に新設 file が入り、その行の write-set が不完全になって `contracts check` と gate が落ちる（2026-09-21 の便 1 本で実測・finding 1）。終端の 1 語は最後の `RunStage` / `RunDone` の event の `stage` の `as_str` と `detail` の頭の語を**字面で写す**（既存の読み手 `last_stage_detail` の型・段ごとの分岐は持たない）。`Stage` の `as_str` は variant の名そのもの（`Stopped` / `Failed` / `Questioned`・`fleet/mod.rs`）なので、約束 1 の expect の `Stopped` はその字面と一致する。
  6. **新設の歯の file は `pipe/` の外に置き、宣言 file の diff は `mod` の 1 行だけにする**（flip-check の同梱の条件・2026-09-21 の便 5 周目の gate FAIL で実測）: flip-check は test file を **1 file ずつ単独で** base へ写して撃ち、宣言 file（`mod x;` の 1 行を持つ側）は差分が `mod` 行だけのときに限って本体の file と同梱する（`crates/xtask/src/flipcheck.rs` の `declaration_only` / `plan_of`・歯の外の行だけが動いた file も同梱される）。新設の歯の file を `pipe/` 配下に置くと、`crates/scribe2-boundary/tests/e2e/pipe.rs` の `pipe_hermetic_sites_stay_one`（`pipe/` 配下の tracked の file 数 9 と site 数 1 を pin する歯）を同じ file で 9 → 10 に上げざるを得ず、宣言 file が「歯の中の行が動いた file」になって同梱されない＝本体の file は base に宣言が無いまま単独で写され、compile 対象に入らず全 PASS＝`green-on-base file=<本体>` で gate が落ちる（実測: overlay の 1741 本に `pipe_notify_` の歯は 0 本・flip-check の comment の「本体だけを置くと偽 GREEN」の型）。ゆえに歯の file は行 p の約束 1 の `place` の file（`crates/scribe2-boundary/tests/e2e/` 直下・`pipe/` の外）に置き、宣言は `crates/scribe2-boundary/tests/e2e/main.rs` に `mod` 1 行を足すだけ（同 file の他の行は触らない）。pin は **9 file・1 site のまま**（`pipe/` の外は母集団に入らない）。歯は `crate::pipe` の `pub(super)` の口（`run_pipe` / `pipe_cmd`・`pub(super)` は親＝crate root の全 module から見える）で起こし、`pipe/` 配下の helper（登録 row を書く口等）を使うなら `tests/e2e/pipe.rs` の**歯の外の行**（`mod` の可視性・helper の可視性）だけを動かす（歯の外の行だけなら flip-check が同梱する・歯の中の行を 1 行でも動かすと同梱されない）。
- 触らない: event の kind（通知は記帳しない・pane の行と stdout の 1 行だけ）・`deliver_within` の中身・登録 row の形・`Landed` / PASS の便（送らない）・席の見張り（Monitor）は席の手順のまま（本行の着地後に止めてよい条件は memo .507 の昇格条件）。
- 却下: 席の SessionStart / rebrief に終端の一覧を載せる（席の turn が無いと読めない＝同じ穴）／event を足して席が poll する（poll は席の寿命に縛られる・今の見張りと同じ）／全終端を送る（Landed が多く pane が流れる・落ちた便だけが席の手番）。

## 20. pipe/dispatch.rs の「台帳から候補を組む」群を子 module へ割る（契約表の行 q・純移動・行 o の前）

- 出所（orchestrator の実測 2026-09-21・verified）: `crates/scribe2/src/pipe/dispatch.rs` は 1412 行で、受付の上限 R-C4-2（1500）の余地が **88 行**＝行 o（`s2-07l.525`・size S・見積 100）を受付が `cap-headroom` で断る（`pipe preflight` で refused を実測）。行 n（`s2-07l.524`）の着地で 96 行増えた直後の姿。
- 現物（planner の census・main 47d3c52・行番号は同 commit）: 責務は 5 群——(1) 理由と候補の型（`WaitReason` / `Candidate` / `Unmeasured` / `Handoff` / `Advance`）、(2) 起こす面（`start` / `launched` / `spawn_self` / `launch_log`）、(3) 周の本体（`turn` / `fire` / `revivals` / `order`）、(4) **台帳から候補を組む群**、(5) 表示（`line` / `render` / `usage` / `mark`）。(4) は閉じている＝外の呼び手が 0 で、親の (3) からだけ入る。群の item は 18 個・351 行で、うち**移すのは 16 個・約 325 行**: `is_input` / `entry_of` / `settle` / `Room` / `blocker` / `launch_of` / `tools` / `is_blocking` / `pointer_of` / `settled` / `section_keyed` / `section_moved` / `requeues` / `released_after` / `sizes_of` / `marks_of`（700〜1032・宣言順）。**`Ledger`（519〜530）と `Marks`（996〜1002）の 2 型は親に残す**——親の `turn` の本体が `Ledger { marks, launched, … }` の struct literal と `ledger.materials` / `marks.order` / `marks.launched` の field access で使う（498〜515 行）ので、子へ移すと field に `pub(super)` が要り、純移動の機械証明（`crates/scribe2/src/pipe/move_proof.rs`・可視性を剥くのは item の先頭行だけで body の行は剥かない）が items-differ で落ちる（2026-09-21 の便 4 本目の Question で実測）。子は親の private な field を子孫の特権でそのまま読み書きできる（Rust の可視性は module 単位・子は親の private item と field を見る）ので、子の側は `use super::{Ledger, Marks, …}` で引くだけで本文は 1 字も変わらない。
- 決定的な制約（実測）: 極性一覧（`crates/scribe2/src/polarity.rs`）は `pipe::dispatch::` の型名を 1 つも pin しない（grep 0 件）。親は `crate::seat::ledger` を `ledger` の名で `use` しているので、**子 module の名は `candidates`**（`crates/scribe2/src/pipe/dispatch/candidates.rs`・`ledger` は衝突する）。
- 名前解決の形（§43 / §45 と同じ型・可視性は名前解決をしない）: 親の本体が裸で呼ぶ 7 名（`is_input` / `entry_of` / `settle` / `tools` / `settled` / `requeues` / `marks_of`）は親に `use candidates::{…}` 1 文で戻し、in-file の歯だけが呼ぶ 3 名（`launch_of` / `released_after` / `section_keyed`）は `#[cfg(test)]` を付けた `use` 1 文で戻す（歯の区間の `use super::*` はこの 2 文の名を親の scope から拾う＝歯の本文は 1 字も変えない）。**`#[cfg(test)]` の `use` 文の置き場は歯の区間の直前**（親の `#[cfg(test)]` + `mod tests` の 2 行の直前・src の本体の全 item より後）に限る——xtask の門は file を「最初の行頭 `#[cfg(test)]` より前 = src 区間 / 以後 = test 区間」で切る（`crates/xtask/src/workspace.rs` の `split_test_src`・`env_reads.rs` / `check_sizes.rs` が同じ切り方）ので、この `use` を file の頭（`mod candidates;` の隣）に置くと src の本体が丸ごと test 区間に落ち、`myself` の `std::env::args()` が母集団から消えて `env_reads_passes_on_core_with_a_nonempty_population` が `env-reads=0/5`（期待 6 以上）で落ちる（2026-09-21 の便 2 本目の gate FAIL で実測・core-lines / test-src-ratio の門も同じ切り方で狂う）。子は必要な名を `use super::{…}` で引く（子孫は親の private item を見る・親の `use` 群を 1 行ずつ写してよい）。上げるのは**子側**の可視性だけ（親が呼ぶ 10 名〔本体 7 + 歯 3〕を `pub(super)`・残る 6 名〔`Room` / `blocker` / `is_blocking` / `pointer_of` / `section_moved` / `sizes_of`〕は private のまま・struct の field には触らない）。親側の可視性と `fire` / `turn` の本体は 1 字も変えない。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 16 item（約 325 行・`Ledger` と `Marks` は親に残す）を行 q の write-set の `+` の file へ名・本文・順序を変えずにそのまま移す。
  2. 親に増えるのは `mod candidates;` 1 行と `use` 2 文（本体用 7 名・`#[cfg(test)]` の 3 名）だけ。`turn` / `fire` / `revivals` の本体は 1 字も変わらない。`#[cfg(test)]` の `use` は歯の区間の直前に置き、file の最初の行頭 `#[cfg(test)]` の位置を src の本体より後に保つ（上の名前解決の形・`env_reads_passes_on_core_with_a_nonempty_population` が緑のまま）。
  3. in-file の歯 14 本は 1 本も動かさない（親の `mod tests` に残る・`use super::*` のまま）。
  4. 札 `// flip-check: moved <行 q の bead>` を親の歯の区間の先頭と `+` の file の先頭に対で置く（純移動の機械証明は pipeline.md §5.3）。
  5. 割った後の行数は親が **約 1090**（余地 **約 410**）・`+` の file が **約 345**＝行 o の見積 100 を満たし、size M（300）も受けられる。
  6. `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` と `crates/xtask/src/env_reads.rs` の diff は **0 行**（write-set に在るのは受付が verify の filter の当たる歯の file を要求するためだけ・pipeline.md §43 の `polarity.rs` と同じ型）。verify の 3 行目（xtask の `env_reads_passes_on_core_with_a_nonempty_population` を歯の名の全体で 1 本）が約束 2 の置き場を測る: `#[cfg(test)]` の `use` を file の頭に置いた実装では母集団が 6 → 5 に落ちて RED（2026-09-21 の便 2 本目の gate FAIL の形）、歯の区間の直前に置けば GREEN。
- verify の filter が当たる歯の母集団（orchestrator の実測・main 6c0bbc8・verified）:
  - **lib（verify 1 行目・接頭辞 7 個）**: 親の in-file の歯は **14 本**で、名は次のとおり（宣言順）——pipe_dispatch_drive_advance_is_forward_same_or_backward / pipe_dispatch_drive_hands_off_only_on_forward_and_names_the_reason / pipe_dispatch_drive_is_added_to_every_run_the_queue_starts / pipe_dispatch_drive_ranks_every_stage_from_the_declared_order / pipe_dispatch_drive_tokens_are_the_closed_five / pipe_dispatch_launched_marks_are_cleared_by_run_created_or_release / pipe_dispatch_marks_keep_the_last_one_and_release_removes_it / pipe_dispatch_order_puts_first_before_priority_then_the_issue_number / pipe_dispatch_order_reads_the_issue_number_as_digits_not_text / pipe_dispatch_release_requeues_failed_stopped_and_gated_but_not_landed_or_reviewed / pipe_dispatch_release_requeues_only_when_the_mark_follows_the_last_record_of_the_run / pipe_dispatch_section_key_applies_to_reviewed_only / pipe_dispatch_waiting_gate_admits_only_forward_drivers_and_every_non_driver / pipe_dispatch_wait_reasons_render_the_name_and_the_value。接頭辞ごとの本数は drive 5・launched 1・marks 1・order 2・release 2・section 1・wait 2（wait は末尾の `_` を付けない＝waiting_gate と wait_reasons の 2 本を 1 個で受ける）＝合計 14 で、lib 全体で 7 個の接頭辞に当たる歯も **14 本・全部この file**（母集団は lib の `#[test]` 全数・当たりの file 数 1）。lib で名に pipe_dispatch_ を含む歯は他に 1 本（`crates/scribe2/src/pipe/mod.rs` の pipe_dispatch_driver_hold_is_an_atomic_lock_that_reclaims_only_dead_owners）だけ在り、接頭辞 driver_ は 7 個に無いので当たらない。
  - **e2e（verify 2 行目・filter pipe_dispatch_）**: 名に pipe_dispatch_ を含む e2e の歯は **51 本・全部 `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`**（同 file の `#[test]` は 70 本・当たりの file 数 1＝write-set の中）。write-set の外の e2e file（`crates/scribe2-boundary/tests/e2e/pipe/stop.rs` 等）に在る driver_ の名は helper で、pipe_dispatch_ を名に含まないので filter に当たらない。
- 触らない: (1)(2)(3)(5) の群の本体・`WaitReason` / `Turn` / `Candidate` の欄・in-file の歯の名と assert・e2e の歯・**`crates/scribe2/src/pipe/mod.rs`**（src の側の file・e2e に pipe/mod.rs は無い。pipe_dispatch_driver_ の歯はそこに在り、verify の filter はどちらの行も当たらない＝上の母集団のとおり）。
- 却下: (1) の型の群を移す（`WaitReason` は行 o が variant を足す＝行 o の write-set が 2 file に割れて交差が増える）／(5) の表示の群を移す（80 行で余地が 100 に届かない）／行 o を S より小さく書く（size は S が最小）／割らずに据え置く（行 o が受付で止まったまま）／`Ledger` と `Marks` も子へ移して field に `pub(super)` を付ける（純移動の機械証明が body の行の可視性を剥かないので items-differ＝残差 0 が外れる・約束 1 と 2 が両立しない・便 4 本目の Question）。

## 21. 通知の送達を消費で閉じる — notify は置き場を渡して自席の記録と打刻を測り、Queued の周は自席の残りに Enter を 1 回だけ再送する（契約表の行 r）

やさしく言うと: 通知の本文が入力欄に入ったまま Enter だけ落ちると、席は次の通知が来るまで気づかない。送った側が「本当に turn に入ったか」を席の打刻で見て、入っていなければ Enter をもう 1 回だけ押す。

- 何が起きているか（orchestrator の実測 2026-09-21 15:54Z・verified）: `Reviewed` FAIL の通知（§19 の 1 行）が席の入力欄に本文だけ残り、Enter が落ちていた（user が入力欄で発見）。現物（main）: `crates/scribe2/src/pipe/notify.rs` の `send` は `Request` の `state_dir` を `None` で `deliver_within` に渡す。その結果 (a) 自席の注入の記録（`record`・`tick.jsonl`）が書かれず、次の通知の入力欄の門（`guard_input`）は残った本文を人の打ちかけ（`Foreign`）と読んで `refused:busy` で止まる＝最初の 1 本が残ると以後の通知が全部届かない、(b) 消費の証拠（`Watch` の seat）が無く `Settled` は常に `Unmeasured`＝本文が pane に現れただけで `notify=delivered` と出す（入力欄に居るのと turn に入ったのを弁別しない）。`send` は `send-keys -l <本文>` と `send-keys Enter` を間を置かず連続で撃つ（TUI が連続入力を貼り付けと読む周に Enter が改行に畳まれる・inferred）。自席の残りへ Enter を送り直す修復（`pass_input` の `OwnQueued`）は**次の**注入の入口にしか無い。
- 形（送達の読みは 1 本のまま・人の打ちかけと merge しない極性は不変）:
  1. **notify は解決済みの置き場を渡す**: `Queue` の `state_dir`（運転手の `--state-dir`・`crates/scribe2/src/pipe/cli.rs` の `notices` が持つ）を `StateDir`（`source` は `Provenance::Flag`）にして `Request` の `state_dir` に載せる。記録（`record`）と証拠（`Watch`）は既存の関数がそのまま動く。
  2. **Queued の周の再送は同じ呼び出しの中で 1 回だけ**: `deliver_within` は `settle` が `Queued` で窓を閉じた周に pane を取り直し、入力欄の残りが**この周の本文**（`own_queued` に `Request` の `payload` を渡す・記録の先頭ではなく送った字面そのもの）なら `send_enter` で Enter を 1 回だけ再送して同じ窓でもう 1 度 `settle` する。2 度目も `Queued` なら `Queued` のまま返す（3 回目は無い）。残りが本文でない周（`Foreign` / `UnknownInput`）は 1 key も送らない（不変）。
  3. **本文と Enter の間に settle の 1 歩**（`SETTLE_STEP`）を置く（`send` の 2 つの `send-keys` の間・新しい rules 行も定数も足さない）。
  4. **stdout は消費を写す**: `notify=delivered consumed=<true|false|unknown[:理由]>`（`Settled` の `as_str` と `reason` の既存の字面・tick の `consumed=` と同じ語彙・C10）。`refused:<理由>` / `unconfirmed` / `no-seat` の字面は不変。
- 歯（`pipe_notify_queued_` / `pipe_notify_delivery_` / `pipe_notify_foreign_` の接頭辞・`crates/scribe2-boundary/tests/e2e/notify.rs`・`pipe/` の外＝§19 形 6）: 偽の `tmux` を状態付きにする——`send-keys -l` は入力欄の file へ書き、`send-keys Enter` は「落とす回数」の file が 0 でなければ 1 減らして何もせず、0 なら入力欄を pane の本文へ移して席の打刻 file（`seat/<席>/state.jsonl`）に `UserPromptSubmit` の 1 行を足す、`capture-pane` は本文の後に prompt 行 + 入力欄を返す。(a) 落とす回数 1: 記録に Enter が 2 回・stdout に `consumed=true`。(b) 落とす回数 0: Enter 1 回・`consumed=true`。(c) 落とす回数 2: Enter 2 回（3 回目は無い）・`consumed=false`。(d) 入力欄に他人の文を先に置く: `send-keys` 0 回・`refused:busy`（不変）。
- 触らない: `guard_input` の 3 値と `Foreign` の極性・`pass_input`（次の注入の入口の修復）・`SETTLE_STEP` / `SETTLE_TRIES` の値・記録の schema と `tick.jsonl` の置き場・§19 の契機と本文と宛先・event（通知は記帳しない）。
- 却下: Claude Code の session 間 message（口座 dir の `sessions/<pid>.json` が指す socket）で送る（公開 docs に無い内部 protocol・版で変わる・他人の帳簿に書く型＝[consumer-sync.md](./consumer-sync.md) §11 の `installed_plugins.json` と同じ却下）／notify が自分で pane を読んで再送する（送達の読みが 2 本になる・C3.4）／Enter を常に 2 回送る（消費済みの周に空の submit が 1 回入る）／席の hook が通知を poll する（席の turn が無いと読めない＝§19 の却下と同じ穴）。

## 22. 審査を測れなかった便（`Reviewed` の INCONCLUSIVE `kind:unparsed`）を `release` で列へ戻す（契約表の行 s）

やさしく言うと: 審査役の答えが 1 行も返らなかった便（lens の出力に判定の行が無い）は「契約に穴がある」のではなく「測れなかった」。今はこの便も審査 FAIL と同じく契約の字か § を変えるまで列に戻らず、席が `release` を打っても効かない。測れなかった周だけは印で戻せるようにする。

- 出所（orchestrator の実測 2026-09-21T17:24Z・verified）: 便 `s2-07l.388` の審査が `verdict:INCONCLUSIVE kind:unparsed`（`review.json` の evidence = lens の出力に json の行が無い）で終端。契約も § も正しいので変える理由が無く、`pipe dispatch release` を打っても `dispatch ls` の理由は `settled:<sha>/Reviewed` のまま（§12 の「戻さない段」に `Reviewed` が丸ごと入っている）。同じ周に測れた便 `s2-07l.502` の `INCONCLUSIVE kind:section-material-missing` は § を直す正規の経路（§16 の鍵）で戻った＝2 つは別物である。
- 現物（verified・main 8af3516）: 列外の判定は `crates/scribe2/src/pipe/dispatch/candidates.rs` の `settled`（直前の同じ契約の便が終端 → `requeues(stage) && released_after(…)` → `section_keyed(stage) && section_moved(…)` の順）。`requeues` は段の型の網羅 match 1 本で `Failed` / `Stopped` / `Gated` だけを戻し、`Reviewed` は判定の中身を見ずに戻さない。in-file の歯 `pipe_dispatch_section_key_applies_to_reviewed_only` が「§ の鍵と `release` の印は同じ段を持たない」を全段で測る（この歯は 1 字も変えない）。判定の読み手は `crates/scribe2/src/pipe/review.rs` の `judgement_of`（`review.json` → `Judgement { verdict, kind, at }`・PASS でない周の `kind` は必ず `Some`・語でない周は `Unparsed`）で、`unparsed` は [FR49](../../design-intent/spec/srs.html#FR49) が「PASS でない便は FAIL として終端」と定める判定の語ではなく、lens の欠けを契約の型に化けさせないための語（C10）。
- 形:
  1. `settled` に **判定で引く 2 つ目の戻し**を足す: 段が `Reviewed` ∧ `judgement_of` が `Some` ∧ verdict が `INCONCLUSIVE` ∧ kind が `Unparsed` の便は、`released_after` が真の周に列外にしない。段で引く `requeues` は**変えない**（`Reviewed` は従来どおり false・段の census の歯は不変）。判定で引く述語は `candidates.rs` の pure な関数 1 つ（`Judgement` を受けて bool）に置き、`settled` はそれを `requeues` の次に読む（順: 段の戻し → 判定の戻し → § の鍵）。
  2. 戻すのは **INCONCLUSIVE ∧ unparsed の対だけ**: `FAIL`（kind を問わず・FR49 の判定）・`INCONCLUSIVE` で kind が他の 6 語（審査役が材料を読んで出した理由）・`review.json` が無い / 読めない（`None`・fail-closed で列外のまま）は戻さない。
  3. 印 1 回で起き直るのは 1 回（§12）: 起こし直した便がまた `unparsed` で終端すれば再び列外になる。§16 の § の鍵は不変（§ を直す経路と印の経路の両方が効く）。
  4. `dispatch ls` の理由の字面（`settled:<sha>/Reviewed`）と `release` の記帳は不変。
- 歯（接頭辞 `pipe_dispatch_release_unparsed_`・in-file は `crates/scribe2/src/pipe/dispatch.rs` の `mod tests`（`candidates` の歯の隣）・e2e は `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` の §12 の `release` の歯の隣・既存の `reasons_around_release` と `review.json` を書く fixture の型）: (a) in-file: 判定で引く述語が `INCONCLUSIVE` + `Unparsed` で真、`FAIL` + `Unparsed` / `INCONCLUSIVE` + 他の 6 語 / `PASS` で偽（母集団 = `FINDING_KINDS` の 7 語 × 3 値）。(b) e2e: `review.json` を `{"verdict":"INCONCLUSIVE","kind":"unparsed"}` にした便が `release` の後に理由 `-` へ戻り（base は `settled:…/Reviewed` のまま → RED）、同じ sha でまた同じ終端に着けば列外に戻る。(c) e2e: `{"verdict":"INCONCLUSIVE","kind":"section-material-missing"}` の便は `release` の後も理由が変わらない（不変・(b) が「INCONCLUSIVE を全部戻す」変異でないことを測る）。
- 触らない: `requeues` / `section_keyed` の網羅 match と in-file の census の歯 3 本・`released_after`・`judgement_of` と `FindingKind` の 7 語・審査の判定と `review.json` の形・`dispatch ls` の理由の字面。
- 却下: `requeues` の match に `Reviewed => true` を足す（審査 FAIL まで戻る・FR49 / FR68 に反する・census の歯が落ちる）／lens が `unparsed` の周に器が自分で撃ち直す（見分けを誤った周に §2 の無限再起動へ戻る・§12 の「器が自分で戻すことはしない」と同じ却下）／`unparsed` を `Failed` の段へ倒す（審査の段で終端した事実を消す・C10）／§ に空の 1 字を足して鍵を動かす運用（散文の作法・N2・doc の history を汚す）。

## 23. regate で Implemented へ戻された便を列が起こし直す（契約表の行 t・`s2-07l.581`）

やさしく言うと: 器の欠陥や上限で落ちた便を席が裁定つきで 1 段戻しても、その便をもう一度動かす人が居ない。戻された便は、器が次の周で自分で再開する。

- 何が起きているか（実測 2026-09-23T15:05Z・verified）: gate の一過性の赤を `pipe regate --run` で `Implemented` へ戻した便は、driver が正常に抜けて札を外していたので、手動の 1 周（`pipe dispatch`）を撃っても `resumed:0` のまま動かなかった。regate の記帳は `RunStage` 1 件（段 `Implemented`・`detail` は `regate:` + 逐語・actor は human）で札は書かない（[pipeline.md](./pipeline.md) §49 形 3 のとおり）。§49 形 3 は「既存の列が同じ worktree で gate をもう 1 周撃つ」と書くが、列の 1 周が拾う条件をこの便は満たさない。席の行は resume の権能を持たないので、人が `pipe resume --drive` を撃つ手順が戻っていた。便は driver の居ないまま live で、他の便の交差を塞ぎ続ける。
- **裁定（要件）**: [FR68](../../design-intent/spec/srs.html#FR68) は、所有者の印（本 doc の札）を持たなくても再開を 1 回起こす便を **4 種**に閉じた。4 種目が「[FR77](../../design-intent/spec/srs.html#FR77) の regate で `Gated` から `Implemented` へ戻され、その後 gate を通っていない便」である（札が無いか所有者が死んでいる便・退役の節の再開と同じ周で重なっても二重に起こさない・regate の後に gate を通って追随で `Implemented` へ戻った便と regate の記帳を持たない `Implemented` の便は起こさない・専用の契機を持たず既存の契機の周で拾う）。測るのは [AC38](../../design-intent/spec/srs.html#AC38) の「regate 後の再開 2/2（起きない 2/2・二重起動 0）」で、regate の口そのものは [AC47](../../design-intent/spec/srs.html#AC47) が測る。本行は要件の 4 種目そのもので、要件の外の例外を足さない。
- 現物（本行の base・verified）:
  - 起こし直しの候補は `crates/scribe2/src/pipe/dispatch.rs` の `revivals` の 1 本が決める。置き場を 1 回読み（pipe の mod の `current`＝置き場の全 event を読んで replay する 1 本・event の列は手元に残さない）、段の生死の述語 `live` が真（Some(true)）の便を待ちの段（`WAITING` = `Blocked` / `Questioned`）とそれ以外に分ける。それ以外の便は `super::driver_is_dead`（札の所有者が死んでいる）か、`gated` の周の `passed_gate`（`Gated` ∧ verdict が PASS ∧ 札が無いか所有者が死んでいる・§15）で候補になる。
  - `Implemented` は `live` が真（Some(true)）を返し、待ちの段に無いので後者に落ちる。札は driver が正常に抜けた周に外れる（§5）ので、regate で戻された便の札は `Ticket::Absent` で、`driver_is_dead` は偽になり、`passed_gate` も段が違うので偽になる＝候補にならない。札が `Ticket::Dead` の便だけは今も `driver_is_dead` で起こされる。
  - 「最新の `Gated` の `RunStage` より後ろに `regate:` の `RunStage` が在るか」を読む述語は既に 1 本在る: `crates/scribe2/src/pipe/regate.rs` の `regated_since_gate`（event の列と便 id を受ける pure な関数・可視性は file の中だけ）。regate の口が「1 つの FAIL につき 1 回」を判じるのに使っている（§49 形 4）。
  - 札の読みは `super::driver_ticket` の 4 値（`Ticket::Absent` / `Ticket::Dead` / `Ticket::Live` / `Ticket::Unreadable`）で、§13 と §15 が同じ読みを使う。
  - 起こし直しの結果（`Revive`）は便 id と argv だけを持ち、理由の型を持たない。1 周の行（`dispatch=started:…,resumed:…,waiting:…`）も本数だけである。
- 形（判定の順: 段の生死 → 待ちの段か → 既存の枝 → 新しい枝〔段 → regate の記帳 → 札〕。番号は done と 1:1）:
  1. **`revivals` に 1 枝だけ足す**: 待ちの段でない live 便は、今までの 2 つの枝に加えて **`gated` の周に「段が `Implemented` ∧ `regated_since_gate` が真 ∧ 札が `Ticket::Absent` か `Ticket::Dead`」** でも候補にする。既存の枝（`driver_is_dead` と `passed_gate`）と待ちの段の枝は 1 字も変えない。読み手は regate の口と同じ 1 本（C2）で、`crates/scribe2/src/pipe/regate.rs` の `regated_since_gate` の可視性を pipe の中へ開いて列から呼ぶ（本体は 1 字も変えない）。event の列は `revivals` が `crates/scribe2/src/fleet/store.rs` の `read_all` で 1 回だけ読み、`crates/scribe2/src/fleet/replay.rs` の `replay` で便の表にし、同じ列を `regated_since_gate` へ渡す（現物の `current`〔read_all → replay の 1 本・列を返さない〕は `revivals` では使わず、その 2 段を `revivals` が自分で持つ＝置き場を 2 回読んで表と列が食い違う周を作らない・`pipe/mod.rs` は触らない）。口の本体 `regate`（`crates/scribe2/src/pipe/regate.rs`・pipe の中だけの可視性）は不変。
  2. **起こさない便**（FR68 の除外をそのまま）: regate の記帳を持たない `Implemented` の便（札が無い便は §5 のまま触らない）。regate の後に `Gated` の `RunStage` を経た便（gate を通った後に追随で `Implemented` へ戻った便を含む＝最新の `RunStage` は追随の `rebase:` で、`regated_since_gate` は偽）。札が `Ticket::Live` の便と `Ticket::Unreadable` の便（§13 の 4 値の読みのまま・測れないを「居ない」に読み替えない）。
  3. **`gated` の絞りをそのまま受ける**（§13 と §15 と同じ止め金）: この候補の札は起こす前も後も無いので、driver が撃つ終端の 1 周は**段を前へ進めた周だけ**この候補を起こす。driver でない契機（手動の 1 周・印の直後・回答や承認の記帳の直後）は絞らない。regate の口は 1 周を撃たない（FR68「専用の契機を持たない」）＝戻した後の最初の契機は手動の 1 周か別の便の終端である。
  4. **二重にしない**: 札が `Ticket::Dead` の便は既存の `driver_is_dead` の枝と新しい枝の両方に当たるが、`revivals` の絞りは便ごとに 1 回の判定なので `Revive` は 1 本である。呼び手の便を継ぐ push は既存の重複の除外（同じ便 id が在れば足さない）をそのまま通る。
  5. **flag の無い driver の除外は要らない**（§15 との違い）: 自分の便を前へ進めた driver の周は、その便の最新の `RunStage` が `Gated` になっているので `regated_since_gate` が偽＝自分の便に当たらない。進めなかった driver の周は形 3 で 0 本である。`Input` の項目は足さない。
  6. **型も字面も足さない**: `Revive` に理由の field を足さない・`WaitReason` の変種も `Stage` と `EventKind` の変種も増えない・1 周の行と `dispatch ls` の行の字面は変わらない。よって core の enum は write-set に入らず、外形の snapshot も動かない。
  7. **AC47 の不足分を歯に足す**（突き合わせは下の歯の節）: 札の死んだ形で regate を通した周の不変 3 記録と、裁定なしの周を K 回回しても戻らないこと（自動の regate 0/K）を列の e2e の歯で測る。通る 2 形と断る 6 形を口の本体で撃って、断る形は何も書かず理由が形ごとに違うこと（母集団 8 形）と、戻した直後の 2 度目の断りの理由が段であることを regate の in-file の歯で測る。`crates/scribe2/src/pipe/regate.rs` の src は形 1 の可視性の 1 行だけが変わる。
- 触らない: 待ちの段の集合と §13 の候補の規則・§15 の `passed_gate` と呼び手の便の除外・§5 の「待ちの段でない便は札の所有者が死んだものだけ」の規則（`Implemented` の regate の枝の外）・札の形と 4 値・段の生死の match・regate の口の受付の 4 条件と記帳の形・起こし直しの構築点（`--drive` 付きの `pipe resume`・道具は列と同じ 1 本）・1 周の行と `dispatch ls` の行の字面。
- 却下案: regate の口が自分で driver を起こす（`--runner` / `--lens` を受ける・起こす側が 2 つになり、関門の口が起動の権能を兼ねる＝ADR-0045 §2 (1)・§15 の却下と同じ）／現状のまま人が resume を撃つ（planner 裁定 2026-09-19 の「人が resume を撃つ手順を戻さない」に反する）／`Implemented` ∧ 札が無い便を全部候補にする（spawn の直後や追随の後の便まで起こす＝§5 の「札の無い便は触らない」を丸ごと外す・FR68 の除外に反する）／最新の `RunStage` の `detail` の頭の語だけで判じる（regate の口と別の読み手を作る＝C2・「gate を通ったか」を記帳の順で読む述語は既に 1 本在る）。
- 歯（e2e は接頭辞 `pipe_dispatch_regated_`・`crates/` 全体の fn 名の substring として base に 0 件・置き場は `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` の §15 の歯の隣。その file は段の型の変種を 1 つも名指さない〔base で 0 件〕ので、段は既存の helper の字面で数え、段の型の変種を名指さない＝他の doc の行の閉包を広げない）。fixture は §15 の `Gated` の helper に verdict FAIL を渡して置き、`pipe regate` を撃って戻す。
  (a) 形 1: 札の無い regate 済みの便が手動の 1 周で `--drive` 付きの resume で起こされ（`resumed:1`）、`Gated` の記帳が 1 件増えて `Landed` まで進む。base は `resumed:0`＝**機能不在の RED**。
  (b) 形 4 と AC47 の通る側の札の死んだ形: 札の所有者が死んでいる判定 FAIL の `Gated` の便に `pipe regate` を撃つと rc 0 で 1 行が出て、便の worktree の path・worktree の HEAD・判定の file の verdict が 1 字も変わらない（AC47 の不変のうち札の死んだ形の 3 記録）。続く手動の 1 周は `resumed:1`（2 でない）で、`Gated` の記帳が 1 件だけ増え、死んだ札は外れる。base でも緑（regate の口は land 済み・起こし直しは既存の `driver_is_dead` の枝）＝二重起動 0 と不変の回帰の歯で、単独の変異を持たない。
  (c) 形 2: regate の後に PASS の gate を通し、main を進めて `pipe follow` で `Implemented` へ戻した便（札なし）は手動の 1 周で `resumed:0`・段は動かない。「最新の `Gated` より後ろ」を「regate の記帳が 1 件でも在る」に替える変異で赤になる（base では緑）。
  (d) 形 2: 札の所有者が生きている regate 済みの便と、札が在るのに読めない regate 済みの便は `resumed:0` で札も触らない（母集団 = 札の 4 値・(a)(b) と合わせて 4 形）。札の条件を外す変異で赤になる。
  (e) 形 3: 行 b の便を INCONCLUSIVE の `Gated` に置いて INCONCLUSIVE の lens で `--drive` の resume を撃つ（段が動かない＝`drive=no-progress`）。その終端の 1 周は、同じ置き場で regate 済みの行 a の便（札なし）を起こさず、その後の手動の 1 周は起こす（正負の対）。正の側は base で `resumed:0`＝**機能不在の RED**。`gated` の絞りを外す変異で負の側が赤になる。
  (f) AC47 の自動の regate 0/K: 判定 FAIL の `Gated` の便（札が無い形と札の所有者が死んでいる形の 2 形）に、`pipe regate` を撃たずに手動の 1 周を K 回撃つと、どの周も `resumed:0` で、`Gated` の後ろに `Implemented` の記帳が 1 件も増えない（assert に K と 2 形を出す）。base でも緑＝「裁定なしに自動では戻さない」の回帰の歯で、本行の枝の単独の変異は持たない（判定 FAIL の `Gated` は段の生死の絞りで先に外れる）。
  (g) 形 2 の「regate の記帳を持たない `Implemented` の便は起こさない」は既存の歯 `pipe_dispatch_driver_live_run_without_a_ticket_is_left_alone` が測る側である（`regated_since_gate` の条件を外す変異で、札の無い `Implemented` の便が起きて赤になる）。行の verify がその歯を含む接頭辞を撃つ。重複の歯は足さない。
  (h) 形 1 / 3 / 5 の不変: §15 の歯（`pipe_dispatch_gated_pass_`）・§5 の歯（`pipe_dispatch_driver_`）・§13 の歯（`pipe_dispatch_waiting_gate_`）は 1 字も変えずに緑のまま（行の verify が撃つ）。
- 歯（AC47 の突き合わせ・in-file は `crates/scribe2/src/pipe/regate.rs` の `mod tests`・接頭辞 `pipe_regate_forms_`・base に 0 件）。[pipeline.md](./pipeline.md) §49 の行 ar の歯（in-file 3 本・e2e 1 本）が既に測るもの: 札の無い形が通って記帳 1 件・段 `Implemented`・逐語をそのまま（in-file と e2e）／再び `Gated` を挟むと 1 回通る（in-file）／札の無い形で worktree・HEAD・判定の file が変わらない（e2e が 3 つとも・in-file が判定と worktree）／受付の pure な述語が 6 形を断り札の死んだ形を通す（in-file）。不足は 4 点で、札の死んだ形の不変 3 記録は上の (b) が、自動の regate 0/K は上の (f) が測る。残りの 2 点を in-file に足す:
  (i) 形 7 の母集団 8 形: 口の本体（`regate` の関数）に、札の無い形・札の所有者が死んでいる形（通る 2 形）と、段が `Gated` でない・判定が FAIL でない・札の所有者が生きている・逐語が空・判定を読めない・札を読めない（断る 6 形）を 1 形ずつ別の置き場で渡す。通る 2 形は rc 0 で記帳がちょうど 1 件、断る 6 形は rc 1 で記帳 0 件・理由の行が形ごとに違う語を持つ（理由の語は入力の逐語に無い字面で測り、出所を弁別する）。assert に母集団 8 を出す。口の本体が札を読まずに固定の値を渡す変異・判定を読まずに固定の値を渡す変異・理由の行を 1 つに畳む変異・札の死んだ形を断る変異で赤になる。
  (j) 形 7 の直後の 2 度目: 通した直後に同じ便へ撃つと rc 1 で、理由の行が段の条件の語を持ち「最新の `Gated` の後に 1 度戻している」の語を持たない（AC47「理由 = 段」）。受付の 4 条件より先に「1 度戻している」の判定を置く変異で赤になる（既存の歯は rc と記帳 0 件だけを測るので、この変異は生き残る）。
  (i)(j) は base の挙動を測る歯で base でも緑になるので、`crates/scribe2/src/pipe/regate.rs` の歯の区間の行頭に retroactive の札（本行の契約 bead の id）を置き、上の変異の proof（変異ごとに落ちる歯の名）を契約 bead の notes に記帳する。e2e の file（(a)〜(f)）には札を置かない（(a)(e) が機能不在の RED で入口を測る）。

## 24. 札と lock の所有者を pid の再利用に釣られず判じる — 本文に起動時刻を添え、読み手は pid が生きていても起動時刻が違えば死んだと判じる（契約表の行 u・§5 の続き・`s2-07l.608`）

やさしく言うと: 器は「札に書かれた pid の process が居るか」で driver の生死を判じる。process の番号は使い回されるので、死んだ driver の番号を別の新しい process が受け取ると、器は死んだ driver を生きていると読み、その便を誰も継がない。札に「番号」だけでなく「その process がいつ起動したか」も書いておけば、番号が同じでも起動時刻が違う相手を別人と判じられる。

- 出所: 台帳 `s2-07l.608`（memo・2026-09-24 に 2 回・CI と local で `pipe_dispatch_gated_pass_dead_ticket_` / `pipe_dispatch_regated_dead_ticket_` が交互に落ちる）。2 つの落ち方: (a) 1 周が `resumed:0` を返す（0.12 秒）＝死んだ札の pid を隣の process が受け取り「生きている」と読んだ。歯の fixture は `true` を起こして抜けた pid を札に書くので、負荷の高い周（隣の worktree の build・歯の並列）に再利用が起きやすい。(b) `gone(&ticket)` の 20 秒の待ちが切れる＝継いだ resume の子（toy の gate → land）が負荷で 20 秒を越える。落ちた歯の残骸に `Failed precheck` の event が在ったが、`Driver` は `Drop` で札を外し `process::exit` の site は無いので、札が残る道は「process が死ぬ」だけ（候補 2 は反証）。
- 現物（verified・main 75829d9）:
  - 札と lock の本文は `crates/scribe2/src/fleet/store.rs` の `acquire_with` が `create_new` の直後に `process::id()` を 10 進 1 行で書く 1 か所。読み手は `lock_owner`（pure・`owner_pid` が 10 進 1 行だけを受ける）で、`probe`（実物は `started_ms`・`/proc/<pid>/stat` の starttime）が `Absent` の周だけ `Dead`、`Started(_)` は値を見ずに `Live`。
  - `Driver`（`crates/scribe2/src/pipe/mod.rs`）は `hold` で `acquire_with(Reclaim::DeadOnly)` を撃ち、`Drop` で本文を pid として読んで自分の pid と等しい札だけ外す。
  - 受付札（gate-cost.md §3.2）と追記の lock も同じ 1 本を使う（C6.3）＝本行で直すと 3 つの面が同時に直る。
  - 歯の fixture `put_dead_ticket`（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`）は抜けた `true` の pid を 1 行で書き、`gone` は 20 秒・50 ms 刻みで不在を待つ。
- 形（1 つずつ歯が測る・行 u の done と 1:1）:
  1. **本文に起動時刻を添える**: `acquire_with` は `<pid> <starttime>`（10 進 2 語・空白 1 つ・末尾改行 1 つ・starttime は `started_ms` と同じ単位の値）を書く。自分の起動時刻を読めない周（`Probe::Unreadable`）は今までどおり pid 1 行を書く（書けないを断りにしない）。
  2. **読み手は 2 語も受ける**: `owner_pid` の後継は本文を「pid 1 語」か「pid + 起動時刻の 2 語」の 2 形で読み、それ以外は `Unreadable`。`lock_owner` は 2 語の周に `probe` が `Started(t)` を返して **`t` が本文の値と違えば `Dead`**（pid の再利用）、等しければ `Live`。1 語の周は今までどおり（`Started(_)` は `Live`）＝古い札との跨版互換（schema は変えない・新しい語が無い本文は旧形）。
  3. **`Driver` の `Drop` は先頭の語で自分を判じる**（2 語の本文でも自分の札を外す・他人の札は落とさない）。
  4. **歯の fixture は再利用に強い死んだ札を書く**: `put_dead_ticket` は抜けた `true` の pid に加えて、その process の起動時刻と一致しない値（例 `1`）を 2 語目に書く（pid が再利用されても `Dead`）。`gone` の待ちは 20 秒から 60 秒へ（継いだ子は toy の gate と land を撃つ・負荷の周の実測は 20 秒超・上限は rules 行にしない＝歯の定数）。
  5. **外形は不変**: 1 周の行（`dispatch=…`）・`dispatch ls` の行・`Ticket` の 4 値・`Owner` の 3 値・`EventKind` の列・受付札と追記の lock の断りの字面は 1 字も変わらない。
- 触らない: `Reclaim` の 2 値と回収の順（§5 の「原子的でない」は本行の外）・`started_ms` の読み方（`btime` → `<pid>/stat`）・`LockPolicy`・`Driver::hold` の排他。
- 歯（置き場は既存の file）:
  - `crates/scribe2/src/fleet/store.rs` の in-file の歯（`fleet_store_owner_` 接頭辞）: 2 語の本文で `Started(t)` の `t` が本文と違えば `Dead`・等しければ `Live`・`Absent` は `Dead`・1 語の本文は `Started(_)` で `Live`（base では 2 語の本文が `Unreadable` ＝ RED）／ 3 語や非数の本文は `Unreadable` ／ `acquire_with` が書いた本文が 2 語で 1 語目が自分の pid・2 語目が `started_ms(自分)` の値（fixture の proc root を注入・base では 1 語 ＝ RED）。
  - `crates/scribe2/src/pipe/mod.rs` の in-file の歯（`pipe_driver_ticket_` 接頭辞・既存の `Driver` の歯の隣）: 2 語の本文の自分の札を `Drop` が外し、1 語目が他人の pid の札は外さない（base では 2 語の自分の札を外せない ＝ RED）。
  - `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`: `put_dead_ticket` と `gone` の形 4 の差し替えだけ（既存の歯は約束を変えずに緑のまま・新しい歯は足さない）。
- 却下: 歯の側だけで済ませる（fixture に 2 語を書いても読み手が 1 語しか受けなければ `Unreadable`＝「読めない札は触らない」で同じく `resumed:0`）／`gone` を無限に待つ（負荷で止まった子を歯が隠す）／pid の namespace や `/proc` の inode で弁別する（`started_ms` の 1 本を増やす・C6.3）／lock file の本文を JSON にする（1 行 2 語で足りる・器の唯一の lock 実装を重くしない）。
- 後続: §5 の「死んだ所有者の札の回収は原子的でない」は本行の外（同じ面だが別の穴）。

## 25. 追随の起こし直しの後に driver が抜けた便を列が起こし直す（契約表の行 v・§23 の隣の 4 枝目・`s2-07l.633`）

やさしく言うと: gate を通った便が着地の順番待ちで main に追随し、衝突を runner が解いて段が Implemented に戻った後、flag の無い driver（`pipe resume`・1 段だけ進める形）は設計どおりそこで抜ける。その便を拾う枝が dispatcher に無く、誰も gate を撃たないまま列の鍵も持ち続ける（2026-09-25 の実測: 2 時間 40 分・後続は 90 分の待ちを丸ごと食った）。regate で戻された便を拾う §23 の枝と同じ型の穴なので、同じ絞りで 4 枝目を足す。

- 出所: 台帳 `s2-07l.633`（便 s2-07l.614-20260925T040409Z の実測）・裁定の記録は ADR-0068（要件 FR68 / AC38 の 5 種目・SRS v0.24）。
- 現物（verified・main dae3b91）:
  - 起こし直しの候補は `crates/scribe2/src/pipe/dispatch.rs` の `revivals`（3 枝: `driver_is_dead`〔札 Dead〕／`passed_gate`〔Gated ∧ PASS ∧ 札 Absent | Dead〕／`regated`〔Implemented ∧ 最新の Gated より後ろに `regate:` の記帳 ∧ 札 Absent | Dead〕・後の 2 枝は `gated` の周だけ）。起こす argv は `resume --run <id> … --drive`（1 本が組む・`DRIVE` を末尾に足す）。
  - 追随の記帳は `crates/scribe2/src/pipe/follow_step.rs`（`RunStage` 段 Implemented・`detail` = `rebase:<base>..<main>`）と `crates/scribe2/src/pipe/follow.rs`（衝突の起こし直し・`rebase-conflict:` / `rebase-stale-rows:`・読み手 `is_conflict` は接頭辞 2 語）。
  - regate の記帳の読み手は `crates/scribe2/src/pipe/regate.rs` の `regated_since_gate`（最新の Gated より後ろに `regate:` が在るか・pure）。
  - 列の鍵は最初の Gated の ts（pipeline.md §36・札 Dead の便だけ `skipped-dead` で外す）。
- 形（行 v・1 つずつ歯が測る・done と 1:1・判定の順は §23 と同じ: 段の生死 → 待ちの段か → 既存の 3 枝 → 4 枝目）:
  1. **`revivals` に 4 枝目**: `gated` の周に「段が Implemented ∧ 最新の Gated の `RunStage` より後ろに追随の記帳（`detail` が `rebase:` か `is_conflict` の 2 語で始まる `RunStage`）が在る ∧ その後ろに Gated / Landed の `RunStage` が無い ∧ 札が Absent か Dead」でも候補にする。読み手は `regated_since_gate` と同じ形の pure な 1 本（`regate.rs` の隣に置き、event の列と便 id を受ける・接頭辞の弁別は `is_conflict` と `rebase:` の literal を写す）。既存の 3 枝と待ちの段の枝は 1 字も変えない。
  2. **起こす argv は既存の 1 本**（`resume … --drive`）＝flag つきで起こし直すので、次は gate → 列 → 着地まで同じ driver が進む。
  3. **起こさない便**: 追随の記帳の後に Gated を経た便（gate をもう 1 度通った＝最新の Gated が追随より後ろ）／追随の記帳を持たない Implemented（§5「札の無い便は触らない」のまま）／札 Live / Unreadable。
  4. **`gated` の絞り**は §23 形 3 と同じ（driver の終端の周は段を前へ進めた周だけ・手動の 1 周と印の直後は絞らない）。**二重にしない**は §23 形 4 と同じ（便ごとに 1 判定）。
  5. **型も字面も足さない**（§23 形 6 と同じ）: `Revive` に理由の field を足さない・`WaitReason` / `Stage` / `EventKind` の変種は増えない・1 周の行と `dispatch ls` の行は不変。
- 触らない: 列の鍵と `turn_skipping`（§36・Dead だけ外す規則はそのまま＝4 枝目が次の周で driver を戻せば鍵は着地で自然に離れる・memo の候補 2 は要らない〔C17 の 1 段目〕）・flag の無い `pipe resume`（手動の 1 段進めは席の道具として残す）・追随と衝突の起こし直しの記帳の字面。
- 却下: `turn_skipping` を「Absent ∧ Implemented」へ広げる（4 枝目で足りる・列の判定の読みを増やす）／flag の無い resume を禁じる（席の道具）／追随の起こし直しの側で driver を閉じない（flag 無しの driver は 1 段の契約・設計どおり）。
- 歯（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` に `pipe_dispatch_revive_followed_` 接頭辞・§23 の regated の歯と同じ fixture〔置き場の event log を手で書く・偽 runner〕）: (a) Gated PASS → 追随 `rebase:` の Implemented ∧ 札 Absent → `gated` の周の 1 周で `resumed:1`・argv の末尾が `--drive`（base では `resumed:0` ＝ RED）(b) 追随が `rebase-conflict:` でも同じ (c) 追随の後にもう 1 度 Gated が在る → `resumed:0`（新しい fixture。既存の歯 `pipe_dispatch_regated_then_gated_and_followed_run_is_left_alone`〔§23 の regate → PASS の gate → follow〕は追随が最新の Gated より後ろなので 4 枝目で `resumed:1` に変わる＝名を「regate の後の追随でも起こす」に改めて `resumed:1` を測る歯に書き換える）(d) 追随の記帳の無い Implemented ∧ 札 Absent → `resumed:0`（不変）(e) `gated` でない周（手動の 1 周）でも `resumed:1`。lib は `crates/scribe2/src/pipe/regate.rs` の隣の pure な読み手に `followed_since_gate_` 接頭辞（追随あり / なし / 追随の後の Gated の 3 本）。
- 実装の注（行 v・land 時の現物）:
  - 読み手は `regate.rs` の `followed_since_gate`（`regated_since_gate` と同じ形・`detail` の頭は `rebase:` の literal と `follow.rs` の `is_conflict` の 1 本）。「最新の Gated より後ろ ∧ その後ろに Gated / Landed が無い」は、最新の Gated か Landed の `RunStage` より後ろに追随の記帳が在るかの 1 回の走査で読む（2 つの条件は同値）。
  - (a) は `Gated` を先に持つ便 B の `--drive` の resume（`Gated` → `Landed`＝前進・`drive=settled`）の終端の 1 周で測る。B を先に `Gated` へ着けるのは列の鍵（§36 の最初の `Gated` の ts）を B に持たせるため。flag の無い driver の終端の 1 周は行を出さない（効果だけ）ので `resumed:1` の字面を測れない。
  - (b)(c) の fixture は衝突の記帳の後ろに runner の完了の記帳（`detail` の無い `Implemented`）を置く。最新の `RunStage` が衝突の記帳のままだと resume は runner を起こし直す側（pipeline-conflict.md §3）へ分かれ、gate へ進まない。実測の便（衝突を runner が解いた後に抜けた便）もこの形である。
  - `--drive` で起こした証拠は `Landed` まで進むこと（flag の無い resume は 1 段で止まる）。base（4 枝目なし）で (a)(b)(e) と書き換えた歯は `resumed:0` で赤、(c)(d) は緑。(c) は読み手の「最新の Gated より後ろ」を外す変異で赤になる。

## 26. idle の知らせに並列の実測を足す — live の本数・0 本の分数・重なりで待つ本数と file 名を 1 関数で作り、heartbeat と共用する（契約表の行 w・裁定 user 2026-09-27T13:32Z）

やさしく言うと: 「いま何本走っていて、何本が重なりで待っていて、何分 0 本か」を器が数えて知らせに書く。並列を席の記憶に頼らない。

- 何が起きているか（実測 2026-09-27・verified）: 列の知らせ（§19 形 3 (b)）は `ready=<列の本数> launched=0 reason=<先頭の理由>` だけで、live の本数・重なりで待つ本数と file・0 本が続いた長さを言わない。置き場の event log（2026-09-17T22:35Z〜09-27T11:39Z・便 746 本・便ごとに最初と最後の event の区間を live と近似・最後の段は Reviewed 289 / Landed 271 / Gated 91 / Stopped 88 / Failed 7 で全部が終端の形）を畳むと、live の区間 225・0 本の区間 224。0 本の区間は 5.0〜8.8 分に 49 本が固まり（着地の後の CI と close を待つ直列の隙間）、8.8〜10.0 分は 0 本、10 分以上は 48 本で合計 6784 分＝0 本の総 7216 分の 94%。長い 0 本は誰の目にも数で出ていない。
- 現物（main 8f6072d・verified）: idle の 1 行は `crates/scribe2/src/pipe/notify.rs` の `idle_line`（`Turn` の `candidates` の長さと先頭の `reason` の `render` だけを読む）。列の待ちの理由 `WaitReason::Overlap` は `with` と `files`（交差した file の**本数**）しか持たず、`crates/scribe2/src/pipe/dispatch/candidates.rs` の `blocker` の 2 か所が交差の file の列（`overlaps` / `crossings` の返り値）を本数に畳んで捨てる。live の判定は `crates/scribe2/src/pipe/cli/state.rs` の `live`（受付・列・終端の軸が共用する 1 本）。便の最後の時刻は replay の `Run` の `updated`、字面の秒への読みは `crates/scribe2/src/fleet/wait.rs` の `epoch_of`。
- 事実の関数が読む write-set の外の 3 つ（実測 main cbb3b0a・verified・どれも可視性を変えず file も編集しない）: `live` は `crates/scribe2/src/pipe/cli/state.rs` の 30 行目の pub(in crate::pipe) の fn で、`crates/scribe2/src/pipe/cli.rs` の 37 行目が pub(super) の use で cli へ再輸出する。dispatch の子 module は `crates/scribe2/src/pipe/dispatch/candidates.rs` の 15 行目が既に super::super::cli から `live` を引いている（行 w の兄弟 module も同じ path で引ける）。`epoch_of` は `crates/scribe2/src/fleet/wait.rs` の 363 行目の pub fn で、`crate::fleet` が pub use で再輸出する。`Run` は `crates/scribe2/src/fleet/replay.rs` の 18 行目の pub struct で欄 updated は pub、`crate::fleet` が pub use で再輸出する。 便の列を組む replay（同 file の 166 行目の pub fn）も `crate::fleet` の pub use で見える。兄弟 module の親の親の可視性: `crates/scribe2/src/pipe/mod.rs` の 20 行目が pub mod で dispatch を宣言し、pipe の外の `crates/scribe2/src/hook/group.rs` の 810 行目が既に crate::pipe::dispatch の path で dispatch の fn を呼んでいる。よって行 w が `crates/scribe2/src/pipe/dispatch.rs` に pub(crate) の module 宣言を 1 行足せば、事実の関数は crate::seat::tick から届き、pipe/mod.rs は編集しない（行 w の write-set の外のまま）。
- 形（番号は done と 1:1）:
  1. **事実を作る 1 関数を兄弟 module に置く**（行 w の write-set の `+` の file・`pipe/dispatch.rs` は hub なので宣言の 1 行だけ）: module の宣言と事実の関数（形 2 の字面の関数も）は `pub(crate)` で、heartbeat（[seat-heartbeat.md](./seat-heartbeat.md) §16 行 t）が `crate::seat::tick` から呼ぶ（`pipe/dispatch.rs` の子 module は今どれも private＝`mod candidates;` / `mod group;`・t の write-set は `pipe/dispatch.rs` を持たないので、見える形は本行が作る）。入力は置き場・列の 1 周の結果（無い呼び手は無し）・今の UTC 秒。出すのは 3 つ: (a) live の本数＝置き場の全便に `live` を撃ち `Some(true)` を数える（1 本でも `None` なら測れない）(b) 0 本の分数＝live が 0 の周だけ、全便の `updated` の最大から今までの分（切り捨て）。live が 1 本以上の周・便が 1 本も無い周は値なし、`updated` の 1 つでも `epoch_of` で読めない周と (a) が測れない周は測れない (c) 重なりで待つ本数＝列の結果の候補のうち理由が `Overlap` の本数と、その交差の file 名（path の最後の 1 要素・dir 項目は末尾の / を残す・重複を除いて字の順）。列の結果の無い呼び手には (c) が無い。live の判定を 2 本目に書かない（C2）。(c) が数えるのは今在る `Overlap`（live な便との交差）だけで、台帳の閉じていない行どうしの重なりで待つ理由は本 PR の外の後の行が足す（数えるかはその行が決める）。
  2. **字面も同じ 1 関数**: 知らせの末尾に足す字面は ` live=<n> idle=<m>m held=<k>:<名,名>` の順で、測れない値は `?`、値なしは `-`、重なり 0 は `held=0`（コロンなし）、(c) の無い呼び手は `held=` を出さない（出す key は出所で決まり、測れないと融合しない・C10）。
  3. **待ちの理由に交差の file の列を持たせる**: `WaitReason::Overlap` の `files` を本数から交差した契約側の file の列に変え、`blocker` の 2 つの構築点は捨てていた列をそのまま渡す。`render` は列の長さを書く＝`reason=overlap:<相手>/<本数>` と `dispatch ls` の字面は 1 字も変わらない。
  4. **idle の 1 行は既存の key と順を変えず末尾に足す**: `scribe2 pipe: idle ready=<n> launched=0 reason=<r>` の後ろに形 2 の字面。`pipe/cli.rs` の `notices` が同じ周の `Turn` と運転手の置き場で形 1 を 1 回撃って渡す。終端の 1 行（§19 形 3 (a)）・送る契機・宛先・送達は不変。
- 触らない: `ready=` の意味（列の本数・`dispatch ls` の件数の行の `ready=` とは別義のまま）・`launched=` / `reason=` の字面・`dispatch=` の行・`WaitReason` の他の variant と `as_str` の列・event の kind（事実は記帳しない・読むだけ）・列の判定と起こす契機（時計の契機を足さない・§5）。
- 歯（接頭辞 `pipe_notify_facts_`・`crates/scribe2-boundary/tests/e2e/notify.rs`・`pipe/` の外＝§19 形 6・既存の `idle_round` の型）: (a) 候補の write-set が live な便 1 本の write-set と 1 file で交差する周の終端 → idle の行が ` live=1 idle=- held=1:<その file 名>` で終わる。(b) live な便が無く候補が hold の周 → ` live=0 idle=0m held=0` で終わる。(c) 既存の `ready=1 launched=0 reason=hold` の字面は同じ行に在る。base では末尾の字面が無いので (a) (b) が RED（機能不在）。grep の件数（2026-09-27）: `fn pipe_notify_facts_` は crates/ に 0 件。
- 限界: live の区間の近似（最後の event の時刻）は、終端の後に同じ便へ記帳が足された周（retire 等）に 0 本の分数を短く読む。終端の周に送る行なので idle の分数はほぼ 0m で、分数が効くのは時計で撃つ heartbeat の側（[seat-heartbeat.md](./seat-heartbeat.md) §16）。file 名は最後の 1 要素なので、同名の別 file（mod.rs 等）は区別しない。
- 却下: 列の知らせの `ready=` の意味を「起こせる本数」に替える（既存の key の意味が変わり読み手が壊れる）／交差の file を知らせの側で数え直す（交差の判定が 2 本になる・C2）／列の 1 周の事実を置き場の file に書き tick が読む（新しい記録と 2 本目の読み手・起票の後に dispatch を撃たない周に古い値を出す）。

## 27. 依存を待つ行に受付の機械の審査を先に撃つ — 未着地の依存の宣言か実物で base を予想し、確定と暫定を分けて置き場に残し、確定の誤りを根で束ねて直しへ導く（契約表の行 x / y / aa・裁定 user 2026-09-27T13:32Z / 14:02Z）

やさしく言うと: 依存の着地を待つ契約は、待っている間に受付の審査を 1 度も受けない。器が「依存が着地したらこうなる」木を予想してその上で受付と同じ審査を撃ち、依存が着地しても消えない誤り（確定）だけを束にして直させる。予想の結果で便を起こしも止めもしない（起こす時の受付は今どおり実物の main で撃つ）。

- 何が起きているか（実測 2026-09-27・verified）:
  - 列の 1 件の解き（`entry_of`）は依存 → 印 → 設計 pointer → 契約の生成 → 列外の鍵の順で、依存待ちを最初に返す。依存を待つ bead は契約の生成（`generated`＝契約表の検査の 1 行）も受付の判定（`judge`＝`pipe preflight` と同じ 1 本）も受けない。
  - 非公開の隣の project の依存待ち 10 本に今の main で `pipe preflight` を撃つと ok 3・refused 6。6 本とも write-set-item-unresolved で、未着地の依存（live の 1 本）が `+` で作る file を素の path で持つ。依存の宣言の `+` の file を空の file として置いた木（宣言の予想）で撃ち直すと、6 本と同じ依存を推移的に待つ 1 本の計 7 本が全部 ok、依存の便の実物の木（Gated・足した 9 file と変えた 2 file）で撃っても 7 本とも ok。1 本 0.02 秒（process 全体）。本 repo の行は置き場なしの `pipe preflight` で 1 本 0.05〜0.15 秒（材料の読みを含む・6 行・起草の時）、0.21〜0.32 秒（10 行・2026-09-28 の直しの時・host の負荷の違い）。
  - 素朴に待ち行へ preflight を撃つと依存の着地で消える偽の断りが出て、しかも契約表の段で止まり、閉包・歯の置き場・上限の余地の検査に届かない。契約表の検査の write-set の項目の実在（`write_set_findings`）は tracked だけで解き、宣言済みの新規 file（`declared_files`・§39 の母集団）は名指しにしか効かない。
  - 依存の宣言の `+` を当てにする予想には外れる道が在る: 受付は `+` が base に無いことしか測らず（`WriteSetItem::New` を読むのは宣言の読み手と受付の 2 file だけ）、gate も land も `+` の file が作られたかを測らない。設計 doc の write-set の `+` の項目 232（重複なし 136 path・約束の行の `files` の `+` 4 を除く）のうち main に無いのは 2 path で、1 つは作られた後に別の便が消し（正しい）、もう 1 つ（consumer-sync.md 行 f）は git の全履歴に 1 度も現れない＝宣言した新設 file を作らずに着地した実例（232 中 1）。出所で測る門は [pipeline.md](./pipeline.md) §58（行 ba）。
  - 着地した依存が足した fn 名が待ち行の verify の filter 語に当たり、受付で teeth-outside-write-set になった実例が非公開の隣の project に 1 つ在る（orchestrator の実測）＝宣言だけの予想では見えず、依存の実物の木で測り直すと見える食い違いである。
- 現物（main 8f6072d・verified）: `crates/scribe2/src/pipe/dispatch/candidates.rs` の `entry_of`（:43・依存は :52 で返り、設計 pointer は :66）・`is_input`（:33）・`is_blocking`（:212）・`pointer_of`（:217）／`crates/scribe2/src/pipe/cli/intake.rs` の `generated`（:363・契約表の検査は :377）・`judge`（:626）・`Materials`（:241・1 周に 1 回読む tracked と `.rs` / `.snap` の本文）・`Denial`（:556・断りの名と描いた行だけを持ち、型の断りを捨てる・構築は `denied` と `refuse` の 2 か所）／`crates/scribe2/src/pipe/table/check.rs` の `write_set_findings`（:242）／`crates/scribe2/src/pipe/dispatch.rs` の `turn`（:499）・`fire`（:580）・`render`（:863）・`spawn_self`（:390・自分を process group を分けて起こす 1 本）／`crates/scribe2/src/pipe/cli.rs` の `dispatch ls` の口（:478）／`crates/scribe2/src/pipe/review.rs` の `REVIEW_DIR`（:65・審査の材料の dir）・`materials`（:307）・`decide`（:473・lens を撃って判定を読む）／`crates/scribe2/src/fleet/store.rs` の `lock_owner`（:157・pid と起動時刻の 2 語で所有者の生死を判じる）。
- 行 x が呼ぶ列の読み手の可視性（main be51991・verified・便 `s2-07l.705` の 1 回目の lens の section-material-missing の根）: 新しい子 module `crates/scribe2/src/pipe/dispatch/precheck.rs` は `crates/scribe2/src/pipe/dispatch.rs` の `mod precheck;` で宣言する兄弟で、`mod candidates;`（:33・私有）には親 module を通して届く（私有の子 module は親と兄弟から見える）。`crates/scribe2/src/pipe/dispatch/candidates.rs` の `is_input`（:33）と `entry_of`（:43）は `pub(super)` で今のまま呼べるが、`is_blocking`（:212）と `pointer_of`（:217）は私有の `fn` なので、行 x は 2 つの宣言の頭を `pub(super) fn` にする（1 語ずつ・本文と doc 行は不変・母集団と到達の 1 関数が親 module を通してこの 2 つを呼ぶ＝列の読みを 2 本目に書かない・C2）。そのため行 x の write-set に `candidates.rs` を入れる。`generated` / `judge` / `Material` / `Materials` / `Denial` は `crates/scribe2/src/pipe/cli.rs` の `pub(in crate::pipe) use intake::{…}`（:32）で届き、`BLOCKS` / `CLOSED` / `OPEN` / `MEMO_LABEL` / `DESIGN_KEY` は `dispatch.rs` の私有の `const`（:44〜:56）で子 module から見える。
- 形（行 x・番号は done と 1:1）:
  1. **閉じていない契約の行の母集団と blocks の到達を 1 関数で持ち、その上で予想の base を組む**: 母集団と到達の 1 関数（行 x の write-set の `+` の file に置き、列の他の子 module から呼べる `pub(super)`）は、同じ周に `turn` が読んだ台帳の全件と置き場の run の列から、(a) 閉じていない契約の行（status が closed でない ∧ memo の label が無い ∧ acceptance の設計 pointer が列と同じ読み `pointer_of` で解ける bead・live な便の在る bead はその run dir の契約の写しの write-set を持つ・契約の弁別を 2 本目に書かない）と、(b) blocks の到達（全件の `blocks` の依存を推移でたどる・`parent-child` は数えない・closed の bead で止まる・1 度訪ねた bead で止まり循環で回らない）を返す。live でない行の write-set は呼び手が自分の材料で受付と同じ `generated` を撃って決める（材料を引数に取る＝本行は予想の base で撃ち、台帳の行どうしの重なりを列で測る後の行〔本 PR の外〕は実物の main で同じ 1 本を呼ぶ・C2）。待ち行 B（列の理由が依存・設計 pointer を持つ）の open な祖先＝(b) で B から届く (a) の行を依存の順に並べ、祖先 A ごとに write-set を 1 つ決める: A の便が live なら (a) の写し（受付が凍結した値）、live でなければ `generated` を A 自身の予想の base で撃った契約。宣言の予想＝base の tracked に祖先の `+` の file を足し `~` の file を除いたもの（本文は持たない）。**A の便が Gated PASS なら実物で重ねる**: A の worktree の base から HEAD までの差分（name-status・rename は対）で tracked を足し引きし、足した / 変えた file の本文を A の木から読んで材料の本文を置き換える。live な便どうしの write-set は受付の排他で交わらないので、Gated PASS の依存を複数重ねても同じ file を 2 本が書かない（merge-tree は要らない）。**動く file**＝宣言で重ねた祖先の write-set の全項目（接頭辞を剥がし dir は展開）で、実物で重ねた祖先の file は入れない。祖先の write-set が決まらない周（生成が断る・写しを読めない・差分を読めない）は B を測れない（forecast）とする（推さない・C10）。
  2. **審査は受付の 1 本のまま**: 予想の base は `Materials` の口 1 つ（tracked と本文を差し替えた写しを組む・判定の関数は不変）で作り、`generated` → `judge`（置き場なし・lock の前の読みなし＝列が起こせる候補に撃つ `blocker` と同じ形）を撃つ。置き場の要る判定（交差・同時本数・重複 run・同型の停止・焼き直しの門）と base の木の実走（入口の検証）は撃たない（待ちの間は測る意味が無いか重い）。
  3. **確定と暫定は 1 関数で分ける**: `Denial` に型の断りの列（`Refuse` の値・契約表の段は finding ごと）を持たせ、`Refuse` と `TableError` に証拠の在り処を返す網羅の match を 1 つずつ足す（在り処は閉じた型の 4 値: 行の字と規則だけで決まる／名指した file の列／本文の読み手が解く名／置き場と host）。弁別の 1 関数は在り処と動く file から 3 値を返す: 行 → 確定、file の列 → 動く file と交わらなければ確定・交われば暫定、名 → 動く file に本文の読み手が読む file が 1 本も無ければ確定・在れば暫定、置き場と host → 測れない。型を持たない断り（宣言・rules 行・size・置き場の名）も測れない（名を残す）。
  4. **撃つのは起こす側の周だけ・鍵が動いた行だけ**: `fire` の起こし終えた後（起こす便を遅らせない）に、待ち行ごとの鍵＝base の HEAD の sha・rules の写しの blob の sha（写しが無ければ器の版）・祖先ごとの id と状態の語（declared／run:<便>／tree:<便>@<HEAD の sha>）の字を組み、置き場の結果の鍵と字が同じ行は撃たない（§2 の列外の鍵と同じ型）。依存が Gated PASS になった周と着地した周は鍵が動くので、依存の実物で自動に測り直される。台帳と材料は同じ周に `turn` が読んだ 1 回を借りる（2 度読まない）。見る側（`dispatch ls`）は撃たず、形 1 の母集団の 1 関数も呼ばない（`turn` は呼ばない＝ls の費用は不変）。
  5. **結果は置き場の file（台帳に書かない・C15）**: 置き場の pipe の下の事前審査の dir に bead ごとの 1 file（鍵・結果の語〔clean／firm:<k>,provisional:<j>／unmeasured:<名>〕・finding ごとの 1 行〔確定か暫定・断りの名・在り処・理由の 1 行・前の結果に無かった確定の印 new〕）。書きは一時 file → rename。読めない・鍵の字が違う file は無いと同じ（撃ち直す・跨版の約束を持たない cache）。列の依存待ちに居ない bead の file は同じ周に外す。event kind は足さない。
  6. **予想は通行証にしない**: 事前審査の結果は `WaitReason`・起こす判定・受付のどれも読まない。依存が閉じた後の起動の受付は今どおり実物の main で `judge` を撃つ（予想で clean の行も実物で断られうる）。
  7. **見る口**: `dispatch ls` は依存待ちの候補ごとに `[DISPATCH-PRECHECK] bead=<id> result=<結果の語か -> base=<current|moved>` の 1 行を件数の行の前に足す（既存の `[DISPATCH]` と `[DISPATCH-COUNT]` の行の字は 1 字も変えない・結果の file を読むだけで撃たない）。
- 費用（行 x・実測と推定を分ける）:
  - 実測（2026-09-28・本 repo）: 置き場なしの `pipe preflight`（process 全体・材料の読みを含む）は 10 行で 1 本 0.21〜0.32 秒。`pipe dispatch ls`（台帳の読み 1 回・候補 0 本）は 0.95 秒で、その大半は台帳の子 process。本行は `dispatch ls` から撃たれないので、ls の費用は変わらない。
  - 推定（実測の上限 0.32 秒で上から押さえた値・未実測）: 1 周の費用 ≈ 鍵の動いた待ち行の本数 W × 0.32 秒 ＋ 祖先の write-set の生成（母集団の 1 関数が bead ごとに 1 周 1 回）。材料の読みは周に 1 回で W 本が借りるので、実際はこれを下回る。鍵は base の HEAD を含むので、着地の直後の周は全部の待ち行の鍵が動く＝最も重い周。待ち行 20 本の周で約 6 秒以下、同時に閉じていない契約の過去の最大 74 本（本 repo の台帳の履歴・起草の時の実測・再測していない）が全部待つ周で約 24 秒以下。便の起動は遅れない（起こし終えた後に撃つ）が、終端の周の知らせ（`pipe/cli.rs` の `notices`）は `fire` が返った後に送られるので、その分だけ遅れる。1 周の本数に上限を持たせるなら値は rules 行（user の裁定）で、本行は持たない。
- 行 y と行 aa が触る先の現物（行 x の着地後・main d2cf7d6・verified）と、それを受けた write-set の足し（便 s2-07l.717 の審査 INCONCLUSIVE と便 s2-07l.718 の審査 FAIL の根）:
  - 事前審査の module は `crates/scribe2/src/pipe/dispatch.rs` の :42 が私有の子として宣言する（`mod precheck;`）。外へ見せる名は `Row` / `Population` / `population` / `round` / `lines` の 5 つで、どれも `pub(super)` である。`round` は起こす側の周（:654）が、`lines` は `dispatch ls`（:905）が呼ぶ。次の名は私有である: 結果の file の読み手 `read`、その戻りの `Kept`（鍵・結果の語・確定の finding の名と在り処）、書き手 `write`、置き場の dir を返す `dir_of`、dir の名 `PRECHECK_DIR`、行の頭の語 `LINE`、finding の型 `Finding`、結果の語を作る `result_word`。結果の file は 1 行目が `key=`、2 行目が `result=` で、3 行目から finding ごとに 1 行（確かさ・名・在り処・new の印・理由）が並ぶ。
  - 並列の実測の module は同じ file の :39 が `pub(crate) mod facts;` で宣言し、`facts` と `line` は `pub(crate)` である。`line` は知らせの末尾の字面を作る 1 本で、次の 2 つが同じ 1 本を呼ぶ。
    - idle の知らせ（`crates/scribe2/src/pipe/notify.rs` の :72）
    - heartbeat の合図（`crates/scribe2/src/seat/tick.rs` の :870・その後ろに行 u の ` alarm=`）
  - 段の上げの語は tick.rs の `idle_alarm`（:409）が `Facts` から決める。`Facts` を作るのは facts.rs の中の 3 か所（:63・:70・:82）だけである。
  - 席への送達は notify.rs の `send`（:91・`pub(super)`＝pipe の子孫から呼べる）で、idle の知らせは `crates/scribe2/src/pipe/cli.rs` の同じ周がこの 1 本で送る。
  - 束の file に写す節の本文と行の TOML（形 1）の読み手は、どちらも既に在る 1 本を呼ぶ（便 s2-07l.717 の 2 回目の審査 INCONCLUSIVE の根）。
    - 節の本文: `crates/scribe2/src/pipe/review.rs` の `design_material`（:387・`pub(in crate::pipe)`・引数は repo と設計 pointer の字）。審査の材料の dir に置かれる設計の file と同じ形（1 行目が pointer と節の番号、2 行目から本文）を返す。review は pipe の :30 の `pub mod review;` で、束の module（pipe の子孫）から呼べる。節の読み（`section_text`・私有）と契約表の検査の節の読み（`section_lines`・table/check.rs の私有）は呼ばない（読み手を 2 本にしない・C2）。
    - 行の TOML: `find_row`（`crates/scribe2/src/pipe/table/parse.rs` の :358・`pub`・table.rs の :39 の再輸出で table の直下に出る）が返す行の `line`（行の頭の `[[contract]]` の行番号・contracts check の行番号と同じ）から、次の `[[contract]]` か区間の終わりの前までの字を、そのまま写す（TOML を組み直さない）。
    - どちらも行 y の write-set の外の file を 1 語も変えずに届く。
  - ⇒ 行 y の write-set に、事前審査の file と実測の file（`crates/scribe2/src/pipe/dispatch/facts.rs`）の 2 つを足す。
    - 事前審査の file: `read` と `Kept` と `dir_of` を `pub(super)` に上げ、`Kept` に確定の finding の理由を持たせる。`round` の終わりには、束を作る 1 関数を呼ぶ 1 行を足す。
    - 束の file の読み書きと `[DISPATCH-BUNDLE]` の行は、行 y の `+` の file に置く。結果の file の読み手は `read` の 1 本のままにする（2 本にしない）。
    - ` precheck=` の字面は、`Facts` の新しい欄（`pub(crate)`）と `line` の 1 か所で足す。idle の知らせと heartbeat の末尾が同じ 1 本から出て、heartbeat では ` alarm=` の前に並ぶ（形 3）。段の上げは `idle_alarm` がこの欄を読む（形 4）。
  - ⇒ 行 aa の write-set にも事前審査の file を足す。先撃ちの判定を確定の finding として結果に載せるには `Finding` と `write` と `result_word` が要り、行末の ` prelens=unset` には `lines` が要るためである。
  - 行 x の e2e の歯 `pipe_dispatch_precheck_` の (e)（`crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs` の :412）は、`dispatch ls` の `[DISPATCH-PRECHECK]` の行を字面の全体で比べる。その `--rules` は e2e の手組みの写し（`crates/scribe2-boundary/tests/e2e/pipe.rs` の `write_rules`）で、行 aa が足す rules 行を持たない。` prelens=unset` を `--lens` の有無によらず付けると、この期待の行の末尾に ` prelens=unset` が足される。そのため terminal.rs を行 aa の write-set に入れる（期待の 1 行だけを直す・便 s2-07l.718 の 1 周目の gate FAIL の根＝write-set に無いので実装が `--lens` の周に狭めた）。
  - 審査の材料の組み手（行 aa が予想の base で撃つ・便 s2-07l.718 の審査 INCONCLUSIVE の根）は `crates/scribe2/src/pipe/review.rs` の私有の `materials`（:318）と `keep`（:454）で、`Review` の入口（`review`・:294）からだけ撃たれる。`materials` は review の子の `base_text`（`crates/scribe2/src/pipe/review/base.rs` の :46・`pub(super)`）と `outside_text`（`crates/scribe2/src/pipe/review/outside.rs` の :97・`pub(super)`）を repo の path で呼ぶ。どちらも tracked の一覧を table の `tracked_files`（`git ls-files -z`）で、本文を table の `read`（作業の木の file）で読む。組み手は repo の path だけを読むので、予想の base は overlay のままでは渡せず、木に実体化して渡す（形 aa 1）。review.rs は行 aa の write-set に在る。
- 形（行 y・番号は done と 1:1）:
  1. **直しの束**: 周の終わりに確定の finding を根（断りの名と在り処の file か名）で束ね、束ごとに 1 file（束の id・根・当たった行の pointer と bead の列・各行の TOML の写し・節の本文・findings・測り直しの 1 行〔各行の `pipe preflight` の argv〕・初めて見た周の時刻）を事前審査の dir に置く。10 本が同じ根なら 1 束。束の id は根の字から決まる（同じ根は周をまたいで同じ束）。確定が消えた束の file は外す。
  2. **orchestrator へは周ごとに 1 通**: 束の集合が前の周から変わった周だけ、§19 と同じ宛先へ同じ送達の 1 関数で `scribe2 pipe: precheck bundles=<n> rows=<m>` の後ろに束ごとの ` <束の id>=<束の file の path>` を並べた 1 行を送る（載せるのは束の一覧と在り処だけで、席への作法の散文は載せない・N2）。束が 0 本になった周も集合が変わった周に数え、`scribe2 pipe: precheck bundles=0 rows=0` の 1 行を 1 回送る。`dispatch ls` は束ごとに `[DISPATCH-BUNDLE] id=<束> rows=<m> root=<名> file=<path>` の行を出す。
  3. **idle の知らせと heartbeat の末尾**: 行 w / t の事実の字面の後ろ（heartbeat では行 u の ` alarm=` の前）に ` precheck=<k>/<n>:<b>`（k＝確定を持つ行・n＝結果を持つ行・b＝束の数）を足す。置き場に事前審査の dir が無い周は出さない（測っていないを 0 に畳まない）。heartbeat は台帳を読まず置き場の file だけを読む（直近の周の値）。
  4. **段の上げ**: rules 行 seat.precheck_alarm_s（kind SeatPrecheckAlarmS・Int・秒・値 900・裁定 id は user 2026-09-27T17:33Z 項 2-1）を manifest の seat.idle_alarm_s の直後、kind を `ALL` の SeatIdleAlarmS の直後に足す（どちらも行 u が足す名＝u の着地の後）。最も古い確定の束の初めて見た時刻から値の秒数を越えた周（今 − 時刻 ≥ 値・どちらも秒）は、行 u の段の上げの 1 本（黙りの門を `seat.tick_stale_s` と値の小さい方にし、梯子を段 0 に留める）で heartbeat の段を上げ、行 u の ` alarm=` の列に語 `precheck` を u の語の後ろに足す。値 0 は上げない。行が無い・読めない周は段を上げず（今の挙動）、` precheck=` を出す周に限り ` alarm=` の列に語 `precheck-unset` を足す（測っていないを黙って「上げない」に畳まない・C10）。事前審査の dir の無い置き場は ` precheck=` も語も出さない＝既存の tick の歯の合図は変わらない。
- 置き場の形（行 y の着地で決めた現物）: 束の file は事前審査の dir の下の `bundle` の dir に束の id（根の字の FNV-1a 64 の 16 桁）の名で置く（事前審査の周が依存待ちに居ない bead の結果の file を外す掃除は file だけを消すので dir は残る）。前に送った束の集合は同じ dir の `.sent` に 1 行で残し、`.` を含む名（この印と一時 file）は束に数えない。印の無い置き場は「前の集合は空」と読む（束の無い置き場に `bundles=0` を送らない）。
- 器が起こす直しの worker（確定のまま長く残る束を器が直させる側）は、その ADR の後の別の行が扱う（本 PR の外）。
- 形（行 aa・裁定 user 2026-09-27T14:02Z で是認した先撃ちの前半＝撃つ・読む・確定に載せる・番号は done と 1:1）:
  1. **撃つ行と上限**: 事前審査が clean（確定 0 ∧ 暫定 0）の待ち行だけに撃つ。1 周に起こす本数の上限は rules 行 pipe.precheck_lens_per_round（kind PipePrecheckLensPerRound・Int・値 1・裁定 id は user 2026-09-27T17:33Z 項 2-2・manifest は `gate.lens_count` の直後・kind は `ALL` の `GateLensCount` の直後）で、周の頭に撃ち中の行を数え、上限に含める（周をまたいで越えない）。上限は lens を起こす本数だけに掛ける（材料の組み直しは形 3 で、上限の外）。値 0 は撃たない。行が無い・読めない周は撃たず、`[DISPATCH-PRECHECK]` の行の末尾に ` prelens=unset` を足す（C10）。` prelens=unset` は rules の状態を言うので、`--lens` の有無によらず付く。起こす側の周が `--lens` を持たない周は撃たない（Reviewed の段と同じく lens が無い）。口座は選ばない: Reviewed の段の lens（review の `decide`）は口座を選ばず起こす側の環境を継承する（口座を選ぶのは gate の lens の `lens_account` だけで、`pipe review` の入口は `Pool` を渡さない）。先撃ちも同じく起こす側（dispatcher の周）の環境を継承し、席の pane の変数 `TMUX_PANE` だけを外す。審査の材料は、予想の base を実体化した木から Reviewed の段と同じ組み手で、置き場の材料の dir（`REVIEW_DIR` と同じ名）に組む（下の 3 つ）。
     - **実体化**: 置き場（形 2）の `tree` に、main の HEAD の detached な一時の worktree を作り、行 x の予想と同じ層（`Layer`）を依存の順に当てる。層の列は precheck.rs の 1 本（行 x の `resolve` と同じ memo）から bead ごとに受ける（その口の可視性と渡し方〔関数か閉包か〕は実装が選ぶ・層の読み手を 2 本にしない・C2）。Gated PASS の祖先（`Layer::Tree`）は、`add` の file を祖先の木の HEAD から `git checkout <HEAD> -- <path>` で写し（`bodies` と違い拡張子で絞らない＝審査の材料は `.md` も読む・一時の worktree は anchor と object db を共有する）、`remove` の file を消す。宣言だけの祖先（`Layer::Declared`）は、write-set の `+` の file を空で作り、`~` の file を消す（`~` の file を素で持つ行は行 x の予想で clean にならず先撃ちに届かないので、`~` を消す動作は歯で測らない・便 s2-07l.718 の N+3 の質問への回答 2026-09-27T21:4xZ）。当てた後に index へ載せる（`git add -A`）。組み手の tracked の一覧は `git ls-files` を読み、本文は作業の木の file を読むためである。材料を組み終えたら worktree を外す（`git worktree remove --force`）。落ちた周の残り（置き場の `tree` に worktree が在る・dir が無く登録だけが残る）は、次の周の頭に外す（worktree が在れば `git worktree remove --force`、登録だけなら `git worktree prune`。worktree でない file は消さない＝形 1 の組めない周の歯 (f) を保つ）。`worktree add` は在る path にも登録だけの path にも作れないので、残りを外さないと以後の周が組めない。
     - **組み手**: review.rs の私有の `materials` と `keep` を、repo・契約・置き先の dir を受ける `pub(in crate::pipe)` の口 1 つから撃つ。repo には実体化した worktree の path を渡す。Reviewed の段は今の入口のまま同じ 2 本を撃つ（組み手を 2 本にしない・C2）。
     - **予想の印**: 宣言だけの祖先を 1 つ以上持つ行は、設計の材料の末尾に「予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた」の 1 行と path の列を足す。lens には空の file が予想であることが渡り、材料の鍵は実物の base の鍵と一致しない。
     - **組めない・起こせない周**: 実体化・組み手・lens の起動のどれかが断った行は撃たず、置き場に理由の 1 行の file `unbuilt` を置き、`[DISPATCH-PRECHECK]` の行の末尾に ` prelens=unbuilt` を足す（黙って撃たないに畳まない・C10 / NFR4）。次に組めた周に `unbuilt` を外す。
  2. **裏で撃ち、次の周が読む**: lens は `spawn_self` と同じ起こし方（process group を分け stdin を閉じる）で裏に起こし、箱（systemd の scope）では包まない（器の箱の片付け〔`crates/scribe2/src/pipe/confine.rs` の :477〕は作り手の pid が死んだ scope を畳むので、起こした周の process が終わると裏の lens が殺される）。起こした周はその終わりを待たない＝終端の周の行と知らせ・便の起動を遅らせない（lens は数分かかる）。
     - **置き場**: 事前審査の dir の下の `lens/<bead>/`。材料の dir の隣に、材料の鍵の file `key`・起こした時の材料の鍵の file `fired`・起こした lens の cmd の字の file `lens`・撃ち中の印の file `pid`・lens の rc と stdout の file `rc` と `out` を置く（`rc` を置いてから `out` を rename する＝`out` が在れば終わった）。`fired` と `lens` は lens を起こす時に写す。一時の worktree は同じ置き場の `tree` に作る。置き場は bead が母集団（行 x の閉じていない契約の行）に居る間は残し、居なくなった周の頭に外す（依存待ちを抜けた行の置き場も残る）。
     - **撃ち中の印**: 本文は `<pid> <起動時刻>` の 2 語で、pid は lens を包む `sh -c` の pid（group を分けて起こすので process group の id と同じ）。`lock_owner` が生きていると判じる行と、印を読めない行は撃ち中に数える（fail-closed）。死んで `out` の無い行は印を外して、上限の空いた周に撃ち直す。
     - **読む**: 次の起こす側の周が、Reviewed の段と同じ読み手（`decide` の後段の 1 本・rc → 最後の JSON 行の順）で `out` と `rc` から判定を読む。
  3. **材料の組み直し（上限の外）**: 撃ち中でない clean の行は、事前審査の鍵が `key` の 1 行目と違う周に、材料を組み直して `key` を付け替える（上限を埋める撃ち中の行が在る周も組み直す）。組み直しと形 4 の写し直しは、撃つ条件（起こす側の周の `--lens` の有無・rules 行の有無と値）に依らず毎周撃つ（撃たない周も、既に写した確定の finding を消さない）。撃ち中の行自身の材料は、その lens が終わるまで組み直さない（走っている lens が読む dir を消さない）。組み直した材料の鍵が `fired` と違う行は、同じ周のうちに `rc`・`out`・`fired` を外す（別の材料の判定を写さない）。形 4 で結果に写さなかった判定（rc が 0 でない・`unparsed`）の `out` を持つ行は、組み直した周に材料の鍵が `fired` と同じでも `rc`・`out`・`fired` を外す（測れなかった判定を先撃ちの段に留めない）。`fired` を外した行は、上限の空いた周に撃ち直す。鍵は、材料の dir の全 file の名と本文から名の順に決まる 1 つの字（器の既存の digest でよい・材料の種類を列挙しない）。
  4. **確定の finding**: `out` の判定が FAIL か INCONCLUSIVE で、lens の JSON が理由の型を持つ（`unparsed` でない）周だけ、理由の型を断りの名に、在り処を語 `prelens` にした確定の finding を事前審査の結果に載せる（在り処は行 x の 4 値の外の語で、同じ理由の型の先撃ちの finding は行をまたいで 1 束になる）。rc が 0 でない・JSON を読めない（`unparsed`）周は測れないとし、結果に写さない（`out` は残し、事前審査の鍵が次に動いて材料を組み直す周に形 3 で外して撃ち直す＝材料が変わらない行も撃ち直す。鍵が動かない周は撃ち直さない＝同じ材料で撃ち続けない）。事前審査が結果を書き直す周（main が動いて事前審査の鍵が変わった周）も、clean の行で材料の鍵が `fired` と同じ行は、書き直す**前**の結果を前の結果として同じ周のうちに判定を写し直す（`--lens` の無い周・rules 行の無い周・値 0 の周も写し直す）。前の結果に在った finding は `new=false` で残り、束の集合も変わらない。
- 形（行 ac・同じ裁定の後半＝Reviewed の段の使い回し・番号は done と 1:1・契約表の行は行 aa の着地の後に足す＝使い回しの読み口は行 aa が作る `crates/scribe2/src/pipe/dispatch/prelens.rs` に置くので、その file が base に在る周に write-set を書く・行 y と同じ手順）:
  1. **使い回しは材料と lens が同じ時だけ**: `pipe run` の Reviewed の段は、実物の base で組んだ材料の鍵（形 aa 3 と同じ 1 関数）が置き場の `fired` と同じで、`out` の判定が `unparsed` でなく、起こす側の lens の cmd の字（置き場の file `lens` に `fired` と同じ時に写す）が便の lens の cmd と同じで、先撃ちの lens と審査の lens の model の行が同じ model に解ける時（[pipeline.md](./pipeline.md) §61 形 5）だけ、先撃ちの判定を写す。段の detail の末尾に語 ` prelens:reused` を足す（`read_detail` は語で読むので `pipe report` は変わらない）。違えば今どおり lens を撃つ。予想した file や名が実物の base に無ければ材料が変わって鍵が外れる＝予想の外れを使い回さない。読み口は prelens.rs に置き、review.rs（`pipe::review`＝`pipe::dispatch` の子でない兄弟）から呼べるよう、`crates/scribe2/src/pipe/dispatch.rs` の私有の module 宣言 `mod prelens;` の可視性を `pipe` の中へ広げる（その 1 行だけ・置き場の dir は prelens.rs から precheck.rs の `dir_of`〔`pub(super)`＝`dispatch` の子孫から見える〕で引くので precheck.rs は変えない・便 s2-07l.724 の審査 FAIL 2026-09-27T23:17Z）。
  2. 写した周は lens を撃っていないので、審査の消費の 1 件を書かない（先撃ちの lens の消費は便に属さず記録しない・限界）。
- 台帳の順（表の `depends` は同じ doc の中しか指せないので、doc を跨ぐ順は台帳の blocks で持つ）: 行 w → [seat-heartbeat.md](./seat-heartbeat.md) 行 t → 同 行 u → 行 y（y は u が足す rules 行と ` alarm=` の列の後ろに足す）／行 x → y・x → aa → ac（表の `depends`）／[contract-source.md](./contract-source.md) 行 bc → 行 aa（bc は審査の材料に外の材料を足すので、aa の鍵がそれを含む版で先撃ちする）。
- 触らない: 列の判定と順序・`WaitReason`・`Turn`・起こす契機（時計の契機を足さない・§5）・受付の判定関数の中身と断りの順・`pipe preflight` の外形・既存の `[DISPATCH]` / `[DISPATCH-COUNT]` の行・event kind。
- 言語に依らない所と Rust に依る所: 予想の base の file の足し引き・鍵・結果の file・束・知らせ・段の上げは file・行・契約表・台帳だけで持つ。Rust に依るのは受付の判定の中の本文の読み手（`.rs` / `.snap` の閉包・歯の置き場・名指し）で、在り処の「名」の値と、実物で重ねる時の本文の置き換え（読み手の読む拡張子の file だけ）がその上に乗る。読み手が他の言語の file を読むようになれば、同じ弁別がそのまま効く。
- 歯（接頭辞 9 つ・`grep -rn "fn <接頭辞>" crates/` がどれも 0 件・2026-09-27／行 aa の `pipe_prelens_` は 2026-09-28）:
  - 行 x: e2e `pipe_dispatch_precheck_`（`crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs`・偽の台帳と toy repo）: (a) open な依存 A（行が `+` の file F を宣言）を待つ B の行が F を素の path で持つ周の終端の 1 周の後、B の結果が clean（base では結果の file が無い＝RED・機能不在）(b) B が base にも A の宣言にも無い file G を素で持つと firm:1（断りの名 write-set-item-unresolved）(c) B の file が A の write-set と交わり上限の余地が足りないと provisional:1 (d) 同じ鍵の 2 周目は結果の file を書き直さず、A の便を Gated PASS にした周は撃ち直され、A の木で足した fn 名が B の filter 語に当たると teeth-outside-write-set が firm と new で出る (e) `dispatch ls` が `[DISPATCH-PRECHECK]` の行を出し、`[DISPATCH]` の行の reason は変わらない (f) 予想で clean の B は、A が `+` の F を作らずに閉じた周に受付の断り（`admission:contract-table`）で待つ（予想は通行証でない）(g) 推移の祖先: A → C → B の blocks で C が宣言する `+` の file を B が素で持つと clean、`parent-child` だけで繋がる bead の宣言は予想に入らず firm:1、closed の bead の宣言も入らない。lib `pipe_refuse_evidence_`（`crates/scribe2/src/pipe/refuse.rs`）と `pipe_table_evidence_`（`crates/scribe2/src/pipe/table.rs`）: 在り処が `REFUSALS` の 23 語と `TableError` の 17 variant の母集団で 1 つずつ決まり、弁別の 3 値を在り処 4 値 × 動く file の有無で測る。
  - 行 y: e2e `pipe_notify_precheck_`（`crates/scribe2-boundary/tests/e2e/notify.rs`）: 同じ根の確定を持つ待ち行 3 本が束 1 つになり、束の file が 3 行の pointer を持ち、席の pane への 1 行が周ごとに 1 回（束の集合が同じ次の周は送らない）でその 1 行が `precheck bundles=1 rows=3 <束の id>=<束の file の path>` で終わり、idle の行の末尾に ` precheck=3/3:1`。同じ接頭辞で、(d) 束を持つ置き場の `dispatch ls` が束ごとに `[DISPATCH-BUNDLE] id=<束> rows=3 root=<名> file=<path>` の 1 行を出し（base は行が無い）、(e) fixture の設計を直して 3 行の確定が消えた周に、束の file が置き場から外れ、`[DISPATCH-BUNDLE]` の行が 0 になり、`precheck bundles=0 rows=0` の 1 行が 1 回だけ送られる（次の周は送らない）。e2e `seat_tick_precheck_`（`crates/scribe2-boundary/tests/e2e/seat/tick.rs`）: (a) 事前審査の dir の在る置き場の合図の末尾に ` precheck=` が付き、無い置き場は付かない（既存の合図の期待は不変）(b) 値 60 の写しで束の時刻が 120 秒前・席の打刻が 90 秒前（`seat.tick_stale_s` より新しい）の周に合図が 1 回出て ` alarm=` の列に `precheck` が在る（base と黙りの門を短くしない実装は stamp-recent の noop＝短くした門を測る）(c) 行の無い写しで dir の在る置き場の同じ周は noop（門は短くならない）で、打刻が `seat.tick_stale_s` を越えた周の合図の ` alarm=` の列に `precheck-unset` が在る。rules `rules_precheck_`（`crates/scribe2-boundary/tests/e2e/rules.rs`）: 行の形・値・位置（manifest と `ALL`）。base では束の file も末尾も無い＝RED。
  - 行 aa: e2e `pipe_prelens_`（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・偽 lens が起動のたびに回数の file へ 1 行を足す・`grep -rn "fn pipe_prelens_" crates/` は 0 件・2026-09-28）: (a) 形 1 の上限: 上限 1 の写しで clean の待ち行 2 本を置く。1 周目は 30 秒眠る偽 lens を 1 本だけ起こす（回数 1）。その lens が生きている間の 2 周目も、残りの 1 本を起こさない（回数 1・撃ち中を上限に数える）。印の group を SIGKILL で殺した後の 3 周目は 1 本だけ起こす（回数 2）。値 0 の写しの周は偽 lens 0 回 (b) 行の無い写しの周は偽 lens 0 回で、`--lens` を持たない `dispatch ls` の `[DISPATCH-PRECHECK]` の行が ` prelens=unset` で終わる (c) 口座: 偽 lens が `${TMUX_PANE-unset}` と別の変数の値を file に書き出し、`TMUX_PANE` とその変数を持つ env で周を撃つと、前者が `unset`・後者が継承の値 (d) 実体化（宣言）: 宣言だけの祖先 A（`+` の file F を宣言）を待ち、F を素の path で持ち、設計の節が F の path を backtick で名指す待ち行 B の先撃ちで、偽 lens が受けた設計の材料の末尾に予想の base の 1 行と F の path が在り、base の要約で F の行が `行数 全体 0` で、外の材料に F が載り（空の F が tracked）、先撃ちの後に一時の worktree が置き場にも `git worktree list` にも残らない (e) 実体化（実物の木）: 祖先 A が Gated PASS で、A の木が `.md` の file G（`.rs` でない）を足して 2 行を書いた周に、G を素の path で持つ待ち行 B へ先撃ちすると、偽 lens が受けた base の要約で G の行が `行数 全体 2` で、設計の材料に予想の印が無い (f) 組めない周: 置き場の `tree` の path に file を置いて worktree を作れない周は偽 lens 0 回で、ls の行が ` prelens=unbuilt` で終わり、その file を外した次の周は撃って ` prelens=unbuilt` が消える (g) 起こした周は待たない: 30 秒眠る偽 lens を起こした周が返った時に、置き場に `out` が無く、印の本文が 2 語で、印の pid が生きている (h) 印の弁別: 印の 2 語目を別の数に書き換えた周（pid は生きている）は印を死んだと判じて撃ち直し（回数 2）、印の本文を数でない字に壊した周（`lock_owner` が読めないと返す）は撃ち中に数えて撃ち直さない（回数 1） (i) 形 4 の確定: 偽 lens が `{"verdict":"FAIL","kind":"vacuous-assert",…}` を返した先撃ちの次の周に、`[DISPATCH-PRECHECK]` の行が `result=firm:1,provisional:0` を持ち、`[DISPATCH-BUNDLE]` の行が `root=vacuous-assert` を持ち、結果の file の finding の行の在り処が `prelens` である。`{"verdict":"INCONCLUSIVE","kind":"section-material-missing",…}` も同じく `result=firm:1,provisional:0` と `root=section-material-missing` を持つ。rc 1 で終わる偽 lens の周は `result=clean` のまま (j) 周またぎ（材料が同じ）: 上限 1 の写しで、(i) の FAIL を写した待ち行 B と、30 秒眠る偽 lens が撃ち中の待ち行 C を置く。B と C の材料に入らない file を 1 つ main に commit した次の周に、B の行が `result=firm:1,provisional:0` のままで、finding の行が `new=false` を持ち、`[DISPATCH-BUNDLE]` の行が commit の前の周と同じ。同じ commit の次の周を `--lens` 無しで撃った周と、値 0 の写しで撃った周も同じ（撃たない周も写し直す） (k) 周またぎ（材料が変わる）: 同じ置き方で B の write-set の素の path の file の本文を main で変えて commit した次の周に、B の結果が `result=clean` で、B の置き場に `out` と `fired` が無く、C の材料の dir の file は C の lens が生きている間は変わらない (l) 置き場の寿命: 依存を閉じて依存待ちを抜けた B の置き場は残り、B の bead を閉じた周の頭に B の置き場が消える (m) 測れなかった判定の撃ち直し: 偽 lens が JSON の行の無い stdout（`unparsed`）を返した先撃ちの後、main を動かさない周は偽 lens の回数が 1 のままで B の行が `result=clean`。B の材料に入らない file を 1 つ main に commit した後の 2 周のうちに偽 lens が撃ち直され（回数 2）、main を動かさない次の周も回数 2 のまま（base の実装は材料の鍵が `fired` と同じなので撃ち直さず回数 1＝RED） (n) 残りの片付け: 置き場の `tree` に一時の worktree を作ったまま残した置き場と、`tree` の dir を消して登録だけを残した置き場のそれぞれで周を撃つと、どちらも ls の行が ` prelens=unbuilt` で終わらずに偽 lens が 1 回起こり、周の後の `git worktree list --porcelain` がその path を持たない。rules `rules_prelens_`（`crates/scribe2-boundary/tests/e2e/rules.rs`）: 行の形・値・位置（manifest と `ALL`）。
  - 行 ac: e2e `pipe_review_reuse_`（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・行 aa の偽 lens と周の helper を使う）: (a) 祖先 A が Gated PASS で A の木が `.md` の file G に 2 行を書いた周に、G を素で持つ B へ先撃ちし、A がその木のまま着地した後に起こす側の周を 1 回撃ってから B の Reviewed を撃つと、偽 lens を撃たず判定を写し、Reviewed の detail が語 ` prelens:reused` で終わり、審査の消費の event が増えない (b) G が 3 行で着地した周（着地の本文が Gated の木と違う）の Reviewed は偽 lens を撃ち（回数 2）、detail にその語が無い (c) 宣言だけの祖先を持つ行（予想の印を持つ）は、祖先が着地した後の Reviewed で偽 lens を撃つ (d) 先撃ちが rc 1（`unparsed`）で終わった行の Reviewed は偽 lens を撃つ (e) 起こす側の lens の cmd の字と便の lens の cmd の字が違う周の Reviewed は偽 lens を撃つ。
- 限界: 行 x の受付の判定について、宣言の予想は本文を持たないので、依存が足す本文に由来する断り（閉包の広がり・歯の置き場の当たり）は宣言の周に見えず、Gated PASS か着地の周に初めて確定として出る（偽陰性の向き・偽の警報は出さない）。行 aa の先撃ちは宣言の予想でも確定に載せるので、空で置いた file に由来する FAIL / INCONCLUSIVE は祖先の着地で消えうる（早い警告の代価・偽の警報の向きに外す）。祖先 A の Gated PASS の後に main が A の `add` の file を変えた周は、A の古い本文で材料を組む（偽の警報の向き・A の着地で base が動くまで）。固まった lens は印の group を殺すまで上限を埋める（lens の時間の上限の rules 行は無い）。先撃ちの lens の消費は便に属さず記録しない。祖先の便が Gated PASS の後に追随や衝突の解きで本文を変えた周は、着地で base が動いた周に測り直すまで古い実物で測った値が残る。在り処の「名」は本文の読み手の母集団（今は `.rs`）の file が 1 本でも動けば暫定に倒す粗い弁別で、確定を減らす向きに外す。事前審査と先撃ちの結果は起こす側の周にしか更新されないので、列が 0 本で止まっている間は古いまま（時計の契機を足さない・§5）。
- 却下: 待ち行に `pipe preflight` を素のまま撃つ（偽の断りで束が埋まり、契約表の段で止まって深い検査に届かない）／依存の live な木（Spawned / Implemented）を base にする（編集の途中・gate を通っていない木で確定を出すと偽の警報になる。Gated PASS の木だけを実物として使う）／複数の依存の木を merge-tree で 1 つにして material を読み直す（live な便の write-set は交わらないので file の重ね合わせで足り、merge の費用と衝突の読みが要らない）／断りの字面を読んで確定と暫定を分ける（自由文を判定の入力にする・C3.3）／結果を event に記帳する（新しい event kind・C17.1。結果は鍵で捨てられる cache）／予想の結果で起こす判定を緩める（予想は外れうる・通行証にしない）／先撃ちの lens を `fire` の中で待つ（終端の周の行と知らせが lens の数分だけ遅れる）／先撃ちの鍵に材料の種類を並べる（材料が増えた版で鍵が追わず、足りない材料で出した判定を写す）。

## 28. 死んだ札の歯の待ちを札の継ぎ替えの間に釣られない形にする — `gone` は札の不在が 500 ミリ秒続くまで待つ（契約表の行 ab・memo `s2-07l.709`）

やさしく言うと: 「札が片付いたか」を見る歯の待ち方が、札の持ち主が入れ替わる一瞬の空白を「片付いた」と読んで落ちることがある。空白は一瞬なので、「無い状態がしばらく続く」まで待つ形に直す。

- 何が起きているか（verified）: main の CI の nextest で、同じ歯 `pipe_dispatch_gated_pass_dead_ticket_is_resumed_regardless_of_verdict` が 2 回落ちた（2026-09-27 の着地 c6c997b と f4d891d・どちらも `crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs` の :403 の `gone` の assert・0.166 秒と 0.174 秒・`gh run rerun --failed` で緑）。どちらも器は終端の CI の赤で close を撃たず、orchestrator が緑を実測して手で close した。`gone`（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` の :960）は札が在る間 60 秒まで 50 ミリ秒ごとに見て、loop を抜けた後にもう 1 回在るかを見る。60 秒を待たずに偽を返すのは、1 回目の不在の観測の後に札がまた在る周だけである。
- 根（verified は code・deduced は落ち方との対応）: 札を継ぐ側（`crates/scribe2/src/fleet/store.rs` の `acquire_in`・:462）は、死んだ所有者の札を `reclaim` で外し（:482）、loop の先頭の `create_new`（:467）で自分の札を書く。外してから書くまでの間（同じ process の続く数 syscall）だけ札の path が無い。deduced: `gone` がこの間を不在と読んで loop を抜け、2 回目の観測で継いだ resume 自身の札を読んだ。落ちた時の段の並びが `Gated` 止まり（resume が始まったばかり）で、0.17 秒で落ちたことと合う。
- 形（番号は done と 1:1）:
  1. **`gone` を「不在が続いた」で判じる**: 札の path の不在が 500 ミリ秒続いた周に真、60 秒のうちに続かなければ偽（50 ミリ秒ごとに見て、在る観測で数え直す）。継ぎ替えの間は同じ process の続く数 syscall で 500 ミリ秒に届かず、不在が続くのは最後の持ち主が抜けて札を外した後だけである。呼び手 3 本（`waiting.rs` の `pipe_dispatch_gated_pass_dead_ticket_is_resumed_regardless_of_verdict` と `pipe_dispatch_regated_dead_ticket_keeps_three_records_and_resumes_once`・`dispatch.rs` の `pipe_dispatch_drive_revives_a_dead_driver_all_the_way_to_landed`）の assert と期待は変えない。
  2. **歯を足さない helper の変更に札を置く**: e2e の `dispatch.rs` は歯を足さないので、flip-check の overlay（file ごとに base へ重ねて赤になる歯を探す）は base で緑になる。変えた `gone` の直前に札 `// flip-check: retroactive <本行の bead の id>` を 1 行置く（account-autonomy.md §22・行 s と同じ逃がし・判定行に `retroactive=` で残る）。
- 触らない: src（`acquire_in` の回収の形・札の本文・`Driver` の Drop）・歯の名と assert・`put_dead_ticket`・`waiting.rs` の本文。
- 限界: 500 ミリ秒は時間の閾値である。継ぎ替えの間がそれより長くなるのは、継ぐ process が `reclaim` と `create_new` の間で 500 ミリ秒止まった周だけである。
- 却下: 継いだ resume の pid を札の本文から読み、その process の終わりを待ってから 1 回で判じる（継ぎ替えの後で最初の観測より前に resume が抜けると pid を見られず、別の分岐が要る）／src の回収を rename で原子的にして間を無くす（deduced: 本番の読み手が間に札を Absent と読んで 2 本目の resume を起こしても、その resume は生きた所有者の札を取れずに抜ける。本番の直しは要らず、歯の待ちだけの問題である）。

## 29. 未処置の終端を idle の知らせに毎周載せ、送達の結果を stderr にも残す（契約表の行 ad・memo `s2-07l.732`）

やさしく言うと: 便が落ちた知らせは 1 回きりで、席が turn の途中だと届かずに消える。台帳で開いたまま処置の付いていない落ちた便を、列の idle の知らせに毎周載せて、届くまで言い続ける。送れたかどうかも記録に残す。

- 何が起きているか（2026-09-28）:
  - verified: 終端の知らせ（§19 形 3 (a)）は運転手の終端の周に 1 回だけ撃たれる。送達の窓は rules 行 `pipe.stop_grace_ms`（2000 ミリ秒）で、席が turn の途中だと届かない。2026-09-28T04:04Z の本 repo の Gated FAIL と、04:16Z の非公開の隣の project の Reviewed FAIL の 2 件で、知らせが席に届かず、席は `dispatch ls` を撃つまで気付かなかった。
  - verified: 送達の結果（`notify=` の行）は運転手の stdout にだけ書かれる。列が起こした運転手の stdout は捨てられ（`crates/scribe2/src/pipe/dispatch.rs` の `spawn_self` の `Stdio::null`）、stderr だけが置き場の launch.log に残る。届いたか・断られたかは、どこにも残らない。
  - verified: 2026-09-27T00:00Z 以後の終端（Reviewed / Gated の FAIL と INCONCLUSIVE・Failed・Questioned）のうち、席への注入の記録が無いものは本 repo で 34 件中 3 件、隣の project で 109 件中 20 件（Gated の再試行の途中の判定を含む上限値）。
- 現物（main・verified）: `crates/scribe2/src/pipe/cli.rs` の `notices` が、自分の便の終端の 1 行（`alarm_word` が知らせる終端を判じる・段の網羅 match）と idle の 1 行（`crates/scribe2/src/pipe/notify.rs` の `idle_line`・列の結果が「起こした便 0 ∧ 候補 1 本以上」の周だけ）を組み、`notify.rs` の `send` の結果の行を stdout に返す。列の候補の理由の `Settled` は、直前の便が同じ契約 file の sha で終端に着いた bead を列外にする（段を持つ・run id は持たない）。台帳で閉じた bead は候補に並ばない。
- 形（番号は done と 1:1）:
  1. **未処置の終端を数える**: `notices` が、同じ周の列の結果の候補のうち理由が `Settled` のものについて、置き場の replay からその bead の最新の便（run id の昇順の最後）を引き、`alarm_word` が `Some` を返すものを未処置とする。判じ手は `alarm_word` の 1 本のまま（2 本目を書かない・C2）。台帳で開いた bead だけが候補に並ぶので、閉じた bead は数えない。周ごとに導き直し、記録を持たない（C10）。
  2. **idle の 1 行の末尾に足す**: idle の 1 行の今の末尾（§26 形 2 の並列の実測・§27 形 3 の事前審査）の後ろに ` pending=<k>:<bead>/<段>=<語>,…`（候補の並びの順・段は `as_str` の字面・語は `alarm_word` の返り値）。未処置が 0 本の周は key を出さない（既存の idle の行の字面は 1 字も変わらない）。
  3. **契機は変えない**: 送るのは今どおり §19 形 1 (b) の周（運転手の終端の周で、起こした便 0 ∧ 候補 1 本以上）。未処置の bead は `Settled` の候補として必ず列に載るので、終端の周が来るたびに同じ行が送られる（処置＝close・release・行の直しが付くまで）。
  4. **送達の結果を stderr にも写す**: `notices` が返す `notify=` の行を、stdout に加えて stderr にも同じ字面で出す。列が起こした運転手の周は launch.log に残る（event も tick.jsonl の schema も変えない）。置き場に席の無い周も `notify=no-seat` を写す（誰にも届かなかった事実こそ残す）。終端の stderr を完全一致で照合する既存の歯は e2e の `pipe/land/retire.rs` の `pipe_retire_reviewed_unreadable_refused_and_names_the_verdict` の 1 本で（2026-09-28・本行の実装の木で e2e 1662 本を撃ち、落ちたのはこの 1 本・verified）、`notify=` で始まる行を除いた stderr で断りの 1 行を照合する形に直す（断りの字面は変えない）。直した歯は base でも緑（base の stderr に `notify=` の行が無い）で、flip-check は歯の中の行を動かした file を歯の本体として base で赤を求める（歯の外の file として同梱するのは、動いた行が 1 本も歯の中に無い file だけ）ので、retire.rs の test 区間の行頭に `// flip-check: retroactive s2-07l.732` の札を置く（2026-09-28 の 1 便目が札なしで `green-on-base file=…/retire.rs` の FAIL・verified）。
  5. `notify.rs` は段の閉じた型の variant を名指さない（§19 形 5）: 未処置の段と語は `notices` が字面で渡す。
- 触らない: 終端の 1 行（§19 形 3 (a)）・送る宛先と窓と送達の 1 関数・`alarm_word` の判定・`WaitReason` の variant と `render` の字面（`dispatch ls` の `reason=`）・列の判定・event の kind・heartbeat（[seat-heartbeat.md](./seat-heartbeat.md) §16）の行。
- 歯（接頭辞 `pipe_notify_pending_`・`crates/scribe2-boundary/tests/e2e/notify.rs`・`pipe/` の外＝§19 形 6・既存の `intake_bead` と `idle_round` の型・`grep -rn "pipe_notify_pending" crates/` は 0 件・2026-09-28）:
  - (a) 審査の判定 FAIL で終端に着いた便の bead が台帳で ready のまま `Settled` の候補になる置き場で、別の live な便の終端（`pipe stop --run`）を撃つと、idle の 1 行が ` pending=1:<その bead>/Reviewed=<語>` で終わる。base は key が無いので RED（機能不在）。
  - (b) 同じ周の終端の stderr が、stdout の `notify=` の行と同じ行を持つ。base は stderr に無いので RED。
  - 未処置が 0 本の周に key を出さないことは、既存の `pipe_notify_facts_hold_round_without_live_runs_reports_zero_live_and_zero_minutes`（末尾が ` held=0` で終わる）が不変で GREEN のまま pin する。
- 限界:
  - 知らせ直しは運転手の終端の周にだけ起きる。走っている便が 1 本も無く終端の周が来ない間は、知らせ直さない。時計で撃つ heartbeat は台帳を読まないので、未処置を数えられない。
  - 列が便を起こした周（起こした便が 1 本以上）は idle の行を送らないので、その周は知らせ直さない。
  - 着地したのに close できなかった便（`Landed` のまま台帳で開いた bead）は `alarm_word` が知らせない段なので、未処置に数えない。
- 却下:
  - 終端の 1 行を送れなかった周に、運転手が窓を延ばして再送する。運転手は席の turn の終わりを待てず、同じ穴が残る。
  - 未処置を event log だけから数える。閉じた bead の古い便が残り、本 repo の置き場で 12 本中 11 本が雑音になった（台帳を読まない数え方）。
  - 送達の結果を event に記帳する。通知は副作用で、event を足すと replay と schema の面が増える（§19 の「記帳しない」）。

## 30. 便の worktree の build と依存の置き場を、便が live でなくなった周に器が消す — 運転手の終端の周ごとに、live でない便の木から、名が閉じた列に在り追跡されている file を持たない dir を消す（契約表の行 ae・[ADR-0081](../../design-intent/decisions/ADR-0081-run-worktree-intermediates-are-swept-when-runs-stop-being-live.html)・裁定 user 2026-09-28T04:45Z / 04:46Z）

やさしく言うと: 器は便ごとに作業用の木を作り、着地したら退役の置き場へ移すが、build の産物と依存の置き場は誰も消さなかった。持ち主の disk が満杯になり、3 つの project の便が落ちた。便が終わった木から、器が対応する言語の build と依存の置き場（作り直せる物）だけを器が消す。

- 何が起きているか（2026-09-28・verified）:
  - host の root の file system（1.8 TB）が満杯になり、空きが 0 に落ちた。本 repo の `s2-07l.731` の便は、歯が一時 dir を作れず（No space left on device）Gated FAIL になった。非公開の隣の project 2 つでも便が同じ理由で落ちた。
  - 便の worktree は 1 本 2 GB 前後の build の置き場を持つ。退役（設計 [pipeline.md](./pipeline.md) §5.4・可逆な move）は木ごと退役先へ移すだけで、build の置き場は残り続ける。非公開の隣の project で 140 本・597 GB（うち退役済み 117 本の build の置き場が 540 GB）、本 repo で 80 GB だった。
  - 持ち主の承認（裁定 user 2026-09-28T04:45Z）で、退役済みの worktree の build の置き場を手で消し、空きは 401 GB に戻った。続けて、中間生成物を掃除しながら進める器の直しを最優先で入れる指示（裁定 user 2026-09-28T04:46Z）。[ADR-0081](../../design-intent/decisions/ADR-0081-run-worktree-intermediates-are-swept-when-runs-stop-being-live.html) は、この指示を形 1 の閉じた列の dir を器が消すことへの常設の承認（憲法 A1・逐語は `RulingReceived`）と読む。
- 現物（main・verified）: `crates/scribe2/src/pipe/cli.rs` の `dispatch` が、終端の subcommand（`TERMINALS`＝run / resume / land / stop / retire）の後に列の 1 周を撃つ。便が live かは `crates/scribe2/src/pipe/cli/state.rs` の `live` の 1 本で判じる（測れない周は `None`）。便の repo は置き場の記録（`crates/scribe2/src/pipe/mod.rs` の `repo_of_run`）から、木の場所は同じ file の `worktree_path` と `crates/scribe2/src/pipe/retire.rs` の `retired_path` から解ける。置き場ごとの lock は `crates/scribe2/src/fleet/store.rs` の `acquire_with` の 1 実装を使える。
- 形（番号は done と 1:1）:
  1. **消す dir の閉じた列**（器が対応する言語の build と依存の置き場・宣言順・const の slice）: `target`（Rust）・`node_modules`（TypeScript）・`.venv`・`__pycache__`・`.mypy_cache`・`.pytest_cache`・`.ruff_cache`（Python）・`.expo`（React Native + Expo）。どれも commit した木と toolchain（と lock file）から作り直せる。列に無い dir と file・無視の規則・host の個人設定の除外は読まない（`git clean` は撃たない）。
  2. **掃除の 1 関数**（行 ae の write-set の `+` の file）: 置き場の replay の全便のうち、`live` が `Some(false)` の便について、置き場の記録から repo を解き、元の場所と退役先のうち在る方の木を歩く（`.git` には降りない）。名が形 1 の列に在る dir のうち、追跡されている file を 1 つも持たないもの（その木の `git ls-files` の 1 回で判じる）を std の `remove_dir_all` で消し、その下へは降りない。`live` が `Some(true)` の便・測れない便（`None`）・repo を解けない便・木が無い便は撃たない（残す側に倒す）。`git ls-files` を撃てない木は消さずに失敗に数える。
  3. **撃つ周**: `cli.rs` の `dispatch` が、終端の subcommand の周に、段の記帳の後・列の 1 周（§5）の前に 1 回撃つ（空きを作ってから次の便を起こす）。要るのは `--state-dir` だけで、`--repo` の無い周も撃つ。冪等で、終端の周ごとに置き場の全便を見るので、器の入れ替えの後の最初の終端の周が古い木の溜まりも片付ける。
  4. **置き場ごとに 1 本**: 置き場の pipe の dir の掃除の lock を `acquire_with` で取ってから撃ち、取れない周（別の運転手が掃除している）は撃たずに stderr に 1 行を残す。第 2 の lock の実装は作らない（C17 の「既に在るか」の段）。
  5. **残すのは stderr の 1 行だけ**: 1 つ以上の dir を消した周か、失敗した木が在る周か、lock を取れない周だけ、stderr に `sweep: removed=<消した dir の数> runs=<dir を消した木の数> failed=<本数>[:<便 id>,…]`（lock の周は `sweep: skipped=lock`）を 1 行出す。stdout・event・rc は変えない（掃除の失敗で終端の rc を変えない）。
  6. 変えないもの: 退役の move（N1.2）・`WorktreeCheck` の判定・`live` の判定・列の判定と起こす契機・rules 行・event の kind。席の手が打つ `git clean -f` を断る rules 行 `host_guard.git` も変えない（掃除は `git clean` を撃たない）。
- 歯（接頭辞 `pipe_sweep_`・`crates/scribe2-boundary/tests/e2e/pipe/stop.rs`・`grep -rn "pipe_sweep" crates/` は 0 件・2026-09-28）: toy repo の置き場に記録した便ごとに木を作り、`pipe stop` の終端を撃つ。
  - (a) `Stopped` の便の元の場所の木から、追跡されていない `target/` と `node_modules/` が消える。追跡されている file を持つ `target` の名の dir（例 `docs/target/keep.md`）・列に無い未追跡の dir（`out/`）・無視の規則に当たる列に無い file は残り、stderr が `sweep: removed=2 runs=1` で始まる行を持つ。終端の後も live のままの便（`Implemented`）の木の `target/` は残る。base は消えないので RED（機能不在）。
  - (b) `Landed` の便の退役先の木の `target/` が、下に入れ子の `.git` を持っていても消える。base は残るので RED。
- 限界:
  - 列に無い build の置き場（`dist`・`build` のような汎い名、言語の外の道具の cache）は消さない。器が対応する言語を足す便は、同じ ADR の型で列を足す。
  - Gated の FAIL の便を regate で撃ち直す周と、`Stopped` の便を resume で起こし直す周は、build と依存の取り込みを最初からやり直す（数分・正しさは変わらない）。落ちた便の木で歯を撃ち直して調べる周も同じ。
  - live の判定と掃除の間に、同じ便が regate や起こし直しで live に戻る競合は、lock の外の操作（regate・resume）とは排他でない。戻った便は消えた置き場を作り直す。
  - 空きが少ない周に起こすのを止める遮断器（§18 の健康の遮断器に disk の空きを足す形）は持たない。閾値の rules 行は持ち主の裁定が要るので、次の行の候補にする。
  - 席の起草の置き場の写しは §33 が同じ関数と lock で掃く（書きの線つき）。
- 却下（[ADR-0081](../../design-intent/decisions/ADR-0081-run-worktree-intermediates-are-swept-when-runs-stop-being-live.html) の比較と同じ）: 今のまま人が片付ける／`git clean -d -X -f` で無視された file を全部消す（無視の規則は鍵・手書きの script・local の設定のような作り直せない物も持ち、host の個人設定の除外も読む）／退役先の木だけを掃除する（regate の手戻りは無いが、退役しない終端の木が溜まり続ける）／repo ごとに build の置き場を 1 つ共有する（言語に依り、並んだ便の build が 1 つの lock で順に待つ）／退役先の木ごと消す（N1.2 の可逆な退役を破る）／project ごとに消す dir を宣言に書く（跨版の新しい key・書かない project では効かない）。

## 31. 台帳の問い（label intake:question）を起動の列の入力と事前審査の母集団から memo と同じく外し、memo と問いの label の字の定義を 1 か所に寄せる（契約表の行 af・FR68・FR51・ADR-0083・ADR-0088）

やさしく言うと: 台帳の問い（席から user への問いを残す bead）は契約ではない。ところが列は memo の印しか見ていないので、問いに受入条件の文が入っていると、列はそれを契約として起こしてしまう。列が memo と同じく問いも外すようにし、あわせて 4 か所に散った memo の印の字を 1 か所にまとめる。

- 何が起きているか（main 24f6ef1e・verified）:
  - 列の入力の判定 `is_input`（`crates/scribe2/src/pipe/dispatch/candidates.rs` の :33）は「status が open ∧ acceptance が空でない ∧ label `intake:memo` が無い」の 3 つだけを見て、label `intake:question` を見ない。いま問いが列に並ばないのは、問いがたまたま acceptance を持たないからである。acceptance を持つ問いは列の候補に並び、設計 pointer の行を持てば起こせる側（`dispatch ls` の reason が `-`）に立つ。起票の門は問いの acceptance を断らない（FR81 (a) の欄に acceptance は無く、席の子 process の書きは門の外）。
  - 事前審査（§27）の母集団と到達の関数 `population`（`crates/scribe2/src/pipe/dispatch/precheck.rs` の :50）も「closed でない ∧ label `intake:memo` が無い ∧ 設計 pointer が解ける」bead を契約の行に数え（:54）、問いを外さない。設計 pointer を持つ問いが待ち行の blocks の祖先に居ると、その問いの宣言が予想の base に入る。
  - 要件は外すと言う: SRS FR68 の起動の列は「memo でなく ∧ 台帳の問い（label intake:question・FR81）でなく」、ADR-0083 の決定 (1) は「この label の bead は台帳の lint と起動の列が契約にも memo にも数えない」。台帳の形の lint の側は [ledger-form.md](./ledger-form.md) §13（行 i）で着地済みで、列の側だけが残っている。
  - 印の字の定義: `intake:memo` は 4 つ在る。`crates/scribe2/src/ledger/form.rs` の :25（公開の const `MEMO_LABEL`）・`crates/scribe2/src/ledger/lint.rs` の :23（公開の const `MEMO_LABEL`）・`crates/scribe2/src/pipe/dispatch.rs` の :53（私有の const `MEMO_LABEL`・子の candidates.rs と precheck.rs が `super::` で引く）・`crates/scribe2/src/ledger/memo.rs` の :22（公開の const・memo の plan の引数に書く）。起票の門（`crates/scribe2/src/hook/ledger_guard.rs` の :20）は form.rs の const を借りるので、定義には数えない。`intake:question` は form.rs の :37（`QUESTION_LABEL`）の 1 つだけ。lint.rs と memo.rs の const に外の呼び手は 0 本。
  - 台帳の bead の label を自分で比べる判定は 5 か所: form.rs の is_memo（:103）と問いの除外（:187）・lint.rs の is_memo（:60）・candidates.rs の :36・precheck.rs の :54。起票の門は台帳の書きの command の label を比べる（:245・台帳の bead ではない）。
  - [ADR-0088](../../design-intent/decisions/ADR-0088-case-positions-are-computed-once-by-the-vessel-and-read-from-one-file.html) の代償の 1 項が「label intake:memo の定数を局面の関数が 5 つ目の読み手にしないよう、実装の行で 1 か所へ寄せる手も要る」と言う。
  - 本 repo の台帳（2026-09-29・読むだけ）: bead 810 件のうち問い 3 件（全部 closed・acceptance を持つもの 0）・memo 133 件（acceptance を持つもの 0）。この repo で誤って起こされた問いは今は無い。穴は、問いが acceptance を持った周に開く。
- 形（番号は done と 1:1）:
  1. **印の字の定義を form.rs に寄せる**: `intake:memo` の字を定義するのは form.rs の `MEMO_LABEL` の 1 つだけにする。lint.rs・memo.rs・dispatch.rs の const は消し、form.rs の const か形 2 の述語を引く。`intake:question` は form.rs の `QUESTION_LABEL` のまま。器の src で 2 つの印の字を定義する所は form.rs の 2 つの const だけになる。memo の plan の引数（`arg: --labels=intake:memo`）・doctor の台帳の 2 行・起票の門の断り文の字は 1 字も変えない。lint.rs・memo.rs・dispatch.rs は行が減るだけである。
  2. **判定の述語を form.rs に 2 つ置く**: form.rs の is_memo を公開にし、同じ形で「label `intake:question` を持つか」を判じる公開の述語 is_question を足す。form.rs の 4 象限の問いの除外・lint.rs の memo の数え・列の入力・事前審査の母集団は、この 2 つを引いて label を自分で比べない（C2）。後の行が作る局面の関数も同じ 2 つを引く（5 つ目の読み手を作らない）。起票の門は台帳の bead でなく書きの command の label を見るので、今どおり form.rs の const を引く。
  3. **列の入力から問いを外す**: `is_input` が、label `intake:question` を持つ bead を memo と同じく外す（acceptance の有無に依らない）。外れた bead は memo と同じく `dispatch ls` にも出ない（§2「ここで落ちた bead は ls にも出ない」）。`WaitReason` の値・`dispatch ls` の行の形・`order` は変えない。
  4. **事前審査の母集団から問いを外す**: `population` が契約の行を数えるとき、memo と同じく問いも外す。blocks の到達には問いも今どおり残る（到達は契約の行でない bead も含む・§27 形 1）。問いは祖先の重ね方に入らなくなるだけである。
- 触らない: 起票の門の判定と断り文（問いの欄の検査〔FR81 (a)〕は後の行）・台帳の lint（lint.rs）の数え方（設計 pointer を持つ問いを契約に数えるのは [ledger-form.md](./ledger-form.md) §13 の「変えない」のまま・本行は定義の置き場だけを動かす）・form.rs の `judge` の結果・memo の plan の出力・event の kind・列の待ちの理由と起こす契機・§27 の可視性の段落（`MEMO_LABEL` を dispatch.rs の私有の const と書いた main be51991 の時点の記述として残す）・`crates/scribe2/src/seat/ledger.rs` の台帳の 1 件の型。
- 歯（接頭辞 `pipe_dispatch_intake_label_` と `precheck_intake_label_`・`grep -rn "intake_label" crates/` は 0 件・2026-09-29）:
  - (a) e2e（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`・既存の 2 行の repo と偽の台帳）: 行 a を指す契約 1 件と、label `intake:question` と行 b を指す設計 pointer の acceptance を持つ問い 1 件の台帳で `dispatch ls` を撃つ。契約の reason が `-`（台帳を読めて判定に届いた対照）、問いの bead の `[DISPATCH]` 行が無く、件数の行が `total=1 ready=1`。
  - (b) e2e（回帰の歯・base でも緑）: (a) の問いの代わりに label `intake:memo` と同じ acceptance を持つ bead を置くと、その行が無く `total=1 ready=1`。列の memo の除外を測る歯は今まで 1 本も無かった（e2e の列の台帳の fixture は label が全部空）。
  - (c) in-file（`crates/scribe2/src/pipe/dispatch/precheck.rs` の末尾に歯の区間を新設）: 設計 pointer を持つ契約 c（問い q に blocks される）・label `intake:question` と設計 pointer を持つ問い q・label の無い同じ pointer の bead p の 3 件で `population` を撃つ。契約の行が c と p だけ（p は pointer が解ける対照）で、c の到達が q を持つ。
  - (d) in-file（回帰の歯・base でも緑）: (c) の q の label を `intake:memo` に替えると、行が c と p だけ。
  - 歯は e2e の台帳の 1 件を組む既存の helper と偽の台帳の helper の本文を変えない（label を持つ 1 件は、空の label の列を置き換える新しい小さな helper で組む）。(c)(d) は form.rs の既存の 2 つの const を引き、形 2 の新しい述語を呼ばない（base でも compile が通り、赤は判定の差で出る）。(a)(b) は印の字を自分で書く（器の字を借りない）。
  - 寄せた定義の字を外から測る既存の歯（本文は変えない・verify に載せる・どれも base でも緑）: lint.rs の in-file の `ledger_lint_judge_counts_each_defect_apart`（台帳の JSON の字で memo を数える）・form.rs の in-file の `quadrant_exempts_question_from_the_shaped_population`・e2e の `ledger_memo_plan_carries_label_parent_and_relates_to`（plan の引数の `arg: --labels=intake:memo`）。最後の歯の file は本行で中身を変えないので、write-set に置き場だけの印 `=` で載せる。
  - 判定の順と変異（条件 1 つに歯 1 本）: 列の入力は status → acceptance → memo → 問いの順に見る。問いの判定を外す変異は (a) を、memo の判定を外す変異は (b) を落とす。母集団は status → memo → 問い → pointer の順で、問いを外す変異は (c) を、memo を外す変異は (d) を落とす。問いの述語を常に偽にする変異は (a)(c) と `quadrant_exempts_question_from_the_shaped_population` を落とす。`QUESTION_LABEL` の字を打ち違える変異は (a) だけが落とす（(c) と form.rs の歯は同じ const を引くので落ちない）。`MEMO_LABEL` の字を打ち違える変異は (b) と lint.rs の歯と memo の plan の歯が落とす。
- base で RED の理由: (a) は base の `is_input` が label `intake:question` を見ないので、問いが reason `-` で候補に並ぶ（件数の行が `total=2`・機能不在）。(c) は base の `population` が問いを契約の行に数える（機能不在）。(b)(d) は回帰の歯で base でも緑である。flip-check は file ごとに撃つので、同じ file の (a) か (c) が赤なら file は赤になる。
- 限界:
  - 問いの印と memo の印を併せ持つ bead と、設計 pointer を持つ問いは、列から外れるだけで doctor も名指さない。形を断るのは起票の門（FR81 (a)・ADR-0087 の併せ持ちの断り）を実装する後の行である。
  - 列から外れた問いは `dispatch ls` に出ない（memo と同じ）。問いに誤って契約の acceptance を書いた周に、planner は列からは気付けない。
- 却下:
  - 問いを列に載せて、待ちの理由で見せる。`WaitReason` の閉じた型に値が増え、SRS FR68 の閉じた理由の列にも無い。問いは契約でないので、memo と同じく列の母集団に入れない。
  - bead の型（`issue_type`）で外す。問いは型 task で起票され、器が問いとして読むのは label だけである（ADR-0083）。
  - dispatch.rs に問いの印の const をもう 1 つ置く。定義が 5 つ目になり、ADR-0088 の代償の項と C2 に反する。
  - 契約かの判定を form.rs の 1 関数にまとめて列も使う。列は設計 pointer の無い bead を `NoDesignPointer` で見せる約束（§2）を持ち、form.rs の契約の数え（pointer 行を持つ bead）と母集団が違う。
  - 事前審査の母集団は変えず、列の入力だけ直す。問いが待ち行の祖先に居ると問いの宣言が予想の base に入り、§27 形 1 の「閉じていない契約の行」と食い違う。

## 32. 起動の列の起こす側の周が、受付の断りを契約ごとに event に残す — 断りに入った周と名が変わった周に IntakeRefused を 1 件（契約表の行 ag・[FR68](../../design-intent/spec/srs.html#FR68) / AC60・[ADR-0088](../../design-intent/decisions/ADR-0088-case-positions-are-computed-once-by-the-vessel-and-read-from-one-file.html) (6)）

やさしく言うと: 列が「この契約は受付で断られる」と判じても、今はどこにも残らない（`dispatch ls` を撃ったときに見えるだけ）。後で作る局面の出力（案件ごとの今の段と次の番）が読めるよう、断りを記録に残す。同じ断りが続く間は 1 件だけにして、記録が周ごとに伸びないようにする。

- 何が起きているか（main 7c4ab0a1・verified）:
  - 列が受付の断りを知る所は `crates/scribe2/src/pipe/dispatch/candidates.rs` の 2 つ: `entry_of`（repo の材料を読めない周 :71・契約の生成の断り :78・event log を読めず印を測れない周の `MARK` :62）と `blocker`（交差を読めない周 :146・受付の判定 `judge` の最初の断り :160・受付札の枠 `SLOT` :164）。どれも `WaitReason::Admission` に断りの名（`'static` の字）だけを持たせる。`crates/scribe2/src/pipe/dispatch.rs` の `fire` は起こせなかった候補の理由を `MARK` か `SPAWN` に上書きする（:652）。
  - 名の出所は受付である。`crates/scribe2/src/pipe/cli/intake/refusal.rs` の `refuse` が `Refuse` の名を、`denied` が `Refuse` を持たない断りの名（args・generated・declaration・rules・size・store）を `Denial` の name に置く。`SLOT`・`SPAWN`・`MARK` の 3 語は dispatch.rs の私有の const（:71 / :74 / :78）。
  - 列は受付の判定を置き場なしで撃つ（`blocker` の `Material` の state_dir が None）。ゆえに同時走行の最大値（max-live）・同じ秒の run の重複は列では判じず、列が起こした子の `pipe run` の受付だけが判じる。子の断りは run dir も event も作らず（受付の約束・e2e の 9 file・43 か所の歯が「断りは event を増やさない」を測る）、stderr が `<state_dir>/pipe/launch.log` に `pipe: <理由>` の 1 行で残るだけで、列の次の周はその bead を `launched:<ts>` で待たせる（§17）。
  - 周は event log を 1 回全部読む（`measure` の `read_all`・:535）。読んだ列は `Ledger` の events に入り、`Read`（:514）は台帳と材料だけを `fire` へ返す。
  - kind `IntakeRefused` と key（bead・refuse）は読み書きの両側に在る（[fleet-event-log.md](./fleet-event-log.md) §12・行 f）。本体は `crates/scribe2/src/fleet/mod.rs` の `Case` の `Refused`。`fleet record` はこの kind を断るので、書き手は器の中の列である。
  - 周の数の目安（本 repo の置き場・2026-09-29 に読んだ）: 便の段の記帳（RunCreated・RunStage・RunDone・RunStopped）は 1 日 97〜807 件で、周はその終端ごとに 1 回撃つ。event log は 7.5 MB・33,230 行（2026-09-09〜09-29）で 1 日の伸びは 0.1〜1.0 MB。IntakeRefused の 1 行は約 150 byte。
- 形（番号は done と 1:1）:
  1. **何を記帳するか**: 起こす側の周（`fire`）が、起こし終えた後（`MARK` / `SPAWN` の上書きの後・事前審査の前）に、候補のうち理由が `WaitReason::Admission` で名が列自身の 2 語（`MARK`・`SPAWN`）でないものを候補の順に (bead, 名) で選ぶ。受付の断りの名（`Refuse` の名・受付の材料の断りの名）と受付札の `SLOT` が入る。`HostBusy` と他の待ちの理由は入らない（受付でない・§18）。
  2. **いつ記帳するか（変わった周だけ）**: 選んだ (bead, 名) ごとに、同じ周が読んだ event の列のうち**その bead を持つ最後の行**が「kind `IntakeRefused` で refuse が同じ名」なら書かない。それ以外（その bead の行が無い・最後の行が別の名の IntakeRefused・最後の行が起こした印や release の印や便の段など別の kind）なら 1 件書く。event log を読めなかった周（`read_all` が Err）は 1 件も書かない。
  3. **書く行**: kind `IntakeRefused`・bead = 契約の bead id・refuse = 断りの名・actor = kind の既定（machine）・detail なし・ts は書く時刻。追記は fleet の 1 本（`crates/scribe2/src/fleet/store.rs` の `append`）で、lock の待ち方は `launched` と同じく manifest の `LockPolicy` から読む。policy を読めない・書けない周は、列の結果（候補・理由・起こす便・rc・行）を 1 つも変えず stderr にも足さない。次の周は同じ判定で書き直す（最後の行が同じ断りでないので）。
  4. **置き場**: 形 1〜3 の判定 2 つ（選ぶ純関数と、書くかを決める純関数）と書き手は子 module 1 つ（行 ag の write-set の `+` の file）に置く。dispatch.rs に足すのは mod 宣言・`Read` に周が読んだ event の列（読めない周は None）の 1 欄・`fire` の呼び出し 1 か所だけ。子は候補の列（`Candidate` の slice）を受け、`Turn` の literal を組まず、`EventKind` と `Stage` の match の arm と variant の構築を書かない（[consumer-sync.md](./consumer-sync.md) の行 g・本 doc の行 a・[contract-source.md](./contract-source.md) の 2 行の閉包を広げない）。子は `WaitReason::Admission` を読むので、`WaitReason` を touches に持つ本 doc の行 w の閉包に入る（行 w の write-set に子を `+` で足す・同じ docs PR）。
  5. **書かない口**: 観測の口（`turn`・`dispatch ls`）・台帳を読めない周と実装役の口の無い周（`Unmeasured`）・受付（`pipe run` / `pipe intake` の断り。手で撃つ周も列が起こした子の周も）は書かない。
- 触らない: `WaitReason` の値と `render` の字・`dispatch ls` の行・起こす判定と起こす便・受付の判定と断りの外形（run dir も event も作らない）・kind と key（行 f）・launch.log・事前審査（§27）・局面の出力がこの行を読む形（案件の局面の行の持ち分: 契約の断りの局面は、全部の書き直しで列の周が返す今の理由から決め、局面に入った時刻をその bead の最後の IntakeRefused の ts で読める）。
- 限界:
  - 列が起こした子の `pipe run` の受付の断り（max-live・同じ秒の run の重複・列の後に live になった便との交差など、列が判じない断り）は記帳しない。その bead は `launched:<ts>` で待ち、理由は launch.log の散文にしか無い。max-live を列の理由に上げるのは [gate-cost.md](./gate-cost.md) §24 (4) の約束の行で、`WaitReason::Admission` に受付の名 `max-live` を入れる形で起こせば、本行の書き手は変えずに拾う。
  - 断り → 別の待ち（依存・hold）→ 同じ名の断りと移り、その間にその bead の行が 1 本も無ければ 2 度目は書かない（局面に入った時刻が古いまま残る）。
  - 同じ時に 2 つの周が走ると、どちらも周の始めに読んだ log で判じるので同じ断りを 2 件書きうる（害は重複の 1 行）。
  - 書けなかった周は次の周まで記録が無い（周は時計を持たないので、次の便の終端か手動の 1 周まで）。
- 却下:
  - 毎周 1 件書く（ADR-0088 の代償 N8 をそのまま受ける）。log が周 × 断りで伸び、断りが続く契約 10 本で 1 日最大 8,000 行・約 1.2 MB（今の 1 日の伸びを超える）。tick は log を全部読む（`read_all`）。変わった周だけでも、断りの名と bead と入った時刻は全部残る。
  - 受付（`pipe run` / `pipe intake` の create）の断りで書く。受付の断りが event を作らないことは受付の約束で、e2e の 43 か所の歯が pin する。手で撃つ周を列の周と区別できず（`--drive` は手でも付く）、FR68 の「起動の列の周」の外まで数える。
  - 前の周の断りを state dir の file に持って比べる。event log が同じ事実を既に持つ（新しい状態・C3）。
  - 断りが解けた周に解除の event を書く。kind と key は行 f で閉じていて、key か kind を足すと跨版の面が増える。
  - 子の断りを launch.log から読む。散文を判定に読む（C3.3）。
  - mark と spawn も書く。受付の断りでない（mark は event log を読めない・書けない周で、書いても読めない）。
- 歯（接頭辞 `refusal_record_`（in-file）と `pipe_dispatch_intake_refused_`（e2e）・`grep -rn` はどちらも 0 件・2026-09-29）:
  - in-file（行 ag の `+` の file の歯の区間・event は `Event` の `from_line` で JSON の 1 行から組む・kind の字は歯が自分で書く）:
    - (a) 理由が `Admission` の cap-headroom・`SLOT`・`MARK`・`SPAWN`、`Dependency`・`Hold`、理由なしの 7 候補から、選ばれるのが cap-headroom と slot の 2 件だけで候補の順。
    - (b) その bead の最後の行が同じ名の IntakeRefused → 書かない。
    - (c) 最後の行が別の名の IntakeRefused → 書く。
    - (d) 同じ名の IntakeRefused の後にその bead の DispatchMark（release）の行 → 書く。
    - (e) 同じ名の IntakeRefused の後に別の bead の行だけ → 書かない。
    - (f) その bead の行が無い → 書く。
    - (g) event log を読めない周（None）→ 1 件も書かない。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs` に足す・新しい module は作らない・module の doc の接頭辞の列に 1 つ足す。helper は同じ file の `precheck_repo`・`precheck_row`・`waiting_on`・`hold`・`precheck_turn` と親の `ls`・`reason_of`・`release`）:
    - (A) write-set が base に無い file を素で持つ行 r と、base に在る file の行 h の toy repo・台帳は r と h・h に hold。**周の前に** `dispatch ls` を 1 回撃ち、その後の event log の `"kind":"IntakeRefused"` の行が 0 本（観測の口は書かない。r の理由がこの時点で `admission:` で始まることも測る＝書きうる候補が在る周の 0 本）。続けて起こす側の手動の 1 周を 2 回、`dispatch ls` をもう 1 回撃つ。`dispatch ls` の r の理由が `admission:` で始まり mark でも spawn でもない（fixture の対照）。event log の `"kind":"IntakeRefused"` の行がちょうど 1 本で、bead が r・refuse が `dispatch ls` の理由の `admission:` の後ろの字と同じ。h の行は 0 本。2 周目の後も `dispatch ls` の後も 1 本のまま。
    - (B) (A) の 1 周の後に r へ `release` を打ち、もう 1 周撃つと r の IntakeRefused の行が 2 本になり、2 本目が release の行より後に在る。
  - 判定の順と変異（条件 1 つに歯 1 本）: `Admission` だけを選ぶ条件を外す変異と mark / spawn の除外を外す変異は (a)、いつも書く変異は (b)、名を比べない変異は (c)、最後の IntakeRefused だけを見る変異は (d)、log の最後の行を bead に依らず見る変異は (e)、行が無いと書かない変異は (f)、読めない周に書く変異は (g)、観測の口（`turn`・`dispatch ls`）で書く変異は (A) の周の前の `dispatch ls` の後の本数 0（周の後の `dispatch ls` は最後の行が既に同じ名なので、書き手を呼んでも本数が動かず測れない）、書き手の呼び出しを消す変異は (A)(B) が落とす。
- base で RED の理由: (A)(B) は base の起こす側の周が IntakeRefused を 1 件も書かないので本数が 0（機能不在）。in-file の歯は新しい module と一緒に生まれるので flip-check は測らない（新しい module の file は base へ写せない。flip-check は e2e の file の赤 → 緑で入口を通す）。
- 着地の後: PATH の binary を入れ替えた後から書く（swap-binary.sh は走行中の driver が在れば断る）。今の PATH の binary は行 f の着地より前の build で、この kind を読めない。入れ替えの 1 回で書き手と読み手が揃うので、古い読み手が新しい行を読む周は無い。

## 33. 席の起草の置き場の中間生成物も同じ掃除で消す — 置き場の state dir の下の席ごとの起草の置き場を器が持ち、運転手の終端の周の掃除が同じ lock の中で、置き場の git の木から、名が閉じた 8 つに在り追跡されている file を持たず書きが rules 行の時間無い dir を消す（契約表の行 ah・[ADR-0096](../../design-intent/decisions/ADR-0096-seat-drafts-are-vessel-owned-and-swept-after-writes-stop.html)・FR68 / FR30・裁定 user 2026-09-29T11:43Z・memo `s2-07l.737.14`）

やさしく言うと: 席は試作のたびに repo の写しを作って組み立てるが、写しの置き場は Claude Code の session ごとの一時 dir で、器も人も消さない。組み立ての産物（Rust の target など）が写しごとに溜まり、消費側の席で host の disk を満たした。器が state dir の下に席ごとの起草の置き場を持ち、席の指示文でそこを知らせ（seat-roles.md §31）、§30 の掃除が同じ周・同じ lock・同じ関数で、その置き場の写しからも作り直せる置き場を消す。ただし書きが続いている置き場（組み立て中）は消さず、写しそのものも消さない。

- 出所（2026-09-29）:
  - 消費側の席からの依頼と実測（量は台帳の memo `s2-07l.737.14` にだけ在る・PUBLIC に写さない）: 席の起草の係（subagent）が写しごとに組み立てを撃ち、写しの build の置き場が溜まって disk を満たし、持ち主の承認で手で消した。組み立ての作り直しの費用は小さい（同じ memo）。
  - 持ち主の裁定 user 2026-09-29T11:43Z（逐語は台帳の memo の notes と `RulingReceived` の event）: 席の起草の置き場の、書きが 6 時間無い中間生成物を器の掃除で消してよい（憲法 A1 の「消す」の承認）と、N = 6 時間の値の裁定 id。**退役の時に即座に消すことと、木の写しそのものを消すことは承認の外**。
  - 本 repo の現物（PUBLIC）: 席が切った worktree が anchor の `.worktrees/` の下に 19 本在り、うち 1 本は target が 1.1 GB で最新の書きが 6 時間より前。§30 の掃除は便の記録を持たない木を見ないので、これも残り続ける（限界へ・本 § は置き場を state dir の下へ移す）。
- 現物（main c14588cd・verified）:
  - `crates/scribe2/src/pipe/sweep.rs` の `sweep` は pipe の dir の無い置き場で何もせず（:39-41）、置き場の lock（`LOCK`・`acquire_with` の `Reclaim::DeadOnly`）を取ってから、置き場の replay を読めない周は行を出さずに抜ける（:47）。`swept` は木ごとに `git ls-files -z` を 1 回撃ち（:78）、`.git` と dir でない entry を飛ばし（:99・`DirEntry` の型は symlink を辿らない）、名が `NAMES` に在り追跡されていない dir を消し（:105）、年齢は見ない。
  - 呼び手は `crates/scribe2/src/pipe/cli.rs` の `dispatch` の 1 か所（:241-243）で、`TERMINALS`（run / resume / land / stop / retire）の周に subcommand の rc に依らず撃つ。手動の 1 周（`pipe dispatch`）・関門の記帳・管理 tick は撃たない。
  - 席の置き場は `crates/scribe2/src/seat/mod.rs` の `seat_dir`（:207-215・`tick_path` の親）で、根の名 `SEAT_DIR` は `crates/scribe2/src/seat/inject.rs` の私有の const（:37）。根を返す関数は無い。`seat retire`（`crates/scribe2/src/seat/role.rs` の `retire`・:295-320）は event を 1 件足すだけで席の dir を動かさない。
  - 整数の rules 行の pipe の読み手は `crates/scribe2/src/pipe/cli/args.rs` の `int_row`（:60）。`RuleKind` の `ALL` で `PipeCiPollS` の直後（`LedgerTimeoutS` の前）の並びを pin する歯は無い（末尾と Seat 系の並びは 5 本の歯が pin する）。
  - 埋め込みの manifest は 79 行・kind 77（`crates/scribe2-boundary/tests/e2e/rules/embedded.rs` :768 / :825 と外形 snapshot）。
- 形（番号は行 ah の done と 1:1）:
  1. **置き場**: 席ごとの起草の置き場は `<state_dir>/seat/<潰した target>/drafts/`（`seat_dir` の子・名 drafts は席の置き場の既存の名の並び〔小文字の 1 語〕に揃える）。`crates/scribe2/src/seat/mod.rs` に名の const と「置き場と target から起草の置き場の path」を返す関数を足し、置き場の根（`<state_dir>/seat`）は `SEAT_DIR` の可視性を親 module へ上げて同じ file の関数 1 本で返す（dir 名の字を 2 面に持たない・`seat_dir` の注記と同じ理由）。器は起草の置き場の dir を**作らない**（席の git が作る）。
  2. **起草の木**: 置き場の根の直下の dir（symlink は辿らない）ごとに起草の置き場（形 1）を見て、その直下の子のうち `.git`（file か dir）を持つ dir（git の worktree か clone・`.git` を持つ写し）だけを木とする。`.git` を持たない子の dir は触らずに数える（nogit）。symlink の子・file の子は木にも nogit にも数えない。
  3. **書きの線と掃き方**: 掃除の 1 関数 `swept` に「書きの線」（`Option` の時刻）を 1 つ渡す。便の木（§30）は線を持たず今のまま消す。起草の木は線 = 今 − N 時間（N は形 5 の rules 行）。名が `NAMES` に在り追跡されている file を持たない dir のうち、線を持つ木では、その dir 自身と下の全 entry（file と dir・symlink は辿らずに symlink そのものの mtime を読む）の mtime の最新が線より前のものだけを消す。線以後の entry を 1 つ見つけたらその dir の走査を打ち切って残し、下へは降りない。mtime か dir を読めない entry が在れば残して失敗に数える（測れないを「古い」に読み替えない・C10）。消し方・降り方・追跡の判じ（木ごとの `git ls-files` の 1 回）は §30 形 2 のまま。
  4. **撃つ周と lock**: `sweep` の中で、便の木の後に、同じ lock の中で撃つ（2 つ目の消す仕組みと lock を足さない・C17）。置き場の replay を読めない周も起草の木は掃く（便の段を読まない）。撃つ周は §30 形 3 のまま（`TERMINALS` の周・`--state-dir` だけで撃つ）。`cli.rs` は `sweep` に manifest を渡す（rules 行を読むため・引数 3）。管理 tick には足さない。
  5. **rules 行**: 行 seat.drafts_stale_h（kind SeatDraftsStaleH・Int・値 6（時間）・enabled・裁定 id user 2026-09-29T11:43Z・裁定日 2026-09-29）を manifest の `pipe.ci_poll_s` の直後に、kind を `ALL` の `PipeCiPollS` の直後に足す（並びを pin する既存の歯に当たらない位置）。読み手は `sweep.rs` の const の id と `int_row` の 1 回で、**起草の木が 1 本以上在る周だけ読む**。読めない周（無い・不発効・整数でない）は起草の木を 1 本も撃たない（既定値に倒さない）。値 0 は「線 = 今」（書きの有無を問わず掃く）で、値を変えるのは C5 の裁定だけ。
  6. **stderr の 1 行**: §30 形 5 の行 `sweep: removed=<n> runs=<k> failed=<m>[:<名>,…]` のまま、置き場の根の下に起草の置き場が 1 つでも在る周は末尾に ` drafts=<t> nogit=<p>` を足す（t = 1 つ以上の dir を消した起草の木の数・形 5 で読めない周は語 `no-rule`、p = 形 2 の nogit の数）。`removed` は便の木と起草の木の合計、`failed` と名の列は両方の木（起草の木の名は `<潰した target>/<木の dir 名>`）。行を出すのは §30 の周（dir を消した・失敗が在る・lock を取れない）に加えて t が `no-rule` の周だけ（nogit だけの周は出さない）。起草の置き場が無い周の行の字は 1 byte も変わらない。stdout・event・rc は変えない。
  7. 変えないもの: `NAMES` の 8 つ・lock の名と取り方・便の母集団と `live` の判定・`tree_of`・撃つ周・退役の move・席の登録と `seat retire`・管理 tick・rules 行 host_guard.git・event の kind。
- 触らない: `crates/scribe2/src/pipe/mod.rs`（余地 150）・`crates/scribe2/src/pipe/cli/state.rs` の `live`・`crates/scribe2/src/fleet/store.rs` の lock の実装・§30 の歯 2 本（字も期待も不変）。
- 却下（[ADR-0096](../../design-intent/decisions/ADR-0096-seat-drafts-are-vessel-owned-and-swept-after-writes-stop.html) の比較と同じ）: 今のまま人が消す／Claude Code の一時 dir を器が掃く（別の道具の内側の path で user 名と uid を含む・器は env を読まない・N2・CON2）／`seat retire` の時に即座に消す（承認の外・retire は event だけ）／管理 tick で掃く（写しの走査を 15 秒の周期に載せる）／cargo の lock file の有無で組み立て中を判じる（Rust だけに効く・lock file は組み立ての後も残る）／木の写しごと消す（承認の外・commit していない仕事を失う・N1）／置き場の path を指示文に字で書く（穴を足さない・席が潰し方と state dir を自分で解く）／起草の置き場専用の 2 つ目の掃除（C17）／読みの新しさ（atime）も数える（mount の設定で当てにならない・host で分岐する・N2）。
- 限界:
  - 便を走らせない置き場（終端の周が来ない project）の席の起草の置き場は掃かれない。
  - 書きだけを数える: 組み立てずに test や読むだけの使用が続いた置き場も N 時間で消え、次の組み立てで作り直す（数分・正しさは変わらない）。
  - 判じと消しの間に組み立てが始まる競合は排他でない（その組み立ては落ちうる・撃ち直せば戻る）。
  - 席が指示に従わず一時 dir や anchor の `.worktrees/` に写しを置けば掃かれない。指示文の 1 行は案内で、置き場を強いる門は無い（門を足すかは別 memo・report の論点）。
  - `.git` を持たない写しと木の写しそのもの（worktree / clone の本体）は消さない。disk は木の分だけ残る。
  - 起草の木の中の入れ子の repo の列の名の dir は、外の木の `git ls-files` で判じるので追跡されていない扱いで消えうる（§30 と同じ）。列の名で置かれた未追跡の物は中身ごと失う（ADR-0081 の代償のまま）。
  - 大きい build の置き場は書きの線を判じるのに entry を全部 lstat する（消す直前の 1 回と、線以後の entry が深い所に在る周）。
  - `sweep.lock` の本文の書きの前に死んだ掃除が残す空の lock は、rules 行 `fleet.lock_stale_ms` を越えるまで起草の置き場の掃除も止める（[fleet-event-log.md](./fleet-event-log.md) §11 の回収で越えた周に外れ、永久には止まらない）。skipped=lock が続く周を知らせる口は無い（memo `s2-07l.736.8` の候補 3）。
- 歯（接頭辞 pipe_sweep_drafts_ と rules_drafts_stale_・どちらも `grep -rn` は crates / docs で 0 件・2026-09-29）:
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/stop.rs` の §30 の歯の後ろ・helper `put_file` と `sweep_line` を使う・新しい module は作らない）。起草の木は toy repo から起草の置き場へ `git worktree add --detach` で切り、「古くする」は std の File の set_modified で entry を子から先に N 時間より前へ戻す（dir も File として開いて同じ呼び出し・e2e の fleet.rs に set_modified の前例）。終端は live な便 1 本の `pipe stop`。
    - (a) 席 2 つの起草の置き場: 1 つ目の木の 7 時間前の `target/`・書いたばかりの `node_modules/`・追跡されている `docs/target/keep.md` を持つ 7 時間前の `docs/target/`・列に無い 7 時間前の `out/`、同じ置き場の `.git` を持たない写しの 7 時間前の `target/`、2 つ目の木の 7 時間前の `.venv/`。終端の後、1 つ目の `target/` と 2 つ目の `.venv/` だけが消え、ほかと木の追跡されている file と `.git` は残り、stderr の sweep: の行が `sweep: removed=2 runs=0 failed=0 drafts=2 nogit=1`、stdout に sweep: が無い。
    - (b) 7 時間前の `target/` の下に書いたばかりの file が 1 つ（`target/debug/deps/` の中）・7 時間前の `.mypy_cache/` の深い所に書いたばかりの空の dir が 1 つ（親の dir は古くし直す）・7 時間前の `__pycache__/`。`__pycache__/` だけが消え、行が `sweep: removed=1 runs=0 failed=0 drafts=1 nogit=0`。
    - (c) `--rules` に行 seat.drafts_stale_h を持たない tmp manifest（`ceiling_rules` の本文に stop が読む `pipe.stop_grace_ms` の行を足した写し）: 7 時間前の `target/` が残り、行が `sweep: removed=0 runs=0 failed=0 drafts=no-rule nogit=0`。
    - (d) (c) の写しに行 seat.drafts_stale_h = 0 を足した tmp manifest: 書いたばかりの `target/` の中に、木の外の dir を指す symlink を置き、木の外の dir の file の mtime を 1 日先へ進める。`target/` が消え、木の外の file は残る（書きの線の走査は symlink を辿らず、消しも辿らない）。
  - e2e（`crates/scribe2-boundary/tests/e2e/rules.rs`・`rules_ci_poll_row_follows_the_ci_wait` の後ろ）: (e) 埋め込みの manifest に行 seat.drafts_stale_h が kind SeatDraftsStaleH・形 Int・値 6・enabled・裁定 id と裁定日で 1 本在り、行は `pipe.ci_poll_s` の直後・kind は `ALL` の `PipeCiPollS` の直後で字面から引け、`int_row` で 6 が読め、文字列の値の写しは「形と合わない」で断られる。
  - 直す既存の歯（同じ便）: `rules_embedded_manifest_is_valid_and_covers_all_kinds`（行数 79 → 80）・`rules_embedded_manifest_declares_one_capability_row_per_role`（kind 77 → 78）・`rules_external_form` の snapshot（`rows=80 kinds=78` の 2 行）。直した後の歯は base で落ちる（base は 79 / 77）ので retroactive の札は要らない。§30 の `pipe_sweep_` 2 本は字も期待も変えず緑（起草の置き場を持たない置き場の行は 1 byte も変わらない）。
  - 判定の順と変異（条件 1 つに歯 1 本）: 起草の木を掃かない → (a)・線を見ずに消す → (b) の target・dir の mtime を数えない → (b) の .mypy_cache・`.git` を持たない写しも木にする → (a) の nogit・行を読めない周に既定値へ倒す → (c)・走査で symlink を辿る → (d)・行を末尾でなく別の位置で読む（ALL の位置）→ (e)。
- base で RED の理由: (a)(b)(d) は base の掃除が起草の置き場を見ないので古い dir が残る（機能不在）。(c) は base が行を出さない（sweep: の行が空）。(e) は base に行も kind も無い（`RuleKind::parse` が None）。直す既存の歯は base の本数で落ちる。
- 順: ADR-0096 と本 § と seat-roles.md §31 と行 ah / y を同じ docs PR で land → 行 ah の便 → 行 y の便（行 y の雛形の pointer が本行の rules 行を名指すので、台帳で行 y の bead を行 ah の bead の blocks に置く）。束 E の核の後に置く（memo の昇格条件・priority は束 E の行より下）。
- 着地の後: 掃除の振る舞いが変わるので PATH の binary を `swap-binary.sh` で入れ替える（走行中の運転手が在れば断られる）。入れ替えの前の運転手は起草の置き場を掃かない。消費側の席への知らせは行 y の着地の後に 1 回（置き場の path は指示文が渡す）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "pipe dispatch の本体 — 列の導出（台帳 + 設計 pointer + 審査 FAIL の列外）・順序の 1 関数（first → priority → 起票順）・起動条件は intake の判定を再利用・first / hold / release の印の event kind 1 つ・dispatch ls"
req = ["FR30", "FR39", "FR49"]
section = "3"
touches = ["crate::fleet::EventKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/fleet/cli.rs", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", ".config/nextest.toml", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_"]
size = "M"
done = "偽の台帳と偽の live 便で、交差する便は Overlap で待ち交差しない便だけが起動の構築点に届き、first が priority より先に来て hold は起こさず、直前の便が Reviewed FAIL の契約は同じ sha では ReviewFailed で列外、台帳が読めない周は UNMEASURED で 0 本"

[[contract]]
id = "b"
title = "契機 — 便の終端（land / stop / retire・run と resume の終端）の直後・pipe dispatch の手動 1 周・first / release の記録の直後に同じ dispatch::turn を撃つ（tick は無い）・1 周の repo の材料の読みは 1 回（候補ごとに読み直さない）"
req = ["FR30", "FR68"]
section = "5"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/retire.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", ".config/nextest.toml", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_dispatch_"]
size = "S"
done = "偽 remote の toy repo で land と stop の終端の直後に列が 1 周撃たれて交差の解けた便が起こされ（RunCreated が増える）、契機が重なっても受付の入口の排他で便は 1 本に留まり、着地した bead は起こし直されず契約の行を改訂して sha が動くと列に戻り、run の終端の 1 周は自分の道具で便を起こし、pipe dispatch の手動 1 周と first / release の記録の直後も同じ関数を撃ち、dispatch ls は 1 本も起こさず、終端の中の 1 周が失敗しても終端の rc は変わらず、候補 N 件の 1 周で repo の材料の読みが 1 回（母集団 = 候補数）"
depends = ["a"]

[[contract]]
id = "d"
title = "driver の死亡 — 札の書き・消し、turn 関数の起こし直し（pipe resume）、record token resumed:<m>、base_of_run の typed 化"
req = ["FR68", "FR14", "FR50"]
section = "5"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/src/fleet/store.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_driver_"]
size = "M"
done = "driver を殺した便に dispatch の 1 周を撃つと pipe resume が 1 回起きて record に resumed:1・その便が 1 段進み、札の無い live 便と Blocked の便は起こし直さず、生きている driver が握っている札は取れず・古さでも奪われず、正常に抜けた driver は自分の札を外す"
depends = ["a", "b"]

[[contract]]
id = "g"
title = "dispatch の歯の診断と結合の切り離し — assert の文に rc / stdout / stderr を写し、印の直後の 1 周は子 process を起こさない台帳で測る（歯だけ・src/ は触らない）"
req = ["FR68", "NFR4"]
section = "8"
write-set = ["crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_dispatch_marks_fire_without_children"]
size = "S"
done = "印の直後の 1 周の歯が便を 1 本も起こさずに（RunCreated 0・子 process 0）release の直後の dispatch= 行を測り、既存の手動の 1 周の歯は起こした効果だけを測り、dispatch の歯の assert の文が落ちた周の rc と stdout と stderr を写し、src/ は 1 行も変わらない"
depends = ["b"]

[[contract]]
id = "e"
title = "便の自走 — pipe run / pipe resume の flag --drive を持つ driver だけが終端の 1 周で自分の便を次の driver に渡し（前進 ∧ 待ちでない ∧ 終端でない周だけ・渡さない理由は record token）、列と起こし直しが起こす便は --drive を持つ"
req = ["FR68", "FR14", "FR50"]
section = "5"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/size.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_drive_"]
size = "M"
done = "--drive を持つ pipe run が toy repo の契約 1 本を偽 runner と偽 lens で人の手なしに Landed まで通し（run_all の 1 process・record は drive=settled）、--drive を持つ pipe resume が段ごとに次の driver へ継いで Landed まで通り、--drive の無い run / resume は 1 段で止まり既存の歯は 1 本も変わらず、Blocked の便と段の動かなかった周は渡さず理由が record に載り、列の起こし直しが起こす resume は --drive を持ち殺した driver の便が Landed まで通り、usage の外形 snapshot が更新される"
depends = ["d", "g"]

[[contract]]
id = "h"
title = "列へ戻す印 — Settled の判定が、同じ契約 file を持つ直前の便の最後の記帳より後の release を見て 1 回だけ列外を外す（Landed と審査 FAIL は外さない・新しい event kind は足さない）"
req = ["FR68", "FR49", "NFR4"]
section = "12"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_release_requeues_"]
size = "S"
done = "偽の台帳と run dir の fixture で、Failed で終端した便の bead が Settled で列外に居り、その後の release で同じ sha のまま列に戻って dispatch ls の reason が - になり、起こし直した便が同じ sha でまた終端に着くと再び Settled になり、終端より前の release は効かず、Landed の便と審査 FAIL の便は release の後も Settled のままで、Stopped の便と gate の判定で終端になった便は戻り（母集団 = 終端の段の種類）、戻すかどうかの段の弁別は段の型の網羅の match 1 本で持つ"

[[contract]]
id = "i"
title = "起動の失敗の理由と repo の名指し — spawn が実装役の stderr を捕らえて run dir の log に残し呼び手の stderr にも流す・pipe の --repo の読み手を 1 本に畳んで値を std::path::absolute で絶対にする"
req = ["FR30", "NFR4"]
section = "12"
write-set = ["crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2-boundary/tests/e2e/pipe/launch_failure.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_spawn_runner_stderr_", "cargo nextest run -p scribe2 --no-tests=fail pipe_repo_relative_"]
size = "S"
done = "stderr に 1 行書いて rc 2 で落ちる偽 runner の便が Failed detail=runner-rc:2,commits:0 に着いた後、run dir の stderr の log にその 1 行が見出し行付きで残り、呼び手の stderr にも同じ行が出て、stderr が空の周は file が作られず、stderr に書いても rc 0 で commit 1 の偽 runner は Implemented に着き、--repo を相対 path で渡した pipe run が絶対 path で渡した周と同じ worktree の場所と同じ段に着く"

[[contract]]
id = "j"
title = "関門が開いた待ちの便の再開 — 起こし直しの候補に回答済みの Questioned と承認済みの Blocked（生きている driver の居ない便）を足し、pipe answer / pipe approve の記帳の直後にも同じ 1 周を撃つ（関門の判定は resume の入口と同じ 1 本・新しい段も event kind も足さない）"
req = ["FR68", "FR32", "FR16"]
section = "13"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_waiting_gate_", "cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_driver_", "cargo nextest run -p scribe2 --no-tests=fail pipe_question_", "cargo nextest run -p scribe2 --no-tests=fail pipe_approval_"]
size = "M"
done = "偽の台帳と偽 runner の toy repo で、回答済みの Questioned の便（driver の札なし）が手動の 1 周で --drive 付きの resume で起こされて先の段へ進み（resumed:1）、未回答の Questioned の便と古い質問に回答が在っても最新の質問が未回答の便は起こされず（resumed:0）、承認済みの Blocked の便も同じく起こされ、札の 4 値（無い・所有者が死んでいる便は起こす／所有者が生きている・在るのに読めない便は触らない）がそれぞれ測られ、道具を渡した pipe answer と pipe approve の記帳の直後に同じ 1 周が撃たれて便が進み、道具を渡さない pipe answer は記帳だけで rc 0 のまま、回答と承認の stdout は記帳の 1 行だけで、1 周が失敗しても回答の rc は変わらず、候補の選別の pure な fn が段の前進の 3 値のそれぞれで測られ（段を前へ進めた driver の周は関門の候補をそのまま起こし、同じ段のままと段が戻った driver の周は 0 本にし、driver でない周は絞らない・in-file の歯）、段を前へ進めた driver の終端の 1 周が別の回答済みの便を起こし（resumed:1）、関門の判定は resume の入口と列が同じ述語 1 本を呼び、待ちの段でない便の起こし直しの規則と未承認の Blocked を外す既存の歯（pipe_dispatch_driver_ の歯）と、質問と回答の歯（pipe_question_）と承認の歯（pipe_approval_）は測っている約束を変えずに緑のまま"

[[contract]]
id = "k"
title = "台帳の読みの子 process を名指された repo の中で撃つ — 台帳の読みが cwd を引数で取り、列は --repo の値を・SessionStart は payload の cwd を渡す（器は cwd を推さない・close は上限の余地が足りず別の行・新しい断りも rules 行も足さない）"
req = ["FR68", "FR30", "NFR4"]
section = "14"
write-set = ["crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/hook.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_ledger_cwd_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail hook_brief_ledger_is_unknown_when_the_client_is_unreadable"]
size = "S"
done = "cwd を書き出してから台帳の JSON を吐く偽の台帳 client を --bd で渡し process の cwd を別の dir にしたまま pipe dispatch ls --repo <toy> を撃つと子の見た cwd が toy repo になり（process の cwd でない）、--repo を相対 path で渡した周も子の見た cwd が同じ絶対 path になり、無い dir を --repo に渡した周は列が DISPATCH-UNMEASURED の行で 0 本になり（DISPATCH-NONE と融合しない）、SessionStart の {ledger} の行は 1 字も変わらず、台帳 client の引数と待ち上限の rules 行と LedgerError の 2 値と件数の 1 行の字面は変わらない"

[[contract]]
id = "l"
title = "席が測り直して PASS になった Gated の便を列が起こし直す — 起こし直しの候補に「Gated ∧ verdict が PASS ∧ 札が無いか所有者が死んでいる」を 1 枝足す（既存の枝は不変・PASS 以外と読めない verdict は候補にしない・段を前へ進めた driver の周だけ起こす）"
req = ["FR68", "FR14", "NFR4"]
section = "15"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_gated_pass_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_driver_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_waiting_gate_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_drive_resume_hands_off_only_with_the_flag"]
size = "S"
done = "flag の無い driver の終端の 1 周は自分が段を進めた便を新しい枝の候補にせず（歯 pipe_dispatch_drive_resume_hands_off_only_with_the_flag が緑のまま・flag の無い resume が PASS の Gated で抜けた直後に自分の便が起きないことを新しい歯が測る）、verdict の読みは既存の読み手（land.rs の verdict_of・可視性は不変）を列から呼ぶだけで足り、偽の台帳と偽 runner の toy repo で、verdict が PASS ∧ 札の無い Gated の便が手動の 1 周で --drive 付きの resume で起こされて先の段へ進み（resumed:1）、verdict が INCONCLUSIVE の便と verdict を読めない便は起こされず（resumed:0）、札の所有者が生きている便と札が在るのに読めない便は触らず（母集団 = 札の 4 値）、札の所有者が死んでいる Gated の便は verdict が PASS の周も INCONCLUSIVE の周も今までどおり起こされ、段を前へ進めなかった driver の終端の 1 周はこの候補を 1 本も起こさず、待ちの段の候補の規則と待ちの段でない Gated 以外の便の規則を測る既存の歯（pipe_dispatch_driver_ と pipe_dispatch_waiting_gate_）は測っている約束を変えずに緑のまま"

[[contract]]
id = "m"
title = "列外の鍵に審査役へ渡る材料を含める — Reviewed で終端した便の鍵に、行の section が指す § の本文（審査の材料の dir の写し）を足す（§ の読みは審査と同じ 1 本・写しが無い周は契約 file だけの鍵に倒す・Reviewed 以外の段の鍵は不変・新しい file も field も足さない）"
req = ["FR68", "FR49", "NFR4"]
section = "16"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_section_key_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_release_requeues_"]
size = "M"
done = "偽の台帳と run dir の fixture で、審査 INCONCLUSIVE で終端した Reviewed の便の契約が § の本文を直した後の 1 周で列に戻って dispatch ls の理由が値なしの欄になり、§ も契約 file も変わっていない周は列外のままで、審査 FAIL で終端した便も § を直せば戻り、§ の写しを持たない便と写しが在るのに読めない便は契約 file だけの鍵で今までどおり列外になり（母集団 = 写しの 3 値）、Landed の便は § を直しても戻らず（母集団 = 終端の段の種類）、§ の本文を 1 文字だけ変えた周も戻り、Reviewed 以外の段の鍵と dispatch ls の理由の字面と event kind は変わらず、release の印の既存の規則を測る歯（pipe_dispatch_release_requeues_）は測っている約束を変えずに緑のまま"

[[contract]]
id = "n"
title = "起こした便が受付に届かない周は同じ bead を起こし直さない — Mark に Launched を足して起こす前に印を書き、最新の Launched の後に RunCreated も Release も無い bead は候補にせず、子の stderr を state dir の launch.log に残す（約束の行の形）"
req = ["FR68", "NFR4"]
section = "17"
size = "S"

[[promise]]
of = "n"
n = 1
text = "起こす前に bead 名義の DispatchMark（mark = Launched・detail = 起こす subcommand の 1 語）を既存の mark の口で書き、書けない周は起こさない"
files = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/dispatch.rs"]
symbols = ["crate::fleet::Mark", "+Mark::Launched", "marks_of("]
teeth = ["pipe_dispatch_launched_mark_is_written_before_the_child_is_spawned"]
place = "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"
fixture = "偽の台帳に ready の bead 1 本と toy repo を置いて pipe dispatch の 1 周を撃つ。負の枝は event log を読み取り専用にして印が書けない周"
expect = "event log に bead 名義の DispatchMark mark=launched が RunCreated より前の行として在り、印が書けない周は子が起きず dispatch=started:0 で ls の理由が admission:mark（測れない側）に出る"

[[promise]]
of = "n"
n = 2
text = "最新の Launched より後に RunCreated も Release の印も無い bead は起こさず、dispatch ls の理由が launched:<ts> になる（WaitReason に 1 値 Launched）"
files = ["crates/scribe2/src/pipe/dispatch.rs"]
symbols = ["crate::pipe::dispatch::WaitReason", "+WaitReason::Launched"]
teeth = ["pipe_dispatch_launched_bead_is_not_relaunched_until_run_created_or_release"]
place = "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"
fixture = "偽の台帳の ready の bead に DispatchMark launched だけを積んだ event log で 2 周目を撃つ。対照は Launched の後に RunCreated を積んだ log と、Launched の後に Release を積んだ log の 2 つ"
expect = "印だけの周は起こさず ls の理由が launched:<ts>、RunCreated の後は理由が live の側（overlap）に変わり、Release の後の周は起こす（started:1）"

[[promise]]
of = "n"
n = 3
text = "spawn_self の stderr を <state_dir>/pipe/launch.log に append し、file を開けない周は null に落として起動を止めない"
files = ["crates/scribe2/src/pipe/dispatch.rs"]
teeth = ["pipe_dispatch_launch_log_keeps_the_child_stderr"]
place = "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"
fixture = "偽の bd が 1 回目の呼び出しだけ答えて 2 回目以後は rc 1 で断る（親の周は台帳を読めて起こし、子の intake は台帳で断って stderr に 1 行書く）。負の枝は launch.log の path を dir にして開けなくする"
expect = "launch.log に子の断りの 1 行が append され、開けない周も子は起きて dispatch=started:1（起動を記録の失敗で止めない）"

[[contract]]
id = "o"
title = "列が起こす前に器の健康の遮断器を通し（gate と同じ 1 関数・Wait の周は host-busy で 1 本も起こさない）、受付で止まったまま運転手の札が無いか死んでいる便を live に数えない（約束の行の形）"
req = ["FR68", "FR39", "NFR4"]
section = "18"
size = "S"
depends = ["n", "q"]

[[promise]]
of = "o"
n = 1
text = "列の 1 周は起こす前に health::now を読み、act が Wait の周は 1 本も起こさず WaitReason::HostBusy（ls の理由 host-busy）、Unmeasured は act のとおり起こす。per_core は gate と同じ 2 行を同じ 1 関数で読む（breaker を health.rs 側へ寄せる）"
files = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/health.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/cli/step.rs"]
symbols = ["crate::pipe::dispatch::WaitReason", "+WaitReason::HostBusy", "crate::pipe::health::Breaker"]
teeth = ["pipe_dispatch_host_busy_round_launches_nothing"]
place = "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"
fixture = "rules fixture の host.runnable_per_core を 0（閾値 0 = 常に Busy）にした周と既定の値の周の対で、偽の台帳に ready の bead 1 本を置いて 1 周を撃つ"
expect = "0 の周は dispatch=started:0 で全候補の ls の理由が host-busy、既定の周は started:1"

[[promise]]
of = "o"
n = 2
text = "live の Stage::Intake の枝を運転手の札で読む: Live なら true・Dead / Absent なら false・Unreadable なら None（他の段の枝は不変・新しい probe は足さない）"
files = ["crates/scribe2/src/pipe/cli/state.rs"]
symbols = ["live(", "crate::pipe::Ticket"]
teeth = ["pipe_dispatch_intake_run_without_a_live_driver_is_not_live"]
place = "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"
fixture = "RunCreated stage=Intake だけを持つ run を state dir に置き、札の 4 形（無い・死んだ pid・生きた pid〔歯の自分〕・読めない）で対照。同じ write-set の別 bead を候補にする"
expect = "無い・死んだ周は overlap にならず候補が起きて started:1、生きた周は理由 overlap:<run>、読めない周は起こさず理由が unmeasured の側"

[[contract]]
id = "p"
title = "運転手の終端と「起こす便 0 ∧ 候補あり」を登録 row（Role::Orchestrator・anchor = repo）の席の pane へ 1 行で知らせる — 送達は seat::inject::deliver_within の 1 関数・結果は stdout の notify= の 1 行・Landed と PASS は送らない（約束の行の形）"
req = ["FR30", "FR68"]
section = "19"
size = "S"

[[promise]]
of = "p"
n = 1
text = "運転手の終端の周で最後の段が Reviewed / Gated の FAIL・INCONCLUSIVE、Failed、Questioned、Stopped のとき、fleet の replay の State.registrations から (Role::Orchestrator, anchor = repo) の最新 row の target へ 1 行を deliver_within で送り、stdout に notify=<delivered|refused:<理由>|unconfirmed|no-seat> を残す（row が無い周は送らず no-seat・便の rc は変えない）"
files = ["crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/notify.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "+crates/scribe2-boundary/tests/e2e/notify.rs"]
symbols = ["seat::inject::Request"]
teeth = ["pipe_notify_terminal_failure_reaches_the_registered_seat_pane", "pipe_notify_without_a_registered_seat_reports_no_seat"]
place = "+crates/scribe2-boundary/tests/e2e/notify.rs"
fixture = "偽の tmux（send-keys の引数を file に記録する script）を PATH に置き、SeatRegistered の row（role orchestrator・anchor = toy repo・target = 任意の pane 名）を state dir に積んだ上で、live な run に pipe stop --run を撃つ（Stopped は終端の 1 つ）。負の枝は row を積まない"
expect = "記録に send-keys が 1 回だけ在り payload が bead と run と Stopped を含む 1 行で stdout に notify=delivered、row 無しの周は send-keys 0 回で notify=no-seat"

[[promise]]
of = "p"
n = 2
text = "同じ周の列の結果が起こした便 0 ∧ 候補 1 本以上のとき、同じ宛先へ idle の 1 行（ready=<本数> launched=0 reason=<先頭の候補の理由>）を送る（Landed と PASS の終端でも列が idle ならこの 1 行だけ送る）"
files = ["crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/notify.rs", "+crates/scribe2-boundary/tests/e2e/notify.rs"]
teeth = ["pipe_notify_idle_round_reports_ready_count_and_top_reason"]
place = "+crates/scribe2-boundary/tests/e2e/notify.rs"
fixture = "偽の台帳の ready の bead 1 本を hold にした state dir（起こす 0 ∧ 候補 1）と登録 row と偽の tmux を置き、pipe stop --run の終端を撃つ。負の枝は候補 0 の台帳"
expect = "idle の 1 行に ready=1 launched=0 reason=hold が在り、候補 0 の周は idle の行を送らない（send-keys は終端の 1 行だけ）"

[[contract]]
id = "q"
title = "pipe/dispatch.rs の「台帳から候補を組む」群のうち 16 item（約 325 行・Ledger と Marks の 2 型は親に残す）を子 module candidates へ割る — 純移動・親に増えるのは mod 1 行と use 2 文・in-file の歯 14 本は動かさない・e2e の歯の file は 1 byte も変えない"
req = ["FR68", "NFR4"]
section = "20"
write-set = ["-crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/xtask/src/env_reads.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_dispatch_drive_ pipe_dispatch_launched_ pipe_dispatch_marks_ pipe_dispatch_order_ pipe_dispatch_release_ pipe_dispatch_section_ pipe_dispatch_wait", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_", "cargo nextest run -p xtask --no-tests=fail env_reads_passes_on_core_with_a_nonempty_population"]
size = "S"
done = "(1) 16 item（Ledger と Marks を除く）が名・本文・順序を変えずに子へ移り、Ledger と Marks は親に残って field の可視性が不変、flip-check の moved の機械証明が残差 0 (2) 親に増えるのは mod 1 行と use 2 文だけで turn / fire / revivals の本体は不変、#[cfg(test)] の use は歯の区間の直前（親の #[cfg(test)] + mod tests の直前）に在り、file の最初の行頭 #[cfg(test)] は src の本体の全 item より後＝xtask の env_reads_passes_on_core_with_a_nonempty_population が緑（母集団 6・base と同じ） (3) in-file の歯 14 本と e2e の pipe_dispatch_ の歯が 1 字も変わらず緑 (4) 親の行数が約 1090 で余地が 400 以上 (5) tests/e2e/pipe/dispatch.rs と crates/xtask/src/env_reads.rs の diff が 0 行"

[[contract]]
id = "r"
title = "通知の送達を消費で閉じる — notify は置き場を渡して自席の記録と打刻を測り、settle が Queued の周は入力欄の残りがこの周の本文なら Enter を 1 回だけ再送して同じ窓で settle し直し、stdout に notify=delivered consumed=<true|false|unknown[:理由]> を出す"
req = ["FR30", "FR68"]
section = "21"
write-set = ["crates/scribe2/src/pipe/notify.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/seat/inject.rs", "crates/scribe2-boundary/tests/e2e/notify.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_notify_queued_ pipe_notify_delivery_ pipe_notify_foreign_"]
size = "S"
done = "(1) notify の Request が運転手の置き場を StateDir（Provenance::Flag）で持ち、送達後に tick.jsonl へ自席の記録が 1 行増える (2) settle が Queued で窓を閉じた周に入力欄の残りがこの周の本文なら Enter を 1 回だけ再送して settle し直し、2 度目も Queued ならそのまま返す（Enter は最大 2 回・text の再送は 0 回・Foreign / UnknownInput の周は 0 key） (3) 本文と Enter の間に SETTLE_STEP の 1 歩が在る (4) stdout が notify=delivered consumed=<true|false|unknown[:理由]> で、refused: / unconfirmed / no-seat の字面は不変 (5) 偽 tmux の落とす回数 1 で Enter 2 回・consumed=true、0 で Enter 1 回・consumed=true、2 で Enter 2 回・consumed=false、他人の文が先に在れば send-keys 0 回・refused:busy"

[[contract]]
id = "s"
title = "審査を測れなかった便（Reviewed の INCONCLUSIVE kind:unparsed）を release で列へ戻す — settled に判定で引く戻しを 1 つ足し（INCONCLUSIVE ∧ unparsed の対だけ・印 1 回で 1 回）、段で引く requeues と § の鍵は不変"
req = ["FR49", "FR68"]
section = "22"
write-set = ["crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_dispatch_release_unparsed_"]
size = "S"
done = "(1) review.json が INCONCLUSIVE ∧ kind unparsed の Reviewed の便は、その便の最後の記帳より後の release で列外を外れ、dispatch ls の理由が - になる (2) FAIL（kind を問わず）・INCONCLUSIVE で kind が他の 6 語・review.json が無い / 読めない便は release の後も settled:<sha>/Reviewed のまま (3) 起こし直した便が同じ sha でまた unparsed に着けば再び列外（印 1 回で 1 回） (4) requeues / section_keyed の網羅 match と in-file の census の歯 3 本・released_after・judgement_of が 1 字も変わらず緑 (5) 判定で引く述語は pure な関数 1 つで、in-file の歯が FINDING_KINDS の 7 語 × 3 値の母集団で測る"
[[contract]]
id = "t"
title = "regate で Implemented へ戻された便を列が起こし直す — 起こし直しの候補に「段が Implemented ∧ 最新の Gated より後ろに regate の記帳 ∧ 札が無いか所有者が死んでいる」を 1 枝足し（読み手は regate の口と同じ 1 本・既存の枝は不変・段を前へ進めた driver の周だけ）、AC47 の不足 3 点を regate の in-file の歯に足す"
req = ["FR68", "FR77"]
section = "23"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/regate.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs"]
growth = ["crates/scribe2/src/pipe/dispatch.rs:40", "crates/scribe2/src/pipe/regate.rs:130"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_regated_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_regate_forms_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_gated_pass_ pipe_dispatch_driver_ pipe_dispatch_waiting_gate_"]
size = "M"
done = "(1) revivals の待ちの段でない枝に、gated の周の「段が Implemented ∧ regated_since_gate が真 ∧ 札が Absent か Dead」が 1 つ足り、読み手は regate.rs の既存の 1 本（可視性を pipe の中へ開くだけで本体は不変）、event の列は revivals が 1 回だけ読み、既存の driver_is_dead と passed_gate と待ちの段の枝は 1 字も変わらず、札の無い regate 済みの便が手動の 1 周で --drive 付きの resume で起こされて（resumed:1）Gated が 1 件増えて Landed まで進む (2) regate の後に PASS の gate を通って pipe follow で Implemented へ戻った便は resumed:0、札の所有者が生きている便と札を読めない便は resumed:0 で札も触らず、regate の記帳の無い Implemented の便は既存の歯 pipe_dispatch_driver_live_run_without_a_ticket_is_left_alone が緑のまま起こされない (3) 段を前へ進めなかった driver の終端の 1 周は regate 済みの便を起こさず、その後の手動の 1 周は起こす (4) 札の所有者が死んでいる判定 FAIL の Gated の便は regate を通って worktree の path と HEAD と判定の verdict が変わらず、続く手動の 1 周は resumed:1 で Gated が 1 件だけ増え（二重起動 0）、判定 FAIL の Gated の便（札なしと札の所有者が死んでいる 2 形）に regate を撃たずに手動の 1 周を K 回撃っても Implemented の記帳は 0 件（K と 2 形を assert に出す） (5) Input に項目を足さない (6) Revive・WaitReason・Stage・EventKind・1 周の行と dispatch ls の行の字面が変わらず、e2e の歯の file は段の型の変種を名指さない (7) regate.rs の in-file の歯が、口の本体に通る 2 形（札なし・札の所有者が死んでいる）と断る 6 形を渡して、通る形は記帳 1 件・断る形は記帳 0 件で理由の語が形ごとに違い（母集団 8）、戻した直後の 2 度目の理由が段の条件の語を持ち、歯の区間の行頭に retroactive の札が在り、変異の proof が bead の notes に在り、pipe_dispatch_gated_pass_ と pipe_dispatch_driver_ と pipe_dispatch_waiting_gate_ の既存の歯は測っている約束を変えずに緑のまま"

[[contract]]
id = "u"
title = "札と lock の所有者を pid の再利用に釣られず判じる — 本文を pid + 起動時刻の 2 語にし、読み手は 2 語の周に起動時刻が違えば Dead（1 語は今のまま）、Driver の Drop は先頭の語で自分を判じ、死んだ札の fixture は 2 語で書き gone の待ちを 60 秒に（§24・s2-07l.608）"
req = ["FR68", "FR14"]
section = "24"
write-set = ["crates/scribe2/src/fleet/store.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/dispatcher.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail fleet_store_owner_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_driver_ticket_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_gated_pass_dead_ticket_ pipe_dispatch_regated_dead_ticket_ pipe_dispatch_driver_"]
size = "S"
done = "(1) acquire_with が create_new の直後に pid と started_ms の値を空白 1 つで並べた 2 語 1 行を書き、自分の起動時刻を読めない周は pid 1 語を書く (2) 本文の読み手は pid 1 語と pid + 起動時刻の 2 語の 2 形を受けてそれ以外を Unreadable とし、lock_owner は 2 語の周に probe の Started の値が本文と違えば Dead・等しければ Live・Absent は Dead、1 語の周は今までどおり (3) Driver の Drop は先頭の語で自分の札を判じて 2 語の自分の札を外し他人の札は落とさない (4) put_dead_ticket は抜けた pid と一致しない起動時刻の 2 語を書き、gone は 60 秒を待ち、既存の e2e の歯は約束を変えずに緑のまま (5) 1 周の行と dispatch ls の行と Ticket の 4 値と Owner の 3 値と EventKind の列と受付札と追記の lock の断りの字面は 1 字も変わらない 歯: fleet_store_owner_ の歯が 2 語の Dead / Live と 1 語の互換と 3 語の Unreadable と acquire_with の書く 2 語を測り、pipe_driver_ticket_ の歯が Drop の自分 / 他人の弁別を測り、pipe_dispatch_gated_pass_dead_ticket_ と pipe_dispatch_regated_dead_ticket_ と pipe_dispatch_driver_ の既存の歯が緑のまま"
[[contract]]
id = "v"
title = "追随の起こし直しの後に driver が抜けた便を列が起こし直す — revivals の 4 枝目（Implemented ∧ 最新の Gated より後ろに rebase: / rebase-conflict: の記帳 ∧ 札 Absent | Dead）・argv は既存の resume --drive（§25・s2-07l.633）"
req = ["FR68", "FR77", "NFR4"]
section = "25"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/regate.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/dispatcher.md", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail followed_since_gate_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_revive_followed_"]
size = "S"
growth = ["crates/scribe2/src/pipe/dispatch.rs:20", "crates/scribe2/src/pipe/regate.rs:30"]
depends = ["t"]
done = "(1) revivals は gated の周に「段 Implemented ∧ 最新の Gated の RunStage より後ろに追随の記帳（detail が rebase: か is_conflict の 2 語で始まる）∧ その後ろに Gated / Landed が無い ∧ 札 Absent | Dead」の便も候補にし、読み手は regate.rs の隣の pure な 1 本（event の列と便 id）で既存の 3 枝と待ちの段の枝は 1 字も変わらない (2) 起こす argv は既存の 1 本（resume … --drive） (3) 追随の後に Gated を経た便・追随の記帳の無い Implemented・札 Live / Unreadable は起こさない (4) gated の絞りと二重にしない規則は §23 と同じ (5) Revive / WaitReason / Stage / EventKind と 1 周の行・dispatch ls の行は不変・turn_skipping は不変 歯: pipe_dispatch_revive_followed_ の歯が (a) Gated PASS → rebase: の Implemented ∧ 札 Absent で gated の周に resumed:1・argv の末尾 --drive（base では resumed:0 ＝ RED）(b) rebase-conflict: でも同じ (c) 追随の後にもう 1 度 Gated で resumed:0・既存の regated_then_gated_and_followed の歯は名を改めて resumed:1 に書き換える (d) 追随の記帳の無い Implemented で resumed:0 (e) 手動の 1 周でも resumed:1 を測り、lib の followed_since_gate_ が追随あり / なし / 追随の後の Gated を測る"

[[contract]]
id = "w"
title = "idle の知らせの末尾に並列の実測（live の本数・0 本の分数・重なりで待つ本数と file 名）を足す — 事実と字面を兄弟 module の 1 関数で作り heartbeat と共用し、WaitReason::Overlap は交差の file の列を持つ（既存の key と順・reason= の字面は不変）"
req = ["FR68", "FR44", "NFR4"]
section = "26"
touches = ["crate::pipe::dispatch::WaitReason"]
write-set = ["+crates/scribe2/src/pipe/dispatch/precheck.rs", "+crates/scribe2/src/pipe/dispatch/facts.rs", "+crates/scribe2/src/pipe/dispatch/refused.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/notify.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2-boundary/tests/e2e/notify.rs", "docs/design/dispatcher.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_notify_facts_"]
size = "M"
growth = ["crates/scribe2/src/pipe/dispatch.rs:4", "crates/scribe2/src/pipe/notify.rs:8", "crates/scribe2/src/pipe/cli.rs:6"]
done = "(1) 事実の 1 関数が置き場・列の結果（無しも可）・今の秒から live の本数（live を全便に撃ち None が 1 本でも在れば測れない）・0 本の分数（live 0 の周だけ updated の最大から切り捨て・便 0 本と live 1 本以上は値なし・読めない ts は測れない）・重なりで待つ本数と交差の file 名（最後の 1 要素・dir 項目は末尾の / を残す・重複なし・字の順・数えるのは理由が今の Overlap の候補だけ）を返し、live の判定を 2 本目に書かず、兄弟 module の宣言と事実の関数と字面の関数は pub(crate) で crate::seat::tick から呼べ、write-set の外の live（crate::pipe::cli の再輸出）と epoch_of・Run の updated（crate::fleet の再輸出）は読むだけで可視性を変えず、pipe/mod.rs の pub mod dispatch を通って届くので pipe/mod.rs は編集しない (2) 字面は同じ 1 関数で live= idle=<m>m held=<k>:<名> の順・測れない値は ?・値なしは -・重なり 0 は held=0・列の結果の無い呼び手は held= を出さない (3) Overlap の files は交差した契約側の file の列で、blocker の 2 つの構築点は列をそのまま渡し、render は列の長さを書き reason= と dispatch ls の字面は 1 字も変わらない (4) idle の 1 行は既存の ready= launched= reason= の後ろに (2) の字面が付き、notices が同じ周の Turn で (1) を 1 回撃ち、終端の 1 行と契機と宛先は不変 (5) 歯 pipe_notify_facts_ が live な便と 1 file 交差する周の末尾 live=1 idle=- held=1:<名> と live 0 の hold の周の末尾 live=0 idle=0m held=0 を測り、base で RED"

[[contract]]
id = "x"
title = "依存を待つ行に受付の判定を予想の base で先に撃つ — 閉じていない契約の行の母集団と blocks の到達を 1 関数で持ち、祖先の宣言（Gated PASS の便は実物の木）で tracked と本文を重ね、Denial が持つ型の断りの在り処で確定 / 暫定 / 測れないを 1 関数で分け、鍵が動いた行だけ起こす側の周で撃って置き場の file に残し dispatch ls に 1 行で見せる（予想は通行証にしない）"
req = ["FR48", "FR49", "FR68", "NFR4"]
section = "27"
touches = ["crate::pipe::cli::intake::Denial"]
write-set = ["+crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs", "docs/design/dispatcher.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_precheck_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_refuse_evidence_ pipe_table_evidence_"]
size = "L"
growth = ["crates/scribe2/src/pipe/dispatch.rs:24", "crates/scribe2/src/pipe/cli.rs:8", "crates/scribe2/src/pipe/cli/intake.rs:24", "crates/scribe2/src/pipe/refuse.rs:120", "crates/scribe2/src/pipe/table.rs:60", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs:330"]
done = "(1) 母集団と到達の 1 関数（pub(super)）が同じ周の台帳の全件と置き場の run の列から、閉じていない契約の行（closed でない ∧ memo でない ∧ 列と同じ pointer_of で設計 pointer が解ける bead・live な便は run dir の写しの write-set つき）と blocks の推移の到達（parent-child は数えない・closed で止まる・循環で回らない）を返し、live でない行の write-set は呼び手が自分の材料で generated を撃って決め、依存待ちで設計 pointer を持つ行の open な祖先をその到達で依存の順に並べ、宣言の予想は tracked に + を足し ~ を除き、Gated PASS の便は worktree の base..HEAD の差分で tracked を足し引きして本文を置き換え、動く file は宣言で重ねた祖先の write-set だけで、祖先の write-set が決まらない行は unmeasured:forecast (2) 予想の base は Materials の口 1 つで組み、generated と judge（置き場なし・lock の前の読みなし）を撃ち、置き場の要る判定と base の木の実走は撃たない (3) Denial が型の断りの列を持ち、Refuse と TableError の網羅の match が在り処の 4 値（行・file の列・名・置き場）を返し、弁別の 1 関数が確定 / 暫定 / 測れないの 3 値を返し、型を持たない断りは測れない (4) fire の起こし終えた後に鍵（base の HEAD・rules の写しの sha か器の版・祖先ごとの id と状態の語）の字が置き場の結果と同じ行は撃たず、祖先が Gated PASS になった周と着地した周は撃ち直し、台帳と材料は同じ周の 1 回を借り、dispatch ls は撃たず母集団の 1 関数も呼ばない (5) 結果は置き場の事前審査の dir に bead ごとの 1 file（鍵・結果の語・finding ごとの行と new の印）を一時 file → rename で書き、読めない・鍵の違う file は撃ち直し、依存待ちに居ない bead の file は同じ周に外し、event kind は足さない (6) WaitReason・起こす判定・受付は結果を読まず、予想で clean の行も依存が閉じた後は実物の main の受付で断られうる (7) dispatch ls が依存待ちの候補ごとに [DISPATCH-PRECHECK] の 1 行を件数の行の前に出し、[DISPATCH] と [DISPATCH-COUNT] の行の字は変わらない 歯: pipe_dispatch_precheck_ が宣言の + を素で持つ行の clean・どこにも無い file の firm:1・依存と交わる file の余地不足の provisional:1・同じ鍵の 2 周目の書き直し 0 と Gated PASS の周の撃ち直しで出る teeth-outside-write-set の firm と new・ls の行・+ を作らずに閉じた依存の後の admission:contract-table・A → C → B の推移の祖先の宣言で clean と parent-child だけの bead と closed の bead の宣言が予想に入らない firm:1 を測り base で RED、pipe_refuse_evidence_ と pipe_table_evidence_ が 23 語と 17 variant の在り処と弁別の 3 値を測る"

[[contract]]
id = "y"
title = "事前審査の確定を根で束ねて直しへ導く — 束ごとの file（行の TOML・節・findings・測り直しの 1 行）を置き場に置き、束の集合が変わった周だけ席へ束の id と file の path の 1 行、idle の知らせと heartbeat の末尾に precheck= の本数、最も古い束が rules 行 seat.precheck_alarm_s を越えた周は行 u の段の上げで heartbeat の段を上げる（行が無い周は上げず alarm= に precheck-unset）"
req = ["FR68", "FR27", "FR78", "NFR4"]
section = "27"
touches = ["crate::rules::RuleKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch/bundle.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2/src/pipe/dispatch/facts.rs", "crates/scribe2/src/pipe/notify.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/seat/tick.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/notify.rs", "crates/scribe2-boundary/tests/e2e/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/dispatcher.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_notify_precheck_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_precheck_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_precheck_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all"]
size = "M"
growth = ["crates/scribe2/src/pipe/dispatch.rs:6", "crates/scribe2/src/pipe/dispatch/precheck.rs:8", "crates/scribe2/src/pipe/dispatch/facts.rs:30", "crates/scribe2/src/pipe/notify.rs:24", "crates/scribe2/src/pipe/cli.rs:6", "crates/scribe2/src/seat/tick.rs:14", "crates/scribe2/src/rules/mod.rs:6"]
depends = ["x"]
done = "(1) 周の終わりに確定の finding を根（断りの名と在り処）で束ね、束ごとの 1 file（束の id・根・行の pointer と bead・行の TOML・節の本文・findings・各行の pipe preflight の argv・初めて見た時刻）を置き、同じ根の 10 本は 1 束で、束の id は根の字から決まり、確定の消えた束の file は外れる (2) 束の集合が前の周と違う周だけ §19 の宛先へ同じ送達の 1 関数で precheck bundles= rows= の後ろに束ごとの <束の id>=<束の file の path> を並べた 1 行を送り（作法の散文を載せない・束が 0 本になった周も bundles=0 rows=0 を 1 回送る）、dispatch ls は束ごとに [DISPATCH-BUNDLE] の行を出す (3) idle の行と heartbeat の合図の末尾（heartbeat では alarm= の前）に precheck=<確定の行>/<結果の行>:<束> が付き、事前審査の dir の無い置き場は付かず、heartbeat は台帳を読まない (4) 埋め込みの manifest に行 seat.precheck_alarm_s（kind SeatPrecheckAlarmS・Int・秒・値 900・裁定 id user 2026-09-27T17:33Z 項 2-1）が seat.idle_alarm_s の直後、kind が ALL の SeatIdleAlarmS の直後に在り、最も古い確定の束の初めて見た時刻から値の秒数以上経った周は行 u の段の上げの 1 本（黙りの門を短くし梯子を段 0）で段が上がり alarm= の列に precheck が u の語の後ろに付き、値 0 は上げず、行が無い・読めない周は段を上げず precheck= を出す周に限り alarm= の列に precheck-unset が付く 歯: pipe_notify_precheck_ が同じ根の 3 行の 1 束と周ごとに 1 回の送達とその 1 行の末尾 precheck bundles=1 rows=3 <束の id>=<path> と idle の末尾 precheck=3/3:1 と、dispatch ls の束ごとの [DISPATCH-BUNDLE] の行と、確定の消えた周の束の file の除去と [DISPATCH-BUNDLE] の 0 行と precheck bundles=0 rows=0 の 1 回だけの送達を、seat_tick_precheck_ が dir の有無での末尾の有無と、値 60・束 120 秒前・打刻 90 秒前の周の合図 1 回と alarm= の precheck（短くした黙りの門）と、行の無い写しの同じ周の noop と打刻の古い周の precheck-unset を、rules_precheck_ が行の形と値と manifest と ALL の位置を測り base で RED、manifest の行数と kind の数の pin と rules_external_form の snapshot が 1 ずつ増え、rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all は SeatIdleAlarmS の直後に SeatPrecheckAlarmS が続く並びに書き換わる"

[[contract]]
id = "aa"
title = "事前審査が clean の待ち行に lens を裏で先に撃ち、FAIL / INCONCLUSIVE を確定の finding として束に入れる — 予想の base を一時の worktree に実体化して審査と同じ組み手で材料を組み、1 周に起こす本数は rules 行 pipe.precheck_lens_per_round（撃ち中を含む・組み直しは上限の外）、結果は次の周が読み、main が動いた周も材料が同じ行の finding は消えない（裁定 user 2026-09-27T14:02Z の前半）"
req = ["FR49", "FR36", "NFR1", "NFR4"]
section = "27"
touches = ["crate::rules::RuleKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch/prelens.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2/src/pipe/review.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_prelens_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_precheck_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_prelens_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role"]
size = "L"
growth = ["crates/scribe2/src/pipe/dispatch.rs:8", "crates/scribe2/src/pipe/dispatch/precheck.rs:20", "crates/scribe2/src/pipe/review.rs:20", "crates/scribe2/src/rules/mod.rs:6"]
depends = ["x"]
done = "(1) 事前審査が clean の待ち行だけに撃ち、1 周に起こす本数は埋め込みの manifest の行 pipe.precheck_lens_per_round（kind PipePrecheckLensPerRound・Int・値 1・裁定 id user 2026-09-27T17:33Z 項 2-2・manifest は gate.lens_count の直後・kind は ALL の GateLensCount の直後）を周の頭に数えた撃ち中の行を含めて越えず、値 0 は撃たず、行が無い・読めない周は撃たずに [DISPATCH-PRECHECK] の行の末尾に prelens=unset が --lens の有無によらず付き（行 x の歯 pipe_dispatch_precheck_ の期待の行もこの末尾を持つ）、--lens の無い周は撃たず、口座は選ばず起こす側の環境を継承して TMUX_PANE だけを外し、材料は置き場の tree に作る一時の detached worktree に予想の base を実体化して（層の列は precheck.rs の 1 本〔行 x の resolve と同じ memo〕から受け・口の可視性と渡し方は問わない、Gated PASS の祖先は add の file を祖先の木の HEAD から拡張子で絞らずに写し remove の file を消し、宣言だけの祖先は + の file を空で作り ~ の file を消し〔~ は歯で測らない〕、git add -A で index に載せる）、review の materials と keep を 1 つの口から repo としてその path を渡して組み、宣言だけの祖先を持つ行は設計の材料の末尾に予想の base の 1 行と空で置いた file の path を足し、組んだ後に worktree を外し、落ちた周の残り（tree に在る worktree・登録だけの worktree）は次の周の頭に git worktree remove --force か git worktree prune で外れ（worktree でない file は消さない）、組めない・起こせない行は撃たずに置き場に理由の file unbuilt を置いて ls の行の末尾に prelens=unbuilt が付く (2) lens は spawn_self と同じ起こし方で箱（systemd の scope）に包まずに裏に起こし、起こした周は終わりを待たず、置き場 lens/<bead>/ に key・fired・lens・pid・rc・out を置き（rc を置いてから out を rename）、fired と lens は起こす時に写し、撃ち中の印は <pid> <起動時刻> の 2 語で lock_owner が生きていると判じる行と印を読めない行は撃ち中に数え、死んで out の無い行は印を外して撃ち直し、次の周が decide の後段と同じ読み手で判定を読み、置き場は bead が母集団に居る間は残り居なくなった周の頭に外れる (3) 撃ち中でない clean の行は事前審査の鍵が key の 1 行目と違う周に上限の外で材料を組み直して key を付け替え、撃ち中の行の材料は lens が終わるまで組み直さず、組み直した材料の鍵が fired と違う行は同じ周に rc・out・fired を外し、結果に写さなかった判定（rc が 0 でない・unparsed）の out を持つ行は組み直した周に材料の鍵が fired と同じでも rc・out・fired を外し、fired を外した行は上限の空いた周に撃ち直し、組み直しと写し直しは --lens の有無・rules 行の有無と値に依らず毎周撃ち、鍵は材料の dir の全 file の名と本文から名の順に決まる 1 つの字 (4) out の判定が FAIL か INCONCLUSIVE で理由の型が unparsed でない周だけ、理由の型を名に在り処を prelens にした確定の finding が事前審査の結果に載り、rc が 0 でない周と unparsed の周は結果に写さず（撃ち直しは (3)）、main が動いて事前審査が結果を書き直す周も clean で材料の鍵が fired と同じ行は書き直す前の結果を前の結果として同じ周に写し直し、前の結果に在った finding は new=false で残る 歯: pipe_prelens_ が (a) 上限 1 で撃ち中の間の周に残りを起こさず group を殺した後の周に 1 本だけ起こすことと値 0 の周の 0 回、(b) 行の無い写しの周の 0 回と --lens を持たない ls の末尾 prelens=unset、(c) TMUX_PANE が unset で別の変数が継承の値、(d) 宣言だけの祖先の + の F を素で持つ行の材料の予想の 1 行と F の path と base の要約の F の 行数 全体 0 と外の材料の F と worktree の残り 0、(e) Gated PASS の祖先の木の .md の G の base の要約の 行数 全体 2 と予想の印の無さ、(f) worktree を作れない周の 0 回と prelens=unbuilt とその後の周の撃ち直し、(g) 起こした周が返った時の out の無さと 2 語の印と生きている pid、(h) 2 語目を書き換えた印の撃ち直しと数でない字に壊した印の撃ち中、(i) FAIL（vacuous-assert）と INCONCLUSIVE（section-material-missing）の次の周の result=firm:1,provisional:0 と root=<理由の型> と在り処 prelens と rc 1 の周の result=clean、(j) 別の行の lens が撃ち中の周に材料に入らない file を commit した次の周の result=firm:1,provisional:0 と new=false と束の行の不変（--lens 無しの周と値 0 の周も同じ）、(k) 材料の file を変えた次の周の result=clean と out と fired の無さと撃ち中の行の材料の不変、(l) 依存待ちを抜けた行の置き場の残りと bead を閉じた周の置き場の消え、(m) unparsed を返した行が main の動かない周に撃ち直されず材料に入らない file の commit の後の 2 周のうちに撃ち直されて回数 2 で止まること、(n) tree に worktree を残した置き場と登録だけを残した置き場の周が prelens=unbuilt で終わらずに撃ち、周の後の git worktree list がその path を持たないことを測り、rules_prelens_ が行の形と値と manifest と ALL の位置を測り base で RED、manifest の行数と kind の数の pin と rules_external_form の snapshot が 1 ずつ増え、pipe_dispatch_precheck_ の期待の行が末尾 prelens=unset を持つ"
[[contract]]
id = "ac"
title = "Reviewed の段は、実物の base で組んだ材料の鍵が先撃ちの fired と同じで、判定が unparsed でなく、lens の cmd の字が同じ時だけ先撃ちの判定を使い回す — detail の末尾に語 prelens:reused、写した周は審査の消費を記帳しない（裁定 user 2026-09-27T14:02Z の後半）"
req = ["FR49", "NFR1"]
section = "27"
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/dispatch/prelens.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_reuse_"]
size = "S"
growth = ["crates/scribe2/src/pipe/review.rs:30", "crates/scribe2/src/pipe/dispatch/prelens.rs:20", "crates/scribe2/src/pipe/dispatch.rs:2"]
depends = ["aa"]
done = "(1) pipe run の Reviewed の段は、実物の base で組んだ材料の鍵（行 aa と同じ 1 関数）が置き場の fired と同じで、out の判定が unparsed でなく、置き場の lens の字が便の lens の cmd と同じ周だけ先撃ちの判定を写して段の detail の末尾に語 prelens:reused を足し、違う周は今どおり lens を撃つ (2) 写した周は審査の消費の 1 件を書かない 歯: pipe_review_reuse_ が (a) Gated PASS の祖先の木のまま着地した後に起こす側の周を 1 回撃ってから撃った Reviewed の偽 lens 0 回と detail の末尾の prelens:reused と消費の event の不変、(b) 着地の本文が Gated の木と違う周の偽 lens 1 回と語の無さ、(c) 予想の印を持つ行の着地後の偽 lens 1 回、(d) 先撃ちが rc 1 の行の偽 lens 1 回、(e) lens の cmd の字が違う周の偽 lens 1 回を測り、base で RED"
[[contract]]
id = "ab"
title = "死んだ札の歯の待ちを札の継ぎ替えの間に釣られない形にする — e2e の gone は札の不在が 500 ミリ秒続いた周に真・呼び手 3 本の assert と src は不変・歯を足さない helper の変更に flip-check の retroactive の札（§28・memo s2-07l.709）"
req = ["FR68", "FR14"]
section = "28"
write-set = ["crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_gated_pass_dead_ticket_ pipe_dispatch_regated_dead_ticket_ pipe_dispatch_drive_revives_a_dead_driver_"]
size = "S"
done = "(1) e2e の gone が、札の path の不在が 500 ミリ秒続いた周に真・60 秒のうちに続かなければ偽を返し（50 ミリ秒ごとに見て在る観測で数え直す）、呼び手 3 本（pipe_dispatch_gated_pass_dead_ticket_is_resumed_regardless_of_verdict・pipe_dispatch_regated_dead_ticket_keeps_three_records_and_resumes_once・pipe_dispatch_drive_revives_a_dead_driver_all_the_way_to_landed）が assert と期待を変えずに緑 (2) 変えた gone の直前に札 // flip-check: retroactive <本行の bead の id> が 1 行在り、flip-check が retroactive で通る (3) src と waiting.rs の本文と put_dead_ticket は 1 byte も変わらない"

[[contract]]
id = "ad"
title = "未処置の終端を idle の知らせに毎周載せ、送達の結果を stderr にも残す — 列の候補のうち Settled の bead の最新の便を alarm_word で判じて idle の行の末尾に pending= を足し、notify= の行を stderr にも写して列が起こした運転手の周も launch.log に残す（契機・宛先・終端の 1 行は不変・memo s2-07l.732）"
req = ["FR30", "FR68"]
section = "29"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/notify.rs", "crates/scribe2-boundary/tests/e2e/notify.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_notify_pending_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_notify_facts_"]
size = "S"
growth = ["crates/scribe2/src/pipe/cli.rs:25", "crates/scribe2/src/pipe/notify.rs:20"]
done = "(1) notices が同じ周の列の結果の候補のうち理由が Settled のものについて置き場の replay からその bead の最新の便を引き、alarm_word が Some を返すものを未処置とし、判じ手は alarm_word の 1 本のまま周ごとに導き直す (2) idle の 1 行の今の末尾の後ろに pending=<k>:<bead>/<段>=<語>,… を候補の順で足し、0 本の周は key を出さない (3) 送る契機は §19 形 1 (b) のまま (4) notify= の行を stdout に加えて stderr にも同じ字面で出し、終端の stderr を完全一致で照合する既存の歯 pipe_retire_reviewed_unreadable_refused_and_names_the_verdict（retire.rs）は notify= で始まる行を除いた stderr で断りの 1 行を照合する（断りの字面は不変・直した歯は base でも緑なので retire.rs の test 区間の行頭に // flip-check: retroactive s2-07l.732 の札を置く） (5) notify.rs は段の閉じた型の variant を名指さない 歯: pipe_notify_pending_ の (a) 審査の判定 FAIL で終端に着いた便の bead が ready のまま Settled の候補になる置き場で別の live な便の終端を撃つと idle の行が pending=1:<bead>/Reviewed=<語> で終わる (b) 同じ周の終端の stderr が stdout の notify= の行と同じ行を持つ・既存の pipe_notify_facts_ の 2 本が不変で GREEN・base は (a) の key と (b) の stderr の行が無いので RED"

[[contract]]
id = "ae"
title = "便の worktree の build と依存の置き場を、便が live でなくなった周に器が消す — 運転手の終端の周ごとに、置き場の全便のうち live でない便の木（元の場所か退役先）を歩き、名が閉じた 8 つの列（target・node_modules・.venv・__pycache__・.mypy_cache・.pytest_cache・.ruff_cache・.expo）に在り追跡されている file を持たない dir を remove_dir_all で消す（git clean は撃たない・live と測れない便は触らない・置き場ごとの lock・stderr に 1 行・ADR-0081・裁定 user 2026-09-28T04:45Z / 04:46Z）"
req = ["FR68", "FR30"]
section = "30"
write-set = ["+crates/scribe2/src/pipe/sweep.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_sweep_"]
size = "S"
growth = ["crates/scribe2/src/pipe/sweep.rs:140", "crates/scribe2/src/pipe/mod.rs:2", "crates/scribe2/src/pipe/cli.rs:6"]
done = "(1) 消す dir の名は target・node_modules・.venv・__pycache__・.mypy_cache・.pytest_cache・.ruff_cache・.expo の閉じた 8 つの const の slice で、無視の規則と host の除外は読まず git clean は撃たない (2) 掃除の 1 関数が置き場の replay の全便のうち live が Some(false) の便について置き場の記録から repo を解き、元の場所と退役先のうち在る方の木を .git に降りずに歩き、名が列に在り追跡されている file を持たない dir（その木の git ls-files の 1 回で判じる）を remove_dir_all で消してその下へ降りず、live が Some(true) か None の便・repo を解けない便・木が無い便は撃たず、git ls-files を撃てない木は消さずに失敗に数える (3) cli.rs の dispatch が終端の subcommand の周に段の記帳の後・列の 1 周の前に 1 回撃ち、--repo の無い周も撃つ (4) 置き場の pipe の dir の掃除の lock を acquire_with で取り、取れない周は撃たない (5) dir を消した周・失敗した木が在る周・lock を取れない周だけ stderr に sweep: removed=<n> runs=<k> failed=<m>[:<便 id>,…] か sweep: skipped=lock を 1 行出し、stdout・event・rc は変えない (6) 退役の move・WorktreeCheck・live の判定・列の判定・rules 行・event の kind・host_guard.git は変わらない 歯: pipe_sweep_ の (a) Stopped の便の元の場所の木から追跡されていない target/ と node_modules/ が pipe stop の終端の後に消え、追跡されている file を持つ target の名の dir・列に無い未追跡の dir・無視の規則に当たる列に無い file は残り、stderr が sweep: removed=2 runs=1 で始まる行を持ち、終端の後も live のままの便の木の target/ は残る (b) Landed の便の退役先の木の target/ が下に入れ子の .git を持っていても消える・base は消えないので RED"

[[contract]]
id = "af"
title = "台帳の問い（label intake:question）を起動の列の入力と事前審査の母集団から memo と同じく外し、memo と問いの label の字の定義を ledger/form.rs の const 2 つに、判定を同じ file の公開の述語 2 つに寄せる — 列の待ちの理由・dispatch ls の字・doctor の行・memo の plan の字・起票の門は不変（FR68・FR51・ADR-0083・ADR-0088 の代償の項）"
req = ["FR68", "FR51"]
section = "31"
write-set = ["crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/ledger/lint.rs", "crates/scribe2/src/ledger/memo.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "=crates/scribe2-boundary/tests/e2e/ledger_memo.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_intake_label_", "cargo nextest run -p scribe2 --lib --no-tests=fail precheck_intake_label_", "cargo nextest run -p scribe2 --lib --no-tests=fail ledger_lint_judge_counts_each_defect_apart", "cargo nextest run -p scribe2 --lib --no-tests=fail quadrant_exempts_question_from_the_shaped_population", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail ledger_memo_plan_carries_label_parent_and_relates_to"]
size = "S"
growth = ["crates/scribe2/src/ledger/form.rs:8", "crates/scribe2/src/ledger/lint.rs:0", "crates/scribe2/src/ledger/memo.rs:0", "crates/scribe2/src/pipe/dispatch.rs:0", "crates/scribe2/src/pipe/dispatch/candidates.rs:4", "crates/scribe2/src/pipe/dispatch/precheck.rs:40"]
done = "(1) intake:memo の字を定義するのは form.rs の MEMO_LABEL だけになり、lint.rs・memo.rs・dispatch.rs の const は消えて form.rs の const か (2) の述語を引き、intake:question は form.rs の QUESTION_LABEL のままで、memo の plan の引数 arg: --labels=intake:memo と doctor の台帳の 2 行と起票の門の断り文の字は変わらない (2) form.rs が memo か（is_memo を公開にする）と問いか（is_question を足す）の公開の述語 2 つを持ち、form.rs の 4 象限の問いの除外・lint.rs の memo の数え・列の入力・事前審査の母集団はこの述語を引いて label を自分で比べず、起票の門は今どおり form.rs の const を引く (3) 列の入力の判定が label intake:question を持つ bead を acceptance の有無に依らず外し、その bead は dispatch ls にも出ず、WaitReason の値と dispatch ls の行の形と順序は変わらない (4) 事前審査の母集団の関数が問いを契約の行に数えず、blocks の到達には今どおり残る 歯: pipe_dispatch_intake_label_ の (a) 行 a を指す契約 1 件と label intake:question と行 b を指す設計 pointer の acceptance を持つ問い 1 件の台帳で dispatch ls の契約の reason が - で問いの行が無く件数の行が total=1 ready=1（base は問いも候補に並び件数の行が total=2 なので RED）(b) 問いの代わりに label intake:memo の bead を置いた台帳で同じく行が無く total=1 ready=1（回帰の歯・base でも緑）、precheck_intake_label_ の (c) 問い q（label intake:question・設計 pointer）に blocks される契約 c と label の無い同じ pointer の bead p の 3 件で母集団の契約の行が c と p だけで c の到達が q を持つ（base は q も行に数えるので RED・歯は form.rs の既存の const を引き新しい述語を呼ばない）(d) q の label を intake:memo に替えても行が c と p だけ（回帰の歯・base でも緑）、e2e の台帳の 1 件を組む既存の helper と偽の台帳の helper の本文は変わらず、既存の歯 ledger_lint_judge_counts_each_defect_apart・quadrant_exempts_question_from_the_shaped_population・ledger_memo_plan_carries_label_parent_and_relates_to は本文を変えずに緑"

[[contract]]
id = "ag"
title = "起動の列の起こす側の周が、受付が断った契約ごとに断りの名と bead を IntakeRefused 1 件に記帳する — 選ぶのは WaitReason::Admission のうち列自身の mark と spawn を除く名、書くのはその bead の event log の最後の行が同じ名の IntakeRefused でない周だけ（断りに入った周と名が変わった周）・判定 2 つと書き手は子 module・観測の口と受付は書かない（FR68・AC60・ADR-0088 (6)）"
req = ["FR68"]
section = "32"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/dispatch/refused.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs", "=crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "=crates/scribe2/src/pipe/dispatch/candidates.rs", "=crates/scribe2/src/fleet/mod.rs", "=crates/scribe2/src/fleet/event.rs", "=crates/scribe2/src/fleet/store.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail refusal_record_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_intake_refused_"]
size = "S"
growth = ["crates/scribe2/src/pipe/dispatch.rs:12", "crates/scribe2/src/pipe/dispatch/refused.rs:80"]
done = "(1) 起こす側の周（fire）だけが、起こし終えた後（MARK / SPAWN の上書きの後・事前審査の前）に、候補のうち理由が WaitReason::Admission で名が MARK でも SPAWN でもないものを候補の順に (bead, 名) で選び、受付の断りの名と SLOT は入り HostBusy と他の理由は入らない (2) 選んだ (bead, 名) ごとに、同じ周が読んだ event の列のうちその bead を持つ最後の行が kind IntakeRefused で refuse が同じ名なら書かず、それ以外（行が無い・別の名の IntakeRefused・別の kind の行）なら 1 件書き、event log を読めなかった周は 1 件も書かない (3) 書く行は kind IntakeRefused・bead・refuse = 断りの名・actor は kind の既定・detail なしで、追記は fleet/store.rs の append の 1 本・lock の待ち方は launched と同じく manifest の LockPolicy から読み、policy を読めない・書けない周は候補・理由・起こす便・rc・stdout・stderr を 1 つも変えない (4) 判定 2 つと書き手は行 ag の + の file に在り、dispatch.rs に足すのは mod 宣言・Read に周が読んだ event の列（読めない周は None）の 1 欄・fire の呼び出し 1 か所だけで、子は Candidate の slice を受け Turn の literal と EventKind・Stage の match の arm と variant の構築を書かない (5) 観測の口（turn・dispatch ls）・Unmeasured の周・受付（pipe run / pipe intake の断り）は書かず、WaitReason の値と render の字・dispatch ls の行・受付の断りの外形（run dir も event も作らない）は変わらない 歯: refusal_record_ の in-file 7 本（(a) Admission の cap-headroom・SLOT・MARK・SPAWN と Dependency・Hold・理由なしの 7 候補から cap-headroom と slot の 2 件だけが候補の順で選ばれる (b) その bead の最後の行が同じ名の IntakeRefused なら書かない (c) 別の名なら書く (d) 同じ名の後にその bead の DispatchMark release の行が在れば書く (e) 同じ名の後に別の bead の行だけなら書かない (f) その bead の行が無ければ書く (g) event log を読めない周は書かない・event は Event の from_line で組む）と pipe_dispatch_intake_refused_ の e2e 2 本（terminal.rs に足す・(A) base に無い file を素で持つ行 r と hold した行 h の台帳で、周の前に dispatch ls だけを撃つと r の理由が admission: で始まるのに IntakeRefused の行が 0 本（観測の口は書かない）、続けて起こす側の手動の 1 周を 2 回と dispatch ls をもう 1 回撃つと、dispatch ls の r の理由が admission: で始まり mark でも spawn でもなく、event log の \"kind\":\"IntakeRefused\" の行がちょうど 1 本で bead が r・refuse が dispatch ls の理由の admission: の後ろの字と同じで h の行は 0 本、2 周目と dispatch ls の後も 1 本のまま (B) 1 周の後に r へ release を打ってもう 1 周撃つと r の行が 2 本で 2 本目が release の行より後）・base は起こす側の周が 1 件も書かないので (A)(B) が本数 0 で RED・in-file の歯は新しい module と一緒に生まれるので flip-check は e2e の file で入口を通す"

[[contract]]
id = "ah"
title = "席の起草の置き場の中間生成物も同じ掃除で消す — 器が state dir の下に席ごとの起草の置き場（seat/<潰した target>/drafts/）を持ち、運転手の終端の周の掃除が同じ lock の中で、置き場の .git を持つ木から、名が閉じた 8 つに在り追跡されている file を持たず、自身と下の全 entry の mtime の最新が rules 行 seat.drafts_stale_h（6 時間・裁定 user 2026-09-29T11:43Z）より前の dir を消す（木の写しと .git を持たない写しは消さない・行を読めない周は掃かない・stderr の行に drafts= と nogit=・ADR-0096）"
req = ["FR68", "FR30"]
section = "33"
touches = ["crate::rules::RuleKind"]
write-set = ["crates/scribe2/src/pipe/sweep.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/inject.rs", "crates/scribe2/src/rules/mod.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_sweep_drafts_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_drafts_stale_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_sweep_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role"]
size = "S"
growth = ["crates/scribe2/src/pipe/sweep.rs:95", "crates/scribe2/src/pipe/cli.rs:2", "crates/scribe2/src/seat/mod.rs:14", "crates/scribe2/src/seat/inject.rs:0", "crates/scribe2/src/rules/mod.rs:6"]
done = "(1) 席ごとの起草の置き場は state dir の seat/<潰した target>/drafts/ で、seat/mod.rs に名 drafts の const と、置き場と target から起草の置き場の path を返す関数と、置き場の根を返す関数（根の名は inject.rs の SEAT_DIR の 1 つのまま可視性だけを親 module へ上げる）が在り、器は起草の置き場の dir を作らない (2) 置き場の根の直下の dir（symlink は辿らない）ごとの起草の置き場の直下の子のうち .git（file か dir）を持つ dir だけを起草の木とし、.git を持たない子の dir は触らずに nogit に数え、symlink と file の子はどちらにも数えない (3) 掃除の 1 関数 swept が書きの線（Option の時刻）を受け、便の木は線無しで今のまま消し、起草の木は線 = 今 − rules 行の時間で、名が閉じた 8 つに在り追跡されている file を持たない dir のうち、その dir 自身と下の全 entry（file と dir・symlink は辿らずに symlink そのものの mtime）の mtime の最新が線より前のものだけを remove_dir_all で消し、線以後の entry を 1 つ見つけたら走査を打ち切って残して下へ降りず、mtime か dir を読めない entry が在れば残して失敗に数え、追跡の判じは木ごとの git ls-files の 1 回のまま (4) sweep が便の木の後に同じ lock の中で起草の木を掃き、置き場の replay を読めない周も起草の木は掃き、cli.rs は終端の subcommand の周に manifest を渡して 1 回撃ち（引数 3）、管理 tick と手動の 1 周と関門の記帳の周は撃たない (5) 埋め込みの manifest に行 seat.drafts_stale_h（kind SeatDraftsStaleH・Int・値 6・enabled・裁定 id user 2026-09-29T11:43Z・裁定日 2026-09-29）が pipe.ci_poll_s の直後に 1 本在り、kind は ALL の PipeCiPollS の直後で字面から引け、sweep.rs が const の id を int_row で起草の木が 1 本以上在る周だけ読み、読めない周（無い・不発効・整数でない）は起草の木を 1 本も撃たない (6) stderr の sweep: の行は置き場の根の下に起草の置き場が 1 つでも在る周だけ末尾に drafts=<dir を消した起草の木の数か語 no-rule> nogit=<数> を足し、removed と failed と名の列（起草の木は <潰した target>/<木の dir 名>）は便の木と起草の木の両方を数え、行を出すのは dir を消した周・失敗が在る周・lock を取れない周・no-rule の周だけで、起草の置き場が無い周の行は 1 byte も変わらず、stdout・event・rc は変わらない (7) NAMES の 8 つ・lock の名と取り方・便の母集団と live・tree_of・撃つ周・退役の move・seat retire・管理 tick・rules 行 host_guard.git・event の kind は変わらない 歯: pipe_sweep_drafts_ の e2e 4 本（stop.rs の §30 の歯の後ろ・起草の木は toy repo から git worktree add --detach で切り、古くするのは std の File の set_modified で entry を子から先に戻す・終端は live な便 1 本の pipe stop）の (a) 席 2 つの起草の置き場で、1 つ目の木の 7 時間前の target/ と 2 つ目の木の 7 時間前の .venv/ だけが消え、書いたばかりの node_modules/・追跡 file を持つ 7 時間前の docs/target/・列に無い 7 時間前の out/・.git を持たない写しの 7 時間前の target/・木の追跡 file と .git が残り、行が sweep: removed=2 runs=0 failed=0 drafts=2 nogit=1 で stdout に sweep: が無い (b) 深い所に書いたばかりの file を持つ 7 時間前の target/ と、深い所に書いたばかりの空の dir を持つ 7 時間前の .mypy_cache/ が残り、7 時間前の __pycache__/ だけが消えて行が sweep: removed=1 runs=0 failed=0 drafts=1 nogit=0 (c) 行 seat.drafts_stale_h を持たない tmp manifest（ceiling_rules の本文に pipe.stop_grace_ms の行を足した写し）で 7 時間前の target/ が残り、行が sweep: removed=0 runs=0 failed=0 drafts=no-rule nogit=0 (d) (c) の写しに値 0 の行を足した tmp manifest で、木の外の dir の 1 日先の mtime の file を指す symlink を持つ書いたばかりの target/ が消え、木の外の file は残る、と rules_drafts_stale_ の e2e 1 本（rules.rs の rules_ci_poll_row_follows_the_ci_wait の後ろ）の (e) 行の id・kind SeatDraftsStaleH・形 Int・値 6・enabled・裁定 id と裁定日・int_row で 6・kind の行が 1 本・ALL で PipeCiPollS の直後・行が pipe.ci_poll_s の直後・字面から引ける・文字列の値の写しは形と合わないで断られる・直す既存の歯 rules_embedded_manifest_is_valid_and_covers_all_kinds（行数 80）と rules_embedded_manifest_declares_one_capability_row_per_role（kind 78）と rules_external_form の snapshot（rows=80 kinds=78 の 2 行）は base の 79 / 77 で RED なので retroactive の札は要らず、§30 の pipe_sweep_ の 2 本は字も期待も変えずに緑・base は (a)(b)(d) の古い dir が残り (c) の sweep: の行が無く (e) の kind が無いので RED"
<!-- contracts:end -->
