# 設計: 台帳の形 — memo と契約の状態を bd の field で閉じ、台帳を契約表から生成する

出所: user の裁定 2026-09-21（逐語は台帳 `s2-07l.510` の notes）。「memo と契約の形は閉じ切る話で、beads の機能と合わせて先に設計する。beads の機能を活かしきれていない」。決定は [ADR-0051](../../design-intent/decisions/ADR-0051-contract-rows-carry-promise-rows-and-ledger-state-is-two-fields.html) §4。契約の側（約束の行）は [contract-source.md](./contract-source.md) §33 が持ち、本 doc は**台帳の側**だけを持つ。台帳の規律は憲法 C15（bead が持つのは task と裁定だけ）のとおり `.beads/PRIME.md` と本 doc の行に置き、bead の本文には置かない。

## 1. 何を解くか

- **memo が見つからない**（user の指摘 2026-09-20）: `bd search` は title だけを見る。memo の本文は自由文で、出所も昇格の条件も書式が無いので、議題に当たる memo を機械で引けない。
- **memo と契約の境が field に無い**: 今の弁別は label `intake:memo` と acceptance の pointer 行（`design = <doc>#<id>`）の 2 つで、両方在る・両方無い の 2 形が禁じられていない（契約表の行 e〔contract-source.md §6〕は pointer の解けない契約・pointer 無しの memo・本文を持つ契約の 3 欠陥だけを数える）。
- **台帳の起票が手番**: 契約表の行から bead を起こすのは席の手で、label の継承・edge の張り忘れ・親の付け忘れが起きる（2026-09-21 実測: open 93 bead のうち本文で id を名指すのに edge の無い対が 91、label の体系を後から 76 bead に機械で足した）。
- 母集団（2026-09-21・`bd list --json`）: open 93・memo（label `intake:memo`）16・契約（acceptance に pointer 行）約 60・edge の種類は `blocks` / `parent-child` / `relates-to`（後者は本日の埋め戻し 178 本）。

## 2. 現物（verified 2026-09-21・bd 1.1.0）

- bd の型: built-in 9（task / bug / feature / chore / epic / decision / spike / story / milestone）+ `types.custom`（`.beads/config.yaml`）。**`bd lint` の必須節は built-in の型に固定**（bug = Steps to Reproduce + Acceptance Criteria・task / feature = Acceptance Criteria・epic = Success Criteria）で、custom の型の節は測らない。`--validate` は create / close で同じ表を見る。
- bd の field（create の flag）: `--acceptance` / `--design` `--design-file` / `--spec-id` / `--external-ref` / `--metadata`（JSON）/ `--deps type:id`（10 種）/ `--defer <date>`（ready から隠す）/ `--estimate <分>` / `--graph <JSON>`（複数 bead と edge を 1 回で作る）/ `--no-inherit-labels`。
- 器の台帳の口: 読みは `crates/scribe2/src/seat/ledger.rs` の `read_ledger(`（全件 JSON・1 件の型 `Issue` は id と status と dependencies）、書きは `crates/scribe2/src/ledger/mod.rs` の `close(` の 1 種だけ。lint は行 e が置く（未着地・`s2-07l.371`）。
- 台帳 triage（stale の閾値と復元の marker）は ADR-0045 §2 (2) で超過し消えた（[ledger-triage.md](./ledger-triage.md)）。本 doc は閾値を持たない。

## 3. 形（field と行だけ・散文の免除を持たない）

1. **memo の識別は label `intake:memo` の 1 つ**（型を足さない）。理由: bd の custom 型は lint の節も validate も持たず、識別子が 2 つになるだけ（却下案）。契約の識別は acceptance の pointer 行の 1 つ（行 e と同じ）。
2. **memo の本文の節**（`## memo` の下・行 e の固定の見出しの下に 4 つ・宣言順）: `### 出所`（run id / PR 番号 / 裁定 id / user 逐語の在り処のいずれか 1 行以上）/ `### 観測`（実測の事実・件数は母集団と対）/ `### 候補`（0 個以上・却下も書く）/ `### 昇格条件`（SRS の要件 id か「要 ADR」か「要 裁定」の 1 語）。節の有無は台帳 lint が数える（下の 4）。本文の他の部分は自由。
3. **状態機械は field の 4 象限で閉じる**（判定は純関数・散文の免除なし）:

| label `intake:memo` | acceptance の pointer 行 | 状態 | 
|---|---|---|
| 在る | 無い | memo（設計前） |
| 無い | 在る | 契約（設計 pointer 済み・便の対象） |
| 在る | 在る | 違反（昇格が途中で止まった） |
| 無い | 無い | 違反（形の無い bead・epic と裁定の bead は除く） |

   memo → 契約 の遷移は「acceptance に pointer 行を書き、label を外し、出所の memo へ `discovered-from` を張る」の 3 書きで、席の手番（bdw）。器は写さない（台帳の書きは close の 1 種のまま・C15）。
4. **台帳 lint の項目を 5 つ足す**（行 e の 3 欠陥の後ろ・同じ 1 行に件数と母集団と id・doctor が唯一の口〔FR51〕）: (iv) memo の 4 節のどれかが無い bead／(v) 4 象限の違反 2 形の bead／(vi) 契約で、pointer の先の § の本文か bead の本文（description と notes）が memo の id を名指すのに `discovered-from` の edge が無い bead（§ は行 e の pointer の読み手が既に開く doc の本文・memo の id は label `intake:memo` の bead の id と字面で照合）／(vii) 契約表の未着地の行（下の 5 の弁別）のうち pointer を持つ open の bead が無い行（台帳と契約表の drift）／(viii) `discovered-from` で辿れる契約が全部 closed なのに open な memo（下の 10 の取りこぼし）。読めない周は行 e と同じく件数 0 に倒さず測れていない形で出す（C10 / NFR4）。
5. **台帳は契約表から生成する**: xtask の口が全設計 doc の契約表を読み、**未着地の行**（write-set の `+` の file が tracked に無い行・§33 の `symbols` の `+` も同じ読み）ごとに bead の plan（title = 行の title・parent_id = 口の引数 1 つで渡す epic id〔本 repo は program の epic 1 本・doc ごとの対応表は持たない〕・edge = 行の `depends` → `blocks`（to_key）・§ の本文が名指す bead の id（台帳の id の字面・memo か否かは問わない＝xtask は台帳を読まない）→ `discovered-from`（to_id）・label = doc 名の `doc:` と size の `size:`）を `bd create --graph` の JSON で標準出力の 1 行目に出す。acceptance = pointer 行は graph schema に無いので、JSON の次の行から「plan の key TAB pointer 行」の対応表で同じ標準出力に出し（node と同じ本数・§9）、席が create の出す「key -> id」と突き合わせて acceptance を追い書きする。closed の bead が指す行は `--skip <契約 id>` で plan から外す（plan に無い id は rc 1）。**apply は席の手番**（bdw で撃つ・器は台帳を書かない）。xtask は台帳を読まない（CI は台帳に届かない・plan は行だけから出す）。台帳側の drift は doctor の lint の (vii)（上の 4）が測る。id は bd の採番なので、行 ↔ bead の対応は acceptance の pointer 行で引く（新しい key を作らない）。
6. **棚上げは `--defer`**（label や notes の「棚上げ」の語を規則にしない）。`bd ready` が隠すので列の観測（dispatcher.md §6）と整合する。
7. **見積は `--estimate`（分）**を size から写す（S / M / L の分の値は便の実測の中央値で、値の正本は rules 行〔後続・C5〕）。Jev の較正の材料（見積と実測の差）はここから取る。
8. **memo の入口は器の口 1 つ**（これから起きる memo の形を起票の時点で決める・回り続ける周のため）: 器の read-only の口が memo の plan（bd の create の引数と本文の 4 節）を標準出力に出し、席が bdw で撃つ。出所は 2 つの形だけ: (a) **便の終端から**（`--run <id>`）— 便の終端の event（gate / 審査の FAIL・INCONCLUSIVE・Failed・Questioned）を読み、`### 出所` に run id と段と kind を、`### 観測` に**終端の種類ごとの原本**を器が写す（人が写さない・C10）。原本は 4 形で閉じる: gate の FAIL・INCONCLUSIVE = run dir の `verdict.json` の evidence と at／審査の FAIL・INCONCLUSIVE = run dir の `review.json` の evidence と at／Questioned = event log の質問の逐語と `about`（既存の読み手 `crates/scribe2/src/pipe/mod.rs` の `Question`・FR31・run dir に file は無い）／Failed = event log の `RunStage(Failed)` の detail（閉じた理由の字面）と ts（verdict は無い）。どの形にも当たらない終端（detail が空・log を読めない）は写さず閉じた理由で断る（fail-closed）。`### 候補` と `### 昇格条件` は空の見出しで出し、席が埋める（昇格条件には読める引き金の行〔§15〕を 1 行以上書いてから撃つ。空のままの create は起票の門が no-trigger で断る）。(b) **user の要望から**（`--from user`）— `### 出所` に「user 逐語は本 bead の notes」の 1 行と日付を置き、席が逐語を notes に写す。label `intake:memo`・parent の epic（引数）・関連 bead（引数の列 → `relates-to`）も plan に載る。器は台帳へ書かない（ADR-0045 §2）。
9. **起票の門は guard**（in-loop・fail-closed・極性一覧に 1 つ増える〔ADR-0014 §2.1〕）: PreToolUse の hook が `bd create` / bdw の create の command を読み、label に `intake:memo` を持つか title に `[memo]` を持つ周は、`--body-file` の本文に memo の 4 節の見出しが全部在ることを要求し、無ければ閉じた理由 1 つで止める（散文の免除なし）。契約の bead（acceptance に pointer 行）の create は label `intake:memo` を持たないことを要求する（4 象限の違反 2 形の 1 つを起票の時点で塞ぐ）。読めない command（body-file が無い・開けない）は止める側に倒す。label intake:question の create の形（§14）・memo の引き金の行（§15）も同じ門が台帳を読まずに断る。close の理由（§16）は、vessel 宣言が close-check を true で持つ repo と宣言が在って読めない repo でだけ同じ門が断る（ADR-0097）。
10. **memo の close は器が行う**（書きの口は既存の close の 1 種・増えない・本項の条件と理由の字は ADR-0089 が置き換えた）: land の終端の 3 つの経路（ADR-0094・`pipe retire` を含む）が契約を close した周と dispatch の周に、自動の close の条件（SRS FR93）を満たす memo を §16 の昇格済みの形（`昇格済み <契約 id の列>`）で close する。条件を満たさない memo と台帳が読めない周は close せず、局面の出力（SRS FR90・FR91）が memo-actionable か misfit で名指す（fail-closed・台帳 lint の (viii) の役は局面の出力へ移る・close の段と (viii) の始末は局面の出力の設計の行が持つ）。

## 4. やさしく言うと

memo か契約かを「label が在るか」と「受入条件に設計の 1 行が在るか」の 2 つだけで決め、それ以外の組み合わせは全部「壊れている」と数える。memo の本文は 4 つの見出しを必ず持ち、無ければ doctor が名指す。契約の bead は設計書の表から機械が起こす案を出し、人はそれを撃つだけにする。

## 5. 触らない

- bd の version と `types.custom`（型を足さない）・`bd remember` 系（使わない・PRIME）・台帳の書きの口（`close` の 1 種のまま）・行 e の 3 欠陥の判定と字面・PRIME の R0〜R6（R3 は label のまま・本 doc の節 2 の見出しは PRIME に写す＝docs PR）・rules 行（見積の値は後続）。

## 6. 歯

- 行 a（`ledger_form_` 接頭辞・判定は行 a の write-set の `+` の file に純関数で・doctor の行は既存の口に 1 行）: (a) 4 節の欠けを**件数を違えて**持つ fixture（出所なし 1・観測なし 2・候補なし 0・昇格条件なし 3）で件数と母集団と id が出る／(b) 4 象限の違反 2 形が別々に数えられ、epic と裁定の bead が母集団から外れる／(c) `discovered-from` の無い契約が § の本文の名指しと bead の本文の名指しの両方から数えられ、どちらにも名指しの無い契約は数えられない／(d) 読めない周は測れていない形の行（件数 0 でない）／(e) 外形 snapshot（新しい doctor の 1 行）／(f) drift (vii): 未着地の行 2 本のうち pointer を持つ open の bead が 1 本だけの fixture で 1 と母集団 2 が出る（契約表は toy repo の doc・tracked の集合は toy repo の index・台帳は偽の client）。
- 行 c（`ledger_memo_plan_` 接頭辞・置き場は行 c の write-set の `+` の歯の file）: (a) 便の終端の 4 形（gate の FAIL = `verdict.json`・審査の FAIL = `review.json`・Questioned = event log の質問の逐語と about・Failed = event log の Failed の detail と ts）を**別々の fixture** で持ち、それぞれ `### 出所` と `### 観測` に原本の字面が写り、`### 候補` と `### 昇格条件` が空の見出しで出る（終端でない run と、原本の無い終端〔detail 空〕は断る）／(b) `--from user` の plan が出所の 1 行と日付を持ち観測が空／(c) label と parent と `relates-to` の引数が plan に載る／(d) 出力は標準出力だけで台帳に 1 件も書かない（偽の bd が呼ばれない）／(e) usage の外形 snapshot。
- 行 d（`hook_memo_guard_` 接頭辞・置き場は既存の hook の歯の file）: (a) `[memo]` の title か `intake:memo` の label を持つ create で body-file の 4 節が揃えば通り、1 つでも欠ければ閉じた理由で止まる／(b) acceptance に pointer 行を持つ create が `intake:memo` を持てば止まる／(c) body-file が無い・開けない周は止まる／(d) memo でも契約でもない create（epic・裁定）は従来どおり通る／(e) 極性一覧の snapshot に guard が 1 つ増え、guard の総数を pin する歯が新しい母集団で緑。
- 着地の終端の memo の close（行 e の後続・[contract-source.md](./contract-source.md) §5 の終端に 1 段足す・本 doc の行にはまだ入れない）: 行 e が land した後に contract-source.md の行として起こす（write-set が行 e の `+` の file を含むため・§8）。
- 行 b（`ledger_plan_` 接頭辞・xtask）: (a) 未着地の行だけが plan に載り着地済みの行は載らない（`+` の file が tracked に在る行 = 着地済み）／(b) `depends` が `blocks` に・§ の本文が名指す bead の id（字面）が `discovered-from` に写り、名指しの無い行は edge 0／(c) title と acceptance の pointer 行と label が行から写る／(d) epic id は口の引数 1 つで、無い周は plan を出さず rc 1（fail-closed・既定に倒さない）／(e) 台帳を 1 度も読まない（PATH の先頭に置いた偽の bd が呼ばれない）。

## 7. 却下案

- memo を bd の custom 型で名乗る（lint も validate も効かず識別子が 2 つになる）／memo の節を bd lint に任せる（built-in 型に固定）／台帳を正本にする（契約の正本は設計 doc の行・FR47・ADR-0023）／器が台帳へ起票する（書きの口が増える・C15・席の手番のまま）／stale の閾値で memo を棚卸しする（ADR-0045 で超過・`--defer` で足りる）／memo の状態を label の増設で表す（label は AND filter しか無く排他を測れない・4 象限は 2 field で閉じる）。

## 8. 後続

- 見積の値（S / M / L の分）を rules 行に持つ（C5・裁定 id が要る）。
- 台帳の全文検索の口（`bd search` は title だけ）: 器の read-only の 1 口として別の行に起こす（usage の外形 snapshot が動くので本 doc の行には入れない）。
- 台帳の生成の要件（FR）は SRS に無い。要件面の改訂は user の `/folio-architect` の手番。
- 着地の終端が memo を close する段（節 3 の 10）は、行 e（台帳 lint）の land 後に contract-source.md の行として起こす（`discovered-from` を辿る読み手は行 e の 1 件の型に dependencies が要る）。
- 便の終端から memo の plan を出す口（節 3 の 8 (a)）と、終端を席へ知らせる経路（`s2-07l.507`）は同じ event を読む。通知の経路の設計は .507 の側で、本 doc の口はその経路が呼ぶ read-only の 1 口。

## 9. plan JSON を bd の graph schema へ合わせ、pointer 行の追い書きと除外の口を持つ（契約表の行 e・`s2-07l.536`）

- 出所: memo `s2-07l.536`（orchestrator の実測 2026-09-21・追記 2026-09-22）。棚卸し 2026-09-22 に使い捨ての台帳で再現した（repo の台帳は触っていない）。
- 何が起きているか（bd 1.1.0・main 4f70b12・verified）: `crates/xtask/src/ledger_plan.rs` の `render` が出す node の key は key / title / type / acceptance / parent / labels、edge は from / to / type。bd は acceptance と parent と from と to を**知らない field として黙って落とし**（warning 1 行ずつ）、「edge 0: must specify from_key or from_id」で 1 件も作らない（rc ≠ 0・台帳は無傷）。実測で通る field は node が key / title / type / description / labels / priority / parent_key / parent_id / metadata / assignee の 10、edge が from_key / from_id / to_key / to_id / type の 5。**acceptance は graph schema に無い**＝plan だけでは §3 の 3 の「契約 = acceptance の pointer 行」を満たす bead を作れず、作れば 4 象限の違反（両方無い）に落ちる。成功した graph の create は「plan の key -> bead id」の対応を標準出力に出す（実測）。edge の type は知らない値でも通る（もう 1 つの fail-open 面）。
- 何が起きているか 2（追記の観測・verified）: `landed` は宣言した印つきの file が tracked に在るかだけを見るので、**指す bead が closed でも file が tracked に無い行**を未着地に数える。`consumer-sync.md` の行 f が宣言する新規 file は repo の履歴に 1 度も現れず（追加された path の全数 352 に無い）、指す bead `s2-07l.325` は closed なので、行 f は今も plan に載る（`s2-07l.534` の重複起票の型）。doctor の `drift`（`crates/scribe2/src/ledger/form.rs`）も「未着地 ∧ **open** の bead が指さない」で数えるので同じ偽陽性を持つ。
- 形（done と 1:1）:
  1. node の key を key / title / type / description / labels / parent_id に、edge の key を from_key と to_key（plan の中の行）/ to_id（台帳の既存 bead）と type に直す。出す key の集合は閉じた const 2 本で持つ。
  2. graph schema が運べない pointer 行は、plan JSON の**次の行から**「plan の key」と pointer 行を TAB で並べた対応表として同じ標準出力に出す（stdout の口は `emit` の 1 関数のまま・呼び出しも 1 回）。席は graph の create が出す「key -> id」と突き合わせ、acceptance の update を撃つ（apply が席の手番なのは §3 の 5 のまま）。
  3. 行を plan から外す口 --skip（契約 id の list・既定は空・知らない id は rc 1 で断る）を足し、usage の 1 行に写す。席は doctor の 1 行から closed の bead が指す行の id を渡す。**xtask は台帳を読まない**不変（§3 の 5・行 b の歯 (e)）は保つ。
  4. bd の版を持つ行も const も増やさない: 知らない field は warning と rc ≠ 0 で落ちるので、schema が動けば apply が黙らずに失敗する（C5 の裁定を要らなくする）。§3 の 5 の「acceptance = pointer 行」の字面を、対応表で追い書きする形に写す。
- 触らない: `landed` の印つきの項目の読み（`symbols` の path 形を含む）／`build` の行の選び方と edge の向き／`tracked_files` の git ls-files 1 本（子 process を増やさない）／`emit`／doctor の `drift` の式（同じ偽陽性を持つが台帳を読む側の話＝後続）／bd の呼び出し（器は 1 度も起こさない）。
- 却下: plan を捨てて 1 node 1 回の create の引数列に描く（memo の候補 2。plan の中の行どうしの blocks が席の id 置換になり、§1 が数えた「edge の張り忘れ」を手番へ戻す）／pointer 行を description に入れて §3 の 3 の識別を description 読みに変える（着地済みの状態機械と行 a の歯を動かす）／metadata に pointer 行を隠す（acceptance を読む判定に届かない）／xtask から台帳を読んで closed を弁別する（行 b の歯 (e) が禁じる不変）。
- 後続: doctor の 1 行に「closed の bead が指す未着地の行」を足して `drift` から外し、形の 3 の値をその行から機械で取れるようにする（scribe2 側＝別の行）。
- 歯（行 e が持つ・置き場は `ledger_plan.rs` の in-file の歯・接頭辞 `ledger_plan_` は既存なので**名の全体**で書く）:
  - `ledger_plan_renders_the_bd_graph_schema_field_names`: 出力 1 行目の node が parent_id を持ち parent と acceptance を持たず、edge が from_key と to_key と to_id を持ち from と to を持たない（base は逆＝RED）。
  - `ledger_plan_emits_the_pointer_line_table_after_the_plan`: 出力の 2 行目以降が node と同じ本数で、各行が plan の key と「design = <doc>#<行 id>」を TAB で持つ（base は 1 行だけ＝RED）。
  - `ledger_plan_skips_the_rows_named_by_the_skip_argument`: 名指した行が plan から消え、残りの行と edge が不変で、plan に無い id を渡した周は rc 1（base は知らない引数で rc 1＝RED）。

## 10. 台帳のグラフの形を doctor の 1 行で数える — 根の epic に着かない bead・2 つ目の親・親の輪・直下の open の子が上限を越えた親・子が全部 closed の open な epic（契約表の行 f・持ち主の提案 2026-09-27・裁定 user 2026-09-27T14:02Z）

やさしく言うと: 台帳の bead は「epic の木」にぶら下がる形にそろえる。どの bead も親をたどると根の epic に着き、親は 1 つで、親子が輪にならない。1 つの親の下に open の子が多すぎたら溢れと数える。まず器がこの形の崩れを数えて名指し、直す形を添える（崩れを増やす書きを断るのは §12）。

- 出所: 持ち主の提案（2026-09-27・要約）— 台帳のグラフ（epic の木と blocks）を器の決まりに組み込めば関係が読みやすくなり、棚上げと消化不良が起きにくい。設計と実装が進むと epic が増えて複雑になるので整理が要り、orchestrator が自律して行えるよう器の側に強制の機能が要る。器に乗る全 project に効かせ、違反は減る向きにしか動かさない（増やす書きだけを断る・ratchet）ことは裁定 user 2026-09-27T14:02Z が是認した（上限の値だけは別の裁定・形 4）。
- 何が起きているか（実測 2026-09-27・各 repo で `bd --readonly list --all --limit 0 --json`・下の形 1 の定義で数えた・verified）:
  - scribe2（741 本）: 根の epic は 1 本で、その直下に 691 本が平らに付く（次に多い親は 9 本）。今の open 11 本は根の epic とその直下の 10 本で、10 本とも memo。根に着かない bead は closed の 17 本（全部が親を持たない v1 時代の bug / task）で、open は 0。2 つ目の親と親の輪は 0。今の親子のまま created_at と closed_at から数えると、根の直下の同時に open の子は最大 121 本。
  - 非公開の隣の project 1（54 本）: 全部が根に着く。子が全部 closed の open な epic が 3 本。
  - folio2（265 本）: epic が 0 本で、feature 型の 1 本が 264 本を直下に持つ（open の子 35）。根が epic でないので 265 本全部が根に着かない（open 36）。その 1 本の型を epic にする 1 書きで全部が着く。
  - 非公開の隣の project 2（468 本）: 根の epic 12 本。根に着かない bead は closed の 80 本（全部が親を持たない bead）で、open は 0。直下に open の子 24 本を持つ epic が 1 本。
  - 器に乗る他の 4 project（どれも非公開・計 1714 本）: 根に着かない open 154 本（鎖の終わりの非 epic は 134 本）・closed だけの鎖の終わり 1107 本・2 つ目の親 1 本。
  - 8 project とも blocks の輪は 0（`bd graph check` と自前の数えが一致）。
- 決定はしご（C17・bd 1.1.0 を repo の外の使い捨ての台帳で撃った・repo の台帳には書いていない・verified）:
  - blocks の輪は bd が書きの時点で断る（`dep add` の 2 本目が「would create a cycle」で落ち、`--no-cycle-check` を付けても落ちる）。`bd graph check` は blocks の輪だけを数える。
  - parent-child は bd が守らない: `update X --parent <X の孫>` が通って親の輪ができ、`dep add <子> <別の親> --type parent-child` が通って親が 2 つになり、どちらの後も `bd graph check` は clean を返す。`update --parent ""` は親を外す（孤児ができる）。
  - `bd doctor` は embedded mode で動かない（scribe2 の台帳で「not yet supported in embedded mode」）。`bd epic status --eligible-only` / `bd epic close-eligible` は子が全部 closed の epic を引く・閉じる。`bd list --no-parent --exclude-type epic` は親を持たない非 epic を引く。`bd stale --days` は更新の古い bead を引く。`bd statuses` の組み込みの状態 pinned は「常設・閉じない」（frozen の分類）で、8 project とも使用 0 本。
  - 「親をたどると根の epic に着くか」と「直下の open の子の数」は bd に無い。
- 現物（main 8f6072d・verified）: 台帳の読みは `crates/scribe2/src/seat/ledger.rs` の `read_ledger(`（全件を 1 回・1 件の型 `Issue` の `deps` が parent-child を運び `kind` が型を運ぶ）。doctor の台帳の行は `crates/scribe2/src/ledger/lint.rs` の `doctor_lines(`（`one_read` の区間で台帳 lint の行 → 台帳の形の行の順・台帳は 1 回だけ読む）。同じ決まり（根に着く・closed も数える／blocks と parent-child に輪なし・親は 1 つまで）を非公開の隣の project 1 が自分の repo の中に持つ（器からは呼べない）。
- 形（番号は done と 1:1）:
  1. **判定は兄弟 module の純関数 1 本**（行 f の write-set の `+` の file・`crates/scribe2/src/ledger/mod.rs` は宣言の 1 行）: 入力は `Issue` の列と上限 N だけ。親は `deps` の parent-child の最初の 1 本。根は親を持たない epic。根に着かないは、親をたどって根に着かない（親が台帳に無い・輪に入る・親を持たない非 epic で止まる）こと。鎖の終わり（top）は、根に着かない鎖が止まった非 epic。定義は非公開の隣の project 1 の同じ決まりと揃える（下の v3 の段取り）。起票の門（§12）も同じ関数を引く（付け先が根に着くか・その top・直下の open の子の数・子孫か）ので、それらを module の外へ見せる。
  2. **数えるもの**: (a) 根に着かない bead の件数を open と closed に分けて (b) top を「open を含む鎖の top」（id を名指す）と「closed だけの鎖の top」（件数だけ・`bd list --no-parent --exclude-type epic` で引ける）に分けて (c) 親を 2 つ以上持つ bead (d) 親の輪に乗る bead (e) 直下の open の子（closed でも pinned でもない子）が N を越える親の id と子の数 (f) 子を 1 本以上持ち、子が全部 closed の open な epic。N が 0 の周は (e) を数えない。
  3. **doctor の 1 行を台帳 lint の行と台帳の形の行の間に足す**: `doctor_lines(` の同じ `one_read` の区間で撃つ（台帳は 1 回だけ読み、台帳の形の行が doctor の末尾のまま）。行は台帳のグラフの接頭辞で始まり、beads= open= max= unrooted= unrooted-closed= tops= tops-closed= two-parents= parent-loops= over= close-eligible= の順。id の欄は台帳の形の行と同じ `<件数>:<id>,…`（0 件は `=0`）、over の id は `<id>/<子の数>`、N が 0 の周の over は `-`。違反が 1 つ以上の周だけ、行の末尾に ` — ` と直す形（top は epic の親を付けるか型を epic に・溢れは子 epic を作って付け替え・close-eligible は close）を 1 回添える。外形 snapshot は測れた周と測れない周の 2 行を持つ。
  4. **rules 行を 1 本足す**: id は ledger.open_children_max・kind は LedgerOpenChildrenMax（Int・本数）・値は 15・裁定 id は user 2026-09-27T17:33Z 項 2-3（値の裁定。裁定 user 2026-09-27T14:02Z は効かせる範囲と ratchet を決めたが値は決めていない）。rules-diff は base の別の行の裁定 id と同じ字面を断る（相乗り）ので、値の裁定 id は行 g が `ledger.denied_writes` に写す user 2026-09-27T14:02Z と別の字面になる。manifest の行は `ledger.denied_writes` の直後、kind は `ALL` の LedgerDeniedWrites の直後。id は src の const 1 つで持ち、`timeout_of(` と同じく id で引いて `Int` だけを読む（`RuleKind` の variant を判定の側で名指さない・rules-wired の読み手）。kind を LedgerDeniedWrites の直後に置くので、その位置からの並びを測る `rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all` の期待の列に LedgerOpenChildrenMax を 1 つ足す（末尾の 6 つの並びは動かない）。行が無い・形が違う周は台帳のグラフの行を測れない形（reason=no-rule）で出す。読めない台帳・読めない rules も測れない形（件数 0 に倒さない・C10 / NFR4）。
  5. **blocks の輪は数えない**（bd が書きの時点で断る・決定はしご）。
- v3 の段取り（C2・本行の done の外）: 本行の判定は、非公開の隣の project 1 が持つ同じ決まりの 2 本目の実装になる。定義（根・top・輪・親 2 つ・closed も数える）をそちらと揃えて置き、v3 の合流で doctor と起票の門の呼び出しを 1 本の判定へ向け替えて、本行の `+` の file を消す。上限 N と close-eligible はそちらに無いので、合流の時に足す。
- 足す前に消すもの（C17.2）: 台帳のグラフを席が手で数える使い捨ての集計（2026-09-27 に 8 project で撃った数え）を doctor の 1 行に置き換える。
- 触らない: 台帳 lint の行と台帳の形の行の字面と順・`read_ledger(` と `Issue` の field（parent-child は `deps` で足りる）・台帳の書きの口（`close` の 1 種のまま・C15）・SessionStart の注入（台帳の 1 行の字面・ADR-0045 §2 (3)）・blocks の輪（bd）。
- ADR-0045 §2 (2) との関係（消した口の復活か）: 超過した棚卸し（[ledger-triage.md](./ledger-triage.md)）は memo の滞留を**時間の閾値**（rules 行 ledger.memo_stale_days / ledger.memo_stale_priority）で測り、**復元の DATA（SessionStart の注入）**に marker を出す形で、読み手（作業記憶の復元）ごと消えた。本行は (a) 時間の閾値を持たない（数えるのは親子の形と status だけ・上限は本数）(b) 出す面は doctor の 1 行（引く側・FR51 の唯一の口）で、SessionStart の注入に 1 語も足さない (c) 読み手は起票の門（§12・書きの時点で断る）と doctor を撃つ席、の 3 点で棚卸しと違う。「長く動かない epic」（更新からの日数）は数えない（時間の閾値と marker の復活になる・日数で引くなら `bd stale --days` が在る）。ADR-0045 §2 (2) は session を跨ぐ記憶を台帳の構造（metadata / status / 依存）に置いたので、構造を木に閉じる本行はその前提を強める側である。
- 歯（接頭辞 ledger_graph_・`grep -rn "fn ledger_graph_" crates/` は 0 件・2026-09-27）:
  - e2e（`crates/scribe2-boundary/tests/e2e/ledger_form.rs`・既存の偽の client と toy repo を使う・新しい e2e の file は作らない＝e2e の file 数の pin と flip-check の同梱の条件）: (a) 根の epic 1・その子 3（open 2・closed 1）・親を持たない open の task 1・親を持たない closed の task 1・feature の top の下の open 2・親 2 つの task 1・親の輪 2 本・子が全部 closed の open な epic 1 を持つ fixture で、各欄の件数と id と末尾の直す形が出る (b) 直下の open の子 N+1 本の親と N 本の親を持つ fixture で、over に前者だけが `<id>/<N+1>` で出る (c) 違反 0 の周も行が出て、末尾の直す形が無い (d) 台帳を読めない周は unreadable reason=ledger-unreadable で件数を 1 つも出さない (e) (a) と同じ周で、偽の client の記録が 1 行（台帳は 1 回だけ読む）で台帳の形の行が doctor の末尾のまま（base で緑になる assert を単独の歯にしない） (f) 埋め込みの manifest の写しで行 ledger.open_children_max の値だけを 0 にした rules の file を `--rules` で渡した周は、(b) と同じ fixture で over が `-` になり、N+1 本の子を持つ親も over に載らない (g) 同じ写しから行 ledger.open_children_max を除いた rules の file を渡した周は、台帳のグラフの行が unreadable reason=no-rule になり件数を 1 つも出さない。(f) (g) は rules を差し替えないと届かない周で、doctor の既存の口が届く: `crates/scribe2-boundary/src/main.rs` の `render_doctor_with` は `--repo R` と `--rules F` を `--state-dir` 無しでも受け、台帳の行の読み手（`crates/scribe2/src/ledger/lint.rs` の `doctor_lines`・引数は repo と rules の path）へ同じ F を渡す（main d2cf7d6・verified）。(b) の N も同じ口で小さい値（2）の写しを渡してよい（fixture の子の本数を 16 にしない）。base では台帳のグラフの行が無いので (a)〜(d)・(f)・(g) が RED（機能不在）。
  - 外形 snapshot（`crates/scribe2-boundary/src/main.rs` の歯の区間・行 f の write-set の `+` の snapshot）: 測れた周の 1 行（全欄 1 件以上）と測れない周の 1 行。
  - rules（接頭辞 rules_open_children_・`crates/scribe2-boundary/tests/e2e/rules.rs`・0 件）: 埋め込みの manifest に行が 1 本在り、id / kind / 形 Int / 値 / 裁定 id と裁定日 / 位置が形 4 のとおり（base は行が無い＝RED）。既存の数の pin（`crates/scribe2-boundary/tests/e2e/rules/embedded.rs` の行数と kind の数・`rules_external_form` の snapshot の rows= と kinds=・main 8f6072d では 72 と 70）が base の数から 1 ずつ増え、`rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all` の LedgerDeniedWrites からの並びに LedgerOpenChildrenMax が 1 つ入る（どれも base の数と並びでは RED）。
  - 行 f の `+` の file の in-file の歯は flip-check が base へ写さないので、flip の証拠は e2e が持つ。
- 限界: 同時に open の子の最大（121 本）は今の親子で過去を数えた近似（付け替えの履歴は読まない）。top は親の最初の 1 本をたどる（2 つ目の親は (c) が別に名指す）。doctor は引く口で、撃たれなければ読まれない（新しい崩れを増やさないのは §12 の門が持つ）。
- 却下: `bd doctor --check=conventions` の orphan を使う（embedded mode で動かない）／`bd graph check` に parent-child を任せる（輪も親 2 つも見ない＝実測）／SessionStart の台帳の 1 行にグラフの件数を足す（ADR-0045 §2 (3) の注入を太らせ、消した marker と同じ面になる）／直す 1 行を id ごとに別の行で出す（最大の台帳で 700 行を越え、どの epic に付けるかは席の判断＝器は形だけを示す）／「長く動かない epic」を日数の rules 行で数える（上の ADR-0045 の節）／memo と方針の bead を数えから外す（§12 の却下）。

## 11. 親を運べない起票の口と parent-child の辺を横から張る書きを起票の門が断る — 台帳 write の形に 2 つ足す（契約表の行 g・§10 の続き・裁定 user 2026-09-27T14:02Z）

やさしく言うと: 今の門は「--parent の無い create」だけを止めるので、親を付けられない別の起票の口（q など）と、親子の辺を横から張る口（dep add の parent-child など）が素通りする。この 2 つを同じ門で、台帳を読まずに止める。

- 何が起きているか（bd 1.1.0・使い捨ての台帳・verified）: `bd q` は --parent を受けない（unknown flag）。`bd todo add` と `bd batch` の create 行も親を運ばず（help の字面）、`bd create-form` は対話の口。`dep add <子> <親> --type parent-child` と `link <子> <親> --type parent-child` は、既に親を持つ子に 2 つ目の親を付け、輪も作れる（§10 の決定はしご）。親の付け替えは `update <子> --parent <親>` が 1 本に置き換える（付け替えの後の parent-child は 1 本・実測）。scribe2・folio2 と非公開の隣の project 2 本の計 4 repo の tracked に bd / bdw の q・batch・todo・link を撃つ行は 0 件。
- 現物（main 8f6072d・verified）: 台帳 write の形は `crates/scribe2/src/hook/ledger_guard.rs` の `FORMS`（4 形）と `judge_write(`（subcommand と flag の名だけを読む）で、create-without-parent は「subcommand が create で --parent が無い」だけ。`write_of(` は flag の値と flag でない語を捨てる。断る形の列は rules 行 `ledger.denied_writes`（4 語・裁定 user 2026-09-22T08:44Z）。host の見張り（`crates/scribe2/src/hook/host_guard.rs`）は同じ `FORMS` と `judge_write(` を台帳を持つ repo で撃つ。
- 形（番号は done と 1:1）:
  1. **形を 2 つ足す**（`FORMS` は 6 形・判定の順は既存の 4 形の後ろに create-bypass → parent-edge）。
  2. **create-bypass**: subcommand が q / create-form / batch か、todo の次の語が add（親を運べない起票の口）。todo list と todo done は当たらない。
  3. **parent-edge**: dep の次の語が add か subcommand が link で、--type / -t の値（`=` の形も）が parent-child・dep add が --file を持つ（中身は読まない＝fail-closed）・create が --deps の値に parent-child: を持つ。dep add の blocks・link の既定（blocks）・dep remove は当たらない。
  4. **`Write` に flag でない語と flag の値を運ぶ field を足す**（構築点は `write_of(` の 1 か所・既存の 4 形の判定と字面は変えない）。
  5. **rules 行 `ledger.denied_writes` の値に 2 語を足し**、裁定 id を user 2026-09-27T14:02Z 項 5・裁定日を 2026-09-27 に替える（行数と kind の数は変わらない）。2 語はこの裁定（器に乗る全 project で、台帳のグラフの違反は減る向きにしか動かさない）の実装で、断る範囲はこの裁定に依る: create-bypass は親を持たない bead（根に着かない bead）を作る口、parent-edge は親 2 つと親の輪を作る口で、どちらも違反を増やす向きの書きだけを断り、host の見張りを通して台帳を持つ全 project で効く。rules-diff は値が変わった行に base の別の行の裁定 id と同じ字面を断るので、この字面を裁定 id に持つ rules 行は repo に 1 つになる（§10 形 4 の上限の行は別の字面）。
  6. **断り文は既存の形**（`reason=<語>（<説明と次の一手>・…）`）で、create-bypass は「bdw create <題> --parent <epic> で撃つ」を、parent-edge は「親は bdw update <子> --parent <親> で付け替える（1 本に置き換わる）」を次の一手に持つ。
- 触らない: 既存の 4 形の判定と字面・`WRITES`（bd-outside-bdw の語彙）・memo の判定・極性一覧（同じ guard の中の形が増えるだけ）・`--graph` の plan の中身（file と台帳を読む判定は §12）・host の見張り（`crates/scribe2/src/hook/host_guard.rs` は同じ `FORMS` と `judge_write(` を撃つので本行の手なしに 2 形を断り、その歯は `FORMS` を回して追随する＝形の数を pin しない）。
- 足す前に消すもの（C17.2）: 新しい guard も rules 行も足さない（既存の guard の形と既存の行の値に 2 語）。
- 歯（接頭辞 hook_ledger_edge_・`grep -rn "fn hook_ledger_edge_" crates/` は 0 件・2026-09-27）:
  - lib（`crates/scribe2/src/hook/ledger_guard.rs` の in-file）: (a) q / todo add / batch / create-form が create-bypass で、todo list と todo done は当たらない (b) dep add と link の parent-child（--type と -t と = の形）・dep add --file・create --deps parent-child:x が parent-edge で、dep add の blocks・link の既定・dep remove は当たらない (c) 既存の 4 形の当たりと当たらない例は変わらない（既存の hook_ledger_write_ の歯の母集団は `FORMS` の 6 形に合わせて直す）。
  - e2e（`crates/scribe2-boundary/tests/e2e/hook/guards.rs`）: 埋め込みの rules で `bdw q x` と `bdw dep add a b --type parent-child` が rc 2・stderr 1 行・記録 1 行（base は rc 0＝RED・機能不在）。
  - rules（`crates/scribe2-boundary/tests/e2e/rules.rs` の `rules_ledger_denied_writes_row_is_declared_on_four_faces`）: 値 6 語と新しい裁定 id を pin する（base は 4 語＝RED）。
- 限界: bd の subcommand が増えれば語彙に手が入る（`WRITES` と同じ道具の語彙の const）。`WRITES` は note / tag / link / epic / defer 等の書きを持たず、素の bd の link（blocks）は bd-outside-bdw をすり抜ける（本行の外）。dep add --file は blocks だけの file も断る。
- 却下: create-without-parent の判定を q 等へ広げる（形の語の意味が変わり、記録の語で断りの口を分けられない）／dep add --file の中身を読んで parent-child の行だけ断る（stdin の形を読めず、fail-closed の形が 2 つに割れる）／parent-child の辺の制約を bd の側へ提案して待つ（器の外の変更を待ち、全 project で効かない周が続く）。

## 12. 新しい崩れを増やす書きだけを起票の門が断る — 付け先が根に着かない・溢れた親・親子の輪・根から外す書き（契約表の行 h・§10 / §11 の続き・裁定 user 2026-09-27T14:02Z）

やさしく言うと: 書く前に台帳を 1 回読み、その書きで「根に着かない bead」「溢れた親」「親子の輪」が増えるときだけ止める。すでに崩れている所は、それを直す書きなら通す（減る向きにしか動かさない）。止めるときは直す 1 行を添える。

- 何が起きているか（実測・verified）: §10 の数え。§11 の後も、親の付け先の形（根に着くか・溢れているか・輪になるか）と親を外す書き（update --parent ""・parent-child の辺の dep remove・根の epic の型の変更）は command の字面だけでは判定できず、台帳の中身が要る。`create --graph <file> --parent <epic>` は create-without-parent を通るが、--parent は plan の node に効かない（使い捨ての台帳で node が親なしで作られた・実測）。読みの費用: `bd --readonly list --all --limit 0 --json` が 0.83〜1.13 秒（scribe2・7.8 MB）・1.03 秒（最大の台帳 933 本）で、`list --id` / `list --parent` の狭い読みも 0.65〜0.90 秒（起動が支配する）＝読み 1 回で全部を引くのが最も安い。hook の予算は rules 行 `hook.budget_ms`（2000・NFR5）、plugin の hook の timeout は 10 秒（`plugin/hooks/hooks.json`）。
- 現物（main 8f6072d・verified）: PreToolUse の Bash は `crates/scribe2/src/hook/mod.rs` の `pre_tool_use(` が command guard → 起票の門（`crates/scribe2/src/hook/ledger_guard.rs` の `decide(`）の順に撃ち、台帳 client の差し替えは `Hooked` の `bd`（SessionStart の指示文が同じ client を読む）。create の flag の読みは `ledger_guard.rs` の `create_of(` と `Create`（title・label・body-file・acceptance だけ・値を取る flag の列 VALUED は --parent / --type / --graph を既に持つ）。台帳の読みは `read_ledger(`（待ち上限は呼び手が渡す）。
- 形（番号は done と 1:1）:
  1. **判定の本体は兄弟 module**（行 h の write-set の `+` の file・`crates/scribe2/src/hook/mod.rs` は宣言と `pre_tool_use(` の呼び出しの数行）: 起票の門（§11 までの形）で止まらなかった Bash の command だけを掛け、結果は起票の門の判定の enum（`LedgerDecision`）で返す（同じ guard・極性一覧は増えない・記録は既存の ledger-deny <語>）。
  2. **掛かる書きは閉じた 6 つ**（bd / bdw の segment）: (i) create の --parent P (ii) create の --graph F（file の node） (iii) update X… の --parent P（空も） (iv) update X… の --type T（-t も） (v) dep remove / dep rm A B (vi) 数えに戻す書き: reopen X… と、update X… の --status S（-s も・S が closed でも pinned でもない）か --claim。どれも無い command は台帳を読まない（NFR5）。hook の root（`Hooked` の `root`＝--project の anchor・無ければ payload の cwd）が `.beads` の dir を持たない周は掛けない（台帳の無い repo・host の見張りの台帳の印と同じ向き）。
  3. **読みは 1 回**: 掛かる segment が 1 つ以上在る周だけ `read_ledger(` を 1 回撃つ（client は `Hooked` の bd か既定・cwd は payload の cwd・待ち上限は `hook.budget_ms`）。読めない周と待ち上限を越えた周は断る（fail-closed・理由 ledger-unreadable / ledger-timeout）。判定は読んだ台帳の写し 1 つに segment の順に当て、通った segment の効き（create が足す子の数・親の付け替え・親の外し・型の変更・数えに戻す状態）を写しに足してから次の segment を判定し、最初の断りを返す（1 行に並べた書きも、前の書きの後の形で測る）。
  4. **断る条件（増える向きだけ・判定は §10 形 1 の純関数を使う）**: (a) parent-unrooted: 付け先 P が根に着かない（台帳に無い P も・create の空の P も・create・graph の parent_id・update で X が今は根に着く周・update の空の P は (d) だけが判じる） (b) parent-full: P の直下の open の子（closed でも pinned でもない子・§10 形 2 (e) と同じ数え）に epic でない open の bead を足すと N を越える（create は 1 本・graph は同じ P への epic でない node の本数・update は X が epic でも closed でもなく今の親が P でない周・数えに戻す書きは closed か pinned の epic でない X の今の親を P とする周・N が 0 の周は掛けない） (c) parent-loop: update の P が X 自身か X の子孫 (d) unrooting: update --parent が空か、dep remove の B が A の唯一の親で、A / X が epic でない周・update --type の T が epic でなく X が根の epic の周 (e) plan-orphan: graph の node が parent_key を plan の中でたどって、parent_id を持つ node（その P を (a)(b) に掛ける）にも親を持たない epic の node にも着かない（親を持たない非 epic の node・知らない key・parent_key の輪） (f) plan-unreadable: graph の file が無い・読めない・JSON でない。rules 行（ledger.open_children_max・`hook.budget_ms`）が無い・形が違う周は no-rule。
  5. **数えないもの・断らないもの**: closed の子は数えない（子が全部 closed の epic は溢れでなく close-eligible＝§10）。epic の create と epic の付け替えは溢れで断らない（子 epic を作ることが溢れの直し方で、断ると直せない）。memo と方針の bead も同じ数えに入り、器は例外の列を持たない: 門は**新しい子だけ**を断り、既に根の直下に居る決まりの bead は動かさないので、根の直下を探す読み手は変わらない。閉じない常設の bead（裁定の控え・方針）を数えから外すときは bd の状態 pinned を使う（bd の組み込みの「常設・閉じない」・台帳に見える）。すでに崩れた所を直す書き（根に着かない X を根に着く P へ・溢れた親から子を出す・型を epic にする）は通す。
  6. **断り文は直す 1 行を持つ**（`reason=<語>（<説明と次の一手>・ledger-form.md §12）`）: parent-unrooted は top の id と「bdw update <top> --type epic か --parent <epic>」、parent-full は P と子の数と N と「bdw create <題> --type epic --parent P で子 epic を作り、その下へ置く（既存の子は bdw update <子> --parent <子 epic>）」、parent-loop は P と X、unrooting は「bdw update X --parent <epic> で付け替える」、plan-orphan は node の key と「node に parent_id を書く」。
  7. **flag の読みを足す**: `Create` に --parent・--type（-t）・--graph の値を運ぶ field（構築は `flags_of(` の 1 か所）。update の id は flag でない語で、値を取らない flag の閉じた列（bd 1.1.0 の update の 8 語と大域の旗）を const に持ち、それ以外の flag は次の語を値に取る（`-` で始まる語は値にしない）。rules 行 2 本は id で引いて `Int` だけを読む（`timeout_of(` と同じ読み・行 h の `+` の file で `RuleKind` の variant を名指さない）。
- 全 project への効きと ratchet（裁定 user 2026-09-27T14:02Z: 器に乗る全 project に効かせ、違反は減る向きにしか動かさない）と移し替え: 門は plugin の PreToolUse で、consumer の席は host の面の plugin 行から同じ hook を積む＝器に乗る全 project の席で効く。上限の値は埋め込みの manifest の 1 つ（project ごとの値は持たない）。既存の崩れは断らないので、止まるのは崩れた所へ足す書きだけで、その断り文が直す 1 行を名指す。移し替え（epic を起こし既存の bead を付け替える）は各 project の席が自分の台帳で行い、器は §10 の doctor の行（top と溢れの id）と断り文の 1 行を出す。
- 足す前に消すもの（C17.2）: 崩れが起きた後に席が doctor の行を見て撃つ直しの手番（孤児の付け替え・溢れの分割）の新しい発生を、書きの時点の断りに置き換える（新しい guard は足さず、起票の門の中に判定を 1 つ足す）。
- 触らない: §11 までの 6 形と memo の判定と字面・判定の順（memo → 形 → 本 §）・`read_ledger(` と `Issue`・台帳の書きの口・host の見張り（NFR5 の git 1 回の予算に台帳の読みを足さない）・SessionStart の読み。
- 歯（接頭辞 hook_graph_guard_・行 f の verify の filter `ledger_graph_` を名に含まない＝行 f の write-set の外の `hook/guards.rs` に置いても行 f の歯に数えられない・`grep -rn "fn hook_graph_guard_" crates/` は 0 件・2026-09-28）:
  - e2e（`crates/scribe2-boundary/tests/e2e/hook/guards.rs`・新しい e2e の file は作らない）: toy repo に `.beads` の dir を置き、`--bd` に偽の client（fixture の JSON を返し argv を記録）を渡して `pre-tool-use` を撃つ。(a) 根の epic E（open の子 N 本）への create は parent-full で rc 2・stderr 1 行が「--type epic --parent E」を持ち、同じ E への --type epic の create は通る (b) feature の top の下の親への create は parent-unrooted で top の id を名指す (c) update X --parent <X の子> は parent-loop (d) update X --parent "" と dep remove X E（E が X の唯一の親）は unrooting、X が epic なら通る (e) --graph の file の node に親が無ければ plan-orphan・parent_id が溢れた E なら parent-full (f) 根に着かない X を根に着く親へ付け替える update は通る（減る向き） (g) 偽の client が rc 1 なら ledger-unreadable で断る (h) 掛からない command（close・append-notes・show）は偽の client を 1 回も起こさない (i) `.beads` の無い repo では同じ create を台帳を読まずに通す（(h)(i) は (g) と同じ rc 1 の偽の client で撃つ＝読めば断られる形で、読まないことを測る） (j) 溢れた E の closed の子の reopen と update --status open は parent-full で、update --status closed は通る (k) 1 行に並べた `update A --parent B && update B --parent A`（A と B は E の子）は 2 つ目の segment が parent-loop。base では (a)〜(e)・(g)・(j)・(k) が rc 0 で通る＝RED（機能不在）。既存の起票の門の e2e（`.beads` の無い toy repo）は 1 字も変えずに緑。
  - lib（行 h の `+` の file の in-file）: 6 つの書きの読み（update の値を取らない flag・--parent= の空・dep rm・reopen の複数の id・--status と -s と --claim）と、6 つの条件の当たりと当たらない例（効きを写しに足す segment の順を含む）。
- 限界: 読みは hook の予算の内側だが、host の負荷で 2 秒を越えた周は ledger-timeout で断る（席は撃ち直す）。create が作る bead の id は台帳の採番で写しに無いので、同じ行の後の segment がその id を名指すと (a) で断る（撃ち分ければ通る）。delete（A1 で user に聞く書き）は掛けない（溢れと孤児は §10 の doctor が後から名指す）。host の見張りだけの session（席でない session）は本 § の門を持たない（§11 の形は持つ）。plugin の hook が 10 秒で切れた周の扱いは harness の側（未実測）。pinned にした子は数えから外れる（外したことは `bd list --status pinned` に見える）。
- 却下: 読みを狭める（--id と --parent の 2 回で親と子を引く＝起動が 2 回で予算を越える・実測）／門を dispatcher の周や管理 tick の合図で撃つ（書いた後に知らせるだけで止められない）／違反の総数を基準値と比べて増えた書きを断る（基準値の置き場と毎回の全件の数え直しが要る＝書き 1 つの差分で足りる）／memo と方針の bead を数えから外す（scribe2 の根の直下の open は全部 memo で流入の主が素通りし、例外の列の置き場が要る）／closed の子も数える（子が全部 closed の epic が永久に溢れ、直し方が close でなく分割になる）／根に着かない親への create を通して doctor だけで数える（folio2 の形が増え続ける）／1 行に書きを 1 つだけ許す（移し替えの `&&` の連ねまで断る・効きを写しに足せば同じ穴を塞げる）。

## 13. 台帳の問い（label intake:question）を 4 象限の母集団から外す（契約表の行 i・FR51・ADR-0083）

やさしく言うと: 台帳の問いは、席から user への問いを残す記録で、契約でも memo でもない。いまの台帳の形の行は、型が epic と decision の bead だけを 4 象限（memo か設計 pointer か）の数えから外しているので、開いている問いが「どちらの印も無い」（neither）に数えられ、答えが出るまで doctor の行の違反に名指され続ける。問いも epic と裁定の bead と同じく数えから外す。

- 何が起きているか（verified・main 4d3ba2ca）: 台帳の形の判定 `judge`（`crates/scribe2/src/ledger/form.rs`）は、4 象限の母集団を open の bead から型が epic か decision のもの（型の除外の定数）を除いて作り、neither はその母集団のうち memo の label も設計 pointer も持たない bead である。台帳の問いは型 task で起票される（器を導入した repo の問いも同じ）ので母集団に入り、開いている間 neither に名指される。台帳の形の行を写す消費側の面も、その間ずっと問いを違反として出す。
- 出所: SRS v0.28 の FR51（「label intake:question の bead（台帳の問い・FR81）は、epic と decision の型の bead と同じく契約にも memo にも数えず名指さない」）と ADR-0083（器が問いとして読む記録は label intake:question の bead だけで、型は問わない）。消費側の席の指摘（2026-09-28・doctor の行が開いた問いを neither に数える）で、要件の文と判定の食い違いが分かった。
- 形: 4 象限の母集団から、型が epic か decision の bead に加えて label intake:question を持つ bead を外す。label で外し型では外さない（問いは型 task で起票されるため・ADR-0083）。label の字は 1 つの定数に置き、起票の門（FR81）の実装は同じ定数を引く。doctor の台帳の形の行の key と順は変えない: open= は closed でない bead の全部の件数のまま、shaped= と both= と neither= だけが母集団から外れた問いの分だけ変わる。台帳の lint（`crates/scribe2/src/ledger/lint.rs`）は、契約を設計 pointer を持つ bead、memo を label intake:memo を持つ bead で数えるので、問いは既にどちらにも入らない（変えない）。
- 歯: `quadrant_exempts_question_`（form.rs の既存の歯の区間に 1 本）が、open の問い 1（label intake:question・型 task・設計 pointer なし）と open の memo 1 と設計 pointer を持つ open の契約 1 と印の無い open の task 1 の台帳で、open が 4、shaped が 3、neither が印の無い task の id だけを名指し、問いの id がどの欄にも出ないことを測る。base の judge は問いを neither に数えるので RED（機能不在）。
- 限界: label intake:question を持つ bead は、memo の label か設計 pointer を併せて持っても数えから外す（問いの label を先に見る）。そうした混ざった形を起票の時点で断るかは起票の門（FR81）の設計が決め、本行は数えだけを変える。

## 14. 台帳の問い（label intake:question）の create の形を起票の門が断る — 本文の 4 行・metadata の effect と asked・label intake:memo の併せ持ち・親の label を継がない指定（契約表の行 j・FR81 (a)・FR89・ADR-0083・ADR-0087）

やさしく言うと: 席が user に聞くときは台帳に「問い」の bead を立てる。問いは 概要・技術・理由・推奨 の 4 行と、答えを文書へ写すか（effect）・誰のきっかけの問いか（asked）の印を持つ。形の欠けた問いを起票の時点で止め、memo の印を併せ持つ問いと、親の memo の印を黙って継いでしまう起票も止める。門は台帳を読まない。

- 何が起きているか（main 7c4ab0a1・verified）:
  - 起票の門 `crates/scribe2/src/hook/ledger_guard.rs` の `decide` は memo の判定（`judge`）→ 台帳 write の 6 形（`judge_write`）の順で、label intake:question を見ない。create の読み `create_of` / `Create` は title・label・body-file・acceptance・parent・type・graph だけを運び、`-d` / `--description` と `--metadata` は値を取る flag の列 `VALUED` で値を捨て、`--stdin` と `--no-inherit-labels` は読まない。
  - 問いの label の字は `crates/scribe2/src/ledger/form.rs` の `QUESTION_LABEL`（§13）。
  - bd 1.1.0（`bd help create`）: `--metadata` は JSON の字か `@file.json`・`--stdin` は `--body-file -` の別名・`--no-inherit-labels` は親の label を継がない・`-d` / `--description` は本文。親の label は既定で継がれる（`.beads/PRIME.md` の R2）ので、memo の子に立てた問いは印を付けないと intake:memo を併せ持つ。
  - 台帳の問い 3 本（2026-09-29・全部 closed）: 本文は 3 本とも `概要 = …` / `技術 = …` / `理由 = …` / `推奨 = …` の 4 行。metadata は 1 本が effect と asked を持ち、2 本は持たない。
- 形（番号は done と 1:1）:
  1. **読み手は行 j の write-set の + の file の純関数**（本文の字・metadata の字・label の列だけを読み、台帳も file も読まない）: (i) 4 行 — 行頭の空白と任意の `- ` を除いた行が 概要・技術・理由・推奨 の語で始まり、語の直後が行末・空白・`=`・`:`・`：` のどれか（`技術的…` は技術の行でない）。順は問わず、宣言順で最初の欠けを返す (ii) metadata — JSON の object で、key effect の値が文字列 document か operation、key asked の値が文字列 seat か user（ほかの key は読まない）。JSON の object でない字は読めない (iii) label intake:memo の併せ持ち。JSON は `crates/scribe2/src/fleet/json_tree.rs` の `parse` で読む（依存を足さない）。
  2. **門の段**: label intake:question を持つ create の segment は memo の判定の代わりに問いの段に掛かる（title が `[memo]` で始まっても memo の判定を掛けない＝問いの label が先）。順は 併せ持ち → 継がない指定（`--parent` の値が空でなく、`--no-inherit-labels` も `--no-inherit-labels=true` も無い）→ 本文を読めるか → 4 行 → metadata を読めるか → effect → asked で、最初の欠けで止める。6 形はこの段の後（今の順のまま）。
  3. **読む字**: 本文は `--body-file` の file（payload の cwd から解く・memo の判定と同じ）の字と、`-d` / `--description` の最後の値を行の列として合わせたもの。どちらも無い周は空の本文（4 行の最初の欠け）。`--stdin`・値が `-` か値の無い `--body-file`・開けない file・`$` か backtick を含む `-d` の値は読めない。metadata は `--metadata` の最後の値で、`@<path>` は同じ cwd から読み、無い周は effect と asked の欠け。
  4. **断りの語**（記録は `ledger-deny <語>`・閉じた 12 語）: question-memo-label・question-inherits-labels・question-body-unreadable・question-no-summary・question-no-technical・question-no-reason・question-no-recommendation・question-metadata-unreadable・question-no-effect・question-bad-effect・question-no-asked・question-bad-asked。断り文は既存の形 `deny bd create は起票の門が止める reason=<語>（<説明と次の一手>・ledger-form.md §14）` の 1 行で、欠けた行の語・外れた値の字・併せ持つ label・継ぐ親の id（台帳を読まないので親の label の字でなく「親 <id> の label を継ぐ」）を名指し、次の一手（`概要 = …` の 4 行・`--metadata '{"effect":"document","asked":"seat"}'`・`--no-inherit-labels`）を持つ。
  5. **`Create` に field を足す**: `-d` / `--description` の値・`--metadata` の値・`--stdin` と `--no-inherit-labels` の有無。構築は `flags_of` の 1 か所のまま。
  6. label intake:question を持たない create は問いの段に掛からない（memo の判定と 6 形は今のまま）。席が起こした子 process の書きは道具の呼び出しでないので門の外（hook は道具の呼び出しの command の字だけを読む）。
- 触らない: memo の判定と字面（`judge` と memo の 4 理由）・6 形（`FORMS`）と rules 行 `ledger.denied_writes`・`Refusal`（変種を足さない）・極性一覧（同じ guard の中の段）・`crates/scribe2/src/hook/mod.rs`・台帳の読み（NFR5）。閉包: 行 j の + の file は `Create` と `Write` と `Refusal` を名指さず、`Create` の字面の構築（中括弧の literal）をどの file にも足さない（行 g・h の閉包を広げない・§ の歯の fixture も `create_of` で組む）。
- 却下: `--metadata @file` を断る（決めたこと 4）／`--stdin` の本文を heredoc から読む（決めたこと 6）／4 行を `<語> =` の形に限る（要件の「で始まる行」より狭い）／rules 行の語に載せて効かせる（決めたこと 15）／`create --graph` と `create -f` の file の node を読んで問いの形を判じる（今の memo の判定も読まない・ledger-plan の出力は問いの label を持たない・読み方がもう 1 つ増える＝限界に残す）。
- 限界: 問いの metadata と label を後から書き換える update（`--set-metadata`・`--add-label intake:memo` 等）は (a) の外（FR81 (a) は create の形）。`create --graph` / `create -f` で起こした問いは判じない。`-d` の値の `$` は単引用符の中の字でも読めない側に倒れる（字の出どころを分けない）。
- 歯（接頭辞 hook_question_form_ と ledger_question_form_・`grep -rn` はどちらも 0 件・2026-09-29）:
  - e2e（`crates/scribe2-boundary/tests/e2e/hook/guards.rs`・新しい e2e の file は作らない）: `.beads` の無い toy repo で `hook pre-tool-use` を撃つ。rules は埋め込みの manifest の写しで `ledger.denied_writes` の値から bd-outside-bdw を外した file を `--rules` で渡す。(a) 10 形（4 行の欠け 4・effect の欠け・effect の値の外・asked の欠け・asked の値の外・intake:memo の併せ持ち・継がない指定の欠け）を、狙った欄のほかを揃え `--parent s2-1 --no-inherit-labels` を持つ形（継がない指定の欠けの形だけは持たない）で、`bd create` と `bdw create` の 2 経路で撃ち、20 本がどれも rc 2・stderr 1 行（形ごとの語と中身）・stdout 0 byte・記録 1 行 (b) 揃った問いの create（本文が body-file・`-d`・metadata が `@meta.json` の 3 形）が rc 0 で記録を残さない (c) label intake:question を持たない create（4 行を欠く本文）は問いの語で断られない (d) 4 行を欠く問いの create を中で撃つ toy repo の script を起こす command（bd / bdw の segment を持たない）は rc 0 (e) `--stdin` と `--body-file -` の問いは question-body-unreadable。(c)(d) は base でも緑なので (a) と同じ歯の中に置く（単独の歯にしない）。
  - lib（行 j の + の file の in-file・ledger_question_form_）: 語の直後の字（`=`・`:`・`：`・空白・行末は当たり・`技術的` は当たらない）・`- ` の行頭・object でない JSON・文字列でない effect・宣言順の最初の欠け。
  - lib（`crates/scribe2/src/hook/ledger_guard.rs` の in-file・hook_question_form_）: `create_of` が `-d` と `--description=`・`--metadata`・`--stdin`・`--no-inherit-labels` と `=false` を読む・段の順（併せ持ちが 4 行より先）。
  - 通った問いが台帳の lint と起動の列に出ないことは着地済みの歯（`crates/scribe2/src/ledger/form.rs` の `quadrant_exempts_question_from_the_shaped_population` と `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` の `pipe_dispatch_intake_label_question_is_not_a_candidate`）が持つ（本行は足さない）。
- base で RED の理由: base の門は label intake:question を見ないので、(a) の 20 本と (e) が rc 0 で通る（機能不在）。行 j の + の file の in-file の歯は flip-check が base へ写さないので、flip の証拠は e2e が持つ。
- 着地の後: PATH の binary を入れ替える（門は PATH の binary が撃つ・入れ替えは走行中の運転手が無い周に入れ替えの script で）。binary は同じ host の消費側の席と共有なので、入れ替えの前に消費側へ知らせる。

## 15. memo の引き金の行の文法と読み手 1 本 — 読める引き金の無い memo の create と、昇格条件を読める引き金の無い本文へ書き換える update を起票の門が断る（契約表の行 k・FR81 (c)・FR87・ADR-0085）

やさしく言うと: memo の「昇格条件」に、器が数えて判じられる 1 行（引き金）を書く決まりにする。行の書き方をここに全部決め、読むのは 1 つの関数だけにする。起票の門・便を起こす周・一覧・局面の関数がみな同じ関数で読むので、門が通した行は周でも読める。引き金の無い memo は書く時点で止める。

- 何が起きているか（main 7c4ab0a1・verified）:
  - memo の判定（`crates/scribe2/src/hook/ledger_guard.rs` の `judge`）は `--body-file` の本文に 4 節の見出し（`crates/scribe2/src/ledger/form.rs` の `MEMO_SECTIONS`）が在るかだけを見る。`### 昇格条件` の中身は読まない。update は memo の判定に掛からない。引き金の行の読み手は repo に無い（「引き金」の hit は build の注釈だけ）。
  - 開いた memo 27 本（2026-09-29）のうち、昇格条件に「引き金」の行を持つのは 9 本・9 行。下の文法で読めるのは 1 行（`- 引き金: 期日 2026-09-30T12:00Z`）だけで、`再発 3（…` と `再発 1（…` は値の語に括弧が続くので読めず、残り 6 行は散文。
  - 台帳の接頭辞を解く読み手は `crates/scribe2/src/seat/brief/pointer.rs` の `Anchor`（`.beads/config.yaml` の issue-prefix、無ければ `.beads/metadata.json` の dolt_database）。設計 pointer の読み手は `crates/scribe2/src/pipe/table/parse.rs` の `parse_pointer`（`.md` と `.toml` の 2 形）。UTC の字の読み手は `crates/scribe2/src/fleet/wait.rs` の `epoch_of`（秒までの形）。
- 文法（本 § が字面の正本・ADR-0085 が本 doc の改稿に委ねた）:
  - **置き場**: description の `### 昇格条件` の見出しの行（trim が完全一致）から、次の見出しの行（行頭の空白を除いて `#` が 1〜3 個と空白で始まる行）の前まで。見出しが複数在れば全部の節を読む。節の外の行と notes は引き金として読まない。
  - **行の字面**: 行頭の空白を除き、任意の `- ` の後に `引き金:`（ASCII の colon）で始まる行が引き金の行。残りを空白（Unicode の空白）で語に割り、1 語目が形・2 語目が値・3 語目より後は読まない。語が 2 つ無い・形が 5 つの外・値が形の値の形に合わない行は**読めない引き金**（行の字と理由を返す）。`引き金：`（全角）や `* ` の行頭は引き金の行でない。
  - **5 形**（満ちる条件の判定は dispatch の周の後の行が持つ・定義はここ）:

    | 形 | 読める値の形 | 満ちる条件 |
    |---|---|---|
    | 再発 | ASCII の数字だけの 1 以上の整数 | notes の再発の行（下）の本数が値以上 |
    | 同梱 | repo 相対の path（頭が `/`・`+`・`-`・`=`・`~` でない・`#` を持たない・`..` の段を持たない）。末尾 `/` は dir | 開いた契約（設計 pointer を持ち closed でない bead）が指す契約表の行の write-set の項目から頭の印と `#<名>` の尾を外した字が値と完全一致（値が `/` で終われば前方一致） |
    | 依存 | 同じ台帳の bead id（台帳の接頭辞・`-`・ASCII の英数字の段を `.` で繋いだ字） | その bead が closed（台帳に無い id は満ちず、無いと名指す） |
    | 期日 | `YYYY-MM-DDTHH:MMZ`（UTC・分まで・例 `2026-09-30T12:00Z`） | 周の時刻が値の時刻以後 |
    | 着地 | 便の --design が受ける設計 pointer の字（`parse_pointer` が通す `<path>.md#<行 id>` か `<path>.toml#<行 id>`） | acceptance の設計 pointer の字が値と完全一致する bead が 1 本以上 closed |

  - **notes の行頭**: 行頭の空白を除いて `[再発]` で始まる行が再発の行、`[keep]` で始まる行が keep の記帳（ADR-0085）。読むのは notes だけ。
  - 値に括弧や句点を続けない（`再発 3（…）` は読めない。`再発 3 （…）` と空白で区切る）。
- 形（番号は done と 1:1）:
  1. **読み手は行 k の write-set の + の file の純関数 1 本**: 入力は description の字・notes の字・台帳の接頭辞（無い周も渡せる）だけで、I/O も時計も持たない。上の文法で、節が在るか・行ごとの読める引き金（形と値）か読めない引き金（行の字と理由）・再発の行の本数・keep の記帳の行を返す。期日は `epoch_of` に秒 `:00` を足した字で UNIX 秒へ、着地は `parse_pointer` で読む。起票の門（本行）・dispatch の周の引き金の判定と `pipe dispatch ls` の memo の行（dispatcher の後の行）・局面の関数（案件の局面の設計 doc の後の行）が同じ読み手を引き、自前の読みを持たない（C2）。
  2. **bead id の形の判定は `crates/scribe2/src/ledger/form.rs` の 1 関数**（接頭辞を引数に取る・後の close の理由の行も同じ関数を引く）。門は payload の cwd から上へ辿った最初の `.beads` の dir を持つ dir で `Anchor` を開いて接頭辞を読む（bd が台帳を探す向きと同じ・台帳の client は起こさない）。解けない周は依存の値が読めない。
  3. **memo の create**: 4 節が揃い memo の判定で止まらない create は、昇格条件の節に読める引き金の行が 1 本も無ければ no-trigger で断る。断り文は 5 形の字面（`引き金: 再発 <n>` / `同梱 <path>` / `依存 <id>` / `期日 YYYY-MM-DDTHH:MMZ` / `着地 <pointer>`）と、在れば最初の読めない行の字（先頭 40 字）を名指す。
  4. **update**: bd / bdw の update が `--body-file` か `-d` / `--description` で書く本文が `### 昇格条件` の見出しを持ち、その節に読める引き金の行が無ければ update-no-trigger で断る。見出しを持たない本文は通す（門は台帳を読まないので、書く先が memo かを知らない）。`--stdin`・値が `-` か値の無い `--body-file`・開けない file・`$` か backtick を含む `-d` の値は update-body-unreadable（本文を書く update は bead の種類に依らず掛かる・fail-closed）。
  5. **断りの語**（記録は `ledger-deny <語>`）: no-trigger・update-no-trigger・update-body-unreadable。断り文は既存の形（`deny bd create`・`deny bd update` の頭・`ledger-form.md §15`）。段の位置: no-trigger は memo の判定の直後、update の 2 語は 6 形の後。
- 触らない: memo の判定の字面と 4 理由・`MEMO_SECTIONS`・6 形・`Refusal`（変種を足さない）・`crates/scribe2/src/seat/brief/pointer.rs`（`Anchor` を呼ぶだけ）・memo の plan の口（`crates/scribe2/src/ledger/memo.rs`・昇格条件は空の見出しのまま出す＝席が埋める・§3 の 8）・既存の open な memo（書き換えない限り門は掛からない）・台帳の読み（NFR5）。
- 却下: 全角の括弧で値を切る（決めたこと 7）／同梱の値を write-set の印つきの字で書かせる（行の宣言の字と照合の字が 2 通りになる）／依存に別の台帳の id を許す（満ちるかを判じられず、読める引き金を持つのに永久に満ちない memo が黙って残る＝読めない側に倒して misfit に出す）／update の門で台帳を読んで書く先が memo かを確かめる（NFR5・台帳の読みは 1 秒前後で負荷の下では予算を越える・memo s2-07l.738.3）／期日に日付だけ・秒つき・時差つきの形を許す（読み手が形ごとに分かれる）。
- 限界: update の本文が昇格条件の見出しごと消す書き換えは判じない（書く先が memo かを知らない・4 節の欠けは台帳の lint の (iv) が名指す）。`create --graph` / `create -f` の memo は判じない（今の memo の判定と同じ）。`epoch_of` は月の日数を確かめない（`2026-02-31` は読める）。今の open な memo のうち読める引き金を持たない 26 本は局面の出力で misfit に出る（ADR-0089 CSQ-N4・書き足しは局面の行の前）。
- 既存の歯の直し（同じ便）: `crates/scribe2-boundary/tests/e2e/hook.rs` の `MEMO_BODY`（memo の create の e2e の本文）の昇格条件に `- 引き金: 再発 1` を 1 行足す（通る歯が緑のまま・歯の外の行）。
- 歯（接頭辞 hook_memo_trigger_ と ledger_trigger_・`grep -rn` はどちらも 0 件・2026-09-29）:
  - e2e（`crates/scribe2-boundary/tests/e2e/hook/guards.rs`）: rules は §14 と同じ写し。(a) 4 節が揃い昇格条件が散文だけ（`- 引き金: 再発 3（…）` を含む）の memo の create と、昇格条件の節を引き金の無い本文へ書き換える update（`--body-file`）の 2 形を bd と bdw の 2 経路で撃ち、4 本がどれも rc 2・stderr 1 行（no-trigger / update-no-trigger と 5 形の字面）・stdout 0 byte・記録 1 行 (b) `- 引き金: 再発 1` を持つ memo の create と見出しを持たない本文の update が rc 0 (c) `bdw update s2-1 --stdin` は update-body-unreadable (d) `.beads/config.yaml` に接頭辞を持つ toy repo（graph の門の歯と同じ偽の client と根の epic E・`--parent E`）で、その接頭辞の依存の行を持つ memo は通り、別の接頭辞の依存の行だけの memo は no-trigger。
  - lib（行 k の + の file の in-file・ledger_trigger_）: 5 形 × 読める値と読めない値・節の境（`####` は節の中・`##` で終わる）・`- ` の行頭・全角の colon・3 語目の無視・notes の `[再発]` と `[keep]` の数え・接頭辞の無い周の依存。
  - lib（`crates/scribe2/src/hook/ledger_guard.rs` の in-file・hook_memo_trigger_）: update の本文の出どころ（`--body-file`・`--body-file=`・`-d`・`--stdin`・値の無い `--body-file`）と、cwd から上へ辿る `.beads` の解き方。
- base で RED の理由: base の門は昇格条件の中身も update も見ないので、(a) の 4 本・(c)・(d) の後半が rc 0 で通る（機能不在）。
- 着地の後: §14 と同じく PATH の binary を入れ替え、入れ替えの前に消費側へ文法を知らせる。

## 16. close の理由の文法と読み手 1 本 — close-check を宣言した repo で、理由の無い close・和の外の理由・着地の形の理由を持つ席の close と、理由を持てない close の口を起票の門が断る（契約表の行 l・l1・l2・l3・FR81 (d)・FR91・ADR-0089・ADR-0094・ADR-0097）

やさしく言うと: bead を閉じるときの理由（close の理由）を、種類ごとに決まった 9 つの書き出し（頭）のどれかに限る。「着地した」を言う書き出し landed は、器が測った事実からだけ書く（便の着地の終わりと、PR の便の後始末の口）ので、席は書けない。理由の無い閉じ・形に合わない理由・席の landed・理由を渡せない閉じ方（状態を closed に書き換える・bd が自分の字で閉じる口）を、書く前に止める。**止めるのは、repo が自分の宣言（`.vessel.toml`）に「加わる」と書いた repo と、宣言が在って読めない repo だけ**で、宣言を読めて close-check を書いていない repo の席の閉じ方は 1 字も変わらない。門は台帳を読まず、command の字と理由の file と repo の宣言だけを見る。読み方はここに全部決め、読むのは 1 つの関数だけにする（後の局面の関数も同じ関数で読む）。

- 何が起きているか（main 1d2f5e6e・2026-09-30・verified）:
  - 起票の門 `crates/scribe2/src/hook/ledger_guard.rs` の `decide` は、問いの段・memo の判定・引き金の段 → 台帳 write の 6 形（`judge_write`・rules 行 `ledger.denied_writes`）→ update の引き金の段の順で、close は 6 形の bd-outside-bdw（bd の直の書き）でしか見ない。bdw の `close` と別名 `done`・`gate resolve`・`update --status closed`・`duplicate`・`supersede`・`epic close-eligible` は理由の字を問わず通る。
  - 着地の形の書き手は `crates/scribe2/src/pipe/land/finish.rs` の `close_reason` の 1 本（頭は `CLOSE_REASON`・尾は `CloseTail` の閉じた 2 値）で、3 つの経路（ADR-0094）が引く: (1) remote を宣言する repo の便は land の終端が push と CI の照合の後に `landed <着地 commit id> ci=success`（push の先端でない便は後ろに ` tip=<先端の commit id>`）、(2) remote を宣言しない repo の便は land の終端が照合なしで `landed <着地 commit id> ci=none`（行 bn）、(3) PR で着地した便は `crates/scribe2/src/pipe/retire.rs` の `close_and_fold` が `landed <merge の commit id> ci=success`（merge の commit が remote の main の先端でない周は ` tip=<先端の commit id>`・行 bp）。書きは台帳の子 process で、道具の呼び出しでないので門の外。`already_closed` は理由の 1 語目が landed の閉じを閉じ済みと読む。
  - 止まった終端の撃ち直し `pipe land --run <run> --terminal-only`（`crates/scribe2/src/pipe/cli/step.rs` の `terminal_only`）と `pipe retire` は、`crates/scribe2/src/hook/role_guard.rs` の `CAPABILITY_COMMANDS` で merge と launch の権能の口で、席の rules 行 `role.orchestrator` に無い。名指しの 2 形を席の決着の権能 settle に結ぶのは [seat-roles.md](./seat-roles.md) §32（行 z・ADR-0097）。
  - vessel 宣言の任意 key は `crates/scribe2/src/pipe/declaration/optional_keys.rs` の群（`DECLARED_KEYS`・`OPTIONAL_KEYS`）に在り、HEAD の宣言の読み手は `crates/scribe2/src/pipe/declaration.rs` の `head_declaration` の 1 本（作業ツリーは読まない）。読みの閉じた 3 値の前例は `QuestionRoute`（宣言した値・無い・読めない・[vessel-hook.md](./vessel-hook.md) §20）。値の生の型 `Raw` は文字列・整数・列の 3 つで、TOML の真偽は `value_of` が「value を読めない」にする（manifest の読み `scalar` は真偽を返すが宣言の読みは捨てる）。`Raw` を網羅で match するのは `crates/scribe2/src/pipe/declaration/entrance_flip.rs` の `entrance_of` の 1 か所。未知の key は宣言ごとの不備で、不備の宣言の repo では受付が便を起こさず、権能 guard は全 file を code に倒し、land の終端は unreadable で止まる。
  - bd 1.1.0（`bd help close`・`bd help update`・`bd help duplicate`・`bd help supersede`・`bd help epic`・`bd help gate`・`bd statuses`）: `close` の別名は `done`・id の無い close は最後に触った bead を閉じる・理由は `-r` / `--reason` と `--reason-file`（`-` は標準入力）・複数の id と複数の `--reason` は位置で対応。`update --status closed`（`-s`）は理由を渡せない。`duplicate --of` と `supersede --with` と `epic close-eligible` は bd が自分の字の理由で閉じる（`--dry-run` は閉じない）。`gate resolve` は close と同じで `-r` を持つ。組み込みの状態で閉じを言うのは closed の 1 つ。全体の flag `-C` / `--db` は別の台帳を指せる。
  - 台帳（2026-09-30・`bd --readonly list --status closed`）: closed 821 本。2026-09-25 以後（UTC）の close 167 本は、器の終端が書いた着地の形 124・本 § の形に合う席の閉じ 5・本 § の門が断る形 38（契約 7・memo 26・形の無い bead 5）。手で閉じた契約 7 本（s2-07l.662・.682・.685・.687・.697・.702・.703）はどれも器の便が着地した後に終端が閉じなかった便で（main の CI の初回の flaky で ci:failure 5・forge の限度で運転手が止まった 1・先端でない便の CI が無く ci:unmeasurable 1）、今の口では終端の撃ち直しで閉じる形。
  - 器の plugin を積む消費側の repo には、器の便でなく自前の PR の merge で仕事を着地させ、memo と形の無い bead を自由な字で閉じる運用の repo が在り、門を全部の repo に掛けるとその席の閉じがほぼ全部断られる（repo ごとの件数は台帳の memo にだけ在る）。持ち主の裁定 user 2026-09-29T21:53Z（逐語は台帳）で、門はまず本 repo だけに掛け、他の repo は宣言して加わる（ADR-0097）。
- 文法（頭の字の正本は ADR-0089・着地の形の尾の字の正本は ADR-0094・本 § は値の形と語の切り方の正本）:
  - **語の切り方**: 理由の字の前後の空白を除き、空白（Unicode の空白・全角の空白を含む）で語に割る。1 語目が頭（完全一致・大文字小文字を区別する・`Landed` や `昇格済み:` は頭でない）。値は 2 語目で、その後ろの語は読まない（取り下げと昇格済みと着地の形の尾を除く）。句読点や括弧は区切りでない（`重複 s2-1、` の値は `s2-1、` で bead id でない）。

    | 頭 | 字面 | 値の形 | 書く者 | 種類 |
    |---|---|---|---|---|
    | landed | `landed <着地 commit id> <尾>` | 着地 commit id は 40 桁の 16 進（大文字小文字を問わない）。尾は閉じた 3 形 `ci=success`・`ci=success tip=<40 桁の 16 進>`・`ci=none`（ほかの尾は読めない尾として返し、着地の形かは頭と id で決まる・ADR-0089） | land の終端（経路 1・2）と `pipe retire`（経路 3）だけ（席の close は断る） | 契約 |
    | 重複 | `重複 <bead id>` | 同じ台帳の bead id（§15 の形・`is_bead_id`） | 席 | 契約 |
    | 後継 | `後継 <bead id>` | 同上 | 席 | 契約 |
    | 取り下げ | `取り下げ <理由>` | 頭の後ろの字の全部（空白でない字が 1 つ以上） | 席 | 契約・epic |
    | 裁定 | `裁定 <裁定 id>` | 裁定 id（下の 3 形） | 器の口（bind と答えの口・FR82）と席 | 台帳の問い・decision |
    | 昇格済み | `昇格済み <契約 id の列>` | 頭の後ろの語の全部が同じ台帳の bead id（1 つ以上・`,` は区切りでなく、`,` を含む語は id でない） | 器（memo の自動の close・FR93）と席 | memo |
    | まとめた | `まとめた <memo id>` | 同じ台帳の bead id | 席 | memo |
    | 見送り | `見送り <裁定 id>` | 裁定 id | 席 | memo |
    | 完了 | `完了` | 値なし（後ろの字は読まない） | 席 | epic |

  - **裁定 id の形**（閉じた 3 形・SRS FR82 / FR83 の問い id の形と batch: と policy: の形）: `<問い id>:<YYYYMMDDTHHMMZ>-<n>`（問い id は同じ台帳の bead id・時刻は UTC の年月日と時分で `epoch_of` が読める範囲〔月 1〜12・日 1〜31・時 0〜23・分 0〜59〕・n は ASCII の数字だけの 1 以上の整数で 1 に固定しない・例 `s2-07l.739.1:20260928T1347Z-1`）・`batch:<字>`・`policy:<字>`（字は 1 字以上）。形の判定は読み手の 1 関数が外へ見せ、局面の関数と引用の数え（FR83）の後の行が同じ関数を引く（字句を狭めるならこの関数で）。
  - **台帳の接頭辞**: §15 と同じく、payload の cwd から上へ辿った最初の `.beads` の dir を持つ dir の `Anchor` から解く（`ledger_prefix`）。解けない周は bead id を値に持つ形（重複・後継・まとめた・昇格済みと、問い id の形の裁定 id）が読めない（fail-closed）。
  - 種類と形の食い違い・解けない id・閉じた時点の条件は門の外（局面の出力が misfit で数える・ADR-0089）。
- **門を掛ける repo**（ADR-0097・本 § の全部の段に共通）:
  - **宣言の key** close-check: `.vessel.toml` の任意 key。値は真偽だけ（`close-check = true` / `false`）で、文字列・整数・列・key の重複は key と行番号を名指す宣言の不備。書かない宣言は掛けない（今の消費側の宣言を 1 行も変えさせない）。名は既存の加わりの key（SRS FR83 の ruling-check・FR85 の floor-check）の綴りに合わせる。
  - **読みの形**: close の段に当たる segment（形 l1-2 と形 l2-1・l2-2 の口）を持つ command の周にだけ、`ledger_prefix` と同じ根（payload の cwd から上へ辿った最初の `.beads` の dir を持つ dir）の HEAD の宣言を、行 l が足す閉じた 3 値の読み口 1 本で読む（中は `head_declaration`・作業ツリーは読まない・台帳は読まない）。
    - **加わる**: key が true。本 § の判定を掛ける。
    - **加わらない**: 宣言を読めて key が false か無い・宣言 file が HEAD に無い・git を撃てない・`.beads` の dir を持つ祖先が無い。git を撃てない周は宣言の無い周と同じに読む（読み手は 1 本で 2 つを分けない・ADR-0084 の question-route と同じ読み）。本 § の段を撃たずに通す（他の段は今のまま）。
    - **読めない**: 宣言が在って不備（close-check の値の型違いと、古い binary の読み手が知らない key を含む）。本 § と同じ判定を掛け、断る周は語 **close-declaration-unreadable** で、当たった形の語（下の 6 語のどれか）と宣言を直す次の一手を断り文に名指す。形に合う閉じは通す（C10: 読めないを「無い」に倒さない・C11.2: 起票の門は FailClosed・加わりを名乗っていない repo の形に合う閉じまでは止めない）。
  - key は、その key を知る binary に PATH を入れ替えた後でだけ宣言に書く（古い読み手は未知の key を宣言ごとの不備にし、受付・path の種別・land の終端が止まる）。本 repo は行 l3 で書く。
- 形（行 l・番号は行 l の done の (1)〜(4) と 1:1・(5) は歯）:
  1. **close の理由の読み手は行 l の write-set の + の file の純関数 1 本**: 入力は理由の字と台帳の接頭辞（解けない周も渡せる）だけで、I/O も時計も持たない。上の文法で、頭と値（landed は着地 commit id と尾の閉じた 3 値か読めない尾・昇格済みは id の列・取り下げは理由の字）か、閉じた欠陥（空・頭の外・値の形の外〔値の欠けを含む〕・接頭辞が無く id を読めない）を返す。bead id は `is_bead_id`、裁定 id の時刻は `epoch_of` に `-`・`:`・秒 `:00` を足した字で読む（自前の読みを持たない・C2）。裁定 id の形の判定を外へ見せる。land の終端と `pipe retire` が書く 3 形（ci=success・tip つき・ci=none）は着地の形と読める。
  2. **宣言の値に真偽を足す**: `Raw` に真偽の variant を 1 つ足し、`value_of` は manifest の読み `scalar` が返す真偽をその variant にする（新しい読みを書かない）。`entrance_of` の網羅の match は真偽を閉じた 3 語の外として不備にする（今の整数・列と同じ・entrance-flip = true は key と行番号を名指す不備）。真偽の値を書いた他の key は、今の「value を読めない」から key ごとの型の不備（「は整数である」等・既存の文）に変わる（どちらも不備で、宣言を読めない扱いは変わらない）。
  3. **key close-check と閉じた 3 値の読み口**: `crates/scribe2/src/pipe/declaration/optional_keys.rs` に key の名・読み手（真偽だけを受け、他の値は key と行番号を名指す不備）・`DECLARED_KEYS` と `OPTIONAL_KEYS` の末尾の 1 行ずつと、HEAD の宣言から加わる・加わらない・読めないの閉じた 3 値を返す口を足し（`QuestionRoute` と同じ形・親の再輸出で外へ見せる）、親の `Declared` に欄を 1 つと `parse` の読みの 1 行を足す。
  4. **門の判定は変えない**: `crates/scribe2/src/hook/ledger_guard.rs` を触らず、本 repo の `.vessel.toml` に key を書かない（読み手だけが先に着地し、binary を入れ替えても誰の閉じも変わらない）。
- 形（行 l1・番号は行 l1 の done の (1)〜(4) と 1:1・(5)(6) は歯）:
  1. **掛ける周**: 上の「門を掛ける repo」の読みの形のとおり（close の段に当たる segment が在る周だけ宣言を読む・加わらない周は本 § の段を撃たない）。
  2. **close に数える書き（理由を持つ口）**: bd / bdw の `close` と `done` と `gate resolve`。理由は `-r` と `--reason`（`=` の形も）の値の全部と、`--reason-file` の file の字（payload の cwd から解く・前後の空白を除く）で、出てきた順に読み手へ掛け、最初に外れた理由の語で断る（id の数と理由の数の対応は見ない＝bd が断る）。理由の値を 1 つも持たない close（id の無い close も）は close-no-reason。
  3. **断りの語**（記録は `ledger-deny <語>`・行 l1 は閉じた 5 語）: close-no-reason（理由が無いか空）・close-landed（頭が landed・値の形を問わない）・close-outside-forms（頭の外・値の形の外・接頭辞が解けず id を読めない）・close-reason-unreadable（`--reason-file` が `-` か値が無いか開けない・理由の値が `$` か backtick を含む）・close-declaration-unreadable（宣言を読めない repo で上の 4 つのどれかに当たった周）。断り文は既存の形 `deny bd close は起票の門が止める reason=<語>（<説明と次の一手>・ledger-form.md §16）` の 1 行で、close-landed は「着地の形は land の終端と pipe retire だけが書く — 止まった終端は原因を直して pipe land --run <run> --terminal-only で撃ち直し、PR の便は pipe retire --run <run>（どちらも名指しの 1 行なら席の決着の権能 settle で撃てる・seat-roles.md §32）」を、close-no-reason と close-outside-forms は landed を除く 8 つの頭の字面（重複 <bead id>・後継 <bead id>・取り下げ <理由>・裁定 <裁定 id>・昇格済み <契約 id …>・まとめた <memo id>・見送り <裁定 id>・完了）を（close-outside-forms は理由の先頭 40 字と欠陥も）、close-reason-unreadable は読めない形と `--reason '<形>'` を次の一手に持つ。close-declaration-unreadable は当たった形の語と、`.vessel.toml` の不備を直す（close-check の値は true か false）次の一手を持つ。
  4. **段の位置と読み**: 6 形と update の引き金の段の後（6 形に当たる close は 6 形の語が先＝埋め込みの rules では `bd close` は bd-outside-bdw）。segment の読みは `write_of` の `Write`（flag と値）をそのまま使い、`Write`・`Create`・`Refusal` に変種も field も足さない。台帳を読まない（NFR5・門の予算は rules 行 `hook.budget_ms`）。宣言の読みと接頭辞の解きは close の segment が在る周だけ（git の子 process は宣言の読みの 1 回）。
- 形（行 l2・番号は行 l2 の done の (1)〜(3) と 1:1・(4)(5) は歯）:
  1. **理由を渡せない close の口**: 加わる repo と読めない repo で、bd / bdw の `update` の `--status` / `-s`（`=` の形も）の値 closed は status-closed で断る（次の一手 `bdw close <id> --reason '<形>'`・読めない repo は close-declaration-unreadable で status-closed を名指す）。
  2. **bd が理由を書く口**: 同じ repo で、`duplicate`・`supersede`・`epic close-eligible`（`--dry-run` を持つ周は閉じないので通す）は implicit-reason で断る（次の一手 `bdw close <id> --reason '重複 <id>'` / `'後継 <id>'` / `'完了'`）。行 l1 の 5 語と合わせて断りの語は閉じた 7 語。段と掛ける周は行 l1 と同じ読みで、status-closed は台帳の形の門（`crates/scribe2/src/hook/graph_guard.rs`）の読みより前に断る（台帳を読まない）。
  3. **器の出す 1 行を合わせる**: doctor の台帳のグラフの行の直す形（`crates/scribe2/src/ledger/graph.rs` の `FIX`）の `close-eligible は bdw close <epic>` を `bdw close <epic> --reason 完了` にする（加わる repo の門が理由の無い close を断るので、器の 1 行が断られる command を名指さない・加わらない repo でも同じ字で通る）。key と順は変えない。
- 形（行 l3・番号は行 l3 の done の (1)〜(2) と 1:1・(3) は歯）:
  1. **本 repo が加わる**: 本 repo の `.vessel.toml` の末尾に close-check を true で書き、注記 1 行で本 § と ADR-0097 を指す。
  2. **手順の字を同じ PR で直す**: `CLAUDE.md` の作業の流れの 5 と done の定義の見出し・`.beads/PRIME.md` の前提と R0 と Essential Commands（字は同じ束の c-edits の草稿・path の種別が code の file なので docs PR でなく本行の write-set に置く）。
- 触らない: land の終端と `pipe retire` の close の字と撃ち方（`close_reason`・`CloseTail`・`CLOSE_REASON`・子 process）・6 形（`FORMS`）と rules 行 `ledger.denied_writes`・`Refusal` と `Write` と `Create`（変種も field も足さない）・§14 と §15 の段と語（全部の repo のまま）・極性一覧（同じ guard の中の段）・`crates/scribe2/src/hook/mod.rs`・`crates/scribe2/src/hook/role_guard.rs` と rules 行 `role.orchestrator`（seat-roles.md §32 の行 z が持つ）・reopen・台帳の読み（NFR5）・他の任意 key の読み手と `QuestionRoute`。閉包: 行 l の + の file は `Refusal`・`Write`・`Create` を名指さず、ledger_guard の型の中括弧の literal をどの file にも足さない（行 g・h・j と vessel-hook 行 a の閉包を広げない）。行 l1・l2 は行 l の + の file を write-set に名指さない（同じ docs PR では base に無く、着地の後は + を持つ行が受付で断られる）。
- 締め出しと手順の字（行 l3 の着地の後・本 repo）:
  - **席は契約を着地の形で閉じない**（close-landed）。契約の close は器の 3 つの経路が書く: remote を宣言する repo の便は land の終端が push の後の CI の照合で `ci=success`、remote を宣言しない repo の便は land の終端が照合なしで `ci=none`、`--pr-cmd` の便は merge の後に `pipe retire --run <run>` が照合して閉じる。
  - **終端が止まった便**（`terminal:ci:failure`・`terminal:ci:unmeasurable`・`terminal:push:failed:…`・`terminal:close:failed:…`・`terminal:unreadable`）は、原因を直して（main の CI の撃ち直し等）から `pipe land --run <run> --terminal-only` で終端だけを撃ち直す（冪等・撃った時点の宣言で経路を選び直す・先端でない便は行 bm の先端の照合）。2026-09-25 以後に手で閉じた契約 7 本はどれもこの口で閉じる形。
  - **名指しの 1 行なら orchestrator の席が撃てる**（決着の権能 settle・seat-roles.md §32）: 撃ち直しは `--run <run> --terminal-only` に、置き場か repo の値の対（hook が解いた席の置き場と anchor と同じ字だけ・rules は足さない）だけを足した 1 行、PR の便の閉じは `pipe retire --run <run>` に同じ対だけを足した 1 行（照合の道が unmeasured になる PR の便を畳むだけなら `--fold-only` を 1 つ足してよい・契約は開いたまま残る）。後ろに redirect や pipe を付けた形・道具の flag を持つ形は merge か launch に落ちて断られる。
- 却下（ADR-0097 が持つ）: 門を全部の repo に掛ける（器の便でない着地で閉じる消費側の席の閉じがほぼ全部断られる）／消費側の閉じの形が決まるまで門を起こさない（本 repo の外れが止まらない）／掛ける repo を rules 行か host の面で選ぶ（repo の名乗りを tracked でない面か裁定の要る面に置く）／宣言を読めない repo の close を全部断る（加わりを名乗っていない repo の形に合う閉じまで止める）／宣言を読めない repo を宣言の無い repo と同じに通す（C10・C11.2）。本 § の中の却下: 着地の形を「席が CI を確かめた周だけ」通す（門は台帳も CI も読まない・ADR-0094 は席の申告から着地の印を書く案を退けた）／`Landed`・`着地` を着地の形と読む（閉じた字面は 1 つ・大文字小文字を区別する）／値の語を空白の外の字でも切る（§15 の「空白で割った 1 語」と読み手が 2 通りになる）／`昇格済み` の列を `,` でも区切る（区切りが 2 通りになる）／`update --status closed` を通して局面の出力で数える（FR81 (d) は理由の無い close を編集の時点で止める）／id の数と理由の数の食い違いを門が断る（bd が断る）／台帳を読んで種類と形の食い違いを断る（NFR5・ADR-0089 が退けた）／`-h` と `--help` を免じる（今の門も create の `--help` を断る・help は `bd help <sub>` で読める）／`$` を含む理由を字のまま読む（展開の後の字を門は知らない）／close-check を文字列の `"on"` 等で書かせる（真偽の key の前例 ruling-check と揃えない・値の読みが 2 通りになる）。
- 限界: 加わらない repo では和の外の close と席の着地の形の close がそのまま通り、memo の自動の close（FR93）は席が書いた着地の形も着地と読む。宣言の git を撃てない周は宣言が無いと同じに読み、門を掛けない。宣言の読み手は 1 本で git を撃てない周と宣言 file の無い周を分けず（ADR-0084 の question-route と同じ読み）、加わった repo でも git が落ちた 1 回は門の外になる。`.beads` を持つ祖先の HEAD で読むので、key を書く前の commit を HEAD に持つ worktree の中の close は掛からない。器の便の外で着地した契約（席が自分で commit か PR を出した仕事）は着地の形を持てず、席が書ける契約の形は重複・後継・取り下げだけ（ADR-0089・ADR-0094 は席の申告の着地の印を退けた）。`pipe retire --fold-only` で畳んだ PR の便の契約も同じ。`bdw sql`・`import`・`restore`・`gate check`・custom の閉じの状態・molecule の段の閉じは見ない。`-C` / `--db` / `--actor` など値を取る全体の flag を subcommand の前に置いた command（例 `bdw --actor x close <id>`）と `sh -c '…'` の中の close は subcommand を読み違えて通る（`write_of` の読みと 6 形と同じ限界）。行 l3 の done (2)（手順の字）は歯を持たない（字の pin を書かない・審査が読む）。別の台帳の id を名指す理由は形の外で断る。`-r<字>`（値を続けた短い形）と `-` で始まる理由の値は理由と読まれず close-no-reason に倒れる（断る側）。`$` か backtick を含む理由は単引用符の中の字でも読めない側に倒れる。消費側の面が子 process で書く close は門の外。
- 既存の歯の直し:
  - 行 l: `crates/scribe2/src/pipe/declaration.rs` の in-file の `declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema` が pin する `DECLARED_KEYS` の列の末尾に close-check を足す（base の列は key を持たないので直した歯は base で赤＝札は要らない）。
  - 行 l1: 無い。今の歯の toy repo は close-check を宣言しないので、`hook_ledger_write_passes_the_near_misses` の `bdw close s2-1 --reason x` と `hook_graph_guard_reads_only_for_the_six_writes_in_a_ledger_repo` の `bdw close E.1` は門に掛からず、字を変えずに緑のまま。
  - 行 l2: `crates/scribe2-boundary/tests/e2e/ledger_form.rs` の `FIX` と bin の外形 snapshot（`ledger_graph_doctor_external_form`）の直す形の字を合わせる（base は旧い字を出すので赤）。`hook_graph_guard_plan_and_reopen_count_against_the_parent` の (j) の「--status closed は通る」は宣言の無い toy repo の周なので変えない。
  - 行 l3: 無い（新しい歯だけ）。
- 歯:
  - 行 l（接頭辞 ledger_close_reason_ と declaration_close_check_・`git grep` はどちらも 0 件・2026-09-30）:
    - lib（行 l の + の file の in-file・ledger_close_reason_）: 9 つの頭 × 読める値と読めない値・`Landed` と `昇格済み:`・全角の空白の割り・`取り下げ` だけ・`,` を含む列・裁定 id の 3 形（n が 0 と 2・月 13 の時刻・時刻の桁の欠け）・landed の尾の 3 形と読めない尾・大文字の 16 進・接頭辞の無い周。
    - lib（`crates/scribe2/src/pipe/declaration/optional_keys.rs` の in-file・declaration_close_check_）: true と false と key の無い宣言の値、`"true"`・`1`・`["x"]`・key の重複が key と行番号を名指す不備、HEAD の読みの結果（無い・不備・値）から閉じた 3 値への写し（`QuestionRoute` の写しの歯と同じ形）、真偽を書いた他の key（`remote = true`）が不備のまま、entrance-flip = true が閉じた 3 語の外の不備（key と行番号を名指す）。
  - 行 l1（接頭辞 hook_close_reason_・`git grep` 0 件）:
    - e2e（`crates/scribe2-boundary/tests/e2e/hook/guards.rs`・新しい e2e の file は作らない）: toy repo は次の 6 つ（置き場の helper `linked` は vessel init を撃ち、宣言の無い repo にも雛形の `.vessel.toml` を書いて commit するので、宣言の無い repo はその後に消す）: 加わる repo＝既存の `ask_place` に close-check = true を渡して宣言を commit し、返った repo に `.beads/config.yaml`（接頭辞 toy）を書く／false の repo・key の無い repo＝同じ形で false・空を渡す／読めない repo＝同じ形で文字列 yes を渡す／宣言の無い repo＝`git_repo` と `linked` の後に `git rm .vessel.toml` を commit し `.beads/config.yaml` を書く／`.beads` の無い repo＝close-check = true を commit し `.beads` を書かない／作業ツリーだけの repo＝key の無い宣言を commit した後、作業ツリーの `.vessel.toml` にだけ close-check = true を書く（commit しない）。rules は §14 と同じ写し（`question_rules`・`rules/manifest.toml` を include_str で読んだ `QUESTION_EMBEDDED` から bd-outside-bdw を外した写し。bd の直の close も 6 形の語に落ちずに close の段に届き、(a)〜(g) の bd の経路が close の段の語を測る）で、argv を記録する偽の client を `--bd` に渡す。断りの外形（rc 2・stdout 0 byte・stderr 1 行・`deny bd close` の頭と §16 の末尾・記録 1 行）を測る helper は同じ file に足す（`assert_trigger_deny` は §15 の末尾を pin するので使わない）。(a) 加わる repo で、理由の無い close・和の外の理由（`--reason "done it"`）の close・着地の形の理由（`landed <40 桁> ci=success`）の close の 3 形を bd と bdw の 2 経路で撃ち、6 本がどれも断られ語は close-no-reason・close-outside-forms・close-landed (b) landed を除く頭を種類との組 10 形で書いた close を bd と bdw の 2 経路で撃ち、20 本がどれも rc 0 で記録を残さない (c) `-r`・`--reason=`・`--reason-file <file>`・`done`・`gate resolve -r` の理由が読まれ、2 つの id と 2 つの `--reason` で 2 つ目だけが形の外の周は close-outside-forms (d) `--reason-file -`・値の無い `--reason-file`・無い file・`"$(cat r.txt)"` の理由は close-reason-unreadable (e) 別の接頭辞の `重複 other-1`・`,` で繋いだ昇格済みの列・n が 0 の裁定 id・`Landed <40 桁>` は close-outside-forms (f) false の repo・key の無い repo・宣言の無い repo・`.beads` の無い repo・作業ツリーだけの repo で (a) の 6 本がどれも rc 0 で記録を残さない (g) 読めない repo で理由の無い close が close-declaration-unreadable で断られ断り文が close-no-reason を名指し、`取り下げ x` の close は rc 0 (h) close を中で撃つ toy repo の script を起こす command（bd / bdw の segment を持たない）は rc 0 (i) 撃った全部の周で偽の client が 1 回も起きない。(b)(f)(h)(i) は base でも緑なので (a) と同じ歯の中に置く（単独の歯にしない）。
    - lib（`crates/scribe2/src/hook/ledger_guard.rs` の in-file・hook_close_reason_）: 3 値ごとの掛け方（加わる・加わらない・読めない）を `decide` の手前の pure な判定で、理由の集め方（`-r`・`-r=`・`--reason=`・値の無い `--reason`・`$` と backtick の値・`done`・`gate resolve`・2 つの理由の順）を `decide` で。宣言の読みは toy の git repo を tmp に作って撃つ。
  - 行 l2（接頭辞 hook_close_mouth_・`git grep` 0 件）:
    - e2e（同じ file・行 l1 の helper を使う）: (a) 加わる repo で `update <id> --status closed`・`-s closed`・`--status=closed`・`-s=closed` と `duplicate <id> --of <id>`・`supersede <id> --with <id>`・`epic close-eligible` を bd と bdw の 2 経路で撃ち、14 本がどれも断られ語は status-closed・implicit-reason (b) 同じ repo で `update <id> --status pinned`・`epic close-eligible --dry-run`・`epic status` は rc 0（近い形は台帳の形の門が数えない書きにする＝`--status open` は形の門が数えに戻す書きとして台帳を読み、偽の client では ledger-unreadable で赤になる） (c) 行 l1 の宣言の無い repo で (a) の 14 本が rc 0。(b)(c) は (a) と同じ歯の中に置く。
    - lib（`crates/scribe2/src/hook/ledger_guard.rs` の in-file・hook_close_mouth_）: `--status` の 4 つの綴り・`--dry-run`・`epic status` を `decide` で。
  - 行 l3（接頭辞 declaration_close_check_own_・`git grep` 0 件）: lib（`crates/scribe2/src/pipe/declaration/optional_keys.rs` の in-file）: 本 repo の `.vessel.toml`（`CARGO_MANIFEST_DIR` から 2 つ上）を `Declared` の読みで読み、close-check が true で宣言が読める（`crates/scribe2/src/pipe/declaration.rs` の本 repo の宣言の歯と同じ読み方）。
- base で RED の理由: 行 l — base の宣言の読み手は close-check を未知の key として断り、真偽の値を読めず、3 値の読み口が無い（`crates/scribe2/src/pipe/declaration/optional_keys.rs` の in-file の歯は compile が通らない＝機能不在）。`DECLARED_KEYS` の pin の直しも base の列と食い違う。+ の file の in-file の歯は flip-check が base へ写さないので、flip の証拠は `crates/scribe2/src/pipe/declaration/optional_keys.rs` と `crates/scribe2/src/pipe/declaration.rs` の in-file の歯が持つ。行 l1 — 行 l の着地の後の base の門は close の理由を見ないので、(a) の 6 本・(c) の 2 つ目の理由・(d)・(e)・(g) が rc 0 で通る（機能不在）。行 l2 — 行 l1 の着地の後の base は update の `--status closed` と bd が理由を書く口を加わる repo でも通し、doctor の直す形は旧い字を出す（機能不在）。行 l3 — base の本 repo の宣言は key を持たない（機能不在）。
- 着地の順と binary の入れ替え:
  1. 行 l → l1 → l2 の順（depends）。3 行の着地の後に PATH の binary を 1 回入れ替える（入れ替えの script で・走行中の運転手が無い周に）。宣言を読めて close-check を書いていない repo の閉じは変わらない。**宣言が在って読めない repo には門が掛かる**ので、入れ替えの前に、この binary を使う全部の host の全部の置き場の anchor について、HEAD の宣言が読めること（読めない repo 0 件）を測る。測る口は doctor に無い（`init=` は宣言の在否だけを測る）ので、anchor ごとに `git -C <anchor> show HEAD:.vessel.toml` で宣言が在るかを見て、在る anchor で入れ替える binary の `contracts check --repo <anchor>` を撃ち、宣言の読みの不備の行（key と行番号を名指す行で、上限との突き合わせの行でない）が 0 件であることを見る（宣言の読みは上限との突き合わせより先に落ちるので、読みの不備が在ればその行だけが出る）。読めない repo が在れば、入れ替えの前にその repo の持ち主へ宣言の直しを頼む。
  2. seat-roles.md §32 の行 z（決着の権能）は行 l〜l2 と独立で、先でも後でもよい。行 l3 の前に着地し、同じ入れ替えか先の入れ替えで PATH に載っていること（門が入った後に止まった終端を席が閉じる手）。
  3. 行 l3 は、本 repo を対象にする全部の host・全部の置き場の binary を、行 l の読み手と行 z の権能を持つ binary に入れ替えた後にだけ起こす（古い binary が本 repo の宣言を読めなくなり、受付と land の終端と権能 guard の path の種別が止まる）。台帳では行 l3 の bead を行 l2 と行 z の bead の blocks に置き、bead の起票そのものを入れ替えの後にする（blocks は入れ替えを表せない）。行 l3 の着地の後は入れ替え不要（宣言の key を足すだけ）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "台帳 lint の項目 5 つ — memo の 4 節の欠け・field の 4 象限の違反 2 形・§ か本文が名指す memo への discovered-from の無い契約・契約表の未着地の行と台帳の drift・辿れる契約が全部 closed の open な memo を、件数と母集団と id で doctor の 1 行に出す"
req = ["FR51"]
section = "3"
write-set = ["+crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "+crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "+crates/scribe2-boundary/tests/e2e/ledger_form.rs", "crates/scribe2-boundary/tests/e2e/main.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail ledger_form_", "cargo nextest run -p scribe2 --bin scribe2 --no-tests=fail ledger_form_"]
size = "M"
done = "(1) doctor の項目に台帳の形の 1 行が増え、偽の台帳 client の出力で memo の 4 節の欠け（出所なし 1・観測なし 2・候補なし 0・昇格条件なし 3）と 4 象限の違反 2 形と § か本文が名指す memo への discovered-from の無い契約と契約表の未着地の行のうち pointer を持つ open の bead が無い行（drift・行の id で名指す）と辿れる契約が全部 closed の open な memo の件数が母集団と同じ行に出て、欠陥の bead の id が種類ごとに名指される (2) epic と裁定の bead は 4 象限の母集団から外れる (3) 欠陥 0 の周も 0 と母集団が出て行が消えない (4) client が起動できない・rc ≠ 0・出力が壊れた周は件数 0 に倒れず測れていない形の行が出る (5) 判定は Issue の label と acceptance と description と notes と dependencies と、pointer の先の § の本文（行 e の読み手が開く doc）と契約表の行と tracked な file の集合（(vii) の未着地の行の弁別・§3 の 5 と同じ読み）だけを読む純関数で、台帳の書きの口は増えない"

[[contract]]
id = "b"
title = "xtask の口 — 全設計 doc の契約表の未着地の行から bd create --graph の plan JSON を出す（台帳は読まない・drift は doctor の lint の側）"
req = ["FR47"]
section = "3"
write-set = ["+crates/xtask/src/ledger_plan.rs", "crates/xtask/src/main.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail ledger_plan_"]
size = "M"
done = "(1) 未着地の行（write-set か symbols の + の file が tracked に無い行）だけが plan に載り、着地済みの行は載らない (2) plan の 1 件は title・acceptance の pointer 行・引数の epic id の parent・doc 名と size の label を行から写し、depends が blocks の edge に、§ の本文が名指す bead の id（台帳の id の字面・memo か否かは問わない）が discovered-from の edge に写る (3) epic id の引数が無い周は plan を出さず rc 1 (4) 台帳を 1 度も読まず書かない（PATH の先頭の偽の bd が 1 回も呼ばれない） (5) 出力は標準出力の JSON 1 つ"

[[contract]]
id = "c"
title = "memo の入口 — 器の read-only の口が便の終端（--run）か user の要望（--from user）から memo の plan（bd の create の引数と 4 節の本文）を標準出力に出し、席が bdw で撃つ"
req = ["FR51", "FR31"]
section = "3"
write-set = ["+crates/scribe2/src/ledger/memo.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2-boundary/src/main.rs", "+crates/scribe2-boundary/tests/e2e/ledger_memo.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__ledger_memo__ledger_memo_plan_usage_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail ledger_memo_plan_"]
size = "M"
done = "(1) 終端の run（Gated / Reviewed の FAIL・INCONCLUSIVE・Failed・Questioned）の run dir と event log から、### 出所 に run id と段と kind が、### 観測 に終端の種類ごとの原本（Gated = verdict.json の evidence と at・Reviewed = review.json の evidence と at・Questioned = 質問の逐語と about・Failed = Failed の detail と ts）が写った plan が出て、原本の無い終端は閉じた理由で断られ、### 候補 と ### 昇格条件 は空の見出しで出る (2) 終端でない run と run dir の無い id は閉じた理由で断られ rc 1 (3) --from user の plan は ### 出所 に逐語の在り処の 1 行と日付を持ち ### 観測 が空 (4) label intake:memo・引数の parent・引数の関連 bead への relates-to が plan に載る (5) 出力は標準出力だけで、偽の bd を PATH に置いても 1 回も呼ばれない (6) usage の 1 枚の外形 snapshot"

[[contract]]
id = "d"
title = "起票の門 — PreToolUse の hook が memo の create に 4 節の本文を要求し、契約の create に intake:memo が無いことを要求する guard（in-loop・fail-closed・極性一覧に 1 つ増える）"
req = ["FR20", "FR51"]
section = "3"
write-set = ["+crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail hook_memo_guard_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_summary_counts_match_lines", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_all_is_in_declaration_order"]
size = "M"
done = "(1) [memo] の title か intake:memo の label を持つ bd / bdw の create は body-file の本文に memo の 4 節の見出しが全部在れば通り、1 つでも欠ければ閉じた理由 1 つで deny される (2) acceptance に設計 pointer 行を持つ create が intake:memo を持てば deny される (3) body-file が無い・開けない周は deny に倒れる (4) memo でも契約でもない create（epic・裁定）と create 以外の bd の command は 1 字も変わらず通る (5) 極性一覧の外形 snapshot に guard が 1 つ増え、guard の総数を pin する歯が新しい母集団で緑"
[[contract]]
id = "e"
title = "ledger-plan の plan JSON の node と edge の key を bd 1.1.0 の graph schema に合わせ、schema が運べない pointer 行を plan の後ろの対応表に出し、行を外す --skip の口を足す"
req = ["FR47"]
section = "9"
write-set = ["crates/xtask/src/ledger_plan.rs", "crates/xtask/src/main.rs", "docs/design/ledger-form.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail ledger_plan_renders_the_bd_graph_schema_field_names", "cargo nextest run -p xtask --no-tests=fail ledger_plan_emits_the_pointer_line_table_after_the_plan", "cargo nextest run -p xtask --no-tests=fail ledger_plan_skips_the_rows_named_by_the_skip_argument"]
size = "M"
done = "(1) plan の node の key の集合が key・title・type・description・labels・parent_id に閉じ、edge の key の集合が from_key と to_key か to_id と type に閉じて、acceptance・parent・from・to という key が node にも edge にも無い（key 単位で測る・値の文字列は問わない） (2) 出力の 2 行目以降が node と同じ本数の対応表で、各行が plan の key と design = <doc>#<行 id> を TAB で持ち、stdout へ書く呼び出しは 1 回のまま (3) --skip が名指した契約 id の行だけが plan から消え、残りの行と edge が不変で、plan に無い id を渡すと rc 1 で断り、usage の 1 行が --skip を写す (4) rules/manifest.toml の行数が base と同じで bd の版を持つ行も const も増えず、台帳を 1 度も読まない（PATH の先頭の偽の bd が 1 回も呼ばれない）歯が緑のまま、§3 の 5 の字面が対応表の形を写す"
[[contract]]
id = "f"
title = "台帳のグラフの形を doctor の 1 行で数える — 根の epic に着かない bead（open と closed）・鎖の終わりの非 epic・2 つ目の親・親の輪・直下の open の子が rules 行 ledger.open_children_max を越える親・子が全部 closed の open な epic を件数と id で出し、判定は兄弟 module の純関数 1 本（blocks の輪は bd が書きの時点で断るので数えない）"
req = ["FR51", "NFR4"]
section = "10"
touches = ["crate::rules::RuleKind"]
write-set = ["+crates/scribe2/src/ledger/graph.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/ledger/lint.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/src/main.rs", "+crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_graph_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/ledger_form.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail ledger_graph_", "cargo nextest run -p scribe2-boundary --bin scribe2 --no-tests=fail ledger_graph_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_open_children_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all"]
growth = ["crates/scribe2/src/ledger/mod.rs:3", "crates/scribe2/src/ledger/lint.rs:4", "crates/scribe2/src/rules/mod.rs:6", "crates/scribe2-boundary/src/main.rs:40", "crates/scribe2-boundary/tests/e2e/ledger_form.rs:130", "crates/scribe2-boundary/tests/e2e/rules.rs:25", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs:2"]
size = "L"
done = "(1) 判定は兄弟 module の純関数 1 本で Issue の列と上限 N だけを読み、根は親を持たない epic・親は deps の parent-child の最初の 1 本・根に着かないは親をたどって根に着かない（親が台帳に無い・輪に入る・親を持たない非 epic で止まる）で、付け先が根に着くか・top・直下の open の子の数・子孫かの問いを module の外へ見せる (2) 根に着かない bead の open と closed の件数・open を含む鎖の top の id・closed だけの鎖の top の件数・親 2 つと親の輪の bead の id・直下の open の子（closed でも pinned でもない子）が N を越える親の <id>/<子の数>・子が全部 closed の open な epic の id を数え、N が 0 の周は over を数えず - を出す（歯は doctor の --repo と --rules の口で値 0 の写しを渡す）(3) doctor の --repo の出力で台帳 lint の行と台帳の形の行の間に台帳のグラフの 1 行が増え、beads= open= max= unrooted= unrooted-closed= tops= tops-closed= two-parents= parent-loops= over= close-eligible= の順で、違反が 1 つ以上の周だけ行の末尾に ' — ' と直す形が 1 回付き、違反 0 の周も行が消えず、台帳は 1 回だけ読まれ（偽の client の記録 1 行）台帳の形の行が doctor の末尾のままで、外形 snapshot が測れた周（全欄 1 件以上）と測れない周の 2 行を持つ (4) 埋め込みの manifest に行 ledger.open_children_max（kind LedgerOpenChildrenMax・Int・値 15・裁定 id user 2026-09-27T17:33Z 項 2-3）が ledger.denied_writes の直後に、kind が ALL の LedgerDeniedWrites の直後に在り、行数と kind の数の pin と rules_external_form の snapshot が 1 ずつ増え、LedgerDeniedWrites からの kind の並びを測る歯がその kind を持ち、読めない台帳は unreadable reason=ledger-unreadable・行の無い rules は unreadable reason=no-rule で件数を 1 つも出さない（歯は同じ口で行を除いた写しを渡す）(5) blocks の輪は数えず、行に blocks の輪の欄は無い"

[[contract]]
id = "g"
title = "台帳 write の形に create-bypass（q・todo add・batch・create-form）と parent-edge（dep add と link の parent-child・dep add の --file・create の --deps の parent-child:）を足す — Write が flag でない語と flag の値を運び、rules 行 ledger.denied_writes の値を 6 語・裁定 user 2026-09-27T14:02Z 項 5 に替える（既存の 4 形の判定と字面は不変・host の見張りも同じ列で断る）"
req = ["FR20", "FR51"]
section = "11"
touches = ["crate::hook::ledger_guard::Refusal", "crate::hook::ledger_guard::Write"]
write-set = ["crates/scribe2/src/hook/ledger_guard.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "crates/scribe2-boundary/tests/e2e/rules.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail hook_ledger_edge_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_ledger_edge_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_ledger_denied_writes_row_is_declared_on_four_faces"]
size = "M"
done = "(1) FORMS が 6 形で、判定の順は既存の 4 形の後ろに create-bypass → parent-edge (2) bd と bdw のどちらでも q・create-form・batch と、次の語が add の todo が create-bypass で、todo list と todo done は当たらない (3) dep add と link の --type・-t・--type= の値 parent-child と、dep add の --file と、create の --deps の値の parent-child: が parent-edge で、dep add の blocks・link の既定・dep remove は当たらない (4) Write は flag でない語と flag の値を運び、構築点は write_of の 1 か所で、既存の 4 形の当たりと当たらない例と deny の字面は変わらない (5) 埋め込みの rules 行 ledger.denied_writes の値が 6 語・裁定 id user 2026-09-27T14:02Z 項 5・裁定日 2026-09-27 で、行数と kind の数は変わらない (6) 断り文は create-bypass が bdw create <題> --parent <epic> を、parent-edge が bdw update <子> --parent <親> を次の一手に持ち、埋め込みの rules の hook で bdw q x と bdw dep add a b --type parent-child が rc 2・stderr 1 行・stdout 0 byte・記録 1 行（what が ledger-deny と語）"

[[contract]]
id = "h"
title = "新しい崩れを増やす書きだけを起票の門が断る — create の --parent と --graph・update の --parent と --type・dep remove・数えに戻す reopen と update --status の 6 つの書きに台帳を 1 回読み、付け先が根に着かない・直下の open の子が上限を越える・親子の輪・根から外す・親の無い plan の node を直す 1 行つきで断る（減る向きの書きは通す・.beads の無い repo と掛からない command は台帳を読まない）"
req = ["FR20", "FR51", "NFR5", "NFR4"]
section = "12"
touches = ["crate::hook::ledger_guard::Create"]
depends = ["f", "g"]
write-set = ["+crates/scribe2/src/hook/graph_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_graph_guard_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_graph_guard_"]
growth = ["crates/scribe2/src/hook/mod.rs:10", "crates/scribe2/src/hook/ledger_guard.rs:40", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:200"]
size = "L"
done = "(1) 判定は兄弟 module に在り、起票の門の形で止まらなかった Bash の command だけを掛けて起票の門の判定の enum で返し、記録は ledger-deny と語で、極性一覧の行数は変わらない (2) 掛かる書きは create の --parent・create の --graph の file の node・update の --parent（空も）・update の --type と -t・dep remove と dep rm・reopen と update の --status と -s と --claim（closed でも pinned でもない状態へ）の 6 つで、どれも無い command と、hook の root が .beads の dir を持たない repo の command は台帳を 1 回も読まない (3) 掛かる segment が在る周は台帳を 1 回だけ読み（client は --bd か既定・待ち上限は rules 行 hook.budget_ms）、読めない周は ledger-unreadable・待ち上限を越えた周は ledger-timeout で断り、1 行の segment は通った segment の効きを足した写しで順に判定され、update A --parent B && update B --parent A の 2 つ目は parent-loop で断られる (4) 付け先が根に着かない書き（create と graph の parent_id と、今は根に着く X の update の付け先・update の空の付け先は除く）は parent-unrooted で top の id を、直下の open の子に epic でない open の bead を足すか数えに戻すと rules 行 ledger.open_children_max を越える書きは parent-full で親と子の数と上限を、update の付け先が自身か子孫なら parent-loop を、X / A が epic でない周の親を外す update と唯一の親の dep remove と、根の epic の型を epic 以外にする update は unrooting を（epic の親外しは通す）、parent_key をたどって parent_id を持つ node にも親の無い epic の node にも着かない plan の node（知らない key・輪を含む）は plan-orphan を、読めない plan の file は plan-unreadable を理由に断り、上限の行か hook.budget_ms の行が無い周は no-rule (5) closed と pinned の子は数えず、epic の create と epic の付け替えは溢れで断らず、根に着かない bead を根に着く親へ付け替える書きと型を epic にする書きは通る (6) 断り文は直す 1 行を持ち、parent-full は --type epic --parent <親> の子 epic の作り方を名指す (7) Create は --parent・--type・--graph の値を運び構築点は flags_of の 1 か所で、既存の起票の門の歯（.beads の無い toy repo）は変わらず緑"
[[contract]]
id = "i"
title = "台帳の形の 4 象限の母集団から label intake:question の bead（台帳の問い）を epic と decision の型と同じく外す — doctor の行の key と順は変えず、開いた問いを neither に名指さない（FR51・ADR-0083）"
req = ["FR51"]
section = "13"
write-set = ["crates/scribe2/src/ledger/form.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail quadrant_exempts_question_"]
growth = ["crates/scribe2/src/ledger/form.rs:30"]
size = "S"
done = "(1) form.rs の judge が 4 象限の母集団（shaped）を作るとき、型が epic か decision の bead に加えて label intake:question を持つ bead を外し、label の字は 1 つの定数に置く (2) doctor の台帳の形の行の key と順は変わらず、open= は closed でない bead の全部の件数のまま、shaped= と both= と neither= だけが問いの分だけ変わる (3) 台帳の lint（lint.rs）は変えない 歯: quadrant_exempts_question_（form.rs の既存の歯の区間に 1 本）が、open の問い 1（label intake:question・型 task・設計 pointer なし）・open の memo 1・設計 pointer を持つ open の契約 1・印の無い open の task 1 の台帳で open 4・shaped 3・neither が印の無い task の id だけ・問いの id がどの欄にも出ないことを測る。base の judge は問いを neither に数えるので RED（機能不在）"

[[contract]]
id = "j"
title = "台帳の問い（label intake:question）の create の形を起票の門が断る — 本文の 4 行・metadata の effect と asked の閉じた 2 値・label intake:memo の併せ持ち・--parent を名指して --no-inherit-labels の無い書き（判定は ledger/ の純関数・台帳を読まない）"
req = ["FR81", "FR89"]
section = "14"
touches = ["crate::hook::ledger_guard::Create"]
write-set = ["+crates/scribe2/src/ledger/question.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "=crates/scribe2/src/ledger/form.rs", "=crates/scribe2/src/fleet/json_tree.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_question_form_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_question_form_", "cargo nextest run -p scribe2 --lib --no-tests=fail ledger_question_form_"]
growth = ["crates/scribe2/src/ledger/question.rs:230", "crates/scribe2/src/ledger/mod.rs:1", "crates/scribe2/src/hook/ledger_guard.rs:90", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:170"]
size = "L"
done = "(1) 判定は ledger/ の新しい file の純関数で、本文の字と metadata の字と label の列だけを読み、4 行（行頭の空白と - を除いて 概要・技術・理由・推奨 の語で始まり語の直後が行末・空白・= ・:・： の行）・effect（文字列 document か operation）・asked（文字列 seat か user）・label intake:memo の併せ持ちを判じて宣言順で最初の欠けを返す (2) label intake:question を持つ bd / bdw の create は memo の判定の代わりに問いの段に掛かり、併せ持ち → 継がない指定（--parent の値が空でなく --no-inherit-labels も =true も無い）→ 本文を読めるか → 4 行 → metadata を読めるか → effect → asked の順で、question-memo-label・question-inherits-labels・question-body-unreadable・question-no-summary・question-no-technical・question-no-reason・question-no-recommendation・question-metadata-unreadable・question-no-effect・question-bad-effect・question-no-asked・question-bad-asked の 1 語で deny し、断り文は欠けた行・外れた値・併せ持つ label・継ぐ親の id を名指す (3) 本文は --body-file の file（payload の cwd から解く）と -d / --description の最後の値を合わせて読み、--stdin・値が - か無い --body-file・開けない file・$ か backtick を含む -d の値は読めず、metadata は --metadata の最後の値で @<path> は同じ cwd から読み、JSON の object でない・開けない周は question-metadata-unreadable (4) e2e: 埋め込みの manifest の写しで ledger.denied_writes から bd-outside-bdw を外した rules で、10 形（4 行の欠け 4・effect の欠けと値の外・asked の欠けと値の外・intake:memo の併せ持ち・継がない指定の欠け）× bd と bdw の 20 本がどれも rc 2・stderr 1 行・stdout 0 byte・記録 1 行（ledger-deny と語）で、揃った問いの create（本文が body-file と -d・metadata が @file の 3 形・--parent と --no-inherit-labels つき）は rc 0 で記録を残さず、label intake:question を持たない create と bd / bdw の segment を持たない command（中で create を撃つ script）は問いの語で断られない (5) Create は本文と metadata の値と --stdin・--no-inherit-labels の有無を運び、構築点は flags_of の 1 か所のまま、memo の判定と 6 形と極性一覧は変わらず既存の歯が緑 (6) 通った問いが台帳の lint と起動の列に出ないことは着地済みの歯 quadrant_exempts_question_from_the_shaped_population と pipe_dispatch_intake_label_question_is_not_a_candidate が緑のまま持つ"

[[contract]]
id = "k"
title = "memo の引き金の行の読み手 1 本（§15 の文法・純関数・起票の門と dispatch の周と dispatch ls と局面の関数が引く）と、読める引き金の無い memo の create と昇格条件を引き金の無い本文へ書き換える update を起票の門が断る（台帳を読まない）"
req = ["FR81", "FR87"]
section = "15"
depends = ["j"]
write-set = ["+crates/scribe2/src/ledger/trigger.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "=crates/scribe2/src/seat/brief/pointer.rs", "=crates/scribe2/src/pipe/table/parse.rs", "=crates/scribe2/src/fleet/wait.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_memo_trigger_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_memo_trigger_", "cargo nextest run -p scribe2 --lib --no-tests=fail ledger_trigger_"]
growth = ["crates/scribe2/src/ledger/trigger.rs:330", "crates/scribe2/src/ledger/mod.rs:1", "crates/scribe2/src/ledger/form.rs:20", "crates/scribe2/src/hook/ledger_guard.rs:80", "crates/scribe2-boundary/tests/e2e/hook.rs:1", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:170"]
size = "L"
done = "(1) 引き金の行の読み手は ledger/ の新しい file の純関数 1 本で、入力は description と notes と台帳の接頭辞だけ（I/O と時計を持たない）、§15 の文法（### 昇格条件 の節の中だけ・行頭の - を許す 引き金: の行・空白で割った 1 語目が形で 2 語目が値で後ろは読まない・5 形の値の形・notes の [再発] と [keep] の行頭）で、行ごとに読める引き金（形と値）か読めない引き金（行の字と理由）と再発の行の本数と keep の記帳を返す (2) 同じ台帳の bead id の形の判定は ledger/form.rs の 1 関数で、門は payload の cwd から上へ辿った最初の .beads の dir を持つ dir の設定から接頭辞を解き、解けない周は依存の値が読めない (3) 4 節が揃い memo の判定で止まらない memo の create は、昇格条件の節に読める引き金の行が 1 本も無ければ no-trigger で deny され、断り文は 5 形の字面と最初の読めない行を名指す (4) bd / bdw の update が --body-file か -d / --description で書く本文が ### 昇格条件 の見出しを持ち読める引き金の行が無ければ update-no-trigger、--stdin・値が - か無い --body-file・開けない file・$ か backtick を含む -d の値は update-body-unreadable で deny され、見出しを持たない本文の update は通る (5) e2e: j と同じ写しの rules で、引き金の行が散文だけの memo の create と引き金の無い昇格条件へ書き換える update の 2 形 × bd と bdw の 4 本がどれも rc 2・stderr 1 行・stdout 0 byte・記録 1 行で、再発 1 の引き金を持つ memo の create と見出しの無い本文の update は rc 0、.beads の設定の接頭辞の依存は読めて別の接頭辞の依存だけの memo は no-trigger (6) 既存の memo の create の e2e の本文（tests/e2e/hook.rs の MEMO_BODY）が引き金の行を 1 本持ち、hook_memo_guard_ の歯が緑のまま、台帳を 1 度も読まない"

[[contract]]
id = "l"
title = "close の理由の読み手 1 本（§16 の 9 つの頭と値の形・裁定 id の 3 形・純関数）と、vessel 宣言の任意 key close-check の読み手（真偽だけ・HEAD の宣言から加わる・加わらない・読めないの閉じた 3 値）を足す（門の判定と本 repo の宣言は変えない）"
req = ["FR81", "FR91"]
section = "16"
depends = ["k"]
write-set = ["+crates/scribe2/src/ledger/close_reason.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/entrance_flip.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail ledger_close_reason_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_close_check_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_entrance_"]
growth = ["crates/scribe2/src/ledger/close_reason.rs:350", "crates/scribe2/src/ledger/mod.rs:1", "crates/scribe2/src/pipe/declaration.rs:8", "crates/scribe2/src/pipe/declaration/entrance_flip.rs:1", "crates/scribe2/src/pipe/declaration/optional_keys.rs:80"]
size = "M"
done = "(1) close の理由の読み手は ledger/ の新しい file の純関数 1 本で、入力は理由の字と台帳の接頭辞だけ（I/O と時計を持たない）、前後の空白を除き Unicode の空白で割った 1 語目を 9 つの頭と完全一致で照合し（大文字小文字を区別）、landed は 40 桁の 16 進の着地 commit id と尾の閉じた 3 値（ci=success・ci=success tip=<40 桁>・ci=none）か読めない尾、重複と後継とまとめたは同じ台帳の bead id（is_bead_id）、取り下げは空白でない字を 1 つ以上持つ頭の後ろの全部、裁定と見送りは裁定 id、昇格済みは頭の後ろの語の全部が bead id で 1 つ以上、完了は値なしで読み、形か閉じた欠陥（空・頭の外・値の形の外・接頭辞が無い）を返し、裁定 id の 3 形（<問い id>:<YYYYMMDDTHHMMZ>-<n> は n が 1 以上で時刻を epoch_of で読める・batch:<字>・policy:<字>）の判定を 1 関数で外へ見せる (2) 宣言の値の生の型が真偽を持ち、manifest の読みが返す真偽をそのまま受け、entrance-flip の読みは真偽を閉じた 3 語の外として不備にし（entrance-flip = true は key と行番号を名指す不備）、真偽を書いた他の key は key ごとの型の不備になる (3) 任意 key close-check は真偽だけを受け（文字列・整数・列・重複は key と行番号を名指す不備）、宣言の key の列と任意 key の列の末尾に 1 つずつ載り、HEAD の宣言の読み手 1 本から加わる（true）・加わらない（false・key が無い・宣言 file が HEAD に無い・git を撃てない）・読めない（宣言が在って不備）の閉じた 3 値を返す口が親から外へ見える (4) 起票の門の判定と本 repo の .vessel.toml は変わらない (5) lib: close の理由の読み手の in-file の歯が 9 つの頭の読める値と読めない値・Landed と 昇格済み: ・全角の空白・取り下げ だけ・, を含む列・裁定 id の 3 形と n が 0・月 13・桁の欠け・landed の尾の 3 形と読めない尾・大文字の 16 進・接頭辞の無い周を、optional_keys.rs の in-file の歯が close-check の true と false と無い宣言・文字列と整数と列と重複の不備・3 値の写し・remote = true の不備・entrance-flip = true の 3 語の外の不備を測って緑、declaration.rs の宣言の key の列の歯が close-check を末尾に持って緑"

[[contract]]
id = "l1"
title = "起票の門の close の段 — vessel 宣言が close-check を true で持つ repo と宣言が在って読めない repo でだけ、理由を持つ close の口（close・done・gate resolve）の理由の無い close・和の外の理由・着地の形の理由を断る（--reason-file を読む・台帳を読まない）"
req = ["FR81", "FR91"]
section = "16"
depends = ["l"]
write-set = ["crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_close_reason_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_close_reason_"]
growth = ["crates/scribe2/src/hook/ledger_guard.rs:140", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:210"]
size = "M"
done = "(1) close の段は close の segment を持つ command の周にだけ、payload の cwd から上へ辿った最初の .beads の dir を持つ dir の HEAD の宣言を 3 値の口 1 本で読み、加わる周と読めない周に掛かり、加わらない周（false・key が無い・宣言 file が無い・git を撃てない・.beads を持つ祖先が無い）は段を撃たずに通す (2) 起票の門が理由を持つ close に数える書きは bd / bdw の close と done と gate resolve で、-r と --reason の値（= の形も）と --reason-file の file（payload の cwd から解く・前後の空白を除く）の字を出てきた順に全部読み手に掛け、最初に外れた理由の語で断る (3) 理由の値を 1 つも持たないか空の close は close-no-reason、頭が landed の理由は値の形に依らず close-landed、頭の外・値の形の外・接頭辞が解けず id を読めない理由は close-outside-forms、--reason-file が - か値が無いか開けない周と $ か backtick を含む理由の値は close-reason-unreadable、宣言を読めない repo でこの 4 つに当たった周は close-declaration-unreadable で deny され、断り文は deny bd close は起票の門が止める reason=<語> の頭と ledger-form.md §16 を持つ 1 行で、close-landed は pipe land --run <run> --terminal-only と pipe retire --run <run> と settle を、close-no-reason と close-outside-forms は landed を除く 8 つの頭の字面を、close-declaration-unreadable は当たった形の語と宣言の直し方を名指す (4) 段は 6 形と update の引き金の段の後で、segment の読みは write_of のまま Write・Create・Refusal は変わらず、台帳を 1 度も読まない (5) e2e: ask_place で close-check = true を commit し .beads/config.yaml に接頭辞 toy を持つ加わる repo と、同じ形の false・key 無し・文字列 yes の repo と、git_repo と linked の後に git rm .vessel.toml を commit した宣言の無い repo と、close-check = true を commit して .beads を持たない repo と、key の無い宣言を commit して作業ツリーにだけ close-check = true を書いた repo と、j と同じ写しの rules（guards.rs の question_rules が rules/manifest.toml を include_str で読んだ QUESTION_EMBEDDED から bd-outside-bdw を外した写し。bd の直の close も 6 形の語に落ちずに close の段に届く）と偽の client で、加わる repo の理由の無い close・和の外の理由の close・着地の形の理由の close の 3 形 × bd と bdw の 6 本がどれも rc 2・stderr 1 行・stdout 0 byte・記録 1 行で、landed を除く頭を種類との組 10 形で書いた close × bd と bdw の 20 本が rc 0、-r・--reason=・--reason-file・done・gate resolve の理由が読まれ、2 つの --reason の 2 つ目だけが形の外の周・別の接頭辞の重複・, で繋いだ昇格済み・n が 0 の裁定 id・Landed は close-outside-forms、--reason-file の - と値なしと無い file と $( を含む理由は close-reason-unreadable、false の repo・key 無しの repo・宣言の無い repo・.beads の無い repo・作業ツリーだけの repo では 6 本がどれも rc 0 で記録なし、close-check の値が文字列 yes の repo では理由の無い close が close-declaration-unreadable（close-no-reason を名指す）で 取り下げ x の close は rc 0、close を中で撃つ script を起こす command は rc 0 で、偽の client は 1 回も起きない (6) lib: ledger_guard.rs の in-file の歯が 3 値ごとの掛け方と理由の集め方（-r・-r=・--reason=・値の無い --reason・$ と backtick・done・gate resolve・2 つの理由の順）を測って緑"

[[contract]]
id = "l2"
title = "理由を持てない close の口 — close-check を宣言した repo と宣言を読めない repo で、update の --status closed と bd が理由を書く duplicate・supersede・epic close-eligible を起票の門が断り、doctor の台帳のグラフの行の直す形を理由つきの close にする（台帳を読まない）"
req = ["FR81", "FR91"]
section = "16"
depends = ["l1"]
write-set = ["crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "crates/scribe2/src/ledger/graph.rs", "crates/scribe2-boundary/tests/e2e/ledger_form.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_graph_doctor_external_form.snap", "=crates/scribe2-boundary/src/main.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_close_mouth_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_close_mouth_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail ledger_graph_", "cargo nextest run -p scribe2 --lib --no-tests=fail ledger_graph_", "cargo nextest run -p scribe2-boundary --bin scribe2 --no-tests=fail ledger_graph_doctor_external_form"]
growth = ["crates/scribe2/src/hook/ledger_guard.rs:60", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:90", "crates/scribe2/src/ledger/graph.rs:1", "crates/scribe2-boundary/tests/e2e/ledger_form.rs:1"]
size = "M"
done = "(1) 行 l1 と同じ掛け方で掛かる repo の bd / bdw の update の --status・-s・--status=・-s= の値 closed は status-closed で deny され、断り文は bdw close <id> --reason を次の一手に持つ (2) 同じ repo の duplicate・supersede・epic close-eligible（--dry-run を持つ周を除く）は implicit-reason で deny され、断り文は 重複 <id>・後継 <id>・完了 の理由の close を次の一手に持ち、どちらも行 l1 と同じ deny bd close の頭と ledger-form.md §16 の 1 行（宣言を読めない repo は close-declaration-unreadable で当たった語を名指す）で、台帳の形の門より前に断り台帳を読まない (3) doctor の台帳のグラフの行の直す形の close-eligible の句が bdw close <epic> --reason 完了 になり、行の key と順は変わらない (4) e2e: 行 l1 の toy repo と rules と偽の client で、加わる repo の --status closed の 4 つの綴りと duplicate・supersede・epic close-eligible の 7 形 × bd と bdw の 14 本がどれも rc 2・stderr 1 行・stdout 0 byte・記録 1 行で、同じ repo の update の --status pinned・epic close-eligible --dry-run・epic status は rc 0（どれも台帳の形の門が台帳を読まない書き）、宣言の無い repo では 14 本がどれも rc 0 (5) lib: ledger_guard.rs の in-file の歯が --status の 4 つの綴り・--dry-run・epic status を測って緑、tests/e2e/ledger_form.rs の FIX と ledger_graph_doctor_external_form の snapshot が新しい直す形に直って緑"

[[contract]]
id = "l3"
title = "本 repo が close の理由の門に加わる — .vessel.toml に close-check = true を足し、CLAUDE.md の作業の流れと done の見出しと .beads/PRIME.md の close の字を今の口の名に直す（本 repo を対象にする全部の host・全部の置き場の binary を行 l の読み手と行 z の権能を持つ binary に入れ替えた後）"
req = ["FR81", "FR91"]
section = "16"
depends = ["l2"]
write-set = [".vessel.toml", "CLAUDE.md", ".beads/PRIME.md", "crates/scribe2/src/pipe/declaration/optional_keys.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_close_check_own_"]
growth = ["crates/scribe2/src/pipe/declaration/optional_keys.rs:10"]
size = "S"
done = "(1) 本 repo の .vessel.toml が close-check = true を持ち、注記の行が ledger-form.md §16 と ADR-0097 を指し、宣言は今の読み手で読める (2) CLAUDE.md の作業の流れの 5 が契約の close を器の land の終端に、止まった終端の閉じを orchestrator の名指しの 2 形（pipe land --run <run> --terminal-only・pipe retire --run <run>）に結び、done の定義の見出しが close でなく着地を止める字になり、.beads/PRIME.md の前提と R0 と Essential Commands の close の字が §16 の形と bd help の読みになる (3) lib: optional_keys.rs の in-file の歯が本 repo の .vessel.toml を読み close-check が true であることを測って緑"
<!-- contracts:end -->
