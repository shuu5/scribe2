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
5. **台帳は契約表から生成する**: xtask の口が全設計 doc の契約表を読み、**未着地の行**（write-set の `+` の file が tracked に無い行・§33 の `symbols` の `+` も同じ読み）ごとに bead の plan（title = 行の title・acceptance = pointer 行・parent = 口の引数 1 つで渡す epic id〔本 repo は program の epic 1 本・doc ごとの対応表は持たない〕・edge = 行の `depends` → `blocks`・§ の本文が名指す bead の id（台帳の id の字面・memo か否かは問わない＝xtask は台帳を読まない）→ `discovered-from`・label = doc 名の `doc:` と size の `size:`）を `bd create --graph` の JSON で標準出力に出す。**apply は席の手番**（bdw で撃つ・器は台帳を書かない）。xtask は台帳を読まない（CI は台帳に届かない・plan は行だけから出す）。台帳側の drift は doctor の lint の (vii)（上の 4）が測る。id は bd の採番なので、行 ↔ bead の対応は acceptance の pointer 行で引く（新しい key を作らない）。
6. **棚上げは `--defer`**（label や notes の「棚上げ」の語を規則にしない）。`bd ready` が隠すので列の観測（dispatcher.md §6）と整合する。
7. **見積は `--estimate`（分）**を size から写す（S / M / L の分の値は便の実測の中央値で、値の正本は rules 行〔後続・C5〕）。Jev の較正の材料（見積と実測の差）はここから取る。
8. **memo の入口は器の口 1 つ**（これから起きる memo の形を起票の時点で決める・回り続ける周のため）: 器の read-only の口が memo の plan（bd の create の引数と本文の 4 節）を標準出力に出し、席が bdw で撃つ。出所は 2 つの形だけ: (a) **便の終端から**（`--run <id>`）— 便の終端の event（gate / 審査の FAIL・INCONCLUSIVE・Failed・Questioned）を読み、`### 出所` に run id と段と kind を、`### 観測` に verdict の evidence と at を器が写す（人が写さない・C10）。`### 候補` と `### 昇格条件` は空の見出しで出し、席が埋める。(b) **user の要望から**（`--from user`）— `### 出所` に「user 逐語は本 bead の notes」の 1 行と日付を置き、席が逐語を notes に写す。label `intake:memo`・parent の epic（引数）・関連 bead（引数の列 → `relates-to`）も plan に載る。器は台帳へ書かない（ADR-0045 §2）。
9. **起票の門は guard**（in-loop・fail-closed・極性一覧に 1 つ増える〔ADR-0014 §2.1〕）: PreToolUse の hook が `bd create` / bdw の create の command を読み、label に `intake:memo` を持つか title に `[memo]` を持つ周は、`--body-file` の本文に memo の 4 節の見出しが全部在ることを要求し、無ければ閉じた理由 1 つで止める（散文の免除なし）。契約の bead（acceptance に pointer 行）の create は label `intake:memo` を持たないことを要求する（4 象限の違反 2 形の 1 つを起票の時点で塞ぐ）。読めない command（body-file が無い・開けない）は止める側に倒す。
10. **memo の close は器の着地の終端が行う**（書きの口は既存の close の 1 種・増えない）: 便が Landed で契約を close した周、その契約から `discovered-from` で辿れる memo のうち、辿れる契約が全部 closed になった memo を同じ終端が close する（理由の字面は着地の sha を持つ）。辿れない周・台帳が読めない周は close せず、台帳 lint の (viii)「辿れる契約が全部 closed の open な memo」が名指す（fail-closed・close は写しの操作ではないので器が持てる）。

## 4. やさしく言うと

memo か契約かを「label が在るか」と「受入条件に設計の 1 行が在るか」の 2 つだけで決め、それ以外の組み合わせは全部「壊れている」と数える。memo の本文は 4 つの見出しを必ず持ち、無ければ doctor が名指す。契約の bead は設計書の表から機械が起こす案を出し、人はそれを撃つだけにする。

## 5. 触らない

- bd の version と `types.custom`（型を足さない）・`bd remember` 系（使わない・PRIME）・台帳の書きの口（`close` の 1 種のまま）・行 e の 3 欠陥の判定と字面・PRIME の R0〜R6（R3 は label のまま・本 doc の節 2 の見出しは PRIME に写す＝docs PR）・rules 行（見積の値は後続）。

## 6. 歯

- 行 a（`ledger_form_` 接頭辞・判定は行 a の write-set の `+` の file に純関数で・doctor の行は既存の口に 1 行）: (a) 4 節の欠けを**件数を違えて**持つ fixture（出所なし 1・観測なし 2・候補なし 0・昇格条件なし 3）で件数と母集団と id が出る／(b) 4 象限の違反 2 形が別々に数えられ、epic と裁定の bead が母集団から外れる／(c) `discovered-from` の無い契約が § の本文の名指しと bead の本文の名指しの両方から数えられ、どちらにも名指しの無い契約は数えられない／(d) 読めない周は測れていない形の行（件数 0 でない）／(e) 外形 snapshot（新しい doctor の 1 行）／(f) drift (vii): 未着地の行 2 本のうち pointer を持つ open の bead が 1 本だけの fixture で 1 と母集団 2 が出る（契約表は toy repo の doc・tracked の集合は toy repo の index・台帳は偽の client）。
- 行 c（`ledger_memo_plan_` 接頭辞・置き場は行 c の write-set の `+` の歯の file）: (a) 便の終端の run dir（verdict.json / review.json / 問いの record）から `### 出所` と `### 観測` が写り、`### 候補` と `### 昇格条件` が空の見出しで出る（終端でない run は断る）／(b) `--from user` の plan が出所の 1 行と日付を持ち観測が空／(c) label と parent と `relates-to` の引数が plan に載る／(d) 出力は標準出力だけで台帳に 1 件も書かない（偽の bd が呼ばれない）／(e) usage の外形 snapshot。
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

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "台帳 lint の項目 5 つ — memo の 4 節の欠け・field の 4 象限の違反 2 形・§ か本文が名指す memo への discovered-from の無い契約・契約表の未着地の行と台帳の drift・辿れる契約が全部 closed の open な memo を、件数と母集団と id で doctor の 1 行に出す"
req = ["FR51"]
section = "3"
write-set = ["+crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap", "+crates/scribe2/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "+crates/scribe2/tests/e2e/ledger_form.rs", "crates/scribe2/tests/e2e/main.rs"]
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
write-set = ["+crates/scribe2/src/ledger/memo.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/main.rs", "+crates/scribe2/tests/e2e/ledger_memo.rs", "crates/scribe2/tests/e2e/main.rs", "+crates/scribe2/tests/e2e/snapshots/e2e__ledger_memo__ledger_memo_plan_usage_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail ledger_memo_plan_"]
size = "M"
done = "(1) 終端の run（Gated / Reviewed の FAIL・INCONCLUSIVE・Failed・Questioned）の run dir から、### 出所 に run id と段と kind が、### 観測 に verdict の evidence と at が写った plan が出て、### 候補 と ### 昇格条件 は空の見出しで出る (2) 終端でない run と run dir の無い id は閉じた理由で断られ rc 1 (3) --from user の plan は ### 出所 に逐語の在り処の 1 行と日付を持ち ### 観測 が空 (4) label intake:memo・引数の parent・引数の関連 bead への relates-to が plan に載る (5) 出力は標準出力だけで、偽の bd を PATH に置いても 1 回も呼ばれない (6) usage の 1 枚の外形 snapshot"

[[contract]]
id = "d"
title = "起票の門 — PreToolUse の hook が memo の create に 4 節の本文を要求し、契約の create に intake:memo が無いことを要求する guard（in-loop・fail-closed・極性一覧に 1 つ増える）"
req = ["FR20", "FR51"]
section = "3"
write-set = ["+crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail hook_memo_guard_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_summary_counts_match_lines", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_all_is_in_declaration_order"]
size = "M"
done = "(1) [memo] の title か intake:memo の label を持つ bd / bdw の create は body-file の本文に memo の 4 節の見出しが全部在れば通り、1 つでも欠ければ閉じた理由 1 つで deny される (2) acceptance に設計 pointer 行を持つ create が intake:memo を持てば deny される (3) body-file が無い・開けない周は deny に倒れる (4) memo でも契約でもない create（epic・裁定）と create 以外の bd の command は 1 字も変わらず通る (5) 極性一覧の外形 snapshot に guard が 1 つ増え、guard の総数を pin する歯が新しい母集団で緑"
<!-- contracts:end -->
