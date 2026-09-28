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
8. **memo の入口は器の口 1 つ**（これから起きる memo の形を起票の時点で決める・回り続ける周のため）: 器の read-only の口が memo の plan（bd の create の引数と本文の 4 節）を標準出力に出し、席が bdw で撃つ。出所は 2 つの形だけ: (a) **便の終端から**（`--run <id>`）— 便の終端の event（gate / 審査の FAIL・INCONCLUSIVE・Failed・Questioned）を読み、`### 出所` に run id と段と kind を、`### 観測` に**終端の種類ごとの原本**を器が写す（人が写さない・C10）。原本は 4 形で閉じる: gate の FAIL・INCONCLUSIVE = run dir の `verdict.json` の evidence と at／審査の FAIL・INCONCLUSIVE = run dir の `review.json` の evidence と at／Questioned = event log の質問の逐語と `about`（既存の読み手 `crates/scribe2/src/pipe/mod.rs` の `Question`・FR31・run dir に file は無い）／Failed = event log の `RunStage(Failed)` の detail（閉じた理由の字面）と ts（verdict は無い）。どの形にも当たらない終端（detail が空・log を読めない）は写さず閉じた理由で断る（fail-closed）。`### 候補` と `### 昇格条件` は空の見出しで出し、席が埋める。(b) **user の要望から**（`--from user`）— `### 出所` に「user 逐語は本 bead の notes」の 1 行と日付を置き、席が逐語を notes に写す。label `intake:memo`・parent の epic（引数）・関連 bead（引数の列 → `relates-to`）も plan に載る。器は台帳へ書かない（ADR-0045 §2）。
9. **起票の門は guard**（in-loop・fail-closed・極性一覧に 1 つ増える〔ADR-0014 §2.1〕）: PreToolUse の hook が `bd create` / bdw の create の command を読み、label に `intake:memo` を持つか title に `[memo]` を持つ周は、`--body-file` の本文に memo の 4 節の見出しが全部在ることを要求し、無ければ閉じた理由 1 つで止める（散文の免除なし）。契約の bead（acceptance に pointer 行）の create は label `intake:memo` を持たないことを要求する（4 象限の違反 2 形の 1 つを起票の時点で塞ぐ）。読めない command（body-file が無い・開けない）は止める側に倒す。
10. **memo の close は器の着地の終端が行う**（書きの口は既存の close の 1 種・増えない）: 便が Landed で契約を close した周、その契約から `discovered-from` で辿れる memo のうち、辿れる契約が全部 closed になった memo を同じ終端が close する（理由の字面は着地の sha を持つ）。辿れない周・台帳が読めない周は close せず、台帳 lint の (viii)「辿れる契約が全部 closed の open な memo」が名指す（fail-closed・close は写しの操作ではないので器が持てる）。

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
<!-- contracts:end -->
