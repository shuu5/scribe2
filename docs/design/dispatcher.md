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
  1. **列の遮断器**: `turn` / `fire` が候補を起こす前に `health::now(per_core)` を 1 回読み、`act` の `Action::Wait` の周は 1 本も起こさない（`WaitReason` に 1 値 **`HostBusy`**・`dispatch ls` の理由は `host-busy`）。`Unmeasured` は `act` のとおり起こす側（gate と同じ 1 実装・C2）。`per_core` は gate と同じ 2 行（`host.runnable_per_core` / `host.blocked_per_core`）を同じ読み手で読む（`breaker()` を `health.rs` 側へ寄せて gate と列が同じ 1 関数を呼ぶ・値は不変）。
  2. **受付で止まった便は live でない**: `live` の `Stage::Intake` の枝を「札（`<state_dir>/pipe/<run>/driver`・`pipe/mod.rs` の `Ticket` の 4 値）が `Live` なら `Some(true)`・`Dead` / `Absent` なら `Some(false)`・`Unreadable` なら `None`」にする。他の段の枝は不変。読み手は既存の `Ticket`（`fleet/store.rs` の `Owner` を写す）で、新しい probe は足さない。
- 触らない: 遮断器の閾値と 3 値・gate の呼び方・`Ticket` の 4 値・`Intake` 以外の段の live・overlap の式（live の集合が変わるだけ）。
- 却下: 列だけの別の閾値（rules 行が増える・gate と違う判断になる）／Intake の便を時間で畳む（時間の裁定が要る・札で読める）／dispatcher が亡骸の run を `RunStopped` で終端にする（列が記帳する面を増やす・stop の口は席にある）。

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
  6. **歯の置き場の pin を同じ便で上げる**: `crates/scribe2/tests/e2e/pipe.rs` の `pipe_hermetic_sites_stay_one` は `pipe/` 配下の tracked の file 数と binary を起こす字面の site 数を pin する（9 file・1 site）。新設の歯の file で file 数は 10 になり、site は増やさない（歯は `run_pipe` / `pipe_cmd` の口で起こす）＝pin の file 数を 10 に上げる（`tests/e2e/pipe.rs` は約束 1 の files に在る）。
- 触らない: event の kind（通知は記帳しない・pane の行と stdout の 1 行だけ）・`deliver_within` の中身・登録 row の形・`Landed` / PASS の便（送らない）・席の見張り（Monitor）は席の手順のまま（本行の着地後に止めてよい条件は memo .507 の昇格条件）。
- 却下: 席の SessionStart / rebrief に終端の一覧を載せる（席の turn が無いと読めない＝同じ穴）／event を足して席が poll する（poll は席の寿命に縛られる・今の見張りと同じ）／全終端を送る（Landed が多く pane が流れる・落ちた便だけが席の手番）。

## 20. pipe/dispatch.rs の「台帳から候補を組む」群を子 module へ割る（契約表の行 q・純移動・行 o の前）

- 出所（orchestrator の実測 2026-09-21・verified）: `crates/scribe2/src/pipe/dispatch.rs` は 1412 行で、受付の上限 R-C4-2（1500）の余地が **88 行**＝行 o（`s2-07l.525`・size S・見積 100）を受付が `cap-headroom` で断る（`pipe preflight` で refused を実測）。行 n（`s2-07l.524`）の着地で 96 行増えた直後の姿。
- 現物（planner の census・main 47d3c52・行番号は同 commit）: 責務は 5 群——(1) 理由と候補の型（`WaitReason` / `Candidate` / `Unmeasured` / `Handoff` / `Advance`）、(2) 起こす面（`start` / `launched` / `spawn_self` / `launch_log`）、(3) 周の本体（`turn` / `fire` / `revivals` / `order`）、(4) **台帳から候補を組む群**、(5) 表示（`line` / `render` / `usage` / `mark`）。(4) は閉じている＝外の呼び手が 0 で、親の (3) からだけ入る。item は **18 個・351 行**: `Ledger`（519〜536）と、`is_input` / `entry_of` / `settle` / `Room` / `blocker` / `launch_of` / `tools` / `is_blocking` / `pointer_of` / `settled` / `section_keyed` / `section_moved` / `requeues` / `released_after` / `sizes_of` / `Marks` / `marks_of`（700〜1032・宣言順）。
- 決定的な制約（実測）: 極性一覧（`crates/scribe2/src/polarity.rs`）は `pipe::dispatch::` の型名を 1 つも pin しない（grep 0 件）。親は `crate::seat::ledger` を `ledger` の名で `use` しているので、**子 module の名は `candidates`**（`crates/scribe2/src/pipe/dispatch/candidates.rs`・`ledger` は衝突する）。
- 名前解決の形（§43 / §45 と同じ型・可視性は名前解決をしない）: 親の本体が裸で呼ぶ 8 名（`Ledger` / `is_input` / `entry_of` / `settle` / `tools` / `settled` / `requeues` / `marks_of`）は親に `use candidates::{…}` 1 文で戻し、in-file の歯だけが呼ぶ 3 名（`launch_of` / `released_after` / `section_keyed`）は `#[cfg(test)]` を付けた `use` 1 文で戻す（歯の区間の `use super::*` はこの 2 文の名を親の scope から拾う＝歯の本文は 1 字も変えない）。子は必要な名を `use super::{…}` で引く（子孫は親の private item を見る・親の `use` 群を 1 行ずつ写してよい）。上げるのは**子側**の可視性だけ（親が呼ぶ 11 名を `pub(super)`・残る 7 名は private のまま）。親側の可視性と `fire` / `turn` の本体は 1 字も変えない。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 18 item（351 行）を行 q の write-set の `+` の file へ名・本文・順序を変えずにそのまま移す。
  2. 親に増えるのは `mod candidates;` 1 行と `use` 2 文（本体用 8 名・`#[cfg(test)]` の 3 名）だけ。`turn` / `fire` / `revivals` の本体は 1 字も変わらない。
  3. in-file の歯 14 本は 1 本も動かさない（親の `mod tests` に残る・`use super::*` のまま）。
  4. 札 `// flip-check: moved <行 q の bead>` を親の歯の区間の先頭と `+` の file の先頭に対で置く（純移動の機械証明は pipeline.md §5.3）。
  5. 割った後の行数は親が **約 1065**（余地 **約 435**）・`+` の file が **約 370**＝行 o の見積 100 を満たし、size M（300）も受けられる。
  6. `crates/scribe2/tests/e2e/pipe/dispatch.rs` の diff は **0 行**（write-set に在るのは受付が verify の filter の当たる歯の file を要求するためだけ・pipeline.md §43 の `polarity.rs` と同じ型）。
- verify の filter が当たる歯の母集団（orchestrator の実測・main 6c0bbc8・verified）:
  - **lib（verify 1 行目・接頭辞 7 個）**: 親の in-file の歯は **14 本**で、名は次のとおり（宣言順）——pipe_dispatch_drive_advance_is_forward_same_or_backward / pipe_dispatch_drive_hands_off_only_on_forward_and_names_the_reason / pipe_dispatch_drive_is_added_to_every_run_the_queue_starts / pipe_dispatch_drive_ranks_every_stage_from_the_declared_order / pipe_dispatch_drive_tokens_are_the_closed_five / pipe_dispatch_launched_marks_are_cleared_by_run_created_or_release / pipe_dispatch_marks_keep_the_last_one_and_release_removes_it / pipe_dispatch_order_puts_first_before_priority_then_the_issue_number / pipe_dispatch_order_reads_the_issue_number_as_digits_not_text / pipe_dispatch_release_requeues_failed_stopped_and_gated_but_not_landed_or_reviewed / pipe_dispatch_release_requeues_only_when_the_mark_follows_the_last_record_of_the_run / pipe_dispatch_section_key_applies_to_reviewed_only / pipe_dispatch_waiting_gate_admits_only_forward_drivers_and_every_non_driver / pipe_dispatch_wait_reasons_render_the_name_and_the_value。接頭辞ごとの本数は drive 5・launched 1・marks 1・order 2・release 2・section 1・wait 2（wait は末尾の `_` を付けない＝waiting_gate と wait_reasons の 2 本を 1 個で受ける）＝合計 14 で、lib 全体で 7 個の接頭辞に当たる歯も **14 本・全部この file**（母集団は lib の `#[test]` 全数・当たりの file 数 1）。lib で名に pipe_dispatch_ を含む歯は他に 1 本（`crates/scribe2/src/pipe/mod.rs` の pipe_dispatch_driver_hold_is_an_atomic_lock_that_reclaims_only_dead_owners）だけ在り、接頭辞 driver_ は 7 個に無いので当たらない。
  - **e2e（verify 2 行目・filter pipe_dispatch_）**: 名に pipe_dispatch_ を含む e2e の歯は **51 本・全部 `crates/scribe2/tests/e2e/pipe/dispatch.rs`**（同 file の `#[test]` は 70 本・当たりの file 数 1＝write-set の中）。write-set の外の e2e file（`crates/scribe2/tests/e2e/pipe/stop.rs` 等）に在る driver_ の名は helper で、pipe_dispatch_ を名に含まないので filter に当たらない。
- 触らない: (1)(2)(3)(5) の群の本体・`WaitReason` / `Turn` / `Candidate` の欄・in-file の歯の名と assert・e2e の歯・**`crates/scribe2/src/pipe/mod.rs`**（src の側の file・e2e に pipe/mod.rs は無い。pipe_dispatch_driver_ の歯はそこに在り、verify の filter はどちらの行も当たらない＝上の母集団のとおり）。
- 却下: (1) の型の群を移す（`WaitReason` は行 o が variant を足す＝行 o の write-set が 2 file に割れて交差が増える）／(5) の表示の群を移す（80 行で余地が 100 に届かない）／行 o を S より小さく書く（size は S が最小）／割らずに据え置く（行 o が受付で止まったまま）。

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
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_gated_pass_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_driver_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_waiting_gate_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_drive_resume_hands_off_only_with_the_flag"]
size = "S"
done = "flag の無い driver の終端の 1 周は自分が段を進めた便を新しい枝の候補にせず（歯 pipe_dispatch_drive_resume_hands_off_only_with_the_flag が緑のまま・flag の無い resume が PASS の Gated で抜けた直後に自分の便が起きないことを新しい歯が測る）、verdict の読みは既存の読み手（land.rs の verdict_of・可視性は不変）を列から呼ぶだけで足り、偽の台帳と偽 runner の toy repo で、verdict が PASS ∧ 札の無い Gated の便が手動の 1 周で --drive 付きの resume で起こされて先の段へ進み（resumed:1）、verdict が INCONCLUSIVE の便と verdict を読めない便は起こされず（resumed:0）、札の所有者が生きている便と札が在るのに読めない便は触らず（母集団 = 札の 4 値）、札の所有者が死んでいる Gated の便は verdict が PASS の周も INCONCLUSIVE の周も今までどおり起こされ、段を前へ進めなかった driver の終端の 1 周はこの候補を 1 本も起こさず、待ちの段の候補の規則と待ちの段でない Gated 以外の便の規則を測る既存の歯（pipe_dispatch_driver_ と pipe_dispatch_waiting_gate_）は測っている約束を変えずに緑のまま"

[[contract]]
id = "m"
title = "列外の鍵に審査役へ渡る材料を含める — Reviewed で終端した便の鍵に、行の section が指す § の本文（審査の材料の dir の写し）を足す（§ の読みは審査と同じ 1 本・写しが無い周は契約 file だけの鍵に倒す・Reviewed 以外の段の鍵は不変・新しい file も field も足さない）"
req = ["FR68", "FR49", "NFR4"]
section = "16"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs"]
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
place = "crates/scribe2/tests/e2e/pipe/dispatch.rs"
fixture = "偽の台帳に ready の bead 1 本と toy repo を置いて pipe dispatch の 1 周を撃つ。負の枝は event log を読み取り専用にして印が書けない周"
expect = "event log に bead 名義の DispatchMark mark=launched が RunCreated より前の行として在り、印が書けない周は子が起きず dispatch=started:0 で ls の理由が admission:mark（測れない側）に出る"

[[promise]]
of = "n"
n = 2
text = "最新の Launched より後に RunCreated も Release の印も無い bead は起こさず、dispatch ls の理由が launched:<ts> になる（WaitReason に 1 値 Launched）"
files = ["crates/scribe2/src/pipe/dispatch.rs"]
symbols = ["crate::pipe::dispatch::WaitReason", "+WaitReason::Launched"]
teeth = ["pipe_dispatch_launched_bead_is_not_relaunched_until_run_created_or_release"]
place = "crates/scribe2/tests/e2e/pipe/dispatch.rs"
fixture = "偽の台帳の ready の bead に DispatchMark launched だけを積んだ event log で 2 周目を撃つ。対照は Launched の後に RunCreated を積んだ log と、Launched の後に Release を積んだ log の 2 つ"
expect = "印だけの周は起こさず ls の理由が launched:<ts>、RunCreated の後は理由が live の側（overlap）に変わり、Release の後の周は起こす（started:1）"

[[promise]]
of = "n"
n = 3
text = "spawn_self の stderr を <state_dir>/pipe/launch.log に append し、file を開けない周は null に落として起動を止めない"
files = ["crates/scribe2/src/pipe/dispatch.rs"]
teeth = ["pipe_dispatch_launch_log_keeps_the_child_stderr"]
place = "crates/scribe2/tests/e2e/pipe/dispatch.rs"
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
files = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/health.rs", "crates/scribe2/src/pipe/gate.rs"]
symbols = ["crate::pipe::dispatch::WaitReason", "+WaitReason::HostBusy", "crate::pipe::health::Breaker"]
teeth = ["pipe_dispatch_host_busy_round_launches_nothing"]
place = "crates/scribe2/tests/e2e/pipe/dispatch.rs"
fixture = "rules fixture の host.runnable_per_core を 0（閾値 0 = 常に Busy）にした周と既定の値の周の対で、偽の台帳に ready の bead 1 本を置いて 1 周を撃つ"
expect = "0 の周は dispatch=started:0 で全候補の ls の理由が host-busy、既定の周は started:1"

[[promise]]
of = "o"
n = 2
text = "live の Stage::Intake の枝を運転手の札で読む: Live なら true・Dead / Absent なら false・Unreadable なら None（他の段の枝は不変・新しい probe は足さない）"
files = ["crates/scribe2/src/pipe/cli/state.rs"]
symbols = ["live(", "crate::pipe::Ticket"]
teeth = ["pipe_dispatch_intake_run_without_a_live_driver_is_not_live"]
place = "crates/scribe2/tests/e2e/pipe/dispatch.rs"
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
files = ["crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/notify.rs", "crates/scribe2/tests/e2e/pipe.rs", "+crates/scribe2/tests/e2e/pipe/notify.rs"]
symbols = ["seat::inject::Request"]
teeth = ["pipe_notify_terminal_failure_reaches_the_registered_seat_pane", "pipe_notify_without_a_registered_seat_reports_no_seat"]
place = "+crates/scribe2/tests/e2e/pipe/notify.rs"
fixture = "偽の tmux（send-keys の引数を file に記録する script）を PATH に置き、SeatRegistered の row（role orchestrator・anchor = toy repo・target = 任意の pane 名）を state dir に積んだ上で、live な run に pipe stop --run を撃つ（Stopped は終端の 1 つ）。負の枝は row を積まない"
expect = "記録に send-keys が 1 回だけ在り payload が bead と run と Stopped を含む 1 行で stdout に notify=delivered、row 無しの周は send-keys 0 回で notify=no-seat"

[[promise]]
of = "p"
n = 2
text = "同じ周の列の結果が起こした便 0 ∧ 候補 1 本以上のとき、同じ宛先へ idle の 1 行（ready=<本数> launched=0 reason=<先頭の候補の理由>）を送る（Landed と PASS の終端でも列が idle ならこの 1 行だけ送る）"
files = ["crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/notify.rs", "+crates/scribe2/tests/e2e/pipe/notify.rs"]
teeth = ["pipe_notify_idle_round_reports_ready_count_and_top_reason"]
place = "+crates/scribe2/tests/e2e/pipe/notify.rs"
fixture = "偽の台帳の ready の bead 1 本を hold にした state dir（起こす 0 ∧ 候補 1）と登録 row と偽の tmux を置き、pipe stop --run の終端を撃つ。負の枝は候補 0 の台帳"
expect = "idle の 1 行に ready=1 launched=0 reason=hold が在り、候補 0 の周は idle の行を送らない（send-keys は終端の 1 行だけ）"

[[contract]]
id = "q"
title = "pipe/dispatch.rs の「台帳から候補を組む」群（18 item・351 行）を子 module candidates へ割る — 純移動・親に増えるのは mod 1 行と use 2 文・in-file の歯 14 本は動かさない・e2e の歯の file は 1 byte も変えない"
req = ["FR68", "NFR4"]
section = "20"
write-set = ["-crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_dispatch_drive_ pipe_dispatch_launched_ pipe_dispatch_marks_ pipe_dispatch_order_ pipe_dispatch_release_ pipe_dispatch_section_ pipe_dispatch_wait", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_"]
size = "S"
done = "(1) 18 item が名・本文・順序を変えずに子へ移り、flip-check の moved の機械証明が残差 0 (2) 親に増えるのは mod 1 行と use 2 文だけで turn / fire / revivals の本体は不変 (3) in-file の歯 14 本と e2e の pipe_dispatch_ の歯が 1 字も変わらず緑 (4) 親の行数が約 1065 で余地が 400 以上 (5) tests/e2e/pipe/dispatch.rs の diff が 0 行"
<!-- contracts:end -->
