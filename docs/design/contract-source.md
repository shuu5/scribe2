# 設計: 契約の正本と席の作業の境界 — 契約は設計 doc の契約表 1 行、器が契約 file を生成し、審査の段を通してから実装役を起こし、着地の終端（push・CI・台帳の close）まで器が持つ

- 要件: [FR47](../../design-intent/spec/srs.html#FR47) 契約の正本 / [FR53](../../design-intent/spec/srs.html#FR53) 契約 file の生成 / [FR54](../../design-intent/spec/srs.html#FR54) 設計 pointer の拒否 / [FR48](../../design-intent/spec/srs.html#FR48) write-set の閉包 / [FR55](../../design-intent/spec/srs.html#FR55) 契約表の検査 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査の段 / [FR50](../../design-intent/spec/srs.html#FR50) land の終端 / [FR51](../../design-intent/spec/srs.html#FR51) 台帳の lint / [AC21](../../design-intent/spec/srs.html#AC21) [AC22](../../design-intent/spec/srs.html#AC22) [AC23](../../design-intent/spec/srs.html#AC23) [AC24](../../design-intent/spec/srs.html#AC24) [AC26](../../design-intent/spec/srs.html#AC26)・既存 [FR2](../../design-intent/spec/srs.html#FR2) / [FR4](../../design-intent/spec/srs.html#FR4) / [FR9](../../design-intent/spec/srs.html#FR9) / [FR12](../../design-intent/spec/srs.html#FR12) / [FR31](../../design-intent/spec/srs.html#FR31) / [FR32](../../design-intent/spec/srs.html#FR32) / [FR39](../../design-intent/spec/srs.html#FR39) / [NFR1](../../design-intent/spec/srs.html#NFR1) / [NFR4](../../design-intent/spec/srs.html#NFR4)
- 決定: [ADR-0023](../../design-intent/decisions/ADR-0023-contract-source-is-the-design-document-table.html)（本 doc の決定の正本・§2.1〜§2.8）/ [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.3（契約 file = TOML subset・本 doc では生成物）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.2（閉じた enum の充足形・字面走査）/ [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html) §2.1（intake の排他）/ [ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html) §2.6（record の任意 field）
- 土台: [pipeline.md](./pipeline.md) §3（契約 file）/ §5.1（intake）/ §5.4（land）・[pipeline-question.md](./pipeline-question.md)（質問と回答）・[pipeline-conflict.md](./pipeline-conflict.md) §3（起こし直し）・[gate-cost.md](./gate-cost.md) §6（着地の順序）・[rules-manifest.md](./rules-manifest.md)（TOML subset の parser）・[fleet-event-log.md](./fleet-event-log.md)・[seat-roles.md](./seat-roles.md)（権能・本 doc はその外側）
- 位置づけ: 席の産物（契約）と手順（写し・回答の適用・着地の終端・台帳）のうち**器が持つ範囲**を決める。権能（seat-roles.md）は「誰がしてよいか」、本 doc は「器が代わりに持つもの」。裁定 = user 2026-09-13（Q1〜Q3・台帳 s2-07l.197 notes に逐語）。

## 1. 何を解くか

便の一周（要望 → 要件 → 設計 → 契約 → 受付 → 実装 → gate → land → 終端 → 台帳）のうち、器が構造で持つのは受付の排他（FR39）・実装役の write-set guard（FR20）・gate（FR8 / FR9）・land の CAS と順序（FR10〜12・gate-cost.md §6）だけで、残りは席の判断・散文・手写しである（2026-09-13 の実測: 15 手順中 5 つ）。特に契約は **3 段の写し**（設計 doc の契約の割り〔散文〕→ 台帳の acceptance〔散文〕→ 契約 file〔管理席の手写し〕）で、各段で write-set の落ち（5 例）と設計の inline（gate で初めて見つかる穴 2 つ）が起きた。台帳には設計本文を持つ memo が 40 本溜まる（設計 doc に置き場が無い）。

本設計は (1) 契約の正本を設計 doc の**契約表**（機械が読む 1 区間）に置き、器が契約 file を**生成**する (2) 契約表を CI と受付が**検査**する（閉包・実在・形）(3) 実装役を起こす前に**審査の段**を置く (4) land の**終端**（push・CI の照合・台帳の close）を器が持つ (5) 台帳を doctor の項目で lint する。planner の産物に門が付き、管理席の手写しが消える。

## 2. 契約表（FR47）

- **本文の形**: **TOML subset の `[[contract]]` の表**（rules manifest と同じ parser・ADR-0004 §2.3。現物の parser は `[[rule]]` / `[[account]]` の 2 種だけを受け**空の配列を拒む**ので、array-of-tables の種類に `[[contract]]` を 1 つ足し〔C2・variant 1 つ〕、**空の列は key を省いて表す**〔`touches` / `depends` が無い行 = 空・空配列の拒否は緩めない〕）。**置き場は 2 形を同じ読み手で受ける**: (i) 記録時点 = 設計 doc（`docs/design/<題>.md`）の末尾の機械が読む区間 `

<!-- contracts:begin -->` … `
<!-- contracts:end -->`（CLAUDE.md の憲法区間と同じ marker 形・行走査で区間を抜いて同じ parser に渡す・設計 doc 1 本に区間は 0 か 1 つ）(ii) 後続 = folio2 が設計ノート（YAML 正本）から導出する tracked な `.toml` 1 file（全文を同じ parser に渡す・folio2 planner との擦り合わせ 2026-09-13・scribe2 側の受け口は §47＝行が運ぶ節の本文の逐語 goal）。読み手は path の拡張子（`.md` = 区間 / `.toml` = 全文）で形を決め、それ以外は typed に断る。
- **契約 id** = `<doc id>#<row id>`（doc id = file 名の stem・row id = 行の `id`・folio2 の設計ノートと同じ形）。doc id は **append-only**（file を改名しても id は変えない＝改名は新 id + 旧 id の廃止・folio2 へ移すとき設計ノートの meta.id に同じ文字列を写す）。
- **行の field**（**正本は core の型** = `pipe/table.rs` の const・`<NAME> contracts schema` が tracked な生成物 `contracts/schema.toml` へ描き〔hooks.json / 極性一覧と同型・xtask check が render と tracked の差分 0 を測る〕・本節はその pointer・folio2 M1 はその file を「外部 schema 参照型」として読む＝欄の追加は scribe2 の版上げで folio2 の ADR は要らない。現物の契約 file の REQUIRED 9 欄との共通は 5 欄〔req / write-set / verify / size / done〕で 1:1 ではない）: `id`（doc 内で一意・`a` `b` …）/ `title` / `req`（要件 id の列）/ `section`（本 doc の節 anchor・審査の材料 `{design}` がこの節の本文を読む＝説明文を二重に書かない）/ `touches`（閉じた型の宣言の列・`crate::fleet::Stage` の形・空可・§3）/ `write-set`（path の列）/ `verify`（positional filter 形の列・`(` を含まない）/ `size` / `done`（1 行）/ `depends`（同 doc の契約 id の列・順序・床が解決と輪の無さを数える）/ `classes`（optional・既存）/ `opens`（optional・[seat-roles.md](./seat-roles.md) 契約 (b) が足す印・(b) の land までは未知 key として断る）。**散文の欄は `title` と `done` の 2 つだけ**（他は id / path / 型名 / 命令の識別子・folio2 の床〔語彙に無い裸の英字語 0〕はこの 2 欄に掛かる・括弧の中は免除。数 + 単位の検査は散文一般には掛けず「規範の印を持つ文」にだけ〔§12〕）。`section` は同じ doc の **節番号（`§N` の N・append-only・見出しの字面ではない）** で、`contracts check` は `## N.` の見出しが在り本文が非空であることを見る。`goal` は**行の `title`** である（planner 裁定 2026-09-19・`.209`）: 生成は title を `goal` に写し、節の本文は写さない。理由は実測（`docs/design/*.md` の節 276 のうち **41 が `"` を含み**、最大の節は **27,044 byte**）——契約 file の値は 1 行 1 key の TOML subset で escape を持たないので、節の本文は 15% の節で**表現できず**、書けても 27 KB の 1 行になる。審査の材料 `{design}` が行の `section` から節の本文をそのまま読むので、`goal` が title でも審査の材料は薄まらない。`schema = 1` は両形とも先頭に置く（rules manifest と同じ parser の前提）。**契約表の意味検査（id / req / section / depends / 閉包）は scribe2 の 1 関数（編集時・fail-closed）だけが持つ**。folio2 が持つのは器・導出・導出物の差分 0（post の drift 検出・C16 の代替ではない）。`owner` / `disposition` は生成時に固定値（現物の contract.rs が要求する field を埋める）。
- **台帳の bead**: title・status・裁定（notes）・acceptance は `design = docs/design/<題>.md#<id>` の **1 行だけ**。契約の改訂 = 設計 doc の改訂（PR・folio と CI の門を通る）。台帳の acceptance に本文を書く形は §9 (a) の land 後に止める（FR51 の lint が名指す）。
- **生成**: `<NAME> pipe intake --design docs/design/<題>.md#<id> --bead <bead id> --repo R [--rules PATH]`。器は base（`--repo` の HEAD）の設計 doc から区間を読み、行 1 つを契約 file（run dir の `contract.toml`・field は現物の REQUIRED + `design` = pointer + `touches`）へ写す。**`--contract PATH` は廃止**（手書きの契約 file を受け付けない・FR47）。歯の toy repo は設計 doc の fixture を持つ。
- **表の検査**（`<NAME> contracts check --repo R`・CI の 1 job・xtask check は core に依存しないので撃たない）: 全 tracked 設計 doc の区間を parse し、id の一意・`req` の id が対象 repo の要件面（宣言 `requirements`＝`.vessel.toml` の任意 key・宣言の parser の閉じた key 列に足す〔`declaration.rs`〕・無ければ既定 path・拡張子で読み手分岐）に実在・`section` が同 doc に実在し本文が非空・`verify` の形・`depends` の解決・`touches` の閉包 ⊆ `write-set`（§3）・§3 の拡張のうち静的な 3 つ（`surfaces` の外形 pin・write-set 項目の実在と dir の展開・名指しの実在）を全件・行番号付きで出す（FR18 と同じ「全件・黙って落とさない」）。intake は同じ関数を 1 行に対して撃ち、加えて受付時点の事実である**上限の余地**（§3）を撃つ（1 実装・C2・余地は CI では撃たない＝§3「撃つ場所は受付だけ」）。

## 3. write-set の閉包（FR48）

- **入力**: `touches` の各型（`crate::fleet::Stage` の形）と base の tree。
- **導出**（字面走査・`pipe/closure.rs`・pure 関数・I/O は呼び手）: core と歯の `.rs` を全部読み、型ごとに次を持つ file を集める。(i) **literal 構築** = `Type {`（struct・`pub struct` の宣言行は除く）(ii) **match の arm** = `Type::` を `=>` の左に持つ行（網羅 match の面・`as_str` / `parse` / 段の分岐）(iii) **件数 pin** = その型の const slice（enum-slices の対応: `pub const NAME: &[Type]`）の `NAME.len()` を持つ行 (iv) const slice の宣言 file。読めない file・区間の parse 失敗は違反（fail-closed・NFR4）。
- **判定**: 閉包 ⊄ write-set なら `Refuse::WriteSetIncomplete { run, missing: Vec<path> }`（typed・足りない file を全部名指す・FR39 の `WriteSetOverlap` の隣）。
- **限界（残す側）**: 字面走査は「型の名が別名で現れる形（`use … as`・generic の中）」を見ない＝閉包の**下界**。上界を求めるには構文木が要り A3 の依存になる（却下・§11）。見落とした構築点は実装役の質問（FR31）で出て planner が設計 doc を直す（§7）＝運用ではなく質問 record と PR に落ちる。
- CI（§2 の `contracts check`）と受付が同じ関数を撃つ。
- **外形 pin（第 5 形・user 裁定 2026-09-14）**: 契約表の行が `surfaces`（任意・値 = 外形の名の列。記録時点の名は外形 snapshot の file 名の stem〔`doctor_external_form` / `seat_external_form` / `fleet_external_form` / `pipe_external_form`〕と usage 行を持つ subcommand の名）を宣言したら、閉包に (v) **その外形を pin する file** = 外形 snapshot の file と、その snapshot 名か subcommand の usage 文字列を literal に持つ歯の file（`tests/e2e/*.rs`・in-file の `#[cfg(test)]` を含む src）を足す。宣言の無い行は (v) を持たない（外形を触らない契約に費用を掛けない）。出所 = s2-07l.243 run 1 / run 2 の QUESTION（doctor の行数・末尾行・snapshot を pin する歯 5 本 + 埋め込みの宣言に依る歯 2 本が write-set 外で赤になった）。
- **write-set の項目の実在と展開**: write-set の各項目は base に実在する file か、末尾 `/` の dir（base に実在）か、新規 file（`+` 接頭辞で宣言・base に無いことを検査）か、**縮む面**（`-` 接頭辞で宣言・base に実在する file・「この便でその file の増分は 0 以下」という見積の符号を項目が運ぶ＝挙動不変の module 分割で歯と本文を外へ出す元の file の形・s2-07l.279 が受付で断られた 2026-09-14 の型）か、**置き場だけの file**（`=` 接頭辞で宣言・base に実在する file・中身を変えず verify の置き場として載せただけ＝縮む面と同じく上限の余地も core の見積の本数も求めない・§43 (1)）のいずれかで、それ以外は `Refuse::WriteSetItemUnresolved { item }`（`-` の先が base に無い項目も同じ理由）。契約表の検査では、`+` の項目が tracked に在れば land 済みの実在 file として読む（intake は在れば断る・場面は閉じた型 `NewFilePolicy` の値 1 つで渡し、読む関数は 1 本・s2-07l.346）。接頭辞は受付の宣言であって path の一部ではない＝交差の照合（FR39・ADR-0019）・runner の guard へ渡す write-set・便の worktree の allowlist は接頭辞を剥がした素の path で持つ（spawn が剥がす・管理席が契約 file で剥がして写す手順を要らなくする）。交差と guard の照合は dir 項目を base の file 一覧に**展開してから**数える（dir で書いた snapshot の置き場が、配下 1 file を持つ別便と偽の交差を起こした 2026-09-14 の型・s2-07l.206 × .243）。
- **上限の余地**: write-set の `.rs` file のうち **R-C4-2 の測定範囲と同じ `crates/<c>/src/` 配下のもの**（xtask check の file-lines が数える file・`tests/` の歯と `xtask` の外の file は門の対象外ゆえ余地を求めない＝受付が門より広くならない・s2-07l.249 run 7 の受付が 2000 行級の test file で .206 / .245 を断った 2026-09-14 の型）について、base の行数と R-C4-2 の値の差（余地）を測り、行の `size` の見積が余地を超える file が在れば `Refuse::CapHeadroom { file, headroom, size }` で断る。core の合計（R-C4-1）も同じ式で 1 回（core の見積 = write-set の `.rs` 本数 × 同じ値）。§46 以後は行の任意の欄 growth が file ごとの見込みで `size` を上書きし（無い file は `size` のまま）、core の見積はその和（契約表の行 ax）。**縮む面（`-` 接頭辞）は余地を求めず、core の見積の本数にも数えない**（`size` は「1 file あたりの増分の見積」で、縮む面の増分は 0 以下と宣言されている＝余地 96 の file を 3 module へ割る便が、割る前の file に +S を当てられて断られる形〔s2-07l.279・受付が満杯の file を割る便そのものを断る＝満杯が固定される〕を作らない）。宣言を信じるのは受付の先読みだけで、上限そのものは gate の `cargo xtask check`（R-C4-2 の deny 線・測定値）が守る＝宣言が偽でも上限は超えられない（C10・宣言値は測定を経ずに有効値にならない）。縮む面が本当に縮んだかを gate が測る（head の行数 ≤ base の行数・fail-closed）のは後続（gate.rs の分割 s2-07l.286 の後）。**S / M / L と行数の対応は rules 行 3 本**（`pipe.size_s_lines` / `pipe.size_m_lines` / `pipe.size_l_lines`・1 file あたりの増分の見積・[rules-manifest.md](./rules-manifest.md) §4・user 裁定 2026-09-14）で、値は rules の読み手から取る（数を code に書かない）。**行の数え方**は xtask check の file-lines / core-lines と同じ式（幅で正規化した行数・R-C4.line-width・rules-manifest.md §4）＝1 行に詰め込んでも余地は増えない。core と xtask は互いに依存しないので式は 2 か所に在り、同じ fixture で両方を突合する歯が一致を守る。出所 = s2-07l.189 run 1（land.rs 1495/1500）・s2-07l.208 run 4（core 上限）・s2-07l.249 run 2（cli.rs / land.rs 1498/1500・満杯の file に M を当てる契約表）。**撃つ場所は受付だけ**（`pipe intake` が起こす 1 行に対して・`contracts check`〔CI〕は撃たない）: 余地は「その便を今の base に当てたら入るか」という**受付時点の事実**で、契約表の行は履歴を持つ（landed 済みの行 a / f の write-set の file はその後も育つ・表は状態を持たない〔C15・状態は台帳〕）ので、表の全行に当てると landed 行が上限で永久に違反になる（s2-07l.249 run 3・check.rs 1499 / fleet/mod.rs 1287）。§3 の他の 3 つ（外形 pin・項目の実在と展開・名指しの実在）は base に対する静的な事実なので受付と CI の両方が撃つ。
- **名指しの実在**: 行の `title` / `done` と設計 doc の当該 `section` の本文で backtick に囲まれた字面のうち、**中身全体が**次の 3 形のどれかに一致するものだけを名指しと読む（一致しない字面〔glob・属性・散文〕は名指しではない。構造体の literal・field 付きの variant・引数付きの呼出しは**先頭の token** を 3 形に当てて読む＝§25）: (1) path 形 = 英数字と「_ . / -」だけで拡張子が .rs（例: pipe/closure.rs）→ base の tracked file の path がその字面と等しいか、その字面を「/」区切りの末尾に持てば解ける（例: declaration.rs は pipe/declaration.rs に解ける）(2) 型の path 形 = 「::」で結んだ識別子の列（例: crate::fleet::Stage・Guard::Intake）→ 末尾の 2 節「型::項目」が base の `.rs` に現れれば解ける（§26 が impl の block 経由の第 2 経路を足す＝method / 関連 fn は字面が無くても解ける）。**その行の `touches` に宣言した型の variant は名指しと読まない**（`touches` は「その型に variant を足す契約」の宣言そのものなので、未来の variant〔§4 の審査の段の名・§5 の照合の完了条件の名〕は行 (c) / (d) の `touches` が説明する）(3) fn 形 = 識別子 + 「(」(+「)」)（例: overlaps(）→ base の `.rs` に `fn` 識別子 の宣言が在れば解ける。解けないものを `Refuse::NameUnresolved { name, at }` で全件名指す（字面走査の下界・別名や generic は見ない・新規 file は write-set の `+` 接頭辞で除く）。出所 = s2-07l.243 run 2 の「Guard::Rules」（実在しない variant を設計 doc と契約が名指した）・s2-07l.249 run 1 の QUESTION（本節の例示と未来の variant を検査が拾い、現物の契約表が違反 0 にならなかった＝形の定義を精密にし、例示は backtick に置かない）。
- 上の 4 つは閉包と同じ関数の列（1 実装・受付と CI・C2）で、Guard は増えない（`Guard::Intake` の断りの理由が増えるだけ・極性一覧の行数は不変）。
- **write-set の導出（契約 (h)・user 指摘 2026-09-14「恒久的に write-set が適切に分配される質の高い計画を書けるような修正」）**: write-set を planner が手で列挙する形は、不足（閉包の file が無い＝(g) が断る）と余分（触らない file を挙げて別の便と偽の交差を起こす＝直列化の根）の両方を人の注意に頼る。行は **何を触るか（`touches`）・何で測るか（`verify`）・新設する file（`creates`）・歯の新しい置き場（`tests`）・Rust の外で触る file（`also`）** を宣言し、write-set は器が**導出値**として作る。導出 = 次の和集合（各項は 1 関数・宣言順・**置き場は closure の子 module derive**〔file は契約表の行 m の新規項目・`weighted_lines` / `Fields` / `Base` / `derive_write_set` / `check_drift` / `teeth_places` と in-file の歯・純移動 `s2-07l.363`・呼び手は `pipe/closure.rs` の `pub use` で不変〕。型の閉包の字面走査〔上の 4 形・`sees`〕は `pipe/closure.rs` のまま）: (i) 閉包 = `touches` の型を構造として持つ `.rs`（上の 4 形）(ii) **歯の置き場（base の実測）** = `verify` 行のうち `cargo nextest run` の行から crate（`-p` の値・無ければ core crate）と末尾の filter 語を取り、その crate の `tests/` 配下と `src/` の test 区間で **`#[test]` の直下の `fn` の名が filter 語を含む** file の全部（helper の fn は数えない・nextest の positional filter と同じ「含む」で解く）。新しい接頭辞は base で必ず 0 本なので、そのときは行の `tests` 欄の file〔base に実在するか `creates` に在る〕が置き場（`tests` 欄はこの周に必須）で、`tests` も無ければ `Refuse::TeethPlaceUnresolved { filter }`。(iii) 外形 = `surfaces` の snapshot と pin の file（上の第 5 形）(iv) `creates` = 新規 file の列（項目は `+` を付けずに書く・`+` 付きは `Refuse::WriteSetItemUnresolved` の形で断る・base に無いことを検査＝write-set の `+` 接頭辞と同じ意味を欄で表す）(v) `also` = base に実在する **非 `.rs`** の file（`rules/manifest.toml`・`.github/workflows/ci.yml`・`docs/design/*.md`・`contracts/schema.toml`）。`also` に `.rs` を書いた行は `Refuse::AlsoNamesRust { item }`（Rust の面は `touches` と `tests` から導く＝「どの型を触るか・歯をどこに置くか」を宣言させる側に倒す）。導出値と手書きは型で区別する（閉じた enum `WriteSet` の variant `Derived` / `Declared`・C10 の派生値）。
- **手書きの write-set の扱いと撃つ場所**: 行に `write-set` が在れば導出値と**集合として一致**しなければ断る（`Refuse::WriteSetDrift { missing, extra }`・不足も余分も全部名指す）。`write-set` の無い行は導出値を write-set とし、契約 file（(b) の生成・(b) の前は admin が写す file）と runner の allowlist に書く。**撃つのは受付だけ**（上限の余地と同じ構図: 表は履歴で landed 行は導出値と離れていく＝CI の `contracts check` は従来の「閉包 ⊆ write-set」だけを撃ち drift を撃たない）。**弁別は欄の有無（typed・散文の免除を持たない）**: `creates` / `tests` / `also` のどれも持たず `write-set` を持つ行は `Declared`＝導出も drift も撃たず (g) までの検査だけで通る（(h) の前に書かれた行〔a〜g・他 doc の表〕の通常形・行を新形へ移すのは各行の planner の手番）。新欄を 1 つでも持つ行と、新欄も `write-set` も無い行（`touches` と `verify` だけの行）は `Derived`＝導出の対象で、`write-set` が在れば drift を撃ち、無ければ導出値を write-set にする（4 象限を 2 値で閉じる・C2）。drift の集合比較は `+x`（write-set）と `x`（`creates`）を同じ項目に正規化して行う。(h) 自身は `Declared` の形で intake される。`--contract` の形（契約 file を直接渡す・(b) で廃止）は `touches` を持たないので導出できず、従来の閉包検査だけを撃つ。契約 file に載る `write-set` の新欄（`creates` / `tests` / `also`）は生成した契約 file には写さない（runner が読むのは導出済みの write-set だけ）。
- **限界（残す側）**: (ii) は base の歯の名で置き場を決めるので、名が filter 語を含まない歯（別の接頭辞で書かれた歯）は置き場に入らず、実装役が置いた歯が write-set 外に出れば FR20 の guard が止め質問（FR31）で出る。閉包の下界（別名・generic）は上と同じ。`tests` 欄は歯の file だけを受ける（`tests/` 配下か `src/` の test 区間を持つ file・それ以外は `Refuse::TestsNotATeethFile { item }`）。**write-set の `+` の 2 つの場面**（`s2-07l.346`）: `+path` は「新規 file の宣言」で、intake は base に無いことを要求し（`MustBeAbsent`）、契約表の検査（CI・land 後の main）は tracked に在れば実在の file と読む（`MayBeLanded`）——場面は閉じた型の値 1 つで渡し、読む関数は 1 本。**閉包の同名衝突**（`s2-07l.347`）: 4 形（literal 構築・match の arm・件数 pin・const slice）はいずれも「その file から `touches` の型が見えているか」を 1 関数 `sees(path, text, touched, names)`（`closure.rs`・pure・path を受けるのは (a) の判定に要るから）で判定してから数え、別 module の同名の型（headless の runner と hook の guard がそれぞれ持つ `Decision`）を拾わない。見えている = 次の 3 条件のいずれか: (a) **宣言** = その file が型を宣言し（`enum <Name>` / `struct <Name>` の宣言行）、path が `touches` の module に当たる（`src/` からの相対で `<module>.rs` か `<module>/` の直下・`module` は `touches` の型名の直前の 1 段＝多段 module は最後の段で弁別し親 dir は見ない〔`crate::hook::guard::Decision` → `src/hook/guard.rs`〕・`crate::Type` は `lib.rs` / `main.rs`・`crates/<c>/` の接頭辞は任意）(b) **import** = `use <module>::Name` で同名に取り込む（`as` の別名は下界の外）(c) **修飾** = 本文に `<module>::Name` の path 修飾が在る。(b)(c) の `<module>` の段は、その file が `<module>/` の直下の子 file（`mod.rs` を除く）なら `super` も同じ module と読む（`src/fleet/cli.rs` の `use super::Stage` は `crate::fleet::Stage`・re-export 先の子 file〔`src/fleet/wait.rs` の `Completion`〕は (a) で自 module）。const slice の名（`crate::paint::HUES.len()` の件数 pin）は (b)(c) で型名と同じに読む＝型を名指さずに型の構造を持つ file を落とさない。const slice の宣言 file も `sees` を通す（別 module の同名 const slice を拾わない）。件数 pin の 1 出現の解決（修飾か取り込み済みか）も同じ (b)(c) の照合で、2 実装にしない。glob（`use m::*` / `use super::*`）越しの取り込みは下界の外（別名・generic と同じ・見落とした file は FR20 の guard が止め質問で出る）。schema の面: 契約表の field に `creates` / `tests` / `also`（任意・list）を足し `write-set` を必須から任意へ（生成物 `contracts/schema.toml` が写す・(a) の schema 版は変えない＝任意 field の追加）。intake の判定行に `write-set=<derived|declared> files=<N>` を足す（`pipe` の外形 snapshot は intake の判定行を pin していないので動かない・ok 行の等値 assert が動く＝`tests/e2e/pipe.rs` の側で write-set 済み）。

## 4. 契約の審査の段（FR49）

- **段**: `Stage::Reviewed`（宣言順は `Intake` の直後・`as_str` = `Reviewed`）。`pipe intake` の直後に器が lens を 1 回撃ち、verdict を run dir の `review.json`（gate の `verdict.json` と同型・tmp → rename の atomic 書き）と `RunStage stage=Reviewed detail=verdict:<PASS|FAIL|INCONCLUSIVE>` に残す。
- **lens の口**: 既存の `<NAME> lens` に雛形を 1 枚足す（`headless/lens-contract.txt`・穴 = `{contract}`〔生成した契約 file〕/ `{design}`〔`section` の本文〕/ `{requirements}`〔`req` の要件本文・SRS から抜く〕・diff は無い）。観点は 3 つ（契約と設計の節の適合・設計が名指す状態遷移の一周〔段・完了 enum・列の所属〕・write-set の連鎖〔§3 の閉包に無い構造の落ち〕）。verdict は既存の 3 値。予算は NFR1 の cap をそのまま使う（契約 + 節 + 要件で cap を超えたら INCONCLUSIVE＝FR9 の極性）。要件本文の読み手は要件面の形ごとに 1 関数（html の anchor / yaml の `id` + `text` / md の見出し・`s2-07l.354`・契約表の行 k・id の集合の読み手 `requirement_ids` と同じ拡張子の 1 match）で、読めない形・本文の無い id は理由の 1 行を材料に載せる（黙って空にしない・NFR4）。
- **効き方**: `pipe run` / `pipe resume` は `Reviewed` かつ verdict PASS の run だけを spawn する（現物の `launch(.., &[Stage::Intake])` の入口を `Reviewed` に改める）。FAIL / INCONCLUSIVE は終端（`live` は false・retire 可）。直しは設計 doc の改訂 → PR → 再 intake（run 2）。
- **人の関与 0**: 審査を人が飛ばす口は無い（`--no-review` を作らない・C16）。歯の側も同じ: `tests/e2e/pipe.rs` の helper（`intake` → `spawn_with` / `implemented` / `gated_pass` / `questioned`・pipe/gate.rs・land.rs・lifecycle.rs の約 100 か所が使う）は lens 無しで intake → spawn を通しているので、helper の中で**偽 PASS の lens**（gate の `fake_lens` と同じ作り）による審査を 1 回通す形に改める（呼び出し側は不変・審査の段を飛ばす flag を歯にも作らない）。intake 直後の event 数を pin する歯（`event_count == 1`）は審査の段の event を数に入れる。行 (c) の write-set はこの 5 file を含む（.241 run 1 の QUESTION 2026-09-14・planner 裁定 (a)）。
- **順序**: 契約 (c) は (b) より先に流す（user 裁定 2026-09-14・台帳 s2-07l.197 notes）。生成 (b) が無い間は、受付が読んだ契約 file（設計 pointer 付き・(a) が検査済み）の `section` の節を設計 doc から読んで審査する＝`{design}` の穴の出所は (b) の前後で変わらない（行の pointer）。理由: 契約の不備が入口で止まらず runner と gate の周を費やした実測（2026-09-13〜14: .208 run 1 / 2・.235・.238・.222 の 5 件が (c) の観点で止まる種類）。

## 5. land の終端（FR50）

Landed（gate-cost.md §6 の CAS の後）に続けて器が行う。各段は typed な event を **1 件ずつ**記す（`RunDone` の `detail` で弁別・schema 1 のまま・push / ci / close の 3 段）。止まった段から先は撃たず、記録もそこで終わる。終端の結末は閉じた 7 値（Closed / Undeclared / Unreadable / PushFailed / CiFailed / CiUnmeasurable / CloseFailed）で、CI の「測れない」を failure に畳まず、宣言を読めない周（Unreadable）を「押す先が無い」（Undeclared）にも「push の失敗」にも畳まない（push を 1 度も撃っていない＝測れていない・C10）。rc 0 は Closed と Undeclared の 2 値だけである。CI の照合は**落ちた run を先に見る**（複数の workflow が並ぶ repo では 1 本が落ちた後も別の 1 本が走っているのが常態で、未完了を先に見ると deadline を空費した末に failure が「測れない」に化ける・`s2-07l.382` の lens の指摘）。

1. **push**: `git push <remote> main:main`（子 process・remote 名は `.vessel.toml` の宣言 `remote`）。**既定の remote は持たない**: push は repo の外へ出す行為（A1「出す」）なので、押す先を宣言していない repo の便は終端を持たない（`terminal=undeclared`・rc 0・event 0 件・`--pr-cmd` 形と同じ極性・既存の toy repo の歯は動かない）。失敗は `RunDone detail=push:failed:<reason>` で止める（close しない・rc 1）。
2. **CI の照合**: 唯一の wait 実装に `Completion::CiResult { repo, sha }` を足し、forge の CLI（`.vessel.toml` の `ci-cmd`〔optional・無ければ既定の 1 行 = `gh run list --commit {sha} --json status,conclusion`〕）を deadline（rules 行 `pipe.ci_wait_s`・Int・裁定 id）まで待つ。`ci-cmd` は **argv 1 本として撃つ**（shell を通さない・宣言は対象 repo の tracked file から来るので shell に渡すと 1 行が別の command を継ぎ足せる）。`{sha}` の穴は**必須**で、穴の無い行は断る（別の commit の判定を読んで success と言いうる）。`{sha}` には full の sha を入れる（短縮 sha は forge の CLI が一致させない）。結果は 3 値（success / failure / unmeasurable）。**success 以外は close しない**（FailClosed）・記帳して rc 1。
3. **台帳の close**: 台帳 adapter（§6）で `close <bead> --reason "landed <sha> ci=success"`。adapter が撃てない・rc ≠ 0 なら `RunDone detail=close:failed` で止める（着地は成立している＝やり直しは `pipe land --terminal-only <run>` で終端だけ再実行・冪等）。
4. **binary の世代**: record（verdicts.jsonl の行）に `generation=<build 元 commit>` を足す（§2・`--version` の括弧の中身と同じ 1 本）。同じ行の `sha` が着地した commit を持つので、値を landed sha にすると同値の欄が 2 つ並び、§12 の版の比較にどちらを使うか読み手が判じられない（C10）。自分の版が landed sha より古い周に起動を断るかは後続（§12）。
5. **commit の trailer**: squash commit の本文末尾に `<Name>-Contract: <doc id>#<row id>` と `<Name>-Requirements: <req の列>` の trailer を書く（名は NAME 定数から導出・C2.2・他の道具の trailer と衝突しない）（既存の `run:` trailer と 1 組に統合・**着地の正本は record〔面 5・event log〕で、trailer は器が squash message に同時に書く導出面**・folio2 の RTM は trailer だけを読み、無ければ「まだ分からない」と出す〔「未着地」とは言わない〕・`--pr-cmd` 形は trailer も record も無い恒久の穴として RTM に出る）。

`--pr-cmd` の形（自 repo への PR）は終端を持たない（従来どおり）。

## 6. 台帳 adapter（FR50 / FR51）

- **置き場**: 新 module `ledger/`（core）。読み = `bd --readonly show <id> --json` / `bd --readonly list --status open --limit 0 --json`（子 process・git / tmux と同型・crate 依存なし・出力は既存の `json_lite` で読む）。書き = `bd close <id> --reason <text>` の **1 種だけ**（起票・acceptance・裁定は席）。binary の名は const・path は PATH 解決（env を読まない・C2.2）。
- **lint**（`<NAME> doctor --state-dir S --repo R` の項目 1 行・C3.2・契約表の行 e）: open の bead を全件読み、(i) 契約（acceptance が `design =` で始まる bead）で pointer が解けない（doc が無い・区間に id が無い）(ii) memo（label `intake:memo`）で本文に機械が読む設計の見出し（固定の 1 つ・`## memo`）が在り `design =` / `research =` の pointer 行が無い (iii) 契約で acceptance が pointer の 1 行を超える本文を持つ（§2「台帳の bead」・生成 (b) の Landed 後は本文を機械が読まず、lens と実装役が読む契約は行と節だけ＝本文は写しの矛盾の置き場になる・.209 が審査で 7 周止まった型）、を名指す。件数と母集団を同じ行に出す（`ledger: open=N contracts=K unresolved=U bodied=B memos=M unpointed=P`）。
- **lint の置き場と読み口（現物・verified 2026-09-20）**: 台帳 adapter の module（`crates/scribe2/src/ledger/mod.rs`）は着地済みで、**書きの `close` と `CloseError` と `POLARITY` だけ**を持つ（lint も読みも無い）。読みは席の側の `crates/scribe2/src/seat/ledger.rs`（`DEFAULT_BD` を adapter が `pub use` で借りている）に在るものを**そのまま使い 2 本目の reader を作らない**（C2）——現物の `read_ledger(` は全件（`--all --limit 0 --json`）を取り、1 件の型は id と status を持つ。lint は open の絞りをその戻り値に掛け、**acceptance と label を 1 件の型へ足す**（同じ 1 本の reader を広げる＝`issues_of(` の parse の側に欄が増える）。lint 自身は adapter の子 module（新規 file・`crates/scribe2/src/ledger/mod.rs` に宣言を 1 行）に置き、**極性は増やさない**（doctor は読むだけで判定しない・C10.2＝`crate::polarity` の列と外形は不変）。歯の file も新規で、e2e の親（`crates/scribe2-boundary/tests/e2e/main.rs`）に module の宣言を 1 行足す。
- **lint の形（番号は done と歯の対）**:
  1. **読む**: 読みの口で open の bead を全件取り、読めない周（client が起動できない・rc ≠ 0・出力が読めない）は件数 0 に**倒さず**、行を測れていない形（`ledger: unreadable reason=<語>`）で出す（C10・NFR4）。
  2. **数える**: 3 つの欠陥 (i)(ii)(iii) を純関数で判定し、**件数と母集団を同じ行に**出す（母集団 = open の全件と契約の件数と memo の件数）。0 件の周も 0 を出す（行が消えない）。
  3. **名指す**: 欠陥の在る bead の id を、3 つの欠陥ごとに宣言順で列にして行の後に出す（件数だけで終わらせない）。
- **歯**（`ledger_lint_` 接頭辞・置き場は新規の歯の file・偽の台帳 client は PATH の先頭に置く shim で、引数と出力を歯の中で組む）: (a) 3 つの欠陥を**件数を違えて**持つ fixture（pointer の解けない契約 1 件・本文を持つ契約 2 件・pointer 無しの memo 3 件）で `unresolved=1 bodied=2 unpointed=3` と母集団が出て、id が 6 つとも名指される（件数を揃えると 3 つの数と欠陥種の対応が pin されず、render の数を入れ替える変異が生存する＝gate の lens が実測した空虚性）／(b) 欠陥 0 の fixture で 3 つの数が 0 になり母集団だけが在る（**0 と不在を弁別**する (2) の枝）／(c) client が起動できない周・rc ≠ 0 の周・出力が壊れた周は 3 とも 0 でなく測れていない形の行になる（(1) の否定の枝・現物が 0 を出す形なら落ちる）／(d) pointer が**解ける**契約と `## memo` の見出しを持たない memo は数に入らない（偽陽性の pin）。doctor の項目列の**外形**は `crates/scribe2-boundary/src/main.rs` の in-file の歯（snapshot `crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap`）が受け、台帳の 1 行が増えた差分がそこに写る。この歯は接頭辞 `ledger_lint_` で新しく 1 本足す——名は `ledger_lint_doctor_external_form` で、insta が歯の名から作る snapshot file は行 e の write-set の `+` の snapshot（`crates/scribe2-boundary/src/snapshots/` の下・`scribe2__tests__` + 歯の名）である。既存の外形の歯の code は 1 字も動かさないが、その snapshot（write-set の既存 file）は doctor の 1 行が増えた分だけ動く（既存の外形の歯の名を verify の filter に書くと、席の doctor の外形の歯の名がその字面を**末尾に含む**ため filter が 2 file に当たり、触らない file を write-set に要求する＝2026-09-20 の preflight で実測）。
- **CI は撃たない**（private な台帳に届かない）。

## 7. 質問と契約の改訂（.133 の解消）

回答（FR32）は逐語のまま。**回答が write-set を広げる形は持たない**: 実装役が「write-set に file が足りない」を質問したら、planner は設計 doc の契約表を直す（PR）→ 再 intake（run 2・base は新 main）。写しは生成物なので手で直さない（管理席の手順 #9 が消える）。§3 の閉包が先に拾うので、この形の質問は「字面走査の下界の外」だけになる。

## 8. 極性（[polarity.md](./polarity.md)）

| guard | 段 | 極性 | 何を止めるか |
|---|---|---|---|
| `ContractTable` | in-loop（intake・`contracts check`） | FailClosed | 区間が無い・parse できない・id が無い・req / section が解けない・閉包 ⊄ write-set・verify の形が違う契約 |
| `Review` | in-loop（`Reviewed` の段） | FailClosed | verdict が PASS でない run の spawn |
| `LandTerminal` | in-loop（land の終端） | FailClosed | push 失敗・CI が success でない・adapter が撃てない周の close |

`ledger lint`（doctor の項目）は guard ではない（行為を止めうる判定を返さない・ADR-0014 §2.1）。

## 9. 歯（`crates/<NAME>/tests/e2e/` に `contract_` / `pipe_review_` / `pipe_terminal_` / `ledger_` 接頭辞・名前の列は現物が SSOT）

- 契約表: toy repo の設計 doc（区間 1 つ・3 行）から `intake --design` が正常の 1 行で run を作り contract.toml の field が REQUIRED 全部 + design + touches を持つ／閉包が足りない行は `WriteSetIncomplete` で足りない file を全部名指し run dir が増えない／section が無い行・req が SRS に無い行・区間の無い doc・`--contract` の形はいずれも typed に断る／`contracts check` が全 doc を全件・行番号付きで出す（AC21）。 write-set の項目が末尾 `/` 無しで既存の dir を指す行は `WriteSetDirWithoutSlash` で断る（guard は末尾 `/` 無しを字面一致でしか通さず、runner が配下の file を書けない＝末尾 `/` は配下全部・個別名は既存 file だけを更新する便に使う）。
- 散文の門（(f)・AC25）: fixture の設計 doc（印を持つ文 3 つ = pointer 無し・数 + 単位付き・適合）で xtask の項目が違反 2 件を file:line 付きで名指し非 0・適合だけの fixture で 0 件・印の一覧 / pointer の形 / 単位の一覧は const slice（型付き）で in-file の歯が宣言順と件数を pin・property（適合する文に印を足しても pointer が在れば適合のまま・数 + 単位を足すと違反）。現物の `docs/design/*.md` は 0 件（違反は planner が設計 doc の便で直す・runner は設計 doc を触らない）。
- 閉包の拡張（契約 (g)・`contract_closure_ext_`）: `surfaces` を持つ fixture で snapshot と pin する歯の file が足りないと名指す／dir 項目を展開して配下の別 file と交差 0 になる fixture／実在しない項目は `WriteSetItemUnresolved`／余地を超える `size` の fixture で `CapHeadroom`（file と core の 2 形）／実在しない名指しの fixture で `NameUnresolved`（全件・行番号）／現物の契約表で 4 つとも違反 0。
- 閉包（in-file・pure）: literal 構築・match の arm・件数 pin・const slice の 4 形を固定 fixture で pin・読めない file は違反・別名の形は下界の外（歯で「拾わない」を pin し限界を残す）。
- 審査: 偽 lens が FAIL を返す run は `Reviewed(FAIL)` で止まり構築点の呼出 0・PASS は Spawned へ・`review.json` と event・`--no-review` の引数は usage で断る（AC22）。
- 終端: 偽 remote（bare repo）+ 偽 CI cmd + 偽 adapter（stub が argv を写す）で Landed の後に push → CI（success）→ close の 3 event・CI が failure の fixture は close されず rc 1・adapter が rc 1 の周は `close:failed` と `--terminal-only` の冪等（AC23）。`pipe.ci_wait_s` の欠落は `RuleError`。
- lint: 偽 adapter の出力（memo 2 本 pointer 無し・契約 1 本 pointer 不解決）で doctor の行が件数と母集団を出す・全件揃えば 0（AC24）。
- 極性一覧 snapshot に 3 行（件数 +3・N = K + M）。`STAGES` の宣言順（`Reviewed` は `Intake` の直後）。
- 実地（done の一部・歯にしない）: 本 doc 自身の契約表から (a) を intake し Landed → 終端 → close まで席の手順なしに通す（AC22 の D・AC23 の D）。

## 10. 憲法・制約との整合

C15 / C15.2（台帳は task と裁定・pointer の欠落は lint）・C16 / C16.2（受付・審査・終端で止める in-loop guard・極性一覧）・C2（`Refuse` / `Stage` / `Completion` に variant を足す・1 関数）・C2.2（env を読まない・remote / ci-cmd は宣言 file・binary の名は const）・C3 / C3.4（段は event log・待ちは唯一の wait 実装）・C5（`pipe.ci_wait_s` は裁定 id）・C10（record は typed・`unmeasurable` を success に読み替えない）・C11.2 / C11.3（極性の定数・Timeout は Result）・C12.5（区間の parse・doctor の行・review.json の外形 snapshot）・C12.7（閉包の property）・N1（終端の失敗は着地を取り消さない・retire は move）・A3（構文木 crate を採らない・forge CLI と bd は子 process）。

## 11. 却下案（ADR-0023 §5 の写しは持たない・設計固有のもの）

- 台帳の acceptance を typed にして正本のままにする。却下: 台帳は機械が読む形式を持たず（free text）、器が読むには adapter が要り、C15 に逆行する。
- 入口の検査だけ足して 3 段の写しを残す。却下: 写しの各段で drift する（今日 5 例）・設計の inline を止められない。
- 閉包を構文木（syn 等）で求める。却下: A3 の依存・NFR3（実行時依存 0）。字面の下界 + 質問 record で足りる。
- 契約の審査を人が回す（dispatch 前に planner が lens）。却下: N2（散文の規則）・C16（後段で代替しない）。
- CI が台帳 lint を撃つ。却下: CI は private な台帳に届かない。doctor + tick。
- 回答で write-set を広げる口（`pipe answer --write-set-add`）。却下: 写しは生成物・正本は設計 doc（§7）。

## 12. 後続

- **散文部の門（暫定）**: 設計 doc の散文のうち「規範の印を持つ文」（印の一覧 = 型付きデータ）が pointer を持ち数 + 単位を持たないことを、folio2 M1 が同じ式を引き取るまで scribe2 の `xtask check` の 1 項目として持つ（**user 裁定 (b)・2026-09-13T08:53Z**・契約 (f)・FR52）。数値一般の検査は採らない（折り合い 2026-09-13: folio2 の正本で 48〜87 件・scribe2 の設計 doc で 510 件の偽陽性）。撤去の条件 = folio2 の床が同じ式を持ち scribe2 の設計 doc を検査した記録（M1）。
- **folio2 の grill G-f / G-h（scribe2 側の立場）**: G-f（folio2 文書の pack 分割）は scribe2 の所掌外＝所見なし。G-h（folio2 の文書が席へ届く経路）は scribe2 の役割注入（seat-roles (c) の雛形の pointer 行）が carrier で、(c) の land までは planner 間の file 交換で代替する。
- **folio2 への移行（M1）**: 契約表の正本を設計ノート YAML へ移し (ii) の導出 file を読む（順序は folio2 planner の G-g・推奨 (a) 今の形で land → M1 で移す）。移行の費用の実測（2026-09-13）: 設計 doc 15 本 = 約 210k 字・表 118・fence 12・link 254。
binary の世代で起動を断る（§5 4.）・契約表から台帳の bead を起票する口（台帳 write の 2 種目・A1 の「出す」に当たらないが scope の改訂が要る）・`Reviewed` の lens の観点を rules 行にする（.176）・memo 40 本の設計内容を各設計 doc の「未契約の機構」表へ移す（planner の一括作業・§6 の lint が残りを名指す）。

## 13. consumer の最小の整え方（`s2-07l.354`・pointer だけ）

scribe2 を載せる consumer が pipe を通すのに要る面は 3 つで、いずれも既存の口の pointer だけを持つ（本節は規則を持たない）: (1) **要件面** = `.vessel.toml` の任意 key `requirements`（repo 相対 path・無ければ既定 path・`pipe/declaration.rs`）。形は `.html`（`id="FR1"` の anchor）/ `.yaml`（`- id: FR1` と同じ mapping の `text:`）/ `.md`（行頭 `#` 見出しの先頭 token が要件 id・本文は次の見出しの直前まで）の 3 つで、id の集合と本文の読み手は §4「lens の口」の同じ 1 本。(2) **契約表** = 設計 doc の末尾の区間に行 1 つ（欄の正本は `contracts schema` の出力・§2）。(3) **契約の `req`** = 要件 id の形（英大文字 + 数字・台帳の id は通らない・憲法 C15.2）。初例 = uns の照会（2026-09-15・台帳 s2-07l.354）。

## 14. pipe/declaration.rs の分割（契約表の行 n・純移動）

- 何が起きているか: `pipe/declaration.rs`（約 1280 行・src 818 + in-file の歯 458）は R-C4-2 の余地が 213 行しか無く、size M の便（.170）を受付が断る。責務のうち「write-set の項目の読みと上限の余地」の群（WriteSetItem / NewFilePolicy / read_write_set / read_item / is_under / Caps / Headroom / CORE / line_count / headroom_shortfalls / core_of・src 160 行 + 対応する歯 172 行）は宣言 parse（`.vessel.toml`）と Effective の写しの群に依存しない閉じた集合（agent の実測 2026-09-16・grep で確認）。
- 形（§3 の .363 と同型）: 子 module declaration/write_set.rs へその群と歯をそのまま移す。親は mod 宣言と `pub use`（headroom_shortfalls / line_count / read_write_set / Caps / Headroom / NewFilePolicy / WriteSetItem / CORE）と `pub(crate) use`（is_under）で呼び手（`pipe/table.rs`・`pipe/cli/intake.rs`）を無傷に保つ。移動に伴う唯一の書き換えは子から 1 段深くなる 3 参照（`super::refuse::…` 2 つ・`super::closure::weighted_lines` 1 つ）を crate からの path に直すこと。親に残る私有 item を子が呼ぶ周は可視性を `pub(super)` に上げる＝可視性の 1 語と mod 宣言・`pub use`・`use` の path・移動で生じた可視性の制約を説明する doc コメント行は移動の一部（純移動の残差として許す・.363 / .372 と同じ）。札 `// flip-check: moved <bead>` は親と子の歯の区間に対で置く。割った型を `touches` に持つ他の行の閉包が新 file へ広がる周は、契約表の検査の歯が名指す行の write-set に新 file を同じ PR で足す（本 doc が write-set に在る理由・§15 と同じ）。
- 見積: 親 約 948 行・子 約 340 行。

## 15. pipe/table.rs の分割（契約表の行 o・純移動）

- 何が起きているか: `pipe/table.rs`（約 1275 行・src 908 + in-file の歯 367）は R-C4-2 の余地が 213 行しか無く、契約表を触る便（.277 / .354 ほか）が S しか置けない。責務は 6 群（schema 正本 / 区間の抜き出しと TOML の parse / findings の語彙 / check_table の本体 / 要件面の読み / CLI の駆動）。
- 決定的な制約（実測）: `TableError` は `tests/e2e/polarity.rs` が `std::any::type_name` の字面（`pipe::table::TableError`）を pin しており、`pub use` の再輸出では型名が変わらない＝**TableError / Finding / Context / unreadable は親に残す**（子へ実体を移すと極性一覧の snapshot が割れる）。
- 形（§3 の .363 と同型）: 子 module 2 つ。table/parse.rs = 区間の抜き出しと TOML の parse の群（Form / form_of / region / shift / read_rows / typed / text_of / list_of / find_row / Pointer / PointerError / parse_pointer / contract_id / doc_id・181 行 + 歯 2 本）。table/check.rs = 検査の本体と要件面の読みと CLI の駆動（check_table から read_all まで 21 item・326 行 + 歯 5 本と fixture）。親は mod 宣言 2 つと名指しの `pub use` で呼び手（`pipe/cli.rs`・`pipe/cli/intake.rs`・`pipe/review.rs`・`rules/manifest.rs`・歯）を無傷に保つ。共有 fixture（full_row）は親の歯に `pub(super)` で残し子は `super::super::tests::` で読む（複製しない）。親に残る私有の helper（typed / text_of / list_of 等）を子が呼ぶ周は可視性を `pub(super)` に上げる＝**可視性の 1 語と mod 宣言・`pub use`・`use` の path・移動で生じた可視性の制約を説明する doc コメント行は移動の一部**（純移動の残差として許す・.363 / .372 と同じ）。札は 3 file の歯の区間に対で置く。外形（極性一覧の snapshot）は verify で `polarity_external_form` を名指して不変を測る。割った型を `touches` に持つ他の行（行 h の ContractRow 等）の閉包は新 file へ広がるので、契約表の検査の歯が名指す行の write-set に新 file を同じ PR で足す（本 doc が write-set に在る理由・純移動の便は自分が消す file を名乗る他の行を同じ PR で直す）。
- 見積: 親 約 354 行・parse 約 181 行・check 約 326 行。

## 16. write-set の外形 pin（第 5 形）の探索域を歯の区間に限る（契約表の行 p・`s2-07l.282`）

- 何が起きているか: planner の実測 2026-09-14 17:2xZ（#176）で、契約表の `surfaces` を宣言すると `write-set-incomplete` になる（`pipe/closure.rs` が閉包の file に数えられる）。第 5 形（外形 pin）の探索が「その snapshot 名か usage 文字列を literal に持つ file」を src 全体で数え、導出の実装 file 自身と fixture を pin file に数える自己言及の偽陽性が起きる。結果、外形を触る契約が閉包の便との偽の交差を起こす（dispatcher の交差判定に直結）。現物（verified）: `pipe/closure.rs` の `surface_closure`（第 5 形）と `test_region`（歯の区間 = `tests/` 配下は全体・src は `#[cfg(test)]` 以降）は既に在る。
- 形: `surface_closure` の literal 探索を**歯の区間**（既存の `test_region`）に限る。`tests/e2e/*.rs` は全体・src の file は `#[cfg(test)]` 以降だけを数え、実装の本文（`closure.rs` の導出・fixture の const）は数えない。
- 触らない: 第 1〜4 形・`surfaces` の名の検査（snapshot 名 / usage を持つ subcommand の名）・`test_region` の定義。
- 却下案: `closure.rs` を固定で除外（字面の特例・C2）／`surfaces` を snapshot の path で宣言（宣言の形が変わり既存の行を書き直す＝この案で足りる）。

## 17. write-set の導出に creates の親 mod と subcommand の閉じた enum を足す（契約表の行 q・`s2-07l.337`）

- 何が起きているか: planner 実測 2026-09-15 10:4xZ で、契約 (a) の直命の表を Derived で書けなかった。write-set の導出（`pipe/closure.rs`）は Rust の面を `touches` の型の閉包と `tests` からしか導かず、subcommand を足す便が触る 2 面（cli の文字列 match の腕・`seat/mod.rs` の `pub mod <新 module>;`）が写らない。`also` は非 `.rs` 限定・`creates` は新規のみ・`surfaces` は歯の区間だけ＝「口を 1 つ足す」契約は Declared に戻る。
- **現物の測り直し（verified 2026-09-20・main f678bd0・`s2-07l.479` の席の自律機能の削除と ADR-0045 の役割の統合の後）**: `crates/scribe2/src/seat/cli.rs` の `dispatch(` の match は **`Some("…") =>` が 2 本**（`register` / `launch`）＋ **guard 付きの腕 1 本**（第 1 token が `--` で始まらない非空の文字列を口座 label と読む短い形・[account-lifecycle.md](./account-lifecycle.md) §14）＋ `_` の腕。`crates/scribe2/src/seat/mod.rs` の `pub mod` 宣言は **8 本**。`crates/scribe2/src/pipe/cli.rs` の `Some("…")` は **17 本**。どちらの cli module にも subcommand の閉じた enum は無い。記録当時の 10 本 / 14 本は削除の前の数である。
- **形 (vi)**（`creates` の親 module の宣言 file・置き場は §3 の導出の側＝closure の子 module derive・新規 file の検査 `created` の隣に 1 関数）: `creates` の各 `.rs` について、**その新設 file を module の木に繋ぐ `pub mod <名>;` の 1 行が載る file**を導出値に足す。候補は項目の dir から次の **3 形**で組み、**base の tracked に在るものを全部**足す（どれも tracked に無い周は足さない＝dir ごと新設する契約の親は `creates` の側が載せる）。`.rs` でない項目と dir を持たない項目は親を持たない。
  - (vi-1) `<dir>/mod.rs`（dir の module を dir の中の file で宣言する形）。
  - (vi-2) `<dir>.rs`（dir の module を dir の隣の file で宣言する形）。(vi-1) と (vi-2) は Rust では並び立たないので、tracked に在るのは高々 1 つである。
  - (vi-3) **項目の dir が crate の src の根（`crates/<c>/src`）である周の crate の根の宣言 file** = 同じ dir の `lib.rs` と `main.rs`。この 2 つは**並び立つ**（実測 2026-09-20 main 1e40c2b: core の crate は両方が tracked で、`lib.rs` が 12 本・`main.rs` が 1 本の module 宣言を持つ／xtask の crate は `main.rs` だけが tracked で 24 本を持つ）ので、**どちらが `pub mod <名>;` を受けるかは導出では決まらない＝tracked な方を全部**導出値に入れる（両方 tracked の crate では 2 面・片方だけの crate では 1 面。交差が 1 面広がる費用は払い、下界の形〔字面を読まず path だけで組む〕は変えない）。(vi-1) / (vi-2) だけを候補にすると、`crates/<c>/src` 直下に新設する契約の親（`crates/<c>/src/mod.rs` も `crates/<c>/src.rs` も存在しない）が 1 つも入らず、宣言の 1 行を書く file が write-set の外に出る。
- **形 (vii)**（本節に**既に在る**定義・本行の便は doc に (vii) を足さない）: `crates/scribe2/src/seat/cli.rs` と `crates/scribe2/src/pipe/cli.rs` の文字列 match を閉じた enum（`SeatCommand` / `PipeCommand`・`as_str` / `parse`・宣言順・const slice `SEAT_COMMANDS` / `PIPE_COMMANDS` の件数 pin）にし、以後「口を足す」契約は `touches = ["crate::seat::cli::SeatCommand"]` で cli.rs が閉包（match の arm）に入る。
- **閉じた語でない腕の扱い（(vii) の境界・run の QUESTION を先に閉じる）**: seat の短い形の腕は **subcommand ではない**（値が口座 label で、閉じた集合を持たない）。enum は**既知の verb だけ**を語にし、`parse` が `None` を返した token は今までどおり guard 付きの腕へ落とす＝腕の順序（既知の verb → label → `_`）と各腕の rc は不変で、label の腕を enum の語にも `_` にも畳まない。`SEAT_COMMANDS` の件数は**既知の verb の本数**（記録時点 2）であって dispatch の腕の本数ではない。
- (vii) の続き: const slice の名は 2 つで**別**にする（`ALL` のような同名にしない）: 2 つの cli module は最後の段が同じ `cli` なので、`sees` の (b)(c) は module `cli` の `ALL` を seat / pipe のどちらの型の件数 pin とも読み、両者の閉包が互いの歯の file を拾う（多段 module を最後の段で弁別する下界の限界・§3）＝名を分けて 2 つの閉包を素にする。件数 pin の歯は `crates/scribe2-boundary/tests/e2e/seat.rs` / `crates/scribe2-boundary/tests/e2e/pipe.rs` に置き、`vessel::seat::cli::SEAT_COMMANDS.len()` / `vessel::pipe::cli::PIPE_COMMANDS.len()` の修飾形で書く＝§3 (iii) の件数 pin（const slice の名を (c) 修飾で解く・module は型名の直前の 1 段 = `cli`）でその歯の file だけが自分の型の閉包に入る。親 module（`seat/mod.rs` / `pipe/mod.rs`）は `pub mod cli;` を既に持つ（現物）ので触らない。base には型も const slice も無いので、その歯を base に当てた周は e2e binary ごと compile error＝flip-check はこれを RED と数える（overlay 後の compile error は RED の規則・`crates/xtask/src/flipcheck.rs` の module doc）。
- 歯（接頭辞と置き場・done と 1:1）:
  1. `contract_derive_creates_parent_`（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`・toy repo の tracked は歯の中で組む）= `creates` に新設 file を宣言した行の導出値に**親の宣言 file が入る**ことを **(vi) の 3 形とも**測る: (vi-1) 親が `<dir>/mod.rs` の周／(vi-2) 親が `<dir>.rs` の周／**(vi-3) 項目が `crates/<c>/src` の直下で `lib.rs` と `main.rs` が両方 tracked の周は 2 面とも入り、`main.rs` だけが tracked の crate は 1 面だけ入る**（`<dir>/mod.rs` と `<dir>.rs` の 2 形しか持たない実装ではこの周の導出値が空になって落ちる）／親の候補がどれも base に無い周は導出値に足さない（**否定の枝**）／`.rs` でない項目と dir を持たない項目は親を持たない（**否定の枝**）。
  2. `contract_derive_subcommand_enum_`（同 file）= `touches` に cli の型を宣言した行の導出値に **cli.rs と件数 pin の歯の file が入り**、もう一方の cli の歯の file は**入らない**（名を分けた効き目の pin＝同名なら落ちる）。
  3. `seat_command_all_` / `pipe_command_all_`（`crates/scribe2-boundary/tests/e2e/seat.rs` / `crates/scribe2-boundary/tests/e2e/pipe.rs`）= const slice の件数と宣言順が型と一致し、`as_str` と `parse` が往復し、**未知の token は `parse` が `None`** を返す（label の腕へ落ちる側の pin）。
  4. **usage の字面が不変**であることは既存の歯が受け、行の verify が完全名 `seat_usage_external_form` / `pipe_external_form`（外形 snapshot `e2e__seat__seat_usage_external_form.snap` / `e2e__pipe__pipe_external_form.snap`）で撃つ。
- 触らない: 各 subcommand の実装関数・口座 label の短い形の腕の意味と rc・usage の字面（`SEAT_COMMANDS` / `PIPE_COMMANDS` から組んで同じ字面になることを外形 snapshot で pin）。
- 却下案: 導出に「usage 行を持つ .rs」の形を足す（字面の形が増える・閉じた enum で既存の第 2 形に乗せる方が C2）／Declared のまま（`.303` の型の QUESTION が再発する）。

## 18. write-set の導出に fn 形の touches を足す（契約表の行 r・`s2-07l.358`）

- 何が起きているか: planner の `.323` の契約化（2026-09-15 16:5xZ・実測）で、Derived 形の write-set が subcommand の入口 file（`pipe/cli.rs` の `resume` の match・`ratelimit.rs` の段の分岐）に届かない。導出（`pipe/closure.rs` の `closure`）は `touches` を型の path（`crate::module::Type`・末尾が大文字始まり・`touched`）としか読まず、分岐を 1 本足す契約が触る入口の file を型で表せない（届く型は `Stage` / `Outcome` / `Verdict` で全木に広がる）＝Declared に戻るか全木へ広がるかの二択。現物（verified・main 797389f）: 名指しの検査（`unresolved_names`）は fn 形（`Form::Fn`・`declares_fn`）を既に読むが、導出の `touched` は大文字始まりの名だけを受け、fn 形は `ClosureError::TypeForm` で断る。§17（行 q）の (vi)(vii) は creates の親 mod と subcommand の閉じた enum を足す形で、fn の名指しは持たない。
- 形: 導出の第 8 項として **fn 形の touches**（`crate::<module>::<snake_case の識別子>`・末尾が小文字始まり）を足す。閉包 = その module の段（`scopes` / `in_module`・型形と同じ 1 関数を通す）で `fn <識別子>(` を宣言する file（`declares_fn`・下界のまま・呼び手は数えない）。宣言する file が 0 の周は typed に断る（`ClosureError` の variant 1 つ・空集合に潰さない・C10）。型形の 4 形・§16 の第 5 形・§17 の (vi)(vii) は不変。
- 断りの名と字面（run 3 = 審査 FAIL 2026-09-16「新 variant を既存 `TypeForm` の流用と区別できない・型形の退行 pin が空虚」の解として本文に置く）: 新 variant は **`ClosureError::FnUndeclared { module, name }`**、`reason()` の字面は **「touches の <module>::<name> を宣言する file が base に無い」**（既存 `TypeForm` の「crate::module::Type の形でない」とは別の字面）。歯は断りの字面でこの variant を名指して弁別する。型形の退行 pin の fixture は **型を持つ toy**（例: paint の module file に `pub enum Hue`・別の file に `Hue::Red =>` の arm・toy の path は歯の中で組む＝base の file ではない）で、fn 形の行と型形の行を同じ toy に置き、型形の導出値が base と同じ集合であることを assert する（fn だけの toy では pin が空虚）。
- 閉包の置き場（run 1 = 審査 INCONCLUSIVE 2026-09-16「touches は `Touched` だけを名指すが §18 は `ClosureError` の variant 追加も要求する」の解）: 行 r の `touches` は `Touched` と **`ClosureError`** の 2 つ。`ClosureError` は `pipe/closure.rs` の外で `closure/derive.rs`（`Err(ClosureError::…)` の構築）・`pipe/table.rs` と `cli/intake.rs`（`ClosureError` の variant を `=>` の左に持つ arm）・`pipe/refuse.rs`（`Refuse` → `ClosureError` の写し・`=>` の右辺で variant を構築）の 4 file が名指す（verified・main 43706fe・母集団 = `crates/scribe2/src` の grep）。**run 2 = 審査 FAIL 2026-09-16「write-set に refuse.rs が無い」の実測**: Derived の閉包（`files_of`）は「見えている」file のうち `<Type> {` の literal 構築・`<Type>::` が `=>` の左の arm・const slice の pin・宣言の 4 形しか数えず、**enum の variant 構築**（`ClosureError::WriteSetDrift { … }` を `=>` の右辺や `Err(…)` の中で作る refuse.rs / derive.rs）に当たらないため 2 file が落ちる（memo `s2-07l.387`・第 6 形の契約化は別便）。したがって行 r は **.387 の Landed まで手書きの `write-set`（6 面）で運ぶ**（`also` に `.rs` は書けない＝`AlsoNamesRust`）。新 variant を受付の断り（`Refuse`）へ写すかは実装役の判断で、写すなら `refuse.rs` は write-set の中。歯の toy repo は `tests/e2e/pipe/intake.rs` の既存の `contract_derive_` の歯と同じく **test の中で組む**（on-disk の fixture は置かない・`src/pipe/cli.rs` に `fn resume(` を書いた 1 file）。
- 触らない: `unresolved_names` の fn 形（名指しの検査は別の面）・`Form` / `Touched` の型名・契約表の schema（`touches` の値の形が 1 つ増えるだけで field は増えない）。
- 却下案: 入口の match を dispatch の閉じた enum に寄せる（§17 の (vii) が同じ向きで担う・分岐の追加が variant の追加になる大きい形）／`also` に `.rs` を許す（Rust の面を手書きに戻す＝Declared の再来）／呼び手まで閉包に入れる（上界に化ける・`Stage::` と同じ全木の広がり）。

## 19. write-set の導出に enum の variant 構築の形を足す（契約表の行 s・`s2-07l.387`）

- 何が起きているか: `.358` run 2（2026-09-16 03:45Z）が審査 FAIL。行 r（touches = `Touched` + `ClosureError`）から受付が焼いた write-set は 4 面で、`ClosureError` を名指す file（母集団 = `crates/scribe2/src` の grep・当時の main 43706fe で 5 本）のうち `pipe/refuse.rs` と `pipe/closure/derive.rs` が落ちた。現物（verified・main dce6aea）: `pipe/closure.rs` の `files_of` は「見えている」file（`sees`）のうち **4 形**＝宣言（`declaring`）・literal 構築 `constructs`（needle = `<Type> {`）・match の arm `matches_arm`（`<Type>::` が `=>` の**左**）・const slice の件数 pin `pins` のどれかを持つ file だけを導出値に入れる。落ちた 2 file が `ClosureError` について持つのは **enum の variant 構築**だけである（`pipe/refuse.rs` は `Refuse` の arm の**右辺**で同名の variant を組み直して理由の字面を 1 本にし、`pipe/closure/derive.rs` は `Err(…)` の中で組む）＝4 形のどれにも当たらない。同じ型を `=>` の**左**に持つ arm で拾われるのは `pipe/cli/intake.rs`（断りへの写し）で、この非対称が穴の形である。§3「閉包の同名衝突」の 4 形と §16 の第 5 形（外形 pin）はこの形を持たない＝FR48「閉じた型を構造として持つ file を含む」の穴。
- 形: `files_of` の述語に **第 6 形 = variant 構築** を 1 つ足す（C2・述語 1 つ）: 本文の行を**最初の `=>` で割った右側**（`=>` の無い行は行の全部）に `<Type>::<Variant> {` または `<Type>::<Variant>(` の出現（`<Variant>` = 大文字始まりの識別子・`{` / `(` の前の空白は任意）が在る file。`=>` の左のパターン側は数えない（`matches_arm` が数える面と重ねない）。`sees` の門（型が見えている file だけ）は同じ 1 関数を通す（同名の型の衝突は §3 のまま）。`Self::<Variant> {` は数えない（`Self` は型名でない＝宣言 file は `declaring` が持つ）。
- 約束（行 s の done の (1)〜(4) と 1:1・番号は done の順）:
  1. **正の枝**: variant 構築だけを持つ file（4 形のどれも持たない file）が導出値に入る。`{` の構築と `(` の構築の両方・`=>` の右辺の構築と `Err(…)` の中の構築の両方。
  2. **負の枝**: (a) `Self::<Variant> {` の構築しか持たない file（型の宣言 file の `impl` の中）は入らない。(b) 小文字始まりの項目（`<Type>::assoc_fn(` の呼出し）しか持たない file は入らない。(c) `<Type>::<Variant> {` が `=>` の左のパターン側にしか無い出現を第 6 形は数えない。(d) `sees` が通さない file（別 module の同名の型）は従来どおり入らない。
  3. **既存 4 形の導出値は不変**: 4 形のどれかを持つ file の集合と、`sees` の 3 形・alias を見ない下界は変わらない。
  4. **現物の契約表は `contracts check` の findings 0 のまま**: 第 6 形で導出値が広がる着地済みの行の write-set に、広がった file を**同じ便で**追記する（下の「着地済み行への波及」の 6 行 19 項目）。
- 歯（`closure_variant_construction_` 接頭辞・`pipe/closure.rs` の in-file の歯・base で compile し assert で RED〔機能不在〕）: 約束 1 と約束 2 の (a) (b) (d) は `closure` を既存の signature で呼んで導出値の集合で測る（`=>` の右辺に構築を持つだけの file と `Err(…)` の中に構築を持つだけの file が入る〔`{` 形と `(` 形を別の file で〕／`Self::` だけの file・小文字の項目だけの file・型が見えていない同名の file は入らない）。約束 2 の (c) は**第 6 形の述語を直に呼ぶ**（`=>` の左だけの出現を持つ本文で偽・同じ本文の右側へ移すと真）＝導出値では `matches_arm` が同じ file を入れるので弁別できない面で、in-file の歯だけが測れる。約束 3 = 既存 fixture の 4 形の導出値が base と同じ集合。
- 検証行（3 本・歯の file はどれも行 s の write-set の中＝`teeth-outside-write-set` を出さない）: 約束 1 / 2 = `closure_variant_construction_`（新しい接頭辞・base で 0 本＝RED の理由は機能不在）／約束 3 = 既存の `closure_picks_each_of_the_four_forms_from_its_own_file`（`pipe/closure.rs` の in-file・緑のまま）／約束 4 = 既存の `contract_closure_ext_real_table_has_zero_findings`（`tests/e2e/pipe/intake.rs`・現物の契約表を撃つ歯＝**述語だけ足して表を直さない実装はこの行で赤になる**）。
- 触らない: `constructs` / `matches_arm` / `pins` の判定・`sees` の 3 形・`Touched` / `Form` の型名・契約表の schema・fn 形（§18・.358）・外形 pin（§16）・他 module の doc コメントの呼び名「4 形」（`pipe/closure/derive.rs` / `pipe/closure/names.rs` / `pipe/table/check.rs`）＝第 5 形（§16）と fn 形（§18）が着地した後も同じ呼び名のままで、形の数え方の正本は `pipe/closure.rs` の module doc（write-set の中）だけが持つ・検証行が名指す既存の歯 2 本の名（4 形の退行の歯と現物の契約表の歯・改名すると検証行が空を撃つ）。
- 着地済み行への波及（run 1 の QUESTION 2026-09-16 06:44Z・数は planner の再走査 2026-09-20・main dce6aea で取り直した）: 第 6 形は**着地済みの Derived 行の閉包も広げる**。契約表の検査（`closure_findings`）は `touches` の閉包が行の write-set に収まることを撃つので、広がった file を持たない行は write-set-incomplete で赤くなり、歯 `contract_closure_ext_real_table_has_zero_findings` が main を赤にする。母集団の実測（第 6 形だけを足した述語を現物の tracked な `.rs` に当て、行ごとに 4 形の導出値と突き合わせた走査。4 形だけで write-set の外に出る行は 0＝現物の findings 0 と一致する）: 設計 doc 21 本・契約表の行 151・`touches` を持つ行 18（うち write-set も持ち閉包の検査が撃たれる行 12・write-set が空で検査の撃たれない行 6）。第 6 形で導出値が広がる行は 8、**write-set に無い file が出て赤になる行は 6 = 本 doc の行 a / b / d / g / h / w だけ**（追記は 19 項目 = a 3 / b 3 / d 5 / g 3 / h 2 / w 3）で、**他の設計 doc 20 本の行は 0**（`dispatcher.md` の行 a は 1 file 増えるが既に write-set の中）＝波及は本 doc の中で閉じ、行 s の write-set に他の設計 doc は要らない。19 項目のうち 5 項目（行 a / b / g / h / w の `pipe/closure/names.rs`）は字面の雑音（歯の fixture の文字列 literal と `matches!` のパターン）で、第 6 形が上界側へ広がる面である（雑音の無い 14 項目は `=>` の右辺か `Err(…)` の中の実構築）。行 w は未着地の便の行で、追記はその便の write-set を広げる（残る 5 行は着地済み＝追記は履歴の側で、閉包の検査だけが読む）。純移動の行 n / o（§14 / §15）と同じ型＝**同じ PR で本 doc の契約表の該当行の write-set に広がった file を追記する**（行 s の write-set に本 doc を持つ理由）。追記は導出値をそのまま写す（手で選ばない・Declared に戻さない）。別便に送る案は却下（main が赤の窓を作る＝C12.6）。
- 却下案: `matches_arm` を「`<Type>::` の出現全部」に広げる（`use` 文や doc コメントの `[`ClosureError::Unreadable`]` まで拾い上界に化ける）／`also` に `.rs` を許す（`AlsoNamesRust`・Declared の再来）／行 r を恒久に Declared にする（手書きの write-set は数え落とす＝.303 の型）。
- 着地後: 行 r（§18）の暫定 Declared を `touches` / `tests` の Derived に戻す（docs PR・planner）。

## 20. Declared 行にも歯の置き場の門（verify の歯の file ⊆ write-set）を撃つ（契約表の行 t・`s2-07l.391`）

- 何が起きているか: admin の実測 2026-09-16 04:4xZ（本日の審査の終端 19/42 便・45%・母集団 = 本日の便）で、型 (a)「検証行の歯が write-set の閉包の外の file を要る」が `.358` run 3 / `.383` run 1 / `.164` ほかを審査 FAIL に倒した。Derived 行は §3 (ii)（`closure/derive.rs` の `teeth_places`）が `verify` の filter 語から base の歯の file を解いて導出値に入れるが、Declared 行（`creates` / `tests` / `also` を持たず `write-set` を持つ行・`cli/intake.rs` の `settle_write_set`）は「導出も drift も撃たない」だけで、planner が手で数えた `write-set` に歯の file が無くても受付を通り、審査（`Stage::Reviewed`）で 1 周（数十分）払う。
- 形: `settle_write_set` の Declared 分岐で、導出と drift は撃たないまま **歯の置き場の門**を 1 つ撃つ。読み手は §3 (ii) と同じ 1 関数（`teeth_places`・`pub(crate)` 化・`tests` 欄は空のまま渡す＝2 本目の読み手を作らない・C2）で、`verify` の nextest 行ごとに base の歯の file を解き、解けた file のうち行の `write-set`（`check_drift` と同じ正規化・dir 項目はその配下）に無いものを**全部**名指して断る: 新 variant `ClosureError::TeethOutsideWriteSet { files }`（辞書順）→ `Refuse::TeethOutsideWriteSet { files }`（名 `teeth-outside-write-set`・rc 1・理由の字面は導出の側と同じ 1 本・`refuse_of` に 1 行・`Refuse` の宣言順の末尾）。base で 0 本の filter 語（新しい歯の接頭辞）は、Declared 行に `tests` 欄が無いので、行の `write-set` に歯の file（`teeth_file` と同じ弁別 = `test_region` が空でない file）が 1 つも無ければ従来の `TeethPlaceUnresolved`（字面不変）で断り、1 つでも在ればそれを置き場と読んで通す。判定行の token（`write-set=declared` / `files=<行の項目数>`）と写しの契約 file（write-set は行のまま）は不変。nextest 形でない `verify` 行（例: `git status`）は (ii) と同じく読み飛ばす＝門は撃たない。
- 触らない: Derived の経路（導出・drift・`TeethPlaceUnresolved` の条件）・Declared の弁別（3 欄の不在 ∧ `write-set` の存在）・契約表の schema・`nextest_filter` / `test_fns` の判定・歯が読む読み手の module（例: `claude_md.rs`）まで追うこと（型の閉包の領分＝審査に残す）。
- 却下案: 審査に任せる（1 周 = 数十分の損失が続く・本日 3 例）／Declared を廃止して全部 Derived にする（§17〜§19 の未 Landed の形が残るうちは Declared が要る・行 r の型）／新しい filter 語の周も断る（新設の歯の置き場は planner が write-set に書く以外に無い・歯の file が 1 つ在れば通す）。

## 21. 器の口 pipe preflight — 受付と同じ判定を run を作らず撃ち、契約の実態突合を planner が edit time に測る（契約表の行 u・`s2-07l.394`）

- 何が起きているか: user の相談 2026-09-16 06:0xZ「planner が契約を実態に測定するためのツールや突き返された修正を適切に行うためのツールをもっときっちり用意したほうが良いのでは」。本日の実例: `.164`（審査 4 周・毎回別の理由）/ `.209`（10 周・7 周が字面の不一致）/ `.392`（verify の歯が write-set の外）/ #249（§ が base に無い名を名指し CI 赤）。現物（verified・main c3f2fdb）: 契約の実態突合は **受付**（`cli/intake.rs` の `intake_run` = 契約 file の読み → `freeze`〔宣言の写し〕→ `settle_write_set`〔行の pointer・§3 の導出・§20 の歯の門〕→ `exclude_cap_shortfall`〔余地〕→ `exclude_overlap`〔live との交差〕→ run dir の作成と `RunCreated`）と **CI の歯**（`contracts check` の行の形と § の名指し）に在り、どちらも planner が契約を書いた時点で撃てる口ではない。planner の点検は state dir の script（design-of / section-of / candidates.py）と admin の preflight.sh / preflight-ws.py に散っている＝器の外の散文の作法（N2）で、怠った周が審査へ届いて 1 周（数十分）払う。
- 形: `pipe` に subcommand **`preflight`**（`--contract F --bead B --repo R [--state-dir S]`・intake と同じ引数）を足す。中身は `intake_run` を **判定（`judge`・pure に近い・run を作らない）と作成（`create`・run dir と event）の 2 段に割り**、`intake` = judge → create、`preflight` = judge だけ（C2・判定関数は 1 本・2 本目を作らない）。judge は断る理由を **最初の 1 件で止めず全部集めて**返し（`Refuse` の列・受付は従来どおり先頭の 1 件で断る。集める粒度は判定関数 1 本につき高々 1 件＝`freeze` / `settle_write_set` / `exclude_cap_shortfall` / `exclude_overlap` / 重複 run の検査をこの順に全部撃ち、各関数が返した断りを列に積む・各関数の中身と「先頭の 1 件で返す」形は不変。各関数の入力は契約 file と base の tracked / sources と state dir で〔現物: `exclude_cap_shortfall(manifest, contract, tracked, sources)` / `exclude_overlap(state_dir, contract, tracked)`〕、前段の Ok 値を取るのは「導出値で write-set を置き換える」1 点だけ＝`settle_write_set` が Err の周は契約 file の write-set のまま後段を撃つので、Declared 行では前段の Err と後段の Err が同時に列に載る。事実の出所: 余地〔file ごとの行数と余地〕と交差〔run ごとの file〕は `exclude_cap_shortfall` / `exclude_overlap` が中で既に計算している値で、各関数の Ok 値を `()` から閉じた struct へ広げて返す〔判定と極性と Err の形は不変・定義は同じ `cli/intake.rs`〕。歯の置き場〔filter ごとの file〕は導出の側の `teeth_places`（closure の子 module derive・既存・`declared_teeth` と導出 (ii) が中で撃つのと同じ 1 実装）を judge が同じ入力（`settle_write_set` が既に組む `Fields` / `Base`・`pipe/closure.rs` の `pub use` で見える）で 1 回読む＝`pipe/closure.rs` の `pub(crate) use` に名を 1 つ足すだけで derive.rs は触らない。関数が Err の周はその関数の事実の行の代わりに `refuse=` の行が立つ。state dir が無い周は交差と重複 run の 2 検査を撃たず `overlap=unmeasured`〔重複 run は intake の秒で決まる事実で、run を作らない preflight の判定には載らない〕）、preflight は stdout に 1 行 1 事実で並べる: `design=<doc>#<id> section=<n>`（pointer と行は `settle_write_set` が既に読む・§ の本文の行数は lens の材料の大きさであって判定に効かないので出さない）/ `write-set=<declared|derived> files=<n>` / `teeth=<filter>:<本数>@<file,…>`（verify の nextest 行ごと）/ `headroom=<file>:<余地>/<size の上限>`（余地の小さい順）/ `overlap=<live run>:<file,…>`（在れば）/ `refuse=<名>:<理由>`（judge の断り・全部）/ 末尾に `preflight: <ok|refused n=<件数>|broken>`。rc = 0（断り 0）/ 1（断り ≥ 1・全部列挙）/ 2（読めない・受付と同じ `RC_BROKEN` の周）。宣言の写し（`freeze`）は読むだけで書かない・event は書かない・state dir は交差の読みにだけ使う（無ければ交差の行を `overlap=unmeasured` と出して rc は他の断りで決める＝測れないを 0 に潰さない・C10）。usage の外形 snapshot に subcommand 1 語が増える（C12.5）。
- 触らない: 受付の判定の中身（`settle_write_set` / 余地 / 交差の順序と極性）・`Refuse` の variant と rc・契約 file の schema・`contracts check`（§ の名指しは CI の歯のまま・preflight は契約 file 側の名指し `NameUnresolved` を judge の中で従来どおり撃つ）・dispatcher（`.345` の入口が同じ judge を呼ぶのは行 a の便の側）。
- 却下案: planner の state dir の script を増やす（器の外・host 固有・散文の作法）／`pipe intake --dry-run`（intake の引数に既定と逆の flag が増え、flag の有無で run が出来たり出来なかったりする口になる・subcommand で分ける方が typed）／審査（lens）に任せる（1 周 = 数十分・本日の審査 FAIL 19/42 便）／judge を複製して preflight 専用にする（受付と preflight が静かにずれる・C2）。

## 22. 審査 FAIL の理由を閉じた型（FindingKind）で review.json と event に残し、report が型別に数える（契約表の行 v・`s2-07l.395`）

- 何が起きているか: user の相談 2026-09-16 06:0xZ「今までのミスの型を DB に登録していって潰していく」。本日の実例: 審査の終端 19/42 便（admin の手集計）・`.209` の 10 周のうち 7 周が同型「字面が現物と合わない」。現物（verified・main c3f2fdb）: 審査の段（§4・`pipe/review.rs`）は lens の最終行の JSON `{"verdict":…,"evidence":…}` を `parse_lens` が読み、`settle` が `review.json`（schema / run / verdict / evidence / scope / ts）と `RunStage stage=Reviewed detail=verdict:<V>` を書く。理由は `evidence` の自由文だけで**型を持たない**＝型別に数える口が無く、「どの型が残っているか」を機械が示せない。手戻りの型は本日 4 つ（(a) 検証行の歯が write-set の外 / (b) goal と done の矛盾 / (c) 空虚な assert / (d) 字面が現物と合わない）+ 材料の欠け（§ の本文が無い）。
- 形: (1) lens の雛形 `headless/lens-contract.txt` の最終行の JSON に **`kind`**（閉じた語の 1 つ: `teeth-outside-write-set` / `goal-done-contradiction` / `vacuous-assert` / `literal-mismatch` / `section-material-missing` / `other`・FAIL と INCONCLUSIVE の周は必須・PASS の周は無し）と **`at`**（指した場所の列・path か識別子か §・自由文でなく `,` 区切りの語）を足す。(2) `crates/scribe2/src/pipe/review.rs` に閉じた enum `FindingKind`（(1) が列挙した 6 語・`as_str` / `parse`・宣言順の const slice・網羅 match）を置き、`parse_lens` が `kind` を読む（FAIL / INCONCLUSIVE で `kind` が無い・読めない周は **`unparsed`** の 7 語目に倒し verdict は lens の値のまま＝理由の欠けを INCONCLUSIVE や `other` に化けさせない・C10。lens の判定に届かず器が作る INCONCLUSIVE〔`--lens` 無し・写しを読めない・起動できない・出力を読めない・scope の中で死んだ〕も同じく `unparsed`＝lens の JSON が無い周はすべて 7 語目）。`settle` は `review.json` に `kind` と `at` を任意 field で足し（schema 1 のまま・古い読み手は無視・§5 の足し方）、event の detail を `verdict:<V> kind:<k>`（PASS は従来どおり `verdict:PASS`）にする。(3) `pipe report` の 1 行に **`review_fail=<本数> by_kind=<k1>:<n1>,…`**（母集団 = `pipe report` が読む event 列の `RunStage stage=Reviewed` のうち verdict が PASS でないもの全部＝既存の `runs=` と同じ範囲で日付では絞らない・`kind:` を持たない古い event は `unparsed` に数える・kind 別の内訳を宣言順に全部・0 も出す）を足す。「潰す」= kind ごとに §21 の preflight の門が 1 つ増え、report の内訳でその kind が 0 に落ちたことを機械で見る。同型の回数で run N+1 を止める線（rules 行 `review.same_kind_stop` = **2**・user 裁定 2026-09-16T05:53Z「２論点とも推奨で進めて」）と、焼き直しが前回の指摘（`at`）に対応する差分を持たない周を受付が断る門は、この kind と `at` を入力にする**別の行**（後続・§23 予定・rules 行を足すので変異と生成物の一覧が同じ PR）。
- 現物の読み手と書き手の所在（verified 2026-09-20・(2) が触る 2 本）: どちらも `crates/scribe2/src/pipe/review.rs` の **private**（`fn parse_lens(` = lens の最終行の文字列を取り verdict と evidence の 2 値を返す／`fn settle(` = 審査の 1 件と verdict と evidence と scope を取り `review.json` と event を書いて結果を返す）。`pipe report` の 1 行の組み立ては `crates/scribe2/src/pipe/report.rs` の `pub fn report(` 1 本で、既存の token（`runs=` / `landed=` / `human_events=`）は `crates/scribe2/src/pipe/report.rs` の 1 か所の書式に並ぶ。
- 歯（`pipe_review_kind_` 接頭辞・偽 lens が最終行の JSON を書く既存の審査の fixture と同型・置き場は下の 3 file）:
  1. **読みと書き**（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）= FAIL の周に `kind` と `at` が **`review.json` の任意 field に**残り、同じ周の event の detail は **`verdict:<V> kind:<k>` の 2 語だけ**である（**`at` は event に載せない**＝`at` は `,` 区切りの語の列なので空白区切りの detail に載せると `pipe report` が detail から `kind:` を読む面と衝突する・形 (2) の線）／6 語をそれぞれ書いた lens の周でその語が両面に逐語で残る／**PASS の周は `review.json` に `kind` も `at` も持たず** detail は `verdict:PASS` のまま（否定の枝）。
  2. **7 語目へ倒す枝**（同じく `crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）= FAIL / INCONCLUSIVE で `kind` が無い周・語でない周・JSON が読めない周は `unparsed` になり **verdict は lens の値のまま**（`other` にも INCONCLUSIVE にも化けない・C10）／lens の判定に届かず器が作る INCONCLUSIVE 5 形（`--lens` 無し・写しを読めない・起動できない・出力を読めない・箱の中で死んだ）も `unparsed`。この 5 形の歯は既存の置き場に倣い `crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` / `crates/scribe2-boundary/tests/e2e/pipe/stop.rs` の審査の fixture を使う。
  3. **数える面**（`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`・`pipe report` の歯の file）= `review_fail=<本数>` が母集団（`RunStage stage=Reviewed` のうち verdict が PASS でないもの全部）と一致し、`by_kind=` が**宣言順に 7 語とも出る（0 も出す）**／`kind:` を持たない古い event は `unparsed` に数える。
  4. **雛形の外形**は既存の歯が受け、行の verify が完全名 `headless_lens_contract_prompt_external_form`（`crates/scribe2-boundary/tests/e2e/headless.rs`・snapshot `e2e__headless__lens_contract_prompt_external_form.snap`）で撃つ＝雛形に `kind` と `at` の穴を足した差分がそこに写る。
  5. **`report` の既存 token が不変**であることは既存の歯 2 本が受け、行の verify が完全名 `pipe_report_counts_human_events` と `pipe_report_counts_landed_runs_not_landed_events`（どちらも `crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`）で撃つ。
- 触らない: verdict の 3 値と rc・`review.json` の既存 key・lens の起動の形（`{contract}` / `{design}` / `{requirements}`）・審査の観点 3 つ・gate の verdict.json（審査の段だけ）・`report` の既存 token（`runs=` / `landed=` / `human_events=`）。
- 却下案: memory / notes の散文で型を数える（N2・母集団が測れない）／`evidence` の字面を grep して型を推定する（自由文の字面判定・C3.3）／型を rules 行に置く（型は理由の語彙であって閾値でも極性でもない・閉じた enum の領分）／`kind` を PASS にも必須にする（PASS に理由の型は無い・空の値を作らない）。

## 23. 同型の審査 FAIL が N 回目で材料不変の run N+1 を受付が断り、直前の指摘（at）に対応する差分の無い焼き直しを受付が断る（契約表の行 w・`s2-07l.396`）

- 何が起きているか: §22 の後続（user 裁定 2026-09-16T05:53Z・同型の停止の回数 N = 2・逐語は台帳 `s2-07l.395` notes）。§22 が Landed すると `review.json` と event に kind と at が残るが、受付（`cli/intake.rs` の `intake_run`・§21 の judge の側）は同じ bead の前の便の判定を読まない＝同じ kind の FAIL が何周続いても run N+1 は無限に出せ、焼き直しが前回の指摘（at）に触れていなくても通る（本日の実測: `.209` の 10 周のうち 7 周が同型「字面が現物と合わない」）。現物（verified・main a620600）: 受付の断りは write-set の弁別（§3）・上限の余地・live な便との交差（`exclude_overlap`）だけで、便の履歴を読む口は無い。審査の材料は run dir の `review/` に残る（`keep`・契約の写し + `design.txt`〔行の section の本文〕+ `requirements.txt`）。lens の verdict は同じ材料でも揺れる（`.380` で PASS ↔ FAIL）ので、材料不変の再 intake を 1 回も許さない形は採らない＝回数の線は rules 行。
- 形: (1) **rules 行** `review.same_kind_stop`（`RuleKind` の variant `ReviewSameKindStop`・Int・**値 = 2**・裁定 id `user 2026-09-16T05:53Z`・C5・宣言順の末尾・値は manifest だけが持ち本節は写さない）。行の無い manifest は受付を 1 byte も動かさない（rc 2・行を名指す・`pipe.land_wait_s` と同じ極性）。(2) **同型の停止（受付の門・run dir も event も作らない・write-set の弁別の後・余地と交差の前）**: 受付は置き場の replay から同じ bead の便を id の新しい順に並べ、段が Reviewed 以降の便の `review.json` を読み（読めない便は `WriteSetUnreadable` と同じ断り＝`live` と同じ読み手・段が Intake の便は数えない）、先頭の便の kind と同じ kind が verdict PASS で途切れるまで連続する本数を数える（kind が unparsed の便は数えず連鎖も切らない＝lens の欠けを契約の型に化けさせない・C10）。本数が値に達し、かつ先頭の便の材料（`review/` の契約の写しと `design.txt`）が今回の材料（受付が写す形の契約 file〔導出値を置いた後〕と base から読む節の本文〔§4 の `design_text`・同じ 1 本〕）と両方とも同じ字面の周は、`Refuse` の新 variant `SameKindRepeated { kind, runs }`（名 `same-kind-repeated`・rc 1・runs = 数えた便 id の列・新しい順・理由の 1 行に kind と本数と行の値）で断る。契約か節のどちらかが変わっていれば通す（「焼き直しは書き直し」を器の線にする・§7 の形）。(3) **焼き直しの門（受付の門・同じ場所・停止の後）**: 同じ bead の直前の便（新しい順の先頭）の verdict が PASS でない周、その `review.json` の kind と at の各項目に「対応する差分」が在るかを **kind ごとに 1 関数**（`review.rs`・閉じた型 FindingKind〔§22〕の網羅 match）で測る: teeth-outside-write-set → at の各 path が今回の write-set（弁別済み・dir 項目はその配下）に在る／literal-mismatch → at の各識別子が今回の契約 file と節の本文に無い、または base に解ける（`NameUnresolved` の名指しの読み手と同じ 1 本）／section-material-missing → 節の本文が直前の便の `design.txt` と異なる。対応の無い項目が 1 つでも在る周は `Refuse` の新 variant `FindingUnaddressed { kind, at }`（名 `finding-unaddressed`・rc 1・at = 対応の無かった項目だけ・辞書順）で断る。goal-done-contradiction / vacuous-assert / other / unparsed と at の空な周は測れない＝通す（判断を要する型は planner に残す・裁定の (2) の線）。(4) 断りは §21 の preflight にもそのまま出る（judge の側に置く＝run を作らずに撃てる・planner が edit time に測る）。
- 触らない: 審査の段（§4）と lens の起動・§22 の kind / at の書き方と `review.json` の既存 key・verdict の 3 値と rc・回数の値（manifest だけが持つ）・交差と余地の判定・`.394`（§21）の judge / create の割り方（先に Landed なら judge の中に置き、後なら `exclude_overlap` の隣に置いて `.394` が寄せる）・台帳（受付は run を作らないので QUESTION event の宿主が無い＝断りの 1 行と preflight で planner に届く）。
- 歯（`pipe_intake_repeat_` 接頭辞・`tests/e2e/pipe/intake.rs`・偽 lens が最終行の JSON に kind と at を書く §22 の fixture・toy repo の設計 doc と契約 file を歯が書き換えて commit する）: 同じ kind の FAIL 2 便の後、契約 file と節の本文がともに不変の 3 便目は `same-kind-repeated` と 2 便の id を名指す／節の本文か契約 file のどちらかを変えると通る／kind が違う 2 便は通る／unparsed 2 便は通る／PASS を挟むと数え直す／teeth-outside-write-set at=path の便の後、write-set に path の無い契約は `finding-unaddressed` と path・在れば通る／literal-mismatch at=識別子 の便の後、識別子を書いたままで base に無い契約は断られ・消すか base に足すと通る／section-material-missing の便の後、節の本文が不変の契約は断られ・変えると通る／行の無い manifest は rc 2 で行を名指す。rules 行は `rules_review_same_kind_` 接頭辞（`crates/scribe2-boundary/tests/e2e/rules.rs`・値と kind と裁定 id と宣言順の pin・行と variant を対で足させる）で、**外形**は既存の歯が受け行の verify が完全名 `rules_external_form`（同 file・外形 snapshot `e2e__rules__rules_external_form.snap` の `rows=` / `kinds=` が 1 つ増える）で撃つ。
- 却下案: 回数を散文の作法にする（N2）／at を `evidence` の自由文から grep する（C3.3）／回数に達したら台帳へ QUESTION event を書く（受付は run を作らない・宿主が無い）／全 kind に門を撃つ（測れない型を偽の「対応済み」に倒す・C10）／同型の停止を契約 file の sha の差だけで解く（節の本文を見ない＝acceptance の言い換えだけで通り §7 の線に反する）／材料不変の再 intake を 1 回目から断る（lens の揺れの再測を塞ぐ・回数の線は裁定の値）／停止を段（Stage）の variant にする（受付の断りは段の遷移でない・run が無い）。

## 24. 着地で消える file の宣言 — write-set の項目の `~` 接頭辞（契約表の行 x・`s2-07l.405`）

- 何が起きているか: 別 repo の planner の実測 2026-09-16（要旨: 着地済みの行の write-set に着地で削除された file が残ると、契約表の検査が `write-set-item-unresolved` を出して CI が永久に赤になる・4 path）。現物（verified・main a620600）: write-set の項目は `WriteSetItem` の 4 値（`File` / `Dir` / `New`〔`+`〕/ `Shrink`〔`-`〕）で、「この便で消す file」を表す形が無い。`+` は場面の閉じた型 `NewFilePolicy`（`MustBeAbsent` = 受付 / `MayBeLanded` = 契約表の検査）で 2 場面を分けるが、消える file は逆向き（受付では在り、着地の後は無い）で、`-` は「縮むが残る」を意味する（§3）。行から path を外すと契約表が「何を消したか」の履歴を保てない。
- 形: (1) 接頭辞 `~`（tilde）= **着地で消える file** の宣言。`WriteSetItem` に variant `Delete`（接頭辞を剥がした path・宣言順の末尾）を足す。受付（`MustBeAbsent` の場面）は base に**実在する file** を要し、無ければ従来の `WriteSetItemUnresolved` で断る（消す予定の file が無い＝宣言の誤り）。契約表の検査（`MayBeLanded` の場面）は tracked に**無ければ着地で消えたと読んで通し**、在れば「まだ消していない実在 file」として通す（両場面とも解ける・履歴が残る）。(2) 交差の照合（FR39）・runner の guard・worktree の allowlist・閉包（§3）は `-` と同じく**接頭辞を剥がした素の path**で読む（消す file は触る file・guard は消す操作を許す側）。**純移動（rename）の弁別**: 同じ行に `+新` と `~旧` を両方書いたものが純移動で、2 項目は独立に解ける（`+` は base に無いこと・`~` は base に在ることを受付が検査し、契約表の検査は両方とも着地の後も解ける）。`-旧` は「縮むが残る」なので純移動には使わない（`-` の先が着地で消えると契約表の検査が `write-set-item-unresolved` になる＝§3 の現行のまま）。交差の照合と gate の照合は 2 つの素の path として数える。**消す操作の境界は gate**: pre-tool-use の write-set guard が見る tool は `Edit` / `Write` / `MultiEdit` / `NotebookEdit` だけで `Bash` は通す（`hook/guard.rs` の `GUARDED`）ので、runner の `git rm` は guard の判定を受けない。消す操作の境界は gate の write-set 照合（`gate/verify.rs` の `check_write_set`・`git diff --name-only` は消えた path も列に出す・`listed` は接頭辞を剥がして当てる）で、`~` の項目に無い path を消した便は `outside-scope` で落ちる＝`~` の宣言が無いと削除は gate で止まり、在れば通る（機構の追加は無い・`normalize` の 1 本で効く）。上限の余地は求めず core の見積の本数にも数えない（増分は負・`Shrink` と同じ扱い）。(3) 名指しの実在（§3）は、その行の `~` の項目と等しい path 形の名指し（`title` / `done` / 節の本文）を解けたものと読む（`+` の新規 file と同じ除外・着地の後に本文の名指しが赤になる型を塞ぐ）。(4) Derived 行の欄（`deletes`）は本 § の外（後続・schema の欄の追加は別便）＝当面は Declared 行の `write-set` の接頭辞だけ。接頭辞を剥がす規則は `pipe/refuse.rs` の `normalize` の **1 本**（`+` / `-` を剥がす・drift の集合比較 `check_drift`・交差の照合 `overlaps`・spawn の `write_policy` が同じ 1 本を撃つ）で、`~` もそこで剥がす（剥がす規則を 2 か所に持たない）。接頭辞の const は `NEW_FILE` / `SHRINK_FILE` の隣（同じ file）。
- 閉包の置き場（verified・main d462fce・母集団 = `crates/scribe2/src` の grep）: `WriteSetItem` を構造として持つ file（§3 の 4 形）は `declaration/write_set.rs` **だけ**（`read_item` の構築と `headroom_shortfalls` の網羅 match）。`cli/intake.rs` / `pipe/refuse.rs` / `table/check.rs` の字面は `Refuse` の variant `WriteSetItemUnresolved`（別の型）で、`declaration.rs` は再輸出だけ＝型の閉包に入らない。残る 3 面は **fn 形の touches**（§18）で名指す: `pipe/refuse.rs` の `normalize`（`~` を剥がす・接頭辞の const）／`cli/intake.rs` の `exclude_cap_shortfall`（接頭辞付きで解けない項目を受付で断る分岐が `+` / `-` の列を持つ）／`closure/names.rs` の `unresolved_names`（宣言 file・`pipe/closure.rs` は再輸出だけで面に入らない・(3) の除外・`+` の新規 file を除く分岐）。`closure/derive.rs`（`check_drift` は `normalize` を呼ぶだけ）・`table/check.rs`（`read_write_set` が `~` を読む・名指しの検査は行の write-set をそのまま渡す）・`pipe/spawn.rs`（`write_policy` は `normalize` を呼ぶだけ）は触らない。`also` に `.rs` は書けない（`AlsoNamesRust`）。
- 触らない: `+` / `-` の意味と検査・`NewFilePolicy` の 2 値（場面の弁別は同じ型の同じ値で足りる）・契約表の schema・runner の消す操作の許し方（guard の面は path の一致だけ）。
- 歯（`contract_closure_ext_delete_` 接頭辞・置き場は `contract_closure_ext_` の歯と同じ file・fixture の契約表）: `~` の項目が base に在る行は受付を通り契約 file の write-set は素の path／無い行は受付で `write-set-item-unresolved`／契約表の検査は tracked に無い `~` の項目を持つ行で findings 0・在る行でも 0／節の本文がその path を backtick で名指しても着地の後に `name-unresolved` にならない／`+` と `-` の既存の歯は緑のまま。
- 却下案: 着地済みの行の実在検査を撃たない（着地済みを CI が知る手段が台帳〔private〕か git log の字面〔散文〕しか無い・現在面と履歴面の弁別が typed にならない）／`-` に「無ければ消えた」を足す（縮む面と消える file は受付の意味が違う・`-` の先が無い項目は宣言の誤りとして断る現行を緩めない）／行から path を外す（履歴が消える・memo の指摘そのもの）。

## 25. 契約表の名指し検査が struct-like variant の literal 形と引数付きの呼出し形を先頭の token で読む（契約表の行 y・`s2-07l.399`）

- 何が起きているか（verified・2026-09-16 06:4xZ・#255）: 名指しの実在（§3・`unresolved_names` → `form_of`・どちらも `pipe/closure/names.rs`〔`pipe/closure.rs` から割った既存の子 module・2026-09-22 の現物〕に在る）は backtick の中身**全体**を path / 型の path / fn / 散文の 4 形に分ける。未 land の名を 2 つ backtick で書いた節で、素の 型::項目 の字面は型の path 形で `name-unresolved` になったが、型::項目 { 欄: 値 } の形（struct-like variant の literal・本節の例示は backtick を持たない＝走査に掛けない）は { を含むため散文に落ちて通った（findings=1 であって 2 ではない）。関数呼出しに引数が付く形（識別子(引数)）も同じ穴＝名指しなのに散文扱いの偽陰性で、未 land の名が検査を黙って抜ける。
- 形: `form_of` は backtick の中身の**先頭の token**（最初の `{` / `(` / 空白の手前まで・**末尾の `::` は落とさない**＝module path の字面 seat::account:: は従来どおり散文）を取り出して 3 形（path / 型の path / fn）に当て、残りは捨てる: 型::項目 { 欄: 値 } → 型の path 形の 型::項目（先頭の token）／型::項目(引数) → 型の path 形の 型::項目（関連関数の呼出しも同じ 1 規則・fn 形に割らない）／識別子(引数) → fn 形の識別子（**小文字始まりの識別子だけ**・大文字始まりの 識別子(…)〔Some(…) / Err(…) / Gated(FAIL)〕は tuple variant の構築の字面＝散文・§18 の fn 形の弁別と同じ）／先頭の token が Rust の予約語（pub(crate) / pub(super) の pub）なら散文（予約語は閉じた const の列・strict keywords）／識別子 + ( の形（末尾が ( か () ）は従来どおり fn 形／先頭の token がどの形にも合わない周だけ散文。`touches` に宣言した型の variant の除外（§3）は先頭の token に対して従来どおり効く。path 形の判定は先頭の token でなく中身全体のまま（path に空白や括弧は無い・変えない）。判定は 1 関数のまま（受付と CI が同じ関数を撃つ・C2）。
- 走査の母集団と本便後の findings（verified・2026-09-16 22:0xZ・本 doc の branch で全行の `title` / `done` と各行の § の本文を `backticked` と同じ対の取り方で走査・母集団 = 16 doc・backtick 6193 個）: 上の規則で新しく名指しに読まれる backtick は 73 個。直す前の同じ走査では 78 個のうち base（main d462fce）で解けないものが 7 個で、5 個は本 § と同じ PR で字面を直した（§18 / §19 の ClosureError::X の arm の例示・gate-cost §3 の Completion::pid() の呼出し・pipeline §28 の current_dir(…) の呼出し・dispatcher §2 の order(rows) の signature＝どれも未 land か std の名を backtick に持っていた）。残る 2 個は §20 の行 t の新 variant（`ClosureError` / `Refuse` の TeethOutsideWriteSet）で行 t の Landed で解ける＝行 y は行 t の後（`depends`）。予約語・大文字始まり・末尾 :: の 3 つの弁別を落とすと解けない backtick が 35 個増える（pub(crate) / pub(super) ×16・Some / Err / Ok / Gated / RunStage 等の構築 ×17・module path ×2）＝規則の 3 つの絞りは母集団の実測から出た。 再走査（2026-09-22・main 4f70b12・同じ規則を docs/design の backtick 全数 14404 個に当てた）: 2026-09-16 の走査の後に増えた解けない呼出し形は 16 個（gate-cost §31 / §33 / §34 の数式 7 個と nextest の filter 式 1 個・pipeline §28 / §43 / §45 / §48 の std の `current_dir` 4 個と属性 `#[cfg(test)]` 2 個と toy の純関数 2 個）で、行 y を起こす前の docs PR で散文（太字の数式・bare の識別子・属性の字面）に直した＝本便は 2 doc を触らない。
- 触らない: 3 形の解き方（tracked の path・型::項目 の出現・fn の宣言）・`Refuse::NameUnresolved` の形と `at` の字面・backtick の対の取り方・散文の欄の語彙検査（§2）。
- 歯（`contract_name_form_` 接頭辞・`pipe/closure/names.rs` の in-file の歯・`form_of` の隣）: fixture の未 land の型の 型::項目 { 欄: 値 } の字面が型の path 形（先頭の token）に読まれ base に無ければ `name-unresolved`／既存 fn `parse_pointer` に引数を付けた呼出し形が fn 形に読まれ base に在れば解ける／無い識別子の呼出し形は解けない／大文字始まりの 識別子(…) と pub(crate) と末尾 :: の module path は散文のまま／glob の use と属性の字面は従来どおり散文／既存の `contract_closure_ext_` の歯と現物の契約表（findings 0）は緑のまま。
- 却下案: 中身全体を正規表現で 3 形に当てる（形の数が増えるたびに regex が育つ・先頭の token の 1 規則で足りる）／`{` を含む字面を型の path 形として丸ごと解く（field 名まで base に求める・literal の中身は名指しでない）／散文の欄の語彙検査（folio2 の床）に任せる（未 land の名は語彙にも無い＝別の理由で赤になり planner が根を読めない）。

## 26. 名指しの実在の型の path 形を impl の block 経由でも解く — method / 関連 fn の偽陽性を閉じる（契約表の行 z・`s2-07l.432`）

- 何が起きているか: 別 repo の planner の報告 2026-09-17（run 前の contracts check）。§3「名指しの実在」の (2) 型の path 形は、末尾 2 節「型::項目」の**字面**が base の `.rs` に語として現れれば解ける（`unresolved_names(` の中の `holds_word(`・§25 の先頭 token の規則は分類の側で、解決の側は本 § が変える）。項目が method / 関連 fn のときは呼び手が「値.項目(」か impl の中の「Self::項目」で書くので「型::項目」の字面は現物に無く、実在する fn が name-unresolved に倒れる。現物で再現（verified・2026-09-17・fixture repo）: struct と impl の fn を持つ base で「Report::violation」（done と § 本文）と generic impl の「Wide::width」が name-unresolved になり、実在しない「Report::nope」と同じ 1 語で並ぶ＝実在と不在が判定で区別されない。memo の「file の path と読む」は不正確で、path 形 (1) は `:` を含む語を候補にしない。scribe2 側はこの偽陽性を「型::項目 を backtick に書かない」という散文の回避で避けていた（規則が散文に在る形・N2）。
- 母集団（planner の走査 2026-09-17・docs/design の backtick の型の path 形）: 全 doc 151 語（distinct 110）。行の検査対象（title / done + 行が指す § の本文）119 語・touches 除外 3・字面で解けない 0（現物の契約表は違反 0＝歯 contract_closure_ext_real_table_has_zero_findings の緑）。項目が小文字（fn / module 形）の語 43 は全部 `use` か module path の字面で解けている。検査対象外の § に字面で解けない distinct 5 語（Guard::ALL / Marker::ALL / PointerKind::ALL / LaunchError::AccountDirMissing / std::net）が在り、本 § の経路でも解けない（const slice は module 直下・variant は改名済み）＝処置は「母集団外のまま」（その § を指す行が立つ便で直す・本便は触らない）。本 § の解決は現行の上位集合なので現物の findings は 0 のまま変わらない（差が出るのは consumer repo と、散文の回避を外した後の §）。
- 形: (2) の解決を 2 経路の OR にする。(a) 現行＝「型::項目」の字面が語の境界で現れる。(b) impl 経路＝base の `.rs` のうち **`impl` で始まる行に「型」を語として持つ file**（impl 型／impl<'a> 型<'a>／impl Trait for 型 のどれも同じ照合）が、**同じ file** に「fn 項目」の宣言（`declares_fn(`）を持つ。(b) は同じ file に限る（別 file の同名 fn を拾わない・下界のまま）。`form_of(` の 3 形の分類（§25 の先頭 token を含む）・touches の型の除外・path 形 / fn 形の解決・Refuse の variant と字面・at の形は変えない。
- 触らない: `Refuse` の variant NameUnresolved の名と字面・`contracts check` の rc と判定行・§3 の (1) (3)・touches の除外・§24 (3) の `~` の除外・§25 の分類・alias（`use` の `as`）と generic の解決（下界の外のまま）・検査対象外の § の 5 語。
- 歯: in-file（closure.rs の test 区間・接頭辞 closure_names_impl_・`unresolved_names(` を既存の signature で呼ぶ＝base で compile し assert で RED）: impl の 3 形（素の impl・generic impl・trait impl）の file が fn を宣言する対で解け、impl 行の無い file の同名 fn では解けず、fn の無い項目（.243 の Guard::Rules の型）は解けないまま、variant は字面の (a) で解けたまま。e2e（intake.rs・接頭辞 contract_names_impl_）: 上の fixture repo で findings が「Report::nope」の 1 件だけ・rc 1・実在の 2 語は stdout に無い。現物の契約表の歯（違反 0）は変えない。
- 却下案: (i) `::` を含む語を候補から外す（memo 案 1）＝.243 の「Guard::Rules」（実在しない variant）を再び通し、(2) の下界を丸ごと失う。(ii) rustdoc / cargo metadata で型と項目の実在を引く＝契約表の検査に compile と外部 process を持ち込む（CI の歯が cargo を撃つ）。(iii) 「fn 項目」が任意の file に在れば解ける＝別の型の同名 method で偽陰性（impl 行で型に結ぶ (b) の方が狭い）。(iv) 散文の回避規則を続ける＝N2。

## 27. 受付は契約の散文（goal / done）を走査しない — 行 aa の門を消した（契約表の行 aa・`s2-07l.429` → `s2-07l.476`）

- 何が起きたか: 行 aa（`s2-07l.429`・Landed 9f44667）は受付に契約の散文（goal / done）の字面走査を足した——(a) backtick で名指した base の歯が verify の filter 語に当たらなければ断る・(b) backtick の中の判定行 token の literal を持つ file を write-set に求める。着地した当日に同型の受付拒否が 5 件（.341 / .381 / .395 / .418 / .462 系・母集団 = 当日の投入 9 便）並び、planner が契約の字面を門に合わせて焼き直す周が繰り返された（字面の門のいたちごっこ）。
- 裁定（user 2026-09-18 07:4xZ・逐語は台帳 epic `s2-07l` の notes・ここは要旨）: 問題のある門は消してよい。同時に、単純にできる問題を複雑に作り直していないかの指摘＝新しい走査・判定・record を足す前に「何を消せばこの問題が消えるか」を先に書く。
- 消した（`s2-07l.476`・admin の直接実装）: 散文の閉包の pure 関数と 2 つの拒否理由（歯の名指しの不被覆・pin の file の不足）・受付の `settle_write_set(` からの呼出し・判定行の散文の 3 数の token・対応する歯（in-file 4 本と e2e 6 本）。**検出線にも record にも変えない**（機構を残さない・字面走査を別の面へ移さない）。閉包の実測は gate の共通 verify（test 全件）の 1 本だけで、契約の散文が名指す歯が走るかは gate が測る。
- 残るもの（不変）: §3 の 6 形と `sees(`・§3 (ii) の歯の置き場 `teeth_places(`・§20 の Declared 行の門（verify の歯の file ⊆ write-set・`check_teeth_cover(`）・Declared / Derived の弁別・契約表の schema。行 aa の verify 行と write-set は歴史として表に残す（bead は close 済み・焼く契約は無い）。
- 却下＝散文の字面走査は増殖の型: 契約の散文を機械が読んで断る門は、散文の書き方（backtick の有無・token の字面）を門に合わせて変える圧を planner に掛け、契約の中身でなく字面で受付が割れる。当日この門で断られた契約（.341 / .381 / .395 / .464 / .462 / .471）の字面の回避は仕様不変なのでそのまま残す。
- 歯: `pipe_intake_prose_` の 2 本（`tests/e2e/pipe/intake.rs`・負例＝散文に既存の歯の名・判定行 token を backtick で書いた契約が受付で断られない・rc 0・判定行に散文の数の token が無い）。base は断る＝RED。消した歯は削除便の flip（removed-only）で立つ。

## 29. pipe/closure.rs の名指しの解決の群を closure/names.rs へ割る（契約表の行 ac・`s2-07l.458`・純移動）

- 何が起きているか（planner の実測 2026-09-18・main 36d9c39・`pipe preflight` で verified）: `pipe/closure.rs`（1190 行・src 710 + in-file の歯 480）は R-C4-2 の余地が 262 行しか無く、size M の便（行 aa・`s2-07l.429`）を受付が `cap-headroom` で断る（rc 1 を実測）。行 ab（`s2-07l.451`）も同じ余地で S に固定されている。責務は 3 群（型の閉包の 4 形と `sees`〔§3〕／名指しの解決〔`unresolved_names` と `Form` の判定・§3 の名指し〕／外形 pin と歯の区間〔`surface_closure` / `test_region` / `texts_of`〕）で、名指しの解決の群は他の 2 群に依存しない閉じた集合（呼び手は `table/check.rs` の 1 か所と親の歯だけ・grep で確認）。
- 形（§14 / §15 と同型）: 子 module（行 ac の write-set の `+` の file）へ名指しの解決の群（`unresolved_names` / `Form` / `resolves_type` / `impls_type` / `form_of` / `backticked` / `path_matches` / `holds_word` / `declares_fn`・src 約 116 行）と対応する歯（`closure_names_` の 3 本・fixture `name_fixture` / `named_texts` / `impl_fixture`・約 126 行）をそのまま移す。親は `mod` 宣言と `pub use`（`unresolved_names`）で呼び手（`table/check.rs`）を無傷に保つ。**親に残す**: `ClosureError` / `Source` / `surface_closure` / `test_region` / `texts_of` / `closure` と 4 形の helper / `is_ident` / `is_ident_char`（`derive.rs` の import は不変）。子は親の私有 item（`texts_of` / `test_region` / `is_ident` / const 群）を `super::` でそのまま呼べる（Rust の可視性＝子孫は祖先の私有を見る）ので親側の可視性は変えない。上げるのは**子側**の可視性＝親が再輸出する `unresolved_names`（`pub`・現物のまま）と `backticked`（`pub(super)`）の 2 つだけ。子の歯が使う親の歯の共有 fixture（`source` / `set`）は現物で既に `pub(super)`（親の歯の module に在り `pipe::closure` の子孫から見える＝derive.rs の歯と同じ `super::super::tests::` の読み方）なので語を足さない。可視性の 1 語と mod 宣言・`pub use`・`use` の path・移動で生じた可視性の制約を説明する doc コメント行は移動の一部（純移動の残差として許す・§14 と同じ）。札 `// flip-check: moved s2-07l.458` は親と子の歯の区間に対で置く。行 aa（`s2-07l.429`）が `derive.rs`（`pipe::closure` の子孫）で使う `backticked` は子 module（行 ac の write-set の `+` の file）から `super::` 始まりの path で直接引く（子側の `pub(super)` で足りる・親の再輸出は要らない＝再輸出が item より広い可視性になる形を作らない・行 aa の write-set は不変）。
- 見積: 親 約 948 行（余地 約 550）・子 約 245 行。
- 歯: 既存の `closure_` / `contract_closure_ext_` / `contract_derive_` / `prop_closure_` の歯が全部緑で期待を変えない。極性一覧の snapshot は不変（closure に境界の型名の pin は無い・`tests/e2e/polarity.rs` で実測）。
- 却下: `unresolved_names` だけを移して `Form` 系を親に残す（呼び合いが 2 module に跨り可視性の 1 語が 8 つに増える）／歯の module だけを別 file に出す（R-C4-2 は歯込みで測るので余地は増えるが責務が割れず、次の M で同じ詰まりに戻る）／行 aa を S に落とす（`prose_closure` の見積 150 行が S の 100 を超える＝size の字面だけ変える嘘）。

## 30. 受付は depends の相手を同じ doc の全行の id から解く（契約表の行 ad・`s2-07l.496`）

- 何が起きているか（orchestrator の実測 2026-09-20・verified）: 設計 doc の契約行に `depends` を書くと、受付と事前の検査の口が**必ず** `depends-unresolved` で断る。受付は §2 のとおり表の検査をその 1 行の slice に撃ち、検査は渡された行の id だけを「同じ doc の id」と読むので、相手の id は常に見えない。CI の `contracts check` は全行で撃つので緑＝CI は通って受付だけが落ちる。既に Landed の行（dispatcher の行 e ほか）でも再現する。回避として新しい行に `depends` を書かない運用が続いている（順序は台帳の blocks で表している）。
- 約束（done と 1:1）:
  1. `depends` の相手が同じ doc の契約表の**自分でない別の行**である行は、受付がそれを理由に断らない（rc 0・run dir が出来る）。
  2. 相手の id が同じ doc の契約表に無い行は、従来どおり `depends-unresolved` で断る（run dir を作らない・字面は `contracts check` と 1 byte 同じ）。
  3. 事前の検査の口（preflight）は受付と同じ 1 判定を通る＝(1) の行で `refuse=` に `depends-unresolved` が出ず、(2) の行では出る。
  4. 既存の受付・事前の検査・表の検査の歯は期待を変えない。
- 形: 検査する行は 1 つのまま（§2 の「その 1 行に撃つ」は不変＝閉包・名指し・write-set の検査を全行へ広げない・受付の時間を増やさない）。`depends` の解決の母集団だけを、同じ base（HEAD）の doc の**全行の id** にする。全行の id は受付が既に読んでいる doc の本文から取る（新しい読みを足さない）。表の検査の口は 1 本のまま＝母集団は引数で渡す（検査の文脈の型に欄を足さない・新しい公開 fn を作らない）。引数が増える呼び手は既に在る 2 か所だけで、どちらも行 ad の write-set の中に在る: 受付の 1 行の呼び手（`pipe/cli/intake.rs`）と、全行で撃つ `contracts check` の呼び手と in-file の歯（`pipe/table/check.rs`）。事前の検査の口（preflight）は**既存の構造で**受付と同じ判定関数（`pipe/cli/intake.rs` の契約を組む関数）を直に呼んでいるので、preflight の source は触らない＝約束 3 は受付の直しがそのまま効く面で、歯 (3) はそれを外から測る。表の検査の公開の形（全行で撃つ `contracts check`）は結果を変えない。
- 触らない: 輪（`depends-cycle`）の検出は 1 行の slice で測れる範囲（自分自身を指す `depends`）のまま変えず、多行に跨る輪は全行を見る `contracts check`（CI）の持ち分のまま。`depends` は表の検査の key であって、列の順序づけには使わない（順序は [dispatcher.md](./dispatcher.md) の列と台帳の blocks が持つ・本段で変えない）。断りの型と字面は増やさない。
- 歯（`pipe_intake_depends_` 接頭辞・`tests/e2e/pipe/intake.rs`・toy repo の設計 doc に 2 行を commit する）: 2 行の doc は `tests/e2e/pipe/intake.rs` の行の helper（`table_row` / `table_region` / `table_doc`）と `derive_repo` で組む＝`tests/e2e/pipe.rs` の共有 helper は変えない。(1) 相手が同じ doc の**自分でない別の行**である行の受付が rc 0 で run dir が 1 つ出来る——base は断る＝RED（自分を指す `depends` は base でも解けて輪で断られるので fixture に使わない）。(2) 相手が doc に無い行は断られて run dir が 0・断りの 1 行が `contracts check` の描画（doc と行番号・`contract-table:depends-unresolved`・理由の文）と逐語で一致し、他の理由の行を伴わない（別の検査で先に落ちた偽の緑を除く）。(3) preflight が (1) の行で `refuse=` に `depends-unresolved` を出さず、(2) の行で出す。(4) は verify の既存の接頭辞 3 本（`pipe_intake_design_` / `pipe_preflight_` / `table_check_`）が緑のまま。
- 却下: 受付で表の検査を全行に撃って当該行の findings だけを残す（他の行の閉包と名指しまで毎回測る＝受付が doc の行数に比例して遅くなり、他の行の不備で無関係の便が断られる経路が出来る）／slice に渡す前に `depends` を空にする（(2) の断りが消える＝相手の無い `depends` が CI を通らず main に入った周に受付が黙って通す）／`depends` の key を schema から消す（既存の行が使っており、表の順序の宣言として CI の検査は働いている）。

## 31. 受付の門の判定式の生存 9 本に歯を足す（契約表の行 l・`s2-07l.277`・歯だけ・門の判定は 1 字も動かさない）

- 何が起きているか: `s2-07l.249` run 7 の検出線（gate 2026-09-14 14:55Z・母集団 166 = 撃墜 144 / 生存 9 / timeout 0 / unviable 13・C12.4 の検出線であって deny ではない）で、受付の門の判定式に生存 9 本が残った。生存 = **その分岐を pin する歯が無い**。本行は歯だけを足し、門の判定・断りの字面・rc は 1 字も変えない。
- **所在の測り直し（verified 2026-09-20・main f678bd0）**: 記録当時の所在（`pipe/cli.rs` / `pipe/closure.rs` / `pipe/declaration.rs` / `pipe/table.rs`）はその後の純移動（`s2-07l.279` / `.349` / `.363` / `.373`）で移った。今の所在と判定式は 5 群:
  1. **(a) 余地の段の除外の否定**（`crates/scribe2/src/pipe/cli/intake.rs` の private な `exclude_cap_shortfall(`）= 項目の解決に失敗した周に「解けない項目を**除いた**列」で数え直す否定の条件（`!` を落とすと解けない項目だけで数え直す）。
  2. **(b) 外形の usage 行の名の判定**（`crates/scribe2/src/pipe/closure.rs` の private な `usages(`）= 名が空か、識別子の文字と `-` 以外を含む行を飛ばす条件（`||` を `&&` にすると空の名が通る／`==` を `!=` にすると `-` を含む名が落ちる）。2 本の生存はこの 1 行の 2 つの演算子。
  3. **(c) 余地の境界**（`crates/scribe2/src/pipe/declaration/write_set.rs` の `pub fn headroom_shortfalls(`）= 見積が余地を**超える**ときだけ断る比較（`>` を `>=` にすると見積 = 余地ちょうどの便が断られる）。
  4. **(d) core の名の切り出し**（`crates/scribe2/src/pipe/declaration/write_set.rs` の private な `core_of(`）= crate 名が空でなく、かつ `/` を含まないときだけ core と読む連言（`&&` を `||` にすると `crates//src/` の形が core として通る）。
  5. **(e) 節の切り出し**（`crates/scribe2/src/pipe/table/check.rs` の private な `section_lines(`）= 契約表の区間の開始と終了の 2 つの腕（腕を落とすと区間の中身が節の本文に混ざる）・fence の中を見出しと読まない guard（`false` にすると fence の中の `## ` が節を切り替える）・本文を拾う条件の否定（`!` を落とすと fence の中だけを拾う）の 4 本。
- **歯**（接頭辞 `contract_closure_ext_survivor_`・生存 1 本に歯 1 本・**変異の A/B で撃墜されること**が done の条件）:
  - (a)(c)(d)(e) は**判定式を持つ file の in-file の歯**（(a) は `crates/scribe2/src/pipe/cli/intake.rs`・(c)(d) は `crates/scribe2/src/pipe/declaration/write_set.rs`・(e) は `crates/scribe2/src/pipe/table/check.rs`。判定式が private な純関数で、受付を通すと別の断りが先に立って分岐に届かない＝負例が別の理由で通る型を避ける）。接頭辞の後は `a_` / `c_` / `d_` / `e_begin_` / `e_end_` / `e_inside_` / `e_fence_`。
  - (b) は **e2e**（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）。`usages(` は `crates/scribe2/src/pipe/closure.rs` の私有で、`crates/scribe2/src/pipe/closure.rs` は R-C4-2 の余地が薄い（§29 の実測）ため in-file の歯を増やさず、`surfaces` を宣言した契約を受付に通して外形 pin の閉包の結果で測る。
  - **(b) の分岐の実測（2026-09-20・main 9a218c5）**: 生存 2 本が乗る 1 行は、名を捨てる条件（名が空である**か**、名の文字が識別子の文字でも `-` でもないものを含む）で、演算子の site は **3 つ**ある——外側の論理和・内側の論理和・`-` との等値。観察できる面は `surface_closure(` の結果 1 つ（名が `usages(` の列に無ければ `ClosureError::SurfaceUnknown` で受付が断り、在れば usage 文字列を literal に持つ file が導出値に入る）なので、3 site を**極性で 2 本に割る**:
    - `b_name_`（**肯定側**・2 例）= 素の英数字の名を持つ usage 行と、`-` を含む名を持つ usage 行。どちらも解けて、その usage 文字列を持つ歯の file が導出値に入る。**内側の論理和**を積に変える変異は素の名を落とし、**等値**を非等値に変える変異は `-` の名を落とす＝この歯が落ちる。
    - `b_match_`（**否定側**・1 例）= 識別子の文字でも `-` でもない文字を含む名は `SurfaceUnknown` で断られる（導出値が出ない）。**外側の論理和**を積に変える変異は「名が空でない」側が偽になって行が捨てられなくなり、**等値**を非等値に変える変異はその文字を通す＝どちらもこの名を解いてしまい、この歯が落ちる。
    - したがって 3 site のうち内側の論理和は**肯定側だけ**・外側の論理和は**否定側だけ**が受け、等値は**両方**が受ける（1 site 1 歯の対応にならないのはこの 1 行に 3 site が同居するため）。空の名の site は、名が空になる usage 行を fixture に置いても外側の論理和と同じ枝を通るだけなので、否定側の 1 例に畳む（空の名を `surfaces` に宣言する形は取らない＝行の欄の読み手が空の要素を落とすかどうかに歯を依存させない）。
  - **空虚さの柵**: どの歯も「母集団と件数を同じ assert で出す」（0 件を「変化なし」と読まない）・境界の歯は**両側**（余地ちょうど＝断らない／余地 −1＝断る）を持つ・(e) の 4 本は開始の腕と終了の腕を**別々に**落として別の歯が落ちること（1 本で 4 本を兼ねない）。
- **触らない**: 受付の判定・断りの型と字面・rc・rules 行・`crates/scribe2/src/pipe/closure.rs` の src（(b) の歯は e2e から測る）。
- **極性**: 歯だけの便なので検出線の母集団は 0 になる（§16 の gate-cost 側の行 g が的を宣言する形を持つまでは、撃墜の proof は便の notes に手で残す）。
- 却下案: 9 本を 1 本の歯にまとめる（どの分岐が撃墜されたか分からない・変異 1 本ずつの A/B ができない）／門の判定を「歯を書きやすい形」に直す（歯だけの便に仕様変更を混ぜる）／e2e だけで 9 本とも測る（(a)(c)(d)(e) は受付の手前の断りが先に立ち、負例が別の理由で通る）。
- flip-check の逃がし（`.277` run 2 の gate FAIL `green-on-base file=crates/scribe2/src/pipe/cli/intake.rs` の根）: 足す歯は既に着地した判定式を pin するので base でも緑である＝**各歯の fn の中の行頭に `// flip-check: retroactive s2-07l.277` の札を 1 行ずつ付ける**（in-file の 3 file は `mod tests {` の内側・e2e の file は全体が歯の区間。効く条件は test 区間内 / 行頭 / bead id 必須 / base に無い札、の 4 つ。札の無い歯は gate の flip-check が green-on-base で落とす・[seat-roles.md](./seat-roles.md) §23 と同じ形）。札は歯の緑を免じるのではなく「後から足した歯」と申告する印なので、done の変異の A/B（撃墜の本数と母集団）を notes に残すことが対になる。

## 32. 要件面 yaml の本文は `text:` → 無ければ `shall:` の順で読む（契約表の行 ae・`s2-07l.467`）

- **出所**: 別 project の席の要望（2026-09-17・急ぎでない・逐語は台帳 `s2-07l.467` の notes）。EARS 形の要件書（`when:` / `shall:` / `plain:` を持ち `text:` を持たない yaml）を正本にしている project は、§4 の審査の材料のためだけに読み物の html とその生成器を残している。本 repo の裁定（本節）は要望の推奨（本文の欄を順に読む）を採り、代替（`.vessel.toml` に欄名を宣言する行）を却下し、`when:` の連結を足す。
- **現物（verified 2026-09-20・main d875aaf）**:
  - yaml の要件面の本文の読み手は `crates/scribe2/src/pipe/review.rs` の **private** な `requirement_yaml(`（引数は要件面の全文と id の 2 つ・戻り値は閉じた 3 値〔`Found::Body` / `Found::Empty` / `Found::Absent`〕）。
  - 呼び手は同じ `crates/scribe2/src/pipe/review.rs` の private な `requirements_text(`（repo と要件面の path と id の列を取り、id 1 つにつき 1 行の文字列を返す）**1 か所だけ**で、要件面の形の呼び分けは §4 のとおり**拡張子の 1 match**（`.html` = `requirement_row(`・`.yaml` / `.yml` = `requirement_yaml(`・`.md` = `requirement_md(`）から関数 pointer を選ぶ形である。
  - `requirement_yaml(` の本文の組み立ては**1 本の loop**で、id の mapping の行を走りながら「いま読んでいる欄が `text:` か」の印を立て、値と block の続きを集め、最後に**空白 1 つで繋いで空白を畳む**。`title:` は読まない。裸の列と `text:` の無い mapping は `Found::Empty`、id が無ければ `Found::Absent`。
  - 既存の歯は `crates/scribe2/src/pipe/review.rs` の in-file の歯 `pipe_review_requirements_text_reads_yaml_text_and_md_headings_by_extension` 1 本で、yaml の fixture の 5 つの id（`text:` の値・block の続き・裸の列・`title:` だけ・不在）を測る。
- **約束（番号は done と歯の対）**:
  1. `text:` が在る mapping の本文は**今までどおり `text:` の値**（値の選び方も block の畳み方も 1 字も変えない）。
  2. `text:` が無く `shall:` が在る mapping の本文は **`shall:` の値**にする（block の続きの畳み方は `text:` と同じ 1 本の形を通す）。
  3. 2 の周に同じ mapping の `when:` が在れば、本文は **`when:` の値 + 区切り + `shall:` の値**の 1 本にする（EARS 形の要件は条件を落とすと審査役が約束の範囲を誤る）。**区切りの字面は前後に空白 1 つを伴う `—`（em dash）1 文字**に決める——本文の組み立ては値と block の続きを空白 1 つで繋いで畳む形なので、空白だけでは `when:` と `shall:` の境が消える（現物の組み方に合わせた 1 つの選択）。
  4. `plain:` は読まない。`title:` も従来どおり読まない。`when:` だけを持ち `shall:` を持たない mapping からは本文を作らない。
  5. `text:` も `shall:` も無い mapping と裸の列は**従来どおり `Found::Empty`**、id が要件面に無い周は `Found::Absent`＝呼び手が出す「本文が無い」と「要件面に無い」の行の字面は 1 字も変わらない。
  6. 欄の名と順序は `requirement_yaml(` の中だけが持つ（`.vessel.toml` にも rules 行にも宣言を足さない・規則を増やさない・C17）。読み手は形ごとに 1 関数のままで、html と md の読み手・拡張子の 1 match・関数 pointer の型・3 値の型・呼び手の行の組み立ては不変。
- **歯**（`pipe_review_yaml_shall_` 接頭辞・置き場は `crates/scribe2/src/pipe/review.rs` の in-file の歯・fixture は歯の中で組む yaml の字面）:
  - (a) `shall:` だけを持つ mapping の本文が `shall:` の値になる（約束 2）。
  - (b) `when:` と `shall:` を持つ mapping の本文が「`when:` の値 + 空白 + `—` + 空白 + `shall:` の値」の 1 本になる（約束 3・**区切りの字面を逐語で pin** し、`when:` の値が落ちていれば落ちる）。
  - (c) `text:` と `shall:`（と `when:`）を両方持つ mapping の本文が `text:` の値だけになり、**`shall:` の値も `when:` の値も 1 字も混ざらない**（約束 1 の優先・**否定の枝**）。
  - (d) `when:` だけの mapping・`plain:` だけの mapping・`title:` だけの mapping・裸の列はどれも `Found::Empty`（約束 4 と 5 の**否定の枝**＝`when:` だけで本文を作らず `plain:` を読まない）。
  - (e) id が要件面に無い周は `Found::Absent`（約束 5）。
  - (f) `shall: |` の block の続きを持つ mapping の本文が空白で畳まれた 1 本になる（約束 2 の block の面・`text:` の block と同じ扱い）。
  - 約束 1 の「1 字も変えない」は**既存の歯**が受け、行の verify が完全名 `pipe_review_requirements_text_reads_yaml_text_and_md_headings_by_extension`（同じく `crates/scribe2/src/pipe/review.rs` の in-file の歯）で撃つ。
- **触らない**: html の読み手 `requirement_row(` と md の読み手 `requirement_md(`・拡張子の 1 match と関数 pointer の型・3 値の型・呼び手 `requirements_text(` の行の組み立てと断りの字面・id の集合の読み手 `requirement_ids(`・`.vessel.toml` の欄・rules 行・§4 の審査の段の形（本節が広げるのは §4 の「yaml の `id` + `text`」の句の yaml の面だけで、html と md の句は不変）。
- **却下案**: `.vessel.toml`（か rules 行）に本文の欄名を宣言させる案は、宣言の読み手と断りが増え要件面 1 つのために project ごとの設定面が育つため不採用（規則を増やさない・C17）。`plain:` も順に読む案は、平易化の欄であって約束の正本ではなく、審査役に渡る材料が緩むため不採用。`when:` を落として `shall:` だけを本文にする案は、条件を落とすと審査役が約束の範囲を誤るため不採用。欄の順序を呼び手（拡張子の match）の側に持つ案は、yaml の本文の規則が 2 か所に分かれるため不採用（C2）。

## 45. 契約表の検査が Declared 行の歯の置き場を数える — verify の filter 語が write-set の外の歯に解ける行を、まず判定行の本数（検出線）で出し、次に findings に上げる（契約表の行 av / aw・`s2-07l.552`）

- 出所: `s2-07l.441` の (2) の草稿の申し送り 3（逐語は台帳 `s2-07l.552`）。契約表の Declared 行（write-set と verify を持つ行）のうち、verify の filter 語で解ける歯の file を write-set の外に持つ行が main に在ったが、契約表の検査は置き場の門を回さないので findings 0 のままだった。
- 実測（2026-09-22 main 3b258a3 → 2026-09-23 main c5738a2・verified）: 母集団 = Declared 行 192。verify の filter 語を nextest の一覧（2093 本）に当て、解けた歯の file を write-set と突き合わせて当たった行 = 5（gate-cost の g・pipeline の a と j・rules-manifest の h・seat-roles の m）。5 行は filter を歯の名の全体の接頭辞へ絞るか歯の置き場を write-set に足す形で直した（PR #591）。粗い走査（`-E` の式や tests 欄を読まない）では 18 行＝器の導出（§28 の scope・`-E` の不読・`--exact`）で数え直すと母集団が変わりうる。
- 現物（main c5738a2・verified）: 置き場の照合は `crates/scribe2/src/pipe/closure/derive.rs` の `teeth_places`（受付と preflight が撃つ・歯の fn 名が filter 語を含むかを §28 の scope で見る）。契約表の検査 `crates/scribe2/src/pipe/table/check.rs` の `judge_repo` は行ごとに requirement / verify / write-set / closure / name の findings を集めるが、`teeth_places` は呼ばない。判定行は `check_repo` の 1 行（`docs= rows= untracked= findings=`・名乗りの周だけ `entrance=` の欄）。`contracts check` の引数の読みは `crates/scribe2/src/pipe/cli.rs` の `contracts`（`--repo` 必須・`--rules` 任意・`--verbose` の旗は無い）で、使い方の文字列 `contracts_usage` も同じ file に在る（e2e の歯は `contracts <check` の字面を含むことだけを pin する）。`teeth_places` は `crates/scribe2/src/pipe/closure.rs` が `pub(crate) use` で crate 内へ出し（`table/check.rs` から呼べる）、入力の `Fields` / `Base` は pub。受付の組み立て `fields_of` / `base_of` は `crates/scribe2/src/pipe/cli/intake.rs` の private fn で、`judge_repo` の `Context` は同じ材料（sources / snapshots / tracked）を既に持つ。
- 形（行 av・検出線）:
  1. `judge_repo` が Declared 行ごとに `teeth_places` を撃ち、解けた歯の file のうち write-set（`+` / `-` / `~` / `=` を剥がした集合）の外の file を持つ行を数える。解けない行（`TeethPlaceUnresolved` 相当）は数えず、既存の closure の findings に任せる。
  2. 判定行の末尾に `place-out=<行数>/<Declared 行数>` の欄を足す（母集団つき・C10）。findings には上げない（rc は変えない）。判定行を全行で pin する既存の歯は e2e の `crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` に 16 本と `crates/scribe2-boundary/tests/e2e/pipe/intake.rs` に 1 本（`assert_eq!` の字面・末尾を `findings=0` で読む 2 本を含む）が在り、欄の追加ぶん字面を進める＝行 av の write-set と verify に載る（verify の filter 語は歯の名の全体か、e2e の他 file に substring で当たらない接頭辞だけ・`prop.rs` の性質の歯が `contract_closure_ext_` を名の途中に持つ）。
  3. 数えた行の id と file は `--verbose` の周だけ 1 行ずつ出す（doc・行 id・file）。`--verbose` の旗は行 av が `crates/scribe2/src/pipe/cli.rs` の `contracts` に足す（`check_repo` の引数を 1 つ増やし、使い方の文字列にも載せる）。`Fields` / `Base` は `Context` の材料から受付の `fields_of` / `base_of` と同じ形を check.rs が自前で組む（private fn は共有しない）。
  4. 実測（行 av の便の木・main eb2a5e1 の上・2026-09-23）: 現物の契約表の判定行は place-out=6/210（当たった行は contract-source の a / c / g / j / n / ah・どれも verify の filter 語が歯の名の接頭辞として広く、write-set の外の in-file の歯の file に解ける）。行 aw の前提（main で place-out=0）はこの 6 行を直すまで満たない。6 行の直し（2026-09-23・main 57f27e6 の上）: a / c / g / j / ah は歯の file を write-set に足し（landed 行なので素の path）、n は filter 語を write_set.rs に閉じる 2 つ（`declaration_write_set_` / `declaration_headroom_`）に絞った。直した木で `--verbose` の判定行は place-out=0/210（当たった行 0・母集団 210）。
- 形（行 aw・findings への昇格・行 av の着地後に main で `place-out=0/<n>` を実測してから）:
  1. 行 av の数えた行を findings の 1 語（TeethOutsideWriteSet・doc・行 id・file）に上げ、判定行の欄はそのまま残す。
  2. 現物の契約表の歯（`contract_closure_ext_real_table_has_zero_findings`）が 0 件のまま緑であることを同じ便で確かめる（母集団 = 行 av の判定行の分母）。
  3. 語 TeethOutsideWriteSet は `crates/scribe2/src/pipe/table.rs` の閉じた列挙 `TableError` の variant（語は `as_str`・網羅 match は table.rs と `crates/scribe2/src/pipe/table/parse.rs`・語の一覧を宣言順で pin する歯 `table_error_names_are_pinned_in_declaration_order_and_carry_their_line` の const `TABLE_ERRORS` は table.rs の in-file の歯・14 値・行 aw の verify が名の全体で名指す）＝行 aw は touches に型を名指し、write-set に table.rs と parse.rs と、touches の閉包が名指す `crates/scribe2/src/pipe/cli/intake.rs` と `crates/scribe2/src/pipe/refuse.rs`（型を構造として持つ file・contracts check の write-set-incomplete で実測）を持つ。
  4. 実装（行 aw の便・2026-09-23）: 置き場の検出線の測り（`judge_doc` の同じ本文）が doc ごとに当たった行を TeethOutsideWriteSet の 1 件ずつ返し、findings に足して行番号の順に並べ直す（行番号は行の見出しの行・rc 1・`contracts: <doc>:<行> contract-table:teeth-outside-write-set: 行 <id> の歯の file が write-set の外: <file>`）。閉包の入力を読めない周は 0 件（欄は `?`・読めなさは既存の closure の findings が名指す）。`repo_findings` も同じ 1 本を通るので追随の後の検査も同じ 1 件を行 id で名指す。TableError の網羅 match（`line` / `as_str` / `reason`）は table.rs だけに在り、parse.rs は variant を構築するだけで match を持たない（実測）＝parse.rs は字面が動かない。現物の判定行は findings=0 place-out=0/210（本便の木）。
- 触らない: `teeth_places` の述語と scope の読み・受付と preflight の断り・`=` の意味（§43 (1)）・`--exact` の読み（§43 (2)）・判定行の既存の欄の並び。
- 却下: 直接 findings に上げる（器の導出が粗い走査の 18 行側に倒れると現物の歯が赤になる＝母集団を判定行で先に見る）／verify 行の tests 欄を必須にする（宣言の形の門が変わる・別便）／置き場の門を受付だけに残す（landed 行の drift を誰も数えない）。
- 歯: in-file（`crates/scribe2/src/pipe/table/check.rs` の `mod tests`・接頭辞 `contract_check_place_`・`crates/` 全体で 0 件。同じ `mod tests` の既存の歯 13 本の接頭辞は `table_check_` / `contract_closure_ext_` / `contract_promise_` / `contracts_untracked_` で、`contract_check_` の歯は base に無い）で、tmp の repo に Declared 行 3 本（歯が write-set の内 / 外 / 解けない）を置いて撃ち、判定行が `place-out=1/3` を出し、rc と findings が変わらず、`--verbose` で当たった行の id と file が 1 行出ること。旗の読み（`cli.rs` の `contracts` と使い方の文字列）は private なので、e2e の歯 1 本（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`・接頭辞 `contract_check_place_verbose_`）が binary を撃って測る: 同じ 3 行の tmp の repo で `--verbose` の周は外の行の 1 行が stdout に出て旗の無い周は 0 行、引数の無い周の使い方の文字列に `--verbose` が載る。既存の lib の歯 13 本は verify 行が名の全体か check.rs に閉じる接頭辞（`table_check_` / `contracts_untracked_`）で名指す（`contract_promise_` と `contract_closure_ext_` は他の src file の歯の名に substring で当たるので名の全体）。行 aw は同じ族に `contract_check_place_finding_` の接頭辞で、外の行が findings 1 件・語 1 つ（doc・行 id・file）になり、判定行の欄が残ること。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "契約表の parser と検査（contracts check）・台帳の pointer 形"
req = ["FR47", "FR48", "FR54", "FR55"]
section = "2"
touches = ["crate::pipe::refuse::Refuse", "crate::polarity::Guard"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2/src/polarity.rs", "contracts/schema.toml", "crates/xtask/src/check.rs", "crates/xtask/src/check_facts.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/", "crates/scribe2-boundary/src/snapshots/", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/table/parse.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_"]
size = "M"
done = "contracts check が本 doc の区間を全件通し、閉包が足りない fixture を名指す"

[[contract]]
id = "b"
title = "intake の生成（--design）と --contract の廃止"
req = ["FR53", "FR54", "FR48", "FR39"]
section = "2"
touches = ["crate::pipe::refuse::Refuse"]
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_intake_design_"]
size = "L"
done = "toy repo の設計 doc の行から契約 file が生成されて run dir に載り、--contract は typed な断り（hand-written-contract）で拒まれ、行の欠陥（閉包 / 節 / 解けない pointer）は run を作らずに断られる"
depends = ["a", "c"]

[[contract]]
id = "c"
title = "契約の審査の段（Stage::Reviewed・lens-contract.txt・review.json）"
req = ["FR49", "FR9"]
section = "4"
touches = ["crate::fleet::Stage", "crate::polarity::Guard"]
write-set = ["+crates/scribe2/src/hook/live_row.rs", "crates/scribe2/src/fleet/mod.rs", "+crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2/src/pipe/regate.rs", "+crates/scribe2/src/pipe/follow_step.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/health.rs", "+crates/scribe2/src/ledger/memo.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/polarity.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "+crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "docs/design/contract-source.md", "crates/scribe2/src/pipe/review/base.rs", "+crates/scribe2/src/pipe/review/outside.rs", "+crates/scribe2/src/fleet/phase.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_review_"]
size = "S"
done = "偽 lens FAIL で構築点の呼出 0・PASS で Spawned（既存の core 10 面は各 +100 行以内・本体は新規 pipe/review.rs ≤ 500 行）"
depends = ["a"]

[[contract]]
id = "d"
title = "land の終端（push・CI の照合・台帳の close）と rules 行 pipe.ci_wait_s"
req = ["FR50", "FR12"]
section = "5"
touches = ["crate::fleet::Completion", "crate::rules::RuleKind", "crate::polarity::Guard"]
write-set = ["rules/manifest.toml", ".vessel.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2/src/pipe/stop.rs", "+crates/scribe2/src/pipe/health.rs", "+crates/scribe2/src/pipe/land/finish.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_land_"]
size = "M"
done = "偽 remote + 偽 CI + 偽 adapter で Landed → close の 3 event・failure は close しない・consumer の要件面の path を宣言で受ける（拡張子で読み手分岐・無ければ既定・既定も無ければ断る）"
depends = ["b"]

[[contract]]
id = "e"
title = "台帳 lint（doctor の項目 1 行）— open の bead から pointer の解けない契約・pointer 無しの memo・本文を持つ契約を件数と母集団と id で名指す"
req = ["FR51"]
section = "6"
write-set = ["crates/scribe2/src/ledger/mod.rs", "+crates/scribe2/src/ledger/lint.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2/src/seat/ledger.rs", "+crates/scribe2-boundary/tests/e2e/ledger.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "+crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail ledger_lint_", "cargo nextest run -p scribe2 --bin scribe2 --no-tests=fail ledger_lint_"]
size = "S"
done = "doctor の項目に台帳の 1 行が増え、偽の台帳 client の出力で 3 つの欠陥（pointer の解けない契約・pointer 無しの memo・本文を持つ契約）の件数と母集団が同じ行に出て欠陥の bead の id が名指され、欠陥 0 の周も 0 と母集団が出て行が消えず、client が起動できない・rc ≠ 0・出力が壊れた周は 0 でなく測れていない形の行になり、pointer の解ける契約と見出しの無い memo は数に入らず、極性の列は増えない"
depends = ["d"]

[[contract]]
id = "f"
title = "設計 doc の散文の門（暫定床）— 規範の印を持つ文は pointer を持ち数 + 単位を持たない（xtask check の 1 項目・folio2 M1 が同じ式を引き取ったら撤去）"
req = ["FR52"]
section = "12"
write-set = ["crates/xtask/src/prose_gate.rs", "crates/xtask/src/main.rs", "crates/xtask/src/check.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail prose_gate_"]
size = "S"
done = "docs/design の現物で違反 0 件・fixture の違反 2 件を file:line 付きで名指して非 0"

[[contract]]
id = "g"
title = "閉包の拡張 — 外形 pin（第 5 形・surfaces）・write-set 項目の実在と dir 展開・上限の余地・名指しの実在"
req = ["FR48", "FR39", "FR54", "NFR4"]
section = "3"
touches = ["crate::pipe::refuse::Refuse", "crate::rules::RuleKind"]
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli.rs", "contracts/schema.toml", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/declaration/write_set.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_"]
size = "M"
done = "surfaces / dir 展開 / 余地 / 名指しの 4 fixture が typed に断られ、現物の契約表で違反 0"
depends = ["a"]

[[contract]]
id = "h"
title = "write-set の導出 — touches の閉包 + verify の歯の置き場（base の実測）+ surfaces + creates / tests / also から器が write-set を作り、手書きの write-set は受付で導出値との集合一致だけを認める（Landed 後の intake から効く）"
req = ["FR48", "FR39", "FR47"]
section = "3"
touches = ["crate::pipe::refuse::Refuse", "crate::pipe::closure::ClosureError", "crate::pipe::table::ContractRow"]
write-set = ["+crates/scribe2/src/pipe/table/check/collide.rs", "+crates/scribe2/src/hook/live_row.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/contract.rs", "contracts/schema.toml", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure/names.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_derive_"]
size = "M"
done = "write-set 無しの fixture 行が導出値で intake を通り契約 file に導出値が載る・手書きが導出値とずれた fixture 行は missing / extra を名指して断られる・also の .rs / tests の非歯 file / 解けない filter は typed に断られる・schema.toml に creates / tests / also が載り write-set が任意・現物の契約表の intake 済みの行は断られない（受付だけ・CI は従来の閉包）"
depends = ["g"]

[[contract]]
id = "i"
title = "契約表の + は land すると解けなくなる — 契約表の検査では tracked な + を実在 file と読む（intake は断ったまま・閉じた型の値 1 つ）"
req = ["FR48", "FR39"]
section = "3"
write-set = ["crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_table_landed_plus_"]
size = "S"
done = "land 済みの + 項目を持つ行が contracts check で 0 件・intake は同じ行を断ったまま"
depends = ["a"]

[[contract]]
id = "j"
title = "閉包の 4 形が同名の型の file へ広がる — sees() 1 関数で「その file から型が見えているか」を判定してから数える"
req = ["FR48", "FR39"]
section = "3"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", "docs/design/contract-source.md", "crates/scribe2/src/pipe/table/check.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_same_name_"]
size = "S"
done = "別 module の同名の型を持つ toy で閉包が広がらず、見えている file だけが導出値に入る"
depends = ["a"]

[[contract]]
id = "k"
title = "要件本文の読み手を要件面の形ごとに — yaml の id + text と md の見出しを lens の材料に載せ、consumer の審査が要件面を読めないで終端しない"
req = ["FR49", "FR2"]
section = "4"
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_check_reads_requirements_ pipe_review_reads_requirements_"]
size = "S"
done = "yaml / md の要件面を宣言した toy で contracts check の id 検査と lens の要件本文が同じ読み手を通り、本文の無い id は理由付きで材料に載る"

[[contract]]
id = "l"
title = "受付の門の判定式の検出線の生存 9 本に歯を足す — §31 の (a) 余地の段の除外の否定 1 本・(b) 外形の usage 行の名 2 本・(c) 余地の境界 1 本・(d) core の名の切り出し 1 本・(e) 節の切り出し 4 本の計 9 本を in-file と e2e で赤にする（歯だけ・門の判定は動かさない）"
req = ["FR48", "FR47"]
section = "31"
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_a_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_closure_ext_survivor_b_name_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_closure_ext_survivor_b_match_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_c_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_d_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_begin_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_end_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_inside_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_fence_"]
size = "S"
done = "§31 の (a)〜(e) の生存 9 本それぞれに歯が在り、その分岐を変異させた A/B で撃墜される（撃墜の本数と母集団を notes に残す）。(a)(c)(d)(e) は 1 site 1 歯で対応する歯だけが落ち、(b) は 1 行に同居する 3 site を極性で 2 本に割る＝b_name_ は肯定側（素の名と - を含む名が解ける）で内側の論理和と等値の変異に落ち、b_match_ は否定側（識別子でも - でもない文字を含む名が SurfaceUnknown で断られる）で外側の論理和と等値の変異に落ちる。境界の歯は余地ちょうどと余地 −1 の両側を持ち、節の切り出しの 4 本は開始の腕と終了の腕を別々に落として別の歯が落ち、各 assert が件数と母集団を同じ行に出す。受付の判定・断りの型と字面・rc・rules 行は 1 字も変わらない"

[[contract]]
id = "m"
title = "pipe/closure.rs の write-set の導出（weighted_lines / Fields / Base / derive_write_set / check_drift / teeth_places）を pipe/closure/derive.rs へ割る — 純移動・呼び手は pub use で不変"
req = ["FR48", "FR39"]
section = "3"
write-set = ["-crates/scribe2/src/pipe/closure.rs", "+crates/scribe2/src/pipe/closure/derive.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe::closure::derive::"]
size = "S"
done = "closure.rs の余地が 400 行以上に戻り、導出の関数と歯が derive.rs に移って本数と中身が不変、呼び手の use は動かず、gate の lens 入力が要約"

[[contract]]
id = "n"
title = "pipe/declaration.rs の write-set の項目の読みと上限の余地の群を declaration/write_set.rs へ割る — 純移動・呼び手は pub use で不変・札 moved"
req = ["FR48"]
section = "14"
write-set = ["-crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/pipe/declaration/write_set.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail declaration_write_set_", "cargo nextest run -p scribe2 --no-tests=fail declaration_headroom_"]
size = "S"
done = "群の 11 item と対応する歯 5 本が子 module に在り、親は mod 宣言と pub use / pub(crate) use だけが増えて呼び手の import は不変、既存の declaration_ の歯が全部緑で純移動の機械証明の残差が use と path だけ"

[[contract]]
id = "o"
title = "pipe/table.rs を table/parse.rs（区間と TOML の parse）と table/check.rs（検査の本体・要件面・CLI の駆動）に割る — TableError は親に残す・純移動・札 moved"
req = ["FR47"]
section = "15"
write-set = ["-crates/scribe2/src/pipe/table.rs", "+crates/scribe2/src/pipe/table/parse.rs", "+crates/scribe2/src/pipe/table/check.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail table_ polarity_external_form"]
size = "S"
done = "parse と check の群が子 module に在り、TableError / Finding / Context は親に残って極性一覧の snapshot が不変、親は mod 宣言と pub use だけが増えて呼び手の import は不変、既存の table_ と contract_ の歯が全部緑で純移動の機械証明の残差が use と path だけ"

[[contract]]
id = "p"
title = "surface_closure の literal 探索を歯の区間だけに限る — 導出の実装 file 自身を pin file に数える自己言及を止める"
req = ["FR48"]
section = "16"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_surfaces_"]
size = "S"
done = "外形を触る契約の導出値に導出の実装 file が入らず、閉包の便との偽の交差が消える"

[[contract]]
id = "q"
title = "write-set の導出に (vi) creates の親 mod の宣言 file と (vii) subcommand の閉じた enum（SeatCommand / PipeCommand）を足す"
req = ["FR48"]
section = "17"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_derive_creates_parent_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_derive_subcommand_enum_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_command_all_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_command_all_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "M"
done = "creates を宣言した行の導出値に §17 (vi) の 3 形とも親の宣言 file が入る＝(vi-1) <dir>/mod.rs の周と (vi-2) <dir>.rs の周に加えて、(vi-3) 項目が crates/<c>/src の直下である周は同じ dir の lib.rs と main.rs のうち base の tracked に在るものが全部入り（両方 tracked の crate は 2 面・main.rs だけの crate は 1 面）、親の候補がどれも base に無い周と .rs でない項目と dir を持たない項目は 1 面も足さない。seat と pipe の cli の既知の verb が閉じた enum と別名の const slice になって件数と宣言順が型と一致し往復し未知の token は parse が None を返し、口座 label の短い形の腕と各腕の rc は不変で、cli の型を touches に宣言した行の導出値に cli.rs と自分の件数 pin の歯の file だけが入り（もう一方の cli の歯の file は入らない）、usage の字面と 2 つの外形 snapshot は 1 字も変わらない"

[[contract]]
id = "r"
title = "write-set の導出に fn 形の touches（crate::module::snake_ident）を足し、その fn を宣言する file を閉包に入れる"
req = ["FR48"]
section = "18"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_derive_fn_"]
size = "S"
done = "touches に fn 形を書いた契約の導出値にその fn を宣言する file が入り、宣言する file が無い周は typed に断られ、型形の閉包は不変"

[[contract]]
id = "s"
title = "write-set の導出に enum の variant 構築（<Type>::<Variant> { / ( を => の右辺や Err(…) の中で作る file）の第 6 形を足す"
req = ["FR48"]
section = "19"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail closure_variant_construction_", "cargo nextest run -p scribe2 --no-tests=fail closure_picks_each_of_the_four_forms_from_its_own_file", "cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_real_table_has_zero_findings"]
size = "S"
done = "(1) variant 構築だけを持つ file が導出値に入り、(2) Self:: の構築だけの file・小文字始まりの項目だけの file・型が見えていない同名の file は入らず => の左のパターン側の出現を述語が数えず、(3) 既存 4 形の導出値は不変で、(4) 第 6 形で閉包が広がる本 doc の行 a / b / d / g / h / w の write-set に 19 項目を同じ便で追記して contracts check が findings 0 のまま"

[[contract]]
id = "t"
title = "Declared 行にも歯の置き場の門を撃つ — verify の filter 語が base で解く歯の file が write-set に無ければ file を名指して断る"
req = ["FR48"]
section = "20"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_declared_teeth_"]
size = "S"
done = "Declared 行の verify の歯の file が write-set の外に在る契約を受付が file を全部名指して断り、中に在る契約と nextest 形でない verify の契約は従来どおり通る"

[[contract]]
id = "u"
title = "pipe preflight — 受付の判定を judge / create に割り、judge だけを run を作らず撃って断りと事実を全部 1 行 1 事実で出す口"
req = ["FR48"]
section = "21"
write-set = ["+crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "-crates/scribe2/src/pipe/closure.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_preflight_", "cargo nextest run -p scribe2 --no-tests=fail pipe_external_form"]
size = "M"
done = "preflight が run dir も event も作らずに受付と同じ断りを全部列挙して rc 0 / 1 / 2 を返し、intake の断りの先頭 1 件と一致し、usage に preflight が載る"

[[contract]]
id = "v"
title = "審査の理由を閉じた型 FindingKind で review.json と event に残し、pipe report が by_kind で数える"
req = ["FR49"]
section = "22"
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/pipe/report.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_kind_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_lens_contract_prompt_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_report_counts_human_events", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_report_counts_landed_runs_not_landed_events"]
size = "M"
done = "FAIL / INCONCLUSIVE の周の review.json が閉じた 6 語の kind と at を任意 field で持ち、同じ周の event の detail が verdict:<V> kind:<k> の 2 語になり（at は event に載せない）、PASS の周は review.json に kind も at も持たず detail が verdict:PASS のままで、kind の無い・語でない・JSON が読めない周と器が作る INCONCLUSIVE 5 形は 7 語目 unparsed に倒れて verdict は lens の値のまま、report の 1 行に review_fail= が母集団つきで出て by_kind= が宣言順に 7 語とも（0 も）出て古い event は unparsed に数えられ、lens の雛形の外形 snapshot に kind と at の穴が写り、report の既存 token と verdict の 3 値と rc は不変"

[[contract]]
id = "w"
title = "同型の審査 FAIL が rules 行 review.same_kind_stop の回数に達した bead の材料不変の run N+1 を same-kind-repeated で断り、直前の at に対応する差分の無い焼き直しを finding-unaddressed で断る"
req = ["FR49"]
section = "23"
touches = ["crate::pipe::refuse::Refuse", "crate::rules::RuleKind"]
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/rules-manifest.md", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_repeat_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_review_same_kind_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_external_form"]
size = "M"
done = "同じ kind の FAIL が行の値の本数続いた bead の材料不変の intake は same-kind-repeated で断られて便 id の列を運び、契約 file か節の本文のどちらかが変われば通り、kind の違う 2 便と unparsed の 2 便は通り PASS を挟むと数え直し、直前の at に対応する差分の無い契約は finding-unaddressed で断られて対応の無い項目だけを辞書順で名指し、測れない 4 型（goal-done-contradiction / vacuous-assert / other / unparsed）と at の空な周は通り、どちらの断りも run dir と event を作らず preflight にも出て、行の無い manifest は rc 2 で行を名指し、rules 行 review.same_kind_stop が裁定 id 付きで 1 本増えて RuleKind の variant と対になり外形の rows= と kinds= が 1 つ増える"
depends = ["v"]

[[contract]]
id = "x"
title = "着地で消える file の宣言 — write-set の項目の ~ 接頭辞（受付は base の実在を要し、契約表の検査は無ければ着地で消えたと読み、名指しの実在から外す）"
req = ["FR48", "FR55"]
section = "24"
touches = ["crate::pipe::declaration::write_set::WriteSetItem", "crate::pipe::refuse::normalize", "crate::pipe::cli::intake::exclude_cap_shortfall", "crate::pipe::closure::unresolved_names"]
tests = ["crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_delete_"]
size = "S"
done = "~ の項目が base に在る行は受付を通り契約 file の write-set は素の path になり、無い行は write-set-item-unresolved で断られ、契約表の検査は tracked に無い ~ の項目を持つ行で findings 0、節の本文のその path の名指しは着地の後も解ける"

[[contract]]
id = "y"
title = "契約表の名指し検査が struct-like variant の literal 形と引数付きの呼出し形を先頭の token で読む — 散文扱いの偽陰性を塞ぐ"
req = ["FR54"]
section = "25"
touches = ["crate::pipe::closure::Form"]
tests = ["crates/scribe2/src/pipe/closure.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_name_form_"]
size = "S"
done = "Type::Variant { field: X } の字面が型の path 形として base に無ければ name-unresolved になり、f(x) が fn 形 f として解け、大文字始まりの X(…) と pub(crate) と末尾 :: の module path と glob と属性は従来どおり散文で、現物の契約表は findings 0 のまま"
depends = ["t"]

[[contract]]
id = "z"
title = "名指しの実在の型の path 形を impl の block 経由でも解く（method / 関連 fn の偽陽性を閉じる）"
req = ["FR54", "FR55"]
section = "26"
touches = ["crate::pipe::closure::Form"]
tests = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_impl_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_names_impl_"]
size = "S"
done = "impl する file が宣言する fn の型の path 形が解け、fn の無い項目と impl 行の無い file の同名 fn は name-unresolved のまま・現物の契約表は違反 0 のまま"
depends = ["y"]

[[contract]]
id = "aa"
title = "（消した・s2-07l.476）受付が契約の散文（goal / done）の歯の名指しを verify の filter と write-set に突合し、判定行 token の pin（第 7 形）を閉包に足す"
req = ["FR48"]
section = "27"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_prose_teeth_", "cargo nextest run -p scribe2 --lib --no-tests=fail prose_closure_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_derive_reasons_are_last_and_name_their_payload"]
size = "M"
done = "（消した・s2-07l.476・user 裁定 2026-09-18 07:4xZ）受付は契約の散文（goal / done）を走査せず、歯の名指しの被覆と判定行 token の pin の門は無い。行の verify と write-set は歴史として残す"

[[contract]]
id = "ab"
title = "歯の置き場が verify 行の scope を閉じた 3 値で読む — --test <name> の行は統合 test の file だけ、--lib の行は src の file だけを置き場に数え、読めない旗と複数の旗は従来どおり crate 全体"
req = ["FR48"]
section = "28"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail closure_scope_"]
size = "S"
done = "--test の行が統合 test の file だけを、--lib の行が src の file だけを置き場に返し、旗なしと読めない旗と 2 つ以上の旗の行は crate 全体のまま、0 本の行は従来の字面で断られ、現物の契約表は findings 0"

[[contract]]
id = "ac"
title = "pipe/closure.rs の名指しの解決の群（unresolved_names / Form 系 / backticked と歯 closure_names_）を closure/names.rs へ割る — 純移動・ClosureError と 4 形と surface_closure は親に残す・呼び手は pub use で不変・札 moved"
req = ["FR48"]
section = "29"
write-set = ["-crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/names.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_"]
size = "S"
done = "名指しの解決の群 9 item と歯 3 本と fixture が子 module に在り、親は mod 宣言と pub use / pub(super) use だけが増えて呼び手の import は不変、既存の closure_ と contract_ の歯が全部緑で極性一覧の snapshot が不変、純移動の機械証明の残差が use と path と可視性の 1 語だけ"

[[contract]]
id = "ad"
title = "受付は depends の相手を同じ doc の全行の id から解く — 検査する行は 1 つのまま・相手の無い depends は従来どおり断る"
req = ["FR48", "FR54"]
section = "30"
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/table.rs", "=crates/scribe2/src/pipe/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_intake_depends_", "cargo nextest run -p scribe2 --no-tests=fail pipe_intake_design_", "cargo nextest run -p scribe2 --no-tests=fail pipe_preflight_", "cargo nextest run -p scribe2 --no-tests=fail table_check_"]
size = "S"
done = "depends の相手が同じ doc の自分でない別の行である行を受付が rc 0 で受けて run dir が 1 つ出来、相手が doc に無い行は contracts check と逐語で同じ depends-unresolved の 1 行だけで断られて run dir が 0、preflight は（受付と同じ判定関数を既に直に呼んでいるので source を変えずに）前者で refuse= に depends-unresolved を出さず後者で出し、既存の pipe_intake_design_ / pipe_preflight_ / table_check_ の歯が緑のまま"

[[contract]]
id = "ae"
title = "要件面 yaml の本文を text: → 無ければ shall: の順で読み、shall: の周に when: が在れば条件と一緒に 1 本にする（欄の宣言は足さない）"
req = ["FR49", "FR2"]
section = "32"
write-set = ["crates/scribe2/src/pipe/review.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_yaml_shall_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_requirements_text_reads_yaml_text_and_md_headings_by_extension"]
size = "S"
done = "約束 1 = text: を持つ mapping の本文は今までどおり text: の値で、既存の yaml と md の歯が 1 字も変わらず緑。約束 2 = text: が無く shall: が在る mapping の本文が shall: の値になり、shall: の block の続きも空白で畳まれた 1 本になる。約束 3 = その周に同じ mapping の when: が在れば本文が when: の値 + 空白 + em dash + 空白 + shall: の値の 1 本になり、区切りの字面を逐語で測る歯が when: の値の脱落で落ちる。約束 4 と 5 = text: と shall: を両方持つ mapping は text: の値だけになり shall: と when: の値が 1 字も混ざらず、when: だけ / plain: だけ / title: だけの mapping と裸の列はどれも本文なしの 3 値の 1 つに倒れ、id が要件面に無い周は不在の 1 つに倒れ、呼び手が出す「本文が無い」と「要件面に無い」の行の字面は 1 字も変わらない。約束 6 = .vessel.toml にも rules 行にも欄の宣言は 1 つも増えず、html と md の読み手と拡張子の 1 match と関数 pointer の型と 3 値の型と呼び手の行の組み立ては不変"

[[contract]]
id = "af"
title = "約束の行 [[promise]] の parse と schema — 9 欄の宣言順と必須 / 任意・親の行の無い of と n の重複 / 欠番・空の必須欄を TableError の値で名指し、contracts schema の生成物に 9 欄が載る"
req = ["FR47", "FR55"]
section = "33"
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "contracts/schema.toml"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_parse_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2 --lib --no-tests=fail table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes", "cargo nextest run -p scribe2 --lib --no-tests=fail table_check_"]
size = "S"
done = "(1) 契約表の区間の [[promise]] が 9 欄（of / n / text / files / symbols / teeth / place / fixture / expect・宣言順）で parse され、行の型が親の行 id で引ける (2) of が同じ doc の行に無い・n が重複か欠番・必須欄が空 の 3 形が TableError の値（新しい 2 値 + 既存の欄検査）で contracts check の 1 行に名指され rc 1 (3) contracts schema の生成物に 9 欄が載り、共通 verify の cargo xtask check（contracts-schema の drift）が 0 (4) TableError の名の slice と FIELDS の件数を pin する既存の歯 2 本（verify の完全名）が新しい母集団（TableError 13・欄 16 + 約束の 9）で緑 (5) 約束の行を持たない既存の 161 行の parse と検査（table_check_ の歯・実表の findings=0 は CI の contracts check が測る〔FR55〕）が 1 字も変わらず緑"

[[contract]]
id = "ag"
title = "Promised の行 — 約束の行から touches / creates / also / tests / surfaces を組んで write-set を導き、verify と done を生成し、手書きの write-set / done と base に解けない symbols を受付が断る"
req = ["FR48", "FR47", "FR39"]
section = "33"
depends = ["af"]
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "contracts/schema.toml", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_derive_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_render_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_need_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_names_are_pinned_in_declaration_order", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_promise_"]
size = "M"
done = "(1) 約束の行を 1 つでも持つ行は WriteSet の 3 値目 Promised に弁別され、symbols の閉じた型が touches に・+ の file が creates に・.rs でない file が also に・place が tests に・_external_form の歯と名付き snapshot の歯が surfaces に写り、write-set が §3 の derive_write_set の値と一致して契約 file と runner の allowlist に載る (2) verify は teeth を（crate・scope）で束ねた nextest 行（filter は完全名を空白で並べる）・done は n の順の (n) expect の 1 文として契約 file に生成され、設計 doc には書き戻らない (3) Promised の行が write-set / touches / surfaces / tests / also / creates / done のどれかを持つ・symbols の + 無しの名が base に無い・+ 付きの名が base に在る の 3 形は Refuse の値（新しい 2 値）で断られ run dir が 0・REFUSALS の長さを pin する歯が新しい母集団で緑 (4) verify を持つ Promised の行は生成値と集合一致なら受付を通り、不一致は §3 と同じ drift の断り (5) FIELDS の done と verify が Need の 3 値目 Conditional になり、約束の行を持つ行はその 2 欄が無くても parse を通り、持たない行は TableError の必須 key の欠けで名指され、contracts schema の生成物に conditional が 2 欄で載って cargo xtask check の contracts-schema が緑"

[[contract]]
id = "ah"
title = "Promised の行の審査 — lens の雛形に約束の 4 欄を渡し、verdict の kind を VacuousAssert / GoalDoneContradiction / Other の 3 値に限り、焼き直しの門は契約 file の sha だけを見る"
req = ["FR49"]
section = "33"
depends = ["ag"]
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_promise_prompt_external_form.snap", "crates/scribe2/src/pipe/review/base.rs", "+crates/scribe2/src/pipe/review/outside.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_review_ pipe_review_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_lens_ pipe_intake_repeat_ pipe_intake_promise_"]
size = "S"
done = "(1) Promised の行の lens の雛形に約束の行の写し（n / text / fixture / expect の 4 欄・n の順）が載り、約束の行を持たない行の雛形は 1 字も変わらない（外形 snapshot） (2) Promised の行の review.json の kind が 3 値の外なら verdict が INCONCLUSIVE に倒れ、3 値の中ならそのまま (3) Promised の行の焼き直しの門は契約 file の sha が変わった周だけ通し、at の path 照合を撃たない〔歯 pipe_intake_promise_rework_gate_reads_only_the_contract_sha〕 (4) 約束の行を持たない行の審査と門が不変＝既存の headless_lens_ の歯と外形 snapshot・review.rs の pipe_review_ の歯・焼き直しの門の pipe_intake_repeat_ の歯が 1 字も変わらず緑（verify の 2 行がそのまま走らせる）"

[[contract]]
id = "ai"
title = "約束の行の files の + 無しの .rs を write-set にそのまま写し（Fields の 7 つ目・derive_write_set の (vi)）、symbols の crate:: で始まる型の path 形を受付が module の型の宣言で解く（閉じた型の読み手と同じ 1 本）"
req = ["FR47", "FR48", "FR39"]
section = "34"
write-set = ["crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_files_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_promise_files_"]
size = "S"
done = "(1) files の + 無しの .rs（base に実在）が導出の write-set にそのまま載り、base に無い .rs は ItemUnresolved で断られる (2) symbols の crate::<module>::<Type> は module の enum / struct の宣言で解けて touches に写り、+ 付きは宣言が在れば断られ、末尾 2 節の型::項目の形は従来どおり (3) 既存の contract_promise_ / pipe_intake_promise_ / pipe_intake_repeat_ の歯と外形 snapshot が 1 字も変わらず緑"

[[contract]]
id = "aj"
title = "焼き直しの門の teeth-outside-write-set の物差しは at のうち path の形に解ける項目だけを測り、path でない項目（歯の接頭辞・§ の番号）は測れないとして断りの理由から外す"
req = ["FR49", "NFR4"]
section = "35"
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_unaddressed_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_repeat_"]
size = "S"
done = "(1) at に path でない項目（歯の接頭辞・§ の番号）が混ざった teeth-outside-write-set の後、path の項目を write-set に足した契約が受付を通る (2) path の項目が write-set に無い契約は従来どおり finding-unaddressed で断られ、理由に測った項目と測れなかった項目の数を出す (3) 既存の pipe_intake_repeat_ / pipe_review_unaddressed_ の歯が 1 字も変わらず緑"

[[contract]]
id = "ak"
title = "nextest 行の読み手が引数を取る target の旗（--bin / --bench / --example / -E）の次の語を消費して filter 語に数えない — UNREAD_TARGET_FLAGS を引数を取る旗と取らない旗の閉じた 2 slice に分け、scope の倒し方（Crate・fail-closed）と filter 語の規則は不変"
req = ["FR48", "FR55"]
section = "36"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_derive_target_flag_"]
size = "S"
done = "(1) 引数を取る旗（--bin / --bench / --example / -E）の次の 1 語が filter 語にならず消費され、取らない旗（--bins / --benches / --examples / --tests / --all-targets）は従来どおり (2) 引数を取る旗が行末なら nextest_filter が None (3) scope は両方とも Crate のまま・-p / --lib / --test の読みと filter 語の規則（最後の非旗の語）は不変 (4) -p x --bin x foo_ が filter foo_・-p x --bin x --test face foo_ が filter foo_ と Crate・-p x --bin が None・-p x --bins foo_ が filter foo_・-p x -E expr bar_ が filter bar_ (5) 既存の contract_derive_ の歯が 1 字も変わらず緑"

[[contract]]
id = "al"
title = "pipe/review.rs の「要件本文の読み手」の群（13 item・561–768 行・正規化 209 行）を子 module へ割る — 純移動（名・本文・順序・doc comment 不変・歯 0 本・親に mod 1 行と use 2 文の 4 行・子側の pub(super) 6 名・札 2 か所）"
req = ["FR49", "FR30"]
section = "37"
write-set = ["-crates/scribe2/src/pipe/review.rs", "+crates/scribe2/src/pipe/review/requirements.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_requirements_text_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_yaml_shall_"]
size = "S"
done = "(1) 13 item が名・本文・順序・doc comment を変えずに + の file へ移る（move_proof が pure と判じる・items-differ / residual-line 0 件） (2) 親に増えるのは mod 宣言 1 行・素の use 1 行（requirements_text）・#[cfg(test)] だけの 1 行と use 1 行（残り 5 名・既存の行頭 #[cfg(test)] の直上）の 4 行だけ (3) in-file の歯の本文と use super::{…} が 1 byte も変わらず、e2e は触らない (4) 子側の pub(super) は名指しの 6 名だけで、群内の 7 名と親側の可視性は不変 (5) 札 moved が親の mod tests { の直後と子の module doc の直後に 1 行ずつ (6) 既存の 8 本（pipe_review_requirements_text_ 2 本・pipe_review_yaml_shall_ 6 本）が名・本数・本文不変で緑・clippy -D warnings が通常 build と test build の両方で rc 0"

[[contract]]
id = "am"
title = "pipe/review.rs の「判定の読み手と受付の 2 門」の群（14 item・218–364 行・正規化 150 行）を子 module へ割る — 純移動 2 便目（名・本文・順序・doc comment 不変・歯 0 本・親に mod 1 行と pub use 2 行と歯用の 2 行・子側の pub(super) は split_at の 1 名・孤立した import 3 つを削る・札 2 か所）"
req = ["FR49", "FR30"]
section = "38"
write-set = ["-crates/scribe2/src/pipe/review.rs", "+crates/scribe2/src/pipe/review/judgement.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_judgement_reads_kind_and_splits_at", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_unaddressed_measures_each_kind_with_one_ruler", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_unaddressed_teeth_measures_only_path_shaped_items", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_check_reads_review_json_fail_closed"]
size = "S"
done = "(1) 14 item が名・本文・順序・doc comment を変えずに + の file へ移る（move_proof が pure と判じる・items-differ / residual-line 0 件） (2) 親に増えるのは mod 宣言 1 行・pub use 2 行（8 名）・#[cfg(test)] だけの 1 行と use 1 行（split_at・既存の列 0 #[cfg(test)] の直上）の 5 行だけで、孤立した import 2 行と 1 語を削る (3) in-file の歯の本文と use super::{…} が 1 byte も変わらず、e2e と他 module の review:: の path は触らない (4) 子側の pub(super) は split_at の 1 名だけで、pub の 8 名と Judgement / Rework の pub field と親側の可視性は不変 (5) 札 moved が親の mod tests { の内側（§37 の札の次の行・置き換えない）と子の module doc の直後に 1 行ずつ (6) 名指しの既存 4 本が名・本数・本文不変で緑・clippy -D warnings が通常 build と test build の両方で rc 0"
[[contract]]
id = "an"
title = "名指しの実在が他の行の宣言済み・未着地の新規 file を解く — 検査の文脈に repo の全 doc から集めた宣言済みの新規 file の列を足し、CI と受付が同じ 1 本で母集団を組む。断りの字面と在り処の形と他の検査は不変"
req = ["FR48", "FR55"]
section = "39"
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_names_declared_"]
size = "S"
done = "(1) 検査の文脈が宣言済みの新規 file の列を持ち、tracked な設計 doc の区間の全行から印つきの write-set の項目と creates の欄を集める 1 本で組まれる (2) 別の doc の行が宣言した新規 file を名指した行に name-unresolved が出ず、どの行も宣言していない名は従来どおり 1 件出る (3) 同じ母集団を CI の駆動と受付の材料の両方が同じ 1 本から受け、受付でも同じ行が通る (4) 区間を読めない doc が在る周は母集団を縮めたまま通さず従来の読めなさの 1 件が出る (5) name-unresolved の字面と在り処の形・型の path 形と fn 形の解き方・write-set の項目の実在・depends の母集団・findings の順と rc が不変で、現物の契約表は findings 0・rc 0"
[[contract]]
id = "ao"
title = "審査の材料に write-set の各項目の base の要約（行数の 2 面・本体の宣言の名・歯の名）を 1 file として足し、lens の雛形の穴 1 つを器が埋める。観点 3 つと理由の型と既存の 3 材料は不変で、cap を越える周は要約の段だけ落として本数を残す"
req = ["FR49", "NFR1"]
section = "40"
write-set = ["crates/scribe2/src/pipe/review.rs", "+crates/scribe2/src/pipe/review/base.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_base_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_base_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_base_"]
size = "M"
done = "(1) 審査の材料の dir に要約の file が 1 本増え、置く側は既存の 3 本と同じ 1 か所で、組む側は 1 回だけ呼ばれる (2) 要約 1 本が項目の path と行数の 2 面を持ち、.rs は本体の宣言の名の列と歯の名の列を別の列として持ち、.rs でない項目は行数だけ・+ の項目は新設の 1 行・読めない項目は読めなさの 1 行になる (3) 材料の写しが在る周は雛形の穴が本文で埋まり、無い周は雛形が 1 字も変わらず、契約の本文の中の穴の字面は展開されない (4) 要約を足すと cap を越える周は要約の段だけが落ちて落とした項目の本数の 1 行が残り、既存の 4 材料だけで越える周は claude を呼ばず INCONCLUSIVE のまま (5) 歯の区間と本体の区間と行数の 2 面の読み手は既存の 3 本のままで、新しい読み手を作らない (6) 審査の観点 3 つの本文・理由の型の 6 語・判定の JSON の形・diff の審査の極性・審査の rc が不変"

[[contract]]
id = "ap"
title = "Declared 行の歯の置き場の門の断りが、write-set に無い歯の file と、それを解いた verify 行の filter 語を対で名乗る。照合の 1 本と門の条件と断りの語と rc は不変"
req = ["FR48"]
section = "41"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_teeth_origin_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_teeth_origin_"]
size = "S"
done = "(1) 行ごとに解いた置き場が file と filter 語の対として畳まれ、同じ file を 2 行が解いた周は verify の先の行の語が付き、対は file の辞書順 (2) 照合の 1 本が対を受けて write-set に無い分を対のまま断りの payload にし、正規化と dir 項目の扱いは不変 (3) 断りの理由の 1 行が file と filter 語の両方を名乗り、受付の側の同じ名の型も同じ欄を持ち理由は導出の側の 1 本を写すだけ (4) 置き場を解く関数の signature と Promised の導出・門を撃つ条件と順・解けない filter の断りの字面・断りの語・rc・run dir を作らないことが不変 (5) 受付の stderr が file と filter 語を両方名乗り rc 1 で run dir を作らない"

[[contract]]
id = "aq"
title = "Declared 行の歯の置き場の逃がしが、write-set の + の新規 .rs も置き場と読む（宣言済みの新規 file を path だけで認める Promised 形と同じ下界）。断りの字面と型・行ごとに解く形・他の理由の断りは不変"
req = ["FR48"]
section = "42"
write-set = ["crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_declared_place_new_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_declared_place_new_"]
size = "S"
done = "(1) 逃がしの条件が「base に在る歯の file が write-set に在る」か「write-set の + の項目に .rs が在る」のどちらかになり、base 側だけの周の挙動は不変 (2) + の項目は本文を見ず path だけで置き場と認められ、置き場の欄の項目を creates で照合する既存の弁別と同じ 1 つの規則のまま (3) 逃がしが効くのは解けない filter の断りだけで、読めない source と置き場の欄の項目の不整合はそのまま断る (4) base にも + にも歯の置き場が無い write-set は従来と同じ字面と rc 1 で断られ run dir を作らない (5) 契約表の行が + の新規の歯の file と新しい filter 語だけを持つ周に受付が rc 0 で run dir を作る (6) 既存の負例の歯 2 本が本文も期待も変わらず緑"
[[contract]]
id = "ar"
title = "write-set に「中身を変えない・verify の置き場として載せただけ」を表す項目の印を 1 つ足し、上限の余地の検査と core の見積の本数から外す（縮む面と消える file と同じ腕）。交差・guard・gate の照合は素の path のまま"
req = ["FR48", "FR39"]
section = "43"
touches = ["crate::pipe::declaration::WriteSetItem"]
write-set = ["crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_place_only_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_place_only_"]
size = "S"
done = "(1) 接頭辞の字面の定数が既存 3 本の隣に 1 本増える (2) 閉じた列挙の変種が 1 つ増え、その接頭辞は base に在る file にだけ解け、受付と契約表の検査で読みが変わらない (3) 接頭辞を剥がす 1 か所が新しい字面も剥がし、剥がす規則が 2 か所に増えない (4) 網羅 match 2 か所で新しい変種が余地を求めず core の見積の本数にも数えられない (5) 交差・guard・gate の write-set 照合・契約表の検査が素の path のまま 1 字も変わらず、断りの字面と型と rc が不変 (6) 余地の足りない file を印つきで載せた行が受付を通って run dir が出来、印だけを外すと従来の余地不足の字面で rc 1・run dir を作らない"

[[contract]]
id = "as"
title = "verify 行の -- の後ろの --exact を完全一致の filter として読む — 名の全体の末尾の段を fn 名と等値で照合し、-- の後ろの旗（--skip は引数 1 語）は閉じた列で読み飛ばす。-- の前の読みと宣言の形の門（括弧・引用符を断る）は不変"
req = ["FR48", "FR55"]
section = "43"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_teeth_exact_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_teeth_exact_"]
size = "M"
done = "(1) closure.rs に -- の後ろの libtest の旗の閉じた 2 列（引数を取る --skip・取らない --exact / --include-ignored / --nocapture / --no-capture）が増える (2) nextest_filter が -- を境に読みを切り替え、後ろの旗を読み飛ばして裸の語を filter 語にし、--exact が在れば完全一致・無ければ部分一致になる (3) 完全一致は :: で割った末尾の段を fn 名と等値で照合し、同じ段を substring に持つ別 fn の file を置き場に取らない (4) -- の後ろに --exact も裸の語も無い行は従来どおり None で、断りの型と字面と rc が不変 (5) 検出線の語が末尾の段になる (6) -- の前の読み・METACHARS と宣言の形の門・tests 欄の弁別が不変で、現物の契約表（verify 行 208 本・-- を持つ行 0 本）の判定は 1 本も変わらず契約表の歯は緑のまま"

[[contract]]
id = "at"
title = "契約表の検査が未追跡の設計 doc を 1 行知らせ、判定行に未追跡の本数の欄を足す（findings にも rc にも数えない検出線・git が答えない周は ? で 0 に化けさせない）"
req = ["FR55", "NFR4"]
section = "43"
write-set = ["crates/scribe2/src/pipe/table/check.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contracts_untracked_doc_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contracts_untracked_doc_"]
size = "S"
done = "(1) 検査が未追跡の設計 doc を既存の git の 1 本で 1 回引く (2) 判定行に未追跡の本数の欄が 1 つ増え、現物の repo では 0 で出る（判定行を完全一致で読む contracts.rs と intake.rs の既存の歯は新しい欄を含む字面へ更新し、他の期待は変えない） (3) 1 本以上の周は判定行の前に 1 件 1 行で path を名乗り、findings の数も rc も変わらない (4) git が答えない周はその欄が ? になる (5) 追随の口の戻りが 1 つも変わらない (6) 検査の母集団は tracked な設計 doc のままで、findings の順と rc と doc 数と行数の数え方が不変"
[[contract]]
id = "au"
title = "審査の base の要約が置き場だけの印（=）の項目を剥がして本文を読み、行に「置き場だけ」の 1 語を添える — 剥がしは normalize の 1 本に寄せ、+ の 1 行と他の項目の形は不変"
req = ["FR48", "FR9"]
section = "44"
write-set = ["crates/scribe2/src/pipe/review/base.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_base_place_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_base_place_"]
size = "S"
done = "(1) = の項目が + の分岐の後で剥がされて本文が読まれ、+ の項目は従来の 1 行のまま（母集団 = + / - / ~ / = の 4 形を同じ木で要約する） (2) = の項目が本文を読んで本体の宣言の名と歯の名の 2 列を出し、行の頭が契約の字面のままで、行数の後ろに置き場だけの 1 語を持つ (3) + の 1 行・- と ~ の行・repo の外の断り・cap を越える周の落とし方が 1 字も変わらない (4) = の項目を持つ契約の審査の材料 base.txt が「読めない」を持たない"

[[contract]]
id = "av"
title = "契約表の検査が Declared 行ごとに teeth_places を撃ち、解けた歯の file が write-set の外に在る行の本数を判定行の欄 place-out=<行数>/<Declared 行数> で出す（検出線・rc と findings は不変・--verbose の周だけ行の id と file を 1 行ずつ）"
req = ["FR9", "FR48"]
section = "45"
write-set = ["crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_check_place_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_check_", "cargo nextest run -p scribe2 --lib --no-tests=fail contracts_untracked_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_requirement_faces_read_html_anchors_and_yaml_lists_by_extension", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_parse_check_names_orphans_gaps_and_duplicates_one_line_each", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_same_name_table_check_skips_a_file_that_cannot_see_the_type", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_begin_region_start_is_not_section_body", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_end_region_close_resumes_the_section_body", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_fence_heading_inside_a_fence_keeps_the_section", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_inside_body_outside_a_fence_is_collected", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_check_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_surfaces_name_the_snapshot_and_the_teeth_that_pin_it", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_delete_check_passes_after_landing", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_names_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_schema_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contracts_untracked_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_table_landed_plus_item_resolves_as_file", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_promise_row_"]
size = "M"
done = "(1) tmp の repo の Declared 行 3 本（歯が write-set の内 / 外 / 解けない）で判定行が place-out=1/3 を出し、解けない行は数えない (2) rc と findings の本数が欄の追加の前後で同じで、check.rs の mod tests の既存の歯 13 本（table_check_ / contract_closure_ext_ / contract_promise_ / contracts_untracked_）と判定行を全行で pin する e2e の歯 17 本（pipe/contracts.rs の 16 本・pipe/intake.rs の 1 本・末尾を findings=0 で読む 2 本を含む）が欄の追加ぶん字面を進めて全部緑（lib の 13 本は verify 行が名の全体か check.rs に閉じる接頭辞で名指す） (3) --verbose（本便が cli.rs の contracts に足す旗・check_repo の引数 1 つ・使い方の文字列にも載る）の周だけ当たった行の doc・行 id・file が 1 行出て、無い周は 0 行で、e2e の歯 1 本（pipe/contracts.rs・contract_check_place_verbose_ の接頭辞）が binary を撃って旗あり 1 行・旗なし 0 行・使い方の文字列に --verbose が載ることを測る (4) 現物の契約表で撃つと place-out=<n>/<m> の実測が notes に残る"

[[contract]]
id = "aw"
title = "契約表の検査が write-set の外に歯を持つ Declared 行を findings の 1 語 TeethOutsideWriteSet（doc・行 id・file）に上げる — 判定行の place-out= の欄は残し、現物の契約表の歯が 0 件のまま緑（行 av の着地後・main で place-out=0 を実測してから）"
req = ["FR9", "FR48"]
section = "45"
touches = ["crate::pipe::table::TableError"]
write-set = ["crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_check_place_finding_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line"]
size = "S"
done = "(1) tmp の repo の外の行が findings 1 件・語 TeethOutsideWriteSet で doc・行 id・file を持ち、内の行と解けない行は 0 件 (2) 判定行の place-out= の欄が残り本数が findings と一致する (3) 現物の契約表の歯が 0 件のまま緑で、母集団（Declared 行数）が判定行の分母と同じ（行 av の着地後の main で place-out=6/210 だった 6 行〔a / c / g / j / n / ah〕を §45 の 4 のとおり直して 0/210 を実測済み・本便の入口は 0/210） (4) 語 TeethOutsideWriteSet は table.rs の閉じた列挙 TableError の variant で、網羅 match の table.rs と table/parse.rs・語の一覧の pin TABLE_ERRORS（14 → 15）が同じ便で動き、その pin の歯 table_error_names_are_pinned_in_declaration_order_and_carry_their_line を verify 行が名指す"
[[contract]]
id = "ax"
title = "契約表の行の任意の欄 growth（file ごとの見込み行数）— 受付の上限の余地を file ごとの見込みで測り core の見積は見込みの和、写しが欄を運び、表の検査が項目の形の崩れを GrowthForm で名指し、preflight の見積が file ごとになる（size の 3 段と上限の値は不変）"
req = ["FR47", "FR68", "FR55"]
section = "46"
touches = ["crate::pipe::table::TableError"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/mod.rs", "contracts/schema.toml", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_growth_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_check_growth_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_growth_"]
size = "S"
done = "(1) growth に在る file はその値で余地と比べ、無い file は size で比べ、core の見積が見込みの和（size × 本数と値が割れる fixture で A/B） (2) 生成の写しが行の growth をそのまま運び、行に無ければ写しにも無い (3) 表の検査が 5 つの形の崩れ（形・write-set に無い・.rs でない・縮む面や消える file や置き場だけの項目・重複）を GrowthForm で行番号つきに名指し、正しい 3 項目は 0 件 (4) preflight の headroom= の見積が file ごとの値 (5) contracts schema の生成物に growth（任意・list）が在り tracked と差分 0 (6) tmp の repo で同じ base・同じ size の行が growth の有無だけで受付の cap-headroom が反転する (7) 既存の pin が 18 / 11 / 16 に動き、size の 3 段の rules 行と上限の値は 1 字も変わらない"

[[contract]]
id = "ay"
title = "導出物（.toml）の契約表の行が節の本文の逐語 goal を運び、表の検査・審査の材料・契約 file の goal をそこから取る — .md の pointer の経路と FIELDS は不変、先頭の版の宣言の字面を定数 1 つで持ち歯が folio2 の字面に pin する"
req = ["FR47", "FR53", "FR54", "FR49"]
section = "47"
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_whole_goal_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_whole_goal_"]
size = "S"
done = "(1) rules manifest の契約表の key 集合が導出物だけの欄 goal を受け、FIELDS と contracts schema の生成物は 1 字も変わらない (2) .md の区間の行の goal は今と同じ字面の未知の key goal を goal の行番号で出して断られ、.toml の全文の行は goal を二重引用符と backtick を含む単一行のまま逐語で読む (3) .toml の先頭が版の宣言の定数でなければその行番号で断られ、定数の字面が folio2 の ADR-3 決定 (4) の schema = 1 と一致し rules manifest の版と同じ値であることを歯が pin する (4) 導出物が省いた任意の一覧は空と読まれ、req を省いた行は断られる (5) 表の検査は goal を持つ行の節を goal で満たして section-missing 0 件、goal の無い .toml の行は section-missing で断り、goal の中の解けない名を名指す (6) .toml の行を指す受付の審査の材料 design.txt が出所の 1 行と goal を持つ (7) 契約 file の goal が行の goal と等しく読み戻り、goal の無い行は title のままで、.md の pointer の既存の歯は緑のまま"
growth = ["crates/scribe2/src/pipe/table.rs:40", "crates/scribe2/src/pipe/table/parse.rs:70", "crates/scribe2/src/pipe/table/check.rs:50", "crates/scribe2/src/pipe/review.rs:10", "crates/scribe2/src/pipe/contract.rs:30", "crates/scribe2/src/rules/manifest.rs:10"]

[[contract]]
id = "az"
title = "3 クラスを verify 行から導く — 閉じた型 1 つ（delete / publish / consume）が受理集合を持ち、rules 行 1 つ（クラスの語列表・値は裁定 user 2026-09-24T00:13Z の 3 要素）の各要素を禁じる語列と同じ照合で verify 各行に当て、導出が classes に無い行を契約表の検査が ClassUndeclared で全件名指す（受付は便を作らない・行が無い / 読めない / 不発効 / 列でない周は断る・write-set と land の口は入力にしない）"
req = ["FR15", "FR1", "FR47", "FR56", "NFR4"]
section = "48"
depends = ["ba"]
touches = ["crate::pipe::table::TableError", "crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail class_derive_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail class_derive_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_kind_parity_every_kind_has_sample", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_names_declared_real_table_has_zero_findings"]
size = "M"
growth = ["crates/scribe2/src/pipe/contract.rs:90", "crates/scribe2/src/rules/mod.rs:25", "crates/scribe2/src/rules/manifest.rs:90", "crates/scribe2/src/pipe/table.rs:25", "crates/scribe2/src/pipe/table/check.rs:140", "crates/scribe2/src/pipe/table/parse.rs:5", "crates/scribe2/src/pipe/refuse.rs:5", "crates/scribe2/src/pipe/declaration.rs:3", "crates/scribe2/src/pipe/cli/intake.rs:15", "crates/scribe2/src/pipe/cli/step.rs:3", "crates/scribe2/src/pipe/follow.rs:5", "crates/scribe2/src/pipe/cli.rs:5"]
done = "(1) 3 クラスの名は contract.rs の閉じた型 1 つ（3 値・宣言順 delete / publish / consume）で、契約 file の classes の受理集合がその型の名の列と一致し、要素の読み手 1 本が名 + 語列・未知の名・名だけの 3 形を分ける (2) rules 行 1 本（id runner.class_commands・kind RunnerClassCommands・ALL の末尾）が埋め込みの manifest に在り、値が裁定の 3 要素（publish git push / delete git push --delete / delete git push -d・この順・consume は要素なし）で行の裁定 id が user 2026-09-24T00:13Z・裁定日が 2026-09-24 であることを class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id が測り（要素か裁定 id を 1 つ変える変異で赤）、外形 snapshot の rows と kinds が base から 1 ずつ増え、sample_value の分岐 1 つで parity の歯が緑 (3) rules の読みが崩れ (a) 名でない先頭語 (b) 空の語列 (c) allowlist に無い先頭語 (d) 受付が読む禁じる語列の和集合（runner.denied_commands と host の見張りの語列 3 行・denied_of の読み手 1 本）のどれかを含む語列 を 1 件ずつ行番号つきで断り、禁じる語列と一部だけ重なる git push は通り、上限の行か和集合の 4 行が揃わない manifest では (c)(d) を撃たない (4) 契約表の検査が verify 各行を要素ごとに denied_in で当て、行の classes に無いクラスの当たりを TableError の 1 語 ClassUndeclared（行番号・verify 行・語列・クラス）で全件名指し（同じ行の verify 2 本と要素 2 つで 3 件）、classes が同じか広い行は 0 件、語の順が違う verify も当たる (5) write-set に - と ~ の項目を持ち verify に語列の無い行は 0 件（write-set を読む変異で赤） (6) tmp の repo で名乗らない行を contracts check が名指し、受付は rc 1・run dir 0・同じ字面の findings で、classes を足した同じ行は両方とも通る (7) 語列表の行が無い・不発効・列でない --rules で contracts check と受付が行 id を名指して rc 1 (8) Ceiling の組み立ての全箇所が欄を埋め、表の検査を撃たない 2 か所は空の列を渡し、TableFacts と approve.rs と classes の欄の形は 1 字も変わらない (9) TABLE_ERRORS の pin が base から 1 増え、e2e の共有の rules の fixture が語列表の行を持って既存の受付の歯が緑で、現物の契約表の歯 2 本が裁定の値の下で緑。hook/command.rs は write-set の外のまま pub の denied_in / denied_of を呼ぶだけで可視性も本体も変えない"

[[contract]]
id = "ba"
title = "審査の base の要約が pub(in path) の可視性の宣言を読む — 括弧つきの可視性を閉じ括弧まで 1 まとまりとして飛ばし宣言の列に載せる（pub(crate) / pub(super) の既存の読みと歯は不変）"
req = ["FR48", "FR9"]
section = "49"
write-set = ["crates/scribe2/src/pipe/review/base.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_base_pub_in_"]
size = "S"
done = "(1) declared_name が pub(in <path>) を閉じ括弧までの 1 まとまりとして飛ばし、pub(in crate::pipe) struct Materials が struct Materials として宣言の列に載り、閉じ括弧の無い行は宣言と読まない (2) pub(crate) / pub(super) / pub 無し / 修飾語 / const fn の既存の読みと既存の歯 pipe_review_base_rs_lists_declarations_and_teeth_in_separate_columns の期待は 1 字も変わらない (3) 歯 pipe_review_base_pub_in_ が (1) の正例 3 形（struct / fn / const）と負例 1 形（閉じ括弧の無い行）を fixture の逐語で測り、base で 0 本"

[[contract]]
id = "bb"
title = "終端の CI の照合は rules 行 pipe.ci_poll_s（30 秒・裁定 user 2026-09-27T11:14Z）の間隔で撃つ — 唯一の待ちの周期を完了条件ごとの 1 関数で返し CiResult だけが欄 every を持つ（0 は POLL に戻る）・最初の評価は眠る前・上限を越えて眠らない（他の完了条件の周期と終端の 7 値は不変）"
req = ["FR50"]
section = "50"
touches = ["crate::rules::RuleKind", "crate::pipe::land::Land"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/train.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/order.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_terminal_ci_poll_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_wait_ci_interval_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_ci_poll_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role"]
size = "M"
done = "(1) 埋め込みの manifest に行 pipe.ci_poll_s（kind PipeCiPollS・Int・値 30・enabled・裁定 id user 2026-09-27T11:14Z・裁定日 2026-09-27）が pipe.ci_wait_s の直後に 1 本在り、kind は ALL の PipeCiWaitS の直後で字面から引け、終端の材料の読み手が pipe.ci_wait_s と同じ int_row で読んで Land の欄 ci_poll_s で運び、行の無い manifest は pipe.ci_wait_s と同じ極性で断られる (2) Completion の周期を返す 1 関数（網羅の match）が CiResult には欄 every と POLL の大きい方、他の全 variant には POLL を返し、wait がその周期で眠り、every 0 は POLL に戻る (3) 最初の評価は眠る前で、最初から success の偽 CI と間隔 30 秒の周は 10 秒未満で terminal=closed (4) 眠る長さは周期と上限までの残りの小さい方で、上限 1 秒・間隔 30 秒の周は 10 秒未満で terminal=ci:unmeasurable、上限 2 秒・間隔 1 秒の周は偽 CI の呼び出しが 2 回以上 4 回以下 (5) 終端の 7 値・3 段・ci_now の判定・ci-cmd の形・pipe.ci_wait_s の値は変わらず、既存の終端の歯は fixture の行の値 0 で緑のまま、埋め込みの manifest の行数と kind の数の pin と rules_external_form の snapshot が 1 ずつ増える"

[[contract]]
id = "bc"
title = "審査の材料に契約が名指す write-set の外の物（宣言の塊・名指された .rs の要約・data file の鍵の行・crate の依存の表・親 module の宣言・depends の相手の行）を 1 file として足し、lens の雛形の穴 1 つを器が埋める — 名の照合は名指しの読み手の 1 本（backtick の中の名と外の識別子の形の名・/ か拡張子を持つ path の末尾一致・外の素の 1 語は読まない）、cap は既存の残りで名ごとに切り詰めの印を残し、diff の審査と既存 4 材料は不変"
req = ["FR49", "NFR1"]
section = "51"
touches = ["crate::pipe::review::Material"]
write-set = ["crates/scribe2/src/pipe/review.rs", "+crates/scribe2/src/pipe/review/outside.rs", "crates/scribe2/src/pipe/review/base.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_outside_", "cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_mentioned_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_outside_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_outside_"]
size = "L"
growth = ["crates/scribe2/src/pipe/review.rs:20", "crates/scribe2/src/pipe/review/base.rs:10", "crates/scribe2/src/pipe/closure/names.rs:100", "crates/scribe2/src/pipe/closure.rs:2", "crates/scribe2/src/headless/lens.rs:70"]
done = "(1) 名指しの在る契約の審査の材料の dir に外の材料の file が 1 本増え、組む側は materials から 1 回だけ呼ばれ、置く側は約束の行と同じく本文が空でない周だけ置き、名指しの無い契約の材料の dir は 4 本のまま (2) 名の照合は names.rs の 1 本の口で、候補の名を backtick の中身の先頭の token の節か、backtick の外に holds_word の語の境界で現れ字面が識別子の形（_ を持つか大文字の山が 2 つ以上）のものに限って拾い、外の素の 1 語は宣言が在っても拾わず、path 形の文字の連なりは / か拡張子を持つものだけを path_matches で tracked の path に解き（拡張子を問わず・同じ末尾の file は全部・素の 1 語は dir に解かない）、候補は base の要約と同じ declared_name と tooth_names が base の .rs から読み、unresolved_names の判定は不変 (3) 名ごとの塊が .rs の item（所在・doc 行・可視性を含む宣言・struct と enum の本体・fn の署名・歯の本体・write-set の中の名は出さず 2 file 以上の名は所在の 1 行）・名指された .rs の要約・data file（行数と byte 数と拡張子ごとの鍵の列と本文に在る鍵を持つ最初の 1 行だけ・全文は渡さない・dir は配下の file ごとの 1 行）・Cargo.toml の依存の表・親 module の mod の行か宣言なし・depends の相手の行の title と + の項目の在否の 6 形で、並びは構造の材料が先 (4) 塊が cap の残りに収まらない名は切り詰めの 1 行、それも収まらない周は落とした名の本数の 1 行が残り、既存 4 材料だけで越える周は claude を呼ばず INCONCLUSIVE のまま・base の段の落とし方は不変・rules 行は足さない (5) 写しが在る周は雛形の末尾の穴が本文で埋まり、無い周は雛形が 1 字も変わらず、本文の中の穴の字面は展開されない (6) diff の審査の雛形・審査の観点 3 つ・理由の型の 6 語・判定の JSON の形・焼き直しの門の物差し・列外の鍵・審査の rc が不変で、歯 pipe_review_outside_ と closure_names_mentioned_ と headless_lens_outside_ が base で 0 本"

[[contract]]
id = "bd"
title = "列で着地した便のうち push の先端でない便の終端は CI を照合せず待たずに ci:unmeasurable で止まる — forge の CI は push の先端にだけ run を持つので、先端でない sha の待ちは上限（pipe.ci_wait_s）まで空回りし、同じ列の後続の終端と着地の列を 1 本ごとに止める（close しない極性・先端の便と単独の着地と --terminal-only の終端は不変・memo s2-07l.688）"
req = ["FR50"]
section = "52"
touches = ["crate::pipe::land::finish::land_train", "crate::pipe::land::finish::terminal"]
write-set = ["crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_train_terminal_"]
size = "S"
growth = ["crates/scribe2/src/pipe/land/finish.rs:30", "crates/scribe2/src/pipe/land.rs:10"]
done = "(1) land_train が列の最後の便（main を進めた先端の commit を持つ便）の外の便に『push の先端でない』を閉じた 2 値で finish から terminal へ運び、その 1 か所だけが先端を知る (2) 先端でない便の terminal は push を今どおり撃って terminal:push:<remote> を記し、CI の照合（唯一の待ちと ci_now）を 1 回も撃たずに terminal:ci:unmeasurable を記して止まり（close しない・stdout の terminal=ci:unmeasurable）、先端の便・単独の着地・--terminal-only の終端は push → CI → close の今の 3 段のまま (3) 終端の 7 値・event の詞・ci_now の判定・pipe.ci_wait_s と pipe.ci_poll_s の値・列の記帳の順（後続 → 先頭）は変わらない 歯: pipe_train_terminal_ が、偽 remote と常に success を返す偽 CI と偽 bd を持つ 3 本の列の着地で、先端の便の Landed の後ろが push・ci:success・close:ok の 3 件、先端でない 2 本の Landed の後ろが push と ci:unmeasurable の 2 件で close を持たず、偽 CI の呼び出し（ci_call_count）が 2 回（先端の便の待ちの最初の 1 回と読み直しの 1 回）、偽 remote の main が先端の sha を指すことを測り、base では 3 本とも CI を照合して close する（偽 CI の呼び出し 6 回）ので RED"

[[contract]]
id = "be"
title = "列で着地した便のうち push の先端でない便は、自分の commit を祖先に持つ先端の commit の CI の結果で照合して close する — PushTip::Behind が先端の sha を持ち、祖先の周だけ先端の sha で CI を照合し、close の reason に tip= を持つ（FR50・祖先でない周と --terminal-only は不変・memo s2-07l.688）"
req = ["FR50"]
section = "53"
touches = ["crate::pipe::land::finish::land_train", "crate::pipe::land::finish::terminal"]
write-set = ["crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_train_tip_close_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_train_terminal_"]
size = "S"
growth = ["crates/scribe2/src/pipe/land/finish.rs:35", "crates/scribe2/src/pipe/land.rs:5"]
done = "(1) PushTip::Behind が先端の commit の sha を持ち、先端を知るのは land_train の 1 か所のまま (2) 先端でない便の終端は push の後に自分の sha が先端の祖先かを測り、祖先でない周と測れない周は CI を照合せず terminal:ci:unmeasurable で止まり（close しない）、祖先の周は CI の照合（待ちと ci_now）を先端の sha で撃つ (3) success の周は close し reason が landed <sha> ci=success tip=<先端>、failure と unmeasurable は close しない (4) 終端の 7 値・event の詞・ci_now の判定・pipe.ci_wait_s と pipe.ci_poll_s の値・列の記帳の順・先端の便と単独の着地の close の reason・--terminal-only の終端は変わらない 歯: 既存の pipe_train_terminal_only_the_tip_checks_ci_and_closes を接頭辞 pipe_train_terminal_ のまま名と期待を書き直し、常に success の偽 CI の 3 本の列の着地で 3 本とも Landed の後ろが push・ci:success・close:ok の 3 件、偽 CI の呼び出しが 6 回で最後に渡った sha が先端の sha であることを測る・pipe_train_tip_close_ の 1 本の fn の 1 周目が偽 bd の最後の close の reason に tip=<先端の sha> を、2 周目が偽 CI の failure で 3 本とも close しないことを測る・base は先端でない便が close せず止まるので RED"

[[contract]]
id = "bf"
title = "契約表の検査が、着地の前の行の新しい歯の接頭辞が他の Declared 行の verify の filter 語を含み、予想の置き場の候補の全部がその行の write-set の外に解ける衝突を、既存の teeth-outside-write-set で docs の時点に出す — 候補は tests 欄か歯を置ける .rs を module の path の語で絞り、--verbose の行に予想の出所と直し方を足す（TableError の variant は足さない・memo s2-07l.708）"
req = ["FR55", "FR48"]
section = "54"
write-set = ["crates/scribe2/src/pipe/table/check.rs", "+crates/scribe2/src/pipe/table/check/collide.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contracts_prefix_collision_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_check_place_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_check_place_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_names_declared_real_table_has_zero_findings"]
size = "M"
growth = ["crates/scribe2/src/pipe/table/check.rs:30"]
done = "(1) 着地の前の行（write-set の + の項目か creates のうち base の tracked に無い path を 1 つ以上持つ行・Declared か導出かを問わない）の verify の nextest 行ごとに teeth_places と同じく最後の filter 語 1 つ（2 語以上の行も最後の語だけ・先頭の語ではない）を読み、その語が識別子の形で、その行の scope の base の .rs の本文の全体（歯の区間の印の前も含む＝#[path] の子 module の歯の file の既存の歯も数える）に語を含む #[test] の fn が 0 本の語だけを新しい語と読み、読みは teeth_places を各 file の本文の前に歯の区間の印を置いた写しで撃つ (2) 置き場の候補は、行の tests 欄が在ればその項目、無ければ write-set（- の項目を除く）と creates の .rs のうち base に無い新規 file か base の歯の区間が空でない file で、語の verify 行の scope に入るもの（各候補に語の名の歯 1 本を置いた仮の本文を teeth_places に渡して解く） (3) tests 欄でない候補は、module の path（crates/<crate>/src/ か tests/ より後ろの段・mod / lib / main の段は数えない）の末尾の段の連なりを _ で繋いだ字面が語と等しいか語の先頭に _ の境で一致する file が在れば、一致の段数が最も多い file だけに絞り、一致が無ければ全部を残す (4) 他の Declared 行（doc を跨ぐ・自分の行を除く）ごとに、残った候補 1 つずつに語の名の歯 1 本だけを持つ仮の本文を teeth_outside に渡し、全部の候補がその行の write-set の外に解けた周だけ当たりとし、候補が 0 本の語は数えない (5) 当たった行は既存の teeth-outside-write-set の 1 件（その行の見出しの行番号・行 id・候補の file）と判定行の place-out= の 1 行に数えられ（findings の件数と欄の行数の一致は不変）、同じ行の当たりは 1 件に file を併せ、--verbose の知らせの行の末尾に予想の出所（新しい語の行の doc・行 id・語）と直し方（当たった行の write-set に file を足す〔base に無い file は +〕か、語を当たった filter 語を含まない語に変える）が付き、当たりの無い repo の出力は 1 字も変わらず、TableError の variant と TABLE_ERRORS は変わらない (6) 予想を組む 1 本は + の file（check.rs の子 module）に在って judge_repo が全 doc の行から 1 回だけ組み、check_repo と repo_findings の両方に効き、teeth_places と teeth_outside の本体・declared_teeth・受付と preflight・判定行の字面・契約表の schema・rules 行は変わらず、現物の契約表の歯 2 本（contract_closure_ext_real_table_has_zero_findings / contract_names_declared_real_table_has_zero_findings）と既存の置き場の歯 contract_check_place_（lib と e2e）が緑 歯: contracts_prefix_collision_ の e2e 2 本（(a) の toy の表は、既知の語を先・新しい語 dial_knob_ を最後に並べた 2 語の nextest 行も持ち、その行でも dial_knob_ だけが新しい語として当たり、既知の語は当たらない）（crates/scribe2-boundary/tests/e2e/pipe/contracts.rs・toy の repo で contracts check を旗の有無の 2 周撃ち、出力の全行を測る）が、(a) 新しい語 dial_knob_ の行（別の doc・write-set に + の src/dial/knob.rs と既存の src/dial.rs と src/other.rs）の候補が module の path の語で knob.rs に絞られ、filter dial_（--lib）で write-set が src/dial.rs だけの行が knob.rs を持つ 1 件になり、同じ + を write-set に持つ行・scope が --test e2e の行・+ の file を持たない行の新しい語は当たらず、--verbose の行が予想の出所と直し方を持つこと (b) module の path の語に一致しない語 dial_turn_（候補は + の src/wheel.rs と src/other.rs）は候補の 2 つとも外に解ける行だけに当たって候補の 1 つを write-set に持つ行は当たらず、歯の区間の印を持たない file に在る名を語にした行は新しい語と読まれず、tests 欄を持つ導出の行は tests の file を置き場とすること を測り、base は予想を持たず findings=0 の判定行だけを出すので 2 本とも RED（機能不在）"

[[contract]]
id = "bg"
title = "審査の材料に契約の節と done が指す別の設計の § の本文を束ねる — 閉じた 5 形の参照を拾って行 id は契約表の section で § に解き、自分の § を除く § ごとの 1 塊（行を指せば done つき）と解けない参照の 1 行を外の材料の既存の塊の後ろに足し、束ねた本文の名は 1 段だけ名の照合に渡す（cap と材料の file の数は不変）。base の要約の行数は幅で畳んだ数を名乗り、生の行と違う file は生の行を添える（memo s2-07l.710 / s2-07l.728）"
req = ["FR49", "NFR1"]
section = "55"
write-set = ["crates/scribe2/src/pipe/review/outside.rs", "+crates/scribe2/src/pipe/review/outside/linked.rs", "crates/scribe2/src/pipe/review/base.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail linked_section_material_", "cargo nextest run -p scribe2 --lib --no-tests=fail review_base_lines_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_outside_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_base_"]
size = "M"
growth = ["crates/scribe2/src/pipe/review/outside.rs:180", "crates/scribe2/src/pipe/review/outside/linked.rs:240", "crates/scribe2/src/pipe/review/base.rs:40"]
done = "(1) 本文（節の本文〔導出物の行は goal〕・done・約束の行の text）から閉じた 5 形（<doc>.md §N とリンクの字の中の [<doc>.md §N](…)・行き先が .md のリンクの直後の §N・<doc>.md#<id>・<doc>.md 行 <id> と <doc>.md の行 <id>・doc の字を前に持たない同じ doc の §N と 行 <id>）を本文に現れた順に拾い、直後が . と数字の §N と、直前の path の字の連なりが .md で終わらず英数字を持つ §N（ADR-0045 §2 など）は拾わず、<doc> は basename を契約の設計 doc と同じ dir の tracked の file に解く (2) 行 <id> と #<id> はその doc の契約表の行の section で § に解き、§ の本文は review.rs の私有の section_text を outside の子 module から可視性を変えずに呼んで読み（2 本目の節の読み手を書かない）、自分の § に解けた参照は捨て、同じ § は 1 塊に畳み、design が設計 pointer でない周は参照を拾わない (3) 解けた § ごとに頭の行 - <doc の path> §N と § の本文の各行を 2 字下げた行の 1 塊で、行を指した参照があれば塊の末尾に行ごとに 行 <id> の done: <done> の 1 行が付き、解けない参照は正規化した字面（<doc>.md §N・<doc>.md 行 <id>・§N・行 <id>）を本文に現れた順に重複なく並べた - 解けない参照: の 1 塊になる (4) 塊は outside.txt の既存の塊の後ろに解けない参照の塊、§ の塊（指された順）の順で足され、材料の file は 5 本のまま、参照を持たない契約と参照が全部自分の § と行に解ける契約（材料の節の本文の先頭の <doc>#<id> §N を含む）の外の材料は 1 字も変わらず、名指しも参照も無い契約は外の材料の file を置かない (5) § の塊の本文を mentioned_names に 1 回渡し、契約の本文が既に名指した名・file・dir を除いた残りを § の塊の後ろに §51 形 3 の (a) → (b) → (c) の形で足し、束ねた § の中の § の参照は辿らない (6) lens の残りの測り方と outside_block は変わらず、収まらない § の塊と名の塊は塊ごとの切り詰めの 1 行、それも収まらなければ落とした本数の 1 行になり、rules 行は足さない (7) outside.rs の PREAMBLE が解けない参照と § の塊と 1 段の名の塊が既存の塊の後ろに並ぶことと、data file と dir の配下の file の行数が改行で数えた生の行（wc -l と同じ）、.rs の要約の行数が幅で畳んだ数であることを書く (8) item_text の見出しは幅で畳んだ全体の数と生の行の数が違う file だけ全体の数の直後に（幅 <W> で畳んだ数・生の行 <n>）を添え、本体の数は畳んだ数のまま、2 つが等しい file の見出しは 1 字も変わらず、base.rs の PREAMBLE が畳む式（字数 ÷ 幅の切り上げ・最小 1・受付の上限の余地と同じ）と括弧の意味を書き、外の材料の名指された .rs の要約も同じ見出しになる (9) 材料の file の数と名・既存の塊の中身と並び・outside_block と base_block・lens の雛形と穴・diff の審査・design.txt と列外の鍵と焼き直しの門の物差し・mentioned_names と section_text の本文と可視性・FileLines と受付の上限の余地の式・data file と dir の行数の式・審査の観点と理由の型と rc は変わらない 歯: linked_section_material_（outside.rs の既存の歯の区間に 4 本・既存の helper で bundle を通す）が (a) 5 形の全部を持つ本文の § の塊 4 本の頭と並び（母集団を同じ assert で数える）・2 字下げの本文・行ごとの done の行と、自分の § と同じ § の 2 形と ADR-0001 §4 と §4.1 が塊を作らないこと、節の本文の先頭の 1 行と自分の § だけを指す本文が塊を作らないこと (b) 解けない参照の塊 1 本の字面と並びと位置 (c) 束ねた § だけが名指す名の塊が § の塊の後ろに在り、契約の本文も名指す名の塊は既存の位置に 1 本だけで、束ねた § が指す別の § の塊とその名の塊は無いこと (d) 残りが足りない周の § の塊の切り詰めの 1 行と、小さい § の塊と解けない参照の塊の残りと、説明の 1 行の並びを測り、review_base_lines_（base.rs の既存の歯の区間に 1 本）が幅 120 を越える行を持つ .rs の見出しの括弧と越えない .rs の見出しの不変（母集団 2 項目）と base_block と outside_block の説明の 1 行を測る。base の bundle は § の塊も解けない参照の塊も作らず、base の item_text の見出しは括弧を持たないので 5 本とも RED（歯は base に在る関数だけを呼び overlay の上で compile は通る）、既存の pipe_review_outside_ と pipe_review_base_ の in-file の歯は期待を変えずに緑"

[[contract]]
id = "bh"
title = "審査の base の要約の宣言の列が可視性の字面（pub / pub(crate) / pub(super) / pub(in …)・私有は字なし）を運び、私有でない struct は欄の名と可視性を添える — 宣言の読み手と閉じ括弧の数え手は 1 本のまま（memo s2-07l.736.1 の型 5・6）"
req = ["FR49", "NFR1"]
section = "56"
write-set = ["crates/scribe2/src/pipe/review/base.rs", "crates/scribe2/src/pipe/review/outside.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2/src/headless/lens.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail summary_visibility_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_base_", "cargo nextest run -p scribe2 --lib --no-tests=fail review_base_lines_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_outside_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_base_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_outside_room_"]
size = "S"
growth = ["crates/scribe2/src/pipe/review/base.rs:80", "crates/scribe2/src/pipe/review/outside.rs:60", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:2", "crates/scribe2/src/headless/lens.rs:4"]
done = "(1) base.rs の宣言の語の切りが可視性の字面（pub / pub(crate) / pub(super) / pub(in <path>)・無ければ空）も返し、declared_name は今と同じ <語> <名> の字面を返す（外の材料の候補の名・mod の行の照合・歯の名は不変） (2) 宣言の列の各項目は <可視性> <語> <名> で、私有は可視性の字を持たない (3) 可視性が私有でない struct は名の後ろに欄の列（名前つきは { <可視性> <欄の名>; … }・tuple は (<可視性> 0; …)・欄の区切りは ; で宣言の列の項目の区切り , と分ける・欄の無い struct は無し）を置き、欄は宣言の行から閉じ括弧か ; までの行のうち doc 行・注釈・属性の行を除き各行の行末の // から後ろを落とした字面を繋いで読み、名の直後（generic の <…> は山括弧を数えて飛ばし -> の > は数えない）が ( なら tuple・そうでなければ最初の { の中（{ より先に ; が来れば欄なし）を括弧の深さ 0 の , で割り、型は渡さず、私有の struct は欄を持たず、enum の variant は欄として出さない (4) 閉じ括弧までを数える block_end は outside.rs から base.rs へ本文を変えずに移り、outside.rs は base から取り込む (5) base.rs の PREAMBLE が、字の無い宣言と欄は私有（その module と子孫から見える）であることと、欄は名の後ろの括弧の中に ; で区切って並ぶことと、trait の impl の中の fn は trait に従うことを書く (6) 見出しの行・+ と読めない項目の 1 行・歯の列・base_block・外の材料の塊の並びと (a) の字面・declared_name と tooth_names の返す字面は変わらず、外の材料の (b) と §55 (i) の .rs の要約は同じ item_text を通るので base の要約と同じ可視性と欄を持つ 歯: summary_visibility_（outside.rs の既存の歯の区間に 2 本・tmp の dir に fixture の .rs を書き、base.rs の item_text と base_block を review の子孫の module から呼ぶ・期待を直す既存の歯を持つ base.rs には新しい歯を置かない＝flip-check の base.rs の turn は直した既存の歯の赤で必ず RED になり、同じ file の新しい歯の RED を隠す）が (a) 可視性 5 形の fn・名前つきの欄の pub の struct（pub と pub(crate) と私有の欄・doc 行と属性行つき）・tuple の pub の struct・欄の無い pub の struct・欄を 1 つ以上持つ私有の struct・variant を持つ pub の enum・impl の中の pub の fn の宣言の列の字面（母集団 = 宣言の本数を、列を , で割った同じ assert で数える・私有の struct と pub の enum は括弧を持たない） (b) 1 行に並ぶ欄・型に , を含む欄・行末の // 注釈に , を含む欄・generic の中に Fn(u8) の形の括弧を持つ名前つきの欄・型に -> を持つ欄の後ろにもう 1 つ欄を置く名前つきの struct（-> の > を閉じと数えると後ろの欄が落ちる）・名の直後の generic が -> を持つ tuple の pub の struct（generic の中に Fn(u8) と -> を持ち、括弧の中に pub の欄と私有の欄を 1 つずつ持つ形・山括弧を飛ばさないか -> の > を閉じと数えると tuple の欄が出ない）の読み方（偽の欄も tuple の読みも出ない）と base の説明の 1 行の読みを測り、base.rs の山括弧を飛ばす行と -> の > を数えない腕はどちらを消しても (b) が赤になる（便 s2-07l.736.3-20260928T092752Z の gate で、欄の最後に -> を置き tuple の generic を持たない fixture では 2 つを消しても歯の出力が同じと実測）。base の宣言の列は可視性の字を持たないので 2 本とも RED（歯は base に在る item_text と base_block だけを呼び overlay の上で compile は通る）。既存の歯の期待を新しい字面へ直す 5 本（base.rs の pipe_review_base_rs_lists_declarations_and_teeth_in_separate_columns・pipe_review_base_pub_in_path_visibility_declarations_are_listed・pipe_review_base_pub_in_without_closing_paren_is_not_a_declaration・pipe_review_base_place_only_item_is_read_and_marked_while_other_forms_stay と e2e の pipe_review_base_place_only_item_is_read_in_base_txt）は base で赤い普通の flip で、どの file にも retroactive の札を置かない。flip する file は base.rs・outside.rs・e2e/pipe/review.rs の 3 本で、flip-check は file ごとに 1 turn ずつ撃ち、outside.rs の turn は summary_visibility_ の赤だけで RED になる。他の pipe_review_base_ と review_base_lines_ と pipe_review_outside_ の歯は期待を変えずに緑 (7) (5) で PREAMBLE が長くなると、headless/lens.rs の既存の歯 headless_lens_outside_room_is_measured_after_the_base_stage の外の材料（z を 600 字）が base の段の後の残りに切り詰めの 1 行すら収まらず、落とした名の 1 行になって赤になる（便 s2-07l.736.3-20260928T083611Z の gate で実測）。この歯は base の段の長さ base を先に測り、外の材料の z の数を base + 600 にして、残りが PREAMBLE の長さに依らず切り詰めの 1 行を持つ形にする（assert は変えない）。直した歯は base でも緑なので、lens.rs の歯の区間の行頭に // flip-check: retroactive s2-07l.736.3 を置く（lens.rs は flip する file に数えない）"

[[contract]]
id = "bi"
title = "審査の外の材料に、契約の本文が 3 形（<path>.rs#<名>・<path>.rs の直後の <語> <名>・backtick の fn 形）で名指す write-set の中の item の本文（doc 行から閉じ括弧か ; まで）を名ごとの塊で足す — 名指しの読み手は names.rs の 1 本の口、並びは既存の塊の後ろ・別の設計の § の前、cap は既存の残り（memo s2-07l.736.1 の型 2・4）"
req = ["FR49", "NFR1"]
section = "56"
write-set = ["crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/closure.rs", "+crates/scribe2/src/pipe/review/outside/bodies.rs", "crates/scribe2/src/pipe/review/outside.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail named_item_body_", "cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_mentioned_", "cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_impl_", "cargo nextest run -p scribe2 --lib --no-tests=fail linked_section_material_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_outside_"]
size = "S"
growth = ["crates/scribe2/src/pipe/closure/names.rs:50", "crates/scribe2/src/pipe/closure.rs:1", "crates/scribe2/src/pipe/review/outside/bodies.rs:150", "crates/scribe2/src/pipe/review/outside.rs:130"]
done = "(1) 本文の列（節の本文〔導出物の行は goal〕・done・約束の行の text）から 3 形（(P) path の字の連なりが .rs で終わり直後に # と識別子が続く字面・backtick の内外を問わない、(F) <path>.rs の直後〔backtick・空白・の を飛ばす〕の <語> <名>・語は fn / struct / enum / union / trait / type / const / static / mod / impl、(N) backtick の中身が names.rs の form_of の fn 形）を、names.rs に足す 1 本の口が backticked と form_of と path の字の集合で拾い、(P)(F) の path も同じ口が names.rs の私有の path_matches（等しいか / 区切りの末尾一致）で (2) の探し先に解いて解けた path を返し（path の照合の写しを書かない）、closure.rs の pub use から開き、mentioned_names と unresolved_names の判定は不変 (2) 探し先は write-set の .rs の項目（外の材料の木が持つ接頭辞を剥がした path）のうち base に本文が在るものだけで接頭辞は見ず（+ の新設は base に無いので自然に外れ、素と = と - と ~ の項目は含む）、外の材料の木 Tree の field と bundle の引数は変えず、(P)(F) は (1) の口が返した path・(N) は探し先の全部を探し、宣言は base の本文の全行で語と名が一致するもの（(P) は語を問わない）、impl は行頭が impl で名を語の境界で持つ行（impls_type の照合を行 1 本に開いて使う）で、解けない名指しは何も足さない (3) 名指し 1 つが 1 塊で、頭の字面は 2 形だけ: 宣言が 1 file に在れば - <path>#<名>: <path>:<行>（<path> は (P)(F) は返した path・(N) は見つけた path・同じ file の同名は所在を , で並べて全部の本文）、(N) の名が探し先の 2 file 以上に在れば - <名>: 宣言 <n> か所（<path>:<行>, …）の 1 行だけ（§51 形 3 (a) と同じ字面・本文なし）、続きは直上の doc 行と属性行から block_end までを 2 字下げた行（fn は本体の閉じ括弧まで・const と static は ; まで） (4) 塊は外の材料の既存の (f) (d) (e) (a) (b) (c) の後ろ・別の設計の § の解けない参照の塊の前に名指しの順で並び、同じ path と名は 1 塊で、(j) が収まる周は後ろの § の塊が切り詰めの 1 行か落とした本数の 1 行になりうる (5) outside_block は変わらず、収まらない塊は切り詰めの 1 行、それも収まらなければ落とした本数の 1 行になり、rules 行は足さない (6) outside.rs の PREAMBLE が名指しの 3 形の本文の塊の位置と中身と、2 file 以上に在る名は所在の 1 行であることを書く (7) 既存の塊の中身と並び・材料の file の数と名・base の要約・lens の雛形と穴は変わらない 歯: named_item_body_（outside.rs の既存の歯の区間に 3 本・既存の helper で bundle を通す・fixture は歯の中の別の helper で足し共有の FILES と linked_scratch は変えない・行 bi の write-set の + の file には歯を置かない）が (a) write-set の中の fn・const・struct・impl を 3 形で名指した本文の塊の頭と並びと本文の範囲（母集団 = 塊の頭を同じ assert で数える・塊は - <path>#<名> の頭で引く） (b) 宣言を持つ tracked の file を指す write-set の外の path（toy の木の shape.rs の zq_area を (P) で名指す）・宣言の無い名・大文字始まりの tuple variant の字面（write-set の fixture の .rs に pub fn ZqWrap() {} を置き、本文に ZqWrap(1) を backtick つきで書く）が塊を作らず、2 file に在る (N) の名が - <名>: 宣言 2 か所（…）の 1 行と等しいこと (c) 残りが足りない周の切り詰めの 1 行と説明の 1 行を測る。base の bundle は名指しの本文の塊を作らないので 3 本とも RED（歯は base に在る bundle と outside_block だけを呼び overlay の上で compile は通る・札は置かない）、既存の pipe_review_outside_ と linked_section_material_ と closure_names_ の歯は期待を変えずに緑"
depends = ["bh"]

[[contract]]
id = "bj"
title = "審査の外の材料の別の設計の § の参照が、doc の字なしの 行 <id> を同じ dir の契約表の置き場（.md の区間と .toml の全文）でちょうど 1 つに在るときその行に解き、.toml の置き場の行と § は goal を本文にする — 2 つ以上に在る id は数を添えた解けない参照（memo s2-07l.736.1 の型 7）"
req = ["FR49", "FR47", "NFR1"]
section = "56"
write-set = ["crates/scribe2/src/pipe/review/outside/linked.rs", "crates/scribe2/src/pipe/review/outside.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail note_row_lookup_", "cargo nextest run -p scribe2 --lib --no-tests=fail linked_section_material_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_outside_"]
size = "S"
growth = ["crates/scribe2/src/pipe/review/outside/linked.rs:70", "crates/scribe2/src/pipe/review/outside.rs:130"]
done = "(1) doc の字を前に持たない 行 <id> が契約の設計 doc の表に無いとき、設計 doc と同じ dir の直下の tracked の契約表の置き場（form_of が読む .md の区間と .toml の全文・tracked の列で引き dir を disk から読まない・別の dir は引かない）のうちその id の行を持つ置き場がちょうど 1 つならその行の § の塊（done つき）に解け、0 なら今の字面の解けない参照、2 つ以上なら解けない参照の字面の後ろに（同じ dir の <n> 置き場に在る）が付き、同じ doc に在る id は同じ doc を先に読んで別の置き場を引かない（自分の doc は置き場の数に入れない） (2) <file>.toml#<id> と <file>.toml 行 <id> と <file>.toml の行 <id> を拾い、§N の形は .md のまま (3) .toml の置き場の行を指した参照は § の本文の代わりにその行の goal を塊の本文にし、契約の置き場が .toml のときの doc の字なしの §N は section が N の最初の行の goal を本文にし、goal が空なら解けない参照で、塊の頭は - <path> §<section> のまま、同じ置き場の同じ section を指す参照は 1 塊に畳んで本文は最初に解けた行の goal（§47 で goal は節の本文の逐語＝同じ section の行は同じ goal）、行を指した参照は .md と同じく 行 <id> の done: の行を末尾に足し、§N の参照は done の行を持たない (4) (A)(B)(E) の § の拾い方・自分の § の除外・同じ § の畳み・解けない参照と § の塊と 1 段の名の塊の並び・section_text は変わらない 歯: note_row_lookup_（outside.rs の既存の歯の区間に 2 本・歯の中の別の helper で §55 の toy の木に同じ dir の .md と .toml の置き場と囮 3 つ〔別の dir の置き場で同じ行 id を持つ行・disk に在って tracked の列に無い同じ dir の置き場・自分の doc に在る id を持つ別の置き場〕を足し、既存の linked_scratch は変えず、行 id は既存の歯の本文の id と重ねない）が (a) 自分の doc に無く同じ dir の別の 1 置き場に在る行 id の § の塊と done の行（別の dir と tracked でない置き場は数えない）、2 置き場に在る id の数を添えた解けない参照、どこにも無い id の今の字面、自分の doc と別の置き場の両方に在る id の自分の doc の § (b) .toml の pointer の形と 行 の形と契約の置き場が .toml のときの §N が goal を本文にした塊になり、同じ section の 2 行を指す参照が 1 塊（goal 1 つ・done の行 2 つ）になり、goal の空の行が解けない参照になることを測る。base の bundle は別の置き場を引かず .toml の basename を読まないので 2 本とも RED（歯は base に在る bundle だけを呼び overlay の上で compile は通る・札は置かない）、既存の linked_section_material_ と pipe_review_outside_ の歯は期待を変えずに緑"

[[contract]]
id = "bl"
title = "pipe/cli/intake.rs の「断りの組み立てと上限の余地」の群（19 item・正規化 165 行）を子 module へ割る — 純移動（名・本文・順序・doc comment 不変・歯 0 本・親に mod 1 行と use 2 文と歯用の 2 行・子側の pub(super) 12 名・孤立した import 5 語を削る・札 2 か所）"
req = ["FR83", "FR68"]
section = "57"
write-set = ["-crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "=crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_a_cap_shortfall_recounts_without_the_unresolved_items", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_core_headroom_counts_the_core_total_without_in_file_tests", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_growth_preflight_headroom_estimate_is_per_file", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_promise_verify_matches_as_a_set_or_is_refused_as_drift"]
size = "S"
growth = ["crates/scribe2/src/pipe/cli/intake/refusal.rs:210"]
done = "(1) 19 item（断りを組む口 4・Refuse を持たない断りの名 6・余地の判定と報告 3・rules 行の id 6）が名・本文・順序・doc comment を変えずに + の file へ移る（move_proof が pure と判じる・items-differ / residual-line 0 件） (2) 親に増えるのは空行を除いて 6 行だけ（use 群の直後の doc 1 行つきの mod 宣言 1 行と素の use 2 文〔本体が呼ぶ 10 名〕、既存の列 0 の #[cfg(test)] の直上の #[cfg(test)] だけの 1 行と use 1 行〔歯だけが引く 2 名〕）で、孤立した import 5 語を削る (3) in-file の歯の本文と use super::{…} が 1 byte も変わらず、e2e は触らない (4) 子側の pub(super) は親が呼ぶ 10 名と歯が引く 2 名の 12 名だけで、群の中だけの 7 名は私有のまま、親に残る型（Denial / Headrooms / Materials）とその field と親側の可視性と cli の再輸出は不変 (5) 札 moved が親の mod tests { の直後と子の module doc の直後に 1 行ずつで、子にも親の file 頭にも列 0 の #[cfg(test)] を足さない (6) 名指しの既存 4 本（in-file 1 本・e2e 3 本）が名・本数・本文不変で緑・clippy -D warnings が通常 build と test build の両方で rc 0"

[[contract]]
id = "bm"
title = "終端だけの撃ち直し（--terminal-only）が main の今の先端で照合の側を選ぶ — 着地した sha が先端なら今の照合、先端でなければ先端の sha つきの Behind（祖先なら先端の CI で close し reason に tip=・祖先でなければ close しない）、main を読めない周は何も撃たずに断る（FR50・§53 の限界）"
req = ["FR50"]
section = "58"
touches = ["crate::pipe::cli::step::terminal_only"]
write-set = ["crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/order.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_replay_tip_"]
size = "S"
growth = ["crates/scribe2/src/pipe/cli/step.rs:10", "crates/scribe2/src/pipe/land/finish.rs:2", "crates/scribe2-boundary/tests/e2e/pipe/land/order.rs:90"]
done = "(1) 終端だけの撃ち直しは記録の sha を読んだ後に anchor の refs/heads/main の今の sha を読み、読めない周は event を 1 件も書かず push も CI も close も撃たずに stderr 1 行（pipe: refs/heads/main を読めない）の rc 1 で断る (2) 着地した sha が先端の sha と等しい周は先端の側を渡し、自分の sha の CI で照合して reason landed <sha> ci=success で close する (3) 等しくない周は先端の sha つきの Behind を渡し、終端は push の後に祖先を測って、祖先の周は先端の sha で CI を照合し success なら reason landed <sha> ci=success tip=<先端> で close し、祖先でない周は CI を撃たず terminal:ci:unmeasurable を記して close しない（rc 1） (4) finish.rs は PushTip の doc comment だけが変わり、終端の本体・PushTip の 2 値・land_train・finish は変わらない (5) 終端の 7 値・event の詞・ci_now の判定・pipe.ci_wait_s と pipe.ci_poll_s の値・着地の周の先端の読み・close の reason の 2 形・撃ち直しの前提の段と記録の sha の読み・usage と help の字面は変わらない 歯: pipe_replay_tip_ の 3 本（order.rs）が (a) 既存の撃ち直しの歯を名と期待を書き直し、CI が failure の着地の後に別の commit が main を進めた撃ち直しで、CI の argv が main の今の sha を持ち着地した sha を持たず、reason が landed <着地した sha> ci=success tip=<今の sha> と等しいこと (b) 1 本の fn の撃ち直し 2 周で、main と偽 remote の main を着地した commit を祖先に持たない commit へ動かした周が rc 1・terminal=ci:unmeasurable で偽 CI も偽 bd も撃たず、着地した commit へ戻した周が自分の sha の CI で照合して reason landed <sha> ci=success（tip= なし）で close すること (c) refs/heads/main を消した撃ち直しが rc 1 で断りの 1 行を出し event を 1 件も書かないことを測り、base は (a) で着地した sha を照合し (b) の 1 周目で close し (c) で terminal:unreadable を記すので RED"

[[contract]]
id = "bn"
title = "remote を持たない repo の便は走査も push も CI の照合も撃たずに landed <着地 commit id> ci=none で close する — 終端の 7 値の Undeclared を ClosedWithoutCi（closed:no-ci・rc 0）に替え、close の理由の尾の書き手を 1 関数に寄せ、歯の道具箱の台帳の見張りが close を rc 0 で受け、host の PATH を積む 2 つの口を道具箱の上に積む（FR50・ADR-0094 の経路 (2)）"
req = ["FR50", "AC65"]
section = "59"
touches = ["crate::pipe::land::Terminal"]
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn/question.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_terminal_no_remote_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_terminal_land_outcomes_are_the_closed_seven", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_terminal_no_remote_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail e2e_ledger_tripwire_helper_run_to_landed_never_reaches_the_ledger"]
size = "M"
growth = ["crates/scribe2/src/pipe/land.rs:30", "crates/scribe2/src/pipe/land/finish.rs:30", "crates/scribe2-boundary/tests/e2e/main.rs:4", "crates/scribe2-boundary/tests/e2e/pipe.rs:12", "crates/scribe2-boundary/tests/e2e/pipe/land.rs:160", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs:10", "crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs:8"]
done = "(1) Terminal は 7 値のまま 2 つ目の Undeclared を ClosedWithoutCi（字面 closed:no-ci・rc 0）に替え、TERMINAL_TOKENS の 2 つ目も同じ字面で、doc comment を remote を持たない repo の便を CI の照合なしで close した値に直す (2) terminal は宣言を読めて remote が無い周に push と CI を撃たずに台帳の close を撃ち、理由は landed <着地した sha> ci=none（Behind の周も tip= なし）、通れば terminal:close:ok を 1 件記して ClosedWithoutCi、落ちれば今の CloseFailed で、走らなかった段の event を積まず、宣言を読めない周は今のまま close しない (3) close の理由を組む 1 関数が finish.rs に在り（land.rs が再輸出）、尾の閉じた 2 値（CI が success で先端を任意に持つ・CI の照合なし）から landed <sha> ci=success と landed <sha> ci=success tip=<先端> と landed <sha> ci=none の 3 形を返し、経路 (1) の 2 か所の format! もこの関数を呼ぶ（字面は不変） (4) --terminal-only は変えず、remote を持たない repo の便の撃ち直しが close して run=<id> terminal=closed:no-ci を出す (5) 歯の道具箱の台帳の見張りは 1 語目が close の呼び出しだけ argv を記録して rc 0 で返し（ほかは今のまま rc 127）、pipe/gate.rs の systemd_stub と pipe.rs の shim_path は host の PATH の代わりに crate::toolbox_path を自分の bin dir の後ろに積む (6) 着地が close を撃つようになって動く歯を直す: 見張りの記録 0 件の 3 か所（pipe.rs の e2e_ledger_tripwire_helper_run_to_landed_never_reaches_the_ledger・gate.rs の assert_child_measured_the_landed_commit・detection.rs の pipe_landed_detection_measured_round_records_the_landed_commit）は着地の close の 1 件だけ、landed_detail と landed_done_detail は terminal: の detail も飛ばし、already-landed の helper は末尾の terminal: の行を除いた最後の event を読み、detection.rs の Landed の detail の全件の比べは terminal: の行を除き、pipe.rs の注は terminal=closed:no-ci (7) 経路 (1) の段と字面と event・Unreadable と残る 5 値・landed_sha・open_pr・台帳の close・TERMINAL_POLARITY・usage と help は変わらない 歯: lib の pipe_terminal_no_remote_reason_tails_come_from_one_writer（land.rs の歯の区間・3 形の字面と ci=none の周に tip= が無いこと）と既存の pipe_terminal_land_outcomes_are_the_closed_seven の書き直し（rc 0 は closed と closed:no-ci）・e2e の pipe_terminal_no_remote_land_closes_with_ci_none（見張りの記録がちょうど 1 件で理由 landed <sha> ci=none・偽 CI 0 回・remote 0・Landed の着地の後ろは terminal:close:ok の 1 件）・pipe_terminal_no_remote_train_closes_each_run_with_its_own_sha（2 本の列で記録 2 件・各自の sha・tip= なし）・pipe_terminal_no_remote_unreadable_declaration_closes_nothing_until_refired（show HEAD:.vessel.toml を落とす偽 git で rc 1・記録 0・terminal:unreadable 1、偽 git を外した --terminal-only で rc 0・記録 1）・pipe_terminal_no_remote_pr_landed_run_writes_nothing（fake_terminal の repo の --pr-cmd の着地と --terminal-only の撃ちで偽 remote の ref・偽 CI・偽 bd・見張りがどれも 0）で、base は remote の無い周に close を撃たないので lib は compile error、e2e の (b)(c)(d) と直す 3 か所は assert で RED。着地の terminal:close:ok で RunDone stage=Landed が 2 件になる便を 1 件と数える既存の歯（crates/scribe2-boundary/tests/e2e/pipe/spawn.rs の 761 行・同じ file の done_count を引く spawn/question.rs の 66 行・land/follow.rs の 142 行・便 s2-07l.736.20-20260929T085829Z の問い about:write-set 2026-09-29T09:2xZ）を 2 件へ直す"

[[contract]]
id = "bo"
title = "land の終端と pipe retire が共有する問いの部品 — CI の結果を 4 値（success・failure・pending・unmeasured）で読む ci_read と、それの写しにした ci_now・PR の merge の commit を gh pr view <branch> --json state,mergeCommit で問う pr_merge（merged・not-merged・unmeasured）・台帳の 1 件の close_reason（FR96・ADR-0094 の経路 (3) の部品）"
req = ["FR96", "AC65"]
section = "60"
touches = ["crate::seat::ledger::Issue"]
write-set = ["crates/scribe2/src/fleet/wait.rs", "+crates/scribe2/src/pipe/dispatch/unreflected.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/hook/graph_guard.rs", "crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail retire_parts_", "cargo nextest run -p scribe2 --lib --no-tests=fail invocation_fleet_ci_query_names_the_program_and_cwd"]
size = "M"
growth = ["crates/scribe2/src/fleet/wait.rs:120", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/seat/ledger.rs:20", "crates/scribe2/src/hook/graph_guard.rs:1", "crates/scribe2/src/ledger/form.rs:1", "crates/scribe2/src/pipe/dispatch/precheck.rs:1"]
done = "(1) wait.rs に ci_read（ci_now と同じ引数・子 process 1 回）と閉じた 4 値（success・failure・pending〔run が 0 本か、落ちた run が無く走っている run が在る〕・unmeasured〔行を撃てない・rc が 0 でない・JSON を読めない〕）が在り、判定の順は ci_now と同じで、ci_now は ci_read の写し（success と failure は今の 2 形・残る 2 値は None）で外形と呼び手が変わらず、fleet の再輸出に 2 名が足される (2) wait.rs に pr_merge（repo と branch の名・子 process 1 回・起動の記述を通る）と閉じた 3 値（merged〔merge の commit id〕・not-merged・unmeasured）が在り、撃つのは gh pr view <branch> --json state,mergeCommit（cwd は repo・shell を通さない・--repo を渡さない）で、state が MERGED で mergeCommit.oid が 40 桁の 16 進なら merged、他の state は not-merged、起動の失敗・rc が 0 でない・JSON を読めない・MERGED で oid が無いか形が違う周は unmeasured (3) Issue が close_reason（文字列・欄が無ければ空）を持ち issues_of が読み、構築点の 3 か所は空で足す (4) ci_now の外形と 3 形・land の終端の CI の待ちと照合・CiRun・BD_ARGS と待ち上限・issues_of の必須 2 key は変わらない 歯: lib の retire_parts_ci_read_splits_pending_from_unmeasured（wait.rs・Stub で 7 つの答えが 4 値に分かれ ci_now は Some(Success)・Some(Failure)・None×5）・retire_parts_pr_merge_reads_the_state_and_the_merge_commit（wait.rs・MERGED と 40 桁は merged・OPEN と CLOSED は not-merged・null と 39 桁と rc 1 と JSON でないは unmeasured・呼び出し 1 回で program gh・引数 pr view <branch> --json state,mergeCommit・cwd は repo）・retire_parts_issue_reads_the_close_reason（seat/ledger.rs）・不変の invocation_fleet_ci_query_names_the_program_and_cwd で、base に無い関数と欄を引く compile error で RED"

[[contract]]
id = "bp"
title = "PR で着地した便を pipe retire が照合してから close し worktree を畳む — worktree の確かめ・--fold-only・閉じ済みの契約・宣言の remote・forge の merge の commit・remote の main の先端の祖先・先端の CI の順に問い、通れば landed <merge> ci=success [tip=<先端>] で close してから可逆に畳み、通らない周は閉じた 6 語の 1 行と rc 1 で何も書かない（FR96・AC65 (c)〜(e)・ADR-0094 の経路 (3)）"
req = ["FR96", "AC65"]
section = "61"
depends = ["bn", "bo"]
touches = ["crate::pipe::retire::Retire"]
write-set = ["crates/scribe2/src/pipe/retire.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pr_retire_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_retire_"]
size = "L"
growth = ["crates/scribe2/src/pipe/retire.rs:200", "crates/scribe2/src/pipe/cli/step.rs:25", "crates/scribe2/src/pipe/cli/args.rs:2", "crates/scribe2/src/pipe/cli.rs:2", "crates/scribe2/src/pipe/land/finish.rs:1", "crates/scribe2/src/pipe/land.rs:1", "crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs:340"]
done = "(1) 段が Landed でその RunDone の detail が pr の便だけが経路 (3) を通り、他の対象は今のまま畳むだけで forge に問わず台帳に書かない (2) worktree が無いか clean でない周は worktree-unready（git と forge と台帳に問わない） (3) --fold-only（値なし・ALLOWED_RETIRE に足す）の周は照合も close もせず可逆な move で畳み契約を開いたまま残す (4) --bd の台帳を 1 回読み（待ち上限は rules 行 seat.ledger_timeout_s）、便の bead が closed で close_reason の頭の語が landed なら照合も close もせずに畳み、読めない周と待ち上限の行が無い周は unmeasured (5) terminal_facts が読めないか remote が無い周は unmeasured（forge に問わない） (6) pr_merge を branch_name の名で撃ち、not-merged は not-merged、unmeasured は unmeasured (7) git ls-remote <remote> refs/heads/main で先端を読み git fetch --no-tags --no-write-fetch-head <remote> refs/heads/main で object を取り、落ちた周と先端の object が無い周は unmeasured、merge の object が無い周と merge-base --is-ancestor が rc 1 の周は not-ancestor、他の rc は unmeasured (8) ci_read を先端と宣言の ci-cmd で 1 回だけ撃ち（待たない）、failure と pending は ci-not-success、unmeasured は unmeasured (9) 理由を §59 の 1 関数で組んで（merge の commit id・CI が success・先端が merge と違う周だけ tip=<先端>）close し、落ちた周は unwritten で畳まず、通った周は RunDone stage=Landed detail=terminal:close:ok を 1 件記す (10) close の後に retire_worktree で move して detail=retired を 1 件記し（段は Landed のまま）、move が落ちた周は worktree-unready で close は残る (11) 通らない周は stdout 1 行 run=<id> retire=<語>・rc 1・event と台帳の書き 0（close の後の move の失敗だけ形 9 の 1 件が残る）で、語は閉じた 6 語（宣言順 worktree-unready・not-merged・not-ancestor・ci-not-success・unmeasured・unwritten）の enum と const slice、close して畳んだ周の stdout は run=<id> retired=<畳んだ先> close=ok、畳むだけの周は今の run=<id> retired=<畳んだ先> (12) retire_run が manifest を受け（cli.rs の 1 か所）--bd と --fold-only を Retire に運び、usage と help の字面は変わらない (13) e2e の PR の便の既存の 2 本（pipe_retire_moves_pr_landed_worktree_and_keeps_branch・pipe_retire_refuses_unless_landed_and_clean）は retire.rs の中の --fold-only を足した撃ちの helper で撃ち（親 land.rs の retire_once は不変）、期待は変わらない (14) land/finish.rs の close_reason・CloseTail・CLOSE_REASON が pub(in crate::pipe) で land.rs の pub(in crate::pipe) use finish::{…} の列に在り（中身と字面は不変）、retire.rs はこの 1 関数で理由を組む 歯: e2e（land/retire.rs・偽 remote と ci-cmd の偽 CI と PATH の偽 gh と --bd の偽 client の fixture）の pr_retire_closes_then_folds_when_the_merge_is_under_a_green_tip（先端そのものと先端でない 2 周・close 1 回で理由 landed <merge> ci=success と tip=<先端>・retired へ移り branch は残る・Landed の後ろは terminal:close:ok と retired・偽 gh と偽 CI の argv）・pr_retire_refuses_with_one_closed_word_and_writes_nothing（9 つの fixture が閉じた 6 語・rc 1・stdout 1 行・worktree 残る・event と偽の台帳が不変・worktree-unready と remote の無い周は偽 gh 0 回・worktree-unready の撃ち直しが success で close）・pr_retire_fold_only_and_closed_contracts_fold_without_checks（--fold-only が not-merged で畳み close 0 と偽 gh 0・retired/<id> の先置きで close の後の move が落ちて worktree-unready・退けた撃ち直しが close も偽 gh も増やさずに畳む）で、base は PR の便を forge に問わず畳み --fold-only を断るので assert で RED"

[[contract]]
id = "bq"
title = "入れ子の source の根を vessel 宣言の任意 key crate-roots（末尾 / の dir の配列・固定の根 crates/ に足すだけ）で宣言し、追随の再 gate の要否と着地後の検出線の面と受付の 1 file と core の上限の余地が、宣言した根の下の crate を crates/ の直下の crate と同じ 1 関数で読む（宣言を読めない周は撃つ側・§62）"
req = ["FR34", "FR48", "NFR4"]
section = "62"
write-set = ["+crates/scribe2/src/pipe/declaration/crate_roots.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/detection.rs", "crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_crate_roots_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_crate_roots_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_crate_roots_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_landed_detection_crate_roots_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_detection_scope_needed_is_a_closed_prefix_set"]
size = "L"
growth = ["crates/scribe2/src/pipe/declaration/crate_roots.rs:210", "crates/scribe2/src/pipe/declaration.rs:8", "crates/scribe2/src/pipe/declaration/optional_keys.rs:8", "crates/scribe2/src/pipe/declaration/write_set.rs:60", "crates/scribe2/src/pipe/land.rs:56", "crates/scribe2/src/pipe/land/detection.rs:40", "crates/scribe2/src/pipe/cli/intake/refusal.rs:3", "crates/scribe2/src/pipe/cli/intake.rs:2", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:80", "crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs:70", "crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs:50"]
done = "(1) vessel 宣言の任意 key crate-roots（repo 相対の dir の配列・各項目の末尾は /）を読み、書かない宣言と宣言 file の無い repo は今と同じで schema は 1 のまま (2) 固定の根 crates/ は常に在り key は足すだけで置き換えない (3) 空・末尾 / 無し・絶対 path・home の短縮記号・..・crates/ と重なる・項目どうしで重なる項目は key と行番号を名指す不備 (4) 根の列と path から根・crate の名・残りを返すか無しを返す pure な 1 関数と HEAD の宣言から根を読む口を子 module に置き、親が再輸出する (5) DETECTION_SCOPE の値と detection_needed は変えず、子の HEAD の口の閉じた 3 値と path の列から面に触れるかを返す 1 本を land.rs に足し、regate_skippable（否定）と touches_scope がそれを通る (6) 宣言が在って読めない周は 1 本が触れるを返す（regate_skippable が偽・touches_scope が真・呼び手の 2 つを読めない宣言の repo で直撃する lib の歯 (f)(g) で測る） (7) core_of を根の列の形にし、headroom_shortfalls と Caps は変えずに根の列を受ける呼び口を足して親が再輸出し、exclude_cap_shortfall が materials.facts の根で呼ぶ (8) TableFacts に根の列の欄を足して table_facts_named が埋め、歯の区間の字面 1 か所に固定の根の値を足す（retroactive の札） (9) doctor・stdout・event の key は足さない (10) dispatcher の行 ai・ak と並走しない (11) crate_roots.rs は本体も歯も WriteSetItem を名指さず（行 ar の閉包を広げない）、余地の歯は write_set.rs の歯の区間に置き、fixture の home の短縮記号は concat! で割る（paths-clean） 歯: declaration_crate_roots_ が key の有無と不備 7 形・関数の表・面の表・余地の名指し・HEAD の口の 3 値と読めない周の面を、pipe_intake_crate_roots_ が宣言した根の下の file と core の余地の断りと key の無い repo の通過を、pipe_land_crate_roots_ が宣言した根の下だけの main の動きで再 gate を撃つことと key の無い repo の regate=skipped を、pipe_landed_detection_crate_roots_ が宣言した根の下だけの着地で stub を 1 回呼ぶことと key の無い repo の outside-scope を測る。base は key が未知の key の不備で RED"

[[contract]]
id = "br"
title = "閉包と歯の置き場の導出・nextest 行の組み立て・新しい file の親の候補・名の衝突の予想が、行 bq の根の列の 1 関数で入れ子の crate を crates/ の直下の crate と同じに読む — Context と Base に根の列の欄を足し、2 つの CRATES_DIR を消す（§63）"
req = ["FR48", "FR47"]
section = "63"
depends = ["bq"]
write-set = ["crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/table/check/collide.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail closure_crate_roots_", "cargo nextest run -p scribe2 --lib --no-tests=fail contracts_collide_crate_roots_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_crate_roots_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_nest_roots_"]
size = "M"
growth = ["crates/scribe2/src/pipe/closure/derive.rs:60", "crates/scribe2/src/pipe/table/check/collide.rs:40", "crates/scribe2/src/pipe/table.rs:3", "crates/scribe2/src/pipe/table/check.rs:12", "crates/scribe2/src/pipe/cli/intake.rs:4", "crates/scribe2/src/pipe/contract.rs:8", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs:80", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:120"]
done = "(1) Context と Base に根の列の欄を足し、本体の字面は TableFacts の根で埋め、歯の区間の字面（Base 10・Context 7）に固定の根の値を足す（retroactive の札） (2) crate_relative・parent_candidates・target_of・collide.rs の module_path が §62 の 1 関数で path を割り、2 つの CRATES_DIR を消す (3) verify の -p <crate> は宣言したどの根の下の同じ名の crate にも当たる (4) nextest_line と promised_verify が根の列を受け、呼び手が材料の根を渡し、歯の区間の呼び出しは固定の根（retroactive の札） (5) parent_candidates はどの根でも <根><crate>/src の直下に lib.rs と main.rs の候補を足す (6) module_path は宣言した根の下の src/ と tests/ の後ろの段を読む (7) 宣言の無い repo の導出の結果・判定行・findings の字面は不変 (8) 行 bq の着地の後に走る 歯: closure_crate_roots_ が入れ子の歯の置き場（同じ名の crate の 2 根で 2 file）・nextest 行の旗・親の候補を宣言の有無の 2 形で、contracts_collide_crate_roots_ が module の段（src/ と tests/ の後ろ）を 2 形で、contract_crate_roots_ が宣言した repo の teeth-outside-write-set の 1 件と宣言の無い repo の 0 件、入れ子の名の衝突の予想の 1 件と宣言の無い repo の 0 件を、pipe_intake_nest_roots_ が受付の経路の 3 つの読み（判定の関数の base_of が Declared 行の歯を入れ子の file に解き preflight の stdout の refuse= の行が teeth-outside-write-set と入れ子の file を名指して stderr は空・判定の関数の base_of が Derived 行の歯の置き場を入れ子に解く・約束の行の promised_verify の呼び手が -p toy --lib の verify と入れ子の file の write-set を写しに書き、promised_inputs の中の nextest_line の呼び出しが根を渡して別の crate の同じ接頭辞の歯を write-set に入れない）を測り、受付の Context の根の欄は受付の経路に読み手が無いので測らない（限界）。base は欄と関数の形が無く RED"

[[contract]]
id = "bs"
title = "審査の lens に done の番号つき項目ごとの歯の対応の表を出させ、表の欠けた判定を INCONCLUSIVE に、歯の無い項目を持つ PASS を FAIL（vacuous-assert）に倒し、歯の無い項目を全部 at に名指す（§64）"
req = ["FR49", "FR9"]
section = "64"
write-set = ["+crates/scribe2/src/pipe/review/items.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_done_items_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_done_items_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_kind_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_kind_fail_keeps_kind_and_at_in_review_json_and_two_word_detail", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_kind_missing_or_unknown_or_unreadable_falls_to_unparsed_without_moving_the_verdict", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_reuse_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_promise_"]
size = "M"
growth = ["crates/scribe2/src/pipe/review.rs:70", "crates/scribe2/src/headless/lens.rs:50", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:260", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:45"]
done = "(1) done の字を頭から見て (1) から 1 ずつ増える番号の印が順に現れた所で項目に区切り、順の外の印は本文の一部、番号の印は半角の括弧と ASCII の数字だけ（全角の括弧と全角の数字は印でない）、(1) を持たない done は 0 個で、読み手は review/items.rs の 1 関数を材料の書き手と判定の読みが呼ぶ (2) 項目が 1 個以上で Promised でない行の審査だけ材料の dir に items.txt（見出し・K 個の番号を並べた表の形の行・項目を 1 行ずつ）を置き、lens は契約の本文の後ろに空行 1 つを挟んで items.txt の本文を足し、cap に items.txt の byte を数え、在るのに読めない周は rc 2 で、置かない行の材料の dir と prompt は 1 字も変わらない (3) lens の最終行の key done の文字列を , で割り空白を剥がし空を落とした各項目を 番号:歯 と読み（番号は ASCII の数字・歯は空でない字）、歯の - は落ちる歯が無いこと、表が揃うのは番号が 1〜K をちょうど 1 回ずつ覆い形の合わない項目が無い周 (4) 項目 1 個以上で Promised でない行の、lens の JSON が読め verdict が 3 値の周だけ、表が揃わない周は INCONCLUSIVE・unparsed に倒して evidence の頭に理由（key done が無い・文字列でない・無い番号・余る番号・重なる番号・形の合わない項目の件数）を置き、at は lens の verdict が PASS でない周だけ lens の値、表が揃い - を持つ PASS は FAIL・vacuous-assert に倒して at を - の番号の done(n) の列・evidence を歯の無い done の項目と lens の evidence にし、表が揃い - を持つ FAIL / INCONCLUSIVE は verdict と kind を保ち at の末尾に lens の at に無い done(n) を番号の順に足し evidence の末尾に歯の無い done の項目を足し、- の無い表と、項目 0 個・Promised・JSON が読めない・3 値でない・rc が 0 でない・lens に届かない周は判定を変えない (5) 倒しは read_outcome の中で parse_lens の直後に 1 回で、lens を撃った周と先撃ちを使い回した周が同じ関数を通り、review が項目の数を渡す (6) review.json の key の集合と schema 1・event の detail の形・理由の 7 語・items.txt の無い周の prompt・Promised の行の審査・使い回しの鍵と手順は変わらない 歯: lib の pipe_review_done_items_（review.rs の tests・(a) 項目の読み）と headless_lens_items_（lens.rs の tests・(b) prompt への足し・cap・読めない材料）、e2e の pipe_review_done_items_（review.rs・(c)〜(f) は順の外の印を持つ同じ 3 項目の行 (c) 材料と番号を持たない行 (d) PASS の倒しと空白と末尾の , を持つ揃った表 (e) FAIL と INCONCLUSIVE への足しと重ねない at (f) 表の欠けの 7 形と欠けた FAIL の at と器の INCONCLUSIVE の 4 形（rc 7 の周は stdout に PASS と - を持つ揃った表を書き、MAYBE の周は - を持つ揃った表を持ち、どちらも evidence に done の対応の表も歯の無いも含まず at 無し） (g) 先撃ちの使い回し、intake.rs・(h) Promised の行と素の番号つきの行）、変わらない既存の歯 headless_lens_・pipe_review_kind_（lib）・pipe_review_kind_fail_keeps_kind_and_at_in_review_json_and_two_word_detail・pipe_review_kind_missing_or_unknown_or_unreadable_falls_to_unparsed_without_moving_the_verdict・pipe_review_reuse_・pipe_intake_promise_、base は items.txt を置かず key done を読まないので (c)〜(h) が RED で (a)(b) は compile で落ちる"

[[contract]]
id = "bt"
title = "終端だけの撃ち直しは、記録の sha が main の今の先端と違う周に、先端の祖先から本文に run: <run id> の行を持つ squash を既存の読み手で探し直し、見つけた sha を着地の sha として §58 の分岐へ渡す — push が non-fast-forward で止まり anchor の main を揃えた（着地の commit の sha が変わった）便が撃ち直しで閉じる（FR50・§58 の限界・§65）"
req = ["FR50"]
section = "65"
depends = ["bm"]
touches = ["crate::pipe::cli::step::terminal_only"]
write-set = ["crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/order.rs", "=crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_replay_relocate_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_replay_tip_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_already_landed_"]
size = "S"
growth = ["crates/scribe2/src/pipe/cli/step.rs:12", "crates/scribe2/src/pipe/land.rs:2", "crates/scribe2-boundary/tests/e2e/pipe/land/order.rs:110"]
done = "(1) 終端だけの撃ち直しは、記録の sha が anchor の refs/heads/main の今の sha（先端）と等しくない周に、land.rs の既存の読み手 landed_squash_of で先端の祖先から本文に run: <run id> と字面の等しい行を持つ squash を 1 回探し、見つかればその sha を着地の sha として先端と比べ直す (2) 見つけた sha が先端と等しい周は先端の側を渡し、見つけた sha の CI で照合して reason landed <見つけた sha> ci=success（tip= なし）で close し、等しくない周は先端の sha つきの Behind を渡し、終端は祖先を測って先端の sha で CI を照合し reason landed <見つけた sha> ci=success tip=<先端> で close する (3) 見つからない周（当たった commit の本文に run: <run id> と字面の等しい行が無い周を含む）は記録の sha のまま今の分岐で、終端は CI も台帳も撃たず terminal:ci:unmeasurable を記して close しない（rc 1） (4) event の詞・終端の 7 値・stdout の 1 行・記録の sha の読み（landed_sha）は変わらず、器は main も偽 remote の main も動かさず、見つけた sha は CI の argv と close の reason にだけ現れる (5) land.rs は landed_squash_of の可視性と doc comment だけが変わり、既着地の便の読み（rebase_onto の呼び）と finish.rs は変わらない 歯: pipe_replay_relocate_ の 2 本（order.rs）が (a) 1 本の fn の撃ち直し 2 周で、着地した commit の親の上に別の便の commit を積み、その上に着地した commit の本文の run の行だけを run: <run id>-x に替えた写しと、写しの上の 1 commit を置いて main と偽 remote の main をその先端へ付け替えた 1 周目が rc 1・stdout run=<id> terminal=ci:unmeasurable・偽 CI の呼び出しが増えず偽 bd が撃たれないこと、同じ別の便の commit の上に本文を字のまま写した写しと、写しの上の 1 commit を置いて付け替えた 2 周目が rc 0・stdout run=<id> terminal=closed・CI の argv が先端の sha を持ち着地した sha も写しの sha も持たず・偽 bd の reason が landed <写しの sha> ci=success tip=<先端の sha> と等しく・main と偽 remote の main が先端のまま・Landed の後ろが push・ci:failure・push・ci:unmeasurable・push・ci:success・close:ok の 7 件であること (b) 本文を字のまま写した写しそのものを main と偽 remote の main の先端にした撃ち直しが rc 0・CI の argv が写しの sha を持ち着地した sha を持たず・偽 bd の reason が landed <写しの sha> ci=success と等しい（tip= を持たない）ことを測り、変わらない既存の歯 pipe_replay_tip_（記録の sha が先端の祖先の周・trailer を持たない兄弟の commit の周・main を読めない周）と pipe_land_already_landed_（既着地の便の読み）が緑のまま、base は (a) の 2 周目と (b) で記録の sha が先端の祖先でないので CI を撃たず close しない（rc 1）ので RED"

[[contract]]
id = "bu"
title = "欄と宣言の key を読むだけの行 — 契約表の行の任意の欄 done-teeth と code-facts・契約 file の任意 key done-teeth・vessel 宣言の任意 key teeth-check と index-scip と index-roles を読んで形だけ確かめ、値は捨てて何にも効かせない（§67）"
req = ["FR47", "FR53"]
section = "67"
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "contracts/schema.toml", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_fields_read_only_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_fields_read_only_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_need_conditional_is_two_fields_in_the_schema", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_whole_goal_head_pins_the_folio2_schema_and_the_goal_stays_off_the_fields", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_schema_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_growth_schema_lists_growth_as_an_optional_list", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/table.rs:8", "crates/scribe2/src/pipe/table/parse.rs:6", "crates/scribe2/src/pipe/contract.rs:40", "crates/scribe2/src/pipe/declaration/optional_keys.rs:35", "crates/scribe2/src/pipe/declaration.rs:6", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs:140"]
done = "(1) 契約表の行の欄の正本 FIELDS の末尾（growth の後）に任意の文字列の列の欄 done-teeth と code-facts をこの順に足して 20 欄（必須 5・条件付き 2・任意 13）にし、contracts/schema.toml を contracts schema の出力で作り直す〔書き換える既存の歯 table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes・contract_promise_need_conditional_is_two_fields_in_the_schema・contract_whole_goal_head_pins_the_folio2_schema_and_the_goal_stays_off_the_fields と、変わらない既存の歯 contract_schema_ の 2 本・contract_growth_schema_lists_growth_as_an_optional_list〕 (2) 行の型付けは 2 欄を既存の配列の読みで読んで値を捨て（ContractRow に field を足さない）、2 欄を持つ行の doc の contracts check は rc 0・findings 0 で、2 欄を消した同じ repo と判定行が同じ字〔e2e の contract_fields_read_only_ の (a)〕 (3) 2 欄に文字列を書いた行は、欄の名と「は文字列の配列でなければならない」の字で欄の行番号に名指され rc 2〔(b)〕 (4) 契約 file の任意 key に done-teeth を足し、既存の配列の読みで読んで値を捨て（Contract に field を足さない・render は書かない）、done-teeth を持つ契約 file は持たない同じ file と等しい Contract に読め、文字列の done-teeth は key の名を持つ不備になり「未知の key」とは言わない〔lib の contract_fields_read_only_ の (c)〕 (5) vessel 宣言の DECLARED_KEYS と OPTIONAL_KEYS の末尾に teeth-check・index-scip・index-roles をこの順に足して 20 key にし、宣言の読みは teeth-check を真偽・index-scip と index-roles を文字列の配列として読んで値を捨て（Declared にも便の写しにも field を足さない）、3 key を持つ宣言の repo の contracts check は 3 key を消した同じ repo と判定行が同じ字で rc 0〔(a)・書き換える既存の歯 declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema〕 (6) 3 key に文字列を書いた宣言は rc 2 で、stderr が key の名と、teeth-check は真偽・index-scip と index-roles は配列の字を持つ〔(b)〕 (7) 足した code はほかの行の touches の型を今それを名指していない file で新しく名指さず、契約表の閉包を広げない〔verify の 9 行目の契約表の検査〕 歯: e2e の contract_fields_read_only_（contracts.rs・(a) 欄 2 つの行と key 3 つの宣言を持つ repo と、それらを消した同じ repo の判定行の一致と rc 0 (b) 形の違い 5 形の名指し）、lib の contract_fields_read_only_（contract.rs の既存の test 区間・(c)）、書き換える既存の歯 4 本（table.rs の 3 本は欄の宣言順と数 20 と任意 13・declaration.rs の 1 本は key の列 20 本と doc comment の本数）は base の FIELDS と DECLARED_KEYS に対して RED になり flip するので retroactive の札は要らない。base は未知の欄と key で区間・契約 file・宣言を読めないので (a)(b)(c) が RED"

[[contract]]
id = "bv"
title = "write-set の置き場だけの項目（=）の file を runner の allowlist から外し、gate の write-set を照らす段が便の diff に在る = の file を名指して落とす — 欄 done-teeth の有無に依らずどの便にも効く（§66 行 (4)・形 0 の表の place-only）"
req = ["FR4", "FR20", "FR8"]
section = "66"
write-set = ["crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_place_only_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_spawn_write_policy_strips_the_item_prefixes", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_write_set_prefixed_items_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_fails_when_diff_leaves_write_set", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
growth = ["crates/scribe2/src/pipe/spawn.rs:8", "crates/scribe2/src/pipe/gate/verify.rs:25", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs:70", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs:120"]
depends = ["bu"]
done = "(1) spawn が worktree の git dir に書く runner の allowlist（write-set の policy）から write-set の = の項目を外し、ほかの項目は今の剥がし（refuse の normalize の 1 本）のまま同じ順に書く (2) spawn した worktree で、= の項目の file への Edit を hook の pre-tool-use に渡すと deny（rc 2）で、同じ契約の素の項目の file への Edit は通る (3) gate の write-set を照らす段は、便の diff（base..HEAD の name-only）の path のうち = の項目に当たる path を、write-set の外の path と同じ段・同じ rc 1 で、既存の見出しと同じ形の新しい見出し 1 つ（契約の write-set の = の file が便の diff に在る）の下に 1 行ずつ名指して FAIL にする（段・理由の型・verdict を足さない）。runner が Bash で書き換えて commit した便もこの段で落ちる (4) = の file に触れない便はこの段が rc 0 で、= の項目を持たない契約の便はこの段の測りで落ちない (5) 足した歯の名は、ほかの行の歯の置き場を広げない（設計 doc の検証行の filter 語を名の部分に持たない） 歯: e2e の done_teeth_place_only_ の 2 本。spawn.rs の 1 本は (1)(2): 素の項目 1 つと base に在る file の = の項目 1 つの契約を spawn し、policy が素の項目の 1 行だけであることと、= の file への Edit の deny と素の file への Edit の通過を測る。gate.rs の 1 本は (3)(4) の前半: 同じ形の契約で = の file を sh で書き換えて commit する偽の runner の便が FAIL・段 ① の rc 1・stderr の新しい見出しの下にその path、同じ repo の別の bead で素の項目だけを書く便が PASS・段 ① の rc 0。(4) の後半（= の項目を持たない契約の便が落ちない）は verify の 2〜4 行目の既存の歯（本文を変えない）が測り、(5) は verify の 5 行目の契約表の検査が測る。base は = の項目を policy に書き、段 ① が = の path を write-set の内と読むので、新しい 2 本は base で RED"

[[contract]]
id = "bw"
title = "契約表の行の欄 done-teeth を ContractRow に読み、done の番号つき項目ごとの歯の対応を表の検査と受付の同じ 1 つの判定で照らす — 形・覆い・検証行の番号・検証行の選び・仕組みの測る物を全件と行番号で名指し、受付と preflight は既存の歯の base の在りかも照らして、外れた行の便を作らない（§66 行 (1) の照らし）"
req = ["FR105", "FR47", "FR55"]
section = "66"
touches = ["crate::pipe::table::TableError"]
write-set = ["+crates/scribe2/src/pipe/table/teeth.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review/outside.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail done_teeth_table_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_table_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_table_evidence_is_decided_once_for_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_check_names_the_incomplete_write_set_and_the_missing_section_with_file_line", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/table/teeth.rs:180", "crates/scribe2/src/pipe/table.rs:22", "crates/scribe2/src/pipe/table/parse.rs:3", "crates/scribe2/src/pipe/table/check.rs:12", "crates/scribe2/src/pipe/closure/derive.rs:30", "crates/scribe2/src/pipe/closure.rs:1", "crates/scribe2/src/pipe/review.rs:1", "crates/scribe2/src/pipe/cli/intake.rs:10", "crates/scribe2/src/pipe/cli/intake/refusal.rs:1", "crates/scribe2/src/pipe/refuse.rs:1", "crates/scribe2/src/pipe/review/outside.rs:1", "crates/scribe2/src/pipe/contract.rs:1", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs:170", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:90"]
depends = ["bu", "bv"]
done = "(1) 形（§66 形 2 (a)）: 欄を持つ行の要素ごとに、最初の : の前が ASCII の数字の番号で、後ろが 4 形（名・= と名・@ と番号・! と仕組みの名）のどれか 1 つであることを照らし、空か空白か , を含む要素・4 形の外の要素・同じ要素の重なりを名指す (2) 覆い（(b)）: 番号の集合が done の番号つき項目（審査の材料と同じ done_items の 1 本で数える）の 1〜K とちょうど等しいことを照らし、無い番号と余る番号を名指す。K が 0 の行が欄を持つことも名指す (3) 検証行の番号（(c)）: @ の番号が 1〜行の検証行の本数に在ることを照らす (4) 選ばれ（(d)）: 名の歯と既存の歯が、行の nextest の形の検証行のどれか 1 本の filter 語に、その行の一致の型（部分一致か --exact）で当たることを照らし、当たらない名を撃たれない歯として名指す（--exact の行で filter 語を名の部分に持つだけの名は選ばれない） (5) 仕組み（(f)）: ! の後ろが 4 語（write-set・place-only・new-file・closure）のどれかで、place-only は = の項目・new-file は + か ~ の項目か creates・closure は契約表の検査（contracts check）を撃つ検証行を、行が 1 つ以上持つことを照らす (6) 在りか（(e)）: 受付と preflight の判定（base の本文は受付の材料の .rs・閉包の導出と同じ読み）だけが、既存の歯がそれを選ぶ検証行の crate と scope の base の歯の区間の #[test] の直下の fn の名にちょうど 1 つ在ること（0 は無い歯・2 以上は 2 か所の名）と、base に在る名の歯が 1 か所に定まることを照らす。base を持たない表の検査（contracts check）は (e) を照らさない (7) 外れは表の検査の findings の新しい 1 種 done-teeth（TableError の宣言順の末尾の 18 値目・証拠の在り処は Name）で、行番号と要素と理由を持ち、1 行の外れを全件名指す。contracts check は rc 1、受付は同じ判定で contract-table の断り（rc 1）にして run を作らず、preflight は同じ断りを名指す (8) AC77 の base を持たない側: done が 3 項目の行 7 つ（適合 1・4 形の外の要素・項目 2 の要素の欠け・本数を越える @ の番号・どの検証行にも選ばれない名・base に無い既存の歯・= の項目を持たない !place-only）を持つ toy repo で、contracts check は base に無い既存の歯の行を除く 5 行を名指して rc 1 で、適合の行だけの fixture は findings 0・rc 0。受付は 6 行をそれぞれ done-teeth の理由で断って run を作らず、適合の行は受ける (9) AC26: 既存の歯 contract_check_names_the_incomplete_write_set_and_the_missing_section_with_file_line の fixture に、欄が項目 2 を覆わない行を足し、違反 3 行を file と行で名指して非 0 (10) 欄を持たない行の読みと判定は今のまま（現物の契約表は findings 0） (11) 足した子 module と歯は、ほかの行の閉包と歯の置き場を広げない。子 module は TableError・ContractRow・Refuse・ClosureError を literal・match の arm・variant の構築の形で名指さず（外れを自分の型 Miss で返し、table/check.rs と受付が done-teeth の variant に包む）、歯の名は設計 doc の検証行の filter 語（contract_ など）を名の部分に持たない 歯: lib の done_teeth_table_（table/check.rs の既存の mod tests・条件 1 つに歯 1 本で (1)(2)(3)(4)(5) を 1 本ずつと、(6) の 1 本: base に 0 か所と 2 か所の既存の歯・2 か所の名の歯の 3 形を、base を渡す口は名指し、渡さない口は名指さない）、e2e の done_teeth_table_（contracts.rs の 1 本が (7)(8) の contracts check の側・intake.rs の 1 本が (7)(8) の受付と preflight の側）、書き換える既存の歯 3 本（table.rs の宣言順の pin を 18 値に・証拠の在り処の歯を数に依らない名 pipe_table_evidence_is_decided_once_for_every_variant に改めて 18 値に・contracts.rs の AC26 の歯を 4 行の fixture に）、(10) は verify の 6 行目の既存の歯（本文を変えない）、(11) は verify の 7 行目の契約表の検査。書き換える 3 本は、flip-check が base の src 区間に HEAD の歯の区間を重ねて撃つので base の型に無い variant で compile が落ちるか、base が欄を読んで捨てて 3 行目を名指さずに RED になり、retroactive の札は要らない。base は欄を読んで捨てるので新しい歯も RED"

[[contract]]
id = "bx"
title = "受付の生成が行の欄 done-teeth を契約 file の任意 key done-teeth へ写し（targets と同じく Contract の field にせず key の読み手 1 本で読む）、preflight が欄の有無を 1 語で出し、lens の state は key を持つ契約だけ done の次の行に done-teeth: の行を足す（§66 行 (1) の写し）"
req = ["FR53", "FR106", "FR49"]
section = "66"
write-set = ["crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail done_teeth_copy_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_copy_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_preflight_ok_reports_facts_and_matches_intake", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
growth = ["crates/scribe2/src/pipe/contract.rs:22", "crates/scribe2/src/pipe/cli/preflight.rs:8", "crates/scribe2/src/headless/lens.rs:16", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:90"]
depends = ["bw"]
done = "(1) 契約 file の生成（contract.rs の render）は、欄 done-teeth を持つ行だけ任意 key done-teeth を行の要素の順で 1 行書き、欄の無い行の契約 file には key を書かない（字は今のままで、着地の列の settled の鍵も変わらない） (2) key の読み手を contract.rs に 1 本足す（targets の読み手と同じ形・path か本文から要素の列を返し、key の無い file は空の列）。Contract に field を足さない (3) preflight は design= の行の次に、欄を持つ行で done-teeth=present、持たない行で done-teeth=absent の 1 行を出し、ほかの事実の行と受付の stdout は変えない (4) lens の state は、契約 file が key を持つ周だけ done の行の次に done-teeth: と要素の列の 1 行を足し、key の無い契約の state の字は 1 字も変えない (5) 足した歯の名は、ほかの行の歯の置き場を広げない（設計 doc の検証行の filter 語 contract_ などを名の部分に持たない） 歯: lib の done_teeth_copy_（contract.rs の既存の mod tests に (1)(2) の 1 本: 欄を持つ行と持たない行の render と読み手の往復・headless/lens.rs の既存の mod tests に (4) の 1 本: key の有無 2 通りの state の字）、e2e の done_teeth_copy_（intake.rs の 1 本が (1)(3): 欄を持つ行と持たない行を受付と preflight に通し、run dir の契約 file の key と preflight の 1 語と、受付の stdout に done-teeth の字が無いこと）。(3) の「ほかの事実の行は変えない」は verify の 3 行目の既存の歯（本文を変えない）、(5) は verify の 4 行目の契約表の検査が測る。base は key を書かず 1 語も行も出さないので新しい歯は RED"

[[contract]]
id = "by"
title = "契約表の検査に --base <sha> を足し、vessel 宣言が teeth-check を true で持つ repo で base から足された行と done の字が変わった行に番号つきの項目と欄 done-teeth を求め、変わった行の既存の歯の base の在りかも照らす — CI の PR の job が PR の base を渡して撃ち、本 repo の宣言に teeth-check = true を書く（§66 行 (2)）"
req = ["FR104", "FR105", "FR55"]
section = "66"
touches = ["crate::pipe::table::TableError"]
write-set = ["+crates/scribe2/src/pipe/table/changed.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review/outside.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/help.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", ".github/workflows/ci.yml", ".vessel.toml", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "=crates/scribe2-boundary/tests/e2e/main.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail done_teeth_base_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_base_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_table_evidence_is_decided_once_for_every_variant", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_table_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo xtask check", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo .", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_pages_match_the_live_form_and_every_subcommand", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_text_is_ascii_and_fits_the_width"]
size = "M"
growth = ["crates/scribe2/src/pipe/table/changed.rs:110", "crates/scribe2/src/pipe/table.rs:20", "crates/scribe2/src/pipe/table/parse.rs:1", "crates/scribe2/src/pipe/table/check.rs:15", "crates/scribe2/src/pipe/cli/intake.rs:2", "crates/scribe2/src/pipe/cli/intake/refusal.rs:1", "crates/scribe2/src/pipe/refuse.rs:1", "crates/scribe2/src/pipe/review/outside.rs:1", "crates/scribe2/src/pipe/cli.rs:8", "crates/scribe2/src/help.rs:3", "crates/scribe2/src/pipe/declaration.rs:4", "crates/scribe2/src/pipe/declaration/optional_keys.rs:6", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs:260"]
depends = ["bw"]
done = "(1) contracts check が --base <sha> を受け、base の commit を読めない周は理由の 1 行で rc 2。--base の無い周の findings と判定行は今のまま (2) 変わった行は、base の同じ doc に同じ id の行が無い行（足された行）か、在って done の字（表の読みの後の値）が base と違う行で、done 以外の欄と設計の節の散文だけを変えた行は数えず、行の字が同じで code だけが変わった commit の行も数えない（生成した契約 file の sha を比べない） (3) 宣言が teeth-check を true で持つ repo で --base を渡した周だけ、変わった行のうち done を持つ行が番号つきの項目を 1 つも持たなければ done-unnumbered、欄 done-teeth を持たなければ done-teeth-missing（TableError の宣言順の末尾の 19・20 値目・証拠の在り処は Row）で、file と行と理由で名指して rc 1。teeth-check が false の宣言・key の無い宣言・--base の無い周はこの 2 語を出さない (4) --base を渡した周は、変わった行のうち欄を持つ行に行 bw の在りかの照らし（(e)・口 done_teeth_located）を base の本文で撃ち、変わらない行（base に無い既存の歯を名指す欄を持っていても）と --base の無い周には撃たない (5) AC77 の base を渡す側: 行 bw の 7 行の fixture に base を渡すと 6 行を名指して rc 1 で、teeth-check を true にした宣言では base に無い行と done の字を変えた行のうち欄を持たない行と番号の無い行が名指され、変わらない行・write-set だけを変えた行・設計の節の散文だけを変えた行は名指されない (6) CI の flip-check の job（PR のときだけ）に、PR の base の sha を --base に渡して契約表の検査を撃つ step を 1 本足す。step は block scalar（run: |）で書き、CLAUDE.md の done の区間は変わらない (7) 本 repo の .vessel.toml に teeth-check = true を書く (8) usage と help の form に --base を足し、help の頁は生きた form と一致したまま幅に収まる (9) 足した子 module（変わった行の読みと要否）と歯は、ほかの行の閉包と歯の置き場を広げない。子 module は TableError・ContractRow・Refuse・ClosureError を literal・match の arm・variant の構築の形で名指さず（要否の外れを自分の型で返し、table/check.rs が 2 語の variant に包む）、歯の名は設計 doc の検証行の filter 語（contract_ など）を名の部分に持たない 歯: lib の done_teeth_base_（table/check.rs の既存の mod tests・(2) の変わった行の読みに 1 本・(3) の要否に 1 本）、e2e の done_teeth_base_（contracts.rs・(1) の読めない base と usage の --base に 1 本・(4)(5) の AC77 の base の側に 1 本・(3)(5) の teeth-check の 3 通り〔true・false・key 無し〕と --base の有無に 1 本・(6) の ci.yml の flip-check の job の step を読む 1 本・(7) の本 repo の宣言を宣言の読み手で読む 1 本）、書き換える既存の歯 2 本（table.rs の宣言順の pin と証拠の在り処の歯を 20 値に）。(1) の「--base の無い周は今のまま」は verify の 5 行目の行 bw の歯（本文を変えない）、(6) の CLAUDE.md の不変は verify の 7 行目の xtask check、(8) の help は verify の 9・10 行目の既存の歯（e2e の main.rs・本文を変えないので write-set に = で置く）、(9) は verify の 8 行目が測る。書き換える 2 本は base の型に無い variant で compile が落ちて RED になり、retroactive の札は要らない。base は --base を知らず usage で断るので新しい e2e の歯も RED"

[[contract]]
id = "bz"
title = "gate の write-set を照らす段が契約 file の key done-teeth の名の歯と既存の歯を便の HEAD の木で照らし（書かれていない歯・動いていない歯・消えた既存の歯を名指して落とす）、審査の材料 items.txt が項目ごとに宣言の歯を添えて lens の表の歯を宣言の歯の内に閉じる（§66 行 (3)）"
req = ["FR106", "FR8", "FR49"]
section = "66"
write-set = ["crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/review/items.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_gate_", "cargo nextest run -p scribe2 --lib --no-tests=fail done_teeth_review_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_review_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail done_teeth_place_only_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_write_set_prefixed_items_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/gate/verify.rs:70", "crates/scribe2/src/pipe/review.rs:12", "crates/scribe2/src/pipe/review/items.rs:40", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs:260", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:110"]
depends = ["bx", "bv"]
done = "(1) 契約 file が key done-teeth を持つ便だけ、gate の write-set を照らす段が名の歯（= も @ も ! も前に置かない歯）を、それを選ぶ検証行の crate と scope の便の HEAD の歯の区間で、行 bw の在りかの読み手（tooth_sites）で測る: HEAD に無い歯を「書かれていない歯」、2 か所以上に在る歯を「2 か所の名」、本文（fn の頭から次の #[test] の行まで）が base と同じ歯を「動いていない歯」の見出しの下に名指して段の rc 1（FAIL・段と理由の型と verdict は足さない）。base に無い歯と本文が base と違う歯は通す (2) 既存の歯（= と名）は、便の HEAD の同じ範囲にちょうど 1 つ在ることを測り、消えた歯と 2 か所に増えた歯を名指して段の rc 1 (3) @ と番号の歯と ! と仕組みの名の歯は、この段で測らない（@ は検証行の実走・仕組みは欄の有無に依らない測りが持つ） (4) key の無い契約の便は、名の歯を書かなくてもこの測りで落ちず、段の rc と stderr は今のまま (5) 審査の材料 items.txt は、契約 file が key を持つ周だけ、項目の行ごとに「(n) <本文> ／ 歯: <その番号の要素の歯を , で並べた字>」と宣言の歯を添え、表の指示に「<歯> はその項目の宣言の歯のうち、約束を外した実装で落ちる 1 本・無ければ -」と「! の仕組みの歯は構造の制約の項目にだけ当たり、挙動の約束に仕組みの歯しか無ければ -」を足す。key の無い契約の材料は 1 字も変えない (6) lens の表の歯が、その番号の宣言の歯（= の有無は問わない）の外の項目は、形の合わない項目に数えて判定を INCONCLUSIVE に倒す（§64 形 4 と同じ倒し）。key の無い契約の判定は §64 のまま (7) 足した歯の名は、ほかの行の歯の置き場を広げない（設計 doc の検証行の filter 語 contract_ などを名の部分に持たない） 歯: e2e の done_teeth_gate_（gate.rs・条件 1 つに歯 1 本: (1) の書かれていない歯・動いていない歯・2 か所の名の 3 本、(2) の消えた既存の歯の 1 本、全部を書いた便が PASS で @ と ! の要素を持っても段 ① が rc 0 の 1 本〔(1)(2)(3)〕、key の無い契約の便が名の歯を書かずに PASS の 1 本〔(4)〕）、lib の done_teeth_review_（review.rs の既存の mod tests・(5) の key の有無 2 通りの材料の字に 1 本・(6) の宣言の外の歯を形の合わない項目に数える表の読みに 1 本）、e2e の done_teeth_review_（review.rs・偽の lens が宣言の外の歯を返した便が INCONCLUSIVE の 1 本）。(4) の段の今のままは verify の 4〜5 行目の既存の歯（本文を変えない）、(7) は verify の 6 行目が測る。base は段 ① が key を読まず材料が歯を添えないので新しい歯は RED"

[[contract]]
id = "ca"
title = "焼き直しの門の teeth-outside-write-set の物差しは at の path の形の項目から # の後ろと末尾の行の番号を剥がした file の字面で write-set と照らし、断りの理由は剥がした file を重複なしで名指す（§68・§35 の物差しの項目の字面の側）"
req = ["FR49", "NFR4"]
section = "68"
write-set = ["crates/scribe2/src/pipe/review/judgement.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_repeat_"]
size = "S"
growth = ["crates/scribe2/src/pipe/review/judgement.rs:20", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:75"]
done = "(1) at が # の後ろを持つ path の項目（src/other.rs の後ろに # と歯の名）の teeth-outside-write-set の後、write-set に src/other.rs を足した契約が受付を通る (2) at が末尾に : と行の番号を持つ path の項目（src/other.rs の後ろに :12）の後、write-set に src/other.rs を足した契約が受付を通る (3) 同じ file に # の後ろの違う 2 項目と § の番号の後、file を足さない契約は finding-unaddressed で断られ、理由は剥がした file の字面を名指して # の後ろの字を持たず、数は「測った 1 件・測れない 1 件」 (4) 既存の pipe_intake_repeat_ の歯が本文を変えずに緑"

[[contract]]
id = "cb"
title = "vessel 宣言の任意 key contract-tables で契約表の置き場を名乗り、rev の宣言を読む 3 値の口と列挙の 1 関数を置いて contracts check・宣言済みの新規 file・runner の touches 節を通す（§69 行 cb）"
req = ["FR47", "FR55", "FR53"]
section = "69"
write-set = ["crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_table_places_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_places_docs_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_places_declared_files_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contracts_tables_key_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail spawn_touches_from_declared_tables_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contracts_untracked_doc_is_noticed_without_counting_and_joins_the_population_once_tracked", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_touches_section_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/declaration.rs:8", "crates/scribe2/src/pipe/declaration/optional_keys.rs:130", "crates/scribe2/src/pipe/table/check.rs:20", "crates/scribe2/src/pipe/table.rs:70", "crates/scribe2/src/pipe/spawn.rs:5", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs:90", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs:50"]
done = "(1) vessel 宣言が任意 key contract-tables（key の列の末尾・schema は 1 のまま）を repo 相対の項目の配列として読み、空（空白だけ）・絶対 path・home の短縮記号・.. の段の項目を key の行番号を名指す不備で断り、書かない宣言は項目 0 で、key の列の pin は末尾に contract-tables を 1 つ足す〔declaration_table_places_ の (a): key の無い宣言・2 項目・不備 4 形・空配列と文字列の値は既存の字面・直す既存の歯 declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema（retroactive の札）〕 (2) optional_keys.rs の型 TablePlaces（pub・variant Fixed / Declared（欄 items と line）/ Unreadable・declaration が再輸出）の関連 fn at が名指した rev の宣言を読み、宣言 file の無い rev・git でない dir・key の無い宣言は Fixed、key を持つ宣言は Declared、宣言が在って読めない rev は Unreadable を返し、関連 fn items は Fixed で空の列・Declared で項目・Unreadable で無しを返す〔declaration_table_places_ の (b): 使い捨ての git repo で key を持つ commit・key の値を壊した commit・key を消した commit と、HEAD でなく名指した古い sha の Declared〕 (3) design_docs が path の列と項目の列を受け、入力の順のまま既定（docs/design/ の直下の .md）と項目（末尾 / の dir は直下で form_of が読める path・ほかは等しい path）に当たる path を 1 度ずつ返し、項目 0 では今と同じ列を返す〔table_places_docs_ の (c): 項目 0・contracts/・tables/one.toml・docs/design/ の 4 つの表で、下の dir の path・.txt・tables/one.toml.bak を返さず a.md を 2 度返さない〕 (4) contracts check の doc の列（行の検査と名の衝突の予想が同じ列を読む）と未追跡の知らせが HEAD の宣言の項目で同じ 1 関数を通り、判定行の字面は変えない〔contracts_tables_key_ の (e): key で contracts/ を名乗り docs/design/toy.md が区間を持たない toy の findings が contracts/t.toml の goal の無い行 a の contract-table:section-missing の 1 件だけで判定行が docs=2 rows=1・rc 1、key を消した同じ repo は docs=1 rows=0・rc 0 (f): 未追跡の contracts/u.toml が path を名乗る contracts untracked-doc: の知らせと untracked=1 を出す・既存の歯 contracts_untracked_doc_is_noticed_without_counting_and_joins_the_population_once_tracked は本文を変えずに緑〕 (5) declared_files が引数を変えずに HEAD の宣言の項目で同じ列を読み、宣言を読めない周は理由を返す〔table_places_declared_files_ の (d): key で contracts/ を名乗る commit で contracts/t.toml の行の + の file を返し、key の無い commit で返さず、key の値を壊した commit で理由を返す〕 (6) runner の「ほかの行の touches」節が便の base の宣言の項目で同じ 1 関数を通り、読めない周は理由の 1 行〔spawn_touches_from_declared_tables_ の (g): key で contracts/ を名乗る toy の contracts/t.toml の行 b の touches の項目が節に contracts/t.toml#b の pointer で載る・既存の歯 runner_touches_section_ は本文を変えずに緑〕 (7) 器の repo の contracts check は findings 0 のまま〔既存の歯 contract_closure_ext_real_table_has_zero_findings と verify の最終行の contracts check が便の木で findings 0〕 歯: declaration_table_places_（lib・2 本）・table_places_docs_（lib・1 本）・table_places_declared_files_（lib・1 本）・contracts_tables_key_（e2e・2 本）・spawn_touches_from_declared_tables_（e2e・1 本）が base で RED（機能不在: base は名が無く lib は 0 本・e2e は base が key を未知の key で断り rc 2）。接頭辞の後ろの名に contract_ と pipe_intake_ を含めない"

[[contract]]
id = "cc"
title = "局面の行の読み手 read_rows が main の sha の vessel 宣言の契約表の置き場を読み、sha の木の全 path を列挙の 1 関数で絞る — 宣言を読めない sha は Missing（§69 行 cc）"
req = ["FR90"]
section = "69"
write-set = ["crates/scribe2/src/fleet/lifecycle_mark.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail lifecycle_declared_tables_", "cargo nextest run -p scribe2 --lib --no-tests=fail lifecycle_mark_"]
size = "S"
growth = ["crates/scribe2/src/fleet/lifecycle_mark.rs:60"]
depends = ["cb"]
done = "(1) read_rows が引数を変えずに main の sha の宣言を TablePlaces の at で読み、sha の木の全 path（ls-tree -r）を design_docs で絞り、行の pointer は <置き場の path>#<行 id> のまま〔lifecycle_declared_tables_ の (a): key で contracts/ を名乗り表を contracts/x.toml だけに置いた commit A の read_rows が contracts/x.toml#a と行の write-set を返す（docs/design/ に file が無いことを歯の中で assert） (b): key を消した commit B は Missing で、同じ repo で commit A の sha を名指すと (a) と同じ行を返す (d): key の無い宣言で docs/design/y.md に表を置いた commit D は y.md の行を返す〕 (2) 宣言を読めない sha は表が在っても Missing（既定の置き場に倒さない）〔(c): key の値を壊した commit C は contracts/x.toml が在っても Missing〕 歯: lifecycle_declared_tables_（lib・1 本・(a)〜(d)）が base で RED（機能不在: base は名が無く 0 本・本文を当てても base の read_rows は docs/design/ だけを見て (a) が Missing）。既存の lifecycle_mark_ の歯は本文を変えずに緑"

[[contract]]
id = "cd"
title = "live-row の門（編集と commit）が worktree の root の HEAD の vessel 宣言の置き場を読み、列挙の 1 関数の列だけを比べる — docs/design/ の下を深さを問わず読む定義を捨てる（§69 行 cd）"
req = ["FR47", "FR53", "FR32"]
section = "69"
write-set = ["crates/scribe2/src/hook/live_row.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_live_tables_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_live_row_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_live_row_"]
size = "M"
growth = ["crates/scribe2/src/hook/live_row.rs:30", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:140"]
depends = ["cb"]
done = "(1) 編集の門は、対象が form_of の読める path で同じ置き場の worktree の root を解けた周に、root の HEAD の宣言を TablePlaces の at で読み、repo 相対 path が design_docs の列に入る周だけ比べ（.md で区間の始まりの行を変更前にも変更後にも持たない編集は宣言を読まずに通す）、宣言を読めない周は form_of の読める path を全部比べる〔hook_live_tables_ の (a): 宣言（必須 3 key と key contracts/）と contracts/t.toml（行 a / b / c）を commit した toy で、Questioned の便の行 contracts/t.toml#a の done を変える Edit が rc 2・stdout 0 byte・記録 live-row-deny changed (c): 置き場の外の other/u.toml（同じ表の写し）を変える Edit は通る (d): 便を置いた後に key の値を壊す commit を置き (a) と同じ Edit が changed で断られる〕 (2) commit の門は HEAD との差の path を同じ口と同じ 1 関数で絞り、宣言を読めない周は form_of の読める path の全部を比べる〔(b): (a) と同じ変更の git commit が changed で断られる〕 (3) 門は design_docs の列のほかを見ない（docs/design/ の下を深さを問わず読む定義を捨てる）〔(e): key の無い toy で docs/design/sub/t.toml の行に便を置くとその行を変える Edit は通り、同じ toy に key で docs/design/sub/ を名乗る commit を足すと同じ Edit が断られる〕 (4) 比べ・自分の行の除外・deny の行・記録の語・解けない dir の扱いは変えない〔既存の hook_live_row_ の歯（e2e と lib）は本文を変えずに緑〕 歯: hook_live_tables_（e2e・(a)〜(e)）が base で RED（機能不在: base の門は docs/design/ の外を見ず (a)(b)(d) を通し、docs/design/ の下を深さを問わず読んで (e) の 1 つ目の Edit を断る）"

[[contract]]
id = "ce"
title = "台帳の形の検査の docs_of が HEAD の vessel 宣言の契約表の置き場を読み、tracked を列挙の 1 関数で絞る — 読めない周は unreadable reason=declaration-unreadable（§69 行 ce）"
req = ["FR51"]
section = "69"
write-set = ["crates/scribe2/src/ledger/form.rs", "=crates/scribe2-boundary/tests/e2e/ledger_form.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail ledger_shape_tables_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail ledger_form_names_each_defect_with_its_count_and_population"]
size = "S"
growth = ["crates/scribe2/src/ledger/form.rs:50"]
depends = ["cb"]
done = "(1) docs_of が HEAD の宣言を TablePlaces の at で読み、tracked を design_docs で絞り（is_design_doc を消す）、宣言した置き場の .toml の行も未着地と drift に数える〔ledger_shape_tables_ の (a): 使い捨ての git repo（台帳は空の列）で、key で contracts/ を名乗り contracts/t.toml に未着地の行 a（+ の file が tracked に無い）を置いた commit の docs_of を判定に掛けた描画が unlanded=1 と drift=1:t#a を持つ (b): key を消した commit の描画は unlanded=0〕 (2) 宣言を読めない周は理由の語 declaration-unreadable を返し件数を 1 つも出さない〔(c): key の値を壊した commit の docs_of が declaration-unreadable を返し、その語の描画が ledger-form: unreadable reason=declaration-unreadable の 1 行〕 (3) drift の契約 id の形（stem と行 id）と既存の描画は変えない〔既存の e2e の歯 ledger_form_names_each_defect_with_its_count_and_population は本文を変えずに緑〕 歯: ledger_shape_tables_（lib・1 本・(a)〜(c)）が base で RED（機能不在: base は名が無く 0 本・本文を当てても base の docs_of は docs/design/ だけを見て宣言を読まず (a) が unlanded=0）"

[[contract]]
id = "cf"
title = "contracts check が vessel 宣言の置き場を照らし、tracked に当たらない項目を place-empty・doc の file 名の stem の重なりを doc-id-duplicate で名指す（TableError に 2 つ・§69 行 cf）"
req = ["FR55", "FR47"]
section = "69"
touches = ["crate::pipe::table::TableError"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "=crates/scribe2/src/pipe/table/parse.rs", "=crates/scribe2/src/pipe/cli/intake.rs", "=crates/scribe2/src/pipe/cli/intake/refusal.rs", "=crates/scribe2/src/pipe/refuse.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contracts_place_defect_", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_table_evidence_is_decided_once_for_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_closure_ext_real_table_has_zero_findings", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/table.rs:60", "crates/scribe2/src/pipe/table/check.rs:4", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs:80"]
depends = ["cb"]
done = "(1) contracts check が宣言した項目のうち tracked の path に 1 つも当たらない項目（dir 項目は直下に form_of が読める tracked file が 0 本・file 項目は tracked に無い）を .vessel.toml の key の行の contract-table:place-empty で 1 件ずつ名指して rc 1 にし、既定の置き場の 0 本は名指さない〔contracts_place_defect_ の (a): key に contracts/ と tables/none.toml を書き contracts/a.toml に表を置いた toy は tables/none.toml の 1 件だけを名指す (c): key の無い toy は 2 つの語を出さない〕 (2) 列挙した doc の file 名の stem が重なれば、列挙の順で 2 本目以降の doc を行 0 の contract-table:doc-id-duplicate で名指し相手の path を添えて rc 1〔(b): key で contracts/ を名乗り contracts/toy.toml と docs/design/toy.md の両方に表を置いた toy は docs/design/toy.md の 1 件で相手 contracts/toy.toml を名乗る〕 (3) 2 つの語は TableError の variant 2 つ（宣言順の末尾・理由の 1 行・rc 1・在り処は place-empty が項目の path・doc-id-duplicate が 2 本の doc の path の file の列）で、名の列の pin と在り処の pin の母集団を 2 つ伸ばし、受付の行の検査はこの 2 つを撃たない〔直す既存の歯 table_error_names_are_pinned_in_declaration_order_and_carry_their_line と pipe_table_evidence_is_decided_once_for_ で始まる 1 本（retroactive の札）〕 (4) 器の repo の contracts check は findings 0 のまま〔既存の歯 contract_closure_ext_real_table_has_zero_findings と verify の最終行の contracts check が便の木で findings 0〕 歯: contracts_place_defect_（e2e・(a)〜(c)）が base で RED（機能不在: base は key を未知の key で断り rc 2・語が無い）。接頭辞の後ろの名に contract_ を含めない"

[[contract]]
id = "cg"
title = "受付の生成が pointer の path を受付の材料の vessel 宣言の置き場に照らし、外を指す契約を doc を読む前に place-outside で断る（TableError に 1 つ・§69 行 cg・FR54）"
req = ["FR54", "FR53"]
section = "69"
touches = ["crate::pipe::table::TableError"]
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/table.rs", "=crates/scribe2/src/pipe/table/check.rs", "=crates/scribe2/src/pipe/table/parse.rs", "=crates/scribe2/src/pipe/cli/intake/refusal.rs", "=crates/scribe2/src/pipe/refuse.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail intake_place_outside_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_contract_whole_goal_reads_the_goal_from_a_derived_toml", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_a_cap_shortfall_recounts_without_the_unresolved_items", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_table_evidence_is_decided_once_for_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/cli/intake.rs:14", "crates/scribe2/src/pipe/table.rs:25", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs:90", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:2"]
depends = ["cb"]
done = "(1) 受付の生成は pointer の path が受付の材料の宣言の置き場（design_docs に path 1 つと項目を掛けて空でないか）に入らない契約を、doc を読む前に contract-table:place-outside（path を名乗る理由の 1 行・rc 1）で断って run dir と event を作らず、宣言を読めない材料も同じ断りにし、生成を呼ぶ preflight も同じ断りを並べる〔intake_place_outside_ の (a): key の無い toy に contracts/t.toml（goal を持つ行 a・ほかの欄は toy の行と同じ形）を commit し、--design contracts/t.toml#a の受付が rc 1・stderr が contract-table:place-outside と contracts/t.toml を名乗り・run dir と event は撃つ前と同数、同じ pointer の preflight が refuse=contract-table:place-outside の 1 行と rc 1〕 (2) 受付の材料が HEAD の宣言の置き場を材料の読みの 1 本の中で 1 周に 1 回読み、key で名乗った置き場の pointer は通す〔(b): (a) の toy に key で contracts/ を名乗る commit を足すと同じ受付が rc 0 で run を作る・材料の字面を持つ既存の helper short_of_room に欄を 1 つ足し（retroactive の札）それを使う contract_closure_ext_survivor_a_cap_shortfall_recounts_without_the_unresolved_items が緑〕 (3) 語は TableError の variant 1 つ（宣言順の末尾・在り処は宣言 file の名 1 本の file の列）で、名の列の pin と在り処の pin の母集団を 1 つ伸ばす〔直す既存の歯 table_error_names_are_pinned_in_declaration_order_and_carry_their_line と pipe_table_evidence_is_decided_once_for_ で始まる 1 本（retroactive の札）〕 (4) 既定の置き場の pointer は今どおり通り、置き場の外を指していた既存の歯は toy の宣言に key で docs/design/ を名乗る 1 行を足す形に直す〔(c): (a) の toy の docs/design/toy.md の行の受付が rc 0・直す既存の歯 pipe_review_contract_whole_goal_reads_the_goal_from_a_derived_toml（derive_repo_with の files に宣言の上書きを 1 つ足し、ほかの assert は変えない・retroactive の札）〕 (5) 器の repo の contracts check は findings 0 のまま〔verify の最終行の contracts check が便の木で findings 0〕 歯: intake_place_outside_（e2e・(a)〜(c)）が base で RED（機能不在: base の受付は置き場を照らさず (a) の受付と preflight を通す）。接頭辞の後ろの名に pipe_intake_ と contract_ を含めない"
<!-- contracts:end -->


## 28. 歯の置き場が verify 行の scope（-p / --test <name> / --lib）を読む — その行が撃てない file を write-set に要求しない（契約表の行 ab・`s2-07l.451`）

- 何が起きているか: 歯の置き場の読み手（§3 (ii) の `teeth_places` と、§20 が同じ 1 関数で通す Declared 行の門 `declared_teeth`）は、verify の nextest 行から crate（`-p` の値）と filter 語だけを取り、`nextest_filter` が scope の旗（`--test <name>` / `--lib`）を落とす。置き場はその crate の **全 file** から「`#[test]` の直下の `fn` の名が filter 語を含む file」を集めるので、`--test e2e` の行（統合 test の target だけを撃つ行）でも `src` の in-file の歯の file を write-set に要求して断る＝**その行が実際には走らせない file** を書く権利ごと要求している。実測（2026-09-17・母集団 = 設計 doc の契約表の nextest 行 220 本）: `s2-07l.447` run 1 が `seat_account_` で src の歯 4 本に当たって断られ（接頭辞を 2 本に割って回避）・`s2-07l.340` が `confine_reasons_` で同型・純移動の `s2-07l.351` は 12 の接頭辞のうち 6 本が src の歯の file を要求し、**歯の名を変えられない純移動では回避できず run が 1 本も起きない**。
- 形: nextest 行の読み手に scope を足す。scope は **閉じた 3 値の enum**（宣言順 = 旗なし / `--lib` / `--test <name>`）で、行の語から 1 関数で解き、置き場の母集団を `in_crate` の後段で 1 述語に畳む: 旗なし = その crate の全 file（従来どおり）／`--lib` = `crates/<crate>/src/` 配下／`--test <name>` = `crates/<crate>/tests/<name>.rs` とその配下。**読めない旗（`--bin` / `--benches` / `-E` ほか）と、scope の旗が 2 つ以上在る行は旗なしと同じ広い側へ倒す**（fail-closed・緩める側は狭く取るの対）。読み手は 1 本のまま（Derived の導出 (ii) と Declared の門 §20 は同じ関数を通る）で、断りの型も字面も増やさない（`TeethPlaceUnresolved` / TeethOutsideWriteSet のまま）。
- 触らない: filter 語の読み（`-` で始まらない最後の語）・`test_fns` と `test_region` の弁別・`tests` 欄の扱いと `teeth_file`・Declared / Derived の弁別・`check_teeth_cover` の照合と正規化・契約表の schema（欄を足さない＝行の verify から読む）・nextest 形でない verify 行を読み飛ばす規則・受付の判定行の token。
- 歯（`closure_scope_` 接頭辞・`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・fixture は同 module の `source(` の型）: (a) `--test e2e` の行が `tests/e2e/` の歯の file だけを置き場に返し、同じ filter 語に当たる src の in-file の歯の file を返さない（base では返す → RED）／(b) `--lib` の行が `src/` の歯の file だけを返す／(c) 旗なしの行・読めない旗を持つ行・旗が 2 つ在る行は crate 全体を返す（広い側のまま）／(d) scope が返す file が 0 本で `tests` 欄も無い行は従来どおり `TeethPlaceUnresolved`（字面不変）。
- 限界（残す側）: scope は**その行が走らせる target**までしか写さず、target の中の module の木は読まない（`--lib` は `src/main.rs` と `src/bin/` の歯も数える＝真の lib target より広い側。現物の契約表 220 行のうちこの差に当たる行は 0 本・実測）。`--test <name>` の `<name>` は `tests/<name>.rs` と `tests/<name>/` の字面で解き、`Cargo.toml` の `[[test]]` の `path` は読まない（本 repo は宣言を持たない）。断りの字面は不変ゆえ、scope の外に歯が在って 0 本になった行の理由は「base に無い」と読める（下界・`tests` 欄で置き場を宣言する側に倒す）。
- 却下案: 純移動の契約に別の verify の形（filter ごとの nextest list の本数が base = head）を持たせる（`s2-07l.351` だけを救い、`s2-07l.447` / `s2-07l.340` の型〔純移動でない便の scope 誤読〕が残る。本数で数える門は歯の本文の改変を通す＝flip-check の `removed_only` が名前の集合を捨てた教訓と同型）／scope を読めない行を断る（今日通っている 183 本の旗なしの行を全部断る）／crate の target を `cargo metadata` で解く（外部の口と実行時の依存を足す・字面走査の下界のままにする）。

## 33. 約束の行 — 契約表の行が `[[promise]]` の子行から touches / surfaces / tests / verify / done を導き、手書きの write-set を持たない（契約表の行 af / ag / ah・`s2-07l.510`）

- **出所**: user の裁定 2026-09-21（逐語は台帳 `s2-07l.510` の notes）。「memo と契約の形は閉じ切る話で、LLM に任せ切らず形をシステミックに作る。beads の機能と合わせて先に設計する」。案 A（約束を行に落とす）を A のデメリット 5 つの提示の後に user が是認した。決定は [ADR-0051](../../design-intent/decisions/ADR-0051-contract-rows-carry-promise-rows-and-ledger-state-is-two-fields.html) §4（台帳の側は [ledger-form.md](./ledger-form.md)）。
- **何が起きているか（母集団・2026-09-21 実測・run dir の残る全便）**: 契約の審査の FAIL / INCONCLUSIVE は 104 件で、内訳は write-set の閉包 51・名指しの不在 17・done と約束の 1:1 の崩れ 11・verify の filter 11・歯の空虚 5・snapshot 1・その他 8。実装席の問い 19 件のうち about:write-set が 16。9/17 以後は設計 doc の PR 157 本に対し着地 63 bead（1 bead の着地に平均 2 便）。落ちる理由の約 87% が**計算できる性質**で、lens（LLM）の判断が要るのは歯の空虚と goal の矛盾の 1 割強しか無い。
- **現物（verified・main f95d113）**: 契約表の行の欄は 16（`FIELDS`・必須 7 / 任意 9）で、tracked 161 行のうち `surfaces` / `creates` / `classes` / `opens` を使う行は **0**、write-set は Declared 151 / Derived 10。§3 の導出（`derive_write_set` = `touches` の閉包 ∪ 歯の置き場 ∪ `surfaces` の外形 pin ∪ `creates` ∪ `also`）は land 済みだが、導出の入力（`touches` / `surfaces` / `tests` / `also`）を設計者が手で埋める形なので、設計者は結局 write-set と同じ閉包を別の欄で手書きしている＝151 行が Declared のまま残り、閉包の穴は審査の lens と実装席の問いで見つかる。約束 ↔ done ↔ 歯 ↔ verify の 1:1 は散文の規則（§20 の型の各 doc に「約束 ↔ done ↔ 歯 ↔ verify」の段落が在る）で、器は測らない。insta の snapshot の名は歯の fn 名から導ける形が 15 本中 12 本、名付き（fn 名と一致しない）が 3 本。
- **やさしく言うと**: 今は「何を約束するか」を散文で書き、その散文から人が write-set と検証の行と done を写している。写し間違いが落ちる理由の 9 割を占める。約束を 1 行 1 約束の表にして、write-set と検証の行と done を器がその表から作れば、写し間違いという工程が無くなる。lens は「約束の意味が合っているか」と「歯が空虚でないか」だけを見る。
- **形（欄と行だけ・散文の免除を持たない）**:
  1. **約束の行 `[[promise]]`** を契約表の区間に足す（top-level の array of tables・`[[contract]]` と同じ parser の形で、dotted header は使わない）。欄は **9**（宣言順）: `of`（親の行 id・必須）/ `n`（1 から連番・必須）/ `text`（約束の 1 文・必須）/ `files`（触る file の列・`+` `-` `~` の接頭辞は §3 / §24 のまま・必須）/ `symbols`（名指す識別子の列・backtick の 3 形〔path / 型の path / fn〕と同じ字面・base に無い新設は `+` を前置・任意）/ `teeth`（歯の**完全名**の列・必須）/ `place`（`teeth` の置き場の file・base に無い名の周だけ必須）/ `fixture`（歯の fixture の形の 1 文・必須）/ `expect`（歯が観測する結果の 1 文・必須）。真偽の欄は持たない（負の枝の有無は `fixture` の文で lens が読む・欄で名乗らせても嘘を測れない）。
  2. **親の行の弁別**: 約束の行を 1 つでも持つ行は **Promised**（`WriteSet` の 3 値目・宣言順の末尾）。`WriteSet` の定義は `crates/scribe2/src/pipe/cli/intake.rs` の閉じた 3 値（Derived / Declared / Promised・行 ag = `s2-07l.512` が 2026-09-21 に着地して base に在る・着地前は 2 値だった）で、網羅 match は同 file の `WRITE_SETS`（宣言順の slice・`enum-slices` の母集団）と `as_str`（判定行の token）の 2 か所だけ（行 ag の write-set の中・他 module に `WriteSet` の match は無い）。約束の行の有無は行 af が land した parse の返す約束の列を `promises_of`（`crates/scribe2/src/pipe/table/parse.rs`）で引いて決める（`read_table` の返す 2 つ目の値・行 ag は約束の行の parse を変えない）。Promised の行は `write-set` / `touches` / `surfaces` / `tests` / `also` / `creates` / `done` を持ってはならず（持てば `Refuse` の新しい 1 値で断る・行の id と欄の名を名指す）、`verify` は持ってもよい（持てば生成値と集合一致・§3 の drift と同じ照合）。`Refuse` の定義と `REFUSALS`（名の slice・宣言順）は `crates/scribe2/src/pipe/refuse.rs` にあり、網羅 match は同 file の `as_str` / `label` / `reason` / `rc` の 4 か所だけ（他 module は値を作るだけで match を持たない）。`REFUSALS` の長さと宣言順を pin する歯は同 file の `refuse_names_are_pinned_in_declaration_order`（新しい 2 値で母集団を足す・行 ag の write-set の中）。**必須の緩み**: `FIELDS`（`crates/scribe2/src/pipe/table.rs`）は `done` と `verify` を `Need::Required` にしているので、そのままでは `done` を持たない Promised の行が intake の `Refuse` に届く前に parse の `TableError`（必須 key が無い）で落ちる。`Need` に 3 値目 **`Conditional`**（約束の行を持たない行では必須・持つ行では任意・意味は doc comment に書く）を足し、`FIELDS` の `done` と `verify` をそれに変える（必須 7 → 5 + 2・`Need` の網羅 match は `table.rs` の schema の描画〔`required` / `optional` の字面に `conditional` を足す〕と `table/parse.rs` の必須検査の 2 か所・`contracts/schema.toml` は生成物ゆえ同じ便で再生成して write-set に入る）。schema の値は xtask の contracts-schema の導出（`crates/xtask/src/check_facts.rs`・variant の名を小文字にした語）と同じ形でなければ drift で落ちるので、variant の名は小文字にしてそのまま値になる 1 語（`Conditional` → `conditional`）にし、xtask は触らない（write-set の外）。parse は `split_promises` の後に `of` で数えて判定する（`contracts check` は同じ 1 実装・C2）。Declared / Derived の行は不変（旧い形として残す・移すのは各行の手番）。
  3. **導出**（Promised の行・`derive_write_set` の入力を約束の行から組む 1 関数・pure）: `touches` ← `symbols` のうち base で閉じた型（`closure()` の 5 形が読む enum / struct / const slice）に解ける名・`creates` ← `files` の `+` の項目・`also` ← `files` の `.rs` でない項目・`tests` ← `place`・`surfaces` ← `teeth` のうち名が `_external_form` で終わる歯の snapshot の名（§16 の外形 pin と同じ読み）に加え、`place` か base の歯の本文で名付きの snapshot（第 1 引数が文字列 literal の形）を持つ歯の名。write-set = 生成した入力で `derive_write_set` を撃った値（**導出の 1 本は増やさない**・§3 の関数に約束の行から組んだ `Fields` を渡す）。
  4. **生成**（契約 file の `render(` の入力・pure）: `verify` ← `teeth` を（crate・scope）で束ね、束ごとに nextest 行 1 本（filter は完全名を空白で並べる・scope は `place` / base の置き場から §28 の 3 値で決める）／`done` ← `n` の順に「(n) `expect`」を空白で繋いだ 1 文。生成値は契約 file にだけ載る（設計 doc に書き戻さない・C10 の逆流なし）。所在: `render(` は `crates/scribe2/src/pipe/contract.rs`（契約 file を組む 1 関数・生成した `verify` / `done` / write-set を row の値の代わりに渡す）、§28 の scope の 3 値は `crates/scribe2/src/pipe/closure/derive.rs` の `Scope`（歯の置き場から scope を読む既存の関数を使い回す・値を増やさない）。契約 file の write-set は runner の guard が読む allowlist そのもの（契約 file の key は pipeline.md §3 の 9 欄・不変）。生成の歯の置き場は `contract.rs`（in-file・接頭辞 `contract_promise_render_`・歯 (c)）、導出の歯の置き場は `closure/derive.rs`（in-file・接頭辞 `contract_promise_derive_`・歯 (b)・fixture は `closure.rs` の `source(`）。
  5. **名指しの実在**: `symbols` の `+` 無しの名は `unresolved_names` と同じ読み手で base に解け、解けなければ受付が断る（`Refuse` の新しい 1 値・`of` と `n` と名を名指す）。`+` 付きは base に**無い**ことを要求（`creates` の `MustBeAbsent` と同じ極性）。着地後の CI は `+` の名が tracked に在れば land 済みと読む（`MayBeLanded`・§3 の `+path` と同じ 2 面）。
  6. **必須の欄の空**は受付と CI の `contracts check` が同じ語彙で断る（`TableError` の新しい 2 値: 親の行が無い `of`・`n` の重複か欠番）。空の必須欄は `TableError` の既存の欄検査の型に乗せる。
- **審査の分担（行 ah）**: 前提（着地済・verified・2026-09-21）: 行 ag（`s2-07l.512`・約束の行の parse / 導出 / render・`WriteSet::Promised`・`Need::Conditional`・`promises_of`）は base に在り、行 ai（`s2-07l.528`・files の既存 .rs と crate:: の型）と行 aj（`s2-07l.529`・§35 の物差し）も着地済み＝行 ah の write-set の外の前提は全部 main に在る。既存の `e2e__headless__lens_contract_prompt_external_form.snap` が行 ah の write-set に在るのは受付が外形 pin の file を要求するためで、本行は 1 byte も変えない（pipeline.md §43 の `polarity.rs` と同じ型・done (1) 後半と (4) は verify の `headless_lens_` の既存の歯がその snapshot と照合して測る）。Promised の行では `FindingKind` のうち TeethOutsideWriteSet / `LiteralMismatch` / `SectionMaterialMissing` は器が受付で測り終えているので、lens の雛形（`lens-contract.txt`）に約束の行の写し（`{promises}` の穴・`n` / `text` / `fixture` / `expect` の 4 欄）を渡し、verdict の kind を `VacuousAssert` / `GoalDoneContradiction` / `Other` の 3 値に限る（Promised でない行は従来の雛形の 6 語のまま・7 語目の `Unparsed` は器が倒す側で雛形には無い）。所在（行 ah の write-set の中・`Review` の欄と `step.rs` は触らない）: (i) 審査段が「行が Promised か」を知る経路は `crates/scribe2/src/pipe/review.rs` の `review(` が `entry.repo` と契約 file の `design` の pointer から設計 doc の契約表を読み（`read_table` → `promises_of`・受付と同じ読み手）約束の列を得る 1 か所——契約 file の 9 欄は不変（pipeline.md §3）で、Promised の印は契約 file に書かない。(ii) `{promises}` の穴を埋めるのは `crates/scribe2/src/headless/lens.rs`（既存の穴と同じ置換の口・約束の列が空なら穴は空文字＝Promised でない行の雛形は 1 字も変わらない）。(iii) kind の絞りは `review.rs` の verdict の読み（`FindingKind::parse` の直後）で、約束の列が非空 ∧ kind が 3 値の外なら INCONCLUSIVE に倒す（fail-closed・`FindingKind` の 7 値と `FINDING_KINDS` は不変）。(iv) §23 の焼き直しの門は Promised の行では「契約 file の sha が変わったか」だけを見る＝`crates/scribe2/src/pipe/cli/intake.rs` の `exclude_unaddressed` が `today.row` の `WriteSet::Promised` の周は `at` の物差し（`review::unaddressed`）を撃たずに通り、`exclude_same_kind`（材料不変の N 回目）だけが残る（`at` の path 照合は器が測った項目に対しては起きない・§35 の物差しは Promised でない行のまま）。
- **触らない**: 契約 file の key（pipeline.md §3 の 9 欄・runner が読む形は不変）・§7（回答は write-set を広げない。約束の行が閉包を先に拾うので about:write-set の問いは下界の外だけになる）・Declared / Derived の行とその門・`derive_write_set` と `check_drift` の照合・rules 行・極性一覧（guard は増えない）・lens の verdict の 3 値。
- **移行**: 着地済みの行は履歴で触らない。未着地の行（列に在る 11 本と以後の新規）を Promised へ移すのは各行の docs PR の手番で、移した周に scope の重複を測る（`teeth` の完全名が base の歯と同名なら「変更する既存の歯」・無ければ新しい歯）。台帳 lint（§6・行 e）に「open な契約のうち Promised でない行の件数と母集団」を 1 項目足すのは後続（§12）。
- **歯**（in-file は `contract_promise_` 接頭辞・e2e は `pipe_intake_promise_` / `headless_lens_promise_` 接頭辞）: (a) 約束の行の parse: 9 欄の宣言順と必須 / 任意・`of` が行に無い・`n` の重複と欠番・空の必須欄が `TableError` の値で名指され、`contracts schema` の生成物に 9 欄が載る（母集団 = 欄の総数を同時に pin）／(b) 導出: `symbols` の閉じた型が `touches` に・`+` の file が `creates` に・`.rs` でない file が `also` に・`_external_form` の歯と名付き snapshot の歯が `surfaces` に写り、write-set が §3 の関数の値と一致する（fixture は同 module の `source(` の型・3 本の名付き snapshot の型を 1 つ含む）／(c) 生成（`contract.rs`・接頭辞 `contract_promise_render_`）: `teeth` が（crate・scope）ごとに 1 本の nextest 行になり完全名が全部載る・`done` が `n` の順で `expect` を並べる／(d) 受付の断り: Promised の行が `write-set` か `done` を持つ・`symbols` の `+` 無しの名が base に無い・`+` 付きの名が base に在る の 3 形が `Refuse` の値で断られ run dir が 0（母集団 = `REFUSALS` の長さを同時に pin）・`verify` を持つ Promised の行は生成値と集合一致なら通り不一致は §3 と同じ drift で断られる／(e) 審査（in-file は `review.rs`・接頭辞 `contract_promise_review_`・e2e は `headless_lens_promise_`）: Promised の行の lens の雛形に約束の 4 欄が載り（外形 snapshot＝**新設の名付き snapshot** `lens_promise_prompt_external_form`・file は行 ah の write-set の `+` の `.snap`・既存の `e2e__headless__lens_contract_prompt_external_form.snap` は約束の行を持たない行の雛形を写すので 1 字も変わらない）、verdict の kind が 3 値の外なら INCONCLUSIVE に倒れる（fail-closed）／(e′) 門（`cli/intake.rs` の `exclude_unaddressed`・e2e は `pipe_intake_promise_rework_gate_reads_only_the_contract_sha`・`tests/e2e/pipe/intake.rs`・既存の `failed_runs` / `Again` の型の設計 doc に約束の行を足す fixture）: `teeth-outside-write-set` の `at` に write-set の外の path を持つ finding の後でも、Promised の行は契約 file の sha が変われば受付を通り、sha が同じ周は `SameKindRepeated` の N 回目でだけ断られる（base は `finding-unaddressed` で断る → RED）／(f) 必須の緩み（`table.rs` / `table/parse.rs`・接頭辞 `contract_promise_need_`）: `done` と `verify` を持たない行は約束の行が 1 つでも在れば parse を通り、約束の行が無ければ `TableError` の必須 key の欠けで名指される・`FIELDS` の `Need::Required` が 5・`Conditional` が 2（母集団 = 欄の総数を同時に pin）・`contracts schema` の生成物に `conditional` が 2 欄で載る（xtask の contracts-schema と drift しない）。
- **限界（残す側）**: `symbols` の解決は §3 と同じ字面走査の下界（別名・generic・glob 越しは見ない）。`fixture` と `expect` の質は欄では測れない（lens の 2 欄が残る理由）。約束の行を持たない旧い行は今の落ち方のまま（移行は行ごと）。
- **却下案**: 案 B = 散文の § を残し閉包の計算だけ足す（Derived が既にそれで、151 行が使っていない＝入力の手書きが残る限り同じ理由で落ちる）／約束を bead の field に置く（契約の正本は設計 doc の行〔FR47・ADR-0023〕・台帳は写し）／回答が write-set を広げる（§7 の決定と C10 に反する・sha が実装の途中で変わる）／`[[contract.promise]]` の dotted header（parser の subset を広げる・top-level の array で同じ形が書ける）／負の枝の真偽欄（嘘を測れない欄は持たない・C10）／done を設計 doc に書き戻す生成（C10 の逆流・生成物は契約 file にだけ）。

## 34. 約束の行の files の既存 .rs は write-set にそのまま写り、symbols の crate:: の型の path 形は module の型の宣言で解ける（契約表の行 ai・§33 の導出の 2 つ目の穴）

- 何が起きているか（orchestrator の実測 2026-09-21・母集団 = Promised の行 3 本〔dispatcher.md 行 n / o / p〕の便 4 本・4 本とも同じ落ち方・verified）: 約束の行の `files` に `+` 無しの `.rs`（`crates/scribe2/src/fleet/mod.rs` / `crates/scribe2/src/pipe/dispatch.rs`）を書いても、導出（§33 項 3）はそれを**どの欄にも写さない**（`+` は `creates`・`.rs` でない項目は `also`・残りは捨てる）ので、write-set は歯の置き場と `+` の file だけになり、lens が「src の file が閉包の外」で FAIL する（kind `other`・4/4）。src を write-set に入れる経路は `touches` の閉包だけだが、その入力の `symbols` は受付（§33 項 5・`check_symbols`）が名指しの読み手（§26 の 3 形・型の path 形は**末尾 2 節**を型と項目に読む）で解くので、crate::fleet::Mark は「fleet::Mark」の字面が base に無ければ解けない＝**閉じた型の読み手（`closure()` が読む `crate::<module>::<Type>`・§33 項 3 の `touches`）と受付の読み手が同じ字面を違う形に読む**。`+Mark::Launched` の形は受付を通るが `touches` に写らない（`crate` で始まらない）。結果、Promised の行は「src を触る」と書く手段を持たない。
- 形（2 か所・どちらも既存の関数の入口を広げる・新しい module は無い）: (1) **`files` の写し**: `Fields`（`crates/scribe2/src/pipe/closure/derive.rs`）に 7 つ目の欄 `files`（base に実在する `.rs`・`+` 無し）を足し、`derive_write_set` の (vi) として **そのまま** write-set に載せる（tracked に無ければ `ItemUnresolved`・`.rs` でなければ従来どおり `also`）。§33 項 3 の「導出の 1 本は増やさない」は本行で改める（理由は上の実測・約束の行の `files` は「触る file の列」で、`+` の有無で write-set に載るか否かが変わるのは欄の定義に反する）。`Fields` を組む場所は `crates/scribe2/src/pipe/cli/intake.rs`（Declared の行・`files` は空）と `derive.rs`（Promised の行と in-file の歯）の 2 file だけ。(2) **`crate::` の型の path 形**: 受付の `check_symbols` が使う名指しの読み手（`crates/scribe2/src/pipe/closure/names.rs` の `resolved` → `form_of`）に、先頭の節が `crate` で末尾の節が大文字で始まる path 形を **閉じた型の形**として読む分岐を足し、`closure()` と同じ判定（module の file に `enum <Type>` / `struct <Type>` の宣言・`declares_type` と `in_module`）で解く。`+crate::…::<Type>` は宣言が**無い**ことを要求（`creates` と同じ極性）。末尾 2 節の `型::項目` の形（`Mark::Hold`）と fn 形は従来どおり。
- 触らない: `touches` の閉包の計算（`closure()` の 5 形）・`creates` / `also` / `tests` / `surfaces` の導出・§26 の impl 経路・契約 file の key・lens の雛形・約束の行の 9 欄と parser。
- 歯（in-file は `contract_promise_files_` 接頭辞・`derive.rs` の tests・e2e は `pipe_intake_promise_files_` 接頭辞・`tests/e2e/pipe/intake.rs`・既存の `promise_base()` と toy repo の型）: (a) `files` の `+` 無しの `.rs` が write-set に載る（base は載らない → RED）／(b) base に無い `.rs` は `ItemUnresolved`（base は黙って捨てる → RED）／(c) `symbols` の crate::paint::Hue（toy の閉じた型）を持つ約束の行が受付を通り write-set に toy の paint.rs の閉包が載る（base は `PromiseSymbolUnresolved` → RED）／(d) +crate::paint::Hue（宣言が在る）は断られ、+crate::paint::Fresh（無い）は通る／(e) Hue::Red の型::項目の形と fn 形は 1 字も変わらず解ける（既存の歯の緑で受ける）。
- 却下: `files` の `.rs` を `also` に流す（`also` は Rust の外の file と決めた欄・`AlsoNamesRust` の断りを消すことになる）／`tests` に流す（`teeth_file` が歯の区間を要求し src が落ちる）／受付の読み手を `touches` の読み手に置き換える（末尾 2 節の形が解けなくなる・§26 を壊す）／doc 側で `use crate::fleet::Mark` の字面を書かせる（設計 doc が code の字面に合わせる逆流・C10）。

## 35. 焼き直しの門の teeth-outside-write-set の物差しは path の形の項目だけを測る（契約表の行 aj・§23 の物差しの下界・memo `s2-07l.527`）

- 何が起きているか（orchestrator の実測 2026-09-21・便 `s2-07l.513` の 3 周目・verified）: lens が `at` に path でない項目（歯の接頭辞 `headless_lens_promise_`・`§33`）を混ぜて `teeth-outside-write-set` を出すと、物差し（`crates/scribe2/src/pipe/review.rs` の `teeth_unaddressed`）は `at` の全項目を write-set の path として測るので、path でない項目は**どんな契約でも covered にならず**、docs で write-set と § を直しても受付が `finding-unaddressed` で永遠に断る。§23 の「測れない型と `at` の空な周は物差しが空を返す」は kind と空だけを見ていて、**項目単位の測れなさ**を持たない。
- 形: `teeth_unaddressed` は `at` の各項目を **path の形に解けるか**で 2 つに分け（解ける = tracked の file・末尾 `/` の dir・`+` 付きの新規 file の 3 形＝§3 の write-set の項目の形と同じ読み）、解ける項目だけを write-set と照合し、解けない項目は測らない（理由の文に「測った n 件・測れない m 件」を出す・母集団を同時に出す C10）。解ける項目が 0 の周は空を返す＝通す（`at` が空の周と同じ扱い）。`Rework` に tracked を足す（既に持つ・引数は増やさない）。
- 触らない: `literal_unaddressed` / `section_unaddressed`（識別子と § はそれぞれの物差しが読む）・同型 N 回の門（`SameKindRepeated`）・lens の雛形（`at` の形を閉じるのは行 ah 以後の別の行）・`FindingKind` の 7 語。
- 歯（in-file は `pipe_review_unaddressed_` 接頭辞・`review.rs` の tests・e2e は新設の歯が `pipe_intake_repeat_teeth_outside_write_set_` 接頭辞で、verify の行は既存の `pipe_intake_repeat_` の歯 9 本ごと撃つ〔done (3) の不変を同じ行で測る〕・`tests/e2e/pipe/intake.rs`・既存の `failed_runs` / `Again` / `assert_refused` の型）: (a) at が toy の src/other.rs と歯の接頭辞と § の番号の 3 項目の後、src/other.rs を write-set に足した契約が通る（base は断る → RED）／(b) src/other.rs を足さない契約は従来どおり断られ、理由に src/other.rs だけが名指され測れない 2 件が数で出る／(c) in-file: 物差しが path の項目だけを返す（母集団 3・path 1）。
- 却下: lens の雛形だけを直す（過去の便の `at` は書き換わらない・.513 が止まったまま）／`at` の path でない項目を「対応済み」と読む（測れないを「測った」に読み替える・C10）／`FindingUnaddressed` を `SameKindRepeated` の後ろに回す（材料が変われば通るが、path の項目が未対応でも通る＝門が緩む）。

## 36. nextest 行の読み手が引数を取る target の旗（`--bin` / `--bench` / `--example` / `-E`）の次の語を filter 語に数えない（契約表の行 ak）

やさしく言うと: verify に `--bin folio` と書くと、器は「folio」という名の歯を探し、fn 名に folio を含む無関係な歯の file が write-set の外だと断る。旗の後ろの語は target の名であって filter ではない。

- 何が起きているか（consumer の報告 2026-09-22・別 repo の便 87・orchestrator が code で再現・verified・main f133ac3）: verify の行 `cargo nextest run -p <crate> --bin <crate> --test <t1> --test <t2> …` を `pipe preflight` が `teeth-outside-write-set` で断った（fn 名に crate 名を含む歯の file 3 本を名指し）。現物: `crates/scribe2/src/pipe/closure/derive.rs` の `nextest_filter` は `--test` の次の語を `words.next()` で消費するが、`UNREAD_TARGET_FLAGS`（`crates/scribe2/src/pipe/closure.rs`）の旗は scope を `Crate` へ倒すだけで次の語を消費せず、続く `else if !word.starts_with('-')` が旗の引数を filter 語に読む（`--test` との非対称）。filter 語は最後の非旗の語が勝つので、本 doc の行 e の verify（`--bin scribe2 --no-tests=fail ledger_lint_`）は filter 語が旗の後ろに在って偶然通っていた。consumer は verify から `--bin` を外す回避を契約に書いた（散文の作法＝N2・器の側で直す）。
- 形: (1) `UNREAD_TARGET_FLAGS` を **引数を取る旗**（`--bin` / `--bench` / `--example` / `-E`・次の 1 語を消費する）と **取らない旗**（`--bins` / `--benches` / `--examples` / `--tests` / `--all-targets`）の閉じた 2 slice に分ける（宣言順・`closure.rs` の const・旗の表は増やさない）。scope の倒し方（どちらも `Crate`・fail-closed）は不変。(2) 引数を取る旗が行末に在る（次の語が無い）周は `?` で `None`＝行を読めないと断る（`--test` と同じ極性・黙って通さない）。(3) filter 語の規則（最後の非旗の語・fn 名の substring）と `-p` / `--lib` / `--test` の読みは不変。
- 歯（in-file・`crates/scribe2/src/pipe/closure/derive.rs` の `mod tests`・接頭辞 `contract_derive_target_flag_`・`nextest_filter` の pure な歯）: (a) `-p x --bin x foo_` → filter `foo_`・scope `Crate`。(b) `-p x --bin x --test face foo_` → filter `foo_`・scope `Crate`（scope の旗と読めない旗の並び＝従来どおり広い側）。(c) `-p x --bin`（引数なし）→ `None`。(d) `-p x --bins foo_` → filter `foo_`（取らない旗は従来どおり）。(e) `-p x -E expr bar_` → filter `bar_`（式 1 語を消費）。
- 触らない: scope の 3 値と置き場の導出（§28）・`PACKAGE_FLAGS` / `LIB_FLAG` / `TEST_FLAG`・Declared の門（§20）が同じ 1 関数を通ること・filter 語の意味・`teeth-outside-write-set` の断りの字面と極性。
- 却下: consumer が `--bin` を verify から外す運用のまま（散文の作法・N2・他の consumer が同じ穴を踏む）／旗の引数を filter 語にも読む（名と filter の 2 義・偽陽性の根そのもの）／nextest の全旗の表を持つ（旗が増えるたびに表が育つ・「引数を取るか」の 2 slice で足りる）／`-E` の式を scope に読む（式の解釈は器の外・従来どおり広い側）。

## 37. pipe/review.rs の「要件本文の読み手」の群を子 module へ割る（契約表の行 al・純移動・§15 と [pipeline.md](./pipeline.md) §45 の型）

やさしく言うと: 審査役に渡す材料を作る file が上限（1500 行）まで残り 89 行しか無く、この file を触る便が S でも受付で断られる。責務が閉じている「要件面（yaml / md / html）から要件の本文を読む」群を、名前も本文も変えずに子の file へ移して余地を作る。

- 出所（orchestrator の実測 2026-09-22・`pipe dispatch ls` と `pipe preflight`）: `crates/scribe2/src/pipe/review.rs` は幅 120 で正規化した行数が **1411**（上限 R-C4-2 = 1500・余地 **89**）で、行 r（[gate-cost.md](./gate-cost.md)・`s2-07l.462`・size M）と行 c（同・`s2-07l.230`・size S）が `cap-headroom` で受付を通らない（S の見積 100 > 89）。
- 現物（orchestrator が grep と正規化行数で実測・main ca9bb75）: 責務は 6 群（定数 51–77・判定の語彙と読み手 79–267・焼き直しの門 269–362・材料の組み立て 364–559・**要件本文の読み手 561–768**・lens の駆動と決着 770–935）で、in-file の歯は 937 行から（`#[cfg(test)]` の次の非空行が `mod tests {`＝札は `mod tests {` の直後に置ける・[pipeline.md](./pipeline.md) §45 の `#[path]` 形の罠には当たらない）。**要件本文の読み手の群は閉じている**: item は 13 個（`Found` / `requirements_text` / `requirement_row` / `requirement_md` / `md_heading` / `requirement_yaml` / `BODY_KEYS` / `BODY_JOIN` / `yaml_entry` / `unquote` / `Member` / `yaml_member` / `strip_tags`・561–768 行・正規化 **209** 行）で、親の本体から裸で呼ばれるのは **`requirements_text` の 1 site だけ**（438 行・`materials`）、他 module（`crates/scribe2/src/**`・`crates/scribe2-boundary/tests/**`）からの参照は **0 site**（`review::` の 31 site を全数確認・`table/check.rs` の 2 件は doc comment の字面）、群が親から引くのは `table::read`（1 site・579 行）と `std::path::Path` だけで、親の const・型・`Contract` は 1 つも引かない。群に struct は無く（`Found` / `Member` は enum）、field を歯が構築する型も無い＝§45 の「親に残す型」の判断は要らない。歯の `use super::{…}`（939–943 行）が名指す群の名は 6 つ（`requirement_md` / `requirement_row` / `requirement_yaml` / `requirements_text` / `strip_tags` / `Found`）。
- 名前解決の形（§45 と同じ・可視性は名前解決をしない）: 親に `use` を置く。解く名は 6 つで、うち**親の本体に site が在るのは `requirements_text` の 1 つだけ**、残り 5 つ（`requirement_row` / `requirement_md` / `requirement_yaml` / `strip_tags` / `Found`）は歯だけが読む。5 つを素の `use` に入れると通常 build で `unused_imports` → `-D warnings` で rc 101 になるので、`use` は**本体用（1 名・素）と歯用（5 名・`#[cfg(test)]` 付き）の 2 文**に割る（属性は別の行・`residual_allowed` が許す残差は `#[cfg(test)]` だけの 1 行）。群の中だけで呼ばれる 7 名（`md_heading` / `BODY_KEYS` / `BODY_JOIN` / `yaml_entry` / `unquote` / `Member` / `yaml_member`）は可視性を 1 語も変えない。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 13 item（561–768 行・正規化 209 行）を、行 al の write-set の `+` の file へ名・本文・順序・doc comment を変えずにそのまま移す（doc comment は item の一部＝1 字も書き換えない・`[`table::requirement_ids`]` の link は子の `use super::table;` で解ける）。子の頭は module doc と `use super::table;` / `use std::path::Path;` の 2 行だけ。
  2. 親に増えるのは **4 行だけ**——`mod` 宣言 1 行（file 頭の `use` 群〔37–49 行〕の直前・親に既存の `mod` 宣言は無い）、本体用の素の `use` 1 行（`requirements_text`・`pub` は付けない・`use` 群の隣）、歯用の `#[cfg(test)]` だけの 1 行と `use` 1 行（5 名・**既存の行頭 `#[cfg(test)]`〔937 行〕の直上**＝file 頭に置くと xtask の src / test の切れ目が最初の行頭 `#[cfg(test)]` へ動き、本体が丸ごと歯の区間に落ちる）。4 行とも 120 桁に収まる。
  3. 歯は 1 本も足さず 1 本も変えない: in-file の `mod tests` の本文と `use super::{…}` は 1 byte も変えない（その `use` は親の `use` 2 文が解く）。e2e（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs` の `pipe_review_reads_requirements_` 4 本）は binary 越しで名を引かず、write-set の外。
  4. 上げるのは**子側**の可視性だけで、語は `pub(super)` の 1 種類。上げる集合は名指しで **6 つ**（`requirements_text` / `requirement_row` / `requirement_md` / `requirement_yaml` / `strip_tags` / `Found`・全部 item の頭の行）。enum の variant は enum の可視性を継ぐので variant の行は触らない。親側の可視性は変えない。
  5. 純移動の札 `// flip-check: moved <行 al の bead>` を親の `mod tests {` の直後（[dispatcher.md](./dispatcher.md) §20 の着地形）と子の module doc の直後に 1 行ずつ置く（説明 1 行 + 札 1 行の 2 行・[pipeline.md](./pipeline.md) §7 の `moved` の逃がし・入口の RED は札が担う）。
  6. 検証行が名指す歯は**既存の 8 本**（接頭辞 `pipe_review_requirements_text_` の 2 本と `pipe_review_yaml_shall_` の 6 本・全部 in-file・新設 0 本）で、着地後も名・本数・本文が不変。
- write-set の面（§3 の逐語: 縮む面は「`-` 接頭辞で宣言・base に実在する file・この便でその file の増分は 0 以下という見積の符号を項目が運ぶ」）: 親 `review.rs` は **縮む面**（`-`・余地 89 の file を触る便なので、素の path で書くと受付が自分の見積で `cap-headroom` に倒れる＝本行が受付を通らない・実測 2026-09-22 の `pipe dispatch ls`・§15 の行 o と同じ形）、子は **新規 file**（`+`）。diff の面は「親から 13 item が消え、子に同じ 13 item が現れる」の 2 file だけで、`-` は削除の宣言ではない。
- 見積: 親 約 1202 行（余地 約 298＝size M を受けられる）・子 約 215 行。`review.rs` を write-set に持つ未着地の行は 2 本（[gate-cost.md](./gate-cost.md) 行 c と行 r）で、どちらも lens の駆動と決着の群（`decide` / `settle`）を触る＝移す群と交差しない。
- 触らない: 移す群の外の 5 群（定数・判定の語彙・焼き直しの門・材料の組み立て・lens の駆動）・`pub(in crate::pipe)` の `design_material`（`strip_visibility` が剥がさない語＝群 4 を出す周の罠・本行は触らない）・e2e の歯・`table.rs` / `table/check.rs`。
- 却下: 焼き直しの門の群（269–362）を出す（97 行しか減らず余地 186 で M に届かない・`cli/intake.rs` が `Rework` / `unaddressed` を 2 site 使うので親に `pub use` が要る・子が `FindingKind` と `closure::{…}` を引いて結合が重い）／判定の語彙の群を出す（8 module と e2e が `review::` で引く公開面）／材料の組み立ての群を出す（`design_material` の `pub(in crate::pipe)` が `items-differ` に化ける）／lens の駆動の群を出す（未着地の行 c / r が触る面と交差）／`#[cfg(test)] use …;` を 1 行に畳む（`residual-line`・[pipeline.md](./pipeline.md) §45 の便 4 本目の再現）。

## 38. pipe/review.rs の「判定の読み手と受付の 2 門」の群を子 module へ割る（契約表の行 am・純移動・§37 と同じ型・2 便目）

やさしく言うと: §37 で 209 行を出しても、その後の着地で親は上限まで残り 293 行に戻り、size M の便（見積 300）がまた受付で断られる。この file は設計 4 本・28 行が write-set に持つ hub なので、責務が閉じている「審査の判定 file を読む・受付の門が指摘の対応を測る」群をもう 1 つ子へ移して余地を 400 行台にする。

- 出所（orchestrator の実測 2026-09-22・`pipe preflight`）: `crates/scribe2/src/pipe/review.rs` は幅 120 で正規化した行数が **1207**（余地 **293**）で、[gate-cost.md](./gate-cost.md) 行 r（`s2-07l.462`・M）が `cap-headroom` で受付を通らない（M の見積 300 > 293）。
- 現物（orchestrator と census の実測・main a654561）: src 区間は 1–730（列 0 の最初の `#[cfg(test)]` は 731＝§37 が置いた歯用の `use` の属性行・`mod tests {` は 734・735 に §37 の札。flip-check の test 区間の始点は「次の非空行が `mod` で始まる」条件で 733＝札は `mod tests {` の内側に置く）。src の帯は 9 つ（宣言 37–51・定数 53–79・入口の型 81–126・理由の型 128–198・detail 200–217・**判定の読み手と 2 門 218–364**・材料の型と本体 366–434・材料の組立 436–581・lens の駆動と着地 583–729）。**判定の読み手と 2 門の帯は閉じている**: item は 14 個（`review_path` / `review_dir` / `verdict_of` / `ROW_SAME_KIND_STOP` / `Judgement` / `judgement_of` / `split_at` / `Rework` / `unaddressed` / `sorted` / `teeth_unaddressed` / `path_shaped` / `literal_unaddressed` / `section_unaddressed`・218–364 行・正規化 **150** 行）。親の本体から裸で呼ばれるのは 3 名（`review_path` 697 行 `settle`・`review_dir` 565 行 `keep`・`verdict_of` 96 行 `ReviewCheck::judge`）、他 module からの参照は 8 名（`review_path` ← `ledger/memo.rs`・`review_dir` ← `dispatch/candidates.rs` と `cli/intake.rs`・`ROW_SAME_KIND_STOP` ← `refuse.rs` と `cli/intake.rs`・`Judgement` / `judgement_of` ← `dispatch.rs` と `dispatch/candidates.rs` と `cli/intake.rs`・`Rework` / `unaddressed` ← `cli/intake.rs`・全部 `review::` の path で `pub`）。歯の `use super::{…}` が名指す群の名は `Judgement` / `judgement_of` / `split_at` / `Rework` / `unaddressed`。**field を構築する型は `Judgement` と `Rework` で、field は既に `pub`**（`cli/intake.rs` と歯が構築する・可視性を触らない＝items-differ が起きない）。private のまま子に閉じるのは 5 名（`sorted` / `teeth_unaddressed` / `path_shaped` / `literal_unaddressed` / `section_unaddressed`・461–463 行の `sorted` は `render_promises` の局所変数で別物）。
- 名前解決の形（§37 と同じ・`pub use` は残差の許容形〔`is_use_head` は可視性を剥いで `use` を見る・`pipe/table.rs` 37 行と `pipe/land.rs` 87–88 行の前例〕）: 親に **`pub use`** を 2 行置いて 8 名を解く（`pub use judgement::{judgement_of, review_dir, review_path, unaddressed, verdict_of};` と `pub use judgement::{Judgement, Rework, ROW_SAME_KIND_STOP};`・1 行に畳むと 120 桁を超える）。`pub use` は親の名前空間にも入るので、本体の 3 site と他 module の `review::` 始まりの path が 1 字も動かない。**歯だけが読む `split_at`** は `#[cfg(test)]` だけの 1 行と `use judgement::split_at;` の 1 行（既存の列 0 `#[cfg(test)]`〔731 行〕の直上）で解く＝上げるのは `split_at` の可視性 1 語（`pub(super)`）だけ。**親で孤立する import は 3 つ**（`use super::closure::{unresolved_names, ClosureError, Source};`〔39 行・群だけが使う〕と `use super::refuse::covered;`〔43 行・324 行だけ〕は行ごと削り、44 行の `use super::{…}` から `run_dir` だけ外す）。子の頭は `use super::{json_lite, FindingKind, Verdict, REVIEW_DIR, REVIEW_FILE};` / `use crate::pipe::closure::{unresolved_names, ClosureError, Source};` / `use crate::pipe::refuse::covered;` / `use crate::pipe::run_dir;` / `use std::path::{Path, PathBuf};` の 5 行（`covered` は `pub(crate)`・親の private な const は `super::` で子から見える＝§37 の先例）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 14 item（218–364 行・正規化 150 行）を、行 am の write-set の `+` の file へ名・本文・順序・doc comment を変えずにそのまま移す（doc の intra-doc link `` [`ReviewCheck::judge`] `` / `` [`design_material`] `` は 1 字も触らない・rustdoc の門は CI に無い）。子の頭は module doc と札と上の `use` 5 行だけ。
  2. 親に増えるのは **5 行だけ**——`mod judgement;` 1 行（37 行 `mod requirements;` の直前）、`pub use` 2 行（`mod requirements;` の直後）、歯用の `#[cfg(test)]` だけの 1 行と `use judgement::split_at;` 1 行（731 行の直上）。孤立した use の削除（2 行と 1 語）はこの数に含めない（残差の許容形）。
  3. 歯は 1 本も足さず 1 本も変えない: in-file の `mod tests` の本文と `use super::{…}` は 1 byte も変えない。e2e は binary 越しで名を引かず、write-set の外。他 module の `review::` の path も不変。
  4. 上げるのは**子側**の `split_at` の 1 名だけ（語は `pub(super)`）。`pub` の 8 名と `Judgement` / `Rework` の `pub` field、親側の可視性は変えない。
  5. 純移動の札 `// flip-check: moved <行 am の bead>` を親の `mod tests {` の内側（§37 の札 735 行の**次の行に足す**＝既存の札は置き換えない・base に無い札だけが効く・`closure.rs` / `land.rs` に札 2〜3 本の前例）と子の module doc の直後に 1 行ずつ置く。子は歯の区間を持たないので数に入るのは親の札だけ。**file 頭に `#[cfg(test)]` を置かない**（xtask の src / test の切れ目が file 先頭に動く）。
  6. 検証行が名指す歯は**既存の 4 本**（in-file の `pipe_review_judgement_reads_kind_and_splits_at` / `pipe_review_unaddressed_measures_each_kind_with_one_ruler` / `pipe_review_unaddressed_teeth_measures_only_path_shaped_items` / `pipe_review_check_reads_review_json_fail_closed`・新設 0 本・repo 内で名は一意・接頭辞の包含関係なし）で、着地後も名・本数・本文が不変。
- write-set の面（§3 の逐語: 縮む面は「`-` 接頭辞で宣言・base に実在する file・この便でその file の増分は 0 以下という見積の符号を項目が運ぶ」）: 親 `review.rs` は **縮む面**（`-`）、子は **新規 file**（`+`）。diff の面は「親から 14 item が消え、子に同じ 14 item が現れる」の 2 file だけで、`-` は削除の宣言ではない。
- 見積: 親 1207 → 約 1063（余地 約 437＝size M を受けられる）・子 約 165 行。xtask の門の副作用は無い（実測）: `std::env::` の src 区間の site は 0（750 行の 1 site は test 区間）、`Command::new` は 0 site、`pub(in crate::pipe)` の `design_material`（495–500 行）は帯の外、`FindingKind` と `FINDING_KINDS` は親に残るので `enum-slices` は不変、`ROW_SAME_KIND_STOP` の読み手は他 file なので `rules-wired` の数も不変。
- 行 r（[gate-cost.md](./gate-cost.md) §26・`s2-07l.462`）との交差: 行 r が触る `decide` / `settle`（lens の駆動と着地の帯 583–729）は 1 つも動かないので、行 r の write-set は `review.rs` のままでよく、本行の着地で余地だけが増える。
- 触らない: 移す帯の外の 8 帯・`design_material`・`Judgement` / `Rework` の `pub` field・e2e の歯・§37 が置いた子 `requirements.rs` と札。
- 却下: 材料の組立の帯（436–581・154 行）を出す（親に残る `review`〔416–434 行〕が `material.promises` を読むので `Material` の field を `pub(super)` に上げる＝items-differ）／lens の駆動と着地の帯（583–729・149 行）を出す（行 r の `decide` / `settle` と正面衝突し、行 r の write-set に受け皿の file を足す往復が要る）／§37 の札を置き換える（持ち越しの対が崩れる）／`pub use` を 1 行に畳む（120 桁超）。

## 39. 名指しの実在が他の行の宣言済み・未着地の新規 file を解く（契約表の行 an・`s2-07l.475`）

- 出所（隣の repo の planner の報告 2026-09-18・便 25 / 26 で実測。本 repo でも同じ型が出る）: 着地前の便の write-set に宣言した新規 file を、次の便の設計文が backtick で名指すと name-unresolved が出る。直列の便で次の便の契約を先に書く運用だと毎回出る。回避は散文で書くこと＝作法（N2）で運んでいる。
- 何が起きているか（現物・main 4f70b12・verified）: `check.rs` の `name_findings` は解の母集団を**その行の write-set と creates だけ**から組んで `unresolved_names` に渡す。他の行の宣言は見ない。母集団の実測: 契約表の区間を持つ doc は 15 本、宣言の印を持つ write-set の行は 42 本、宣言された新規 file は重複なしで 94 本、うち **41 本が base に無い**＝この 41 本が今はどの行からも名指せない。`depends` の相手は既に doc の全行から解く形（§30・行 ad）が在るが、名指しの母集団は行 1 本に閉じたままである。
- 形（母集団は **repo の全 doc**）: 行 id は doc の中でだけ一意だが **path は repo で一意**である。宣言の印は「その path がこの repo にこれから在る」の唯一の正本で、doc の境は意味を持たない（隣の repo の報告も doc を跨ぐ形だった）。
  1. 検査の文脈（`table.rs` の `Context`・「repo の側の事実」を持つ struct）に**宣言済みの新規 file の列**を 1 つ足す。作る 1 本は `check.rs` に置き、tracked な設計 doc の区間を読んで全行の write-set の印つき項目と creates の欄を集める（印は剥がす・重複は畳む）。
  2. `name_findings` はその行の分に文脈の列を足して母集団にする。`unresolved_names` の引数と、印を剥がして path 形の解に足す規則は 1 字も変えない。
  3. 文脈を組む側は 2 つ——CI の駆動（`check.rs` の repo 判定）と受付の材料（`intake.rs` の `Materials`・**1 周に 1 回の読み**）。どちらも同じ 1 本を呼ぶ。受付は doc を 22 本（1.65 MB）読み足すが、同じ材料が既に tracked な `.rs` 175 本（6.07 MB）を読んでいる。
  4. **読めない doc は黙って飛ばさない**: 区間を読めない doc が在る周は母集団を縮めたまま通さず、その読めなさを従来の 1 件として出す口に合流させる（読めなさを「宣言 0 本」に読み替えない）。
- 触らない: name-unresolved の字面と在り処の形・型の path 形と fn 形の解き方・write-set の項目の実在の検査・`depends` の母集団・findings の順と rc。
- 却下: 母集団を同じ doc の全行に限る（小さいが、報告された doc 跨ぎの形を閉じない。path は repo で一意ゆえ doc の境を引く根拠が無い）／宣言済みを別の理由で出して rc 0 にする（読み手が毎周読み飛ばす音が残る）／名指しを散文に書き替える運用のまま（作法を増やす・N2）／宣言の印を無条件に解く（印の無い path まで通すと base に無い名の検査が空洞化する）。
- 歯（接頭辞 `contract_names_declared_`・`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` の既存の `contract_names_impl_` / `contract_closure_ext_` の歯の隣。`crates/` 全体で 0 件＝衝突なし）: (a) doc を 2 本持つ toy repo で、doc A の行が新規 file を宣言し doc B の行の done がその名を backtick で名指す → findings 0・rc 0（**base で RED**: name-unresolved が 1 件）。(b) どの行も宣言していない名を名指した行は従来どおり 1 件（負例・母集団が無条件に広がらない）。(c) 同じ doc の別の行の宣言でも解ける。(d) 受付でも同じ: doc B の行を pointer に受付を撃つと run dir と event が作られる（base は断られる）。(e) 現物の契約表が findings 0・rc 0。
- 既存の歯の書き換え（1 本）: `contract_closure_ext_unresolved_names_are_named_with_their_place` は「別の行が宣言した新規 file を名指した行は解けない」を assert している（母集団が行 1 本に閉じている pin）。本行はこの前提を変えるので、その assert を「解ける」へ替える。同じ歯の他の 4 件（型の path 形・fn 形・節の本文・一致しない字面）は 1 字も変えない。


## 40. 審査の材料に write-set の各 file の base の要約を足す（契約表の行 ao・`s2-07l.431`）

- 出所（別 project の planner の報告 2026-09-17・便 6 で 4 周 INCONCLUSIVE ＝ 約 40 分。逐語は台帳 `s2-07l.431` の notes・ここは要旨）: 審査（lens）は契約と設計の節と要件しか読めないので、既存の file を触る契約の「base の中身がこうだから done が測れる」を判定できず、`section-material-missing` の INCONCLUSIVE が往復する。回避は planner が base を実測して設計の節に写すことで、設計 doc の散文が実測の文で肥大する（作法で運んでいる＝N2）。本 repo の事前審査でも同じ型が出る。
- 何が起きているか（現物・main f25084c・verified）: 審査の材料の穴は **4 つ**（`crates/scribe2/src/headless/lens-contract.txt` の 43 行のうち :37 の契約・:40 の設計の節・:43 の要件と約束の行）で、write-set の file の中身は 1 つも渡っていない。材料の file は **3 本**（`crates/scribe2/src/pipe/review.rs` の :61 / :64 / :67 の 3 つの定数）で、置く側は同じ file の `keep`（:422）、読む側は `crates/scribe2/src/headless/lens.rs` の `material_of`（:275-290）と `promises_of`（:248）である。約束の行の穴は「材料を 1 つ足して穴を 1 つ足す」が 1 例 land 済みであることを示す（`promise_block`・:258）。memo の論点 (2)（「読めば判る」型を gate へ寄せる）は §22 / §23 の理由の型と焼き直しの門で別経路で進んだので、本 § は論点 (1) だけを採り、lens の観点 3 つの本文は 1 字も変えない。
- 形（材料を 1 本足す・観点は変えない）:
  1. **材料を 1 本足す**: 契約の write-set の各項目の base の要約を器が作り、審査の材料の dir へ 1 file として置く（既存の 3 本と同じ置き方・`keep` の同じ loop）。材料を組む 1 本は**行 ao の write-set の `+` の file**（`review.rs` の子 module）に置き、`materials`（:295）から 1 回だけ呼ぶ。
  2. **要約 1 本の中身**: 項目の path・行数の 2 面（全体と本体）・本体の区間の宣言の名の列・歯の区間の `#[test]` の直下の fn の名の列。`.rs` でない項目は path と行数だけ。`+` の項目（base に無い）は「新設」の 1 行。読めない項目はその読めなさの 1 行にする（黙って落とさない・C10・既存の材料と同じ扱い）。
  3. **雛形の穴を 1 つ足す**: `lens-contract.txt` の末尾に穴を 1 つ足し、`lens.rs` が材料の写しから埋める。**写しが無い周は空文字**＝雛形は 1 字も変わらない（約束の行の穴と同じ形）。diff の審査の雛形（`lens.txt`）は触らない。
  4. **cap は新しい閾値を作らない**: 要約は**最後に**足し、足すと既存の cap を越える周は**要約の段ごと落として**、落とした項目の本数を明示の 1 行に残す。既存の 4 材料だけで越える周の極性（claude を呼ばず INCONCLUSIVE）は不変＝rules 行も新しい値も足さない（C5 の裁定を要らなくする）。
  5. **区間の読み手は 1 本**: 歯の区間は `crates/scribe2/src/pipe/closure.rs` の `test_region`（:330）を crate の中へ開いて使い、本体の区間は同じ file の `src_region`（:340・既に開いている）、行数の 2 面は `crates/scribe2/src/pipe/declaration/write_set.rs` の `FileLines` の `of`（:145・既に開いている）を使う＝2 本目の読み手を作らない（C2）。
  6. **1 走査で埋める**: 穴を埋めるのは既存の 1 走査の対に 1 つ足すだけで、埋めた本文の中の穴の字面は展開しない（外から来る text が雛形の構造へ触れない・既存の裁定と同じ）。
- 触らない: lens の観点 3 つの本文・理由の型の 6 語・判定の JSON の形・diff の審査の雛形と極性・既存の 3 材料の中身と置き方・`promise_block` の見出しと 3 語の限り・審査の rc。
- 却下: 設計の節に base の実測を写す運用のまま（作法を増やす・N2・設計 doc が肥大する）／要約でなく write-set の file の**全文**を渡す（NFR1 の予算を材料 1 本で食う・cap で落ちる周が増える）／lens に tool を渡して自分で読ませる（審査の前提「shell も cargo も撃てない」を壊す）／材料 file を増やさず契約の写しの中へ埋める（契約の字面と器の生成物が 1 file に混ざり、焼き直しの門の突合が割れる）／要約の大きさに rules 行を足す（C5 の裁定が要る・既存 cap で足りる）。
- 歯（接頭辞 2 つ・どちらも `crates/` 全体の fn 名の substring に 0 件＝衝突なし）:
  - `pipe_review_base_`（行 ao の write-set の `+` の file の in-file の歯と、`crates/scribe2-boundary/tests/e2e/pipe/review.rs` の e2e）: (a) fixture の `.rs` 1 本で、本体の宣言の名と歯の名が**別の列**に出る（母集団 = fixture の宣言の本数と歯の本数を同じ assert で数える）・(b) `.rs` でない項目は path と行数だけ・(c) `+` の項目は新設の 1 行・(d) 読めない項目は読めなさの 1 行・(e) cap を越える周は段が落ちて落とした本数の 1 行が残る・(f) e2e は受付から審査まで通した run の材料の dir に要約の file が在り、write-set の各項目の path を持つ。
  - `headless_lens_base_`（`crates/scribe2/src/headless/lens.rs` の in-file の歯）: 写しが在れば穴が本文で埋まり、無ければ雛形が 1 字も変わらず、契約の本文が穴の字面を持っていても展開されない（1 走査）。
  - 既存の歯で名を変えるものは無い。`crates/scribe2/src/headless/lens.rs` の既存の材料の歯（2 本とも在る / 片方だけ在る / 読めない の 3 値）は本数も本文も不変。

## 41. 歯の置き場の門の断りに出所（解いた verify 行の filter 語）を添える（契約表の行 ap・`s2-07l.474`）

- 出所（別 project の planner の報告 2026-09-18・便 23 で 1 往復を失った。逐語は台帳 `s2-07l.474` の notes）: 「verify の歯の file が write-set に無い」の断りが 2 つの出所（verify の filter 語から解いた歯の file と、goal / done の backtick が base の歯の名に当たった周）で同じ字面になり、読み手が前者と読んで verify 行を疑った。
- 何が起きているか（現物・main f25084c・verified）: memo の 2 つ目の出所は**もう無い**。§27（行 aa・`s2-07l.476`）が契約の散文の字面走査を機構ごと消したので、散文の経路の呼び手は 0 件である（`crates/` 全体で `prose_closure` の字面 0 件）。断りを作る 1 本は `crates/scribe2/src/pipe/closure/derive.rs` の `check_teeth_cover`（:135）で、production の呼び手は同じ file の `declared_teeth`（:130）**1 本だけ**＝出所を別の variant に分ける相手が居ない。残っている穴は別で、**照合の 1 本が出所を受け取っていない**: `declared_teeth` は verify の行ごとに置き場を解いて 1 つの集合に畳む（:121-129）ので、断りは write-set に無い file を名乗るが、**どの verify 行の filter 語がその file を連れてきたか**を名乗らない。verify を 3 行持つ契約では planner が対応を手で引き直す。隣の断り（同じ門の解けない filter）は既に filter 語を持っている（`crates/scribe2/src/pipe/closure.rs` の :232 の理由の 1 行）ので、本 § はその形に揃える。
- 現物の site（母集団・verified）: 断りの型を持つ src の file は **4 本**（`crates/scribe2/src/pipe/closure.rs` の宣言 :213 と理由 :242 / `crates/scribe2/src/pipe/closure/derive.rs` の作る側 :140 / `crates/scribe2/src/pipe/refuse.rs` の宣言 :165 と語 :242 と理由 :295 と rc :338 / `crates/scribe2/src/pipe/cli/intake.rs` の写し :828）で、payload を**分解する** site は 4 つ（closure.rs :242・refuse.rs :295・intake.rs :828・derive.rs :140 の作る側）である。審査の理由の型の同名の語（`crates/scribe2/src/pipe/review.rs` の :139）は別の閉じた型で、本行は触らない。
- 形（照合の 1 本は共用したまま payload だけを対にする）:
  1. `declared_teeth` が verify の行ごとに解いた置き場を、file だけの集合でなく **file とその行の filter 語の対**として畳む（同じ file を 2 行が解いた周は verify の**先の行**の語・file の辞書順）。`teeth_places` の signature は変えない（Promised の導出も同じ 1 本を呼ぶ・C2）。
  2. `check_teeth_cover` はその対を受け、write-set に無い分を**対のまま**断りの payload にする。照合（正規化と dir 項目の配下）は 1 字も変えない。
  3. 断りの型の欄を対の列にし、理由の 1 行が file と filter 語の両方を名乗る。受付の側の同じ名の型も同じ欄にし、理由の 1 行は導出の側の 1 本を写すだけの形を保つ（2 面に書かない）。
  4. 断りの語（辞書の 1 語）・rc・run dir を作らないこと・断る条件は 1 字も変えない。
- 触らない: 門を撃つ条件と順・解けない filter の断りの字面・`teeth_places` の signature と Promised の導出・審査の理由の型の 6 語・受付の rc と event・§20 の門の下界。
- 却下: 出所ごとに 2 つ目の variant を足す（memo の案。2 つ目の出所は §27 で消えた＝分ける相手が居ない・空の分岐を作る）／新しい pub な対の型を足す（型を 1 つ増やす。対の意味は欄の doc comment で足りる）／file の列と filter 語の列を**別の欄**で並べる（添字で対応させる形は片方が空の周に嘘になる）／断りは file だけのままにして planner が verify 行を引き直す（回避を作法で運ぶ・N2）。
- 歯（接頭辞 `contract_teeth_origin_`・`crates/` 全体で 0 件＝衝突なし）:
  - in-file（`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・pure な 1 本を直に呼ぶ）: verify 2 行の契約で file ごとに**別の** filter 語が付く（母集団 = 対の本数と行の本数を同じ assert で数える）・同じ file を 2 行が解いた周は先の行の語・write-set に全部在れば通る（対は空）。
  - in-file（`crates/scribe2/src/pipe/refuse.rs` の歯の区間）: 断りの 1 行が file と filter 語の両方を名乗る。既存の歯 `refuse_derive_reasons_are_last_and_name_their_payload`（:523）は payload の字面を測っているので、本行がその 1 件の期待を対の字面に替える（他の理由の期待は 1 字も変えない）。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）: 受付の stderr が file と filter 語を両方名乗り、rc 1 で run dir を作らない。既存の歯 `contract_declared_teeth_outside_write_set_is_refused`（:951）は file だけを測っているので、本行が filter 語の期待を足す（rc と run dir の期待は不変）。

## 42. Declared 行の歯の置き場が write-set の `+` の新規 .rs も置き場と読む（契約表の行 aq・`s2-07l.481`・§20 の逃がしの 1 点）

- 出所（consumer の報告 2026-09-19・逐語は台帳 `s2-07l.481` の notes）: base に 0 本の filter 語と、新設の歯の file と、既存の歯の file を触らない契約が、Declared 行に置き場の欄が無いので受付できない。回避は「本文を変えない既存の歯の file を write-set に載せる」＝要らない file を write-set に足す作法である。
- 何が起きているか（現物・main f25084c・verified）: memo の一部は `s2-07l.391`（§20）で塞がった。`crates/scribe2/src/pipe/closure/derive.rs` の `declared_teeth` は :119 の 1 行で逃がしを立てるが、その条件は「write-set に **base に在る**歯の file（歯の区間が空でない `.rs`）が 1 つでも在る」で、母集団は base の source の本文の列だけである。write-set の `+` の項目は base に無いので本文の列に入らず、**新規の歯の file だけを足す Declared 行は今も解けない filter の断りで落ちる**（既存の歯 `contract_declared_teeth_new_filter_needs_a_teeth_file_in_write_set`・`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` :991 の (c) がこの負例を pin している）。別経路は開いている: Promised 形は `crates/scribe2/src/pipe/closure/derive.rs` の `teeth_file`（:284）が置き場の欄の項目を `creates` の側で照合し、宣言済みの新規 file を**本文を見ずに path だけで**置き場と認める。
- 形（逃がしの条件を 1 つ広げる・下界は Promised 形に揃える）:
  1. :119 の逃がしの条件に、「write-set の `+` の項目に `.rs` が 1 つでも在る」を**または**で足す。base に在る歯の file が在る周の挙動は 1 字も変えない。
  2. 下界は Promised 形と同じにする: 宣言済みの新規 file は本文が無いので path だけで置き場と認める（`teeth_file` が `creates` の項目を照合するのと同じ弁別・2 本目の規則を作らない・C2）。
  3. 逃がしが効く範囲は**解けない filter の断りだけ**で、他の理由（読めない source・置き場の欄の項目の不整合）はそのまま断る＝:125-126 の弁別を 1 字も変えない。
  4. base にも `+` にも歯の置き場が 1 つも無い write-set は従来どおり同じ字面で断る（負例が残る）。
- 触らない: 解けない filter の断りの字面と型・行ごとに解く形（先に在る新しい接頭辞の行で止まらない）・置き場を解く関数と Promised の導出・write-set に無い歯の file の断り・受付の rc と run dir の扱い・§20 の門そのもの。
- 却下: `+` の項目を `tests/` 配下に限る（Promised 形の宣言済みの新規 file は path の位置を見ない＝規則が 2 本に割れる。src の中に歯を置く便を断る根拠も無い）／Declared 行にも置き場の欄を開く（欄の意味が Promised 形と割れ、§33 の導出と衝突する）／base で 0 本の filter 語の行を無条件に読み飛ばす（§20 の門の下界を失い、置き場の検査が空洞化する）／consumer の回避（要らない既存の歯の file を write-set に足す）を作法のまま運ぶ（N2・write-set が事実と食い違う）。
- 歯（接頭辞 `contract_declared_place_new_`・`crates/` 全体で 0 件＝衝突なし。既存の `contract_declared_teeth_` の歯の隣）:
  - in-file（`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・pure な 1 本を直に呼ぶ）: base に 0 本の filter 語 1 つを持つ 3 通の write-set で rc の型を測る（母集団 = 3 通）——`+` の新規 `.rs` を 1 つ持てば通る／`+` が `.rs` でない項目だけなら従来の断り／base の歯の file も `+` の `.rs` も無ければ従来の断り。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）: 契約表の行が `+` の新規の歯の file と新しい filter 語だけを持つ周に受付が rc 0 で run dir を作り、同じ行から `+` の項目を外すと従来の字面で rc 1・run dir を作らない（母集団 = 2 回の受付の rc）。
  - 変えない既存の歯: 同じ file の (c) の負例（:991・`+` を持たない write-set は断られたまま）と、`crates/scribe2/src/pipe/closure/derive.rs` の `contract_declared_teeth_resolves_each_line_and_reads_a_written_teeth_file_as_the_place`（:742）は本文も期待も不変。

## 43. 受付が中身を変えない file で便を断る 3 型（契約表の行 ar / as / at・`s2-07l.441`）

- 出所: 隣の repo の planner の実測 2026-09-17（要旨・逐語は台帳 `s2-07l.441`）。受付が (1) 触らない file にも上限の余地を課し、(2) verify の filter 語が fn 名の部分一致で他の歯に解けて write-set が連鎖的に膨らみ、(3) 未追跡の設計 doc が契約表の検査の母集団に入らないまま findings 0 と出る。回避はそれぞれ「size の申告を下げる」「触らない file を write-set に足す」「追跡してから測り直す」で、3 つとも作法で運ぶ形である（N2）。
- 何が起きているか（現物・main 3b258a3・verified）:
  - (1) `crates/scribe2/src/pipe/declaration/write_set.rs` の `headroom_shortfalls`（:164）は write-set の項目のうち `File` / `New` / `Dir` を母集団に取り、余地を求めないのは `Shrink`（`-`）と `Delete`（`~`）の 2 つだけである。「verify の置き場として載せただけ」を表す項目の形は無い。閉じた列挙（:11）の網羅 match は src に **2 か所**（同 file の `headroom_shortfalls` と `crates/scribe2/src/pipe/cli/intake.rs` の `headrooms_of`）、構築は同 file の `read_item`（:68）の **1 本**、接頭辞の字面の定数は `crates/scribe2/src/pipe/refuse.rs` の **3 本**（`NEW_FILE` / `SHRINK_FILE` / `DELETE_FILE`）、接頭辞を剥がす 1 か所は同 file の `normalize`（:405）である。
  - (2) 置き場の照合は `crates/scribe2/src/pipe/closure/derive.rs` の `teeth_places`（:157）の述語 1 つで、歯の fn 名が filter 語を**含む**かを見る。行 ak（§36）が着地して引数を取る旗の次の語は filter 語に数えなくなったが（`crates/scribe2/src/pipe/closure.rs` の `UNREAD_ARG_TARGET_FLAGS`（:75）に `-E` が在る）、式そのものは verify 行に書けない（宣言の形の門が括弧と引用符を断る）。括弧の要らない完全一致の口（`--` の後ろの `--exact`）は `nextest_filter`（:204）が読み飛ばし、名の全体が末尾の裸の語として部分一致で照合されて 0 本に倒れる。つまり狭く書く道具は在るが受付が読めない。
  - (3) `crates/scribe2/src/pipe/table/check.rs` の `judge_repo`（:406）は `tracked_files`（:503）の列から `design_docs`（:439）で設計 doc を選ぶだけで、未追跡の下書きは母集団に入らず知らせも無い。判定行（`check_repo`・:357）は doc 数・行数・findings 数の 3 値だけを持つ。
- (1) 置き場だけの項目の印（行 ar）
  - 形: 1. `crates/scribe2/src/pipe/refuse.rs` に接頭辞の定数を 1 つ足す（既存 3 本の隣・`=`）。2. 閉じた列挙に変種を 1 つ足し、`read_item` がその接頭辞を base に在る file にだけ解く（`-` と同じ弁別で、契約表の検査と受付で読みを変えない）。3. `normalize` の剥がす集合に 1 文字足す（剥がす規則を 2 か所に持たない・§3）。4. 網羅 match の 2 か所で、新しい変種を `Shrink` / `Delete` と同じ腕に置く（余地を求めず core の見積の本数にも数えない）。5. 交差・guard・gate の write-set 照合・契約表の検査は素の path のまま（`normalize` が剥がすので 1 字も変えない）。
  - 触らない: 断りの字面と型と rc・`+` / `-` / `~` の弁別と policy の読み替え・core の余地の式・gate の write-set 照合の述語・交差の判定・受付の run dir の扱い。
  - 却下: verify の置き場を write-set の外の別欄にする（Declared 行に欄を足すと §33 の導出と欄の意味が割れる・§42 と同じ理由）／解けた歯の file を余地の母集団から機械が自動で外す（本当に変える file との区別が base に無い＝印が要る）／新しい印の file への編集を guard が断る（印の意味を「不変」まで強めると gate に段が 1 つ増える・別便）／size を下げて回避する運用を続ける（申告が歪む・N2）。
  - 歯（接頭辞 `contract_place_only_`・`crates/` 全体で 0 件）: in-file（`crates/scribe2/src/pipe/declaration/write_set.rs` の歯の区間・pure な 1 本を直に呼ぶ）で、印の項目が base に在れば解け・無ければ従来の未解決・余地の母集団に入らず・core の見積の本数に数えない（母集団 = 同じ fixture の項目の本数と対で数え、素の path の項目は従来どおり入ることを同じ assert で見る）。in-file（`crates/scribe2/src/pipe/refuse.rs` の歯の区間）で `normalize` が新しい接頭辞を剥がす（既存 3 形の隣）。e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）で、余地の足りない file を印つきで載せた行が受付を通って run dir が出来、同じ行から印だけを外すと従来の余地不足の字面で rc 1・run dir を作らない。
- (2) verify 行の `--` の後ろの `--exact` を完全一致の filter として読む（行 as）
  - 前提（.550 の便 113148Z の問い・verified）: verify 行は `sh -c` で撃たれるので、宣言の形の門（`crates/scribe2/src/pipe/declaration.rs` の `METACHARS`・ADR-0010 §2.3）が `(` `)` と引用符を断る。nextest の式（`-E`）は括弧と引用符を要るので verify 行には**書けない**——式を読む形は採らない。nextest には括弧の要らない完全一致の口が別に在る: `--` の後ろに `--exact` を置くと、`--` の後ろの裸の語を **test の名の全体（module path 込み・`pipe::closure::derive::tests::<fn>` の形）** と完全一致で読む（実測 2026-09-22・nextest 0.9.143: 名の全体は 1 本に解け、fn 名だけでは 0 本）。
  - 何が起きているか（現物・`nextest_filter`）: `--` と `--exact` は `-` で始まる読めない語として読み飛ばされ、名の全体は末尾の裸の語として filter 語になるが、置き場の照合は fn 名が filter 語を**含む**かなので 0 本に倒れ、`tests` 欄が無ければ `TeethPlaceUnresolved` で断られる（静かに通りはしない・fail-closed は既に在る）。つまり狭く書く道具は在るが受付が読めない。
  - 形: 1. `crates/scribe2/src/pipe/closure.rs` に `--` の後ろの libtest の旗の閉じた 2 列を足す——引数を取る旗（`--skip`）と引数を取らない旗（`--exact` / `--include-ignored` / `--nocapture` / `--no-capture`）。2. `nextest_filter` が `--` を境に読みを切り替える: `--` の前は従来どおり、`--` の後ろは 1 の 2 列で旗を読み飛ばし（`--skip` は次の 1 語も消費）、裸の語を filter 語にする。`--exact` が在れば filter 語の一致の型を**完全一致**、無ければ従来の**部分一致**にする。3. 完全一致の filter 語は `::` で割った**末尾の段**を fn 名と等値で照合する（module path の段は照合しない＝同じ fn 名を持つ別 file も置き場に入る側へ倒す・閉包は広い側で fail-closed）。`::` を持たない語に `--exact` が付いた周は末尾の段 = 語そのもの。4. 1 行に filter 語は 1 つ（`--` の前後で 2 つ在る周は後ろが正本・従来の「末尾の裸の語」の規則と同じ）。5. 照合の述語に一致の型を渡す。`teeth_words` は末尾の段を出す（検出線の語の導出は 1 本のまま・§34 約束 5）。6. `--` の後ろに `--exact` も裸の語も無い行は従来どおり filter 語なし（`None`）。
  - 読みの細部（実装・s2-07l.550）: `--` の後ろに裸の語が無く前に在る行は前の語が filter 語のまま（`--` の前だけの行の読みを変えない＝4 の「後ろが正本」は両方に在る周だけ）。`--exact` は `--` の後ろのどこに在っても一致の型を完全一致にする。`--skip` が行末の行は引数を取る読めない旗と同じ極性で `None`。
  - 触らない: `--` の前の読み（`-p` / `--lib` / `--test` / 引数を取る読めない旗の倒し方）・`METACHARS` と宣言の形の門・`tests` 欄の置き場の弁別・断りの型と字面と rc・Promised 形の導出・gate が行を撃つ形（`sh -c`）。
  - 母集団（main 3ddfd94・verified）: tracked な設計 doc の verify 行 208 本のうち `--` を持つ行は 0 本・式の旗を持つ行も 0 本。既存の行の判定は 1 本も変わらず、`contract_closure_ext_real_table_has_zero_findings` は緑のままで、write-set は本行の 4 file で足りる。
  - 却下: nextest の式（`-E`）を読む（本 § の前の版）——宣言の形の門が括弧と引用符を断り、門を緩めると `sh -c` で撃つ行に包みと連結の口を開く（ADR-0010 §2.3・C6）。／宣言の形の門に `-E` の次の 1 語だけの逃がしを作る（引用符なしの括弧は `sh` が subshell に読む＝撃てない行を受け付ける）。／module path の段まで照合する（file の path から module path を組む 2 本目の読み手が要る・閉包は広い側に倒す方が安全）。／同じ門を契約表の検査の段でも回す——現物で測ると、Declared 行（write-set と verify を持つ行・192 行）のうち**少なくとも 5 行**（`docs/design/gate-cost.md` の g・`docs/design/pipeline.md` の a と j・`docs/design/rules-manifest.md` の h・`docs/design/seat-roles.md` の m）が、いま main で歯の file を write-set の外に持つ（2 通りの走査で共に当たった行・粗い走査では 18 行・memo `s2-07l.552`）。そのまま findings にすると現物の契約表が 0 件でなくなり既存の歯が赤になる。／`tests` 欄を Declared 行にも開く（§42 で却下した形）。
  - 歯（接頭辞 `contract_teeth_exact_`・`crates/` 全体で 0 件）: in-file（`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・pure な 1 本を直に呼ぶ）で、`-- --exact <名の全体>` の行が末尾の段と等値の fn を持つ file だけを置き場に取り、同じ段を substring に持つ別 fn の file を取らないこと（母集団 = fixture の 3 file と当たる 1 file を同じ assert で数える）・`--` の後ろの裸の語に `--exact` が無い周は従来の部分一致になること・`--skip` の次の 1 語が filter 語に数えられないこと・`--` の後ろに語が無い行が `None` になること・検出線の語が末尾の段になること。e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）で、`-- --exact` の行を verify に持つ行が、部分一致なら要求された他の file を write-set に持たないまま受付を通ること（verify 行は括弧も引用符も持たないので宣言の形の門をそのまま通る）。
- (3) 未追跡の設計 doc を知らせる（行 at）
  - 形: 1. `judge_repo` が未追跡の設計 doc（設計 doc の dir 直下の `.md`）を 1 回引く。読む口は既存の git の 1 本を使う（2 本目の読み手を作らない）。2. 判定行に未追跡の本数の欄を 1 つ足す（現物は 0）。判定行の字面を完全一致で読む既存の歯は `crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` の中と `crates/scribe2-boundary/tests/e2e/pipe/intake.rs` の 1 か所（受付の前に契約表の検査を撃つ歯）で、どちらも新しい欄を含む字面へ更新する（write-set に両 file を持つ・期待の他の部分は変えない）。3. 1 本以上の周は判定行の前に 1 件 1 行で path を名乗る（findings には数えない＝rc も findings の数も不変）。4. git が答えない周はその欄を `?` にする（0 に化けさせない・NFR4）。5. 追随の口（`repo_findings`）の戻りは 1 つも変えない（便の判定材料を変えない）。
  - 触らない: 検査の母集団（tracked な設計 doc だけ）・findings の順と rc・doc 数と行数の数え方・契約表の検査そのもの・受付の経路。
  - 却下: 未追跡 doc を検査の母集団に入れる（tracked でない行から契約を作る口を開く＝FR47 に反する）／findings に数えて rc 1 にする（下書きが 1 本在るだけで CI と受付が落ちる）／作業木の状態を読む別の git の口を足す（tracked の一覧と 2 本目の読み手になる）。
  - 歯（接頭辞 `contracts_untracked_doc_`・`crates/` 全体で 0 件）: e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）で、未追跡の `.md` を 1 本置いた木の判定行が未追跡 1 を持ち知らせの 1 行が出て rc 0・findings 0 のまま、その file を追跡すると未追跡 0 になり doc 数が 1 増える（母集団 = 同じ木の 2 回の判定行を対で見る）。in-file（`crates/scribe2/src/pipe/table/check.rs` の歯の区間）で、未追跡の列から知らせの行を組む純関数を 0 本・1 本・2 本で測る。
## 44. 審査の base の要約が置き場だけの印（`=`）を剥がして読む（契約表の行 au・.459 の便 125532Z の審査 INCONCLUSIVE）

- 出所: 行 at（[pipeline.md](./pipeline.md) §51）が `=crates/scribe2-boundary/tests/e2e/pipe.rs` を write-set に持った最初の便で、審査の材料 `base.txt` がその項目を「読めない（No such file or directory）」と出し、lens が印の意味を確かめられず INCONCLUSIVE になった。行 ar（§43 (1)・`s2-07l.441`）は受付と契約表の検査に `=` を足したが、審査の要約（§40・行 ao）は自分で接頭辞を剥がしていて `=` を知らない。
- 何が起きているか（現物・main 891e27e・verified）: `crates/scribe2/src/pipe/review/base.rs` の `item_text` は `+` の項目を「新設（base に無い）」の 1 行にし、`-` と `~` は自分で剥がして本文を読むが、`=`（`crates/scribe2/src/pipe/refuse.rs` の `PLACE_ONLY_FILE`）は剥がさず、`=` を含んだ path を読んで「読めない」になる。剥がす規則が `crates/scribe2/src/pipe/refuse.rs` の `normalize` と `crates/scribe2/src/pipe/review/base.rs` の 2 か所に在る（§3 に反する・行 ar の形 3 が 1 か所に保った規則の外側）。
- 形:
  1. `item_text` の `+` の分岐の後の剥がしを `normalize` の 1 本に替える（`-` / `~` / `=` を同じ 1 か所で剥がす・§3・剥がす規則を base.rs に持たない）。この寄せは挙動に差が出ないので歯では弁別できず、done には載せない（字面の pin は書かない・便の diff の設計適合は審査で見る）。
  2. `=` の項目は本文を読んで既存と同じ 2 列（本体の宣言の名・歯の名）を出し、行の頭は契約の字面のまま（`=` を含む）で、行数の後ろに「置き場だけ・中身は変えない」の 1 語を添える（lens が印の意味を要約から読める）。
  3. 他の項目の行の形・cap を越える周の落とし方・`+` の 1 行・repo の外の断りは 1 字も変えない。
- 触らない: `normalize` の本文と剥がす集合・受付と契約表の検査の `=` の読み（行 ar）・lens の雛形の穴 `{base}`（`crates/scribe2/src/headless/lens-contract.txt`）・要約の cap と本数の 1 行・審査の判定の 3 値。
- 却下: base.rs に `=` の分岐をもう 1 つ足す（剥がす規則が 3 つ目になる）／`=` の項目を要約から外す（lens が置き場の歯の在処を確かめられない＝本便の INCONCLUSIVE がそのまま残る）／lens の雛形に印の凡例を書く（要約の各行が印の意味を持てば足りる・雛形の穴を増やさない）。
- 歯（接頭辞 `pipe_review_base_place_`・`crates/` 全体の fn 名の substring に 0 件。既存の接頭辞 `pipe_review_base_` の verify（行 ao）にも自然に含まれる）: in-file（`crates/scribe2/src/pipe/review/base.rs` の `mod tests`・既存の fixture と同じ形）で、`+` / `-` / `~` / `=` の 4 形の項目を同じ木で要約し、`=` の項目が「読めない」を持たず宣言の列と歯の列を持ち、行の頭が `=` を含む字面のままで、置き場だけの 1 語を持つこと（母集団 = 4 形の行数を同じ assert で数え、`-` と `~` の行は 1 字も変わらない）。e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・既存の `pipe_review_base_summary_file_names_every_write_set_item` の隣）で、`=` の項目を持つ契約の審査の材料 `base.txt` が「読めない」を 1 行も持たず、その項目の行に置き場だけの 1 語が在ること。

## 46. 契約表の行が file ごとの見込み行数を宣言し、受付の上限の余地は file ごとの見込みで測る（契約表の行 ax・`s2-07l.578`・§3「上限の余地」の見積の粒度）

- 出所: [account-lifecycle.md](./account-lifecycle.md) 行 i（§20・`s2-07l.491.4`・size L・write-set の src は 11 file）が受付で cap-headroom（`crates/scribe2/src/pipe/dispatch.rs` の余地 297 / `crates/scribe2/src/fleet/mod.rs` の余地 642 / `crates/scribe2/src/account/mod.rs` の余地 677 が L の見積 800 に足りない）で断られた 2026-09-23 の型。§3 の見積は「1 file あたりの増分 = `size` の値」を write-set の全 `.rs` に一律に当てるので、複数の file を触る契約では実増分の小さい file（行 i では 3 file とも 10〜45 行の見込み・file 単位の実増分は全部 800 以下で最大は群の段の file の約 370）まで `size` の値で測られる。行を M に割っても dispatch.rs で 3 行足りず、席が場当たりの純移動で余地を作る運用（§24・§43）になっていた。user 裁定 2026-09-23（要旨・逐語は台帳 `s2-07l.578` の description）= 契約表の行に file ごとの見込みを宣言する口を足し、受付は file ごとの見込みと余地を比べる。上限の値（`size` の 3 段・R-C4-2・R-C4-1）は変えない。決定は ADR（本 § と同じ PR・契約表の欄の追加＝跨版の形）。
- 形:
  1. 契約表の行に任意の欄 growth（list・項目は path と行数を `:` で結んだ 1 語・path は write-set の項目から接頭辞を剥がした素の path・行数は 0 以上の整数）を足す。`FIELDS` に 1 欄（任意・list）・`ContractRow` に 1 field・`<NAME> contracts schema` の生成物 `contracts/schema.toml` を描き直す（xtask check の contracts-schema が render と tracked の差分 0 を測る）。意味: `size` は今までどおり**既定の 1 file あたりの見込み**で、growth は名指した file だけをその値で上書きする（名指さない file は `size` のまま）。
  2. 生成の写し（契約 file）は行の growth をそのまま運ぶ（`OPTIONAL` の key に 1 つ足す・`classes` / `opens` / `touches` / `targets` と同型・行に無ければ写しにも書かない）。`Contract` に field を 1 つ足し、写しの読み手が読む（`Contract` は既定値を持たないので、構築点 3 か所〔受付の歯・pipe の歯の fixture・極性の e2e の fixture〕が field を足す＝write-set に含める）。項目を path と行数へ分ける読み手は `crates/scribe2/src/pipe/declaration/write_set.rs` に 1 本（受付と契約表の検査が同じ 1 本を撃つ・C2）。
  3. 受付（`exclude_cap_shortfall` → `headroom_shortfalls`）: file の判定は「その file の見込み（growth に在ればその値・無ければ `size` の値）が余地を超える」で断り、core の見積は core に属する write-set の `.rs` ごとの見込みの**和**（今は `size` × 本数）。`-` / `~` / `=` の項目は今までどおり余地も本数も求めない（growth で名指しても意味を持たない＝表の検査が断る・4）。`Refuse::CapHeadroom` の理由の 1 行は file の見込みの値も出す（今は `size` の語だけ）。
  4. 契約表の検査（`contracts check` と intake の同じ 1 本・§45 と同型）: growth の項目が (a) path と行数の形でない（`:` が無い・行数が整数でない）(b) path が行の write-set の file の項目（接頭辞を剥がした素の path・dir 項目は展開しない・write-set の欄を持たない行〔導出・約束〕は growth を持てない）に無い (c) path が `.rs` でない (d) path が `-` / `~` / `=` の項目 (e) 同じ path が 2 回、のどれかなら `TableError` の 1 語 GrowthForm（行番号・項目・理由）で名指す（`TABLE_ERRORS` の末尾・母集団 15 → 16・`contracts check` は全件・行番号付き）。
  5. preflight の `headroom=<file>:<余地>/<見積>` の見積を file ごとの値にする（形は不変・今は全 file が `size` の値）。
  6. 戻す経路（user の問題提起のもう半分・要旨は台帳 `s2-07l.578`）: cap-headroom の断りは file・余地・見込みを名指す（受付の理由の 1 行と preflight の `headroom=` の列）。orchestrator はそれを読んで (i) 実増分が余地に入るなら行に growth を書く（本 §）(ii) 実増分が余地を超えるなら、その file を `-` で宣言する純移動の行を先に通す（§24・§43）か、責務を割る設計に戻る（[core-boundary.md](./core-boundary.md)・`s2-07l.198`）。器に新しい問いの型は足さない（既存の理由の 1 行と preflight が同じ数を出す・C17 の段 2「既に在る」）。
- 触らない: `size` の 3 段の値と rules 行・R-C4-1 / R-C4-2 の値・gate の測定（宣言を信じるのは受付の先読みだけ・宣言が偽でも上限は超えられない・§3・C10）・write-set の項目の文法（6 形・接頭辞 1 字・`normalize`）・xtask の台帳の計画（知らない key は読まない）・dispatch の理由の型・`Refuse` の語。
- 却下: (a) write-set の項目の末尾に `=+N` を添える形（項目の文法が 7 形になり、`=` と `+` の字を別の位置で別の意味に使う・接頭辞を剥がす site 11 file 49 か所に波及・§3 の「素の path」の規則を書き直す）(b) `size` を「契約全体の合計」と読み替えて file の判定を外す（1 file の満杯を測れなくなる・§3 の出所 3 件の型が戻る）(c) 断られた周に dispatch が Questioned の新しい型で orchestrator に返す（受付の理由の 1 行と preflight が同じ数を既に出す・型を増やさない）(d) `size` に 1 段足す / 上限の値を上げる（値は user 裁定・A2・粒度の問題は解けない）。
- 歯（接頭辞 declaration_growth_ / contract_check_growth_ / contract_growth_・`crates/` 全体の fn 名の substring に 0 件）: in-file（`crates/scribe2/src/pipe/declaration/write_set.rs` の `mod tests`）で「growth に在る file はその値で判定し無い file は `size` で判定する（`size` が余地を超え growth が入る file は通り、growth が余地を超え `size` が入る file は断る）」「core の見積が見込みの和（`size` × 本数と値が割れる fixture で A/B）」「項目の読み手が (a)〜(e) を名指す」。in-file（`crates/scribe2/src/pipe/table/check.rs` の `mod tests`）で「write-set に無い path・`.rs` でない path・`-` の項目・重複・形の崩れの 5 例が GrowthForm 5 件（行番号付き）で、正しい 3 項目は 0 件」。e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`・`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）で「tmp の repo で growth を持つ行の受付が通り、同じ行から growth を外すと cap-headroom で断られる（同じ base・同じ `size`）」「preflight の `headroom=` が growth の file だけ別の見積を出す」「`contracts schema` の生成物に growth（任意・list）が在り tracked と差分 0」。既存の pin: `FIELDS` 17 → 18・任意 10 → 11・`TABLE_ERRORS` 15 → 16。
- 行 i 側: 本行の着地後に行 i の write-set の 3 file（dispatch.rs / fleet/mod.rs / account/mod.rs）に growth を書く docs PR を出し（見込み 20 / 40 / 60・実測の見込みは 10 / 30 / 45）、`size` は L のまま受付を通す。

## 47. 導出物（`.toml`）の契約表の行が節の本文の逐語 goal を運び、表の検査・審査の材料・契約 file の goal をそこから取る（契約表の行 ay・`s2-07l.473`）

- 出所: folio2 の設計ノートの導出物の形（folio2 の ADR-3 決定 (4)・同 repo の設計ノートの欄の決まりの derived の節）は、先頭の版の宣言・`[[contract]]` の配列・各行に節の本文の逐語（goal・各行を trim して空行を落とし空白 1 つで繋いだ単一行・引用符を escape しない）・空の一覧は欄名ごと省く、である。§2 (ii) の「path を差し替えるだけ」は現物では 2 点で成立しない（folio2 の実測 2026-09-18・scribe2 main eafbfbc で再確認・要旨は台帳 `s2-07l.473`）。folio2 の M3（folio2 の ADR-16）の判定点「器の受付が導出物を読んで通す」は本行の着地まで測れない。
- 何が起きているか（現物・main eafbfbc・verified）: (1) `read_table` が本文を渡す rules manifest の読み手は契約表の行の key 集合を `FIELDS` から引き、goal を未知の key として断る（表ごと読めない）。(2) 表の検査 `check_table` は行の `section` の `## N.` の見出しを doc の中に探し、審査の材料（`crates/scribe2/src/pipe/review.rs` の `design_text`）も同じ見出しから節の本文を切り出す。`.toml` には見出しが無いので、goal を受けるだけでは検査が section-missing で断り、材料は「節が無いか空」になる。(3) 契約 file の goal は行の `title`（§2・`crates/scribe2/src/pipe/contract.rs` の `render`）。値の読み手は scalar の 1 本で外側の引用符 1 組を剥がすだけなので、中に二重引用符を含む単一行の値も読み戻せる（引用符 1 組ちょうどを求めるのは配列の要素だけ）。
- 形:
  1. 欄の正本 `FIELDS` と生成物 `contracts/schema.toml` は変えない（folio2 が外部の欄の決まりとして読む列に goal を載せない・goal は導出物だけが持つ欄）。導出物だけの欄の名を core の定数 1 つで持ち、rules manifest の契約表の key 集合をその定数の分だけ広げる。`ContractRow` に文字列の field を 1 つ足し、読み手が goal を読む（無ければ空）。構築点は `crates/scribe2/src/pipe/table/parse.rs` の読み手と歯の fixture 2 か所（`crates/scribe2/src/pipe/contract.rs` と `crates/scribe2/src/pipe/table/check.rs` の歯の区間）で、`crates/scribe2/src/pipe/cli/intake.rs` の生成の行は残りの欄を元の行から広げるので変えない（write-set に載せない・`touches` にも書かない）。
  2. 置き場の形で読みを分ける: `.md` の区間の行が goal を持てば、今と同じ字面の「未知の key goal」を goal の行番号で出して断る（区間の行は節の本文を `section` が指す・同じ本文を 2 面に持たない）。`.toml` の全文の行は goal を受ける。
  3. 版の宣言: `.toml` の全文の先頭（空行と `#` の行を除いた最初の行）が版の宣言の字面でなければ、その行番号で断る。字面（key 名と値の形）は core の定数 1 つで持ち、今の値は rules manifest の schema の版と同じ。
  4. 空の一覧: 任意の一覧の欄は今と同じく欄名の省略を空と読む（導出物が省いた欄をそのまま受ける）。必須の欄（`req` 等）の省略は今と同じく断る（空の要件は行の欠陥）。
  5. 表の検査: goal が空でない行は、節の実在と本文の非空を goal で満たす（見出しを探さない）。goal が空の行は今のまま（goal の無い `.toml` の行は section-missing で断る＝材料の無い行を受け付けない・NFR4）。名指しの実在は goal の backtick も `title` / `done` と同じ読みで数える（`.md` の行が節の本文を数えるのと同じ母集団）。
  6. 審査の材料: `design_text` は goal が空でない行では goal を本文にする（出所の 1 行 `<path>#<id> §<section>` は不変）。列外の鍵（`design_material`）は同じ 1 本を通るので変えない。
  7. 契約 file: `render` は goal が空でない行では goal を写し、空の行は今のまま `title` を写す（写しの key 集合と読み手は不変）。
- 触らない: `.md` の pointer の経路（区間の抜き出し・節の読み・goal = `title`）・`FIELDS` と生成物・goal 以外の未知の key の断り・空配列の拒否・契約 file の key 集合と読み手・pointer の形（`form_of` / `parse_pointer`）・`contracts check` の母集団（設計 doc の dir 直下の `.md`）・約束の行の読み。
- 却下: (a) `FIELDS` に goal を任意の欄で足す（生成物に載り、folio2 の正本の側と `.md` の区間でも手で書ける欄になる＝節の本文の二重化）(b) folio2 が `.md` を併産する（正本の二重化・folio2 の ADR-3 に反する）(c) `.toml` の行が別の `.md` の節を読む（導出物 1 file で閉じない）(d) 読み手が goal の行を前処理で抜いてから渡す（第 2 の parser・ADR-0010 §2.1）。
- 歯（接頭辞 contract_whole_goal_・`crates/` 全体の fn 名の substring に 0 件）: in-file（`crates/scribe2/src/pipe/table/parse.rs`）で、goal（二重引用符と backtick を含む単一行）を持つ `.toml` の行が goal を逐語で読み、省いた任意の一覧が空で `req` を省いた行は断られ、同じ行を `.md` の区間に置くと未知の key goal を goal の行番号で断り、先頭が版の宣言でない `.toml` はその行番号で断る。in-file（`crates/scribe2/src/pipe/table.rs`）で、版の宣言の定数の字面が folio2 の ADR-3 決定 (4) の `schema = 1` と一致し（drift の歯・台帳 `s2-07l.214` の (3)）、rules manifest の版と同じ値で、導出物だけの欄の名が goal で `FIELDS` と生成物に無い。in-file（`crates/scribe2/src/pipe/table/check.rs`）で、`.toml` の doc の goal を持つ行が section-missing 0 件・goal の無い行が 1 件で、goal の中の解けない名が goal の在り処で名指される。in-file（`crates/scribe2/src/pipe/contract.rs`）で、goal を持つ行の写しが goal を運び `Contract::parse` で逐語に読み戻り、goal の無い行は `title` のまま。e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`）で、toy repo に `.toml` の導出物を commit し pointer をその行に向けた受付が通り、審査の材料 design.txt が出所の 1 行と goal を持ち、契約 file の goal が行の goal と等しい（同じ行を `.md` に置く既存の歯 `pipe_review_reads_design_section_and_requirements_from_base` は不変で緑）。
- 後続: `contracts check` の母集団に導出物の置き場を足す（置き場は消費側の宣言・folio2 の ADR-16 の着地後に drift の歯の便と同じ便・台帳 `s2-07l.214`）／folio2 の導出の命令が着地したら、導出物 1 本で本行の e2e と同じ受付を実測する（M3 の判定点 ③）。

## 48. 3 クラス（消す / 出す / 使う）を契約表の行の verify 行から rules 行の語列で導き、名乗らない行を契約表の検査が断る（契約表の行 az・`s2-07l.167`・[ADR-0061](../../design-intent/decisions/ADR-0061-classes-are-derived-from-verify-lines-and-undeclared-rows-are-refused.html)）

- 出所: 全体監査 2026-09-12 の塊 9（逐語は台帳 `s2-07l.167`）。A1 の 3 クラスの判定は行と契約 file の `classes`（既定は空）の自己申告だけに効き、`crates/scribe2/src/pipe/approve.rs` の Blocked は名乗った行にしか立たない。main の契約表は rows=238・verify 553 本（全部 `cargo nextest run`・`git` / `bats` で始まる行 0）・`classes` を書いた行 0（main e5da320）。[pipeline.md](./pipeline.md) §5.5 は deny list を未着とし、§11 は機械の enforcer を後続に置く。決定は ADR-0061: 導出の入力は行の verify 行だけ、照合は禁じる語列と同じ 1 本、止めるのは受付（in-loop）、`contracts check` は docs PR の CI での事後の早い知らせ。
- 形:
  1. 3 クラスの名を `crates/scribe2/src/pipe/contract.rs` の閉じた型 1 つ（3 値・宣言順は delete / publish / consume）にし、契約 file の `classes` の受理集合（今の `CLASSES` の字面）をその型から取る。`Contract` と `ContractRow` の `classes` の欄の形（文字列の列）は変えない（approve.rs を触らない）。同じ file に要素の読み手 1 本を置く: 先頭の 1 語がクラスの名で残りが 1 語以上の要素だけを（クラス, 語列）に分け、それ以外は読めない側へ返す。rules の読みと表の検査がこの 1 本を呼ぶ（C2）。行 id の定数も同じ file に置く。
  2. rules 行 1 つ（クラスの語列表・id は runner.class_commands・kind は `RuleKind` の variant RunnerClassCommands で `ALL` の末尾・値は文字列の列）。値（裁定 id user 2026-09-24T00:13Z・要旨・逐語は台帳 `s2-07l.167` の notes）: publish git push ／ delete git push --delete ／ delete git push -d ／ consume は要素なし。manifest の値はこの 3 要素（名 + 語列の字面）をこの順に持ち、行の裁定 id は user 2026-09-24T00:13Z・裁定日は 2026-09-24。consume に要素が無いのは allowlist の cargo・git・bats に追加課金の command が無いため（便の spawn が「使う」に当たるかは ADR-0061 の射程外）。git branch -D は禁じる側の語列なので要素にしない（死に値）。値の変更は裁定 id 付きの user 裁定だけ（C5）。`rules/manifest.toml` の置き場は `runner.denied_commands` の行の直後。`as_str` と `shape` の match は関数の行数の上限の縁に在るので、新しい arm は既存の行に畳む（`HostGuardRmProtected` の arm と同じ手）。
  3. rules の読み（`crates/scribe2/src/rules/mod.rs` の `names_are_known` と `crates/scribe2/src/rules/manifest.rs` の `collect`）が 4 つの崩れを行番号つきで断る: (a) 要素の先頭語が 3 クラスの名でない (b) 語列が空 (c) 語列の先頭語が `runner.allowed_commands` の値に無い (d) 語列が禁じる語列のどれかを含む（語列を 1 本の command と見て `crates/scribe2/src/hook/command.rs` の pub の `denied_in` の照合に当てる・hook/command.rs は write-set の外で可視性も本体も変えない＝禁じる語列の先頭語が語列の先頭語と同じで、残りの語がすべて語列に在る）。(d) の禁じる語列は受付が読む和集合（`runner.denied_commands` と host の見張りの語列の 3 行・`crates/scribe2/src/hook/command.rs` の `denied_of` の読み手 1 本を command guard と共有）とする。(c)(d) に当たる要素は受付が先に断るので当たる行が無い死に値になる。禁じる語列と一部だけ重なる要素（例 git push と git push --force）は通し、禁じる側の断りが先に立った残りの行だけに当たる。(a)(b) は行 1 つで決まり、(c)(d) は行を跨ぐので `check_duplicate_ids` と並べる。上限の行か和集合の 4 行が揃わない manifest では (c)(d) を撃たない（契約表の検査を撃つ各経路は自分が読む上限と禁じる語列の行が欠けた周を既に断る＝形 5）。値の空の列は既存の読みが既に断る。
  4. 契約表の検査（`check_table`・`contracts check` と受付が撃つ同じ 1 本）: 行ごとに verify 各行を語列表の各要素に `denied_in`（禁じる語列と同じ照合の 1 本）で 1 要素ずつ当て（照合は最初の 1 件だけを返すので要素ごとに撃って集める）、当たった要素のクラスの集合（導出）を作り、導出に在って行の `classes` に無いクラスを `TableError` の 1 語 ClassUndeclared（行番号・verify 行・語列・クラス）で全件名指す（`TABLE_ERRORS` の末尾）。`classes` が導出と同じか広い行は通す（字面に現れない操作を書き手が名乗る余地）。クラスは verify 行の命令の語列（名）で決め、宛先を解かない（ADR-0008 の却下案 (ii) の宛先の判定を持ち込まない）。止めるのは受付（in-loop）で、`contracts check` は docs PR の CI での事後の早い知らせとし止めの代わりにしない。行の `classes` の綴り違いは導出を覆わないので、当たった側が名指される。受付は既存の `Refuse::ContractTable` で便を作らない。`TableError` の閉包で `crates/scribe2/src/pipe/table/parse.rs`（variant を作る側）と `crates/scribe2/src/pipe/refuse.rs`（`Refuse::ContractTable` が包む側）も write-set に入る（網羅の match が動かなければ無変更）。
  5. 材料: `Ceiling` に欄 1 つ（語列表の値）を足し、組み立ての全箇所が埋める（main 8ca0dc9 の census: src の 5 か所と test の fixture 3 か所〔`crates/scribe2/src/pipe/declaration.rs` 1・`crates/scribe2/src/pipe/table/check.rs` 2・字面の同じ行に足すので増分 0〕）。表の検査を撃つ全経路（`contracts check` = `crates/scribe2/src/pipe/cli.rs` の `contracts`・受付 = `crates/scribe2/src/pipe/cli/intake.rs` の `ceiling_of`〔`Rows` の `borrow`〕・resume の組み直し = 同 file の `regenerated`・land の追随 = `crates/scribe2/src/pipe/follow.rs` の `stale_rows_in`）は同じ行 id（runner.class_commands）の 1 行を禁じる語列と同じ読み手で読み、行が無い・manifest が読めない・不発効・列でない周は断る（導出を空として通さない・NFR4）。land の追随だけは `--rules` の差し替えを受けず常に埋め込みの manifest を読む（禁じる語列と同じ差）。follow は読めない側＝従来の再 gate へ倒れる。表の検査を撃たない `crates/scribe2/src/pipe/cli/step.rs` の `requirements_of` は空の列を渡し、intake の `freeze` は `ceiling_of` の同じ材料を借りる（欄は読まない）。`table::Context` に欄 1 つ、`Materials` に欄 1 つ（`TableFacts` は変えない）。`crates/scribe2/src/pipe/declaration.rs` の余地は幅で正規化して 3 行（R-C4-2）なので、足すのは欄の doc 1 行と field 1 行だけ。
- 触らない: 承認の口（approve.rs・Blocked・逐語の承認 event・承認の subcommand）と申告の口（行の `classes`）・`TableFacts`・write-set の読み（6 形の項目は導出の入力にしない・`~` の削除は git で戻せるので「消す」に数えない）・land の口（vessel 宣言の remote への push と CI の判定の行・`pipe land` の CLI 引数 `--pr-cmd`）・vessel 宣言の共通 verify と検出線・禁じる語列の判定（`verify_unfit`）・約束の行の生成の verify（表の検査は区間の行の欄を読む・生成値は `cargo nextest run` だけ）・runner が便の中で撃つ Bash（command guard の領分）。∴ 自 repo への branch の push と PR は導出に現れない（ADR-0008 は不変）。
- 歯（接頭辞 class_derive_・`crates/` 全体の fn 名と全 doc の verify の filter 語の substring に 0 件）: in-file（contract.rs の `mod tests`）で「受理集合が閉じた型の名の列と一致」「要素の読み手が 3 形（名 + 語列・未知の名・名だけ）を分ける」。in-file（manifest.rs の `mod tests`）で「崩れ (a)〜(d) が 1 件ずつ行番号つきで断られ、上限の内の要素と禁じる語列と一部だけ重なる要素（git push）は通り、(d) は host の見張りの語列 3 行のどれかを含む要素でも断られる」。in-file（`crates/scribe2/src/pipe/table/check.rs` の `mod tests`）で「同じ行の verify 2 本と要素 2 つが 3 件の ClassUndeclared で出る」「`classes` が同じか広い行は 0 件」「write-set に `-` と `~` の項目を持ち verify に語列の無い行は 0 件（write-set を読む変異で赤）」「語の順の違う verify（git log --grep push と要素 publish git push）も当たる」。e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`・`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）で「tmp の repo で名乗らない行を `contracts check` が名指し、受付は rc 1・run dir 0 で同じ字面の findings を出し、`classes` を足した同じ行は両方とも通る」「語列表の行が無い・不発効・列でない `--rules` で両方とも行 id を名指して rc 1」。fixture の語列は宣言の allowlist の内の `cargo` で始め、`verify-form` の断りと混ざらない形にする。e2e（`crates/scribe2-boundary/tests/e2e/rules.rs`）で「埋め込みの行の値が裁定の 3 要素（publish git push / delete git push --delete / delete git push -d・この順）で、行の裁定 id が user 2026-09-24T00:13Z・裁定日が 2026-09-24 で、各要素が崩れ (a)〜(d) に当たらない」（fn 名は class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id・要素か裁定 id を 1 つ変える変異で赤・tests/ の file は受付の上限の余地の外〔余地は crates の src の core だけを測る〕）。構造の連鎖: `sample_value` に kind の分岐 1 つ（名 + 語列の要素）、e2e の共有の rules の fixture（`crates/scribe2-boundary/tests/e2e/pipe.rs` の `write_rules_capped`）に語列表の行 1 つ（無いと受付の歯が全部断られる）、外形 snapshot の rows / kinds が 1 ずつ動く。
- 却下（ADR-0061 の 5 案）: 導出を申告に足して Blocked に送る（行と器の判定が割れ、止まるのが編集の後）／verify 行の判定を実行の時点に置く（gate の直前の照合でも command guard でも止まるのは token を使った後で、受付の断りを遅れて行うのと同じ・gate の変種は既存の質問の経路に乗れない。runner のそれ以外の Bash に command guard を広げる形は退けず、後続の決定に残す）／`classes` を必須にする（判定が申告のまま）／語列を code の定数表に置く（C1 / C5 / C14 の迂回）／write-set・land の口・宣言まで入力にする（ADR-0008 の廃止形の再導入・A4.3 に反する）。本 § の分岐: 語列表を `TableFacts` に通す案と行 id を declaration.rs に置く案は、declaration.rs の余地を超えるので採らない。
- 後続: 値を広げる裁定（C5）の前には verify の母集団を測り、当たる既存の行に同じ PR で `classes` を書く（裁定 user 2026-09-24T00:13Z の 3 要素は main e5da320 の verify 553 本に当たる行 0 本＝既存の行への書き足しは要らない）。`runner.allowed_commands` を広げる裁定は同じ PR で語列表を裁定し直す。[pipeline.md](./pipeline.md) §5.5 と §11 の散文と `crates/scribe2/src/pipe/land/finish.rs` の注記の更新。`--pr-cmd` に外向きの道具を渡す周と runner の Bash は開いたまま（ADR-0061 の Negative・別の決定）。

## 49. 審査の base の要約が pub(in path) の可視性の宣言を読み落とす（契約表の行 ba・便 `s2-07l.601` の審査 FAIL の根）

- 何が起きているか（実測 2026-09-24T02:53Z・verified）: 便 `s2-07l.601`（行 az）の審査が「write-set の 19 file のどれも `struct Materials` を宣言していない」で FAIL した。現物は write-set の内側 `crates/scribe2/src/pipe/cli/intake.rs` の pub(in crate::pipe) struct Materials で、審査の材料 base.txt の intake.rs の宣言の列に無い。
- 現物（main 629b1fd・verified）: `crates/scribe2/src/pipe/review/base.rs` の `declared_name` は行を空白で語に切り、「pub」に開き括弧が続く語だけを飛ばす。pub(in crate::pipe) の可視性は空白で「pub(in」と「crate::pipe)」の 2 語に割れ、2 語目が先頭語として残って `DECL_KEYWORDS` に無いので宣言と読まれない。母集団: src で pub(in <path>) の宣言は 37 行 / 10 file（intake.rs の `Materials` を含む）。pub(crate) / pub(super) は 1 語なので落ちない。
- 効き: 宣言が材料から落ちると lens が「宣言 file が閉包の外」と読み、正しい write-set の契約が審査 FAIL で止まる。`.601` は行 az の sha を動かして N+1 し、本行の着地を `depends` に持つ（材料が直る前に同じ lens に当てない）。
- 形（番号は done と 1:1）: 1. 括弧つきの可視性は閉じ括弧までを 1 まとまりとして飛ばす（「pub」に開き括弧が続く語から閉じ括弧を含む語まで・閉じ括弧の無い行は宣言と読まない＝fail-closed）。 2. 既存の読み（pub(crate) / pub(super) / 修飾語 / `const fn`）と既存の歯の期待は 1 字も変わらない。 3. 歯は base.rs の in-file（接頭辞 `pipe_review_base_pub_in_`・base で 0 本＝機能不在の RED）。
- 却下: lens の指示文に「pub(in の宣言は見えない」と書く（材料の欠けを散文で埋める・N2）／intake.rs の可視性を pub(crate) に変える（読み手の穴を書き手で避ける・行 az の write-set の外の変更）／`declared_name` を Rust の parser に差し替える（依存の増・A3・語の切りで足りる）。

## 50. 終端の CI の照合は rules 行の間隔で撃つ — 唯一の待ちの周期を完了条件ごとに持ち、CI の照合だけを秒の間隔にする（契約表の行 bb・memo `s2-07l.689`・user 裁定 2026-09-27T11:14Z）

- 何が起きているか（実測 2026-09-27・verified）: 09:25Z 以後、この host の GitHub の user の API 呼び出しが「HTTP 403: API rate limit exceeded」で断られ、窓の reset（10:26Z）まで全 project の gh が止まった。直前の 1 時間に scribe2 の便 8 本と別の器の repo の便 4 本が着地し、それぞれの終端が CI の完了まで数分の間、API を呼び続けていた。
- 現物（main 9b2dc4b・verified）: 唯一の待ちの実装 `crates/scribe2/src/fleet/wait.rs` の `wait` は、どの完了条件も const `POLL`（20 ms）ごとに評価する。§5 手順 2 の `Completion::CiResult` は評価のたびに `ci_now` で CI の 1 行（既定は `gh run list --commit {sha} …`）を子 process で撃つ。1 回が forge の API の 1 回である。20 ms は pid の生存や meminfo の読みの周期で、外の API の周期ではない。
- 形（番号は done と 1:1）:
  1. **rules 行を 1 本足す**: id `pipe.ci_poll_s`・kind `PipeCiPollS`（Int・秒）・値 30・裁定 id `user 2026-09-27T11:14Z`・裁定日 2026-09-27。manifest の行は `pipe.ci_wait_s` の直後に置き、kind は `ALL` の `PipeCiWaitS` の直後に置く。終端の材料の読み手（`pipe/cli/step.rs` の `terminal_input`・land と `--terminal-only` が共有）が `pipe.ci_wait_s` と同じ `int_row` で読み、`Land` の欄 `ci_poll_s` で終端へ運ぶ。行が無い・読めない周は `pipe.ci_wait_s` と同じ極性で断る（値を code に焼かない・C5）。
  2. **待ちの周期は完了条件ごと**: `Completion::CiResult` にデータの欄 `every`（`Duration`）を足す。`Completion` の 1 関数（周期を返す・網羅の match）が、`CiResult` には `every` と `POLL` の大きい方、他の全 variant には `POLL` を返す。`wait` はこの周期で眠る。0 秒の行は `POLL` に戻る（外の API を 20 ms で撃つ既存の挙動と同じ・hot loop にしない）。他の完了条件の周期と評価の中身は 1 字も変わらない。
  3. **最初の評価は眠る前**（今と同じ）: push の直後に 1 回撃ち、解けていなければ周期だけ眠る。success が最初の評価で読める周は待たずに close へ進む。
  4. **上限を越えて眠らない**: 眠る長さは周期と「上限までの残り」の小さい方。`pipe.ci_wait_s` より短い間隔で最後の評価を撃ち、上限の後に周期ぶん余計に待たない。
  5. 終端の 7 値・3 段（push → CI → close）・`ci_now` の判定（落ちた run を先に見る・schedule の run を数えない）・`.vessel.toml` の `ci-cmd` の形・`pipe.ci_wait_s` の値は変えない。
- 歯（番号は形の番号）:
  - e2e（`pipe_terminal_ci_poll_` 接頭辞・`crates/scribe2-boundary/tests/e2e/pipe/land/order.rs`・既存の偽 CI の helper を、呼ばれた回数を数えられる形で使う）: (a) 偽 CI が走り続ける JSON を返し、fixture の manifest が `pipe.ci_wait_s` = 2・`pipe.ci_poll_s` = 1 の周は `terminal=ci:unmeasurable` で、偽 CI の呼び出しが 2 回以上 4 回以下（20 ms の周期なら数十回＝RED）。(b) 偽 CI が最初から success を返し、`pipe.ci_poll_s` = 30 の周は `terminal=closed` まで 10 秒未満（最初の評価が眠る前であることの pin）。(c) 偽 CI が走り続け、`pipe.ci_wait_s` = 1・`pipe.ci_poll_s` = 30 の周は `terminal=ci:unmeasurable` まで 10 秒未満（上限を越えて眠らないことの pin）。base では manifest が kind `PipeCiPollS` を知らずに断るので 3 本とも RED（機能不在）。
  - lib（`fleet_wait_ci_interval_` 接頭辞・`crates/scribe2/src/fleet/wait.rs` の歯の区間）: `CiResult` の周期は `every`、`every` が 0 なら `POLL`、他の全 variant は `POLL`。
  - rules（`rules_ci_poll_` 接頭辞・`crates/scribe2-boundary/tests/e2e/rules.rs`）: 埋め込みの manifest に行が 1 本在り、id / kind / 形 Int / 値 30 / enabled / 裁定 id / 裁定日が上の 1 のとおりで、行は `pipe.ci_wait_s` の直後・kind は `PipeCiWaitS` の直後。文字列の値は形の違いで断られる。
  - 既存の歯の数の pin: 埋め込みの manifest の行数と kind の数（`rules/embedded.rs` の 2 か所）と `rules_external_form` の snapshot が 1 ずつ増える。e2e の共有の fixture manifest（`tests/e2e/pipe.rs` の `write_rules`）に行を 1 本足し、値は 0（既存の終端の歯の挙動を変えない）。
- 却下: 断られた周（403・rate limit）だけ間隔を延ばす backoff（断られるまで 20 ms で撃つ形が残る・断りの字面を読む読み手が要る）／CI の 1 行を `gh run watch` に替える（`ci-cmd` の宣言は repo ごとの任意の 1 行で、forge の CLI の待ちの口に依存させない・判定の形〔落ちた run を先に見る〕が CLI の側へ移る）／`POLL` そのものを延ばす（pid・meminfo・札の待ちまで遅くなる・周期は完了条件の性質）。

## 51. 審査の材料に、契約が名指す write-set の外の物を器が束ねる（契約表の行 bc・非公開の隣の project の契約の審査の section-material-missing 20 件・memo `s2-07l.430`）

- 何が起きているか（実測 2026-09-27T13:45Z・verified）: 同じ器を使う非公開の隣の project（以下「隣の project」・件数だけを書く）の契約の審査は 90 回のうち 20 回が `section-material-missing`（INCONCLUSIVE 18・FAIL 2）で、PASS 以外の理由の型で最も多い。lens の evidence が指す物は大半が write-set の外に在る（件は重なる）: write-set の外の型・fn・field の着地と形 8 件・data file の鍵（json の鍵・stylesheet の class・yaml の id の行）6 件・別の便が置いた fixture の在りかと中身 6 件・crate の Cargo.toml の依存 3 件・依存の相手の行 3 件・親 module の公開の宣言 1 件。残りは別の設計ノートの節 1 件・repo の外の file の形 1 件・節そのものの欠け 4 件。本 repo の置き場でも審査 732 回のうち 71 回（INCONCLUSIVE 65・FAIL 6）が同じ型で、`at` に並んだ識別子 63 個のうち 24 個は材料のどこにも backtick で書かれていない。2026-09-17 の調べ（台帳 `s2-07l.430` の notes）も同じ提案（名指した fn の宣言 file と可視性を材料に足す）を残したまま未着手である。
- 現物（main 8f6072d・verified）: 審査の材料は節の本文・要件・約束の行・base の要約（§40 / §44 / §49）の 4 本で、base の要約は write-set の**中**の file の宣言の名だけを持つ（`crates/scribe2/src/pipe/review/base.rs` の `item_text`・:59）。置く側は `crates/scribe2/src/pipe/review.rs` の `materials`（:307）と `keep`（:436）、読む側は `crates/scribe2/src/headless/lens.rs` の `prompt_of`（:216）と `beside`（:260）、雛形の穴は `crates/scribe2/src/headless/lens-contract.txt` の :43 に 3 つ（要件・約束の行・base）。名指しの読み手（`crates/scribe2/src/pipe/closure/names.rs` の `unresolved_names`・:27）は backtick の中身だけを読み、path 形は拡張子 `.rs` だけを名指しと読む（`form_of`）。隣の project の契約表は 3 file・54 行で backtick を 1 個も持たないので、この読み手が 20 件の契約から拾う名は 0 個である。
- 形（番号は done と 1:1）:
  1. **材料を 1 本足す**: 組む 1 本は行 bc の write-set の `+` の file（`review` の子 module）に置き、`materials` から 1 回だけ呼ぶ。置く側は `keep` の約束の行と同じ形（本文が空でない周だけ置く）で、名指しが無い契約の材料の dir は今の 4 本のまま。既存 4 本の中身と置き方は 1 字も変えない。
  2. **名の照合は名指しの読み手の 1 本**: `crates/scribe2/src/pipe/closure/names.rs` に照合の口を 1 本足し、`crates/scribe2/src/pipe/closure.rs` の `pub use` から開く。口は本文の列と候補を受け、(a) 候補の名のうち、backtick の中身の先頭の token（`head_of` の切り方・`::` で結んだ節のどれか）に等しいもの、または backtick の外に `holds_word` の語の境界で現れ、かつ字面が識別子の形を持つもの（語の中に `_` を持つか、大文字で始まる山が 2 つ以上の CamelCase）、(b) path 形の文字集合（英数字と `_ . / -`・`PATH_CHARS`）の連なりのうち `/` か拡張子（末尾の `.` と英字）を持つもので、tracked の path に `path_matches`（等しいか `/` 区切りの末尾一致）で解けるもの（拡張子を問わない・同じ末尾が 2 file 以上に解ければその全部）を返す。backtick の外の素の 1 語（`_` も 2 つ目の山も持たない名・`/` も拡張子も持たない連なり）は、base に宣言や同名の dir が在っても名とも path とも読まない（下の「素の語の雑音」の実測・数の閾値を持たない構造の規則）。data file の鍵の照合（形 3 (c)）はこの規則を掛けない（照合の相手が名指された 1 file の鍵に限られる）。本文は節の本文（導出物の行は goal）・done・約束の行の text。候補の名は base の `.rs` の宣言の名と歯の名で、読み手は base の要約と同じ `declared_name`（本体の区間）と `tooth_names`（歯の区間）を開いて使う（2 本目の読み手を作らない）。`unresolved_names` の判定と 3 形は 1 字も変えない。
  3. **名ごとの材料**（1 名 = 1 塊・塊の頭は行頭の `- ` と名・並びは (f) → (d) → (e) → (a) → (b) → (c)＝小さい構造の材料を先に置く）:
     - (a) `.rs` の item: 所在（file と行）・直上の doc 行と属性行・宣言の行の字面（可視性を含む）・struct / enum / union / trait は閉じ括弧までの本体（field と variant の列）・fn は本体の開き括弧の行まで（署名）・歯は閉じ括弧まで。write-set の中に宣言が在る名は出さない（base の要約が持つ）。write-set の外の 2 file 以上に宣言が在る名は所在の列の 1 行だけにする（同名の衝突で材料を膨らませない）。
     - (b) 名指された `.rs` の path（write-set の外）: base の要約と同じ 1 項目（`item_text` を開いて使う）。
     - (c) data file（名指された `.rs` でない tracked の file・write-set の中と契約の設計 doc 自身は除く）: path・行数・byte 数の 1 行と、拡張子ごとの鍵の列（`.json` は `"鍵":` の鍵・`.css` は選択子の `.class`・`.yaml` と `.yml` は `id:` の値と行頭の鍵・`.toml` は表の見出しと行頭の鍵・他の拡張子は鍵の列を持たない）と、鍵のうち本文に語の境界で現れるものごとに、その鍵を持つ最初の 1 行（前後の行は付けない）。file の全文は渡さない（fixture の byte で cap を食う型を作らない・memo `s2-07l.691`）。鍵の行だけにするのは、前後の固定の行数の窓（± N 行）が却下の「宣言の前後を固定の行数で切る」と同じく数の閾値を持ち込むからで、囲む object / block を構造で切る形は拡張子ごとの構文の読み手（json・css・yaml・toml の 4 本の手書きか、依存の追加〔A3〕）が要り、最上位の block は file の全文になって全文を渡さない規則と当たる。実例の data file の型（json の鍵・class・yaml の id）が問うのは在否と在りかで、鍵の行（値が 1 行ならその値ごと）で足りる。名指しが tracked の dir に解ければ、配下の file ごとに path・行数・byte 数の 1 行。
     - (d) crate の依存: write-set の各項目の最も近い祖先の Cargo.toml（write-set に無いもの）と名指された Cargo.toml は、鍵の列の代わりに依存の表（見出しが dependencies で終わる表）の見出しと行を出す。依存の表が無い manifest はその 1 行。
     - (e) 親 module の宣言: write-set の各 `.rs`（`+` を含む）から crate の根まで、write-set の外の親 file の mod の行の字面（可視性を含む）か「宣言なし」の 1 行。
     - (f) depends の相手の行: 契約表の行の depends の各 id の行の title と、その行の write-set の `+` の項目ごとの base に在る / 無い（着地の事実・台帳は読まない）。
  4. **cap は新しい閾値を作らない**: lens が既存 4 材料と base の段を足した後の残り（`gate.token_cap` の内側）で測り、塊が残りに収まらない名は切り詰めの 1 行（名と塊の byte 数）に置き換え、その 1 行も収まらない周は最後に落とした名の本数の 1 行を残す（黙って落とさない・C10）。既存 4 材料だけで越える周の極性（claude を呼ばず INCONCLUSIVE）と base の段の落とし方は不変＝rules 行も新しい値も足さない。
  5. **雛形の穴を 1 つ足す**: `lens-contract.txt` の末尾（`{base}` の後ろ）に穴を 1 つ足し、`lens.rs` が写しから埋める。写しが無い周は空文字＝雛形は 1 字も変わらない。既存の 1 走査の対に 1 つ足すだけで、埋めた本文の中の穴の字面は展開しない。
  6. **diff の審査の雛形（`lens.txt`）には渡さない**: gate の lens の INCONCLUSIVE は本 repo の 377 回のうち 11 回で、材料の欠けを理由にした回は 0（cap 5・他 6）。隣の project は 39 回で INCONCLUSIVE 0（実測 2026-09-27）。diff の審査は diff の byte で cap を使い切る周が既に在る（memo `s2-07l.691`）ので、材料を足すと INCONCLUSIVE を増やす側になる。
- 実例の見込み（deduced・隣の project の HEAD の木で模した・各便の base の木とは差がありうる・形 2 の構造の規則を足す前の照合で模した値で、規則の後は測り直していない）: 20 件のうち、本 § の材料で判定に届く見込みが 5 件（Cargo.toml の依存と yaml の id の行・json の鍵・stylesheet の class・別の便の fixture の在りかと鍵・別 crate の型の file）、一部だけ届く 9 件（型は在るが値の意味が doc に無い・同名の fn が多い・write-set の中の fn の可視性・未着地の型・名指されない json の鍵・fixture の値）、届かない 6 件（別の設計ノートの節・呼び手の不在・節そのものの欠け）。
- 素の語の雑音（実測 2026-09-28・verified・base = main 8f6072d の tracked の木・照合は形 2 の語の境界を同じ規則で模した）: 母集団は候補の名 7879（base の `.rs` の本体の区間の宣言の名と歯の区間の歯の名・うち `_` も 2 つ目の山も持たない素の 1 語が 1202）と、契約表の行 35 本（本 § と同じ波の新しい行 15 本と、main の契約表 310 行から seed 固定で抜いた 20 本・本文は節の本文と done）。
  - 素の語を全部照合すると（規則の前）: write-set の外に宣言の在る名は、新しい 15 行で 919（1 行 18〜108）・抜いた 20 行で 905（1 行 13〜85）。そのうち backtick の外の素の 1 語が 704 と 689（77% と 76%）で、多いのは `main` `base` `write` `rules` `boundary` `dir` `done` `gate` `land` `path` など散文の語である（35 行の延べの上位 10 語）。35 行のどれも材料が空にならない（歯 (g) の「名指しの無い契約は材料が空」は toy の木でしか成り立たない）。塊の byte の見積（宣言の塊を base から切った長さ）は 255,036 と 251,517。
  - 形 2 の規則の後: 215（1 行 3〜31）と 216（1 行 0〜27）、塊の byte の見積は 137,862 と 135,119。
  - path 形の連なりも同じ型: 連なりを全部照合すると、解けた file と dir の配下の file の行が 6,593 行と 7,925 行出る（大半は素の 1 語が tracked の dir の名に末尾一致で解けた分・`pipe` `rules` `src` など）。`/` か拡張子を持つ連なりに限ると 169 行と 472 行（dir に解ける連なりは 109 → 3・144 → 5）。
  - 取りこぼし（規則で落ちる側）: 審査の section-material-missing の `at` に並んだ識別子の形の字面は、本 repo 71 回で 78 個（`_` か 2 山 43・素の 1 語 35）。素の 1 語のうち材料の本文の backtick の中に無いのは 19 個（5 語）で、main 8f6072d の木に宣言の在る語は 2（done の欄を指す `at` の 14 個と、もう 1 語の 2 個）、残り 3 語は要件 id＝規則で落ちる宣言の名は 1 語。隣の project（2026-09-28 の実測で審査 121 回・section-material-missing 37 回）は 68 個（`_` か 2 山 21・素の 1 語 47）で、素の 1 語は 13 語（47 回）・隣の project の HEAD の木に宣言が 1 つだけ在る語は 5・2 つ以上の語は 5（規則の前でも形 3 (a) の所在の 1 行だけになる語）・0 の語は 3。落ちるのはこの 5 語の塊で、形 3 (b)〜(f)（path・data file・Cargo.toml・親 module・depends の相手）は素の 1 語の規則に依らない。
- 触らない: lens の観点 3 つ・理由の型の 6 語・判定の JSON・既存 4 材料と base の要約の形・`unresolved_names` の判定・焼き直しの門の物差し（`crates/scribe2/src/pipe/review/judgement.rs` の `unaddressed`）・列外の鍵（`design_material`）・diff の審査の雛形と極性・審査の rc。
- 歯（接頭辞 3 つ・どれも `crates/` 全体で `fn <接頭辞>` が 0 件・名の substring でも 0 件〔実測 main 8f6072d〕）:
  - `closure_names_mentioned_`（`crates/scribe2/src/pipe/closure/names.rs` の in-file）: backtick の中の名と、外の `_` を持つ名と山 2 つの CamelCase の名を拾い、外の素の 1 語は base に宣言が在っても拾わず、同じ語を backtick に入れると拾い、長い名の途中に在る短い名は拾わない・path の連なりは `/` か拡張子を持つものだけが解け（素の 1 語は tracked の dir の名と同じでも解けない）、等しいか末尾一致で解け、同じ末尾が 2 file に在れば両方を返し、`.rs` でない path も解ける。
  - `pipe_review_outside_`（行 bc の write-set の `+` の file の in-file と `crates/scribe2-boundary/tests/e2e/pipe/review.rs` の e2e）: fixture は隣の project の実例の型を toy の木に起こす — (a) 別 crate の山 2 つの CamelCase の struct を本文が backtick の外で名指すと、所在・可視性を含む宣言の行・field の列が出て、write-set の中の同名は出ず、2 file に在る名は所在の 1 行だけ (b) 名指された json・stylesheet・yaml が行数と byte 数と鍵の列を持ち、本文に在る鍵（json の鍵・class・id）を持つ行だけが出て前後の行は出ず、basename だけの fixture の名指しが一意に解ける (c) write-set の `.rs` の crate の Cargo.toml の依存の表の行が出て、Cargo.toml が write-set に在る周は出ない (d) 親 file の mod の行が可視性つきで出て、親に宣言の無い `+` の file は「宣言なし」 (e) depends の相手の行の title と `+` の項目の在否 (f) 残りが足りない周は名ごとの切り詰めの行、全く足りない周は落とした本数の 1 行（母集団 = fixture の名の本数を同じ assert で数える） (g) 名指しの無い契約（素の 1 語だけを持つ契約を含む）は材料が空。e2e は受付から審査まで通した run の材料の dir に外の材料の file が在り、§ が名指した write-set の外の struct の宣言の行を持つこと（名指しの無い既存の e2e の材料の dir の 4 本は不変＝既存の歯 `pipe_review_base_summary_file_names_every_write_set_item` が緑のまま）。
  - `headless_lens_outside_`（`crates/scribe2/src/headless/lens.rs` の in-file）: 写しが在れば穴が埋まり、無ければ雛形が 1 字も変わらず、本文の中の穴の字面は展開されず、残りは base の段を足した後で測る。
  - base で RED の理由: in-file の 3 接頭辞は base に歯が 0 本（nextest の該当 0 本で rc 4・機能不在）、e2e は材料の dir に外の材料の file が無く落ちる（機能不在）。
- 限界: 別の設計 doc・設計ノートの節と、state dir など repo の外の file の形は材料に入らない。値の意味（符号の向き・単位）は宣言の doc 行が書いていなければ渡らない。write-set の中の名の可視性は base の要約の列が持たない（§40 / §49 の歯の期待が動くので本行に入れない）。呼び手の在否（逆向きの参照）は渡らない。`#[path]` 属性で置いた module の親は解かない。backtick の外の素の 1 語は、base に宣言が 1 つだけ在っても拾わない（取りこぼしの実測は本 repo で 1 語・隣の project で 5 語）。data file の鍵の値が複数行に跨ぐ（入れ子の object・配列）ときは、鍵の行の先の値は渡らない。本行の着地より前に section-material-missing で止まった便は、焼き直しの門が節の本文の差しか測らないので、節を直さない限り再受付が断られる。
- 却下: backtick の中だけを読む（隣の project の契約表は backtick を 0 個しか持たず 20 件で拾える名が 0）／契約表の検査の名指しも素の語へ広げる（名指しの実在は解けない名を断りにする検査で、解けた名だけを材料に足す本行と極性が違う・素の語は偽の断りを生む）／名指された file の全文を渡す（fixture の byte で cap を食う・memo `s2-07l.691`）／lens に tool を渡して自分で読ませる（審査の前提「shell も cargo も撃てない」を壊す）／素の語を全部照合する（上の実測で拾う名の 76〜77% が散文の語・35 行のどれも材料が空にならない）／宣言の在る file の本数や名の長さの閾値で素の語を落とす（数の閾値が要る＝rules 行と裁定・構造の規則で足りる）／素の 1 語でも base に宣言が 1 つだけなら拾う（新しい 15 行で 265 名が足され、規則で残る 215 名より多い・大半は `done` `dir` `repo` `commit` など散文の語）／宣言の前後を固定の行数で切る（数の閾値が要る・宣言の閉じ括弧までの構造で足りる）／材料の大きさに rules 行を足す（既存の cap の残りで足りる・C5 の裁定を要らなくする）／台帳を読んで depends の相手を解く（審査が台帳の在否に依存する・同じ doc の depends の欄で足りる）／goal-done-contradiction（done が節に無い数〔窓の時間〕を持つ型・隣の project の便 1 件）を本行で塞ぐ（材料を束ねても節に無い数は現れない＝done の数の出所を測る別の門の型）。

## 52. 列で着地した便のうち push の先端でない便の終端は CI を待たない — 先端でない sha には CI の run が付かず、上限までの空回りが列の後続と着地の列を止める（契約表の行 bd・memo `s2-07l.688`）

- 何が起きているか（実測・verified）:
  - 本 repo（2026-09-27）: 4 本の便が並んで着地した周に、main に載った commit のうち 434cb5f → 653d576 → 8af5903 の 3 つについて、GitHub の CI は push の先端 8af5903 にだけ走った。653d576 の便の終端は CI の照合を上限（rules 行 `pipe.ci_wait_s` = 900 秒）まで待ち、`ci:unmeasurable` で終わって close しなかった（orchestrator が先端の CI と祖先の関係を測って手で close した）。
  - 非公開の隣の project（2026-09-27）: 4 本の列の着地の後、先端でない 3 本の終端がそれぞれ 900 秒待って `ci:unmeasurable` で終わった。その間、着地の窓（`pipe land-window`）は busy のままで、列の後ろの 6 本と設計の merge が止まった（止まる長さは先端でない 3 本 × 900 秒＝約 45 分・deduced）。
- 現物（main・verified）:
  - `crates/scribe2/src/pipe/land/finish.rs` の `land_train` は、列の便ごとに `commit-tree` で commit を連ねて main を CAS で先端まで進め、主実測を先端の木で 1 回撃った後、便ごとに `finish` を撃つ（後続 → 先頭の順）。`finish` は便ごとに `terminal` を撃ち、`terminal` は `git push <remote> main:main` の後に、その便の sha で CI の照合（`Completion::CiResult` の待ちと `ci_now`）を撃つ。
  - 終端 1 本の CI の行の呼び出しは、success の周で 2 回である: 待ちの完了条件 `Completion::CiResult` の評価（`crates/scribe2/src/fleet/wait.rs` の :159 が `ci_now` を撃つ・最初の評価は眠る前）の 1 回と、待ちの後に `terminal` が結果を読み直す `ci_now` の 1 回。e2e の偽 CI は呼ばれるたびに回数の file へ 1 行を足し、`ci_call_count`（`tests/e2e/pipe/land.rs` の :2548）がその行数を返す（`pipe_terminal_ci_poll_first_check_is_before_the_sleep` が同じ数え方で 2 回を測る）。
  - 最初に撃たれた終端の push が列の全部の commit を 1 回で出す。forge の CI（GitHub Actions の push の event）は push の先端の commit にだけ run を作るので、先端でない sha の照合は run を 1 本も見ず、上限まで待つ。
  - 同じ process が便ごとに順に終端を撃つので、先端でない便 1 本ごとに上限ぶん後続の終端が遅れ、列の便は終端まで終わらない（窓の列に残る）。
- 形（番号は done と 1:1）:
  1. **先端を知るのは `land_train` の 1 か所**: 列の最後の便（main を進めた先端の commit を持つ便）の外の便に「push の先端でない」を閉じた 2 値で運ぶ（`finish` から `terminal` へ・`Landing` の variant か引数かは実装が選ぶ）。単独の着地（`land` の経路）と `--terminal-only`（`pipe/cli/step.rs` の終端だけの再実行）は先端の側を渡す。
  2. **先端でない便の終端**: push は今どおり撃ち `terminal:push:<remote>` を記す（列の最初の終端が全部を出すので、2 本目以後の push は何も動かさない）。その後、CI の照合を 1 回も撃たず（待ちも `ci_now` も撃たない）、`terminal:ci:unmeasurable` を記して止まる。close はしない（FR50 の「CI の結果が success でなければ close せず失敗を記帳する」のまま）。stdout の `terminal=` は `ci:unmeasurable`。
  3. 変えないもの: 終端の 7 値・event の詞（`terminal:push:<remote>` / `terminal:ci:unmeasurable` の字面）・`ci_now` の判定・rules 行 `pipe.ci_wait_s` と `pipe.ci_poll_s` の値・列の記帳の順（後続 → 先頭）・先端の便の push → CI → close。
- 歯（`pipe_train_terminal_` 接頭辞・`crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs`・親の `tests/e2e/pipe/land.rs` の `fake_terminal` と列の helper を `use super::*` で使う・`grep -rn "fn pipe_train_terminal_" crates/` は 0 件・2026-09-28）: 偽 remote と常に success を返す偽 CI と偽 bd を宣言した repo で、3 本の列を先頭の land で着地させる。
  - 先端の便の `Landed` の後ろが `terminal:push:fake`・`terminal:ci:success`・`terminal:close:ok` の 3 件で、先端でない 2 本の `Landed` の後ろは `terminal:push:fake` と `terminal:ci:unmeasurable` の 2 件で close を持たない。
  - 偽 CI の呼び出しの回数（`ci_call_count`）が 2（先端の便の待ちの最初の 1 回と読み直しの 1 回だけ）、偽 remote の main が先端の sha を指す。
  - base では 3 本とも CI を照合して close する（偽 CI の呼び出し 6 回＝3 本 × 2 回）ので RED（機能不在）。
- 限界: 先端でない便は close されず、台帳の close は手のまま（先端の CI が success で、便の sha が先端の祖先であることを測ってから閉じる）。先端の CI の結果を先端でない便の close に使う形は、FR50 の改訂の後に §53（行 be）が持つ。
- 却下:
  - 先端の CI の結果で先端でない便も close する。当時の FR50 の照合の対象（着地 commit の CI）を変えるので、要件の改訂が先に要った（改訂の後の形は §53）。
  - 列の便ごとに自分の sha を先端として押し直す。便ごとに CI が走り、列の終端が CI の本数ぶん順に待つ（列で主実測を 1 回に畳んだ意味が減る）。
  - `pipe.ci_wait_s` を短くする。先端の便の CI も上限で打ち切られる（値は user の裁定）。
  - 最初の照合で run が無ければ待たない。forge の CLI は走っている run と run が無い周を同じ「未完了」で返す（`ci_now` の `None`）ので、push の直後の先端の便まで待たなくなる。

## 53. 列で着地した便のうち push の先端でない便は、自分の commit を祖先に持つ先端の commit の CI の結果で照合して close する（FR50・契約表の行 be・memo `s2-07l.688`）

- 何が起きているか（2026-09-28）:
  - verified: §52（行 bd）の着地の後、先端でない便は CI を照合せず `terminal:ci:unmeasurable` で止まり、close は手で撃たれている。非公開の隣の project で 2026-09-27T21:30Z 以後に 16 件あり、16 件とも「着地の commit は押した先端の祖先」を確かめてから手で閉じた（本 repo は 0 件）。
  - verified: FR50 は「候補の木に並べた順に着地し push の先端でない便は、自分の着地 commit を含む push の先端の commit の CI の結果で照合し、note に先端の commit id も持つ」を持つ（§52 の限界が求めた要件の改訂）。
- 現物（main・verified）: `crates/scribe2/src/pipe/land/finish.rs` の `PushTip` は `Tip` / `Behind` の 2 値で、`Behind` は先端の sha を持たない。先端を知るのは `land_train` の 1 か所（列の最後の便の外に `Landing::Behind` を作る周）。`--terminal-only`（`crates/scribe2/src/pipe/cli/step.rs`）は常に `Tip` を渡す。
- 形（番号は done と 1:1）:
  1. `PushTip::Behind` が先端の commit の sha を持つ。先端を知るのは今どおり `land_train` の 1 か所。
  2. 先端でない便の終端は、push の後に自分の sha が先端の祖先か（`git merge-base --is-ancestor <sha> <先端>`）を測る。祖先でない周と測れない周は今どおり CI を照合せず `terminal:ci:unmeasurable` で止まる（close しない・fail-closed）。祖先の周は、CI の照合（待ちと `ci_now`）を先端の sha で撃つ。
  3. success の周は close し、reason は `landed <sha> ci=success tip=<先端>`（note に先端の id を持つ・FR50）。failure と unmeasurable は今どおり close しない。
  4. 変えないもの: 終端の 7 値・event の詞・`ci_now` の判定・rules 行 `pipe.ci_wait_s` と `pipe.ci_poll_s` の値・列の記帳の順（後続 → 先頭）・先端の便と単独の着地の close の reason（`ci=success` で終わる）・`--terminal-only` の終端（`Tip` のまま）。
- 歯（`crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs`・親の `fake_terminal` と列の helper を使う・`grep -rn "pipe_train_tip_close" crates/` は 0 件・2026-09-28）:
  - 既存の `pipe_train_terminal_only_the_tip_checks_ci_and_closes` は期待が反転する。同じ PR で、接頭辞 `pipe_train_terminal_` のまま名と期待を書き直す（行 bd の verify 行を空にしない）: 常に success を返す偽 CI の 3 本の列の着地で、3 本とも `Landed` の後ろが `terminal:push:fake`・`terminal:ci:success`・`terminal:close:ok` の 3 件、偽 CI の呼び出しは 6 回で、最後に渡った sha が先端の sha。
  - `pipe_train_tip_close_`（1 本の fn の 2 周）: 1 周目は常に success の偽 CI の列の着地で、偽 bd の最後の close（列の先頭の便）の reason が `tip=<先端の sha>` を持つ。2 周目は偽 CI が failure を返す列の着地で、3 本とも close しない。
  - base では先端でない便が close せずに止まるので RED（機能不在）。
- 限界: `--terminal-only` で先端でない便の終端を手で撃ち直す周は、今どおり自分の sha で照合する（上限まで待って `ci:unmeasurable`）。手の撃ち直しも先端で照合するには、anchor の main を先端とする判定が要る（§58・行 bm が閉じる）。
- 却下:
  - 先端を先に終端させ、その結果を先端でない便へ写す。列の記帳の順（後続 → 先頭）を変え、終端の失敗の帰属が便ごとに取れなくなる。
  - 便ごとに自分の sha を先端として押し直す。便ごとに CI が走り、列の終端が CI の本数ぶん順に待つ。

## 54. 行の新しい歯の接頭辞が着地済みの行の verify の filter 語を含み、その行の歯の置き場を write-set の外へ広げる衝突を、契約表の検査が docs の時点で予想して出す（契約表の行 bf・memo `s2-07l.708`）

- 何が起きているか:
  - verified（1 件目・2026-09-27）: 行 bc は新しい子 module の in-file の歯に接頭辞 `pipe_review_outside_` を使った。この語は着地済みの行 c（`-p scribe2` の旗なし）と行 ah（`--lib`）の filter 語 `pipe_review_` を含む。そのため便の木では、c と ah の歯の置き場が行 bc の `+` の file まで広がり、現物の契約表の歯 2 本が `teeth-outside-write-set` で赤になった。直しは、c と ah の write-set に同じ file を `+` で先に宣言する docs の PR（#755）だった。
  - verified（2 件目・予測）: ledger-form 行 h の新しい接頭辞 `hook_ledger_graph_` は、着地済みの行 f の filter 語 `ledger_graph_`（`--test e2e`）を含む。行 h の e2e の歯の置き場 `crates/scribe2-boundary/tests/e2e/hook/guards.rs` は、行 f の write-set に無い。main 0c0dfc7 の写しで guards.rs にこの接頭辞の歯を 1 本足して `contracts check` を撃つと、findings が 1 件（`docs/design/ledger-form.md:219` の行 f）出る。足す前は 0 件。本 § と同じ docs の PR で、行 h の接頭辞を `hook_graph_guard_` に変えて外した。
  - verified: 起票の前の preflight と docs の PR の `contracts check` は、2 件ともに 0 件だった。新しい file も新しい名の歯も base に無いので、他の行の歯の置き場を base で解いても当たらない。
  - verified（器の読みを scratch の走査で模した値・母集団は main の first-parent の 624 commit〔2026-09-20 〜 0c0dfc7〕の表と木）: 本 § の形で当たる組（新しい語の行・語・当たる行）は 15 組だった。
    - 11 組は、新しい語の行が着地した commit で、当たる行の filter 語が新しい歯の file を write-set の外に解いた。11 組とも、行 aw（§45）が findings に上げる 2026-09-23 より前である。
    - 2 組は 1 件目、1 組は 2 件目である。
    - 残る 1 組は、着地の前に語が改名されたので判定できない。
    - 形 1 の本文の全体と形 3 の絞りを外し、形 4 を「候補のどれか 1 つで当たる」に換えると、45 組になる（棚卸しの報告の形に近い）。
    - binary で撃って一致を確かめたのは、0c0dfc7 の 1 組（2 件目）だけである。
  - deduced: 新しい語・置き場の候補・他の行の filter 語と write-set は、どれも docs の時点で契約表の字面と base の木から決まる。
- 現物（main 0c0dfc7・verified）:
  - `crates/scribe2/src/pipe/table/check.rs` の `judge_repo`（:584）は doc ごとに `judge_doc`（:649）を撃つ。置き場の検出線 `Places` の `measure`（:473）は、その doc の Declared 行（`is_declared`・:499＝`creates` / `tests` / `also` を持たず write-set を持つ行）ごとに `teeth_outside`（:505）を撃つ。
  - `teeth_outside` は verify の行ごとに `teeth_places`（`crates/scribe2/src/pipe/closure/derive.rs` の :163）を base の `.rs` の本文で解き、write-set の外の file を集める。解けない行（base で 0 本の語＝`TeethPlaceUnresolved`）は数えない。
  - `teeth_places` は、nextest の行の最後の filter 語だけを読む（`nextest_read`・:245）。そのうえで、行の scope（`in_scope`・:305）の file の歯の区間を見て、`#[test]` の直下の fn の名が語を含む file を返す。歯の区間は `crates/scribe2/src/pipe/closure.rs` の `test_region`（:349）が決める。tests の dir の下は file の全体で、src は行頭の `#[cfg(test)]` から末尾までである。
  - このため新しい語は、どちらの側からも見えない。新しい語の行の側は解けないので数えない。他の行の側は、新しい file も新しい名の歯も base に無いので当たらない。
  - `#[path]` で子 module に置いた歯の file は、歯の区間の印を持たない。`crates/scribe2/src/hook/host_guard_tests.rs` は `#[cfg(test)]` の行を持たない（親の `crates/scribe2/src/hook/host_guard.rs` の :906〜:908 が `#[cfg(test)]`・`#[path]`・`mod tests;` を持つ）。そのため、この file に在る `host_guard_kind_` の歯 12 本は `teeth_places` に見えない。
  - 当たった行は `TableError` の `TeethOutsideWriteSet`（`crates/scribe2/src/pipe/table.rs` の :345・行番号・行 id・file）の 1 件になる。`--verbose` の周だけ、`place_notices`（:438）が `contracts place-out: <doc> 行 <id> の歯の file が write-set の外: <file>` の 1 行を出す。
  - 大きさ: check.rs は 1357 行（幅で畳んで 1389）で、R-C4-2 の上限 1500 までの余地は 111 行しかない。e2e の `crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` には、置き場の検出線の歯（`contract_check_place_verbose_names_the_outside_row_only_with_the_flag`・:320）と toy の repo の helper（`derive_repo_with`・`declared_teeth_row`・`ceiling_rules`）が在る。
- 形（番号は done と 1:1）:
  1. **対象の行と新しい語**:
     - 対象の行は、write-set の `+` の項目か `creates` のうち、base の tracked に無い path を 1 つ以上持つ行（着地の前の行）とする。Declared か導出の行かは問わない。
     - 新しい語は、その行の verify の nextest 行ごとに `teeth_places` と同じく**最後の filter 語 1 つ**（1 行に filter 語が 2 つ以上在る行も最後の語だけ・`teeth_words` の先頭の語ではない）を読み、それが次の 2 つを満たすときの語とする。1 行の判定は語 1 つの判定であり、行の中の他の語の在否で変えない。
       - 識別子の形である。
       - その行の scope の base の `.rs` の本文の**全体**で、`#[test]` の直下の fn の名に語を含むものが 0 本である。
     - 本文の全体で数えるのは、`#[path]` の子 module に置いた歯の file の既存の歯を、新しい語と読まないためである。
     - 読みは `teeth_places` を、各 file の本文の前に歯の区間の印を置いた写しで撃つ（読み手を増やさない）。
  2. **置き場の候補**:
     - 行の `tests` 欄が在れば、その項目を候補とする。
     - `tests` 欄が無ければ、write-set（`-` の項目を除く・印は剥がす）と `creates` の `.rs` のうち、歯を置ける file を候補とする。歯を置ける file とは、base に無い新規 file か、base の歯の区間が空でない file である（`declared_teeth` が置き場と読む下界と同じ弁別）。
     - そのうち語の verify 行の scope に入るものを残す。scope の判定は、各候補に語の名の歯を 1 本だけ置いた仮の本文を `teeth_places` に渡して解く（scope の読み手を増やさない）。
  3. **module の path の語で絞る**: `tests` 欄でない候補は、次の規則で絞る。repo の歯の接頭辞は module の path の語で始める慣例に合わせる（`pipe_review_outside_` は `crates/scribe2/src/pipe/review/outside.rs`、`closure_names_mentioned_` は `crates/scribe2/src/pipe/closure/names.rs`）。
     - module の path は、`crates/<crate>/src/` か `crates/<crate>/tests/` より後ろの段とする。末尾の `.rs` は落とし、`mod` / `lib` / `main` の段は数えない。
     - その path の末尾の段の連なりを `_` で繋いだ字面が、語と等しいか、語の先頭に `_` の境で一致する候補を探す。
     - そういう候補が在れば、一致した段の数が最も多い file だけを残す。1 つも無ければ、候補の全部を残す。
  4. **当たり（候補の全部で当たる周だけ）**:
     - 他の Declared 行（doc を跨ぐ・自分の行は除く）ごとに、残った候補を 1 つずつ測る。その候補に語の名の歯 1 本だけを持つ仮の本文を `teeth_outside` に渡す。
     - 全部の候補がその行の write-set の外に解けた周だけを、当たりとする（どこに置いても当たる衝突だけを出す）。
     - 候補が 0 本の語は数えない。
  5. **出し方**:
     - 当たった行は、既存の `teeth-outside-write-set` の 1 件になる（その行の見出しの行番号・行 id・候補の file）。判定行の `place-out=` にも 1 行として数え、findings の件数と欄の行数の一致は保つ。同じ行が base の歯でも外に解ける周や、2 つの語から当たる周は、1 件に file を併せる。
     - `--verbose` の知らせの行の末尾に、予想の出所（新しい語の行の doc・行 id・語）と直し方を足す。直し方は次のどちらかである。
       - 当たった行の write-set に file を足す（base に無い file は `+` を付ける）。
       - 語を、当たった filter 語を含まない語に変える。
     - 当たりの無い repo の出力は 1 字も変えない。`TableError` に variant は足さない（TableError と RuleKind の閉包を広げない）。
  6. **置き場と変えないもの**:
     - 予想を組む 1 本は、行 bf の write-set の `+` の file（`check.rs` の子 module）に置く。`judge_repo` が全 doc の行から 1 回だけ組み、置き場の検出線に渡す。`check_repo` と `repo_findings` は同じ `judge_repo` を通るので、両方に効く。
     - 変えないもの: `teeth_places` と `teeth_outside` の本体・`declared_teeth`・受付と preflight（`check_table` の 1 行の検査は置き場を撃たない）・判定行の字面・`TABLE_ERRORS` の pin・契約表の schema・rules 行。
- 歯（接頭辞 `contracts_prefix_collision_`・e2e の `crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` に 2 本）:
  - 接頭辞の実測（2026-09-28）: `grep -rn "fn contracts_prefix_collision_" crates/` は 0 件。全 doc の verify の filter 語のうち、この接頭辞の名の部分に含まれる語も 0 件。置き場の file は行 bf の write-set と verify 行の scope（`--test e2e`）の中に在る。
  - 2 本とも toy の repo（`derive_repo_with` に file を足す）で `contracts check` を撃ち、旗の有無の 2 周で出力の全行を測る。
  - (a) crate `toy` の src/dial.rs（歯の区間に `dial_ok`）を置く。新しい語 `dial_knob_` の行は write-set に `+` の src/dial/knob.rs と既存の src/dial.rs と src/other.rs を持ち、候補は module の path の語で knob.rs に絞られる。
    - 当たる: filter `dial_`（`--lib`）で write-set が src/dial.rs だけの行が、knob.rs を持つ 1 件になる。
    - 当たらない: 同じ `+` を write-set に持つ行・scope が `--test e2e` の行・`+` の file を持たない行の新しい語。
    - `--verbose` の知らせの行は、予想の出所（doc・行 id・語）と直し方を持つ。新しい語の行は別の doc に置き、doc を跨いで当たることも測る。
  - (b) 一致の無い語と、読まない語を測る。
    - module の path の語に一致しない語 `dial_turn_`（候補は `+` の src/wheel.rs と src/other.rs）は、候補の 2 つとも外に解ける行だけに当たる。候補の 1 つを write-set に持つ行は当たらない。
    - 歯の区間の印を持たない file（`#[path]` の型）に在る名を語にした行は、新しい語と読まない。この行の候補は、新しい語と読めば当たる形に置く。
    - `tests` 欄を持つ導出の行は、`tests` の file を置き場とする。
  - 負の側はどれも、その条件を外す変異で当たりが増える形に置き、当たらない行の本数を同じ assert で数える。
  - base で RED の理由: base は予想を持たないので、2 本とも findings=0 の判定行だけが出る（機能不在）。
  - 現物の契約表の歯 2 本（`contract_closure_ext_real_table_has_zero_findings` / `contract_names_declared_real_table_has_zero_findings`）は、本行を足した表で緑のままである（本 § の形を模した走査で、行 h の接頭辞を直した後の表は 0 組・deduced）。
- 限界:
  - 着地の前かどうかは、`+` の項目か `creates` の base に無い path で読む。新しい file を持たない行（既存の file だけに歯を足す行）の新しい語は予想しない。
  - 置き場は予想である。module の path の語で絞れない語は候補の全部で当たる周だけを出すので、候補の一部だけが外に在る衝突は今と同じく gate の現物の契約表の歯まで残る。
  - verify の 1 行に filter 語を 2 つ以上並べた行は、最後の語だけを読む（`teeth_places` と同じ・nextest は全部の語で当てる）。
  - 識別子の形でない語（`::` を含む module path の filter）は数えない。`--exact` の行は、等しい名だけが当たる。
  - 閉包の形（新しい file の本文が他の行の `touches` の型を match する・memo の notes の 2 件目と 3 件目）は file の本文で決まるので、docs の時点の字面では拾えない。
- 却下:
  - 設計の書き方の注意として散文で残す。規律を散文で渡す形で、N2 に当たる。
  - 絞らずに、候補のどれか 1 つで当たれば出す。模した走査で 45 組（本 § の形は 15 組）になり、歯を置かない file で当てる。例は vessel-hook 行 j の 2 組で、`host_guard_kind_` の 2 語が行 b に当たり、`rules_embedded_manifest_declares_publish_guard_` が carry-prep 行 j に当たる。
  - 語が `_` で終わるなら `+` の file だけに置く。行 bc の `closure_names_mentioned_` が、行 ac（filter `closure_names_`）に偽で当たる。実際の歯は、行 ac の write-set の中の `crates/scribe2/src/pipe/closure/names.rs` に置かれた。
  - 歯の区間だけで新しい語を数える。`#[path]` の歯の file の既存の歯を新しい語と読む。例は vessel-hook 行 j の `host_guard_kind_each_word_kind_names_its_own_row` で、この歯は host_guard_tests.rs の :123 に在る。模した走査では、pipeline 行 am の 3 語の偽の当たりが増える。
  - 仮の名を base の本文の列に重ね、`teeth_outside` を 1 回だけ撃つ。base の歯と混ざるので、どの行のどの語が当てたかを名指せず、候補の全部で当たるかも測れない。
  - `TableError` に variant を足す。直す手は既存の語と同じ（当たった行の write-set か、語）なので、言い分ける意味が無い。語の一覧の pin も動く。
  - 着地済みかを台帳で判じる。契約表の検査が台帳の在否に依存する。
  - 受付と preflight で撃つ。衝突の相手は着地済みの行で、直すには docs の PR でどちらかの行を書き換える。docs の PR の CI（現物の契約表の歯）で止めるのが最も早い。

## 55. 審査の材料に、契約の節と done が指す別の設計の § の本文を束ね、base の要約の行数に式を名乗らせる（契約表の行 bg・memo `s2-07l.710` / `s2-07l.728`）

- 何が起きているか:
  - verified（本 repo の置き場の審査の記録）: 行 bc（§51）の着地（f2ab774・2026-09-27T15:59Z）から 2026-09-28T04:18Z までに審査は 36 回あり、PASS でない周は 14 回、そのうち `section-material-missing` は 8 回だった。8 回のうち 2 回は同じ型である。契約の節と done が別の § を `<doc>.md §N`・`<doc>.md 行 <id>`・`行 <id>` の形で指すだけで、指した先の本文も、そこに書かれた名も材料に無い（memo `s2-07l.710`）。
    - 便 `s2-07l.703`（seat-heartbeat.md §16 行 t・16:07Z・INCONCLUSIVE）: lens の evidence は 2 つ。「dispatcher.md §26 の形 1 / 形 2 と行 w の done が材料に無い」と「mod facts の可視性を判じられない」。done は呼ぶ関数を「dispatcher.md §26 の 1 関数（行 w が pub(crate) で開く事実と字面の関数）」とだけ書き、識別子で名指さない。材料の outside.txt は dispatcher.md を「行数 752 / byte 220200」の 1 行（§51 形 3 (c) の data file の行）でだけ持つ。
    - 便 `s2-07l.700`（vessel-hook.md §14 行 h・18:02Z・INCONCLUSIVE）: `at` は live_row.rs・pipeline.md §57・行 az・歯の接頭辞の 4 つである。evidence は、行 i の読み手の関数名と可視性、行 az の判定と印の書き手が材料に無いことを挙げる。行 i は同じ doc の §15 の行、行 az は pipeline.md §57 の行である。outside.txt は pipeline.md を「行数 1499 / byte 496004」の 1 行でだけ持ち、§57 が書く名（stale・write_mark・git_segments）は 0 件だった。
    - 2 便とも、指した先の事実を自分の § に写す docs の直し（9bbaa82〔#756〕・1079f35）の後、2 回目の審査は PASS だった。§51 の限界の「別の設計 doc の節は材料に入らない」が、設計どおりの穴として出た。残りの 6 回（write-set の外の関数の可視性・節が決めていない経路・歯の欠け）は、別の § を指す型ではない。
  - verified（code）と memo の記録: 審査の材料で、base.txt の行数は幅で畳んだ数、outside.txt の data file と dir の配下の file の行数は改行で数えた生の行である。2 つが同じ「行数」の字で並ぶ（下の現物）。非公開の隣の project の審査の 1 回（2026-09-28）では、同じ file が base.txt で「行数 全体 989 / 本体 989」、outside.txt で「行数 985」と並び、lens がそれを根拠の 1 つにして INCONCLUSIVE を返した（memo `s2-07l.728`・席間の知らせの記録）。本 repo の置き場で evidence に「行数」を含む審査は 5 回あり、食い違いを根拠にした回は 0 回だった。
- 現物（main 0c0dfc7・verified）:
  - 組み手: `crates/scribe2/src/pipe/review.rs` の `materials`（:346）が、節の本文・done・約束の行の text を本文の列にする。節の本文は `design_text`（:421）が作り、先頭に `<doc>#<id> §N` の 1 行を持つ。`crates/scribe2/src/pipe/review/outside.rs` の `outside_text`（:97）から呼ぶ `bundle`（:107）が、塊を (f) → (d) → (e) → (a) → (b) → (c) の順に並べる。本文の中の `dispatcher.md` のような字は、§51 形 2 の path の連なりとして tracked の file に解ける。だが (c) の data file の 1 行（`.md` は鍵の列を持たない）になるだけで、§ の本文は読まれない。
  - 節の読み手: review.rs の私有の `section_text`（:447）は、`## N.` の見出しの次の行から次の `## ` の前までを読む（契約表の区間と fence の中の `## ` は数えない）。review.rs の本体で呼ぶのは `design_text` だけである。Rust の私有の item は、定義した module とその子孫から見える。孫の module から親の親の私有の関数を呼ぶ最小の crate を、2026-09-28 に rustc の edition 2021 で build して確かめた。したがって `outside` の子 module から、可視性を変えずに同じ 1 本を呼べる。行 id を行に解く読み手は `crates/scribe2/src/pipe/table/parse.rs` の `find_row` で、outside.rs の `depends_chunks`（:145）が同じ設計 doc の表で使っている。
  - 名の照合は `crates/scribe2/src/pipe/closure/names.rs` の `mentioned_names`（:78）の 1 本の口である。本文の列を受けて `Mentioned`（名・file・dir）を返す。
  - cap: `crates/scribe2/src/headless/lens.rs` の `prompt_of`（:222）が、既存 4 材料と base の段を足した後の残りを outside.rs の `outside_block`（:460）に渡す。`outside_block` は行頭の `- ` で始まる塊ごとに収め、収まらない塊は切り詰めの 1 行にする。それも収まらなければ、落とした本数の 1 行を残す（§51 形 4）。`gate.token_cap` は 150,000 byte。2026-09-27 の 2 便で、既存の外の材料を置いた後の残りは約 101,000 byte（行 h）と約 77,000 byte（行 t）だった（deduced・材料の file の byte から見積もった）。
  - 材料の鍵: 先撃ちの使い回し（dispatcher.md §27 行 aa / ac）の鍵は、材料の dir の全 file の名と本文の digest である（`crates/scribe2/src/pipe/dispatch/prelens.rs` の `digest`・:295）。先撃ち（同 file の `stage`・:270）も Reviewed の段も、review.rs の同じ `stage` で材料を組む。列外の鍵（review.rs の `design_material`・:415）は design.txt だけを見る。焼き直しの門で `section-material-missing` を測る物差し（`crates/scribe2/src/pipe/review/judgement.rs` の `section_unaddressed`・:156）も、design.txt の差だけを測る。
  - 行数の 2 つの式（.728）: `crates/scribe2/src/pipe/review/base.rs` の `item_text`（:59）は、`crates/scribe2/src/pipe/declaration/write_set.rs` の `FileLines`（`of`・:229）が数えた値を「行数 全体 N / 本体 M」（:73）と書く。この値は幅（rules 行 `R-C4.line-width` = 120）で畳んだ数である。1 行を「字数 ÷ 幅」の切り上げ（最小 1）と数える `weighted_lines` の式で、受付の上限の余地と xtask の file-lines も同じ式を使う。outside.rs の `file_chunk`（:352）と `dir_chunk`（:450）は、改行で数えた生の行（`str` の `lines` の本数）を同じ「行数」の字で書く。外の材料の名指された `.rs`（§51 形 3 (b)）は `item_text` を通るので、畳んだ数になる。説明の 1 行（base.rs の `PREAMBLE`・:42 と outside.rs の `PREAMBLE`・:27）は、どちらも式を書かない。
- 形（番号は done と 1:1）:
  1. **参照を拾う形は閉じた 5 つ**: 本文（§51 形 2 と同じ列＝節の本文〔導出物の行は goal〕・done・約束の行の text）から、次の形（参照の形 (A)〜(E)）を本文に現れた順に拾う。
     - (A) `<doc>.md §N`（リンクの字の中の `[<doc>.md §N](…)` を含む）
     - (B) `[…](<path>.md) §N`（行き先が `.md` のリンクの直後の §N・行き先の `#` から後ろは捨てる）
     - (C) `<doc>.md#<id>`（設計 pointer の形。材料の節の本文の先頭の 1 行もこの形）
     - (D) `<doc>.md 行 <id>` と `<doc>.md の行 <id>`
     - (E) doc の字を前に持たない同じ doc の `§N` と `行 <id>`

     `N` は数字の列である。`§` の直前（空白 1 つまでを飛ばす）が `)` なら、先に (B) の行き先を見る。直後が `.` と数字の §N（`§2.8` のような小節の形）は拾わない。それ以外で、`§` の直前（空白 1 つまでを飛ばす）にある path の字（英数字と `_ . / - #`）の連なりが、`.md` でも `.md#<id>` でも終わらず英数字を持つとき（`ADR-0045 §2` のような別の種類の文書）は、参照と読まない。`<id>` は英小文字で始まり、英小文字・数字・`-` が続く連なりである。`<doc>` は basename で、契約の設計 doc と同じ dir の tracked の file に解く。
  2. **§ に解く読み手は既存の 2 本**: `行 <id>` と `#<id>` は、その doc の契約表の行（`find_row`）の `section` で § に解く。§ の本文は、行 bg の write-set の `+` の file（`outside` の子 module）から review.rs の私有の `section_text` を呼んで読む（2 本目の節の読み手を書かない・可視性を変えない）。契約の自分の §（設計 doc が同じで、行の `section` と同じ番号）に解けた参照は捨てる。同じ § に解けた参照は 1 塊に畳む。契約の `design` が設計 pointer でない周は参照を拾わない（設計の材料が理由の 1 行を既に持つ）。
  3. **塊の形**（§51 形 3 の (a)〜(f) に続けて (g)〜(i) と呼ぶ）:
     - (h) § の塊: 解けた § ごとに 1 塊。頭の行は `- <doc の path> §N`、続きの行は § の本文の各行を 2 字下げたもの（本文の行頭の `- ` を塊の頭と読ませない）。行を指した参照（(C)・(D)・(E) の `行 <id>`）があれば、塊の末尾に行ごとに `  行 <id> の done: <done>` を 1 行足す。
     - (g) 解けない参照の塊: doc が tracked に無い・§ が無いか空・行 id が表に無い・表を読めない参照を、1 塊 `- 解けない参照: <字面>, …` にまとめる。字面は正規化した形（`<doc>.md §N`・`<doc>.md 行 <id>`・`§N`・`行 <id>`）で、本文に現れた順に重複なく並べる（黙って落とさない・C10）。
  4. **置き場は既存の塊の後ろで、file は 1 本のまま**: 塊は outside.txt の既存の (f) → (d) → (e) → (a) → (b) → (c) の後ろへ、(g) → (h)（指された順）の順で足す。大きい塊を後ろに置くのは、§51 の「小さい構造の材料が先」に合わせるためである。材料の file は 5 本のまま（`crates/scribe2-boundary/tests/e2e/pipe/review.rs` の :230 の列は変わらない）。参照を持たない契約と、参照が全部自分の § と行に解ける契約（材料の節の本文の先頭の `<doc>#<id> §N` はこれに当たる）の外の材料は、1 字も変わらない。名指しも参照も無い契約は、今どおり外の材料の file を置かない。
  5. **名は 1 段だけ**: (h) の塊の本文（§ の本文と done の行）を、既存の `mentioned_names` に 1 回渡す（照合の規則は §51 形 2 のまま・同じ 1 本の口）。契約の本文が既に名指した名・file・dir を除いた残りを、(i) として (h) の後ろに §51 形 3 の (a) → (b) → (c) の形で足す。(h) の本文の中の § の参照は辿らない（再帰しない）。
  6. **cap は新しい閾値を作らない**: lens の残りの測り方と `outside_block` は変えない。(g)〜(i) の塊も、収まらなければ塊ごとの切り詰めの 1 行（(h) の頭は `- <doc の path> §N`）になり、それも収まらなければ落とした本数の 1 行に数える（§51 形 4 のまま・rules 行を足さない）。
  7. **外の材料の説明の 1 行**: outside.rs の `PREAMBLE` に 2 つを書く。(g)〜(i) が既存の塊の後ろに並ぶこと（§ の塊は見出しの次の行から次の見出しの前まで・行を指せばその行の done つき）。data file と dir の配下の file の行数は改行で数えた生の行（wc -l と同じ）、`.rs` の要約の行数は base の要約と同じ幅で畳んだ数であること。
  8. **base の要約の行数が式を名乗る（.728）**: `item_text` の見出しは、幅で畳んだ全体の数と改行で数えた生の行の数が違う file に限り、全体の数の直後に `（幅 <W> で畳んだ数・生の行 <n>）` を添える（例: `行数 全体 989（幅 120 で畳んだ数・生の行 985）/ 本体 989`）。本体の数は畳んだ数のまま。2 つが等しい file の見出しは 1 字も変わらない。base.rs の `PREAMBLE` には、行数は 1 行を「字数 ÷ 幅（rules 行 `R-C4.line-width`）」の切り上げ（最小 1）と数えた数（受付の上限の余地と同じ式）であることと、生の行と違う file だけ生の行を括弧で添えることを書く。外の材料の名指された `.rs` の要約（§51 形 3 (b)）も同じ `item_text` を通るので、同じ見出しになる。
  9. 変えないもの: 材料の file の数と名・既存の塊の中身と並び・`outside_block` と `base_block`・lens の雛形と穴・diff の審査・design.txt（列外の鍵と焼き直しの門の物差しの入力）・`mentioned_names` と `section_text` の本文と可視性・`FileLines` と受付の上限の余地の式・data file と dir の行数の式・審査の観点と理由の型と rc。
- 見込み（deduced・今の木〔main 0c0dfc7 と同じ波の docs〕の設計 doc 26 本・契約表の行 332 本で、形 1〜5 の規則を Python で模した。各便の base の木とは差がありうる）:
  - § に解ける参照を 1 つ以上持つ行は 296（89%）で、1 行の (h) の塊は中央値 3・最大 22。束ねる § の本文の byte は中央値 20,282・p90 66,990・最大 187,737（gate-cost.md の 5 行・19 §）で、100,000 を越える行は 10。同じ doc の § だけなら中央値 15,566・p90 47,608。別の doc の § は 146 行が持ち、p90 は 27,301。
  - (i) で足される名（契約の本文が名指さず、束ねた § の本文だけが名指す base の宣言の名）は、中央値 34・p90 102・最大 222。backtick の中だけに限っても中央値 33・p90 100・最大 222 で、ほぼ変わらない。
  - 解けない参照を持つ行は 103（計 174 件）。多いのは散文の「行 id」と、前の文の doc を受ける `§N`（同じ doc の無い節に解ける）である。
  - 2026-09-27 の 2 便を今の木で模すと、行 h（.700）は pipeline.md §57（行 az の done つき）・vessel-hook.md §10 / §15（行 i の done つき）・contract-source.md §30 の 4 塊で計 52,747 byte、行 t（.703）は dispatcher.md §5 / §26（行 w の done つき）・seat-heartbeat.md §2 / §3 / §10 / §17 の 6 塊で計 67,005 byte になる。どちらも上の残り（約 101,000・約 77,000 byte）に収まる。lens が欠けていると名指した「行 w の done」「§26 の形 1 / 形 2」「行 az の判定と書き手」「行 i の読み手」は、どれも束ねた § と done の行に入る。
- 歯（接頭辞 2 つ。どちらも `crates/` 全体で `fn <接頭辞>` が 0 件。契約表の nextest の verify 行 874 本の filter 語 644 語のどれも、この接頭辞で始まる名と、その module の path を含む名の substring にならない〔実測 2026-09-28〕）:
  - `linked_section_material_`（`crates/scribe2/src/pipe/review/outside.rs` の既存の歯の区間に 4 本）: 既存の helper（`scratch`・`text_of`・`chunk_of`）で `bundle` を通す。行 bg の write-set の `+` の file には歯を置かない（flip-check は新しい src file の in-file の歯を base へ写さない）。fixture は toy の木に設計 doc を 2 本足す（自分の doc に § 4 つと行 2 本、別の doc に § 2 つと行 1 本）。
    - (a) 5 形の全部を持つ本文で、(h) の塊が指された順に 4 本並ぶ（母集団 = 塊の頭の数を同じ assert で数える）。塊の本文は見出しの次の行から次の見出しの前までを 2 字下げた行で、行を指した塊の末尾には行ごとに `  行 <id> の done: …` が 1 行ある。自分の § の塊は無い。同じ § を 2 つの形で指しても 1 塊である。`ADR-0001 §4` と `§4.1` だけが指す § 4 は、塊にも解けない参照にもならない。材料の節の本文の先頭の 1 行と自分の § だけを指す本文は、(g) も (h) も作らない。
    - (b) tracked に無い doc の §・無い §・表に無い行 id（同じ doc と別の doc）を指す本文で、(g) の塊が 1 本だけある。正規化した字面を本文に現れた順に重複なく並べ、既存の塊の後ろ・(h) の前に置かれる。
    - (c) 1 段: 自分の本文が参照としては同じ doc の §2 だけを持って `ZqTwin` を名指し、§2 の本文が backtick で `ZqShape` と `ZqTwin` と別の doc の §2 を名指す周を測る。`ZqShape` の宣言の塊は (h) の後ろにある。自分の本文も名指す `ZqTwin` の塊は既存の位置に 1 本だけある。別の doc の §2 の塊と、その本文だけが名指す名の塊は無い（再帰しない）。
    - (d) cap: 本文の長い § を束ねた写しを `outside_block` に渡す。残りが足りない周は `- <doc の path> §N: 切り詰めた（塊 … byte が cap の残りに収まらない）` の 1 行になり、小さい (h) の塊と (g) は残る。説明の 1 行が (g)〜(i) の並びを名乗る。
  - `review_base_lines_`（`crates/scribe2/src/pipe/review/base.rs` の既存の歯の区間に 1 本）: 幅 120 を越える 250 字の行を持つ `.rs` と、越えない `.rs` の 2 項目を要約する。前者の見出しは `行数 全体 <畳んだ数>（幅 120 で畳んだ数・生の行 <生の行>）/ 本体 <畳んだ数>` の字面で、後者は `行数 全体 35 / 本体 23` のまま（母集団 = 2 項目の見出しを同じ assert で数える）。`base_block` の説明の 1 行は畳む式と括弧の意味を持ち、`outside_block` の説明の 1 行は生の行を名乗る。
  - base で RED の理由: base の `bundle` は (g) も (h) も (i) の位置の塊も作らず、base の `item_text` の見出しは括弧を持たない。5 本とも assert が落ちる（機能不在。歯は base に在る関数〔`bundle`・`outside_block`・`base_block` と base.rs の `summary`〕だけを呼ぶので、overlay の上で compile は通る）。
  - 既存の歯は期待を変えない: `pipe_review_outside_`（in-file 7 本と e2e 1 本）・`pipe_review_base_`（in-file 8 本と e2e 2 本）・`headless_lens_outside_`（2 本）がそのままで緑。`pipe_prelens_` の e2e の `行数 全体 0 / ` と `行数 全体 2 / ` の部分一致も同じ。どの fixture も幅 120 を越える行と参照を持たないので、見出しも外の材料も変わらない。e2e は足さない（材料の file の数と置き方が変わらず、`materials` から `outside_text` への道は `bundle` の前で不変）。
- 限界（§51 の限界の更新）: 本行で「別の設計 doc の節」は材料に入る。残るのは次のとおり。
  - 拾うのは閉じた 5 形だけである。`§` と `行` を使わない指し方（「上の節」「その行」）と、doc を `.md` なしで書く形（`gate-cost §26`）は拾わない。「同 §26」は同じ doc の §26 と読むので、前の文の doc を受ける字面は、同じ doc の別の § に解けるか、解けない参照の 1 行に載る。
  - doc の字を前に持たない `行 <id>` は、同じ doc の行と読む。別の doc の行を doc の字なしで指し、同じ doc に同じ id の行が在れば、同じ doc の行の § を束ねる（材料が増えるだけで、判定は lens が持つ）。
  - 設計ノート（folio2 の YAML）・ADR・SRS の節と、`## N.` の見出しを持たない doc の節は束ねない（ADR の `§N` は参照と読まない。要件は既存の要件の材料が持つ）。
  - 束ねた § の中の参照は辿らない（1 段）。(i) の名も §51 形 2 の規則のままで、backtick の外の素の 1 語は拾わない。
  - cap は既存の残りのままである。束ねる本文が残りを越える行（見込みで 332 行のうち 100,000 byte を越える 10 行）は、後ろの塊から切り詰めの 1 行になる。§ の並びは指された順で、重みづけはしない。
  - 焼き直しの門と列外の鍵は design.txt だけを測る。指した先の § だけを直して受付し直す周は、自分の § と契約の字が同じなので断られる（FR49 / FR68 の同じ中身の判定）。直しは今どおり、自分の § にも要点を 1 項足す形になる（.700 / .703 の直しと同じ）。
  - 材料の鍵（dispatcher.md §27 行 aa / ac）: (g)〜(i) は外の材料の file に入るので、指した先の doc の § を変える commit でも先撃ちの鍵が動き、判定の使い回しが外れる。撃ち直しが 1 回増えるだけで、偽の使い回しにはならない。本行の着地の直後は、参照を持つ行の鍵が一斉に変わる。
  - .728: 生の行を並べるのは base の要約の全体の数だけで、本体の数は畳んだ数のままである。data file と dir の行数は生の行のまま（wc -l と同じ）。
- 却下:
  - 契約表の検査が、「§N の 1 関数」のような識別子なしの指し方を finding にする（memo の候補 2）。名指しを強いる門で、指した先の形や done の本文の欠けは埋まらない。「識別子なし」は散文の判定で、数の閾値か偽の断りが要る。
  - 設計を書く側の注意として残す（memo の候補 3・N2）。
  - 別の doc の全文を渡す。pipeline.md 1 本で 496,004 byte あり、cap の 150,000 を越える。
  - 束ねた § の中の参照を辿る（再帰）。2 便とも 1 段で届く見込みで、辿ると広がりに上限が無く、大きい塊で cap を使い切る。
  - 別の doc だけを束ね、同じ doc の参照を除く。.700 の lens が求めた材料の 1 つは同じ doc の §15（行 i）で、除くと 2 件のうち 1 件に届かない。
  - (h) を depends の相手の行の直後（既存の塊の前）に置く。参照を持つ契約で既存の塊の並びと cap の配分が変わり、大きい塊が先に残りを食う（§51 の「小さい構造の材料を先」と逆）。
  - 新しい材料の file を足す。lens の雛形の穴・cap の配分・`keep` の置き方・材料の dir の本数の歯（e2e の :230）が動く。外の材料の file の塊として足せば、どれも変わらない。
  - backtick の中だけを抜いて名の照合に渡す。見込みで足される名はほぼ同じ（中央値 33 と 34・p90 100 と 102）なのに、抜くには names.rs の `backticked` の 2 本目を書くか、その可視性を広げる（write-set が 2 file 増える）。
  - `section_text` の可視性を広げる・節の読み手をもう 1 本書く。私有のまま子孫の module から呼べる（C2）。
  - 束ねる § に新しい上限（本数・byte）を足す。数の閾値で rules 行と裁定が要る。既存の cap の残りで足りる。
  - （.728）差の無い file にも括弧を付ける。既存の歯の期待が 7 本動く（base.rs の in-file 4 本・e2e の `pipe_review_base_place_only_item_is_read_in_base_txt`・`pipe_prelens_` の e2e 2 本の部分一致）。差の無い file では、読み手が食い違いを見ない。
  - （.728）外の材料の data file の行数を畳んだ数へ揃える。data file の行数の意味（wc -l と同じ）を変える（memo が却下した形）。

## 56. 審査の材料に item の可視性・名指しの item の本文・別の置き場の行を足す（契約表の行 bh / bi / bj・memo `s2-07l.736.1`）

- 何が起きているか:
  - 申告（隣の project・2026-09-28・件数だけを写す・要旨は台帳 `s2-07l.736.1`）: 走行 58 回のうち審査が PASS でない周は 12 回で、そのうち 7 回が `section-material-missing` だった。材料の欠けの型は 7 つある。(1) write-set の外の data file の 1 項目。(2) 別の契約の行の節と src の fn の本文。(3) write-set の `=` の file の中身。(4) `=` の歯の file の const の値。(5)(6) item の可視性（mod が pub か・pub な struct の欄と fn が pub か）。(7) 別の設計ノートの行を doc の字なしの「行 <id>」で指す参照。
  - 入れ替えの後（申告・2026-09-28）: §55（行 bg）を含む binary の入れ替え（8dd964f）の後にも 3 回起きた。型 5・6 が 2 回で、2 回目は同じ行の write-set の file の他の fn の本文も求めた。型 7 が 1 回で、§55 の閉じた 5 形のどれでもない（その id が申告の置き場の同じ dir でちょうど 1 置き場に在るかは未測）。memo の昇格条件（入れ替えの後に 1 回以上・候補に当たる）を満たす。
  - 本 repo の置き場（verified・main fd056ef）: 契約表の `=` の項目は 1 本（`crates/scribe2-boundary/tests/e2e/pipe.rs`・102,081 byte）だけである。本 § は型 3・4 を名指しの item の本文（行 bi）で受け、`=` の file の全文は後の行へ送る（限界）。
- 現物（main fd056ef・verified）:
  - 可視性: `crates/scribe2/src/pipe/review/base.rs` の `declared_name`（:106）は、可視性の語（`pub` と括弧つきの `pub(…)`）を飛ばして `<語> <名>` だけを返す。`item_text`（:59）の宣言の列はその字面を並べ、struct の欄は列に無い。`item_text` は外の材料の (b)（名指された `.rs` の要約・§55 (i) の要約も同じ）も作る（outside.rs の `rs_chunks`・:337）。外の材料の (a)（§51 形 3）は宣言の行を字面のまま渡すので可視性を持つ。ただし write-set の中に宣言が在る名は出さない（base の要約が持つ前提）。`declared_name` の字面は、外の材料の候補の名（`crates/scribe2/src/pipe/review/outside.rs` の `declarations`・:127）と、親 module の mod の行の照合（`mod_line`・:267）も読む。
  - 本文: write-set の中の item は、base の要約の名の列にしか現れない。外の材料の (a) の fn は署名までで、本体は渡らない（`item_lines`・:296）。閉じ括弧までを数える読み手は、outside.rs の私有の `block_end`（:319）1 本である。
  - `=` の file: base の要約の 1 項目（行数・宣言の列・歯の列）だけを持つ（§44）。
  - 別の置き場の行: `crates/scribe2/src/pipe/review/outside/linked.rs` の `resolve`（:130）は、doc の字を持たない `行 <id>` を契約の設計 doc の表でだけ引き、無ければ解けない参照にする。doc の字の basename は `.md` だけを読み（`basename`・:221）、§ の本文は `## N.` の見出しで切る（review.rs の `section_text`）。
  - 契約の置き場の `.toml`: 器はこの形を読む。§2 (ii) の置き場で、`crates/scribe2/src/pipe/table/parse.rs` の `form_of` は `.md` を区間、`.toml` を全文として読み、`parse_pointer` は `<path>.toml#<id>` を受ける。§47（行 ay）で goal の欄も着地している。本 repo の設計 doc に `.toml#` の pointer は 0 件で、この形を使うのは導出物を置き場にする消費側である。`.toml` の表の行を `行 <id>` で指されても、`.toml` には見出しが無いので § の本文は空になり、今は解けない参照になる。したがって本 § は `.toml` の置き場を範囲に入れる。
- 形（番号は各行の done と 1:1）:
  - 行 bh（可視性・型 5・6）:
    1. 宣言の読み手は 1 本のまま: base.rs の宣言の語の切り（今の `declared_name` の切り方・括弧つきの可視性は閉じ括弧までを 1 まとまり）が、可視性の字面（`pub` / `pub(crate)` / `pub(super)` / `pub(in <path>)`・無ければ空）も返す形にする。`declared_name` はその `<語> <名>` を今と同じ字面で返す（外の材料の候補の名・mod の行の照合・歯の名は 1 字も変わらない）。
    2. 宣言の列の各項目は `<可視性> <語> <名>` で、私有は可視性の字を持たない。例: `pub(crate) fn width, pub struct Shape { x }, enum Tone, pub fn dot, const LIMIT`。
    3. 可視性が私有でない struct は、名の後ろに欄の列を置く。名前つきの欄は `{ <可視性> <欄の名>; … }`、tuple の欄は `(<可視性> 0; …)`（欄の名は位置の番号）で、欄の区切りは `; ` にする（宣言の列の項目の区切り `, ` と分け、列を `, ` で割れば宣言の本数になる）。欄の無い struct は何も足さない。欄は宣言の行から閉じ括弧（`;` で終わる tuple は `;`）までの行のうち、doc 行・注釈の行・属性の行を除き、各行の行末の `//` から後ろを落とした字面を繋いで読む。名の直後（generic の `<…>` は山括弧を数えて飛ばし、`->` の `>` は数えない）が `(` なら tuple、そうでなければ最初の `{` の中を名前つきの欄とし（`{` より先に `;` が来れば欄は無い）、括弧の深さ 0 の `,` で割る。型は渡さず、可視性と名だけにする。私有の struct は欄を持たない（外の module から欄に触れない）。enum の variant は欄として出さない。
    4. 閉じ括弧までを数える読み手は 1 本: outside.rs の `block_end` を base.rs へ移し、outside.rs は base から取り込む。本文は 1 字も変えず、2 本目の数え手を書かない。
    5. base.rs の `PREAMBLE` に 1 文を足す。宣言は可視性の字を前に持ち、字の無い宣言と欄は私有（その module と子孫から見える）であること。欄は名の後ろの括弧の中に `; ` で区切って並ぶこと。trait の impl の中の fn は字を持たず trait に従うこと。
    6. 変えないもの: 見出しの行（行数・括弧・置き場だけの 1 語）・`+` と読めない項目の 1 行・歯の列・`base_block`・外の材料の塊の並びと (a) の字面・`declared_name` と `tooth_names` の返す字面。外の材料の (b) と §55 (i) の `.rs` の要約は同じ `item_text` を通るので、base の要約と同じ可視性と欄を持つ（読み手を分けない）。
  - 行 bi（名指しの item の本文・型 2・4 と、型 5・6 の 2 回目）:
    1. 名指しの 3 形を拾う。本文の列は §51 形 2 と同じ（節の本文〔導出物の行は goal〕・done・約束の行の text）。
       - (P) `<path>.rs#<名>`: path の字の連なりが `.rs` で終わり、直後に `#` と識別子が続く。backtick の内外を問わない。
       - (F) `<path>.rs` の直後の `<語> <名>`: 間の backtick・空白・「の」は飛ばす。語は fn / struct / enum / union / trait / type / const / static / mod / impl。
       - (N) backtick の中身が §3 の fn 形（`crates/scribe2/src/pipe/closure/names.rs` の `form_of` が fn 形と読む字面）のもので、語は fn。

       拾うのは names.rs に足す 1 本の口で、`backticked` と `form_of` と path の字の集合を使い、closure.rs の `pub use` から開く。(P)(F) の path も同じ口が、names.rs の私有の `path_matches`（等しいか `/` 区切りの末尾一致）で形 2 の探し先に解き、解けた項目の path を返す（path の照合の写しを書かない）。`mentioned_names` と `unresolved_names` の判定は 1 字も変えない。
    2. 解き方: 探し先は、write-set の `.rs` の項目（外の材料の木が持つ、接頭辞を剥がした path）のうち base に本文が在るものだけにし、接頭辞は見ない。`+` の新設は base に無いので自然に外れ、素の項目と `=` / `-` / `~` の項目は含む。外の材料の木（`Tree`）の field と `bundle` の引数は変えない（歯の区間の helper が木を組むので、変えると overlay の上で compile が落ち、assert の RED を測れない）。(P)(F) は形 1 の口が返した path、(N) は探し先の全部を探す。宣言は base の本文の全行（本体の区間と歯の区間）で、語と名が一致するものを探す（(P) は語を問わない）。impl は、行頭が `impl` の行のうち名を語の境界で持つ行で、names.rs の `impls_type` の照合を行 1 本に開いて使う。解けない名指し（write-set の外の path・宣言の無い名）は何も足さない。write-set の外の名は §51 形 3 (a) が持つ。
    3. 塊 (j): 名指し 1 つが 1 塊。頭の字面は次の 2 形だけ:
       - 宣言が 1 file に在る: `- <path>#<名>: <path>:<行>`。`<path>` は宣言の在る探し先の path（(P)(F) は形 1 の口が返した path、(N) は見つけた path）。同じ file に同じ名の宣言が 2 つ以上在れば、所在を `, ` で並べて全部の本文を続ける。
       - (N) の名が探し先の 2 file 以上に在る: `- <名>: 宣言 <n> か所（<path>:<行>, …）` の 1 行だけ（§51 形 3 (a) と同じ字面・所在は探し先の中のものだけ・本文は続けない）。

       続きの行は、直上の doc 行と属性行から宣言の閉じ括弧（`block_end`）までを 2 字下げたもの。fn は本体の閉じ括弧まで、const と static は `;` まで、struct / enum / union / trait / impl は閉じ括弧まで。
    4. 並び: 外の材料の既存の (f) → (d) → (e) → (a) → (b) → (c) の後ろ、§55 の (g) の前に置く。契約の本文が直に名指す item を、1 段先の § の本文より先に置くためである。(j) は小さいとは限らない（見込みの p90 は、設計 doc の § の本文の p90 約 12,000 byte より大きい）。(j) が cap の残りに収まる周は、後ろの (g)(h)(i) が切り詰めの 1 行か落とした本数の 1 行になりうる（黙っては消えない）。塊は名指しの現れた順で、同じ path と名は 1 塊にする。
    5. cap は新しい閾値を作らない: `outside_block` のまま。収まらない塊は切り詰めの 1 行、それも収まらなければ落とした本数の 1 行になる（§51 形 4）。
    6. outside.rs の `PREAMBLE` に、(j) の位置と中身（名指しの 3 形・write-set の中の宣言の本文・2 file 以上に在る名は所在の 1 行）を足す。
    7. 変えないもの: 既存の塊の中身と並び・材料の file の数と名・`mentioned_names` と `unresolved_names`・base の要約・lens の雛形と穴。
  - 行 bj（別の置き場の行と `.toml` の置き場・型 7）:
    1. doc の字を前に持たない `行 <id>` が契約の設計 doc の表に無いとき、設計 doc と同じ dir の直下の tracked の契約表の置き場を引く（tracked の列で引き、dir を disk から読まない・別の dir は引かない）。置き場は `form_of` が読む 2 形（`.md` の区間と `.toml` の全文）で、その id の行を持つ置き場がちょうど 1 つなら、その行に解く（`<doc> 行 <id>` と同じ § の塊・done つき）。0 なら今どおり解けない参照にする。2 つ以上なら、解けない参照の字面の後ろに `（同じ dir の <n> 置き場に在る）` を添える（どれを指すかは器が決めない）。同じ doc に在る id は、今どおり同じ doc を先に読み、別の置き場を引かない（自分の doc は置き場の数に入れない）。
    2. `.toml` の置き場を doc の字として読む: (C) `<file>.toml#<id>` と、(D) `<file>.toml 行 <id>`・`<file>.toml の行 <id>` を拾う（basename の拡張子に `.toml` を足す）。(A)(B) の `§N` は `.md` のままにする。
    3. `.toml` の置き場の本文: 行を指した参照は、§ の本文の代わりにその行の goal（§47）を塊の本文にする。doc の字なしの `§N` は、契約の置き場が `.toml` なら、section が N の最初の行の goal を本文にする（`.toml` に見出しは無い）。goal が空なら解けない参照にする。塊の頭は `- <path> §<section>` のまま。goal は §47 で節の本文の逐語なので、同じ section の行は同じ goal を持つ。同じ置き場の同じ section を指す参照は今どおり 1 塊に畳み、本文は最初に解けた行の goal にする。行を指した参照は `.md` と同じく塊の末尾に `行 <id> の done:` の行を足し、`§N` の参照は done の行を持たない。
    4. 変えないもの: (A)(B)(E) の § の拾い方・自分の § の除外・同じ § の畳み・(g) → (h) → (i) の並び・1 段・`section_text`。
- 見込み（deduced・main fd056ef の木で模した・各便の base の木とは差がありうる）:
  - 行 bi: 26 doc の契約表の区間の行 277 本の節の本文と done を模した。(j) の塊を 1 つ以上持つ行は 60 本。形ごとの本文に解ける名指しは (N) 102・(F) 17・(P) 0（本 repo は (P) を使っていない）。(N) の名が write-set の 2 file 以上に在るのは 11（所在の 1 行）で、宣言が write-set に無い名は 285（何も足さない）。1 行の (j) の byte は中央値 1,247・p90 30,608・最大 34,170。上の端は cap の残りに入らず、切り詰めの 1 行になりうる。
  - 行 bj: 26 doc の表の行 id は 53 種で、1 doc にだけ在る id は 12。節の本文と done の doc の字なしの `行 <id>` のうち、自分の doc に無いものは 230 件ある。そのうち同じ dir のちょうど 1 doc に在るものが 21 件（8 行）、2 doc 以上に在るものが 51 件（数を添えた解けない参照）、どこにも無いものが 158 件。
- 歯（接頭辞 3 つ。どれも `crates/` 全体で `fn` の名の substring に 0 件。契約表の nextest の verify 行の filter 語のどれも、これらの接頭辞の substring にならない〔実測 2026-09-28〕）:
  - `summary_visibility_`（行 bh・outside.rs の既存の歯の区間に 2 本・tmp の dir に fixture の `.rs` を書き、base.rs の `item_text` と `base_block` を review の子孫の module から呼ぶ）。期待を直す既存の歯を持つ base.rs には新しい歯を置かない。flip-check は base.rs を 1 turn で撃ち、その turn は直した既存の歯の赤で必ず RED になるので、同じ file に置いた新しい歯の RED が隠れるためである:
    - (a) fixture の `.rs` 1 本に、可視性 5 形（`pub` / `pub(crate)` / `pub(super)` / `pub(in crate::x)` / 私有）の fn、名前つきの欄を持つ pub の struct（pub と pub(crate) と私有の欄・doc 行と属性行つき）、tuple の pub の struct、欄の無い pub の struct、欄を 1 つ以上持つ私有の struct、variant を持つ pub の enum、impl の中の pub の fn を置く。宣言の列が、それらの字面の列と等しい（母集団 = 宣言の本数を、列を `, ` で割った同じ assert で数える）。私有の struct と pub の enum は名の後ろに括弧を持たない。
    - (b) 欄が 1 行に並ぶ pub の struct、型に `,` を含む欄（generic の山括弧）を持つ pub(crate) の struct、行末の `//` 注釈に `,` を含む欄を持つ pub の struct、generic の中に Fn(u8) の形の括弧を持つ名前つきの欄の pub の struct、型に `->` を持つ欄の後ろにもう 1 つ欄を置く pub の struct、名の直後の generic が `->` を持つ tuple の pub の struct（generic の中に Fn(u8) と -> を持ち、括弧の中に pub の欄と私有の欄を 1 つずつ持つ形）で、欄の名と可視性だけが `; ` 区切りで読まれる（偽の欄も tuple の読みも出ない）。後ろの 2 つは、base.rs の山括弧を飛ばす行と `->` の `>` を閉じと数えない腕のどちらを消しても (b) を赤にする置き方である（欄の最後に `->` を置き tuple の generic を持たない置き方では、2 つを消しても歯の出力が変わらない）。base の説明の 1 行が、私有と欄の区切りと trait の impl の読みを名乗る。
  - `named_item_body_`（行 bi・outside.rs の既存の歯の区間に 3 本・既存の helper で `bundle` を通す・fixture は歯の中の別の helper で足し、共有の `FILES` と `linked_scratch` は変えない）:
    - (a) write-set の中の fn・const・struct・impl を 3 形で名指すと、(j) の塊が既存の塊の後ろ・(g) の前に名指しの順で並ぶ（母集団 = 塊の頭を同じ assert で数える）。fn は本体の閉じ括弧まで、const は `;` まで、doc 行と属性行つき。(j) の塊は `- <path>#<名>` の頭で引く（(e) の塊の頭も `- <path>` で始まるので、path だけで引かない）。
    - (b) 次は塊を作らない: write-set の外の path（宣言を持つ tracked の file を指す・toy の木の shape.rs の zq_area を (P) で名指す）・宣言の無い名・大文字始まりの tuple variant の字面（write-set の fixture の `.rs` に pub fn ZqWrap() {} を置き、本文に backtick つきで ZqWrap(1) を書く。大文字の頭を fn 形と読む実装はその宣言に当たって咬まれる）。write-set の 2 file に在る (N) の名は `- <名>: 宣言 2 か所（…）` の 1 行と等しい。
    - (c) 残りが足りない周は (j) の塊が切り詰めの 1 行になり、説明の 1 行が (j) を名乗る。
  - `note_row_lookup_`（行 bj・outside.rs の既存の歯の区間に 2 本・歯の中の別の helper で、§55 の toy の木に同じ dir の `.md` と `.toml` の置き場と囮 3 つを足す。既存の `linked_scratch` は変えない。囮は、別の dir の置き場で同じ行 id を持つ行・disk に在って tracked の列に無い同じ dir の置き場・自分の doc に在る id を持つ別の置き場。行 id は既存の歯の本文の id と重ねない）:
    - (a) 自分の doc に無く同じ dir の別の 1 置き場に在る `行 <id>` が、その置き場の § の塊（done つき）になる（別の dir の置き場と tracked でない置き場は数えない）。2 置き場に在る id は `（同じ dir の 2 置き場に在る）` を添えた解けない参照、0 の id は今の字面のまま。自分の doc と別の置き場の両方に在る id は自分の doc の § に解ける。
    - (b) `<file>.toml#<id>` と `<file>.toml 行 <id>` と、`.toml` の置き場の契約の doc の字なしの `§N` が、その行の goal を本文にした塊になる。同じ section の 2 行を指す参照は 1 塊（本文は goal 1 つ・done の行 2 つ）になる。goal の空の行は解けない参照になる。
  - base で RED の理由: 3 つの接頭辞の歯は、base に在る関数（`item_text`・`base_block`・`bundle`・`outside_block`）だけを呼ぶ。overlay の上で compile は通り、assert が落ちる（機能不在）。retroactive の札を持つのは、行 bh が直す headless/lens.rs の既存の歯 1 本だけ（直した後も base で緑・done (7)）で、他の file には札を置かない。行 bh は flip する file が 3 本（base.rs・outside.rs・下の e2e の file）なので、flip-check は file ごとに 1 turn ずつ撃ち、outside.rs の turn は `summary_visibility_` の赤だけで RED になる。行 bi / bj は flip する file が outside.rs の 1 本で、その歯の assert の赤を測る。
- 既存の歯の期待が動くもの（行 bh だけ）。どれも期待を新しい字面へ直すので base で赤い普通の flip で、札は置かない:
  - base.rs の in-file の 4 本: `pipe_review_base_rs_lists_declarations_and_teeth_in_separate_columns`（宣言の列が `pub(crate) fn width` / `pub struct Shape { x }` / `pub fn dot` の字面になる）・`pipe_review_base_pub_in_path_visibility_declarations_are_listed`・`pipe_review_base_pub_in_without_closing_paren_is_not_a_declaration`（`pub(in …)` の字面が項目の頭に付く）・`pipe_review_base_place_only_item_is_read_and_marked_while_other_forms_stay`（3 か所の宣言の列）。
  - `crates/scribe2-boundary/tests/e2e/pipe/review.rs` の `pipe_review_base_place_only_item_is_read_in_base_txt`（宣言の列が `pub fn placed` になる）。
  - 動かないもの（実測）: 他の `pipe_review_base_` の歯（`.md` と `+` と読めない項目の 1 行・行数の見出し・`contains` の部分一致）・`review_base_lines_`（見出しと説明の 1 行の部分一致）・`pipe_review_outside_`（(a) の塊は宣言の行の字面・(b) の要約の宣言の列を字面で測る歯は 0 本・`block_end` の本文は不変）・`linked_section_material_`・`headless_lens_` の base と outside の歯（写しの字面を自分で持つ。ただし `headless_lens_outside_room_is_measured_after_the_base_stage` は、行 bh の PREAMBLE の伸びで外の材料が切り詰めの 1 行も持てなくなるので行 bh が直す・done (7)）・`pipe_prelens_` の e2e の `行数 全体` の部分一致。行 bi / bj の fixture は (P)(F)(N) の形と、別の doc に在る doc の字なしの行 id を持たない（`crates/` に `.rs#` の字面は 0 件・review の e2e の toy の節に backtick の fn 形は無い）ので、外の材料は変わらない。
- 限界:
  - 可視性は字面の読みである。`pub use` の再公開・macro が作る item・`#[path]` の module は読まない。trait の impl の中の fn は字を持たない（説明の 1 行が読みを名乗る）。欄の読みは括弧の数えで、字の literal の中の括弧は数え違えうる（`block_end` と同じ下界）。
  - 名指しの本文は 3 形だけである。語の無い「`<path>` の `<名>`」の形は本文を足さない。本 repo の散文で最も多い形で、同じ木で模すと本文の塊を持つ行が 149 本・塊 322 個になる（3 形では 60 本・119 個）。1 行の byte は中央値 2,855・p90 9,696 で、1 段先の § の本文より先に cap の残りを食う。名の列は base の要約が持ち、本文が要る名は 3 形で書ける。`型::項目` の形も本文を足さない。名指されない fn の本文は渡らない（型 2 の「write-set の file の他の fn」は名指せば届く）。
  - (j) の探し先は、base に本文が在る write-set の `.rs` の項目だけである。write-set の dir の項目の配下の file は、(P)(F) で名指しても本文を足さない（(a)(b) も write-set の中として外す）。`+` の新設の file の本文も渡らない。
  - 別の置き場の行は同じ dir だけを引く。設計ノートの YAML の正本・ADR・SRS は読まない。2 置き場以上に同じ id が在る行は解かない。
  - `=` の file は、名指しの item の本文（行 bi）だけを渡す。file の全文は渡さない。小さい `=` の file の全文を足す形は、閾値の値の裁定（C1・C5）が要るので後の行に送る。素の項目の file も全文を渡さない（§40 の却下と同じ・write-set の中身の差分は審査の後の gate が測る）。
  - 材料の鍵（dispatcher.md §27 の先撃ちの使い回し）: 3 形の本文と別の置き場の行は外の材料の file に入る。各行の着地の直後は、名指しや参照を持つ行の先撃ちの鍵が一斉に変わり、撃ち直しが 1 回増える（偽の使い回しにはならない）。
- 却下:
  - 可視性を新しい列（`  可視性:` の行）で渡す。宣言と可視性の対応を lens が 2 列から組み直すことになる。既存の歯の期待は、どちらの形でも行数か列の字面が動く。
  - 私有の宣言に「私有」の字を付ける。全ての宣言の列が伸び、Rust の字面（語の無い宣言は私有）と違う。
  - 欄の型まで渡す。型は宣言の塊（§51 (a)）と (j) が字面で持つ。可視性の問いには名と字で足りる。
  - write-set の file の全文を渡す（§40 の却下と同じ）。
  - 語の無い「`<path>` の `<名>`」にも本文を足す。見込みで本文の塊を持つ行が 60 本から 149 本に増え、cap の残りを 1 段先の § より先に食う。
  - (j) を base の要約（base.txt）に入れる。`base_block` は段ごと落とすので、大きい本文 1 つで要約の全部が落ちる。外の材料の file は塊ごとに切り詰める。
  - 新しい材料の file を足す（§55 の却下と同じ・lens の雛形の穴と cap の配分と材料の dir の本数の歯が動く）。
  - 別の置き場の行を、2 置き場以上に在る id でも最初の 1 つに解く。別の行の § を束ねて lens に誤った根拠を渡す。数を添えて解けない参照に残し、doc の字を書き足す直しに倒す。
  - 設計 doc の dir の外（repo の全 tracked）の置き場を引く。本 repo の 26 doc の外に契約表の置き場は無く、他の dir の `.md` の同名の id を拾う。

## 57. pipe/cli/intake.rs の「断りの組み立てと上限の余地」の群を子 module へ割る（契約表の行 bl・純移動・§38 と同じ型）

やさしく言うと: 受付の本体の file が上限（1500 行）まで残り 14 行しか無く、受付の断りを足す後の行（FR83 の解けない裁定 id の断り・FR68 の断りごとの event）が受付で断られる。断りを組む部品と上限の余地の判定を、名前も本文も変えずに子の file へ移して余地を 190 行ほどにし、後の行が足す物の置き場を先に作る。

- 何が起きているか（main 24f6ef1・verified 2026-09-29）:
  - `crates/scribe2/src/pipe/cli/intake.rs` は 1473 行・幅 120 で正規化して **1486** 行（R-C4-2 = 1500・余地 **14**）。size S（見積 100）の便も、growth で 15 行以上を名乗る便も、受付の `cap-headroom` で断られる。
  - 子の dir は無い。`crates/scribe2/src/pipe/cli.rs` が `mod intake;`（21 行）で宣言し、`crossings` / `generated` / `judge` / `Denial` / `Material` / `Materials` を `pub(in crate::pipe)` で再輸出する（32 行）。兄弟の 6 file（preflight / resume / run / state / show / base_run）は `super::intake` の名で引く。cli の下の file はどれも子の dir を持たないので、子は `crates/scribe2/src/pipe/land.rs` の `mod verify;` と同じ形（file の名の dir の直下）に置く。
  - 歯の区間: 列 0 の `#[cfg(test)]`（1353 行）の次の非空行が `mod tests {`（1354 行）で、札は `mod tests {` の直後に置ける（[pipeline.md](./pipeline.md) §45 の `#[path]` 形の罠には当たらない）。in-file の歯は 4 本で、群を読むのは `contract_closure_ext_survivor_a_cap_shortfall_recounts_without_the_unresolved_items` の 1 本だけ（`use super::{…}`〔1355–1358 行〕が群の `exclude_cap_shortfall` / `ROW_FILE_LINES` / `ROW_SIZE_S` を名指す）。
  - **群は閉じている**（19 item・162 行・正規化 165 行）:
    - (i) 断りを組む口 4 つ: `refuse`（1283–1288）・`denied`（594–597）・`not_a_repo`（564–567）・`refuse_of`（894–909・導出の理由を契約単位の拒否へ写す）。
    - (ii) `Refuse` を持たない断りの名 6 つ: `DENIAL_ARGS`（369–370）・`DENIAL_GENERATED`（456–457）・`DENIAL_DECLARATION` / `DENIAL_RULES` / `DENIAL_SIZE` / `DENIAL_STORE`（582–592）。
    - (iii) 上限の余地の判定と報告: `exclude_cap_shortfall`（1198–1271）・`headrooms_of`（1172–1196）・`size_row`（1273–1281）と、rules 行の id 6 つ（`ROW_FILE_LINES` / `ROW_CORE_LINES` / `ROW_LINE_WIDTH`〔52–59〕・`ROW_SIZE_S` / `ROW_SIZE_M` / `ROW_SIZE_L`〔64–71〕）。`ROW_MAX_LIVE`（61–62）は `exclude_max_live` だけが読むので親に残る。
  - 呼び手（母集団 = 親の全関数と `crates/` の全 `.rs`）: 親の本体から裸で呼ばれる名は 10 個（`refuse` 27 site / 15 関数・`denied` 12 site / 9 関数・`refuse_of` 7 site・`not_a_repo` 2 site〔`intake_run` と `judge`〕・`exclude_cap_shortfall` 1 site〔`judge`〕・`DENIAL_ARGS` / `DENIAL_GENERATED` / `DENIAL_DECLARATION` / `DENIAL_RULES` / `DENIAL_STORE`）。群の中だけで呼ばれる名は 7 個（`headrooms_of` / `size_row` / `DENIAL_SIZE` / `ROW_CORE_LINES` / `ROW_LINE_WIDTH` / `ROW_SIZE_M` / `ROW_SIZE_L`）。歯だけが引く名は 2 個（`ROW_FILE_LINES` / `ROW_SIZE_S`）。他 module からの参照は 0 site（`crates/scribe2/src/pipe/review/base.rs` の `ROW_LINE_WIDTH` は同名の別物）。
  - 群が親から引くもの: 型 `Denial`（literal で組む）・`Headrooms`（literal で組む・私有の field を持つ）・`Materials`（私有の field `tracked` / `sources` を読む）と、`crates/scribe2/src/pipe/cli.rs` が再輸出する `int_row` / `broken` / `refused`。子孫は親の私有の item と field を見るので、どれも可視性を変えずに引ける。
  - **親に残す型**: `Denial`（field `outcome` は `pub(super)` で `crates/scribe2/src/pipe/cli/preflight.rs` が読む）と `Headrooms`（field `rooms` と `estimate` を preflight の `headroom=` の行が読む）。子へ出すと field の語を広げることになり、機械の証明が items-differ で落ちる。`unloadable`（`pub(super)`・`crates/scribe2/src/pipe/cli/resume.rs` が引く）も親に残す。
  - `crates/scribe2/src/pipe/refuse.rs`（`Refuse` の型と理由の字面）は別の file で、本行はそこと item を跨がせない。
  - 閉包の波及（scratch の写しで実測・`scribe2 contracts check`・母集団 26 doc / 346 行）: 子は `Refuse` と `TableError` の値を組み、`ClosureError` と `WriteSetItem` を match し、`Denial` を literal で組む。そのため、これらの型を touches に持つ着地済みの 11 行の閉包に子が入る。子だけを置いた木は findings 11（全部 `write-set-incomplete`・本 doc の行 a / b / g / h / w / ar / aw / ax / az・[dispatcher.md](./dispatcher.md) 行 x・[pipeline.md](./pipeline.md) 行 ay）。11 行の write-set に子を `+` で足すと、子の在る木は findings 0 になる。ただし子の無い木（docs の PR の直後）では、§54 の予想が行 b を着地の前の行と読み、行 b の新しい語 `pipe_intake_design_` が行 ad の歯の置き場を `crates/scribe2/src/pipe/mod.rs` へ広げて 1 件出る。行 ad に `=crates/scribe2/src/pipe/mod.rs` を足すと、子の無い木と在る木の両方で findings 0。
  - 分割の実測（scratch の写し・上の docs の直しと本 § の行を当てた木に分割を当てた）: move_proof の判定は純移動（items=85・moved=19・visibility=12・残差は use / mod / `#[cfg(test)]` だけの行 / `//` / 札だけ）。clippy -D warnings は lib と test の build の両方で rc 0。検証行の 4 本は緑。flip-check は RED-on-base ok・moved=1（1447 行の歯の中の retroactive の札の持ち越しで stale-marker の 1 行が出るが rc 0）。xtask check は ok（core-lines 57622 → 57652・rules-wired 5/74・env-reads 0/7・enum-slices 52 は不変）。`pipe preflight` は ok（write-set=declared files=3・歯の 4 行がどれも 1 file に解ける・子の余地は 1500/210）。
- 形（番号は done と 1:1・0 だけは本 § と同じ docs の PR）:
  0. 着地済みの 11 行（本 doc の行 a / b / g / h / w / ar / aw / ax / az・dispatcher.md 行 x・pipeline.md 行 ay）の write-set に、行 bl の write-set の `+` の file を `+` 付きで足す（`crates/scribe2/src/pipe/cli/intake.rs` の項目の直後）。行 ad の write-set に `=crates/scribe2/src/pipe/mod.rs` を足す。12 行の bead は閉じているので起動の列に戻らない。便の done には入れない。
  1. 上の 19 item を、行 bl の write-set の `+` の file へ、名・本文・順序（base の順）・doc comment を変えずに移す。子の頭は module doc と札と `use` 10 文だけ（`use super::{Denial, Headrooms, Materials};` と `use crate::pipe::cli::{broken, int_row, refused};` と、`Outcome` / `ClosureError` / `Contract` / declaration の `self` と `NewFilePolicy` と `WriteSetItem` / `Refuse` と接頭辞の 4 定数 / `ContractRow` と `TableError` / `Manifest` / `Path` を元の crate の path から引く 8 文）。
  2. 親に増えるのは空行を除いて 6 行だけ: file 頭の `use` 群（32–50 行）の直後に、`///` 1 行つきの `mod` 宣言 1 行と、本体用の素の `use` 2 文（`denied` / `exclude_cap_shortfall` / `not_a_repo` / `refuse` / `refuse_of` の 1 文と、`DENIAL_` の 5 名の 1 文）。既存の列 0 の `#[cfg(test)]`（1353 行）の直上に、`#[cfg(test)]` だけの 1 行と歯用の `use` 1 行（`ROW_FILE_LINES` / `ROW_SIZE_S`）。親で孤立する import の 5 語（42 行の `NewFilePolicy` / `WriteSetItem`・43 行の `SHRINK_FILE` / `DELETE_FILE` / `PLACE_ONLY_FILE`）を削る（残差の許容形・この数に含めない）。6 行とも 120 桁に収まる。
  3. 歯は 1 本も足さず 1 本も変えない: in-file の `mod tests` の本文と `use super::{…}` は 1 byte も変えない（3 名は親の `use` が解く）。e2e は触らない（検証行の置き場として `=` で載せるだけ）。
  4. 子側で `pub(super)` にするのは 12 名だけ（親が呼ぶ 10 名と歯が引く 2 名・全部 item の頭の行）。群の中だけの 7 名は私有のまま。親側の可視性・親に残る型の field・`crates/scribe2/src/pipe/cli.rs` の再輸出は変えない。
  5. 札 `// flip-check: moved <行 bl の bead>` を親の `mod tests {` の直後と子の module doc の直後に 1 行ずつ置く。子は歯の区間を持たないので、数に入るのは親の札だけ。子にも親の file 頭にも列 0 の `#[cfg(test)]` を足さない（xtask の門の src と歯の切れ目が file 頭へ動くため・歯用の `use` の属性は 1353 行の直上だけ）。
  6. 検証行が名指す既存の 4 本（下の「歯」）が、着地後も名・本数・本文を変えずに緑。clippy -D warnings が通常 build と test build の両方で rc 0。
- write-set の面（§3 の逐語: 縮む面は `-` で宣言する）: 親は**縮む面**（`-`・素の path で書くと受付が自分の見積で `cap-headroom` に倒れる）、子は**新規 file**（`+`）、e2e の file は**置き場だけ**（`=`・検証行の歯の置き場・中身を変えない）。diff の面は「親から 19 item が消え、子に同じ 19 item が現れる」の 2 file だけ。
- 見積: 親 1486 → 約 1311（余地 約 189）・子 約 208 行（growth 210）。core の本体の合計は約 +30（子の module doc と `use` の分）。xtask の門の副作用は無い（上の実測）: 群に `std::env::` と `Command::new` の site は 0・`WRITE_SETS`（enum-slices）は親に残る・rules 行の id の const 6 つは読み手（`exclude_cap_shortfall` / `size_row`）と一緒に子へ移るので rules-wired の数は不変・親の src と歯の切れ目は歯用の `use` の属性行へ 2 行ぶん前へ動くだけ。
- 後の行の受け皿: FR83 の受付の断り（解けない裁定 id の引用）を足す行は、判定の 1 関数を子に置き（`refuse` で組む）、`judge` には呼び出しの数行だけを足す。FR68 の断りの event を足す行は、断りの名（`Denial` の `name`）と bead から event の key を組む 1 本を子に置く。どちらも `Refuse` か `Denial` を組むので、閉包が子を write-set に導く（着地の後は素の path で書く）。
- 触らない: `Denial` / `Headrooms` / `Materials` / `Judged` / `Crossed` の型と field・`unloadable`・`check_symbols`・判定関数の順（`judge`）と断りの字面と rc・`crates/scribe2/src/pipe/refuse.rs`・preflight の `refuse=` の行（`crates/scribe2/src/pipe/cli/preflight.rs` の `refuse_line`）・`crates/scribe2/src/pipe/cli.rs` の再輸出・e2e の歯。
- 却下:
  - `Denial` と `Headrooms` も子へ出す（preflight が読む field の語を広げることになり items-differ）。
  - 群を `crates/scribe2/src/pipe/refuse.rs` へ寄せる（別の module へ item を跨がせる・`Denial` は受付の rc と行を持つ受付だけの型・refuse.rs は 999 行で後の行が `Refuse` の値を足す）。
  - `headrooms_of` だけを出す（25 行で余地が 39 にしかならず、呼び手の `exclude_cap_shortfall` と群が割れる）。
  - 焼き直しの門の群（`exclude_repeats` / `history` / `exclude_same_kind` / `exclude_unaddressed` と型 2 つ・約 165 行）を出す（余地は同じだけ出るが、後の行が足す断りの組み立ての受け皿にならない）。
  - 生成の群（`generated` / `regenerated` / `promised` ほか）を出す（`generated` は cli の再輸出・`regenerated` は resume.rs が引き、再輸出の面が増える）。
  - `unloadable` も移す（4 行のために親へ `pub(super) use` の再輸出が要る）。
  - 12 行の write-set の直しを便の PR に入れる（diff に `.md` が入って機械の証明が純移動と読めなくなる）。
  - `#[cfg(test)] use …;` を 1 行に畳む（残差の許容形に当たらず residual-line）。
- 歯: 新設 0 本。検証行は既存の 4 本を名の全体で書く（repo 内で 1 件ずつ・2026-09-29 に grep で実測）。in-file（親の歯の区間）の `contract_closure_ext_survivor_a_cap_shortfall_recounts_without_the_unresolved_items`（余地の判定・余地の列・断りの名）。e2e（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`・write-set に `=` で載せる）の `pipe_intake_core_headroom_counts_the_core_total_without_in_file_tests`（core の余地の断りの行）・`contract_growth_preflight_headroom_estimate_is_per_file`（preflight の `headroom=` の行＝`headrooms_of` の列）・`pipe_intake_promise_verify_matches_as_a_set_or_is_refused_as_drift`（`refuse_of` を通る drift の断り）。
- base で RED の理由: 純移動は歯を足さないので、base で RED になる歯は無い。入口の確かめは札（flip-check の moved）が持ち、移動の正しさは move_proof の機械証明（名と本文の hash の多重集合が一致し、残差は `use` / `mod` / `#[cfg(test)]` だけの行 / `//` / 札だけ）が持つ。

## 58. 終端だけの撃ち直し（`--terminal-only`）は main の今の先端で照合する — 着地した commit が先端そのものなら今の照合、先端でなければ §53 の先端の照合へ回し、祖先でなければ閉じない（契約表の行 bm・FR50・§53 の限界）

やさしく言うと: 止まった便の後始末（push → CI の確認 → 台帳を閉じる）をやり直す口 `pipe land --run <run> --terminal-only` は、今は便の commit そのものの CI を確かめる。ところが CI は main へ押した一番先の commit にしか走らないので、列で着地して先頭でなかった便や、後の commit が main を進めた便は、自分の commit の CI が見つからないか赤のままで、何度やり直しても閉じない。やり直しの周は main の今の先端を読み、便の commit が先端ならそのまま、先端より前（先端の祖先）なら先端の CI で確かめて閉じ、main の歴史から外れた便は閉じない。

- 何が起きているか:
  - verified（main c00026ab の code）: 終端だけの撃ち直しは、記録から着地した sha を読み（`crates/scribe2/src/pipe/land/finish.rs` の `landed_sha`・198 行）、終端に常に先端の側（`PushTip::Tip`）を渡す（`crates/scribe2/src/pipe/cli/step.rs` の 91–92 行）。終端は push の後、その sha そのもので CI を照合する（finish.rs の 261–279 行）。
  - verified（§52 の実測）: forge の CI は push の先端の commit にだけ run を作る。先端でない sha の照合は run を 1 本も見ず、上限（rules 行 `pipe.ci_wait_s`）まで待って `ci:unmeasurable` で終わる。本 repo の CI の workflow は main への push ごとに走り、path の絞りも同時実行の打ち切りも持たない（`.github/workflows/ci.yml`）。
  - deduced: §53 で着地の周は先端の CI で閉じるようになったが、終端が止まった便（先端の CI が上限までに終わらなかった・CI が赤で後の commit が直した・push や close が落ちた）の撃ち直しは、便の sha を先端として照合する。列の先端でなかった便は自分の sha に run が無く、上限まで待って `ci:unmeasurable` に戻る。自分の sha の CI が赤の便は、後の commit で main が緑になっても赤のまま。どちらも撃ち直しでは閉じず、手の close が残る。§53 の限界はこの穴を「anchor の main を先端とする判定が要る（次の行の候補）」と書いている。
  - FR50 は「push の後は push の先端（最後に着地した commit）の CI の結果を commit id で照合し」「push の先端でない便は、自分の着地 commit を含む push の先端の commit の CI の結果で照合し、note に先端の commit id も持つ」を持つ。撃ち直しの push が押すのは anchor の main そのものなので、押した先端は main の今の先端である（要件の改訂は要らない）。
- 現物（main c00026ab・verified）:
  - `crates/scribe2/src/pipe/cli/step.rs`（423 行・in-file の歯の区間は無い）の `terminal_only`（55 行）: 段 `Landed` の確かめ → 記録の sha（60–62 行・読めない周は断る）→ 終端の材料 → `Land` を組み、91–92 行で `PushTip::Tip` を渡す。doc comment（51–54 行）は「HEAD の今の sha に読み替えない」を持つ。
  - finish.rs の `PushTip`（217–222 行）は `Tip` と、先端の sha を持つ `Behind` の閉じた 2 値。doc comment（212–215 行）は「先端を知るのは `land_train` の 1 か所・単独の着地と `--terminal-only` は `Tip`」と書く。`terminal`（230 行）は push（246 行）の後、`Behind` の周だけ `git merge-base --is-ancestor <sha> <先端>` を測り（255 行）、祖先の周は CI の照合（待ちと `ci_now`）を先端の sha で撃ち、祖先でない周は `terminal:ci:unmeasurable` を記して `Terminal::CiUnmeasurable`（close しない）で返す。close の reason は `Tip` の周が `landed <sha> ci=success`、`Behind` の周が `landed <sha> ci=success tip=<先端>`（282–285 行）。
  - main の ref の名は `crates/scribe2/src/pipe/land.rs` の `MAIN_REF`（98 行・`refs/heads/main`）で、着地の本体は同じ ref を読み、読めない周を「refs/heads/main を読めない」で断る（412–413 行）。
  - 歯: `crates/scribe2-boundary/tests/e2e/pipe/land/order.rs` の 374–430 行の既存の撃ち直しの歯は、CI が failure の着地の後に別の commit で main を進め、撃ち直しが CI の argv と close の reason に着地した sha を持ち main の今の sha を持たないことを測る（407–411 行・426–428 行）。本 § で期待が反転する。`--terminal-only` を撃つ歯は repo の中にこの 1 本だけ（ほかに引数の断りの歯が flag の字面を 1 回使う・`crates/scribe2-boundary/tests/e2e/pipe/land.rs` の 3005 行）。
- 判定の順（撃ち直しの周）: 段が `Landed` か → 記録の sha → main の今の sha（形 1）→ 着地した sha と等しいか（形 2 / 3）→ 終端: 押す先の宣言 → push → 祖先か（形 3・§53）→ CI の照合 → close。
- 形（番号は done と 1:1）:
  1. 終端だけの撃ち直しは、記録の sha を読んだ後に anchor の `refs/heads/main`（`MAIN_REF`）の今の sha を読む。読めない周は event を 1 件も書かず、push も CI も close も撃たずに stderr 1 行（`pipe: refs/heads/main を読めない`）の rc 1 で断る（先端の側に倒さない）。
  2. 着地した sha が先端の sha と等しい周は、今どおり先端の側（`PushTip::Tip`）を渡す（自分の sha の CI で照合し、reason は `landed <sha> ci=success`）。
  3. 等しくない周は、先端の sha つきの `PushTip::Behind` を渡す。終端は §53 の分岐のまま: push の後に祖先を測り、祖先の周は先端の sha で CI を照合して success なら reason `landed <sha> ci=success tip=<先端>` で close し、祖先でない周は CI を撃たず `terminal:ci:unmeasurable` を記して close しない（rc 1・stdout は `terminal=ci:unmeasurable`）。
  4. finish.rs は `PushTip` の doc comment の 2 文だけを本 § の読みに直す（着地の周で先端を知るのは `land_train` の 1 か所・撃ち直しは main の今の先端で側を選ぶ）。終端の本体・`PushTip` の 2 値・`land_train`・`finish` は 1 字も変えない。
  5. 変えないもの: 終端の 7 値・event の詞・`ci_now` の判定・rules 行 `pipe.ci_wait_s` と `pipe.ci_poll_s` の値・着地の周の先端の読み・close の reason の 2 形・撃ち直しの前提の段と記録の sha の読み（close の reason が名指すのは記録の sha のまま）・usage と help の字面。
- write-set の面: step.rs は本体（先端の読みと側の選び・約 8 行）、finish.rs は doc comment だけ、order.rs は歯（既存の 1 本の書き直しと新しい 2 本）。e2e の親 `crates/scribe2-boundary/tests/e2e/pipe/land.rs` の helper（偽 remote・偽 CI・偽 bd・Landed の detail の列・event の数）は変えずに使う。
- 見積: step.rs 423 → 約 431・finish.rs 624 → 625・order.rs 627 → 約 705。diff は歯込みで約 110 行（S）。core の本体の伸びは約 9 行。xtask の門の副作用は無い（git は既存の読み手 1 本を撃つ・rules 行を足さない・env の読みと process の起動の site を足さない）。
- 触らない: finish.rs の終端の本体・`PushTip` の 2 値・`land_train` / `finish` / `Landing`・`Terminal` の 7 値と字面・記録の sha の読み手・台帳の close（`crates/scribe2/src/ledger/mod.rs`）・rules 行・usage と help・§52 / §53 の歯（`pipe_train_terminal_` / `pipe_train_tip_close_`）・`--detection-only`。
- 限界:
  - 撃ち直しの先端は anchor の local の main である。forge で merge した commit を anchor が取り込んでいない周（local の main が remote の main の祖先）は、push が non-fast-forward で断られて `terminal:push:failed:git` で止まる（今と同じ・anchor を揃えてから撃ち直す）。
  - 自分の sha の CI を撃ち直して緑にした便でも、main が進んだ後は先端の CI で照合する。先端の CI が赤か上限までに終わらない周は閉じない（close しない側・先端が緑になってから撃ち直す）。
  - 先端を読んでから push するまでに別の着地が main を進めた周は、押した先端と照合の先端が割れる。照合の先端に run が無ければ `ci:unmeasurable` で止まる（close しない側・撃ち直しで継ぐ）。
  - 祖先でない便（main の歴史を書き換えた後など）は撃ち直しでは閉じない。閉じ方は本 § の外。
- 却下:
  - 自分の sha の CI を先に照合し、run が無ければ先端へ回す 2 段の照合。forge の CLI は走っている run と run の無い周を同じ未完了（`ci_now` の `None`）で返すので（§52 の却下と同じ）、自分の sha の照合が上限まで待ち、撃ち直しが上限の 2 倍かかる。`PushTip` に 3 つ目の値を足すと、閉じた 2 値の match の site が全部動く。
  - 等しい周にも `Behind` を渡す。先端の便の reason に `tip=<自分>` が付き、単独の着地の reason（`ci=success` で終わる）と形が割れる。
  - 先端を remote の ref から読む・撃ち直しで fetch する。撃ち直しが押すのは local の main なので、押した先端と照合の先端が割れる。fetch は network を撃つ新しい段になる。
  - main を読めない周に先端の側へ倒す。先端の読みの失敗が記録に残らず、測れない値を効く値として使う（C10）。
  - 祖先の照合を撃ち直しの側で push の前に撃つ。§53 の終端が同じ判定を持ち、判定が 2 か所になる。
- 歯（接頭辞 `pipe_replay_tip_`・`crates/scribe2-boundary/tests/e2e/pipe/land/order.rs` の 3 本・新しい e2e の module は作らない・親の helper を `use super::*` で使う。`grep -rn pipe_replay_tip_ crates/ docs/` は 0 件〔main c00026ab・2026-09-29〕。契約表の nextest の verify 行〔26 doc・351 行〕の filter 語のどれも、3 本の名〔module path 込み〕の substring にならない）:
  - (a) 既存の撃ち直しの歯（374–430 行）を、接頭辞 `pipe_replay_tip_` の名に改めて期待を書き直す（fixture は同じ: CI が failure の着地・別の commit で main を進める・CI を直す・撃ち直す）。rc 0・stdout が `run=<id> terminal=closed` の 1 行・main は器が動かさない・Landed の後ろの 5 件（push・ci:failure・push・ci:success・close:ok）は今と同じ。CI の argv が main の今の sha を持ち着地した sha を持たず、偽 remote の main が今の sha を指し、偽 bd の reason が `landed <着地した sha> ci=success tip=<今の sha>` と等しい。
  - (b) 1 本の fn（CI が failure の着地の後に CI を直す・直した偽 CI も呼ばれた回数を数える）の撃ち直し 2 周: 1 周目は main と偽 remote の main を、着地した commit と同じ木で親も同じ別の commit（着地した commit を祖先に持たない）へ動かして撃ち直す（偽 remote へは別の ref で押してから main を付け替える・force の push は使わない）。rc 1・stdout が `run=<id> terminal=ci:unmeasurable`・偽 CI の呼び出しが増えず・偽 bd が撃たれない。2 周目は main と偽 remote の main を着地した commit へ戻して撃ち直す。rc 0・CI の argv が着地した sha・reason が `landed <着地した sha> ci=success` と等しい（`tip=` を持たない）。Landed の後ろは 7 件（push・ci:failure・push・ci:unmeasurable・push・ci:success・close:ok）。
  - (c) CI が failure の着地の後に `refs/heads/main` を消して撃ち直す。rc 1・stdout が空・stderr が `refs/heads/main を読めない` を持ち、event の数が変わらず、偽 bd が撃たれない。
  - 変異の A/B（判定の順・条件 1 つに歯 1 本）: 形 1 の断りを外して先端の側に倒す → (c) が落ちる（終端が `terminal:unreadable` を記す）。形 2 / 3 の選びを常に先端の側にする（base の形）→ (a)（照合が着地した sha）と (b) の 1 周目（close する）が落ちる。常に `Behind` を渡す → (b) の 2 周目が落ちる（reason に `tip=`）。記録の sha の代わりに main の今の sha を着地した sha として渡す → (a)（reason が着地した sha を名指さない）と (b) の 1 周目（close する）が落ちる。
  - base で RED の理由: 3 本とも base に在る helper だけを使い、overlay の上で compile は通って assert が落ちる（機能不在）。base は (a) で着地した sha を照合し、(b) の 1 周目で close し、(c) で `terminal:unreadable` を記す。(a) は期待が動く既存の歯なので base で赤い普通の flip で、札は置かない。order.rs は 1 file で flip-check は 1 turn（1 行目の base から持ち越した純移動の札は効かず、stale-marker の 1 行が出るだけ）。

## 59. remote を持たない repo の便は、走査も push も CI の照合も撃たずに「landed <着地 commit id> ci=none」で close する — 終端の 7 値の `Undeclared` を close する値に替え、close の理由の尾を 1 つの書き手に寄せる（契約表の行 bn・FR50・[ADR-0094](../../design-intent/decisions/ADR-0094-the-land-terminal-closes-landed-runs-by-three-routes.html) の経路 (2)・裁定 user 2026-09-29T05:46Z）

やさしく言うと: 押す先（remote）を宣言していない repo では、今は main に着地しても台帳の契約が開いたまま残り、人が閉じるしかない。着地した時点で「CI の確かめは無し」と書いて器が閉じる。宣言が読めない周は閉じずに失敗を残し、直した後に終端だけを撃ち直せば閉じる。

- 何が起きているか（main e6ca3e19・verified）:
  - 終端 `terminal`（`crates/scribe2/src/pipe/land/finish.rs` 238 行）は、宣言の事実（`terminal_facts`）が読めない周を `terminal:unreadable` 1 件と `Terminal::Unreadable` で返し（241〜246 行）、`remote` の無い周を event 0 件の `Terminal::Undeclared` で返す（250〜252 行）。close は `remote` が在る周の push と CI の後ろにしか無い（294 行）。
  - close の理由は 2 形を 2 か所の `format!` で組む（290〜293 行・`landed <sha> ci=success` と `... tip=<先端>`）。書き出しは `CLOSE_REASON`（196 行）。
  - 終端の結末は閉じた 7 値で（`crates/scribe2/src/pipe/land.rs` 838 行）、字面の列 `TERMINAL_TOKENS`（865 行）の 2 つ目が `undeclared`、rc 0 は `Closed` と `Undeclared` の 2 値（894 行）。in-file の歯 `pipe_terminal_land_outcomes_are_the_closed_seven`（1103 行）が 7 値と rc 0 の 2 値を pin する。
  - 終端だけの撃ち直し（`--terminal-only`・§58）は同じ `terminal` を呼ぶので、経路は撃った時点の宣言で選ばれる（`crates/scribe2/src/pipe/cli/step.rs` 59〜106 行）。PR で着地した便は `Landed` の detail が `pr` で `sha:` を持たず（finish.rs 92 行）、撃ち直しは記録の sha を読めずに断る（step.rs 64〜66 行）＝経路 (2) は PR の便に掛からない。
  - 歯の道具箱の台帳 client の見張り（`crates/scribe2-boundary/tests/e2e/main.rs` 414〜424 行・gate-cost.md §37）は、呼ばれた argv を記録して rc 127 で断る。§37 は「終端の close は押す先を宣言した repo の後ろに在るので、実台帳へ届いている歯は 0 本」を前提にしている。本 § でその前提が崩れ、remote を持たない toy repo の着地が全部 close を撃つ。
- 着地が close を撃つようになって動く歯（2026-09-29 の grep による census・deduced）:
  1. 見張りの記録が 0 件であることを測る 3 か所: `crates/scribe2-boundary/tests/e2e/pipe.rs` 1535 行（`e2e_ledger_tripwire_helper_run_to_landed_never_reaches_the_ledger`）・`crates/scribe2-boundary/tests/e2e/pipe/gate.rs` 2284 行（helper `assert_child_measured_the_landed_commit`）・`crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs` 105 行。
  2. `Landed` の最後の detail を着地の `sha:` と読む helper と歯: pipe/land.rs の `landed_detail`（819 行・`detection:` だけを飛ばす）・pipe/gate.rs の `landed_done_detail`（1840 行）・pipe/land.rs 609 行（`assert_already_landed_terminal` の `.pop()`）・detection.rs 306 行（`Landed` の detail の全件が着地の 1 件）。
  3. 道具箱を持たない PATH で着地まで撃つ口: pipe/gate.rs の `systemd_stub`（1333 行・host の PATH を後ろに積む）を使う `crates/scribe2-boundary/tests/e2e/pipe/gate/confine.rs` 432 行の land（rc 0 を期待）。この口の close は host の台帳 client（PATH の `bd`）へ届く。pipe.rs の `shim_path`（1052 行）も host の PATH を積むが、今それで着地まで届く歯は無い（land/rebase.rs 148 行・land.rs 1296 / 1344 行・follow.rs はどれも着地の前で止まる）。`landed_path`（gate.rs 2021 行）は既に道具箱を積んでいる。
  4. `RunDone stage=Landed` を 1 件と数える歯（便 s2-07l.736.20-20260929T085829Z の問い about:write-set で見つかった census の外・着地の `terminal:close:ok` で 2 件になる）: `crates/scribe2-boundary/tests/e2e/pipe/spawn.rs` 761 行・同じ file の `done_count`（873 行）を引く `crates/scribe2-boundary/tests/e2e/pipe/spawn/question.rs` 66 行・`crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs` 142 行。2 件へ直す（着地の 1 件と終端の close の 1 件）。
- 形（番号は done と 1:1）:
  1. **7 値のまま 1 値を替える**: `Terminal::Undeclared` を `Terminal::ClosedWithoutCi` に替える（宣言順の 2 つ目・字面 `closed:no-ci`・rc 0）。`TERMINAL_TOKENS` の 2 つ目も同じ字面にする。doc comment は「remote を持たない repo の便を CI の照合なしで close した」に直す。
  2. **経路 (2)**: `terminal` は宣言を読めた周に `remote` が無ければ、push と CI を撃たずに台帳の close を撃ち、理由は `landed <着地した sha> ci=none` とする（`tip` が `Behind` の周も `tip=` を付けない）。close が通れば `terminal:close:ok` を 1 件記して `ClosedWithoutCi`、落ちれば今の `CloseFailed`（`terminal:<close:failed:…>` 1 件・rc 1）。走らなかった push と CI の段の event は積まない。宣言を読めない周は今のまま（`terminal:unreadable`・close しない・rc 1）。
  3. **理由の尾の書き手は 1 本**: close の理由を組む 1 関数を finish.rs に置く（`pub(in crate::pipe)`・land.rs が `landed_sha` と同じく再輸出）。尾は閉じた 2 値で受ける: CI が success（先端の commit id を任意で持つ）と、CI の照合なし。字面は `landed <sha> ci=success`・`landed <sha> ci=success tip=<先端>`・`landed <sha> ci=none` の 3 形で、経路 (1) の 2 か所の `format!` もこの関数に替える（字面は変わらない）。経路 (3)（§61）も同じ関数を呼ぶ。
  4. **撃ち直し**: `--terminal-only` は変えない。remote を持たない repo の便の撃ち直しは経路 (2) で close し、stdout は `run=<id> terminal=closed:no-ci`。
  5. **歯の道具箱**:
     - 見張りは `close` を 1 語目に持つ呼び出しだけ、argv を記録してから rc 0 で返す（ほかの呼び出しは今のまま rc 127・列の 1 周の `unmeasured reason=ledger` の枝は動かない）。
     - pipe/gate.rs の `systemd_stub` は、host の PATH の代わりに道具箱の PATH（`crate::toolbox_path`）を自分の bin dir の後ろに積む（`landed_path` と同じ形）。pipe.rs の `shim_path` も同じ形にする（着地まで届く歯を後から足した周に host の client へ届かないため）。どちらも自分の bin dir が先頭なので、偽 git と偽 systemd-run の解決は変わらない。
  6. **動く歯を直す**（census の 1〜3）: 見張りの 0 件の 3 か所は「着地の close の 1 件だけ」（argv が `close <bead> --reason landed <sha> ci=none`）に直す。`landed_detail` と `landed_done_detail` は `terminal:` で始まる detail も飛ばす。609 行は末尾の `terminal:` の行を除いた最後の event を読む。306 行は `terminal:` の行を除いた全件を比べる。pipe.rs 682 行の注は `terminal=closed:no-ci` に直す。
  7. **変えないもの**: 経路 (1) の段と字面と event（push・CI・close）・`Unreadable` と残る 5 値の字面と rc・`landed_sha`・`open_pr`（PR の便は終端を撃たない）・台帳の close（`crates/scribe2/src/ledger/mod.rs`）・`--terminal-only` の段の確かめと記録の sha の読み・終端の極性（`TERMINAL_POLARITY`）・usage と help。
- 触らない: finish.rs の `finish` と `land_train` と `PushTip`・`crates/scribe2/src/pipe/cli/step.rs`・vessel 宣言の読み（`terminal_facts`）・FR93（memo の自動の close）と FR90（局面の出力の書き直し）の契機（本 § の close が契機になるが、その側は未実装で、束 `s2-07l.739` の行が持つ）。
- 限界:
  - 経路 (2) の close は CI を見ない（`ci=none` の尾がそのことを記録に残す）。main が赤くなっていても close する。着地の前の主実測（main の verify）は今のまま撃つ。
  - remote の宣言を後から足した repo で、それより前に `ci=none` で閉じた契約は閉じたまま（撃ち直しで経路 (1) へ移らない）。
  - FR50 の push の前の走査（経路 (1)）は本 § の外（vessel-hook.md の publish の配線の行が持つ）。
- 却下:
  - `Undeclared` の字面のまま close する — `terminal=undeclared` が「閉じた」を名乗らず、記録から close の有無を読めない（C10）。
  - 8 つ目の値を足す — 「宣言を読めた上で remote が無い」周は 1 つしか無く、`Undeclared` を残すと到達しない値になる。
  - 経路 (2) で CI の行（`ci-cmd`）を撃つ — remote を持たない repo の commit に forge の CI の run は付かない。
  - 見張りを全ての呼び出しで rc 0 にする — 列の 1 周が「0 件を読めた」へ倒れ、`unmeasured reason=ledger` の枝が測れなくなる（§37 却下）。
  - host の PATH を積む口の側で個々の歯に `--bd` を足す — 口が同じ形のまま残り、後から足す歯が同じ穴を踏む。
- 歯（接頭辞 `pipe_terminal_no_remote_`・`grep -rn pipe_terminal_no_remote_ crates/ docs/` は 0 件〔main e6ca3e19・2026-09-29〕・契約表の nextest の verify 行の filter 語のどれも、この接頭辞で始まる名の部分文字列にならない〔実測〕）:
  - lib（`crates/scribe2/src/pipe/land.rs` の歯の区間）: (a) `pipe_terminal_no_remote_reason_tails_come_from_one_writer`: 理由の関数が 3 形の字面を返し、`ci=none` の形は先端を受けても `tip=` を持たない。既存の `pipe_terminal_land_outcomes_are_the_closed_seven` は、2 つ目を `ClosedWithoutCi` に、rc 0 の 2 値を `closed` と `closed:no-ci` に直す（名は変えない）。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/land.rs`・親の helper を使う）:
    - (b) `pipe_terminal_no_remote_land_closes_with_ci_none`: remote を持たず `ci-cmd` に回数を数える偽 CI を宣言した toy repo の着地が rc 0・stdout の末尾が `terminal=closed:no-ci`・見張りの記録がちょうど 1 件で argv が `close` / bead / `--reason` / `landed <着地した sha> ci=none`・偽 CI の呼び出し 0・repo の remote 0・`Landed` の着地の detail の後ろが `terminal:close:ok` の 1 件だけ。
    - (c) `pipe_terminal_no_remote_train_closes_each_run_with_its_own_sha`: remote を持たない repo の列の着地（2 本）で、見張りの記録が 2 件、各 argv の理由が自分の着地 commit の `landed <sha> ci=none` で `tip=` を持たない。
    - (d) `pipe_terminal_no_remote_unreadable_declaration_closes_nothing_until_refired`: `show HEAD:.vessel.toml` だけを落とす偽 git（`land_once_with_git_shim`）の着地が rc 1・stdout に `terminal=unreadable`・見張りの記録 0 件・`terminal:unreadable` 1 件。偽 git を外して `pipe land --terminal-only` を撃つと rc 0・stdout が `run=<id> terminal=closed:no-ci`・見張りの記録がちょうど 1 件で理由が `landed <sha> ci=none`。
    - (e) `pipe_terminal_no_remote_pr_landed_run_writes_nothing`: remote と偽 CI と偽 bd を宣言した repo（`fake_terminal`）で `--pr-cmd` の着地が rc 0・偽 remote の ref 0・偽 CI の呼び出し 0・偽 bd の log 無し・見張りの記録 0 件。同じ便への `--terminal-only` は rc 1 で断られ、書きは 0 のまま（AC65 (b)・base でも緑・同じ file の (b)〜(d) が base で赤い）。
  - 変異の A/B（条件 1 つに歯 1 本）: remote の無い周を今の早期 return に戻す → (b)(c)(d の 2 周目) が落ちる。remote の無い周も push を撃つ → (b) が落ちる（`terminal:push:failed:git`）。`ci=none` の周に `Behind` の `tip=` を付ける → (c) が落ちる。宣言を読めない周を remote の無い周と同じに読む → (d) の 1 周目が落ちる。PR の便にも終端を撃つ → (e) が落ちる。
  - base で RED の理由: (a) は base に無い理由の関数と `ClosedWithoutCi` を引く compile error（land.rs は base に在る file の歯の区間）。(b)(c)(d) は base が remote の無い周に close を撃たない（見張りの記録 0 件・`terminal=undeclared`）ので assert で RED（機能不在）。census の 1 の 3 か所も同じ理由で base で赤い。census の 2 の helper の直しは base の記録（`terminal:` の行が無い）でも同じ値を返す。

## 60. land の終端と pipe retire が共有する問いの部品 — CI の結果を 4 値で読む・PR の merge の commit を forge に問う・台帳の 1 件が close の理由を持つ（契約表の行 bo・FR96・ADR-0094 の経路 (3) の部品）

やさしく言うと: PR で着地した便を閉じる口（§61）が要る「問い」を先に揃える。CI の結果は「成功・失敗・まだ無い・測れない」の 4 つに分け、PR が merge されたかとその commit を forge に 1 回で問い、台帳の読みが「どういう理由で閉じたか」も持つ。どれも今の振る舞いは変えない。

- 何が起きているか（main e6ca3e19・verified）:
  - CI の読み `ci_now`（`crates/scribe2/src/fleet/wait.rs` 237 行）は 3 形を返す: `Some(Failure)`・`Some(Success)`・`None`。`None` は run が 0 本・まだ走っている・行を撃てない・JSON を読めない の 4 つを 1 つに畳む（228〜236 行の doc）。land の終端はそれで足りる（`None` は上限まで待った後の `ci:unmeasurable`）。FR96 は「結果がまだ無い周（ci-not-success）」と「問いを撃てない周（unmeasured）」を分けるので、`None` のままでは足りない。
  - forge へ PR を問う口は src に無い（`gh pr` の起動は `crates/` の src に 0 件・PR を作る seam は `--pr-cmd` の 1 行だけ〔finish.rs 57 行〕）。起動の記述（`crate::invocation::Invocation`）と lib の歯の stub（`crates/scribe2/src/pipe/mod.rs` の fixture の module の `Stub`）は在り、`ci_now` の歯（wait.rs 984 行）が同じ形を使う。
  - 台帳の 1 件 `Issue`（`crates/scribe2/src/seat/ledger.rs` 69 行）は close の理由を持たない。台帳の JSON（`bd list --all --json`）の要素は `close_reason` を持つ（2026-09-29 に閉じた bead 1 件で実測: `landed <40 桁の sha> ci=success`）。構築点は 4 か所（seat/ledger.rs 113 行・`crates/scribe2/src/hook/graph_guard.rs` 572 行・`crates/scribe2/src/ledger/form.rs` 357 行〔歯〕・`crates/scribe2/src/pipe/dispatch/precheck.rs` 458 行〔歯〕・2026-09-29 の grep）。
- 形（番号は done と 1:1）:
  1. **CI の 4 値**: wait.rs に `ci_read`（同じ引数・子 process 1 回）と閉じた 4 値（success・failure・pending〔run が 0 本か、落ちた run が無く走っている run が在る〕・unmeasured〔行を撃てない・rc が 0 でない・JSON を読めない〕）を置く。判定の順は `ci_now` と同じ（schedule の run を外す → 落ちた run を先に見る → 全部完了なら success）。`ci_now` は `ci_read` の写し（success → `Some(Success)`・failure → `Some(Failure)`・残る 2 値 → `None`）にして、外形と呼び手は変えない。fleet の module の再輸出に 2 名を足す。
  2. **PR の merge の問い**: wait.rs に `pr_merge`（repo と branch の名を受ける・子 process 1 回）と閉じた 3 値（merged〔merge の commit id〕・not-merged・unmeasured）を置く。撃つ行は `gh pr view <branch> --json state,mergeCommit`（cwd は repo・shell を通さない・`ci_now` と同じ起動の記述）。`state` が `MERGED` で `mergeCommit.oid` が 40 桁の 16 進なら merged、`state` が他の文字列なら not-merged、起動の失敗・rc が 0 でない・JSON を読めない・`MERGED` なのに oid が無いか形が違う周は unmeasured。forge の既定の repo の選び方は gh に任せる（`--pr-cmd` の gh と同じ解き方・`--repo` を渡さない）。
  3. **close の理由**: `Issue` に `close_reason`（文字列・欄が無ければ空）を足し、`issues_of` が読む。構築点の 3 か所は空で足す。
  4. **変えないもの**: `ci_now` の外形と 3 形・land の終端の CI の待ちと照合・`CiRun`・台帳の読みの引数（`BD_ARGS`）と待ち上限・`issues_of` の必須 2 key。
- 触らない: land の終端（§59）・pipe retire（§61 が呼ぶ）・rules 行。
- 限界: `pr_merge` は forge の CLI が gh であることを前提にする（`ci-cmd` のように宣言で替えられない）。forge の key を vessel 宣言に足すには新しい ADR が要る（ADR-0094 は問う字面を設計へ委ねた）。merge の commit を持たない merge（forge の設定による）は unmeasured になる。
- 却下:
  - `ci_now` の `None` を割って外形を変える — 呼び手（`Completion::CiResult` の待ちと終端）が全部動く。
  - PR の問いを `--repo <owner>/<name>` で撃つ — 宣言の remote の URL から owner/name を導く読み手が要り、`--pr-cmd` の gh と違う repo を問いうる。宣言と別の remote へ出した PR は、git の祖先の照合が not-ancestor で閉じない（ADR-0094）。
  - 台帳の 1 件を `bd show` で読む — 読みの口が 2 本になる（`read_ledger` の 1 本に寄せる）。
- 歯（接頭辞 `retire_parts_`・`grep -rn retire_parts_ crates/ docs/` は 0 件〔main e6ca3e19・2026-09-29〕・filter 語の衝突 0〔実測〕）:
  - lib（wait.rs の歯の区間・`Stub` で子 process を撃たない）:
    - (a) `retire_parts_ci_read_splits_pending_from_unmeasured`: 完了の success・failure・走っている run・空の配列・schedule だけ・rc 1・JSON でない の 7 つが success・failure・pending・pending・pending・unmeasured・unmeasured で、`ci_now` は同じ 7 つに `Some(Success)`・`Some(Failure)`・`None`×5 を返す。
    - (b) `retire_parts_pr_merge_reads_the_state_and_the_merge_commit`: `MERGED` と 40 桁の oid は merged、`OPEN` と `CLOSED` は not-merged、`MERGED` で `mergeCommit` が null・oid が 39 桁・rc 1・JSON でない の 4 つは unmeasured。stub の呼び出しは 1 回で、program は `gh`・引数は `pr view <branch> --json state,mergeCommit`・cwd は repo。
  - lib（seat/ledger.rs の歯の区間）: (c) `retire_parts_issue_reads_the_close_reason`: `close_reason` を持つ要素はその字面、持たない要素は空。
  - base で RED の理由: 3 本とも base に無い関数と欄を引く compile error（wait.rs と seat/ledger.rs は base に在る file の歯の区間）。

## 61. PR で着地した便を pipe retire が照合してから close し、worktree を畳む — worktree の確かめ・畳むだけの指定・閉じ済みの契約・merge の commit・remote の main の先端の祖先・先端の CI の順に問い、閉じた 6 語で断る（契約表の行 bp・FR96・AC65 (c)〜(e)・ADR-0094 の経路 (3)）

やさしく言うと: PR で着地した便は、今は merge の後に人が worktree を畳み（`pipe retire`）、台帳は手で閉じている。畳む口が forge と git に「merge されたか・その commit が remote の main に載ったか・main の CI が緑か」を問い、全部通った時だけ台帳を閉じてから畳む。通らない周は何も書かずに理由の 1 語を出し、同じ口で撃ち直せる。merge されずに閉じた PR などは「畳むだけ」の指定で出口を持つ。

- 何が起きているか（main e6ca3e19・verified）:
  - `retire`（`crates/scribe2/src/pipe/retire.rs` 56 行）は worktree が在ること（58〜61 行）と clean なこと（62〜65 行）を確かめ、`retire_worktree` で可逆に move し（66〜70 行・move の失敗は rc 2）、段を動かさずに `detail=retired` を 1 件記す（71〜84 行）。doc comment（42〜55 行）は「`detail=pr` を前提にしない」「merge 済みかは人が確かめる（forge へ問い合わせない）」と書く。pipeline.md §5.4 も同じ読み。
  - 入口 `retire_run`（`crates/scribe2/src/pipe/cli/step.rs` 417 行）は段 Landed・Failed・Gated・Stopped・Reviewed を受け、manifest を受け取らない（`crates/scribe2/src/pipe/cli.rs` 445 行）。受ける flag は置き場の 3 つと `--run` と道具の 4 つ（`crates/scribe2/src/pipe/cli/args.rs` 154 行・`--bd` を含む）。
  - PR の便は `Landed` の `RunDone` の detail が `pr` の 1 件だけを持つ（finish.rs 92 行）。PR の branch の名は `branch_name`（`crates/scribe2/src/pipe/mod.rs` 302 行・`--pr-cmd` の `{branch}` の穴と同じ 1 本）。
  - 台帳の読みは `read_ledger`（seat/ledger.rs 152 行・待ち上限は rules 行 `seat.ledger_timeout_s` を `timeout_of` で読む）、close は `crate::ledger::close`（`crates/scribe2/src/ledger/mod.rs` 88 行）。
  - PR の便の retire の e2e は 2 本（`crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs` 7 行と 47 行）で、toy repo は remote を持たない。本 § の後は経路 (3) で unmeasured になるので、畳むだけの指定を付けた撃ちに書き直す。2 本が撃つ helper `retire_once` は親の `crates/scribe2-boundary/tests/e2e/pipe/land.rs`（1667 行）に在り、他の畳みの歯（`pipe_retire_failed_any_detail_` ほか）も使う（main 61ddebd0・verified）。
  - 47 行の歯 `pipe_retire_refuses_unless_landed_and_clean` が測るのは rc・worktree の在否・汚れの在否・event の件数・`retired/<id>` の在否だけで、stdout と stderr の字は pin しない（本文の注と assert・verified）。(a) の段が Gated の便は形 1 の「今のまま断る」で rc 1、(b) の Landed で dirty な worktree は形 2 の `worktree-unready`（`run=<id> retire=worktree-unready`・rc 1・event 0・worktree を動かさない）で断られ、どちらも `--fold-only` を付けても assert は同じ値で通る。拭った後と段を解いた後は形 3 の畳むだけで rc 0。
  - forge と CI の問いの部品（§60 の行 bo）は `crates/scribe2/src/fleet/wait.rs` の pub な `pr_merge`（335 行）・`PrMerge`（313 行）・`ci_read`（267 行）・`CiRead`（233 行）で、`crates/scribe2/src/fleet/mod.rs` 22 行の `pub use wait::{…}` が再輸出する＝retire.rs から `crate::fleet::` の path で届き、`fleet/mod.rs` と `wait.rs` は変えない（main 61ddebd0・verified）。
- 判定の順（PR の便の周）: 経路の選び → worktree → 畳むだけの指定 → 台帳（閉じ済み）→ 宣言の remote → forge（merge の commit）→ git（先端・祖先）→ 先端の CI → close → 畳み。
- 形（番号は done と 1:1）:
  1. **経路の選び**: 便の段が `Landed` で、その `Landed` の `RunDone` の detail が `pr` の便だけが経路 (3) を通る。他の対象（squash の形で着地して move だけが落ちた便・Failed・Gated の FAIL・Stopped・Reviewed の FAIL）は今のまま畳むだけで、forge に問わず台帳に書かない（断りの字面と rc も今のまま）。
  2. **worktree**: worktree が無いか clean でない周は `worktree-unready`（git と forge にも台帳にも問わない）。
  3. **畳むだけの指定**: `--fold-only`（値なし・`ALLOWED_RETIRE` に足す）を付けた周は、照合も close もせずに可逆な move で畳み、契約は開いたまま残す。PR の便でない対象では何も変えない。
  4. **閉じ済みの契約**: `--bd`（無ければ既定名）で台帳を 1 回読み（待ち上限は rules 行）、便の bead が closed で `close_reason` の頭の語が `landed`（`CLOSE_REASON`）なら、照合も close も撃たずに畳む。読めない周・待ち上限の行が無い周は `unmeasured`。
  5. **宣言の remote**: `terminal_facts` が読めない周と `remote` が無い周は `unmeasured`（forge に問わない）。
  6. **forge**: `pr_merge`（§60）を便の branch の名で撃つ。not-merged は `not-merged`、unmeasured は `unmeasured`、merged なら merge の commit id を持って進む。
  7. **git**: `git ls-remote <remote> refs/heads/main` で先端の commit id を読み、`git fetch --no-tags --no-write-fetch-head <remote> refs/heads/main` で object を取る。どちらかが落ちた周・先端の object が無い周は `unmeasured`。merge の commit の object が無い周と、`git merge-base --is-ancestor <merge> <先端>` が rc 1 の周は `not-ancestor`、それ以外の rc は `unmeasured`。
  8. **先端の CI**: `ci_read`（§60）を先端の commit id と宣言の `ci-cmd` で 1 回だけ撃つ（待たない）。failure と pending は `ci-not-success`、unmeasured は `unmeasured`。
  9. **close**: 理由は §59 形 3 の 1 関数で組む（merge の commit id・CI が success・先端が merge の commit と違う周だけ先端の commit id）。close が落ちた周は `unwritten`（畳まない）。通った周は `RunDone stage=Landed detail=terminal:close:ok` を 1 件記す。
  10. **畳み**: close の後に `retire_worktree` で move し、`detail=retired` を 1 件記す（段は `Landed` のまま）。move が落ちた周は `worktree-unready`（close は残り、撃ち直しは形 4 で畳むだけ）。
  11. **出力**: 通らない周は stdout の 1 行 `run=<id> retire=<語>`・rc 1・event も台帳の書きも 0（close の後の move の失敗だけは形 9 の 1 件が残る）。語は閉じた 6 語（宣言順 worktree-unready・not-merged・not-ancestor・ci-not-success・unmeasured・unwritten）の enum と const slice で持つ。close して畳んだ周の stdout は `run=<id> retired=<畳んだ先> close=ok`、畳むだけの周（形 3・形 4）は今の `run=<id> retired=<畳んだ先>`。
  12. **入口**: `retire_run` は manifest を受け（cli.rs の 1 か所）、`--bd` と `--fold-only` を `Retire` に運ぶ。usage と help の字面は変えない（verb ごとの flag を列挙しない形のまま）。
  13. **既存の歯の書き直し**: retire.rs（e2e）7 行と 47 行の 2 本の PR の便の撃ちに `--fold-only` を付ける（期待は変えない）。撃ちは retire.rs の中に `--fold-only` を足した撃ちの helper を 1 本置いて使い、親 land.rs の `retire_once` は変えない（他の畳みの歯が使う・land.rs は write-set の外）。
  14. **理由の書き手を land の外へ見せる**: §59 形 3 は理由の 1 関数を `pub(in crate::pipe)` で置き land.rs が再輸出すると決めたが、行 bn の着地（main 52cce30a）で `crates/scribe2/src/pipe/land/finish.rs` の `close_reason` と `CloseTail` は `pub(super)`、`CLOSE_REASON` は私有のまま残り、land の外（retire.rs）から見えない（便 s2-07l.736.22-20260929T101027Z の契約の審査 FAIL 2026-09-29T10:11Z）。本行が 3 つを `pub(in crate::pipe)` に広げ、`crates/scribe2/src/pipe/land.rs` の `pub(in crate::pipe) use finish::{…}` の列に足す（中身と字面は変えない）。
- 触らない: 他の対象の畳み方と断り・`retire_worktree`・`land` の `--pr-cmd`（PR の便は終端を撃たない）・起票の門（席の道具の呼び出しの着地の形の close は今のまま断る・器の子 process の close は門を通らない）・polarity の一覧（retire の 6 語は close を止める判定だが、畳む口の結末として一覧に行を足さない＝限界）・role の権能（`pipe retire` は launch のまま）。
- 限界:
  - FR96 の「close した周に memo の自動の close（FR93）を撃つ」と、ADR-0094 の「局面の出力の書き直し（FR90）の契機」は本 § に無い。FR93 と FR90 の設計と実装は束 `s2-07l.739` の行が持ち、その行が経路 (2) と (3) の close の後ろに契機を足す。
  - 台帳を読めない周の `unmeasured` は、FR96 の unmeasured の定義（git か forge の問いを撃てない周・key remote を持たないか読めない周）の外の周を同じ語に入れている（SRS の字面の見直しの候補）。
  - forge の既定の repo は gh が選ぶ。remote が複数在り gh の既定が決まっていない repo は、gh が断って `unmeasured`。宣言と別の remote へ出した PR は、merge されていても `not-ancestor`（出口は `--fold-only`）。
  - `--pr-cmd` が `{branch}` と別の名の branch を PR の head にした便は、PR が見つからず `unmeasured`（出口は `--fold-only`）。
  - 本 § の前に畳まれた PR の便は射程の外（ADR-0094 CSQ-N8）。
- 却下:
  - 経路 (3) を land の撃ち直し（`--terminal-only`）に置く — land は PR の便の記録の sha を持たず、畳みと close の順（close の後に畳む）を 1 つの口で持てない。
  - CI を上限まで待つ — 撃ち直せる口で待つと、手の撃ちが上限の分だけ止まる（結果がまだ無い周は `ci-not-success` で撃ち直す）。
  - 台帳の閉じ済みを event log（`terminal:close:ok`）で判じる — 別の口で閉じた契約（同じ bead の別の便など）を二度閉じる。
  - 失敗の周に event を記す — FR96 は通らない周の書きを 0 にする（撃ち直しの記録は stdout の 1 行と rc）。
- 見積: retire.rs 104 → 約 290・step.rs 431 → 約 450・args.rs と cli.rs は 1 行ずつ・e2e の retire.rs 746 → 約 1080。diff は歯込みで約 540 行（L）。
- 歯（接頭辞 `pr_retire_`・`grep -rn pr_retire_ crates/ docs/` は 0 件〔main e6ca3e19・2026-09-29〕・filter 語の衝突 0〔実測・既存の filter 語 `pipe_retire_` は `pr_retire_` の部分文字列でない〕・`crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs` に置き、親の helper を使う）:
  - fixture: PR の便（`landed_pr`）・偽 remote（bare repo・宣言の `remote`）と宣言の `ci-cmd` の偽 CI（答えの JSON を file で替える）・PATH の先頭の偽 gh（`pr view` の argv を記録し、答えの JSON と rc を file で替える・道具箱の PATH を後ろに積む）・`--bd` の偽 client（`close` は argv を記録して rc を file で替え、通った周は偽の台帳の JSON を「closed・close_reason」に書き換える／ほかは偽の台帳の JSON を返す）。merge の commit は repo で便の branch と main から作り、偽 remote の main へ path の URL で押す（先端でない fixture はその上に 1 commit 足して押す）。
  - (a) `pr_retire_closes_then_folds_when_the_merge_is_under_a_green_tip`: merge の commit が先端そのものの fixture と先端でない fixture の 2 周。rc 0・stdout が `run=<id> retired=<畳んだ先> close=ok`・偽 client の close がちょうど 1 回で理由が `landed <merge> ci=success`（先端でない周は ` tip=<先端>` を持つ）・worktree が `retired/<id>` へ移り branch は残る・`Landed` の後ろが `terminal:close:ok` と `retired` の 2 件・偽 gh の argv が `pr view <branch> --json state,mergeCommit`・偽 CI の argv が先端の commit id。
  - (b) `pr_retire_refuses_with_one_closed_word_and_writes_nothing`: 9 つの fixture（worktree が clean でない・not-merged・祖先でない・CI が failure・CI の結果が空・宣言に remote が無い・偽 gh が rc 1・close が rc 3・偽の台帳が rc 1）が順に `worktree-unready`・`not-merged`・`not-ancestor`・`ci-not-success`・`ci-not-success`・`unmeasured`・`unmeasured`・`unwritten`・`unmeasured`。各周で rc 1・stdout がちょうど `run=<id> retire=<語>`・worktree が元の場所に在る・event の数が変わらない・偽の台帳の JSON が変わらない。worktree-unready と remote の無い周は偽 gh の呼び出し 0。worktree-unready の fixture を clean に戻して success の答えにした撃ち直しが rc 0 で close する。
  - (c) `pr_retire_fold_only_and_closed_contracts_fold_without_checks`: not-merged の fixture に `--fold-only` を付けた撃ちが rc 0・畳む・close 0・偽 gh の呼び出し 0。`retired/<id>` に先に dir を置いた success の fixture の撃ちが rc 1 の `worktree-unready`・close 1 回（偽の台帳が closed・landed）・worktree が元の場所に在る。dir を退けて撃ち直すと rc 0 で畳み、close も偽 gh も増えない。
  - 変異の A/B（判定の順・条件 1 つに歯 1 本）: worktree の確かめを forge の後ろへ移す → (b) の worktree-unready の周で偽 gh が撃たれる。閉じ済みの確かめを外す → (c) の撃ち直しで close が 2 回。祖先の照合を外す → (b) の not-ancestor の周で close する。pending を success に倒す → (b) の空の結果の周で close する。先端の違いを見ずに `tip=` を付けない → (a) の 2 周目が落ちる。close の前に畳む → (b) の unwritten の周で worktree が消える。
  - base で RED の理由: (a)(c) は base の retire が forge に問わず close を撃たず `--fold-only` を未知の flag として断るので assert で RED、(b) は base が PR の便を畳んで rc 0 を返すので RED（機能不在）。書き直す既存の 2 本は `--fold-only` を base が断るので base で赤い。

## 62. 入れ子の source の根を vessel 宣言の任意 key crate-roots で足し、追随の再 gate の要否・着地後の検出線の面・受付の上限の余地が、宣言した根の下の crate を crates/ の直下の crate と同じに読む（契約表の行 bq・FR34・FR48・NFR4）

やさしく言うと: 器は「crate は repo の根の crates/ の直下に在る」と決めてかかって、いくつかの検査の範囲を決めている。根の workspace の member に入れ子の dir の crate を持つ repo（隣の project が持つ形）では、その crate の変更が 3 つの検査から黙って外れる。main がその crate だけで動いた周に、ほかの便の再 gate を省く。着地した便の検出線も撃たない。受付も 1 file 1500 行と core の合計の上限を測らない。repo が「crate の根はここにも在る」と vessel 宣言に書けば、器が同じ規則でその下も測るようにする。

- 何が起きているか（main b028af03・verified）:
  - 検出線の面は `crates/scribe2/src/pipe/land.rs` 151 行の `DETECTION_SCOPE`（`crates/`・`Cargo.toml`・`Cargo.lock`・`rules/`・`.vessel.toml` の閉じた集合）で、157 行の `detection_needed` と 162 行の `in_face` が repo の根からの相対 path を接頭辞（末尾 `/` の項目）か完全一致で照らす。呼び手は 2 つ。
    - 追随の再 gate の省き: land.rs 672 行の `regate_skippable`（641 行から呼ぶ）。`<base>..<main>` の path が面に 1 つも触れない周に、前周の PASS を引き継いで lens を撃たない。
    - 着地後の検出線: `crates/scribe2/src/pipe/land/detection.rs` 166 行の `touches_scope`（136 行から呼ぶ）。触れない周は `outside-scope` の skip record を書いて撃たない。
  - 受付の上限の余地は `crates/scribe2/src/pipe/declaration/write_set.rs` 256 行の `headroom_shortfalls` が測る。測る file の集合は 294 行の `core_of` が `Some` の file だけで、`core_of` は接頭辞 `crates/` と「crate の名に `/` が無い」を要求する。R-C4-2 の 1 file の上限と R-C4-1 の core の合計の両方が、この集合の外の file を数えない。呼び手は `crates/scribe2/src/pipe/cli/intake/refusal.rs` 125 行の `exclude_cap_shortfall` の 1 つ（128 行で `Caps` を組み、168 行で呼ぶ）。`headroom_shortfalls` の歯の呼び出しは write_set.rs の歯の区間に約 30 か所、`Caps` の struct の字面は 11 か所（本体 1・歯 10）在る。
  - 入れ子の dir の crate の path（例 nest/crates/toy/src/a.rs）は、上の 2 つの読み手のどちらでも crate の file と読まれない。どちらの判定も失敗を出さず、黙って「面の外」「測る集合の外」に倒れる。
  - vessel 宣言の任意 key の置き方は `crates/scribe2/src/pipe/declaration/optional_keys.rs` の頭の注（1〜5 行）が決める。key の名と読み手と key の列（13 行の `DECLARED_KEYS`・63 行の `OPTIONAL_KEYS`）の 1 行ずつと外へ渡す口を置き、`Declared`（`crates/scribe2/src/pipe/declaration.rs` 281 行）の欄と `parse` の読みの 1 行（549 行の `Self` の字面）を親へ足す。配列の値は親の 751 行の `list_of` が読む。path の種別の key 3 本（`crates/scribe2/src/pipe/declaration/path_kinds.rs`・ADR-0047）が同じ族の先例で、子 module に key と読み手と値の型を置く。
  - 受付の材料の `TableFacts`（optional_keys.rs 205 行）は HEAD の宣言から組み（228 行の `table_facts_named` の中の字面 1 か所）、`crates/scribe2/src/pipe/cli/intake.rs` の `Materials` の欄 `facts` として `exclude_cap_shortfall` へ届く。`TableFacts` の struct の字面は本体 1 か所（optional_keys.rs 228 行）と歯 1 か所（intake.rs 1261 行・歯の区間）の 2 か所。
  - HEAD の宣言の読み手は declaration.rs 365 行の `head_declaration`（宣言 file が無い周は `None`・読めない周は `Some(Err)`）。
- 形（番号は done と 1:1）:
  1. **key**: vessel 宣言の任意 key `crate-roots` を 1 つ足す。値は repo 相対の dir の配列で、各項目は末尾が `/`。書かない宣言と宣言 file を持たない repo は今と 1 行も変わらない。schema は 1 のまま。
  2. **足すだけ**: 固定の根 `crates/` は常に在り、key はそれに足す根を並べる（置き換えない）。宣言で器自身の検査の範囲を狭められないようにするためで、path の種別の key（置き換える）と違う。
  3. **項目の検査**: 空の項目・末尾が `/` でない項目・絶対 path・home の短縮記号・`..` の段・`crates/` と同じか一方が他方の接頭辞になる項目・項目どうしで一方が他方の接頭辞になる項目は、key と行番号を名指す不備（DeclError）にする。1 つの path が 2 つの根に入ると crate の名が 2 通りに読めるためである。書いた空配列は既存の配列の層が断る。
  4. **1 つの関数**: 根の列と repo 相対の path から「根・crate の名・crate の中の残り」を返すか、どの根の crate にも入らないなら無しを返す pure な 1 関数を、行 bq の write-set の `+` の file（declaration の子 module・path の種別の子と同じ置き方）に置く。crate の名は根の直後の 1 段で、空でなく `/` を持たない。残りは空でない。根の列の型と関数は親の `declaration.rs` が再輸出し（`path_kinds` と同じ）、行 br の導出の読み手も同じ 1 関数を通る。同じ子に、HEAD の宣言から根の列を読む口を置く。`head_declaration` は親の私有の関数なので子から呼び、結果を閉じた 3 値（宣言した根・固定の根だけ・読めない）で返す（`PathKinds` の `read_at_head` と同じ形）。
  5. **検出線の面**: `DETECTION_SCOPE` の値と `detection_needed` は変えない（既定の面・既存の歯の母集団）。根の列を受ける読み手を 1 本足し、面は `DETECTION_SCOPE` の照らしと「宣言した根のどれかの crate の中の path」の和にする。`regate_skippable` と `touches_scope` は land.rs の同じ 1 本（形 4 の HEAD の口の閉じた 3 値と path の列から「面に触れるか」を返す）を通る。`regate_skippable` はその否定を返す。
  6. **読めない周**: 宣言が在って読めない周は、形 5 の 1 本が「触れる」を返す。`regate_skippable` は偽（撃ち直す）、`touches_scope` は真（撃つ）になり、どちらも既存の「diff を読めない周」と同じ fail-closed の極性になる。宣言 file が無い周は固定の根だけ。
  7. **上限の余地**: `core_of` を根の列を受ける形にし、測る集合を「根のどれかの crate の src の下の file」にする。core の合計は crate ごと（`<根><crate>/src` ごと）で、今の `crates/<crate>/src` ごとの束ね方と同じ。`headroom_shortfalls` の signature と `Caps` は変えない（固定の根で呼ぶ）。根の列を受ける呼び口を 1 本足して親の `declaration.rs` が再輸出し、`exclude_cap_shortfall` はそちらを `materials.facts` の根の列で呼ぶ。約 30 か所の歯の呼び出しと 11 か所の `Caps` の字面を書き直さないためである。
  8. **受付の材料**: `TableFacts` に根の列の欄を 1 つ足し、`table_facts_named` が宣言から埋める。歯の区間の字面 1 か所（intake.rs 1261 行）には固定の根だけの値を足す（retroactive の札）。
  9. **外へ出さない**: doctor の行・stdout の字面・event の key は足さない。宣言の key の名 `crate-roots` だけが跨版の面になる。
  10. **並走しない**: dispatcher.md の行 ai・ak も `declaration.rs` と `optional_keys.rs` を触るので、その 2 行と並走させない（順は台帳の依存で表す）。
- 触らない: 固定の根 `crates/` と `DETECTION_SCOPE` の値・`Caps` の欄・閉包と歯の導出と名の衝突の予測（行 br）・`cargo xtask check` の core-lines と file-lines（器の repo の門・消費側の repo の門は消費側の xtask が持つ）・rules 行の値。
- 限界:
  - 入れ子の workspace の根の manifest（例 nest/Cargo.toml）は根の crate の中の path でないので、検出線の面に入らない。根の workspace の `Cargo.toml` と `Cargo.lock` は今どおり面に入る。
  - crate の名は dir の名と読む（今の `crates/<crate>` と同じ前提）。package の名が dir の名と違う crate は、今と同じく導出の `-p` を誤る（行 br の限界）。
- 却下:
  - Cargo の workspace の members から根を導く案。Cargo の repo にしか効かず、入れ子の表を持つ TOML の新しい読み手も要る。宣言なら言語に依らない。
  - 根を rules 行にする案。pipeline.md §30 は `DETECTION_SCOPE` を「値でなく閉じた path の集合」として rules 行にしないと決めた。根は閾値でなく repo の形なので、repo が持つ宣言に置く。閉じた集合（`DETECTION_SCOPE` の値）は code のまま変えない。
  - 宣言の根で固定の根を置き換える案。宣言 1 行で器自身の crate の検査を外せてしまう。
  - ADR を書く案。path の種別の key（ADR-0047）と同じ族で、宣言の任意 key を 1 つ足すだけ（schema 1 のまま・key の不在は今と同じ・読むのは器の同じ binary だけ）なので、bead の notes に 3 行で残す。
- 見積: 新しい子 module 約 210（本体約 110・歯約 100）・declaration.rs と optional_keys.rs 各約 8・write_set.rs 約 60（歯 (d) を含む）・land.rs 約 56（歯 (f) を含む）・detection.rs 約 40（歯 (g) と歯の区間を含む）・refusal.rs 約 3。e2e 3 file で約 200。diff は歯込みで約 500 行（L）。
- 歯（接頭辞は lib が `declaration_crate_roots_`、e2e が `pipe_intake_crate_roots_`・`pipe_land_crate_roots_`・`pipe_landed_detection_crate_roots_`。`grep -rn crate_roots crates/ docs/` は 0 件〔main b028af03・2026-09-30〕。e2e は既存の file に置く）:
  - lib `declaration_crate_roots_`（(a)(b)(c)(e) は行 bq の `+` の file の歯の区間、(d) は `write_set.rs`・(f) は `land.rs` の既存の歯の区間、(g) は `land/detection.rs` に足す歯の区間）:
    - (a) key が無い宣言は固定の根だけ。2 項目の key は固定の根 + 2 つ。形 3 の不備 7 形（空・末尾 `/` 無し・絶対 path・home の短縮記号・`..`・`crates/` と重なる・項目どうしで重なる）がそれぞれ key と行番号を名指して断られる。
    - (b) 形 4 の関数の表: 宣言した根の下の nest/crates/toy/src/a.rs は根・`toy`・src/a.rs、宣言しない repo では無し、crates/toy/src/a.rs は宣言の有無に依らず今と同じ、根の直下の file（nest/crates/README.md）と crate の名の無い path は無し。
    - (c) 形 5 の面: 宣言した根の下の path だけの列は真、宣言しない列は偽、`DETECTION_SCOPE` の既存の表は根の列に依らず同じ値。
    - (d) 形 7 の余地（`write_set.rs` の歯の区間）: 宣言した根の下の 1 file が 1 file の上限と core の合計の両方で名指される。宣言しない根の列では名指されない。
    - (e) 形 4 の HEAD の口が、宣言 file の無い repo で固定の根だけ、key を持つ宣言で宣言した根、壊した宣言で読めないを返す（使い捨ての git repo）。形 5 の 1 本が読めない 3 値の周に、面の外の path だけの列でも「触れる」を返す。
    - (f) 形 6 の呼び手の配線（`land.rs` の歯の区間）: 使い捨ての git repo の 1 つ目の commit に、key の値が不備（絶対 path の項目）の宣言を置き、2 つ目の commit で面の外の `notes/` の file だけを足す（HEAD は 2 つ目・宣言は動かない）。`regate_skippable` を repo と 1 つ目と 2 つ目の sha で呼ぶと偽（撃ち直す）。同じ 2 commit で宣言だけを key の無い形にした repo では真（今の振る舞いの対照）。
    - (g) 形 6 の呼び手の配線（`land/detection.rs` の末尾に歯の区間を足す）: (f) と同じ repo で `touches_scope` を repo と 1 つ目（親）と 2 つ目の sha で呼ぶと真（撃つ）。key の無い形の対照では偽。
    - (f)(g) は、呼び手が読めない周を固定の根だけに倒す実装を落とす（形 5 の 1 本を直撃する (e) だけでは、呼び手の配線は測れない）。宣言の変化は面の中（`.vessel.toml`）なので、宣言を 2 つ目の commit で壊す形は面に触れて空虚になる。そのため宣言は 1 つ目の commit に置く。
  - 置き方の約束（gate の FAIL 2026-09-30 の直し）:
    - `+` の file（`crate_roots.rs`）は本体も歯も `WriteSetItem` を名指さない。契約表の行 ar（§43）は `touches` に `WriteSetItem` を持つので、その変種を組む file が増えると行 ar の閉包が広がり、現物の契約表の歯（`contract_names_declared_real_table_has_zero_findings` と `contract_closure_ext_real_table_has_zero_findings`）が write-set-incomplete で赤になる。`WriteSetItem` を組む余地の歯は、行 ar の write-set に既に在る `write_set.rs` に置く。
    - 不備の 7 形の歯の fixture のうち home の短縮記号の字は、`concat!` で `~` と `/` を割って書く（`cargo xtask check` の paths-clean が tracked な file の中の private path の形を名指す）。
  - e2e `pipe_intake_crate_roots_`（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）: 本体 1000 行の file を nest/crates/toy/src/heavy.rs に持つ base。key で nest/crates/ を宣言した repo では、その file への M が file の余地で断られ、新規 1 本の S が core の余地で断られる（既存の `pipe_intake_core_headroom_` の 2 本と同じ値）。key の無い repo では同じ契約が通る（今の振る舞いの対照）。
  - e2e `pipe_land_crate_roots_`（`crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs`）: 宣言した repo で main が nest/crates/toy/src/other.rs だけ動いた追随は、再 gate を撃つ（lens の marker が在り、stdout に `regate=skipped` が無い）。key の無い repo で同じ動きは、今どおり `regate=skipped` で引き継ぐ。
  - e2e `pipe_landed_detection_crate_roots_`（`crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs`）: 宣言した repo で着地した diff が nest/crates/toy/src/a.rs だけの便は、検出線の stub を 1 回呼ぶ。key の無い repo の同じ便は、今どおり `outside-scope` の skip record を書く。
  - 変異の A/B（条件 1 つに歯 1 本）: 固定の根を宣言で置き換える → (a) の key の有る表で crates/toy/src/a.rs が無しになる。項目の重なりの検査を外す → (a) の重なりの 2 形が通る。形 5 の 1 本の読めない周を偽にする → (e) が落ちる。`exclude_cap_shortfall` を固定の根の呼び口のままにする → intake の宣言の周が通る。
  - base で RED の理由: lib (a)〜(d) は base に子 module が無く該当 0 本（rc 4）。e2e は base が key を「未知の key」の不備で断り、受付・追随・検出の assert で落ちる（機能不在）。対照の 3 形（key の無い repo）は base で緑で、振る舞いが変わらないことの pin である。

## 63. 閉包と歯の置き場の導出・nextest 行の組み立て・名の衝突の予想が、行 bq の根の列の 1 関数で入れ子の crate を crates/ の直下の crate と同じに読む（契約表の行 br・FR48・FR47）

やさしく言うと: 契約表の検査と受付は、verify の行が名指す歯がどの file に在るかを導き、write-set の外の歯を断る。約束の行からは nextest の行と新しい file の親の宣言 file も導く。この導出も「crate は crates/ の直下」と決めてかかるので、入れ子の dir の crate の歯は見つからず、断るべき行が黙って通る。§62（行 bq）で宣言した根の列を、この導出にも同じ 1 関数で渡す。

- 何が起きているか（main b028af03・verified）:
  - `crates/scribe2/src/pipe/closure.rs` 100 行の `CRATES_DIR`（`crates/`）を `crates/scribe2/src/pipe/closure/derive.rs` が 3 か所で使う。
    - 318 行の `crate_relative`（300 行の `in_crate` と 305 行の `in_scope` が呼ぶ）: verify の `-p <crate>` から歯の置き場の file を探す（163 行の `teeth_places`・172 行）。
    - 396 行の `parent_candidates`: `+` の新しい file の親の宣言 file の候補。`crates/<crate>/src` の直下だけ `lib.rs` と `main.rs` を足す。
    - 565 行の `target_of`: 548 行の `nextest_line` が file の path から `-p <crate>`・`--lib`・`--test <名>` を組む。呼び手は derive.rs 474 行（`promised_inputs` の中）と `crates/scribe2/src/pipe/contract.rs` 479 行の `promised_verify`（482・489 行）。`promised_verify` の呼び手は `crates/scribe2/src/pipe/cli/intake.rs` 476 行。
  - `crates/scribe2/src/pipe/table/check/collide.rs` 23 行に同じ値の `CRATES_DIR` がもう 1 つ在り、193 行の `module_path` が名の衝突の予想（§54）の段を読む。
  - 導出の材料 `Base`（derive.rs 71 行）の struct の字面は、本体 3 か所（collide.rs 85 行・`crates/scribe2/src/pipe/table/check.rs` 494 行・intake.rs 813 行の `base_of`）と歯 10 か所（derive.rs の歯の区間）に在る。
  - 表の検査の文脈 `Context`（`crates/scribe2/src/pipe/table.rs` 544 行）の字面は、本体 2 か所（check.rs 608 行・intake.rs 299 行）と歯 7 か所（check.rs の歯の区間・1008 行は `..closed` の形で字面を足さない）に在る。
  - 入れ子の dir の crate の path は、どの読み手でも crate の外（`-p` 無し・置き場 0 件・module の段 0）と読まれる。`teeth-outside-write-set` の検査は歯を見つけられず、何も言わない。
- 形（番号は done と 1:1）:
  1. **材料**: `Context` と `Base` に根の列の欄を 1 つずつ足す。`Context` の本体の字面 2 か所は `TableFacts` の根の列（§62 形 8）から埋め、`Base` の本体の字面 3 か所は `Context` か `Materials` の根の列から埋める。歯の区間の字面（`Base` 10・`Context` 7）には固定の根だけの値を足す（retroactive の札）。受付の `context` が組む `Context` の根の欄は、受付と preflight の 1 行の検査（`check_table`）が置き場を撃たない（§54 形 6）ので、受付の経路では読み手を持たない。struct の字面を埋めるために `contracts check` と同じ出所（`TableFacts` の根の列）で埋める（歯では測れない・限界）。受付の経路で根を読むのは、判定の関数の `base_of` が組む `Base`（Declared 行の歯の検査と Derived 行の導出）と約束の行の `promised_verify` の呼び手である。
  2. **1 つの関数**: `crate_relative`・`parent_candidates`・`target_of`・collide.rs の `module_path` は、§62 形 4 の関数で path を根・crate の名・残りに割って読む。2 つの `CRATES_DIR` は消し、固定の根の値は §62 の子 module の 1 か所だけにする。
  3. **歯の置き場**: verify の `-p <crate>` は、宣言したどの根の下の同じ名の crate にも当たる（固定の根の下だけに当てない）。
  4. **nextest 行**: `nextest_line` と `promised_verify` は根の列を引数に取り、入れ子の crate の file からも `-p <crate>` と scope を組む。呼び手（derive.rs 474 行・contract.rs・intake.rs 476 行）は材料の根の列を渡す。歯の区間の呼び出し（derive.rs 2 か所・contract.rs 3 か所）には固定の根を渡す（retroactive の札）。
  5. **親の宣言 file**: `parent_candidates` は、宣言したどの根でも `<根><crate>/src` の直下の新しい file に `lib.rs` と `main.rs` の候補を足す。
  6. **名の衝突の予想**: collide.rs の `module_path` は、宣言した根の下の crate の `src/` と `tests/` の後ろの段を読む。
  7. **変えないもの**: 宣言の無い repo の導出の結果・判定行・findings の字面は 1 byte も変えない。
  8. **並走しない**: 行 bq の着地の後に走る（`depends`）。
- 触らない: §62 の検出線の面・受付の上限の余地・宣言の読み・`derive_write_set` の (i) の閉包（型の名で base の `.rs` の全部を読むので、今も入れ子の file を拾う）・`contracts check` の判定行の字面。
- 限界:
  - crate の名は dir の名と読む。package の名が dir の名と違う crate では、導出の `-p` が cargo の package の名とずれる（今の `crates/` の直下と同じ前提）。
  - 2 つの根の下に同じ名の crate が在る repo では、`-p <crate>` の歯の置き場が両方に当たる（cargo も同じ名の package を 2 つ持てない）。
  - 受付の `Context` の根の欄は受付の経路に読み手が無く、固定の根で埋める実装も同じ出力になる（形 1）。受付の 1 行の検査が置き場を撃つようになる便が、この欄を測る歯を足す。
- 却下:
  - 読み手ごとに根の列を持たせる案。§62 の 1 関数を通さないと、根の規則が 2 か所で分かれる。
  - `Base` に欄を足さず導出の関数の引数に根の列を足す案。`derive_write_set` などの公開の関数の歯の呼び出しが `Base` の字面より多い。
- 見積: derive.rs 約 20（歯の書き直しと新しい歯で約 60）・collide.rs 約 10・table.rs 約 3・check.rs 約 12・intake.rs 約 4・contract.rs 約 8・closure.rs は `CRATES_DIR` を消して約 −2。e2e は contracts.rs 約 80・intake.rs 約 120。diff は歯込みで約 420 行（M）。
- 歯（接頭辞は lib が `closure_crate_roots_` と `contracts_collide_crate_roots_`、e2e が `contract_crate_roots_`。`grep -rn crate_roots crates/ docs/` は §62 の行 bq の歯の接頭辞だけ〔`declaration_crate_roots_`・`pipe_*_crate_roots_`〕で、どれも本行の接頭辞を部分文字列に持たず、本行の接頭辞もそれらを持たない）:
  - lib `closure_crate_roots_`（derive.rs の歯の区間）:
    - (a) 根に nest/crates/ を持つ材料で、`-p toy --lib x_` の歯を nest/crates/toy/src/a.rs に見つける。固定の根だけの材料では見つけない。crates/toy/src/a.rs と nest/crates/toy/src/a.rs の両方に `x_` の歯を持つ材料では、根を持つ周の置き場が 2 file（両方）で、固定の根だけの周は crates/toy/src/a.rs の 1 file（最初の根だけに当てる実装を落とす）。
    - (b) `nextest_line` が nest/crates/toy/src/a.rs から `-p toy --lib`、nest/crates/toy/tests/e2e.rs から `-p toy --test e2e` を組む。固定の根だけでは旗の無い行。
    - (c) +nest/crates/toy/src/new.rs の親の候補に nest/crates/toy/src/lib.rs と `main.rs` が入る。固定の根だけでは入らない。
  - lib `contracts_collide_crate_roots_`（collide.rs に歯の区間を足す）: nest/crates/toy/src/a/b.rs の module の段が根を宣言した周は `a`・`b`、固定の根だけでは空。nest/crates/toy/tests/e2e/x.rs の段は、根を宣言した周は `e2e`・`x`（`tests/` の後ろ）、固定の根だけでは空。
  - e2e `contract_crate_roots_` の 2 本目（名の衝突の予想の配線・§54 の (a) の入れ子の写し）: 宣言で nest/crates/ を足した toy repo に、crate `toy` の nest/crates/toy/src/dial.rs（歯の区間に `dial_ok`）を置く。新しい語 `dial_knob_` の行（別の doc）は write-set に `+` の nest/crates/toy/src/dial/knob.rs と既存の nest/crates/toy/src/dial.rs と nest/crates/toy/src/other.rs を持つ。filter `dial_`（`-p toy --lib`）で write-set が nest/crates/toy/src/dial.rs だけの行が、`contracts check` で knob.rs を名指す `teeth-outside-write-set` の 1 件になる。宣言の無い同じ repo では 0 件。collide.rs の `Base` の根を固定の根に戻す実装（候補を scope に解けず 0 件）と、`module_path` が根を読まない実装（候補が絞れず dial.rs が write-set の中に残って 0 件）の両方を落とす。
  - e2e `contract_crate_roots_`（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）: 宣言で nest/crates/ を足した toy repo の `contracts check` が、nest/crates/toy/src/a.rs に在る歯を write-set の外に持つ行を `teeth-outside-write-set` の 1 件で名指す。宣言の無い同じ repo では findings が今と同じ（0 件）。
  - e2e `pipe_intake_nest_roots_`（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`・既存の `pipe_preflight_` と `pipe_intake_promise_` の歯と同じ撃ち方・新しい module は作らない）: 受付の経路の 3 つの読み（`crates/scribe2/src/pipe/cli/intake.rs` の `base_of` が組む `Base` を Declared 行の歯の検査が読む経路・同じ `Base` を Derived 行の導出が読む経路・`promised` の中の `promised_verify` の呼び手）を、`contracts check` と関数の直撃を通らずに測る（受付の `context` が組む `Context` の根は読み手が無いので測らない・形 1）。toy repo は vessel 宣言に行 bq の key `crate-roots = ["nest/crates/"]` を持ち、nest/crates/toy/src/a.rs の歯の区間に接頭辞 `nest_x_` の `#[test]` の fn を 1 本持つ（crates/toy の下には `nest_x_` の歯を置かない。(f) の対照として、別の crate の crates/other/src/b.rs の歯の区間にも接頭辞 `nest_x_` の `#[test]` の fn を 1 本置く）。
    - (d) Declared 行（verify が `cargo nextest run -p toy --lib --no-tests=fail nest_x_`・write-set に a.rs が無い）を `pipe preflight` に通すと rc 1 で、stdout の `refuse=` の行が `teeth-outside-write-set` と nest/crates/toy/src/a.rs を名指し、stderr は空（判定の関数が Declared 行の歯を base_of の `Base` の根で入れ子の file に解いて断る証拠・断りの行は判定の関数が stdout に描く）。key の無い同じ repo では、`refuse=` の行が nest/crates/toy/src/a.rs を名指さない（入れ子の file を crate の外と読む今の振る舞い）。
    - (e) 同じ verify の Derived 行（既存の `pipe_preflight_ok_reports_facts_and_matches_intake` の行と同じ形で、verify だけを替える）を `pipe preflight` に通すと rc 0 で、`teeth=` の行が nest/crates/toy/src/a.rs を名指し、`write-set=derived` の件数がその file を含む（判定の関数の `Base` が根を持つ証拠）。
    - (f) 約束の行 1 つ（files が nest/crates/toy/src/a.rs・teeth が `nest_x_`）の行を `pipe intake` に通すと rc 0 で、便の写しの契約の verify が `cargo nextest run -p toy --lib --no-tests=fail nest_x_` の 1 行、写しの write-set が nest/crates/toy/src/a.rs を含み crates/other/src/b.rs を含まない（`promised_verify` の呼び手が材料の根を渡す証拠と、導出の入力を組む `promised_inputs` の中の `nextest_line` の呼び出しが根を渡して `-p toy --lib` の行で歯の置き場を toy の crate に絞る証拠。固定の根の旗の無い行は crates/other/src/b.rs の同じ接頭辞の歯まで置き場に入れる）。
  - 変異の A/B: `contracts check` の `Context`（check.rs）の根を固定の根に戻す → `contract_crate_roots_` の宣言の周が 0 件。受付の `base_of` だけ固定の根のまま → (d) の `refuse=` の行が a.rs を名指さず、(e) が歯の置き場を解けず rc 1、(f) の write-set が a.rs を持たない。`promised_verify` の呼び手だけ固定の根を渡す → (f) の verify が `-p toy --lib` を持たない。`promised_inputs` の中の `nextest_line` の呼び出しだけ固定の根を渡す → (f) の写しの write-set が crates/other/src/b.rs を含む。`target_of` だけ固定の根のまま → (b) が落ちる。`parent_candidates` だけ固定の根のまま → (c) が落ちる。
  - base で RED の理由: base は行 bq の着地の後の main で、key は読めるが `Context` と `Base` に根の欄が無い。lib の新しい歯は base の関数の signature と欄が無く該当 0 本（rc 4）か compile で落ちる。e2e は入れ子の歯の置き場を解けず、`contract_crate_roots_` は findings の assert、`pipe_intake_nest_roots_` は (d) の名指し・(e) の rc・(f) の verify の assert で落ちる（機能不在）。宣言の無い対照は base で緑。

## 64. 審査の lens に done の番号つき項目ごとの歯の対応の表を出させ、歯の無い項目を 1 周で全部 at に名指す — 表の欠けた判定は INCONCLUSIVE、歯の無い項目を持つ PASS は FAIL（vacuous-assert）に器が倒す（契約表の行 bs・FR49 / FR9・memo `s2-07l.737.30`）

やさしく言うと: 契約の審査の係（lens）は、done の約束に歯（落ちる test）が無いとき、最初に見つけた 1〜3 個だけを名指して FAIL を返す。直して出し直すと別の項目で落ち、同じ行が 2〜3 周する。そこで器は done の番号つきの項目を 1 行ずつ lens に渡し、項目ごとに「その約束を外した実装で落ちる歯」を表にして返させる。表が欠けた判定は「見ていない」として INCONCLUSIVE に倒す。歯の無い項目（`-`）が在るのに PASS なら FAIL に倒す。歯の無い項目は全部 at に並べる。1 周で穴が全部そろう。

- 出所（2026-09-30）:
  - memo `s2-07l.737.30`: 束 E の審査で、行 g（`s2-07l.738.33`）・行 h（`s2-07l.738.34`）・行 ai（`s2-07l.738.37.1`）が vacuous-assert で 2 周ずつ落ち、2 周目は 1 周目に無い done の項目を名指した。起票の前に残りの行の done の項目 93 個を歯と 1 対 1 で照らすと、13 行に 40 件の同じ型が在った。
  - 再発 2（memo の昇格条件）: memo の起票（06:21Z）の後、行 h の便 `s2-07l.738.34-20260930T062304Z` が done (10) を、続く便 `s2-07l.738.34-20260930T063733Z` が 1 周目に無い done (2)(5) を名指して、2 周続けて vacuous-assert で落ちた。
- 現物（main 94181589・verified）:
  - 契約の審査の雛形 `crates/scribe2/src/headless/lens-contract.txt` は最終行の JSON を 4 key（verdict / evidence / kind / at）に閉じる。観点 1 は「verify の各行が done を測れる歯を名指している」を問うが、done の項目ごとの対応は返させない。evidence は 1 行、at は `,` 区切りの語の列。
  - `crates/scribe2/src/headless/lens.rs` の prompt_of は、契約の写しの隣の材料（design.txt・requirements.txt・promises.txt・base.txt・outside.txt）を読み、契約の本文を `{contract}` に埋める。契約の本文は state が goal / done / verify / write-set を 1 本の字に組んだもので、done は 1 本の字のまま渡る。
  - `crates/scribe2/src/pipe/review.rs` の review（:300）は、先撃ちの使い回し（read_outcome）か lens を撃つ decide（:524 → lens_outcome :562 → read_outcome :579 → parse_lens :598）で判定を得る。narrow（Promised の行の kind の絞り）の後に settle（:621）が review.json（schema / run / verdict / evidence / kind / at / scope / ts）と Reviewed の event を書く。材料は stage（:335）が materials（:350）で組み、keep（:487）が置く。先撃ちの判定の読み outcome_of（:588）も read_outcome を通る。
  - 先撃ちの使い回しの鍵（`crates/scribe2/src/pipe/dispatch/prelens.rs` の digest）は、材料の dir の file の全部を数える。
  - review.json の読み手は器の中だけ: 判定の読み手 judgement_of（受付の 2 門）と、memo の起票の口（evidence と at を観測へ写す）。隣の project の crates に review.json の読み手は 0 件（2026-09-30）。
  - 契約表の done の大半は番号つきの項目「(1) … (2) …」で書く。番号を持たない done（歯の fixture の多く）と、器が約束の行から「(n) expect」で組む Promised の行の done も在る。
- 形（番号は行 bs の done と 1:1）:
  1. **項目の読み**: done の字を頭から見て、(1) から 1 ずつ増える番号の印「(n)」が順に現れた所で区切る。順の外の印（(2) の後の 2 つ目の (2)・(1) の前の (3) 等）は本文の一部。番号の印は半角の括弧と ASCII の数字だけで、(1) を持たない done は項目 0 個。読み手は新しい子 module `crates/scribe2/src/pipe/review/items.rs` の 1 関数で、材料の書き手（形 2）と判定の読み（形 4）が同じ関数を呼ぶ。
  2. **材料**: 項目が 1 個以上で Promised でない行の審査は、材料の dir に items.txt を置く（既存の材料と同じ keep の中・同じ書き方）。
     - 本文は、見出し「## done の項目（K 個・番号つき）」と、表の形の指示（K 個の番号を並べた `1:<歯>,2:<歯>,…,K:<歯>` の行と、`-` の意味・`-` の項目を全部書くこと・器が倒す 2 つを告げる文）と、項目を 1 行ずつ「(n) <本文>」。
     - Promised の行と項目 0 個の行は置かない（材料の dir と prompt は 1 字も変わらない）。
     - lens は、契約の写しの隣に items.txt が在れば、契約の本文の後ろに空行 1 つを挟んで items.txt の本文（末尾の改行を除く）を足し、`{contract}` に埋める。雛形・穴・他の材料は変えない。
     - cap の照合は items.txt の byte を契約の本文に数える（越える周は従来どおり claude を呼ばず INCONCLUSIVE）。在るのに読めない周は、他の材料と同じく claude を呼ばず rc 2。
  3. **対応の表**: lens の最終行の JSON の key done の値（文字列）を `,` で割り、空白を剥がし、空を落とした各項目を「<番号>:<歯>」と読む。番号は ASCII の数字、歯は空白を剥がして空でない字で、`-` は「落ちる歯が無い」。表が揃うとは、番号が 1〜K をちょうど 1 回ずつ覆い、形の合わない項目が無いこと。
  4. **器の倒し方**（項目が 1 個以上で Promised でない行の、lens の JSON が読め verdict が 3 値の周だけ）:
     - 表が揃わない → verdict を INCONCLUSIVE、kind を unparsed に倒す。evidence は「done の対応の表が欠ける（<理由>）: <lens の evidence>」。理由は「key done が無い」「key done が文字列でない」のどちらか、または「無い番号 (n)…」「余る番号 (n)…」「重なる番号 (n)…」「形の合わない項目 <k> 件」のうち在るものをこの順に「・」で結んだもの。at は lens の verdict が PASS でない周だけ lens の値のまま（PASS の周は無し）。
     - 表が揃い `-` が 1 つ以上で、lens の verdict が PASS → verdict を FAIL、kind を vacuous-assert に倒す。at は `-` の番号の順の `done(<n>)` の列、evidence は「歯の無い done の項目 (n)…（lens の対応の表）: <lens の evidence>」。
     - 表が揃い `-` が 1 つ以上で、FAIL / INCONCLUSIVE → verdict と kind は lens の値のまま。at の末尾に、lens の at の語（`,` で割って空白を剥がした語）に無い `done(<n>)` を番号の順に足す（lens の at が無ければその列だけ）。evidence の末尾に「・歯の無い done の項目 (n)…」を足す。
     - 表が揃い `-` が 0 → 判定を 1 字も変えない。
     - 項目 0 個の行・Promised の行・lens の JSON が読めない周・verdict が 3 値でない周・lens の rc が 0 でない周・lens に届かない周（器が作る INCONCLUSIVE）は key done を読まず、判定を変えない。
  5. **撃つ所**: 倒しは read_outcome の中で parse_lens の直後に 1 回。lens を撃った周と先撃ちを使い回した周が同じ関数を通り、review が項目の数を渡す。narrow は倒しの後で、Promised の行は項目 0 個として扱うので 2 つは重ならない。先撃ちの確定の読み outcome_of は項目の数を持たないので 0 を渡す（限界）。
  6. **変えないもの**: review.json の key の集合と schema 1・event の detail の形（`verdict:<V> kind:<k>`・倒した周は倒した後の値）・理由の 7 語・受付の 2 門（焼き直しの門は vacuous-assert と unparsed を測らない）・items.txt の無い周の prompt・Promised の行の審査・先撃ちの使い回しの鍵と手順。
- 触らない: `crates/scribe2/src/pipe/dispatch/prelens.rs`・受付（`crates/scribe2/src/pipe/cli/intake.rs`）・雛形 lens-contract.txt と lens.txt（items.txt は穴でなく契約の本文の後ろに足す）・gate の lens の読み（diff の審査は契約の写しの隣に材料を持たないので items.txt を読まない）。
- ADR を書かない理由: review.json の key と schema と event の形は変わらない（歯の無い項目は既存の at と evidence に載る）。lens の最終行に key を足すのは、§22 が kind と at を足したのと同じ、同じ binary の lens と pipe の間の口である。跨版で残る写し（先撃ちの判定）は、items.txt が材料の dir の鍵を変えるので、古い判定は使い回されない。
- 却下:
  - review.json に項目の表の欄を足す: 跨版の on-disk の形が増える（ADR の条件 3）。今の読み手（受付の 2 門・memo の起票の口）が要るのは歯の無い項目の列だけで、それは at が運ぶ。
  - 雛形の文だけで「全部名指せ」と頼む: lens が従わなくても器が気づけない（散文の規律・憲法 N2）。表を必須にし、欠けを INCONCLUSIVE に倒して初めて「見ていない」と「見て 0 件」を分けられる（gate の lens の findings / population と同じ極性）。
  - 事前審査で done の項目の数と § の歯の記号の数を機械で照らす（memo の候補 2）: 字面の数は意味の対応を測らない。10 項目と 10 本の歯が 1 対 1 でなくても数は合い、歯の無い項目を通す。対応は lens の表が持ち、器は表の揃いだけを測る。
  - 歯の無い項目を持つ PASS を INCONCLUSIVE に倒す: lens 自身の表が「測れない約束が在る」と言っているので型は vacuous-assert で、INCONCLUSIVE では直す所（at）が残らない。
  - 起票の前に席が監査の係を回す（memo の候補 3）: 散文の規律（N2）。
- 限界:
  - 指示の文の効き目（lens が表を書くか・`-` を全部書くか）は器が測らない。器が測るのは表の揃い（形 3）と `-` の数（形 4）だけで、書かない lens は INCONCLUSIVE で止まる。
  - 表の歯の字の中身（在る歯か・その項目を測るか）は器が照らさない。lens が偽の歯を書けば通る。表は「全部の項目を見た」ことと「歯の無い項目の全部」を 1 周で出させる口で、歯の意味の正しさは lens の判断のまま。
  - 先撃ちの確定（事前審査の面）は倒しを通らない。先撃ちが PASS の行が、審査で FAIL に倒されることがある（審査が使い回す周は倒しを通る）。
  - items.txt が材料の鍵を変えるので、番号つきの done を持つ行の先撃ちの判定は、入れ替えの後に 1 回ずつ撃ち直される。
  - 全角の括弧・丸数字の番号は項目と読まない（0 個として従来の審査のまま）。
  - 項目の本文に次の番号の印を字として含む done は、そこで項目が割れる（(2) の本文の中の (3) は 3 番目の項目の頭になる）。
- 歯（接頭辞 pipe_review_done_items_ と headless_lens_items_・どちらも `grep -rn` は crates で 0 件・2026-09-30）:
  - lib（`crates/scribe2/src/pipe/review.rs` の tests・既存の区間）: (a) 項目の読み。`(1) 甲 (2) 乙 形 (2) の字 (3) 丙` は 3 項目で 2 番目の本文が `乙 形 (2) の字`。(1)〜(10) を順に持つ done は 10 項目で 10 番目の本文を持つ。`(2) a (1) b` は 1 項目（`b`）。(1) を持たない `d`、全角の括弧の `（1） e`、全角の数字の `(１) f` は 0 項目。
  - lib（`crates/scribe2/src/headless/lens.rs` の tests・既存の区間）: (b) 契約の写しの隣に items.txt が在れば、prompt の契約の本文の直後（`## 契約が実装する設計の節` の前）に、空行 1 つと items.txt の本文が 1 回だけ入る。cap を契約 + items + 設計 + 要件の byte ちょうどにすると prompt を組み、1 byte 少ないと INCONCLUSIVE（`contract material exceeds cap`）。items.txt の名の dir が在る周は rc 2（prompt を組まない）。items.txt が無い周の prompt は既存の歯 headless_lens_base_fills_the_hole_only_when_the_copy_exists_in_one_pass のまま。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・kind の歯の後ろ・偽 lens は既存の lens_finding の行に key done を足した形）:
    - (c)〜(f) は同じ done（`(1) 甲を作る (2) 乙を測る 形 (2) の字 (3) 丙を足す`・順の外の印を 1 つ持つ 3 項目）の行で撃つ（材料の書き手と判定の読みが別の数え方をすると (d)〜(f) が落ちる）。
    - (c) 材料: done が `(1) 甲を作る (2) 乙を測る 形 (2) の字 (3) 丙を足す` の行を受付から審査まで通すと、材料の dir が base.txt・contract.toml・design.txt・items.txt・requirements.txt の 5 本になる。items.txt は見出しの行（3 個）と、`(1) 甲を作る`・`(2) 乙を測る 形 (2) の字`・`(3) 丙を足す` の 3 行をこの順に持ち、表の形の指示の行が `1:<歯>,2:<歯>,3:<歯>` を持つ。同じ置き場で done が `d-plain` の行は items.txt を置かず、偽 lens が PASS と表 `1:-` を返しても PASS のまま（rc 0・kind も at も無い）。
    - (d) PASS の倒し: 3 項目の行に偽 lens が PASS と表 `1:pipe_x_,2:-,3:-` を返すと rc 1。verdict FAIL、kind vacuous-assert、at `done(2),done(3)`、evidence `歯の無い done の項目 (2)(3)（lens の対応の表）: fake`、detail `verdict:FAIL kind:vacuous-assert`。同じ行で表が `1:a, 2:b ,3:c,`（空白と末尾の `,` を持つ揃った表）の PASS は、rc 0 の PASS のまま（kind も at も無く evidence は fake）。
    - (e) FAIL と INCONCLUSIVE への足し: FAIL・kind literal-mismatch・at `§2,done(2)`・表 `1:-,2:-,3:t` は rc 1 のまま、kind literal-mismatch、at `§2,done(2),done(1)`、evidence `fake・歯の無い done の項目 (1)(2)`。INCONCLUSIVE・kind other・at 無し・表 `1:t,2:t,3:-` は rc 3 のまま、kind other、at `done(3)`。FAIL・at `crates/toy/src/lib.rs,§2,Marker`・表 `1:a,2:b,3:c` は、at と evidence が lens の値のまま。
    - (f) 表の欠け: 3 項目の行に、PASS で表の 7 形（key done 無し・`1:a,3:b`・`1:a,2:b,3:c,4:d`・`1:a,1:b,2:c,3:d`・`1:a,2:,3:c`・`1:a,x:b,2:c,3:d`・値が数の key done）を返すと、どれも rc 3、verdict INCONCLUSIVE、kind unparsed、at 無し。evidence はそれぞれ `done の対応の表が欠ける（key done が無い）: fake`・`（無い番号 (2)）`・`（余る番号 (4)）`・`（重なる番号 (1)）`・`（無い番号 (2)・形の合わない項目 1 件）`・`（形の合わない項目 1 件）`・`（key done が文字列でない）` を持つ。FAIL・kind literal-mismatch・at `§2`・key done 無しは、verdict INCONCLUSIVE、kind unparsed、at `§2`。同じ行で lens が rc 7 で終わる周・JSON を返さない周・verdict が MAYBE の周・`--lens` 無しの周は、evidence が従来の器の理由のまま（`done の対応の表` も `歯の無い` も含まない）で kind unparsed、at 無し。rc 7 の周の偽 lens は stdout に PASS と表 `1:-,2:t,3:t` の JSON の行を書いてから rc 7 で終わり、MAYBE の周の偽 lens は同じ表 `1:-,2:t,3:t` を持つ（倒しが rc と 3 値の外しを飛ばす実装は、rc 7 の周を FAIL・vacuous-assert に、MAYBE の周の evidence に `歯の無い done の項目 (1)` と at `done(1)` を足すので落ちる。stdout の無い rc や揃って `-` の無い表の fixture では倒しても判定が変わらず、外しを測れない）。
    - (g) 先撃ちの使い回し: 既存の使い回しの置き場（行 a と行 b）で行 b の done を `(1) 甲 (2) 乙` にし、先撃ちと審査の偽 lens が PASS と表 `1:-,2:t` を返す。審査は偽 lens を撃たず（回数 1）、detail が `verdict:FAIL kind:vacuous-assert prelens:reused`、review.json の at が `done(1)`。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`・約束の行の歯の後ろ）: (h) Promised の行（約束の行 2 つ・done は器が組む番号つきの字）と、同じ doc の番号つきの done（`(1) 甲 (2) 乙`）の素の行を、偽 PASS の lens（key done 無し）で受付から審査まで通す。素の行は items.txt を置いて INCONCLUSIVE（rc 3）、Promised の行は items.txt を置かず PASS（rc 0）。
  - 変わらない既存の歯（行の verify が撃つ・本文は変えない）: headless_lens_（lens.rs の既存の 4 本・items.txt の無い prompt は 1 字も変わらない）・pipe_review_kind_（review.rs の lib の歯）・pipe_review_kind_fail_keeps_kind_and_at_in_review_json_and_two_word_detail と pipe_review_kind_missing_or_unknown_or_unreadable_falls_to_unparsed_without_moving_the_verdict（番号を持たない done の fixture の判定と review.json の key と schema は変わらない）・pipe_review_reuse_（使い回しの既存の歯）・pipe_intake_promise_（Promised の行は偽 PASS で通る）。
  - 判定の順と変異（条件 1 つに歯 1 本）: 順の外の印で割る → (a)・材料を置かない → (c)・番号を持たない行にも表を読む → (c)・Promised の行にも置く／表を求める → (h)・prompt に足さない・cap に数えない・読めない材料で撃つ → (b)・`-` を見ず PASS のまま → (d)・`-` の無い表でも倒す → (d)・FAIL の at に足さない・lens の at と重ねて足す → (e)・表の欠け（無い・余る・重なる・形・番号でない字）を見逃す → (f)・空白を剥がさない・空を落とさない → (d)・欠けた FAIL の at を落とす → (f)・器の INCONCLUSIVE にも表を求める → (f)・rc が 0 でない周に stdout の表を読む → (f) の rc 7 の周・3 値でない verdict に表を読む → (f) の MAYBE の周・書き手と読みで数え方が違う → (d)(e)(f)・ASCII でない数字を番号と読む → (a)・使い回しの周に倒さない → (g)。
  - base で RED の理由: base は items.txt を置かず key done を読まない。(c) は items.txt が無く、(d)(e)(f)(g) は判定が lens の値のまま、(h) は素の行が PASS。lib の (a)(b) は関数と材料の名が無く compile で落ちる。

## 65. 終端だけの撃ち直しは、記録の sha が main の今の先端と違う周に、着地の commit を本文の run の行で探し直す — anchor の main を揃えて着地の commit の sha が変わった便も、§58 の分岐で閉じる（契約表の行 bt・FR50・§58 の限界）

やさしく言うと: 便の後始末（push → CI の確認 → 台帳を閉じる）が push の段で止まるのは、別の PR の merge が forge の main を先に進め、anchor の main が non-fast-forward になった周である。直すには anchor の main を forge の main へ揃える（rebase する）しかないが、揃えると着地の commit は中身が同じまま別の sha になる。今の撃ち直しは記録に残った元の sha を探すので、それが main の歴史に無く、何度撃ち直しても閉じない。器が着地の commit の本文に必ず書く 1 行 `run: <run id>` を手がかりに、main の歴史から今の着地の commit を探し直し、見つかればその sha で §58 と同じに確かめて閉じる。

- 出所（2026-09-30・verified）: 契約 `s2-07l.738.33` の便 `s2-07l.738.33-20260930T085534Z` は、着地（記録の sha d9c660fe）の直後に別の席の docs PR の merge が forge の main を進め、終端が `terminal:push:failed:git` で止まった。§58 の限界の手順どおり orchestrator が anchor の main を forge の main へ rebase した（着地の commit は f66ffdb9 になり、本文の `run: s2-07l.738.33-20260930T085534Z` の行はそのまま）。撃ち直しは push の後に `terminal:ci:unmeasurable` で止まり、main の CI が success になった後の撃ち直しも同じだった（記録の sha は先端の祖先でない）。この契約を blocks で待つ後続の契約が列で止まった。
- 何が起きているか（main 02be3319・verified）:
  - `crates/scribe2/src/pipe/cli/step.rs` の `fn terminal_only`（59 行）は、記録の sha を `landed_sha` で読み（64 行）、anchor の `refs/heads/main` の今の sha を読み（67 行）、両者が等しければ `PushTip::Tip`、違えば `PushTip::Behind` を渡す（99 行）。
  - `finish.rs` の終端は push の後、`PushTip::Behind` の周だけ記録の sha が先端の祖先かを測り（288 行）、祖先でない周は CI を撃たず `terminal:ci:unmeasurable` で止まる（289 行・close しない）。
  - push が non-fast-forward で落ちた周（`terminal:push:failed:git`）の直し方は、今の器に正しい手が無い: anchor の main に forge の main を merge する commit は main-provenance の門で赤になり（手で作った merge の commit は run の行も PR の番号も持たない）、rebase は着地の commit の sha を変えて上の祖先の判定を外す。
  - 着地の commit の本文の最後には、器が `run: <run id>` の 1 行を書く（`finish.rs` の `squash_message`・`land.rs` の `RUN_TRAILER`）。rebase も cherry-pick も本文を変えない。
  - 同じ行で main の祖先から squash を探す読み手が既に在る: `land.rs` の `fn landed_squash_of`（798 行）は `git log <main> -n 1 --fixed-strings --grep=<run の行> --format=%H` で 1 回探し、当たった commit の本文に run の行と字面が等しい行が在ることを確かめてから sha を返す（無い周・読めない周は `None`）。呼び手は既着地の便の読み（`land.rs` の `rebase_onto`・763 行・§29）の 1 か所だけで、可視性は私有。
- 判定の順（撃ち直しの周）: 段が `Landed` か → 記録の sha → main の今の sha（§58 形 1）→ 記録の sha と等しいか → **等しくない周は run の行で探し直す（形 1）** → 見つけた sha（無ければ記録の sha）と先端を比べて側を選ぶ（形 2 / 3・§58 形 2 / 3）→ 終端: 押す先の宣言 → push → 祖先か → CI の照合 → close。
- 形（番号は行 bt の done と 1:1）:
  1. **探し直し**: 記録の sha が先端と等しくない周に、`landed_squash_of` を先端（main の今の sha）と run id で 1 回呼ぶ。見つかればその sha を着地の sha として先端と比べ直す。記録の sha が先端の祖先の周は、見つかるのは記録の sha そのもの（祖先で本文に run の行を持つ）なので、今と同じ分岐になる。
  2. **見つかった周**: 見つけた sha が先端と等しければ `PushTip::Tip` を渡し、見つけた sha の CI で照合して reason `landed <見つけた sha> ci=success` で close する。等しくなければ先端の sha つきの `PushTip::Behind` を渡し、終端が祖先を測り、先端の sha で CI を照合して reason `landed <見つけた sha> ci=success tip=<先端>` で close する。
  3. **見つからない周**（run の行を持つ commit が先端の祖先に無い・当たった commit の本文に字面の等しい行が無い・git を読めない）: 記録の sha のまま今の分岐へ渡す。終端は CI も台帳も撃たず `terminal:ci:unmeasurable` を記して close しない（rc 1・fail-closed）。
  4. **記録は変えない**: event の詞・終端の 7 値・stdout の 1 行（`run=<id> terminal=<値>`）・記録の sha の読み（`landed_sha`）はそのまま。見つけた sha は event に書かず、CI の argv と close の reason にだけ現れる。器は main も forge の main も動かさない（撃ち直しの push は anchor の main をそのまま押す）。
  5. **既存の読み手を借りる**: `land.rs` は `landed_squash_of` の可視性を `pub(in crate::pipe)` に上げ、doc comment に撃ち直しの呼び手を 1 行足すだけ。既着地の便の読み（`rebase_onto` の呼び）と `finish.rs` は 1 字も変えない。
- 撃ち直しの手順（本 § の着地の後）: 終端が `terminal:push:failed:git` で止まった便は、anchor の main を forge の main へ rebase して揃え（着地の commit の sha が変わる）、`pipe land --run <run> --terminal-only` を撃ち直す。merge の commit で揃えない（main-provenance の門が赤になる）。
- write-set の面: step.rs は本体（探し直しと側の選び直し・doc comment）、land.rs は可視性と doc comment、order.rs は歯（新しい 2 本・既存の helper `replay_fixture`・`replay`・`point_main` を使う）。e2e の親 `crates/scribe2-boundary/tests/e2e/pipe/land.rs` は既着地の便の読みの既存の歯の置き場として載せるだけ（`=`・本文は変えない）。
- 見積: step.rs 447 → 約 459・land.rs 1233 → 約 1235・order.rs 693 → 約 803。core の本体の伸びは約 14 行（S）。xtask の門の副作用は無い（git は既存の読み手 1 本が撃つ・rules 行を足さない・env の読みと process の起動の site を足さない）。
- 触らない: `finish.rs` の終端の本体・`PushTip` の 2 値・`land_train` / `finish`・`Terminal` の 7 値と字面・event の詞・`landed_sha`・台帳の close の字（`close_reason` の 3 形）・`--detection-only`（記録の sha を読むまま）・rules 行・usage と help・§52 / §53 / §58 の歯。
- 限界:
  - 探すのは `git log -n 1` の最初の当たりだけ（§29 の読み手の形）。run の行を部分に持つ行（別の字が同じ行に続く・散文の引用）を本文に持つ commit が本物の着地の commit より新しい位置に在ると、奥の本物を見ずに見つからない周になる（close しない側）。
  - 本文に run の行を字のまま持つ commit が先端の祖先に 2 つ在る main（cherry-pick の写しなど）は、新しい方を着地の commit と読む。squash の本文は 1 便に 1 回だけ run の行を書く。
  - 見つからない周は main の祖先を全部読む（git の呼び出しは 1 回・本 repo の main は約 1400 commit）。
  - land-window が clear を返してから席が PR を merge するまでの間に、列の便が anchor の main に着地して push する周がある（本 § の出所の根）。その競合は本 § の外で、memo が持つ。
- 却下:
  - run の行を持つ commit がちょうど 1 つの周だけ見つけたと読む（2 つ以上を断る）。§29 の読み手と数え方が 2 通りになる（C2）。1 便に 1 回だけ書く行なので、2 つ目は写しである。
  - 見つけた sha を event に 1 件記す（新しい詞）。event の詞の閉じた集合が動き、`landed_sha` の読みと `--detection-only` の入力が変わる。探し直しは撃つたびに同じ答えを返す（冪等）ので、記録は要らない。
  - 祖先の判定を撃ち直しの側で先に撃ち、祖先でない周だけ探す。§58 の却下と同じく判定が 2 か所になる。等しくない周に常に探しても、祖先の周に見つかるのは記録の sha そのもの。
  - 終端（`finish.rs` の terminal）の祖先でない枝で探す。列の着地（`land_train`）の周も同じ関数を通り、着地の周の意味まで変わる。撃ち直しの口だけが「記録が古い」を知る。
  - 着地の push が落ちた周に器が自分で rebase して記録の sha を書き直す。着地をやり直す新しい段になり、§58 の「着地をやり直さない口」の外。
  - forge の main から探す（fetch）。§58 の却下と同じく、撃ち直しが押す先端と探す先端が割れ、network を撃つ新しい段になる。
- 歯（接頭辞 `pipe_replay_relocate_`・`crates/scribe2-boundary/tests/e2e/pipe/land/order.rs` の 2 本・新しい e2e の module は作らない・fixture は §58 の `replay_fixture`〔CI が failure の着地で close しない・CI を直す〕。`grep -rn pipe_replay_relocate_ crates/ docs/` は 0 件〔main 02be3319〕。契約表の nextest の verify 行の filter 語〔898 語〕のどれも、2 本の名〔module path 込み〕の substring にならない）。写しは plumbing で作る（着地した commit の親の上に別の便の commit を 1 つ・その上に着地した commit の木と本文で commit を作る・偽 remote へは別の ref で押してから main を付け替える・force の push は使わない）:
  - (a) 1 本の fn の撃ち直し 2 周。1 周目: 本文の run の行だけを `run: <run id>-x` に替えた写しと、その上の 1 commit を main と偽 remote の main の先端にする。rc 1・stdout が `run=<id> terminal=ci:unmeasurable`・偽 CI の呼び出しが増えず・偽 bd が撃たれない。2 周目: 同じ別の便の commit の上に本文を字のまま写した写しと、その上の 1 commit を先端にする。rc 0・stdout が `run=<id> terminal=closed`・CI の argv が先端の sha を持ち、着地した sha も写しの sha も持たない・偽 bd の reason が `landed <写しの sha> ci=success tip=<先端の sha>` と等しい・main と偽 remote の main は先端のまま・Landed の後ろは 7 件（push・ci:failure・push・ci:unmeasurable・push・ci:success・close:ok）。
  - (b) 本文を字のまま写した写しそのものを先端にして撃ち直す。rc 0・CI の argv が写しの sha を持ち着地した sha を持たない・偽 bd の reason が `landed <写しの sha> ci=success` と等しい（`tip=` を持たない）・Landed の後ろは 5 件（push・ci:failure・push・ci:success・close:ok）。
  - 変わらない既存の歯（本文は変えない）: `pipe_replay_tip_` の 3 本（記録の sha が先端の祖先の周は探し直しで記録の sha そのものが見つかり reason が今と同じ・trailer を持たない兄弟の commit の周は見つからず close しない・main を読めない周は探す前に断る）と `pipe_land_already_landed_` の 3 本（既着地の便の読み）。
  - 変異の A/B（判定の順・条件 1 つに歯 1 本）: 探し直しを撃たない（base の形）→ (a) の 2 周目と (b) が落ちる。字面の等しい行を確かめず `--grep` の当たりをそのまま使う → (a) の 1 周目が落ちる（close する）。見つけた sha でなく記録の sha と先端を比べて側を選ぶ（常に `PushTip::Behind`）→ (b) が落ちる（reason に `tip=`）。close の reason に記録の sha を書く → (a) の 2 周目と (b) が落ちる。`PushTip::Behind` の周に見つけた sha で CI を照合する → (a) の 2 周目が落ちる（argv が写しの sha）。祖先の周にも見つけた sha で記録を上書きする形の誤り（等しい周も探して記録を捨てる）は、祖先の周に見つかるのが記録の sha そのものなので観測が変わらない（歯を置かない）。
  - base で RED の理由: 2 本とも base に在る helper だけを使い、overlay の上で compile は通って assert が落ちる（機能不在）。base は (a) の 2 周目と (b) で記録の sha が先端の祖先でないので CI を撃たず close しない（rc 1・`terminal=ci:unmeasurable`）。(a) の 1 周目は base でも同じ観測（1 本の fn の中の対）。

## 66. 契約表の行が done の番号つき項目ごとの歯を欄 done-teeth で名指し、器が表の検査・受付・gate の段 ① で対応を照らす — 欄を読むだけの行 (0) を先に着地させて binary を入れ替えてから欄を書き、欄の要否は vessel 宣言の任意 key teeth-check と、base から足された行・done の字が変わった行で決め、着地済みの行は書き換えない（epic `s2-07l.736.33` の打ち手 5・[ADR-0104](../../design-intent/decisions/ADR-0104-contract-rows-name-a-tooth-for-each-numbered-done-item.html)・行 (1)〜(4) は SRS の round の後に起こす）

やさしく言うと: 契約の done は「(1) … (2) …」と約束を並べるが、どの約束をどの歯（落ちる test）が測るかは § の散文の中にしか無く、器は数えられない。そのため「約束の 1 つに歯が無い」行がそのまま列に入り、便の中の審査で初めて落ちていた（2026-09-28〜30 の FAIL 76 件のうち 14 件）。行に「約束の番号 → 歯の名」の欄を持たせ、器が 3 か所で数える。設計の PR の CI と受付では「全部の約束に歯が在り、その歯を検証行が撃つか」、gate では「名指した歯を runner が本当に書いたか」。歯がその約束を本当に測るかの判断だけは審査の係に残し、係には行が名指した歯の中から選ばせる。外した実装でも観測が変わらない構造の制約（`=` の file を変えないなど）は歯では測れないので、器の仕組みが測る 4 つに限って仕組みの名を書き、それ以外は done に載せない。欄を知らない古い器は、欄を持つ行が 1 本でも在ると表全体を読めなくなるので、先に「欄を読むだけ」の行を着地させて器を入れ替え、そのあとで初めて欄を書く。

- 出所（2026-09-30・verified）:
  - FAIL の根の分析（epic `s2-07l.736.33`・便 165 本の FAIL / INCONCLUSIVE 76 件）で、「done の項目に落ちる歯が無い・歯が判別しない」型が 24 件。24 件のうち 14 件は、少なくとも 1 つの項目に歯が 1 本も無い形（歯は在るが検証行に入っていない形を含む）で、10 件は、歯は在るが fixture が約束を外した実装を落とさない形（空虚）だった。
  - 14 件はどれも、項目と歯の 1 対 1 の表を書けば起票の前に見える型で、審査の lens が材料だけで名指していた。行 bs（§64）はその表を lens に書かせたが、表を作るのは lens なので、器は表の歯が在るか・検証行が撃つかを照らせない（§64 の限界）。照らす所も便の中（列の後）である。
  - 行 bs の着地と PATH の binary の入れ替えの後の最初の 2 便（dispatcher.md 行 al の便 `s2-07l.738.37.4-20260930T122424Z`・pipeline.md 行 be の便 `s2-07l.738.37.6-20260930T122526Z`）が、どちらも契約の審査の vacuous-assert で落ちた。名指された done の項目は、子 module がほかの行の touches に在る型（Refuse・Land・EventKind など）を名指さない・`=` の file を変えない・既存の枝で起こし直す・struct の literal を変えない、だった。#914 はこれらを「挙動に差の出ない構造の制約」と読んで done から外した。§44 形 1 は同じ型の約束を「挙動に差が出ないので歯では弁別できず、done には載せない」と書いた前例である。
  - 行 al の 2 本目の便（2026-09-30T13:1xZ）は gate で落ちた。runner が足した子 module が ContractRow（本 doc の行 h の touches）を名指し、その file が行 h の閉包に入って、現物の契約表の閉包の歯（gate の共通の検証の nextest の歯 contract_closure_ext_real_table_has_zero_findings）が赤になった。「子 module がほかの行の touches に在る型を名指さない」は、挙動に差の出ない制約ではなく、器の閉包の検査が測る約束だった（#914 の読みの誤り）。直し（#918）は、便の木で契約表の検査を撃つ command の 1 行を検証行の最後に置き、done の項目の歯にした。歯の e2e を検証行に置くと歯の file を write-set に `=` で載せることになり、同じ形の行どうしが交差で直列になるので、command の形にした。
  - 読み手を先に着地させる順（形 8）の出所は 2 つ。宣言の検証行に新しい穴を足した便の着地の直後に、古い PATH の binary の受付が全部の bead を宣言の理由で断った事故（auto-memory の記録・2026-09-20）と、[dispatcher.md](./dispatcher.md) §32 の「PATH の binary を入れ替えた後から書く」の前例である。
- 現物（main 633edd9a・verified）:
  - 行の欄の正本は `crates/scribe2/src/pipe/table.rs` の FIELDS（18 欄）で、done は 1 本の text である。約束の行の欄 PROMISE_FIELDS は、約束 1 つごとに歯の完全名の列 teeth を必須で持つ（ADR-0051 §4）。手書きの行には、項目と歯の対応の欄が無い。欄の生成物は `contracts/schema.toml`。
  - 表の key の集合は FIELDS から引き（`crates/scribe2/src/rules/manifest.rs` の key の検査）、未知の key を持つ行が 1 本でも在る doc は区間ごと読めない。区間を読めない doc が 1 本でも在ると、表の検査は全部の doc の全部の行に unreadable を 1 件ずつ出す（`crates/scribe2/src/pipe/table/check.rs` の declared_files と name_findings）。受付の材料も同じ declared_files を読む（`crates/scribe2/src/pipe/cli/intake.rs`）。vessel 宣言の key も閉じた列（`crates/scribe2/src/pipe/declaration/optional_keys.rs` の DECLARED_KEYS）で、未知の key を持つ宣言は読めず、受付が全部の契約を断る。
  - 表の検査の断りの型 TableError は閉じた型（17 値・`crates/scribe2/src/pipe/table.rs` の歯 TABLE_ERRORS が数を pin する）で、`TableError::` を名指す file は 7 本（`crates/scribe2/src/pipe/cli/intake.rs`・`crates/scribe2/src/pipe/cli/intake/refusal.rs`・`crates/scribe2/src/pipe/refuse.rs`・`crates/scribe2/src/pipe/review/outside.rs`・`crates/scribe2/src/pipe/table.rs`・`crates/scribe2/src/pipe/table/check.rs`・`crates/scribe2/src/pipe/table/parse.rs`）。ほかに `crates/scribe2/src/polarity.rs` と e2e の polarity の歯と外形 snapshot が型の path を文字列で持つが、閉包の形（literal・match の arm・件数 pin・const slice・variant の構築）には当たらない。数 17 を pin する歯は同じ table.rs の 2 本（宣言順の pin と、名に 17 を持つ証拠の在り処の歯）。ContractRow を全部の欄で組む literal は表の読み手（table/parse.rs）と歯の helper 2 つ（contract.rs・table/check.rs）の 3 か所で、ほかは更新構文（`..`）で組む。FIELDS の数は同じ file の 3 本の歯が 18 で pin し、DECLARED_KEYS の列は `crates/scribe2/src/pipe/declaration.rs` の歯が pin する。
  - done の項目の読み手は `crates/scribe2/src/pipe/review/items.rs` の done_items の 1 本（§64 形 1・審査の材料の書き手と判定の読みが呼ぶ）で、`crates/scribe2/src/pipe/review.rs` の私有の子 module の中に在る。
  - 歯の名の読み手は `crates/scribe2/src/pipe/closure/derive.rs` の私有の fn に在る: nextest の行の読み（nextest_read・crate・scope・filter 語・一致の型）、歯の区間の `#[test]` の直下の fn の名の列（test_fns）、fn の本文（fn_body・`fn name(` から次の `#[test]` の行まで）。verify 行の形の読み手は nextest の形の 1 つだけで、他の形の行は歯の置き場を持たない。契約 file の任意 key のうち targets と growth は Contract の field にせず、key の読み手（contract.rs の targets の読み手）で読む前例である。
  - 表の検査は `crates/scribe2/src/pipe/table/check.rs` の judge_repo。CI の job に contracts check の step は無く、nextest の job の歯 contract_closure_ext_real_table_has_zero_findings が現物の表を base 無しで撃つ（どの便の gate の共通の検証でも撃たれる）。base を持つ job は PR のときだけの flip-check の job で、rules-diff と deps-delta が同じ base で相乗りしている。`contracts check` は `--base` を持たない（`crates/scribe2/src/help.rs` の form と `crates/scribe2/src/pipe/cli.rs` の usage）。ci.yml の `run: cargo …` の 1 行形の step は CLAUDE.md の done の区間へ写される（`crates/xtask/src/claude_md.rs` の done_body）。
  - 契約 file の key は閉じた列（`crates/scribe2/src/pipe/contract.rs` の REQUIRED と OPTIONAL・未知の key は行番号つきで断る・OPTIONAL の key は読んで Contract の field へ移す）。Contract を組む struct の literal は 5 か所（`crates/scribe2/src/pipe/contract.rs`・`crates/scribe2/src/pipe/mod.rs`・`crates/scribe2/src/case/mod.rs`・`crates/scribe2/src/pipe/cli/intake.rs`・`crates/scribe2-boundary/tests/e2e/polarity.rs`）。runner の prompt の穴 {contract} は契約 file の字をそのまま運ぶ。lens の穴 {contract} は `crates/scribe2/src/headless/lens.rs` の state が goal / done / verify / write-set だけを組んだ字である。
  - 着地の列の settled の鍵は、生成した契約 file の `git hash-object`（`crates/scribe2/src/pipe/dispatch/candidates.rs`）で、生成の render は導出の write-set（base の木の source に依る）を載せ、depends・tests・creates などを載せない。行の欄の値の sha ではない。
  - gate の段 ① は `crates/scribe2/src/pipe/gate/verify.rs` の check_write_set で、[pipeline.md](./pipeline.md) §58 が write-set の + / ~ と約束の行の名を便の木で測る。land の主実測も同じ段を撃つ。write-set の `=` の項目は、spawn が runner の allowlist を組むとき接頭辞を剥がして入れ（`crates/scribe2/src/pipe/spawn.rs` の write_policy）、段 ① の diff の照合も write-set の内と読む（同じ file の listed）ので、中身を変えないという約束はどこでも測られていない。
  - 幅 120 で数えた行数（R-C4-2 = 1500 の数え方）: table/check.rs 1415・closure/derive.rs 1383・declaration.rs 1380・cli/intake.rs 1313・review.rs 1213・gate/verify.rs 960。table/check.rs の余地は 85 行。
  - 母集団: 設計 doc の契約表の行 432（`scribe2 contracts check --repo .` の rows=）。tracked な docs/design の md の `[[contract]]` の区間の done の行を数えると、done の字に `(1)` を持つ行 285・持たない行 143・done の無い行（Promised）4。
- 形:
  0. **done に載せる約束の種類**: done の項目は、外した実装で観測が変わる約束（挙動の約束）と、観測が変わらない約束（構造の制約）に分かれる。
     - 挙動の約束は、落ちる歯（形 1 の名の歯・既存の歯・検証行の番号の歯）を必ず持つ。
     - 構造の制約は、下の表の器の仕組みが測るものに限って done に載せ、歯の代わりに仕組みの名（形 1 の仕組みの歯）を書く。
     - 仕組みが測らない構造の制約（既存の枝を使う・struct の literal を変えない・寄せ方の指定など）は done に載せず、§ の形の側に「done に載せない」と書く（§44 形 1 と同じ・便の diff の設計への適合は審査で見る）。
     - どちらの種類かは器が判じない。審査の lens が形 6 の表で判じる。

     | 仕組みの名 | 測る構造の制約 | 測る仕組み | 測る時 |
     |---|---|---|---|
     | write-set | write-set の外の file を変えない | runner の guard（FR20）と gate の段 ① の diff の照合 | 編集の時・gate |
     | place-only | write-set の `=` の file の中身を変えない | 行 (4) で足す: runner の allowlist から `=` の項目を外す・gate の段 ① が diff の `=` の file を名指す | 編集の時・gate |
     | new-file | write-set の `+` の file を作る・`~` の file を消す | gate の段 ① の + / ~ の測り（[pipeline.md](./pipeline.md) §58 形 1） | gate |
     | closure | 足した file が、ほかの行の touches に在る型を名指して、その行の閉包をその行の write-set の外へ広げない（子 module がほかの行の touches の型を名指さない約束はこの形で測る） | 便の木で契約表の検査（FR48 / FR55 の閉包）を撃つ検証行の 1 行。本 repo では `cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo .` を検証行の最後に置く（#918 の形）。共通の検証の nextest の歯 contract_closure_ext_real_table_has_zero_findings も同じ閉包を測る | gate（検証行の実走）・CI |

  1. **欄 done-teeth**（任意・文字列の列）: 要素 1 つが「<番号>:<歯>」。番号は done の項目の番号（ASCII の数字）で、区切りは最初の `:` だけ。歯は次の 4 形のどれか 1 つ。
     - 名の歯 `<名>`: その行の検証行の読み手が選ぶ歯の名。nextest の形では、歯の区間の `#[test]` の直下の fn の名（識別子 1 つ・module の path は書かない）。
     - 既存の歯 `=<名>`: base に在り、この便が本文を変えない歯（約束のうち「変えない」を測る既存の歯）。印の字は write-set の `=`（置き場だけ）と揃える。
     - 検証行の番号の歯 `@<k>`: その行の検証行の k 本目（1 から数える）の全体。読み手の無い形の行（nextest でない test の撃ち手・`cargo xtask check` のような門）で測る項目は、この形で書く。
     - 仕組みの歯 `!<仕組みの名>`: 形 0 の表の 4 語のどれか。構造の制約の項目だけに書く。
     - 1 つの項目に歯を 2 本以上名指すときは、同じ番号の要素を並べる。例: `done-teeth = ["1:done_teeth_table_reads_items", "2:=done_teeth_table_keeps_rows", "2:done_teeth_table_keeps_order", "3:@4", "4:!place-only"]`。
     - vessel 宣言の共通の検証（common-verify）だけが測る項目は、その command を行の検証行に 1 本足して `@<k>` で名指す（共通の検証の並びを行から番号で指す形は持たない＝宣言の並びが行の字に漏れない）。gate はその command を共通の検証と検証行で 2 回撃つ（費用は限界）。
     - write-set の外の file を触らないという約束は、done に載せるなら `!write-set` で書く（allowlist と gate の段 ① の diff の照合が測り、落ちる歯を持たない）。
     - ほかの行の閉包を広げないという約束（子 module がほかの行の touches の型を名指さない）は `!closure` で書き、行は便の木で契約表の検査を撃つ検証行を 1 本持つ（形 0 の表・形 2 (f)）。その検証行を `@<k>` で名指しても同じ測りになる。
     - Promised の行は欄を持てない（約束の行の teeth が同じ対応を持つ）。
  2. **表の検査と受付の照らし**（欄を持つ行だけ）: 表の検査・受付・preflight が同じ 1 関数を撃ち、外れを全件・行番号つきで名指す。関数は table の下の新しい子 module（`+`）に置く（table/check.rs の余地が 85 行しか無い）。
     - 口（行 (2)・行 (3) が呼ぶ・どれも crate 内に開く）: 子 module は、要素 1 つの読み parse_element（要素の字 → 番号と閉じた型 Tooth の対か、読めない理由の字）、閉じた型 Tooth（4 値 Named・Kept・Line・Mechanism）と Mechanism（4 値 WriteSet・PlaceOnly・NewFile・Closure・as_str が形 0 の 4 語）、形の照らし done_teeth_misses（done の字・検証行・write-set・creates・欄の要素・core の crate の名 → 外れ Miss の列・(a)〜(d) と (f)）、在りかの照らし done_teeth_located（欄の要素・検証行・閉包の Base → Miss の列・(e)）と、外れの型 Miss（要素の字と理由の字）を持つ。歯の名の読み手は `crates/scribe2/src/pipe/closure/derive.rs` に 2 つ足す（私有の nextest の行の読み・歯の区間の fn の名の列・fn の本文を使う）: 選ぶ行の判定 selects（検証行 1 本・名・core の crate の名 → 真偽・teeth_places と同じ一致の型）と、在りかの列 tooth_sites（検証行 1 本・名・Base → その行の crate と scope の歯の区間で `#[test]` の直下の fn の名が名と等しい所の path と fn の本文の字の対の列）。
     - 子 module は、ほかの行の touches に在る型（TableError・ContractRow・Refuse・ClosureError など）を literal・match の arm・variant の構築の形で名指さない。名指すと、その型を touches に持つ着地済みの行の閉包が write-set の外へ広がり、現物の契約表の閉包の歯が赤になる（出所の行 al の型）。Miss を TableError の variant done-teeth に包むのは table/check.rs と受付（cli/intake.rs）である。
     - (a) 形: 要素ごとに番号と歯の形が読めること。空白を含む・`,` を含む・形 1 の 4 形の外の要素と、同じ要素の重なりを名指す。空の要素と空の配列は表の読みが断る（doc の区間ごと読めない・この照らしの finding にはならない）。
     - (b) 覆い: 番号の集合が 1〜K（K は done_items の項目の数）とちょうど等しいこと。無い番号・余る番号を名指す。K が 0 の行（番号つきの項目を持たない done）は欄を持てない。
     - (c) 検証行: `@<k>` の k が 1〜（検証行の本数）に在ること。
     - (d) 選ばれる: 名の歯と既存の歯は、その行の検証行のうち読み手の在る形の行の 1 本が選ぶこと。nextest の形では、行の一致の型で名が filter 語に当たること（teeth_places と同じ述語）。どの行にも選ばれない名は「撃たれない歯」として名指す（歯は在るが検証行に入っていない型）。
     - (e) 在りか: 既存の歯は、選ぶ行の crate と scope の base の歯の区間にちょうど 1 つ在ること（0 は「無い歯」・2 以上は「2 か所の名」）。名の歯は、base に在れば同じく 1 つに定まること（2 以上を名指す）。base に無い名の歯は新しい歯で、置き場は今の tests 欄と write-set の `+` が持つ。(e) を照らすのは、base を渡した表の検査の変わった行（形 3 と同じ母集団）と、受付と preflight（base は main の先端）だけで、base の無い表の検査（CI の nextest の歯）は照らさない。着地済みの行が名指した歯を後の便が改名・移動・削除しても、その便の gate は落ちない。
     - (f) 仕組み: 仕組みの歯の名が形 0 の表の 4 語のどれかであること。`!place-only` は write-set に `=` の項目を 1 つ以上、`!new-file` は `+` か `~` の項目（か creates）を 1 つ以上、`!closure` は契約表の検査（`contracts check` の form）を撃つ検証行を 1 本以上持つ行にだけ書けること（測る物の無い仕組みを名指さない）。4 語は core の閉じた型で持ち、表の検査と gate と審査の材料が同じ型を読む（C1・C2）。行 (4) が行 (1) より先に着地する（行の割り）ので、照らしが `!place-only` を受ける時には測りが在る。
     - 断りは表の検査の findings の 1 種（`done-teeth`・TableError に variant を 1 つ足す）で、要素と理由を持つ。証拠の在り処（事前審査の確かさの材料）は Name（(e) の外れが base の本文に依る）。受付は同じ判定で便を作らず、preflight は同じ断りを名指す。(e) は表の検査の 1 関数の外の口で、受付と preflight と、base を渡した表の検査だけが撃つ（Context に field を足さない）。
  3. **要否**（vessel 宣言の任意 key teeth-check・無ければ false）:
     - teeth-check が true の repo で `contracts check` に `--base <sha>` を渡した周だけ、変わった行のうち Promised でない行に、(i) 番号つきの項目を 1 つ以上持つこと (ii) 欄 done-teeth を持つことを求める。外れは `done-unnumbered`・`done-teeth-missing` の 2 語（TableError の variant）で名指す。
     - 変わった行は、base の同じ doc に同じ id の行が無い行（足された行）か、在って done の欄の字（表の読みの後の値）が base と違う行である。比べるのは表の行の done の字で、生成した契約 file の sha（着地の列の settled の鍵）ではない。settled の鍵は導出の write-set を含むので、code を変える PR で字の同じ行が変わった行に数えられてしまう。
     - done 以外の欄（write-set・touches・creates・tests・verify・depends など）と § の散文だけを変えた行には求めない。着地済みの行の write-set に `+` の子や `=` を足す docs PR と、歯の改名に合わせて検証行だけを直す PR は、着地済みの行に番号と欄を後付けせずに通る。
     - `--base` の無い周（CI の nextest の歯・push(main)・受付・preflight）は要否を求めず、在る欄だけを形 2 で照らす。preflight は、欄の有無を 1 語（`done-teeth=present` か `done-teeth=absent`）で design= の行の次に出す（母集団を見せる）。受付の stdout は run= の行のまま変えない（受付の出力を読む呼び手を動かさない）。
     - CI は、PR のときだけの flip-check の job に `contracts check --repo . --base <PR の base>` の step を 1 本足す（器の binary を PR の木で組んで撃つ）。step は block scalar（`run: |`）で書き、`run: cargo …` の 1 行形にしない（1 行形は CLAUDE.md の done の区間へ写され、done の定義が 1 行増える）。本 repo の `.vessel.toml` に teeth-check = true を書く（形 8 の順で、行 (0) の入れ替えの後）。
     - 着地済みの行と、done を変えない行には求めない（書き換えさせない）。
  4. **写し**: 受付の生成は欄を契約 file の任意 key `done-teeth` へ写す（REQUIRED の外・OPTIONAL の key は行 (0) が先に受ける）。Contract に field は足さず、key targets と同じく契約 file の key の読み手 1 本（contract.rs に足す）で読む（Contract の構築点 5 か所を直さない）。生成は欄の無い行に key を書かないので、欄の無い行の契約 file の字と着地の列の settled の鍵は変わらない。runner は契約 file の字で名を読む。lens の state は、key を持つ契約だけ done の次の行に `done-teeth:` と要素の列を足す（契約の審査の lens と gate の lens の両方が読む）。欄の無い契約の lens の字と外形 snapshot は変わらない。
  5. **gate の段 ①**（契約 file が欄を持つ便だけ）: [pipeline.md](./pipeline.md) §58 と同じ段・同じ rc と stderr の見出しの形で測り、新しい段も理由の型も作らない。
     - 名の歯は、便の HEAD の木で、選ぶ行の crate と scope の歯の区間にちょうど 1 つ在り、base に無いか本文（fn_body の字）が base と違うこと。無い歯を「書かれていない歯」、本文が base と同じ歯を「動いていない歯」として名指す（動かさない既存の歯は `=` で書く）。
     - 既存の歯は、便の HEAD の木で、選ぶ行の crate と scope の歯の区間にちょうど 1 つ在ること（形 2 (e) と同じ述語・消した歯・2 か所に増えた歯を名指す）。
     - 検証行の番号の歯は段 ① では測らない（検証行の実走〔FR8〕が測る）。
     - 仕組みの歯は項目ごとには測らない。表の仕組みは欄の有無に依らずどの便にも掛かる（place-only は行 (4) の後）。closure は、行の検証行の契約表の検査の実走（FR8）と共通の検証の nextest の歯が測る。
     - 読み手の母集団の file（今は .rs）を 1 本も持たない木では名を測らず、stderr に 1 行残す（[pipeline.md](./pipeline.md) §58 形 2 と同じ）。
     - land の主実測も同じ段で撃つ。列の着地は、列の便の欄の和を列の base で測る。
  6. **審査の lens の表**（§64）: 欄を持つ行の材料 items.txt は、項目の行ごとに宣言の歯を「(n) <本文> ／ 歯: <その番号の歯の列>」と添える。表の指示は「<歯> はその項目の宣言の歯のうち、約束を外した実装で落ちる 1 本・無ければ `-`」と告げ、仕組みの歯は「その仕組みが測る構造の制約の項目にだけ当たる・挙動の約束に仕組みの歯しか無ければ `-`」と告げる（形 0 の種類の判じは lens が持つ）。器は、表の歯が宣言の歯（`=` の有無は問わない）の外なら形の合わない項目に数える（§64 形 4 の INCONCLUSIVE）。欄の無い行の材料と判定は §64 のまま。
  7. **変えないもの**: done の 1 本の text と番号の書き方・§64 の項目の読み（done_items）と倒しの 4 形・約束の行の欄と導出・write-set の導出と歯の置き場の門（§3・§20・§28）・着地の列の settled の鍵・flip-check の判定（FR7・歯ごとの base の RED は測らない）・検出線の {teeth} の穴の導出。
  8. **読み手の先行と入れ替えの順**: 欄と宣言の key を main に初めて書く前に、読むだけの行 (0) を着地させ、PATH の binary を `swap-binary.sh` で入れ替える（走行中の driver が在れば断られる）。
     - 行 (0) より前の binary は、欄を持つ行が 1 本でも在る doc の区間を丸ごと読めず、表の検査は全部の doc の全部の行を unreadable と名指し、受付は全部の契約の材料を読めない。teeth-check を持つ `.vessel.toml` も読めず、受付が全部の契約を断る。欄を持つ行を足す docs PR の CI も、PR の木の code が欄を知らなければ real-table の歯で赤になる。
     - 順: 行 (0) の着地 → 入れ替え → 行 (1)〜(4) を表に足す docs PR（行はこの時から自分の欄を持てる）→ 行 (4)（bv）→ 行 (1) の照らし（bw）→ 行 (1) の写し（bx）と行 (2)（by）→ 行 (3)（bz）。`.vessel.toml` の teeth-check は行 (2) が書く（行 (0) の入れ替えの後なので PATH の binary は key を読める）。
     - 行 (4)・(1)・(2)・(3) の着地の後も、そのつど入れ替える（受付・spawn・gate の振る舞いが変わる）。行 (1) の写しの新しい binary が契約 file に写す key done-teeth は、行 (0) の読み手が既に受けるので、入れ替えの前の binary が新しい契約 file を読んでも断らない。
- 消すもの（C17.2）: 起票の前に席が done の項目と歯を 1 対 1 に書き出して監査する手順（散文の規律で、器は回ったかを知らない）と、§64 の表で lens が歯の名を材料の外から書く余地。どちらも欄と形 2・形 6 が置き換える。構造の制約を done に載せて vacuous-assert で落ちる周も、形 0 が「載せない」か「仕組みの名」の 2 つに閉じる。
- 足す語（vocabulary・ADR-0104）: done の項目・歯の対応の欄・既存の歯の印・検証行の番号の歯・仕組みの歯。SRS の glossary には既存の語「歯」を部分文字列に持つ 4 語を足さない（glossary の部分文字列の検査が断る）。要件の字では開いて書き、足すのは done の項目だけ。
- 行の割り（行 (0) は SRS の round を待たずに起こせる・行 (1)〜(4) は round の後・形 8 の順）:
  - 行 (0) 欄を読むだけ: FIELDS に任意の欄 done-teeth（文字列の列）を足して `contracts/schema.toml` を作り直し、契約 file の OPTIONAL に done-teeth を足し（読んで捨てる・Contract の field は足さない）、DECLARED_KEYS に teeth-check を足す（真偽を読み、効かせない）。照らし・写し・要否は持たない。
    - req は FR47・FR53（FR47 の「少なくとも」の内・FR1 の必須の集合は変わらない）。touches なし（閉じた型に variant を足さない）。
    - write-set の見込み: `crates/scribe2/src/pipe/table.rs`・`contracts/schema.toml`・`crates/scribe2/src/pipe/contract.rs`・`crates/scribe2/src/pipe/declaration/optional_keys.rs`・`crates/scribe2/src/pipe/declaration.rs`・e2e の既存の歯の file。growth は各 file 20 行以内。
    - base で RED の理由: 表の読み・契約 file の読み・宣言の読みが、どれも未知の key で断る（機能不在）。FIELDS の数と DECLARED_KEYS の列を pin する既存の歯の本文を書き換える。書き換えた歯は base の列に対して RED になるので flip し、retroactive の札は要らない（§67 の歯）。
    - 起こした形: 契約表の行 bu（§67）。[reverse-index.md](./reverse-index.md) §8 の束ねで、欄 code-facts と宣言の key index-scip・index-roles も同じ行で読むだけにする（入れ替えを 1 回で済ませる）。write-set は現物で数え直し、行の型付けの file と e2e の contracts の歯の file を足した。
  - 行 (4) `=` の file の不変（形 0 の表の place-only）: spawn が runner の allowlist を組むとき `=` の項目を外し、gate の段 ① が diff の path のうち `=` の項目に当たるものを名指して落とす。歯の置き場の門（§20）と閉包と交差は今のまま `=` の項目を write-set に数える。欄 done-teeth の有無に依らずどの便にも効く。
    - req は FR4・FR20・FR8（SRS 0.33 は FR4 に allowlist から置き場だけの印の項目を除く句、FR20 にその項目への編集の deny、FR8 に gate の write-set を照らす段が diff の `=` の file を落とす規範を置いた）。depends は行 (0)（表に足すのは形 8 の順で、行 (0) の入れ替えの後）。
    - `=` の項目だけの write-set の policy は空になり、guard は全部の編集を断る（空の policy は fail-closed の既存の歯のまま）。
    - 起こした形: 契約表の行 bv。
    - write-set の見込み: `crates/scribe2/src/pipe/spawn.rs`・`crates/scribe2/src/pipe/gate/verify.rs`・e2e の spawn と gate の歯の file。touches なし。
  - 行 (1) は照らし（形 1・形 2）と写し（形 4）の 2 行に割る。写しは Contract の field を持たないので小さく、lens の state の file（[pipeline.md](./pipeline.md) 行 bg・row-review.md の行 e と共に持つ）を照らしの行から外せる。
  - 行 (1) の照らし: 照らしの 1 関数と口・表の検査と受付と preflight の断り。depends 行 (0)・行 (4)。
    - touches は TableError（variant done-teeth）。ContractRow に field done-teeth（文字列の列）を足すが touches には宣言せず、全部の欄で組む literal の 3 か所を同じ便で直して write-set に載せる。write-set の見込み: TableError の閉包の 7 file・照らしの新しい子 module（`+`）・`crates/scribe2/src/pipe/closure/derive.rs` と `crates/scribe2/src/pipe/closure.rs`（読み手 2 つ）・`crates/scribe2/src/pipe/review.rs`（done_items を crate 内へ開く）・`crates/scribe2/src/pipe/contract.rs`（歯の helper の literal）・e2e の contracts と intake の歯の file。
    - base で RED の理由: 照らしが無く欄の外れを名指さない（機能不在）。数 17 を pin する既存の歯 2 本（証拠の在り処の歯は名の 17 を外して数に依らない名に改める）は、flip-check が base の src 区間に HEAD の歯の区間を重ねて撃つので、base の型に無い variant で compile が落ちて RED になり、retroactive の札は要らない。
    - 起こした形: 契約表の行 bw。
  - 行 (1) の写し: 形 4（契約 file の key・preflight の 1 語・lens の state）。depends 行 (1) の照らし（ContractRow の field を読む）。
    - touches なし。write-set の見込み: `crates/scribe2/src/pipe/contract.rs`・`crates/scribe2/src/pipe/cli/preflight.rs`・`crates/scribe2/src/headless/lens.rs`・e2e の intake の歯の file。
    - base で RED の理由: 生成が key を書かず、preflight が 1 語を出さず、lens の state が行を足さない（機能不在）。
    - 起こした形: 契約表の行 bx。
  - 行 (2) 要否: 形 3（`contracts check --base`・変わった行の読み・CI の step・`.vessel.toml`）。depends 行 (1) の照らし。
    - touches は TableError（variant done-unnumbered・done-teeth-missing）。write-set の見込み: TableError の閉包の 7 file・変わった行の読みと要否の新しい子 module（`+`・行 (1) の照らしの子 module は行 (1) の着地まで tracked に無く、素の path で名指せないので触らず、口 done_teeth_located を呼ぶだけにする）・`crates/scribe2/src/pipe/cli.rs`（usage）・`crates/scribe2/src/help.rs`・宣言の読み（`crates/scribe2/src/pipe/declaration.rs` と `crates/scribe2/src/pipe/declaration/optional_keys.rs`・teeth-check の値を Declared と TableFacts に持たせる）・`.github/workflows/ci.yml`・`.vessel.toml`・e2e の contracts の歯の file。
    - base で RED の理由: `--base` が無く要否を求めない（機能不在）。pin の歯 2 本の書き換えは行 (1) の照らしと同じく compile で RED になり、札は要らない。
    - 起こした形: 契約表の行 by。
  - 行 (3) gate と審査: 形 5・形 6。depends 行 (1) の写し（gate と材料が契約 file の key を読む）・行 (4)（`crates/scribe2/src/pipe/gate/verify.rs` を共に持つ）。
    - write-set の見込み: `crates/scribe2/src/pipe/gate/verify.rs`・`crates/scribe2/src/pipe/review.rs`・`crates/scribe2/src/pipe/review/items.rs`・e2e の gate と review の歯の file（歯の名の在りかは行 (1) の照らしの口 tooth_sites を呼ぶので、`crates/scribe2/src/pipe/closure/derive.rs` を持たない）。
    - base で RED の理由: 段 ① が key を読まず、材料が宣言の歯を添えない（機能不在）。
    - 起こした形: 契約表の行 bz。
  - 他の設計との順（同じ file を持つ行を blocks で結ぶ・FR39 は走行中の交差を直列にするだけで、書き換える歯の本文や閉包の古びは解かない）:
    - [pipeline.md](./pipeline.md) 行 bg（`crates/scribe2/src/headless/lens.rs` と lens の外形 snapshot・`crates/scribe2/src/pipe/review.rs`）→ 行 (1)・行 (3)。
    - [pipeline.md](./pipeline.md) 行 bh（`crates/scribe2/src/pipe/spawn.rs`）→ 行 (4)。
    - [pipeline.md](./pipeline.md) §66 の終わりの門の行（`crates/scribe2/src/pipe/gate/verify.rs` の run_checks_admitted が段 ① を撃ち直す）→ 行 (4)・行 (3)。行 (4)・行 (3) の段 ① の断りは、終わりの門の撃ち直しにもそのまま効く。
    - 行 (0) → row-review.md の行 b（`crates/scribe2/src/pipe/declaration/optional_keys.rs` と DECLARED_KEYS の pin の歯）。行 (0) は SRS の round を待たないので先に着地し、行 b は teeth-check を含めて pin を数え直す。
    - row-review.md の行 a0・行 a・行 c（`crates/scribe2/src/pipe/review.rs`・§64 の材料）→ 行 (3)。row-review.md の行 e（`crates/scribe2/src/headless/lens.rs`）→ 行 (1)。row-review.md の行 d（`.vessel.toml`）→ 行 (2)。
    - 設計 doc の PR の順は問わない（行の順は台帳の blocks が持つ）。ADR の状態・decisions の README・constitution の被参照は SRS 0.33 の PR で揃っている。
  - 歯の接頭辞は `done_teeth_` で始め、行ごとに分ける: 行 (4) `done_teeth_place_only_`・行 (1) の照らし `done_teeth_table_`・写し `done_teeth_copy_`・行 (2) `done_teeth_base_`・行 (3) `done_teeth_gate_` と `done_teeth_review_`（どれもほかのどれの部分でもない）。2026-10-01 の main で、fn の名に `done_teeth` を持つ歯は 0 本。設計 doc の検証行の filter 語 910 語のうち接頭辞の部分に当たるのは行 o の `table_`（`done_teeth_table_` の部分・行 o の write-set は table.rs と table/check.rs を持ち、照らしの lib の歯はその 2 file に置く）だけ。接頭辞の後ろの語は `contract_` などの filter 語を部分に持たせない（各行の verify の最後の契約表の検査が置き場の外れとして測る）。
- ADR を書く理由: ADR の条件 3（契約表の行の欄・契約 file の key・vessel 宣言の key の 3 つの跨版の形）と 4（却下案）。
- 却下（詳しくは ADR-0104）:
  - lens の表だけで持つ（§64 のまま）: 器が表の歯の在りかも検証行の選びも照らせず、照らす所が便の中に残る。
  - done を子行に割る（約束の行と同じ形の 2 つ目の子行）: 番号つきの 285 行の書き換えと、子行の型が 2 つになる。
  - 全行に欄を求める: 着地済みの行と開いた行の書き換えになる。
  - 欄を持たない行の sha を tracked な一覧に凍らせて免除する: 増える側の運用の一覧を持つ。base からの差分で同じ免除を言える。
  - 歯ごとの base の RED を flip-check の出力から読む: 言語ごとの出力の読みを器が持ち、compile の赤で空に通る。
  - 変わった行を着地の列の settled の鍵（生成した契約 file の sha）で決める: 導出の write-set が動くたびに字の同じ行が変わった行になり、code の PR が着地済みの行に欄の後付けを強いられる。
  - 行 (1) が欄の読み手と照らしを一度に足す（行 (0) を持たない）: 行 (1) を表に足す docs PR の時点で main の code と PATH の binary が欄を知らず、表全体が読めなくなる。
- 限界:
  - 歯がその約束を本当に測るか（空虚でないか）は器が測らない。器が測るのは、対応の揃い・選ばれ・在りか・書かれたか・動いたかである。測りの意味は審査の lens と検出線（変異）のままで、分析の空虚の型 10 件は本 § では塞がらない。
  - 項目が挙動の約束か構造の制約か（形 0）は器が判じない。挙動の約束に仕組みの歯だけを書いた行は、審査の lens が `-` を返して初めて落ちる。
  - closure の仕組みが測るのは、着地済みの行と表の行の閉包だけである。まだ表に無い行の閉包への広がりは測らない（§54 の予想の範囲）。ほかの行が子の file を write-set に先に持つ（`+` で宣言した）ときは、子がその行の touches の型を名指しても閉包は write-set の内に収まり、findings は 0 になる（許された形）。
  - 名の歯を解くのは nextest の形の読み手だけ（Rust の形）。他の形の repo は `@<k>` で書き、項目ごとの名の在りかは測らない。共通の検証の command を検証行へ写した `@<k>` は、gate で同じ command を 2 回撃つ。
  - 「動いた」の測り（fn_body の字）は粗い。本文は `fn name(` から次の `#[test]` の行までなので、すぐ後の歯の doc comment や直後に足した歯・区間の末尾の helper を直すと動いたと読み（偽の通過）、fixture の helper だけを直して約束を満たした歯は動いていないと読む。
  - `--base` の無い経路（直接の push など）では要否を求めない。main-provenance と merge の門がその経路を閉じている前提に立つ。
  - base に無い名の歯が同じ便の中で 2 か所に置かれた周は、gate が名指す（受付では測れない）。
  - 着地済みの行が名指した既存の歯を後の便が改名・移動しても、base の無い表の検査は名指さない（形 2 (e)）。その行が再び受付に出る周と、done の字を変える PR の周に名指される。
  - doc を跨いで行を移す PR と doc の改名は、移した行を足された行と読み、欄を求める。
  - 行 (0) より前の binary は、欄を持つ行を含む doc の区間と teeth-check を持つ宣言を丸ごと読めない（形 8・入れ替えは前へだけ）。行 (0) の着地と入れ替えの前に欄と key を書かない。
  - done の中の全角の番号は項目と読まない（§64 の限界のまま）。欄を求められた行は半角の番号で書き直す。
  - write-set の dir の項目（末尾 `/`）の配下に `=` の項目が在ると、allowlist が dir で許すので編集の時点では断れず、gate の段 ① だけが落とす（行 (4)）。

## 67. 欄と宣言の key を読むだけの行 — 契約表の行の任意の欄 done-teeth・code-facts、契約 file の任意 key done-teeth、vessel 宣言の任意 key teeth-check・index-scip・index-roles を読んで形だけ確かめ、値は捨てて何にも効かせない（契約表の行 bu・§66 の行 (0) と [reverse-index.md](./reverse-index.md) §8 の束ね・FR47 / FR53）

やさしく言うと: §66（done の項目ごとの歯の欄）と reverse-index.md（code の索引）は、どちらも行の欄と vessel 宣言の key を新しく足す。今の器は知らない欄や key を 1 つでも見ると、その doc の区間ごと・宣言ごと読めなくなる。そこで、欄と key を書く前に「読んで捨てるだけ」の版を着地させ、PATH の binary を入れ替えておく。2 つの設計の読むだけの行を 1 本に束ねて、入れ替えを 1 回で済ませる。

- 出所: §66 形 8（読み手の先行と入れ替えの順）と、reverse-index.md §8 の「同じ時期に done の歯の欄の読むだけの行が未着地なら、2 つの欄を 1 本の読むだけの行で足す」。
- 何が起きているか（main 36c34993・verified）:
  - 行の欄の正本 FIELDS（`crates/scribe2/src/pipe/table.rs`）は 18 欄（必須 5・条件付き 2・任意 11）。表の key の集合は FIELDS から引き、未知の key を持つ行が在る doc は区間ごと unreadable（rc 2）になる。
  - 行の型付けは `crates/scribe2/src/pipe/table/parse.rs` の typed が欄ごとに形を読んで ContractRow を組む。table.rs の歯 table_fields_pin_the_schema_columns_and_the_reader_enforces_their_shapes は FIELDS の全部の欄に形の違う値を書き、「<欄> は」の字で名指されることを求める。FIELDS に足す欄は、typed でも形を読まないとこの歯が赤になる。
  - FIELDS の数を 18 で pin する歯は table.rs の 3 本（上の 1 本・contract_promise_need_conditional_is_two_fields_in_the_schema・contract_whole_goal_head_pins_the_folio2_schema_and_the_goal_stays_off_the_fields）。生成物 contracts/schema.toml は contracts schema の出力で、e2e の歯 3 本（contract_schema_ の 2 本・contract_growth_schema_lists_growth_as_an_optional_list）と xtask check の contracts-schema が FIELDS と照らす（どれも数を pin しない）。
  - 契約 file の key は `crates/scribe2/src/pipe/contract.rs` の REQUIRED 9 と OPTIONAL 5（classes・opens・touches・targets・growth）で、未知の key は行番号つきで断る。targets は Contract の field を持たない任意 key の前例（読むのは gate の検出線だけ）。
  - vessel 宣言の key は `crates/scribe2/src/pipe/declaration/optional_keys.rs` の DECLARED_KEYS（17 key）と OPTIONAL_KEYS（14 key）。`crates/scribe2/src/pipe/declaration.rs` の fields は OPTIONAL_KEYS に無い key を必須と読むので、任意 key は 2 つの列の両方に足す。値の読み value_of は空の配列を ruling-fixtures のほかは断る。真偽の読みは optional_keys.rs の bool_key。DECLARED_KEYS の列は declaration.rs の歯 declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema が 17 本で pin する。
  - contracts check は宣言を HEAD から読み、読めない周は rc 2 で stderr に key の名を出す（e2e の contracts.rs の entrance-flip の歯と同じ経路）。
  - 幅 120 で数えた行数: declaration.rs 1380（余地 120）・table.rs 857・parse.rs 653・contract.rs 734・optional_keys.rs 554。core の本体の行は概算で約 69600 / 74000（R-C4-1）。
- 形（番号は行 bu の done と 1:1）:
  1. **行の欄**: FIELDS の末尾（growth の後）に、任意の文字列の列の欄 done-teeth と code-facts をこの順に足す（20 欄・任意 13・1 項目 1 行のまま＝xtask check の contracts-schema が字面で読む）。生成物 contracts/schema.toml は contracts schema の出力で作り直す（手で書かない）。
  2. **行の型付け**: typed は 2 欄を既存の配列の読みで読んで形だけ確かめ、値は捨てる。ContractRow に field を足さないので、構築点は動かない。要素の中身（番号と歯の形・列の語と値）は読まない（§66 の行 (1) と reverse-index.md の行 e が読む）。
  3. **形の違い**: 2 欄に文字列を書いた行は、既存の字「<欄> は文字列の配列でなければならない」で欄の行番号に名指す（新しい字を作らない）。空の配列は今の表の読みが断る。
  4. **契約 file**: OPTIONAL に done-teeth を足す（key の名は contract.rs の const 1 つで、GROWTH と同じ置き方）。build は既存の配列の読みで読んで形だけ確かめ、値は捨てる。Contract に field を足さず、render は書かない（写すのは §66 の行 (1)）。
  5. **vessel 宣言**: DECLARED_KEYS と OPTIONAL_KEYS の末尾に teeth-check・index-scip・index-roles をこの順に足す（20 key・任意 17）。読みは optional_keys.rs の関数 1 つに置き、Declared の parse の読みの列には 1 行だけ足す。teeth-check は bool_key と同じ真偽の読み、index-scip と index-roles は文字列の配列の読みで、値は捨てる。Declared に field を足さず、便の写し（Effective）にも写さない。片方だけの宣言・穴の有無・command の許しは見ない（reverse-index.md の行 a）。
  6. **宣言の形の違い**: 3 key に文字列を書いた宣言は、今の宣言の読みと同じく rc 2 で、key の名と「真偽」か「配列」の字を持つ不備になる。
  7. **閉包**: 足す code は、ほかの行の touches の型（ContractRow は契約表の行 h・TableError は行 aw・ax・az の touches）を、今それを名指していない file で新しく名指さない。便の木で契約表の検査を撃つ検証行を最後に置いて測る（§66 形 0 の closure・#918 の形）。
- 変えないもの: 欄も key も持たない行・契約 file・宣言の読みと出力の字、contracts check の判定行、受付と preflight の判定、生成した契約 file の字（着地の列の settled の鍵）、gate、審査の材料。欄や key を持つ行と宣言も、形が合えば持たないものと同じ出力になる。
- 入れ替え: 着地の後に PATH の binary を入れ替えるまで、欄と key を main に書かない（§66 形 8・reverse-index.md §8）。
- 歯（done の項目ごと・どれも base で RED）:
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`・接頭辞 contract_fields_read_only_）:
    - (a) 欄 2 つを持つ行の doc と、key 3 つを持つ宣言の repo の contracts check が rc 0・findings 0 で、欄と key を消した同じ repo と判定行が同じ字。base は未知の key で区間と宣言を読めず rc 2 なので RED。
    - (b) 形の違い 5 形（done-teeth と code-facts に文字列・teeth-check と index-scip と index-roles に文字列）。欄の 2 形は findings が欄の名と「は文字列の配列」の字を持ち rc 2。宣言の 3 形は rc 2 で stderr が key の名と「真偽」か「配列」の字を持つ。base は「未知の key」の字で断るので、字の照合で RED。
  - lib（contract.rs の既存の test 区間・接頭辞 contract_fields_read_only_）:
    - (c) done-teeth を持つ契約 file が、持たない同じ file と等しい Contract に読める。文字列の done-teeth は key の名を持つ不備になり、「未知の key」とは言わない。base は未知の key で断るので RED。
  - 書き換える既存の歯: table.rs の 3 本（欄の宣言順・数 20・任意 13）と declaration.rs の 1 本（key の列 20 本・doc comment の本数）。書き換えた歯は base の FIELDS と DECLARED_KEYS に対して RED になり flip するので、retroactive の札は要らない。
  - 変わらない既存の歯: contract_schema_ の 2 本と contract_growth_schema_lists_growth_as_an_optional_list（生成物と FIELDS の照らし）。
- 他の行との順: 行 c（[case-lifecycle.md](./case-lifecycle.md) §12 の行 c）が declaration.rs と optional_keys.rs を write-set に持つ（sha の読みを 1 本足す・DECLARED_KEYS は変えない）。走行中の交差は受付が直列にするので、blocks は結ばない。[reverse-index.md](./reverse-index.md) §15 の行 f とは table.rs を共に持つ（行 f は pub(crate) の再輸出の列を 1 語、本行は FIELDS と歯を変える）。
- 限界:
  - 値を捨てるので、欄と key を書いても何も起きない。書いた行を器が照らすのは、§66 の行 (1)・(2) と reverse-index.md の行 a・e の着地の後。この版の binary は中身の誤った欄も通す。
  - code-facts を契約 file へ写す key は足さない（reverse-index.md §7 (c) は行の欄を表の検査・受付・起動の列で測り、契約 file は読まない）。
  - 2 欄の値は ContractRow に載らないので、live な行の書き換えを止める hook の guard と、着地の列の settled の鍵（生成した契約 file の字）は、2 欄だけの変化を見ない。field を足すのは §66 の行 (1) と reverse-index.md の行 e。
- ADR: 書かない（欄と key の形は ADR-0104・ADR-0105 が決めた。本 § はその読み手を先に置く段）。

## 68. 焼き直しの門の teeth-outside-write-set の物差しは、at の path の形の項目から # の後ろと末尾の行の番号を剥がした file の字面で測る（契約表の行 ca・§35 の物差しの項目の字面の側・便 `s2-07l.738.39.6`）

やさしく言うと: 審査役が「この歯は write-set の外の file にある」と指すとき、file の名の後ろに # と歯の名を付けて書くことがある。器はその字面のまま write-set と比べるので、file を write-set に足しても「直していない」と読み、受付が永遠に断る。比べる前に # の後ろ（と末尾の行の番号）を剥がし、file だけで比べる。

- 何が起きているか（orchestrator の実測 2026-10-01・便 `s2-07l.738.39.6` の審査 FAIL の後の preflight・verified）: lens が teeth-outside-write-set の at に 4 項目を書いた。e2e の seat.rs の helper と歯の 2 つと、polarity.rs の歯 1 つは、どれも path の後ろに # と名を付けた形。残る 1 つは設計 doc の行と § の組で、空白を持つ。docs で write-set に seat.rs と polarity.rs を足しても、`pipe preflight` は finding-unaddressed で断った。理由は 3 項目を # の後ろごと名指し、「測った 3 件・測れない 1 件」と数えた。
- 現物（main e41f0368・verified）: `crates/scribe2/src/pipe/review/judgement.rs` の `path_shaped` は、空白を持たず / を含む字面を path と読む。だから path の後ろに # と名を付けた項目は測る側に入る。`teeth_unaddressed` はその字面のまま `covered`（`crates/scribe2/src/pipe/refuse.rs`）へ渡し、`covered` は # の後ろを含む字面を write-set の項目と比べる。どんな契約でも covered にならない。§35 が path でない項目で塞いだのと同じ型（永遠に断る）が、項目の字面の側に残っていた。lens の雛形（`crates/scribe2/src/headless/lens-contract.txt`）は at を「path か識別子か §」と書く。path の後ろに # と名を付けた形は、path と識別子の組み合わせとして雛形の内側で起きる。
- 形（番号は行 ca の done と対応）:
  1. **剥がし**: 空白を持つ項目は従来どおり測れない側に置く（剥がす前に決める・設計 doc の行と § の組を path に読まない）。空白を持たない項目は、最初の # から後ろを剥がし、続けて末尾の「: と数字」を剥がす（: の後ろが数字と : と - だけの形・行と列と行の範囲）。残った字面を、その項目の file の字面とする。剥がした字面が空なら測れない側。
  2. **測る**: path の形か（`path_shaped` の tracked の file との照合と / の読み）と、write-set に在るか（`covered`）を、剥がした file の字面で測る。+ の接頭辞の読みは従来どおり。
  3. **名指しと数**: 断りの理由に出す項目は剥がした file の字面（辞書順・重複なし）。「測った n 件」の n も、剥がした後の重複なしの file の数。測れない m 件の数え方は従来どおり（項目の重複なしの数）。
  4. **変えない**: `covered` と `normalize`（write-set のほかの読み手が共有する）・`path_shaped` の 3 形の読み・literal-mismatch と section-material-missing の物差し・同型 N 回の門・lens の雛形・`FindingKind` の 7 語。剥がしは teeth-outside-write-set の物差しの中だけに置く。
- 歯（e2e・`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`・接頭辞 pipe_intake_repeat_teeth_outside_write_set_at_・既存の `failed_runs` / `Again` / `assert_refused` / `write_set_contract` の型・verify の行は既存の pipe_intake_repeat_ の歯ごと撃つ）。# の後ろの名は、fixture のほかの字面（設計の pointer の anchor など）と衝突しない長い名にする。
  - (a) done (1): at が src/other.rs の後ろに # と長い歯の名を付けた 1 項目の後、write-set に src/lib.rs と src/other.rs を持つ契約が通る。base は項目が covered にならず断る → RED。
  - (b) done (2): at が src/other.rs の後ろに :12 を付けた 1 項目の後、同じ write-set の契約が通る。base は断る → RED。
  - (c) done (3): at が src/other.rs の後ろに # と違う 2 つの長い名を付けた 2 項目と §33 の後、write-set が src/lib.rs だけの契約は finding-unaddressed で断られる。理由は teeth-outside-write-set と src/other.rs と「測った 1 件・測れない 1 件」を持ち、2 つの長い名のどちらも持たない。base は 2 項目を字面のまま名指し、「測った 2 件・測れない 1 件」と数える → RED。
  - done (4) は、既存の e2e の pipe_intake_repeat_ の歯が本文を変えずに緑であることで測る（新しい歯を置かない）。lib の pipe_review_unaddressed_ の歯（`crates/scribe2/src/pipe/review.rs` の tests）は # も末尾の行の番号も持たない項目だけを渡すので、剥がしの後も同じ字で緑のまま。review.rs は未着地の行の write-set に在るので、本行の write-set と検証行には入れない（done の 8 門の nextest が撃つ）。
- 却下: lens の雛形に「at の path は file の字面だけ」と書く（過去の便の at は書き換わらず、`s2-07l.738.39.6` が止まったまま・lens が字を守る保証も無い）／`covered` に剥がしを入れる（write-set の閉包や交差の読み手と共有で、契約の write-set の項目に # を許すことになる）／# の後ろの名を literal-mismatch の物差しで別に測る（kind の違う物差しを混ぜる・§23 (3) の kind ごとに 1 関数）。
- 限界: 空白を持つ項目（設計 doc の path の後ろに # と行の id と空白と § を付けた形）は、今までどおり測れない側に残す。空白を持たない設計 doc の pointer は file として測るので、契約の write-set に設計 doc が無ければ断りは残る（剥がす前も断っていた）。新しく測る側に入るのは、/ を持たず剥がした字面が tracked の file に当たる項目（Cargo.toml の後ろに # と key を付けた形）だけで、その file が write-set に無ければ断る。
- ADR: 書かない（物差しの読みの直し・§35 の型の延長）。

## 69. 契約表の置き場を vessel 宣言の任意 key contract-tables が名乗り、契約表を数える器の読み手と受付が 1 つの関数を通る（契約表の行 cb〜cg・memo s2-07l.738.42.2・[ADR-0109](../../design-intent/decisions/ADR-0109-contract-tables-are-enumerated-from-the-declared-places-by-one-function.html)）

やさしく言うと: 器は「契約表は docs/design/ のすぐ下の .md に在る」と決めてかかって表を数えている。表を contracts/ の .toml に置く repo（隣の project）では、契約表の検査は表を 0 本と数え、局面の出力は行を「読めない」と出し、走っている便の行を書き換える編集も止めない。1 行を pointer で指して読む口（受付・審査・gate）は場所を問わないので、便は通るのに見張りだけが表を見ない。repo が「表はここに在る」と vessel 宣言に書けるようにし、表を数える読み手を全部 1 つの関数に通す。受付も同じ関数で置き場の外を指す pointer を断るので、走っている便の行は必ず置き場の中に在り、見張りは置き場だけを見て足りる。書かない repo の数え方は今と同じ。

### 69.1 何が起きているか（main a6d4234c・verified）

- 「契約表とは何か」の定義が器の中に 3 通りある。
  - 定義 1（docs/design/ の直下の .md）: `crates/scribe2/src/pipe/table/check.rs` 634 行の `design_docs` が「契約表の doc の母集団の読みはこの 1 本」と名乗る。呼び手は 4 系統: contracts check の母集団（`judge_repo` 619 行）と名の衝突の予想（621 行）、未追跡の知らせ（628 行・`git ls-files --others` を docs/design/ に絞って読む 711 行）、宣言済みの新規 file の母集団（`declared_files` 645 行・contracts check と受付の材料 `crates/scribe2/src/pipe/cli/intake.rs` 271 行の両方）、runner の stdin の「ほかの行の touches」節（`crates/scribe2/src/pipe/spawn.rs` 510 行・便の base の木）。
  - 定義 1 の写しが 2 つ在る（同じ filter を自前で書く）: 局面の行の読み手 `crates/scribe2/src/fleet/lifecycle_mark.rs` の `read_rows`（642〜660 行・main の sha で `git ls-tree` を docs/design/ に対して非再帰で撃ち 645 行で絞る・0 本は `Missing`）と、台帳の形の検査 `crates/scribe2/src/ledger/form.rs` の `is_design_doc`（284〜286 行・注は「contracts check と同じ母集団」）。
  - 定義 2（docs/design/ の下・深さを問わず .md か .toml）: live-row の門 `crates/scribe2/src/hook/live_row.rs` の編集（333 行の含みと 383 行の先頭一致）と commit（444 行）。設計 [vessel-hook.md](./vessel-hook.md) §15 形 3 がこの定義を名乗る。
  - 定義 3（表の形の中身を持つ tracked file・場所を問わない）: 行の審査の表の見分け `crates/scribe2/src/pipe/review_ref.rs` 204〜214 行と、code の木の鍵 `crates/scribe2/src/pipe/row_review.rs` 138〜144 行（同じ `bears_table` が 2 本）。
- pointer で 1 行を引く読みは場所を問わない: `parse_pointer`（`crates/scribe2/src/pipe/table/parse.rs` 404〜416 行）は拡張子（`form_of`・26〜32 行）しか見ない。受付の生成 `generated`（intake.rs 380 行）は pointer の doc を HEAD から読むだけで、置き場を照らさない。この 1 本を受付・preflight・列の候補・起動の列の事前の検査・行の審査・resume の取り直しが呼ぶ。だから隣の project の受付は通る。
- 隣の project の症状（memo の観測・その repo は読んでいない）は、定義 1 と 2 の帰結として全部説明がつく: 局面の出力の row と requirement の `table-unreadable`（lifecycle_mark.rs 646〜647 行の `Missing` → `crates/scribe2/src/ledger/phase_main.rs` 26 行の語）、contracts check の `docs=0 rows=0`（check.rs 430 行の判定行）、live-row の門が表の編集を通す（live_row.rs 333 行で弾く）。memo の数えに無い症状が 3 つ在る（deduced）: 受付の名指しの実在が toml の行の `+` の file を宣言済みと読まない（§39）、名の衝突の予想（§54）が toml の行を見ない、runner の「ほかの行の touches」節が toml の行を載せない。
- 数（verified）: main の docs/design/ の tracked file は 30 本で全部が直下の .md。直下の .toml も下の dir も 0 本。`contracts check --repo .` は `docs=30`。区間の marker か `[[contract]]` を持つ tracked な .md / .toml は docs/design/ の外に 0 本。tracked な `contracts/schema.toml` は `[[field]]` の表を持つ生成物で、契約表の読み手は `[[contract]]` 以外の表を断る（`crates/scribe2/src/rules/manifest.rs` 689〜692 行）。test の fixture で置き場（定義 1）の外を pointer で指して受付に通すのは e2e の 1 本だけ（`crates/scribe2-boundary/tests/e2e/pipe/review.rs` 537 行の `docs/design/derived.toml#g`）で、lib と e2e のほかの受付の fixture は全部 docs/design/ の直下の .md を指す（pointer の字面と `format!` の組み立ての grep）。
- vessel 宣言（verified）: 読み手は `Declared::parse`（`crates/scribe2/src/pipe/declaration.rs` 538 行）の 1 本で、未知の key は「未知の key」で積み宣言全体が読めない（703 行）。任意 key は key の名・読み手・2 つの key の列（`crates/scribe2/src/pipe/declaration/optional_keys.rs` 13 行・103 行）に 1 行ずつと外へ渡す口を optional_keys.rs へ、`Declared` の欄と parse の 1 行を親へ足す（同 file 1〜5 行の決まり）。rev を名指して読む先例は `ruling_keys_at`（190 行）と `close_check_at_sha`（219 行）と `requirements_at_sha`（226 行）。閉じた 3 値の先例は `RootsAtHead`（crate_roots.rs 51 行）。書いた空配列は配列の層が key を問わず断る（`ruling-fixtures` だけが例外）。

### 69.2 形（決めること）

1. **key**: vessel 宣言の任意 key `contract-tables` を 1 つ足す。値は repo 相対の項目の配列。末尾が `/` の項目は dir で、その dir の**直下**の file のうち `form_of` が読める（.md か .toml の）file を表の置き場と読む。`/` で終わらない項目は 1 file で、その path と等しい file を置き場と読む。glob と正規表現は持たない（ADR-0047 と同じ・prefix と完全一致で足りる）。schema は 1 のまま。key の列（宣言順）の末尾に置く。
2. **足すだけ**: 既定の置き場（docs/design/ の直下の .md・今の母集団）は常に在り、key はそれに足す（置き換えない）。宣言 1 行で器の検査の母集団を狭められないようにするためで、crate-roots（§62 形 2）と同じ向き。key を書かない宣言と宣言 file を持たない repo は今と 1 本も変わらない。
3. **項目の検査**: 空（空白だけ）・絶対 path・home の短縮記号・`..` の段は、key の行番号を名指す宣言の不備にする。書いた空配列と配列でない値は既存の配列の層が断る。重複と重なり（dir 項目の直下の file を file 項目でも書く）は不備にしない（列挙は path の列を絞るので、同じ path は 1 度）。拡張子の検査は宣言の段でせず、列挙した file を表の読み手が読む段で名指す（`form_of` の知識を宣言の側に写さない・C2）。
4. **宣言の読み（rev を名指す口 1 つ）**: optional_keys.rs に、名指した rev の tree の宣言から閉じた 3 値を返す口を置く（型 TablePlaces・variant Fixed / Declared / Unreadable・関連 fn at と items）。宣言 file がその rev に無い周と git を撃てない周は Fixed（`ruling_keys_at` と同じ）、key の無い宣言も Fixed、在って parse が落ちる周は Unreadable（既定に倒さない・C10）。
5. **列挙の 1 関数**: check.rs の `design_docs` を、path の列と項目の列を受ける形に広げる。返すのは入力の path の列のうち既定に当たる path と項目に当たる path で、入力の順のまま。項目の列が空なら今と同じ列を返す。Unreadable の周の扱いは呼び手が決める（形 6）。
6. **読み手ごとの差し替え**（どの rev の宣言を読み、読めない周にどう倒れるか）:

   | 読み手 | 宣言を読む rev | 列挙の入力 | 読めない周 |
   |---|---|---|---|
   | contracts check（`judge_repo`・衝突の予想・未追跡の知らせ） | HEAD | `git ls-files` の tracked。未追跡は `--others --exclude-standard` の pathspec を docs/design/ と項目にして同じ 1 関数で絞る | 今と同じ rc 2（同じ HEAD の宣言を `table_facts_named` が先に断る） |
   | 宣言済みの新規 file（`declared_files`・contracts check と受付の材料の両方） | HEAD（関数の中で読む・引数は今のまま） | tracked | 理由を返す（読めなさを宣言 0 本に読み替えない・今の Err の形） |
   | runner の「ほかの行の touches」節 | 便の base | base の `ls-tree -r` | 理由の 1 行（今の形・runner は止めない） |
   | 局面の行（`read_rows`） | main の sha（関数の中で読む・引数は今のまま） | main の sha の `ls-tree -r` の全 path | `Missing`。gather は続く close-check の読みで同じ宣言を読めず `declaration` の語で止まる（今の順のまま・出力を書き直さない） |
   | live-row の門（編集と commit） | 編集か commit の worktree の root の HEAD | 編集は対象の path 1 つ・commit は HEAD との差の path | `form_of` が読める path を全部比べる側に倒す |
   | 台帳の形の検査（`docs_of`） | HEAD | tracked | `ledger-form: unreadable reason=declaration-unreadable` |
   | 受付の生成（`generated`・行 cg） | HEAD（受付の材料が 1 周に 1 回読む） | pointer の path 1 つ | 置き場の外と同じ断り（fail-closed） |

7. **置き場の検査**（contracts check・行 cf）: 宣言した項目のうち tracked の path に 1 つも当たらない項目を名指す。列挙した doc の file 名の stem の重なりを名指す（契約 id は stem と行 id の組で、ADR-0023 §2.1 は doc id を一意の鍵とする。置き場が 1 dir の直下の .md だけの今は構造で一意だが、置き場が増えると重なりうる）。
8. **受付の pointer の置き場**（行 cg）: 受付の生成は、pointer の path が HEAD の宣言の形 5 の列に入らない契約を、doc を読む前に typed に断る（FR54 の拡張）。生成を呼ぶ口は全部この 1 本を通るので、受付・preflight・列の候補・事前の検査・行の審査・resume の取り直しが同じ断りになる。これで「live な便の行は置き場の中に在る」が受付で成り立つ。
9. **門は 1 つの定義だけを読む**（裁定・行 cd）: live-row の門は定義 2 を捨て、形 5 の列だけを比べる。行 cg と合わせて、表の定義は器の中で形 5 の 1 本（と、中身で見分ける定義 3・§69.9）になる。

### 69.3 行 cb — key・宣言の読み・列挙の 1 関数・contracts check と宣言済みの新規 file と runner の節

- 約束（番号は done と 1:1）:
  1. vessel 宣言が任意 key `contract-tables` を受け、形 1 の項目の配列として読む。形 3 の不備 4 形（空・絶対 path・home の短縮記号・`..` の段）は key と行番号を名指して断る。書かない宣言は項目 0。schema は 1 のまま。key の列の pin（`declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema`）は末尾に 1 つ伸びる。
  2. optional_keys.rs に形 4 の口を置く。宣言 file の無い rev と git を撃てない周は Fixed、key の無い宣言は Fixed、key を持つ宣言は Declared（項目の列と key の行番号）、宣言が在って読めない rev は Unreadable。HEAD 以外の rev も読む。
  3. `design_docs` が形 5 の 1 関数になる（path の列と項目の列を受ける・入力の順・既定は常に入る・dir 項目は直下で `form_of` が読める path・file 項目は等しい path・同じ path は 1 度）。項目 0 の返りは今と同じ。
  4. contracts check の doc の列（行の検査と衝突の予想が同じ 1 本の列を読む）と未追跡の知らせが、HEAD の宣言の項目で形 5 の列を使う。判定行の字面は変えない。
  5. `declared_files` が引数を変えずに HEAD の宣言の項目で形 5 の列を読む（受付の材料は intake.rs を変えずに同じ列になる）。宣言を読めない周は理由を返す。
  6. runner の「ほかの行の touches」節が、便の base の宣言の項目で同じ 1 関数を使う。読めない周は理由の 1 行。
  7. 器の repo（key を書かない）の contracts check は findings 0 のまま。
- 設計の線（審査が読む・後の行 cc〜cg が呼ぶ口・名と引数と可視性と閉じた返り）:
  - 型 TablePlaces は pub の enum（Debug・Clone・PartialEq・Eq）で、variant は Fixed（既定だけ）・Declared（欄 items は項目の列・欄 line は key の行番号で u64）・Unreadable の 3 つ。親の declaration.rs が `RootsAtHead` と同じく pub で再輸出し、呼び手は declaration の path で引く。
  - 関連 fn at は pub で、repo の path と rev の字を受けて TablePlaces を返す（`git show <rev>:.vessel.toml` を `Declared::parse` に掛ける・作業ツリーは読まない）。関連 fn items は pub で、Fixed は空の列・Declared は項目の列・Unreadable は無し（Option）を返す。
  - `design_docs` は今の可視性（pub(crate)）と table.rs の再輸出のまま、引数を path の列と項目の列の 2 つにする（返りは入力の path の列への参照の列）。
  - `declared_files` は名と引数と返りを変えない。`untracked_files` は check.rs の中の private のまま、項目を受ける。
  - 新しい file は作らない（読み手は optional_keys.rs・1 関数は check.rs のまま）。新しい src の file に in-file の歯を置くと flip-check で落ちるため。
  - check.rs は余地が少ない（幅で数えて 84 行）ので、1 関数と `declared_files` の歯は table.rs の既存の mod tests に置き、check.rs には歯を置かない。宣言の歯は optional_keys.rs の既存の mod tests に置く（declaration.rs の余地 116 行を使わない）。
- 歯（RED の理由は全部「機能不在」）:
  - lib `declaration_table_places_`（optional_keys.rs の既存の mod tests）: (a) key の無い宣言は項目 0、2 項目の key は 2 項目と key の行番号、不備 4 形はそれぞれ key と行番号を名指し、空配列と文字列の値は既存の字面で断る。(b) 使い捨ての git repo（`declaration_ruling_check_reads_the_named_rev` と同じ組み方）で、宣言 file の無い repo と git でない dir は Fixed、key を持つ commit は Declared、key の値を壊した commit は Unreadable、1 つ目の commit に key・2 つ目で key を消すと 1 つ目の sha は Declared で HEAD は Fixed。base には名が無く 0 本（rc 4）。
  - lib `table_places_docs_`（table.rs の既存の mod tests）: (c) 1 関数の表: 項目 0 は docs/design/a.md だけを返し、docs/design/b.toml・docs/design/sub/c.md・contracts/d.toml を返さない。["contracts/"] は docs/design/a.md・contracts/d.toml・contracts/e.md を返し、contracts/sub/f.toml と contracts/g.txt を返さない。["tables/one.toml"] はその 1 file だけを足し、tables/one.toml.bak を足さない。["docs/design/"] は docs/design/b.toml を足し、a.md を 2 度返さない。返りは入力の順。base 0 本。
  - lib `table_places_declared_files_`（table.rs の既存の mod tests・使い捨ての git repo）: (d) HEAD の宣言が key で contracts/ を名乗る repo の `declared_files` が contracts/t.toml の行の `+` の file を返し、key の無い宣言の commit では返さず、key の値を壊した commit では理由を返す。base 0 本。
  - e2e `contracts_tables_key_`（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）: (e) toy repo（docs/design/toy.md は区間を持たない）が key で contracts/ を名乗り、contracts/t.toml に goal の無い行 a を置く: findings は contracts/t.toml の行 a の `contract-table:section-missing` の 1 件だけで、判定行は docs=2 rows=1・rc 1。key を消した同じ repo は docs=1 rows=0・rc 0。(f) 未追跡の contracts/u.toml が `contracts untracked-doc:` の知らせ（path を名乗る）と `untracked=1` を出す。base は key を「未知の key」で断り rc 2。
  - e2e `spawn_touches_from_declared_tables_`（`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`）: (g) key で contracts/ を名乗る toy の、contracts/t.toml の行 b の touches の項目が runner の stdin の「ほかの行の touches」節に `contracts/t.toml#b` の pointer で載る。base は受付が宣言を断る。
  - 既存の歯で今のままを測る: `contracts_untracked_doc_is_noticed_without_counting_and_joins_the_population_once_tracked`（未追跡の知らせの字面）・`runner_touches_section_`（子の dir の doc を載せない）・`contract_closure_ext_real_table_has_zero_findings`（器の repo の表）。
  - 変異の A/B（条件 1 つに歯 1 本）: 既定を項目で置き換える → (c) の ["contracts/"] で a.md が消える。dir 項目を再帰にする → (c) の sub/f.toml が出る。file 項目を接頭辞で当てる → (c) の tables/one.toml.bak が出る。Unreadable を Fixed に倒す → (b) と (d)。
- 見込み（幅で数えた余地の内）: optional_keys.rs 130（歯 (a)(b) 込み）・declaration.rs 8・check.rs 20・table.rs 70（歯 (c)(d)）・spawn.rs 5・e2e contracts.rs 90・e2e spawn.rs 50。size M。

### 69.4 行 cc — 局面の行の読み手

- 約束:
  1. `read_rows` が引数を変えずに、main の sha の tree の宣言を形 4 の口で読み、sha の tree の全 path（`ls-tree -r`）を形 5 の 1 関数で絞る。行の pointer の字は `<置き場の path>#<行 id>`（今の形）。
  2. 宣言を読めない sha は `Missing`（既定の置き場に倒さない）。0 本と、区間を読めない doc が 1 本でも在る周も今と同じ `Missing`。
- 設計の線: lifecycle.rs は触らない。gather は `read_rows` の後に同じ sha の宣言を close-check の読みで読み、読めない周は `declaration` の語で止まって出力を書き直さない（今の順・[case-lifecycle.md](./case-lifecycle.md) §12 の 6 語のまま）。
- 歯: lib `lifecycle_declared_tables_`（lifecycle_mark.rs の既存の mod tests・`lifecycle_mark_main_order_follows_git_ancestry` と同じ組み方の使い捨ての git repo）: (a) 宣言が key で contracts/ を名乗り表を contracts/x.toml だけに置いた commit A の `read_rows` が pointer `contracts/x.toml#a` と行の write-set を返す（前提: docs/design/ に file が無いことを歯の中で assert）。(b) key を消した commit B は `Missing`、同じ repo で commit A の sha を名指すと (a) と同じ行（HEAD でなく名指した sha の宣言を読む）。(c) key の値を壊した commit C は contracts/x.toml が在っても `Missing`。(d) key の無い宣言で docs/design/y.md に表を置いた commit D は y.md の行を返す（既定が残る）。RED: base は名が無く 0 本（rc 4）・本文を当てても base の `read_rows` は docs/design/ だけを見るので (a) が `Missing`（機能不在）。
- 見込み: lifecycle_mark.rs 60（歯込み・余地 210 の内）。size S。

### 69.5 行 cd — live-row の門（門は 1 つの定義だけを読む）

- 約束:
  1. 編集の門は、対象が `form_of` の読める path で、同じ置き場の worktree の root を解けた周に、root の HEAD の宣言を形 4 の口で読み、repo 相対 path が形 5 の列に入る周だけ比べる。.md で区間の始まりの行を変更前にも変更後にも持たない編集は、今どおり宣言を読まずに通す。宣言を読めない周は `form_of` の読める path を全部比べる（表として読めない本文は変化 0 で通るので、倒しても表でない file の編集は止まらない）。
  2. commit の門は HEAD との差の path を同じ口と同じ 1 関数で絞る（宣言を読めない周は `form_of` の読める path の全部）。
  3. 門は形 5 の列のほかを見ない（定義 2 を捨てる・裁定）: docs/design/ の下でも列に入らない path（下の dir の表・key を書かない repo の直下の .toml）の編集と commit は比べない。
  4. 比べ（`hits`）・自分の行の除外・deny の行・記録の語・解けない dir の扱いは変えない。
- 歯: e2e `hook_live_tables_`（`crates/scribe2-boundary/tests/e2e/hook/guards.rs`・既存の `live_run` と `live_hook` の helper を使い、便の写しの design を表の path に差し替える）: (a) 宣言（必須 3 key と key contracts/）と contracts/t.toml（行 a / b / c）を commit した toy で、Questioned の便の行 contracts/t.toml#a の done を変える Edit が rc 2・記録 `live-row-deny changed`。(b) 同じ変更の `git commit` も changed で断る。(c) 置き場の外の other/u.toml（同じ表の写し）を変える Edit は通る（対照・base で緑）。(d) 便を置いた後に宣言の key の値を壊す commit を置き、(a) と同じ Edit が changed で断られる。(e) key の無い toy で docs/design/sub/t.toml の行に便を置くと、その行を変える Edit は通り、同じ toy に key で docs/design/sub/ を名乗る commit を足すと断る。既存の `hook_live_row_` の歯は本文を変えずに緑（docs/design/x.md の表）。RED: base の門は docs/design/ の外を見ないので (a)(b)(d) が通り、定義 2 を読むので (e) の 1 つ目が断られる（機能不在）。
- 見込み: live_row.rs 30・e2e guards.rs 140。size M。

### 69.6 行 ce — 台帳の形の検査

- 約束:
  1. `docs_of` が HEAD の宣言を形 4 の口で読み、tracked を形 5 の 1 関数で絞る（`is_design_doc` を消す）。
  2. 宣言を読めない周は理由の語 `declaration-unreadable`（`ledger-form: unreadable reason=declaration-unreadable`・件数を 1 つも出さない）。
  3. drift の契約 id の形（stem と行 id）は変えない。
- 歯: lib `ledger_shape_tables_`（form.rs の既存の mod tests・使い捨ての git repo・台帳は空の列）: (a) 宣言が key で contracts/ を名乗り、contracts/t.toml に未着地の行 a（`+` の file が tracked に無い）を置いた commit の `docs_of` を判定に掛けた描画が `unlanded=1` と `drift=1:t#a` を持つ。(b) key を消した commit は `unlanded=0`。(c) key の値を壊した commit の `docs_of` は理由の語 declaration-unreadable を返す。RED: base は名が無く 0 本（rc 4）・base の `docs_of` は docs/design/ だけを見て宣言を読まないので (a)(c) が落ちる（機能不在）。
- 見込み: form.rs 50（歯込み）。size S。

### 69.7 行 cf — 置き場の検査（contracts check）

- 約束:
  1. 宣言した項目のうち tracked の path に 1 つも当たらない項目（dir 項目は直下に `form_of` が読める tracked file が 0 本・file 項目は tracked に無い）を、`.vessel.toml` の key の行で 1 件ずつ名指す（rc 1・語 place-empty）。既定の置き場の 0 本は名指さない（表を持たない repo は今どおり）。
  2. 列挙した doc の file 名の stem が重なれば、列挙の順で 2 本目以降の doc を行 0 で名指し、相手の path を添える（rc 1・語 doc-id-duplicate）。
  3. 2 つの語は契約表の欠陥の閉じた enum（TableError）の variant 2 つを宣言順の末尾に足して表す（C2）。理由の 1 行・rc 1・在り処（place-empty は項目の path・doc-id-duplicate は 2 本の doc の path の file の列）を持ち、名の列の pin と在り処の pin の母集団を 2 つ伸ばす（既存の歯の本文を直す＝retroactive の札）。
  4. 器の repo の contracts check は findings 0 のまま（verify の最終行）。
- 設計の線: 検査の関数は table.rs に置き（check.rs の余地を使わない）、`judge_repo` は findings に 1 行で足す。受付（`check_row`）はこの 2 つを撃たない（repo 全体の事実）。touches は TableError で、閉包の file のうち本文を変えない file は `=`（置き場だけ）で write-set に載せる。
- 歯: e2e `contracts_place_defect_`（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`）: (a) key に contracts/ と tables/none.toml（tracked に無い）を書き contracts/a.toml に表を置いた toy は、tables/none.toml の 1 件だけを `.vessel.toml:<key の行>` の `contract-table:place-empty` で名指し rc 1。(b) key で contracts/ を名乗り contracts/toy.toml と docs/design/toy.md の両方に表を置いた toy は `contract-table:doc-id-duplicate` 1 件（docs/design/toy.md・相手 contracts/toy.toml）。(c) key の無い toy は 2 つの語を出さない（対照・base で緑）。lib の既存の 2 本（`table_error_names_are_pinned_in_declaration_order_and_carry_their_line`・`pipe_table_evidence_is_decided_once_for_` で始まる 1 本）は母集団が 2 つ増えた値で緑。RED: base は key を断り rc 2、語が無い（機能不在）。
- 見込み: table.rs 60・check.rs 4・e2e contracts.rs 80。size M。

### 69.8 行 cg — 受付が置き場の外の pointer を断る

- 約束:
  1. 受付の生成（`generated`）は、pointer の path が受付の材料の宣言の置き場（形 5 の 1 関数に path 1 つと項目の列を掛けて空でないか）に入らない契約を、doc を読む前に `contract-table:place-outside`（path を名乗る理由の 1 行・rc 1）で断り、run dir を作らない。宣言を読めない材料も同じ断り（fail-closed）。生成を呼ぶ口（受付・preflight・列の候補・事前の検査・行の審査・resume の取り直し）は全部この 1 本を通るので、同じ断りになる。
  2. 受付の材料（`Materials`）が HEAD の宣言の置き場を 1 周に 1 回読む（材料の読みの 1 本の中・予想の写しは同じ値を持ち回る）。材料の struct の字面を持つ既存の test の helper（`short_of_room`）は欄を 1 つ足す（retroactive の札）。
  3. 語 place-outside は TableError の variant 1 つを宣言順の末尾に足して表す（在り処は宣言 file の名 1 本の file の列・予想の写しで宣言が動くと暫定）。名の列の pin と在り処の pin の母集団を 1 つ伸ばす（retroactive の札）。
  4. 既定の置き場と宣言した置き場を指す pointer は今どおり通る。置き場の外を指す既存の e2e の歯（`pipe_review_contract_whole_goal_reads_the_goal_from_a_derived_toml`）は、toy の宣言に key で docs/design/ を名乗る 1 行を足す形に直し（retroactive の札）、ほかの assert は変えない。
  5. 器の repo の contracts check は findings 0 のまま（verify の最終行）。
- 設計の線: 判定は doc を読む前（`show_head` の前）に置く（置き場の外は表でない＝読む理由が無い）。行の審査（定義 3 で表を見分ける・§69.9）は置き場の外の行を受付と同じ断りの 1 行で記録する（今の「受付の生成が断った行」の扱いのまま）。
- 歯: e2e `intake_place_outside_`（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs`）: (a) key の無い toy に contracts/t.toml（goal を持つ行 a・ほかの欄は toy の行と同じ形）を commit し、`--design contracts/t.toml#a` の受付が rc 1・stderr が `contract-table:place-outside` と contracts/t.toml を名乗り・run dir と event は撃つ前と同数。同じ pointer の preflight が `refuse=contract-table:place-outside` の 1 行と rc 1。(b) 同じ toy に key で contracts/ を名乗る commit を足すと、同じ受付が rc 0 で run を作る。(c) 同じ toy の docs/design/toy.md の行の受付は (a) の toy で rc 0（対照・base で緑）。RED: base の受付は置き場を照らさず (a) を通す（機能不在）。
- 順の条件: 消費側が key を名乗った後に着地させる（先に着地すると、いま contracts/ を指す便の受付と resume の取り直しが全部断られる）。着地の前に、live な便のうち置き場の外を指す便が 0 本であることを数える。
- 見込み: intake.rs 14・table.rs 25・e2e intake.rs 90・e2e review.rs 2。size M。

### 69.9 触らない

- 区間の抜き出しと全文の読み（`form_of`・`read_table`・版の宣言）・pointer の形（`parse_pointer`）・契約 id の形・`FIELDS` と生成物。
- 行の審査の表の見分けと code の木の鍵（定義 3・review_ref.rs と row_review.rs の `bears_table`）: 場所を問わずに中身で見分ける。行 cg の後は、置き場の外の行の審査は受付と同じ断りの 1 行になる（審査の口の側は変えない）。
- review の外の材料の読み（`crates/scribe2/src/pipe/review/outside/linked.rs`・pointer の先を場所を問わずに読む）。
- path の種別の key（`design-doc-paths` ほか・ADR-0047）と role_guard の固定の判定: 編集の権能の種別で、表の置き場とは別の事実。表を docs/design/ の外に置く repo は、席が表を編集するために `design-doc-paths` も名乗る（2 つの key は独立）。
- 着地の留めの判断の欄の読み（ruling_hold.rs）・席の指示文の出所 pointer・xtask の 2 面（器の repo だけの門）。
- contracts check の判定行の字面・局面の出力の `unmeasured` の語・live-row の deny の字面と記録。
- 器自身の `.vessel.toml`（key を書かない）。
- 台帳の形の検査の § の本文の読み（form.rs 309〜315 行は `## N.` の見出しで切り、.toml の行の goal を読まない）: 本件と別の穴として memo に起こす。

### 69.10 限界

- 門の守りの範囲は形 5 の列だけ（裁定・行 cd）: 今の門が守る docs/design/ の下の .toml と下の dir の表は、key で名乗らない限り守らない。器の repo では該当 0 本（verified）。行 cg の後は受付もその外を断るので、置き場の外に live な便の行は出来ない。行 cd から行 cg までの間は、key を書かない repo の docs/design/ の下の .toml と下の dir を指す live な便の行を門が守らない（器の repo では 0 本・隣の project の contracts/ の行は今も守られていないので、その間に減る守りは無い）。
- 宣言を変える commit と同じ commit の表の変更は、門が HEAD（変更の前）の宣言で測る。便が live の間に宣言から置き場を外すと、門はその置き場の行を守らない（宣言の変更そのものは便の審査と gate か user の手を通る）。
- 門は .md か .toml の編集ごとに、worktree の root と置き場の解きに加えて HEAD の宣言を 1 回読む（区間を持たない .md の編集は読まない）。宣言を読めない周の門は .md / .toml の全部を比べる側に倒すので、その周だけ event log を読む回数が増える。
- 古い binary は key を「未知の key」で断り、宣言全体を読めなくする（受付・contracts check・局面の出力・close の門の全部）。消費側が key を書くのは、その host の PATH の binary を行 cb の着地の後の版に入れ替えた後。
- 行 cg の後、置き場の外を指す pointer は受付と resume の取り直しの両方で断られる。行 cg の着地の前に置き場の外を指して走り出した便は、resume で止まる（順の条件で 0 本にしてから着地させる）。
- dir 項目は直下だけを読む（入れ子の dir は項目を並べる）。dir 項目は直下の .toml を全部表と読むので、表でない .toml（欄の生成物など）を同じ dir に置く repo は file 項目で並べる（読めない置き場として contracts check が名指すので、黙っては落ちない）。

### 69.11 却下

- 決め打ちの dir（contracts/ の .toml）を既定で数える: 先方には 1 行で効くが、配置を器が決め打ちする。器の repo の `contracts/schema.toml`（`[[field]]` の生成物）を表と読んで、器自身の contracts check が赤になる。
- 開いた契約の pointer が指す file だけを読む: 表を開かずに済むが、bead の無い行（局面の row-unbeaded・台帳の形の drift）が出なくなる。CI の contracts check（FR55 の全行）は台帳に届かないので母集団を作れない。
- 読み手ごとに置き場の列を持つ（今の形の延長・各読み手に key を読ませる）: 差し替えの便は小さいが、今まさに live-row の門だけが別の定義を持つように、写しは必ずずれる。
- `design-doc-paths` を流用する: key が増えないが、意味が違う（誰が編集してよいか と どこに表が在るか）。その既定（docs/design/ の下の全部）は列挙の既定（直下の .md）と違い、置き換える意味なので、宣言 1 行で表を検査の外へ出せる。
- 中身で見分ける（tracked の全 .md / .toml のうち表の marker を持つ file）: 宣言が要らず、行の審査（定義 3）と揃う。だが marker の打ち間違いで表が黙って母集団から落ち（fail-open・NFR4）、歯の fixture の表を本物と数え、置き場が宣言値でなく推測になる（C10）。
- glob の値（例 contracts の .toml の glob）: 書き方は短いが、新しい読み手が要り、ADR-0047 が prefix と完全一致で足りると決めた形から外れる。
- key が既定を置き換える: 既定を外したい repo には便利だが、宣言 1 行で docs/design/ の表を検査の外へ出せる。表の無い .md は 0 行なので、外す必要が無い。
- 門だけは定義 2 を残す（docs/design/ の下の全部と置き場の和を守る）: 守りは最も広いが、表の定義が 2 本に戻り、受付が通さない場所の行を門だけが守る。行 cg の後は置き場が live な行を必ず含むので、門の別の定義は要らない（裁定で 1 本に決めた）。
- 受付で断らず、門を pointer で守る（live な便の design の path を全部守る）: 受付の挙動は変わらないが、契約表の検査と局面の出力が見ない場所に契約の正本を置けてしまい、定義が 2 本に戻る。

### 69.12 順と行の分け方

- 先に docs PR: ADR-0109・本節と区間の行 cb〜cg・[vessel-hook.md](./vessel-hook.md) §15 形 3 と形 4 の「docs/design/ の下」を「契約表の置き場（§69 形 5 の 1 関数）」に直す 1 行ずつ・vocabulary（設計 doc・契約表・vessel 宣言・新語 契約表の置き場）・decisions の README。SRS の言い回しは user の `/folio-architect` の round で、行 cb の審査の前に入れる（行 cg の FR54 の字も同じ round）。
- 行の順: cb → cc・cd・ce・cf（互いの write-set が交わらない・cf と cg は table.rs で交わるので器の交差の判定が並べない）→ host ごとの PATH の binary の入れ替え → 隣の project へ知らせ（ADR の着地・key の書き方・binary の版・`design-doc-paths` との関係）→ 先方が key を書く → 置き場の外を指す live な便が 0 本 → cg。
- 各行の write-set と verify と done は区間の行 cb〜cg（起票の前に `pipe preflight` で受付の断りと閉包と余地を測って確定する）。
- 同じ file を触る未着地の行（§66 の行 bw・by: check.rs・table.rs・declaration.rs・optional_keys.rs・intake.rs）が先に着地すると、本節の行の余地の見込みと TableError の母集団の数が動く。done は数を「n 増やす」の相対で書いた。
- 歯の接頭辞は crates/ に 0 件で、着地済みと未着地の行の verify の filter 語（1030 語）を部分に含まないことを main a6d4234c で確かめた（`contract_`・`pipe_intake_`・`hook_live_row_`・`ledger_form_`・`lifecycle_mark_` を含む名を避けた）。接頭辞の後ろに付ける名にも、これらの語を入れない。
