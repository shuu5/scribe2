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
<!-- contracts:end -->`（CLAUDE.md の憲法区間と同じ marker 形・行走査で区間を抜いて同じ parser に渡す・設計 doc 1 本に区間は 0 か 1 つ）(ii) 後続 = folio2 が設計ノート（YAML 正本）から導出する tracked な `.toml` 1 file（全文を同じ parser に渡す・folio2 planner との擦り合わせ 2026-09-13・scribe2 側は path を差し替えるだけ）。読み手は path の拡張子（`.md` = 区間 / `.toml` = 全文）で形を決め、それ以外は typed に断る。
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
- **上限の余地**: write-set の `.rs` file のうち **R-C4-2 の測定範囲と同じ `crates/<c>/src/` 配下のもの**（xtask check の file-lines が数える file・`tests/` の歯と `xtask` の外の file は門の対象外ゆえ余地を求めない＝受付が門より広くならない・s2-07l.249 run 7 の受付が 2000 行級の test file で .206 / .245 を断った 2026-09-14 の型）について、base の行数と R-C4-2 の値の差（余地）を測り、行の `size` の見積が余地を超える file が在れば `Refuse::CapHeadroom { file, headroom, size }` で断る。core の合計（R-C4-1）も同じ式で 1 回（core の見積 = write-set の `.rs` 本数 × 同じ値）。**縮む面（`-` 接頭辞）は余地を求めず、core の見積の本数にも数えない**（`size` は「1 file あたりの増分の見積」で、縮む面の増分は 0 以下と宣言されている＝余地 96 の file を 3 module へ割る便が、割る前の file に +S を当てられて断られる形〔s2-07l.279・受付が満杯の file を割る便そのものを断る＝満杯が固定される〕を作らない）。宣言を信じるのは受付の先読みだけで、上限そのものは gate の `cargo xtask check`（R-C4-2 の deny 線・測定値）が守る＝宣言が偽でも上限は超えられない（C10・宣言値は測定を経ずに有効値にならない）。縮む面が本当に縮んだかを gate が測る（head の行数 ≤ base の行数・fail-closed）のは後続（gate.rs の分割 s2-07l.286 の後）。**S / M / L と行数の対応は rules 行 3 本**（`pipe.size_s_lines` / `pipe.size_m_lines` / `pipe.size_l_lines`・1 file あたりの増分の見積・[rules-manifest.md](./rules-manifest.md) §4・user 裁定 2026-09-14）で、値は rules の読み手から取る（数を code に書かない）。**行の数え方**は xtask check の file-lines / core-lines と同じ式（幅で正規化した行数・R-C4.line-width・rules-manifest.md §4）＝1 行に詰め込んでも余地は増えない。core と xtask は互いに依存しないので式は 2 か所に在り、同じ fixture で両方を突合する歯が一致を守る。出所 = s2-07l.189 run 1（land.rs 1495/1500）・s2-07l.208 run 4（core 上限）・s2-07l.249 run 2（cli.rs / land.rs 1498/1500・満杯の file に M を当てる契約表）。**撃つ場所は受付だけ**（`pipe intake` が起こす 1 行に対して・`contracts check`〔CI〕は撃たない）: 余地は「その便を今の base に当てたら入るか」という**受付時点の事実**で、契約表の行は履歴を持つ（landed 済みの行 a / f の write-set の file はその後も育つ・表は状態を持たない〔C15・状態は台帳〕）ので、表の全行に当てると landed 行が上限で永久に違反になる（s2-07l.249 run 3・check.rs 1499 / fleet/mod.rs 1287）。§3 の他の 3 つ（外形 pin・項目の実在と展開・名指しの実在）は base に対する静的な事実なので受付と CI の両方が撃つ。
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
- **lint の置き場と読み口（現物・verified 2026-09-20）**: 台帳 adapter の module（`crates/scribe2/src/ledger/mod.rs`）は着地済みで、**書きの `close` と `CloseError` と `POLARITY` だけ**を持つ（lint も読みも無い）。読みは席の側の `crates/scribe2/src/seat/ledger.rs`（`DEFAULT_BD` を adapter が `pub use` で借りている）に在るものを**そのまま使い 2 本目の reader を作らない**（C2）——現物の `read_ledger(` は全件（`--all --limit 0 --json`）を取り、1 件の型は id と status を持つ。lint は open の絞りをその戻り値に掛け、**acceptance と label を 1 件の型へ足す**（同じ 1 本の reader を広げる＝`issues_of(` の parse の側に欄が増える）。lint 自身は adapter の子 module（新規 file・`crates/scribe2/src/ledger/mod.rs` に宣言を 1 行）に置き、**極性は増やさない**（doctor は読むだけで判定しない・C10.2＝`crate::polarity` の列と外形は不変）。歯の file も新規で、e2e の親（`crates/scribe2/tests/e2e/main.rs`）に module の宣言を 1 行足す。
- **lint の形（番号は done と歯の対）**:
  1. **読む**: 読みの口で open の bead を全件取り、読めない周（client が起動できない・rc ≠ 0・出力が読めない）は件数 0 に**倒さず**、行を測れていない形（`ledger: unreadable reason=<語>`）で出す（C10・NFR4）。
  2. **数える**: 3 つの欠陥 (i)(ii)(iii) を純関数で判定し、**件数と母集団を同じ行に**出す（母集団 = open の全件と契約の件数と memo の件数）。0 件の周も 0 を出す（行が消えない）。
  3. **名指す**: 欠陥の在る bead の id を、3 つの欠陥ごとに宣言順で列にして行の後に出す（件数だけで終わらせない）。
- **歯**（`ledger_lint_` 接頭辞・置き場は新規の歯の file・偽の台帳 client は PATH の先頭に置く shim で、引数と出力を歯の中で組む）: (a) 3 つの欠陥を**件数を違えて**持つ fixture（pointer の解けない契約 1 件・本文を持つ契約 2 件・pointer 無しの memo 3 件）で `unresolved=1 bodied=2 unpointed=3` と母集団が出て、id が 6 つとも名指される（件数を揃えると 3 つの数と欠陥種の対応が pin されず、render の数を入れ替える変異が生存する＝gate の lens が実測した空虚性）／(b) 欠陥 0 の fixture で 3 つの数が 0 になり母集団だけが在る（**0 と不在を弁別**する (2) の枝）／(c) client が起動できない周・rc ≠ 0 の周・出力が壊れた周は 3 とも 0 でなく測れていない形の行になる（(1) の否定の枝・現物が 0 を出す形なら落ちる）／(d) pointer が**解ける**契約と `## memo` の見出しを持たない memo は数に入らない（偽陽性の pin）。doctor の項目列の**外形**は `crates/scribe2/src/main.rs` の in-file の歯（snapshot `crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap`）が受け、台帳の 1 行が増えた差分がそこに写る。この歯は接頭辞 `ledger_lint_` で新しく 1 本足す——名は `ledger_lint_doctor_external_form` で、insta が歯の名から作る snapshot file は行 e の write-set の `+` の snapshot（`crates/scribe2/src/snapshots/` の下・`scribe2__tests__` + 歯の名）である。既存の外形の歯の code は 1 字も動かさないが、その snapshot（write-set の既存 file）は doctor の 1 行が増えた分だけ動く（既存の外形の歯の名を verify の filter に書くと、席の doctor の外形の歯の名がその字面を**末尾に含む**ため filter が 2 file に当たり、触らない file を write-set に要求する＝2026-09-20 の preflight で実測）。
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
- (vii) の続き: const slice の名は 2 つで**別**にする（`ALL` のような同名にしない）: 2 つの cli module は最後の段が同じ `cli` なので、`sees` の (b)(c) は module `cli` の `ALL` を seat / pipe のどちらの型の件数 pin とも読み、両者の閉包が互いの歯の file を拾う（多段 module を最後の段で弁別する下界の限界・§3）＝名を分けて 2 つの閉包を素にする。件数 pin の歯は `crates/scribe2/tests/e2e/seat.rs` / `crates/scribe2/tests/e2e/pipe.rs` に置き、`vessel::seat::cli::SEAT_COMMANDS.len()` / `vessel::pipe::cli::PIPE_COMMANDS.len()` の修飾形で書く＝§3 (iii) の件数 pin（const slice の名を (c) 修飾で解く・module は型名の直前の 1 段 = `cli`）でその歯の file だけが自分の型の閉包に入る。親 module（`seat/mod.rs` / `pipe/mod.rs`）は `pub mod cli;` を既に持つ（現物）ので触らない。base には型も const slice も無いので、その歯を base に当てた周は e2e binary ごと compile error＝flip-check はこれを RED と数える（overlay 後の compile error は RED の規則・`crates/xtask/src/flipcheck.rs` の module doc）。
- 歯（接頭辞と置き場・done と 1:1）:
  1. `contract_derive_creates_parent_`（`crates/scribe2/tests/e2e/pipe/intake.rs`・toy repo の tracked は歯の中で組む）= `creates` に新設 file を宣言した行の導出値に**親の宣言 file が入る**ことを **(vi) の 3 形とも**測る: (vi-1) 親が `<dir>/mod.rs` の周／(vi-2) 親が `<dir>.rs` の周／**(vi-3) 項目が `crates/<c>/src` の直下で `lib.rs` と `main.rs` が両方 tracked の周は 2 面とも入り、`main.rs` だけが tracked の crate は 1 面だけ入る**（`<dir>/mod.rs` と `<dir>.rs` の 2 形しか持たない実装ではこの周の導出値が空になって落ちる）／親の候補がどれも base に無い周は導出値に足さない（**否定の枝**）／`.rs` でない項目と dir を持たない項目は親を持たない（**否定の枝**）。
  2. `contract_derive_subcommand_enum_`（同 file）= `touches` に cli の型を宣言した行の導出値に **cli.rs と件数 pin の歯の file が入り**、もう一方の cli の歯の file は**入らない**（名を分けた効き目の pin＝同名なら落ちる）。
  3. `seat_command_all_` / `pipe_command_all_`（`crates/scribe2/tests/e2e/seat.rs` / `crates/scribe2/tests/e2e/pipe.rs`）= const slice の件数と宣言順が型と一致し、`as_str` と `parse` が往復し、**未知の token は `parse` が `None`** を返す（label の腕へ落ちる側の pin）。
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
  1. **読みと書き**（`crates/scribe2/tests/e2e/pipe/intake.rs`）= FAIL の周に `kind` と `at` が **`review.json` の任意 field に**残り、同じ周の event の detail は **`verdict:<V> kind:<k>` の 2 語だけ**である（**`at` は event に載せない**＝`at` は `,` 区切りの語の列なので空白区切りの detail に載せると `pipe report` が detail から `kind:` を読む面と衝突する・形 (2) の線）／6 語をそれぞれ書いた lens の周でその語が両面に逐語で残る／**PASS の周は `review.json` に `kind` も `at` も持たず** detail は `verdict:PASS` のまま（否定の枝）。
  2. **7 語目へ倒す枝**（同じく `crates/scribe2/tests/e2e/pipe/intake.rs`）= FAIL / INCONCLUSIVE で `kind` が無い周・語でない周・JSON が読めない周は `unparsed` になり **verdict は lens の値のまま**（`other` にも INCONCLUSIVE にも化けない・C10）／lens の判定に届かず器が作る INCONCLUSIVE 5 形（`--lens` 無し・写しを読めない・起動できない・出力を読めない・箱の中で死んだ）も `unparsed`。この 5 形の歯は既存の置き場に倣い `crates/scribe2/tests/e2e/pipe/ratelimit.rs` / `crates/scribe2/tests/e2e/pipe/stop.rs` の審査の fixture を使う。
  3. **数える面**（`crates/scribe2/tests/e2e/pipe/spawn.rs`・`pipe report` の歯の file）= `review_fail=<本数>` が母集団（`RunStage stage=Reviewed` のうち verdict が PASS でないもの全部）と一致し、`by_kind=` が**宣言順に 7 語とも出る（0 も出す）**／`kind:` を持たない古い event は `unparsed` に数える。
  4. **雛形の外形**は既存の歯が受け、行の verify が完全名 `headless_lens_contract_prompt_external_form`（`crates/scribe2/tests/e2e/headless.rs`・snapshot `e2e__headless__lens_contract_prompt_external_form.snap`）で撃つ＝雛形に `kind` と `at` の穴を足した差分がそこに写る。
  5. **`report` の既存 token が不変**であることは既存の歯 2 本が受け、行の verify が完全名 `pipe_report_counts_human_events` と `pipe_report_counts_landed_runs_not_landed_events`（どちらも `crates/scribe2/tests/e2e/pipe/spawn.rs`）で撃つ。
- 触らない: verdict の 3 値と rc・`review.json` の既存 key・lens の起動の形（`{contract}` / `{design}` / `{requirements}`）・審査の観点 3 つ・gate の verdict.json（審査の段だけ）・`report` の既存 token（`runs=` / `landed=` / `human_events=`）。
- 却下案: memory / notes の散文で型を数える（N2・母集団が測れない）／`evidence` の字面を grep して型を推定する（自由文の字面判定・C3.3）／型を rules 行に置く（型は理由の語彙であって閾値でも極性でもない・閉じた enum の領分）／`kind` を PASS にも必須にする（PASS に理由の型は無い・空の値を作らない）。

## 23. 同型の審査 FAIL が N 回目で材料不変の run N+1 を受付が断り、直前の指摘（at）に対応する差分の無い焼き直しを受付が断る（契約表の行 w・`s2-07l.396`）

- 何が起きているか: §22 の後続（user 裁定 2026-09-16T05:53Z・同型の停止の回数 N = 2・逐語は台帳 `s2-07l.395` notes）。§22 が Landed すると `review.json` と event に kind と at が残るが、受付（`cli/intake.rs` の `intake_run`・§21 の judge の側）は同じ bead の前の便の判定を読まない＝同じ kind の FAIL が何周続いても run N+1 は無限に出せ、焼き直しが前回の指摘（at）に触れていなくても通る（本日の実測: `.209` の 10 周のうち 7 周が同型「字面が現物と合わない」）。現物（verified・main a620600）: 受付の断りは write-set の弁別（§3）・上限の余地・live な便との交差（`exclude_overlap`）だけで、便の履歴を読む口は無い。審査の材料は run dir の `review/` に残る（`keep`・契約の写し + `design.txt`〔行の section の本文〕+ `requirements.txt`）。lens の verdict は同じ材料でも揺れる（`.380` で PASS ↔ FAIL）ので、材料不変の再 intake を 1 回も許さない形は採らない＝回数の線は rules 行。
- 形: (1) **rules 行** `review.same_kind_stop`（`RuleKind` の variant `ReviewSameKindStop`・Int・**値 = 2**・裁定 id `user 2026-09-16T05:53Z`・C5・宣言順の末尾・値は manifest だけが持ち本節は写さない）。行の無い manifest は受付を 1 byte も動かさない（rc 2・行を名指す・`pipe.land_wait_s` と同じ極性）。(2) **同型の停止（受付の門・run dir も event も作らない・write-set の弁別の後・余地と交差の前）**: 受付は置き場の replay から同じ bead の便を id の新しい順に並べ、段が Reviewed 以降の便の `review.json` を読み（読めない便は `WriteSetUnreadable` と同じ断り＝`live` と同じ読み手・段が Intake の便は数えない）、先頭の便の kind と同じ kind が verdict PASS で途切れるまで連続する本数を数える（kind が unparsed の便は数えず連鎖も切らない＝lens の欠けを契約の型に化けさせない・C10）。本数が値に達し、かつ先頭の便の材料（`review/` の契約の写しと `design.txt`）が今回の材料（受付が写す形の契約 file〔導出値を置いた後〕と base から読む節の本文〔§4 の `design_text`・同じ 1 本〕）と両方とも同じ字面の周は、`Refuse` の新 variant `SameKindRepeated { kind, runs }`（名 `same-kind-repeated`・rc 1・runs = 数えた便 id の列・新しい順・理由の 1 行に kind と本数と行の値）で断る。契約か節のどちらかが変わっていれば通す（「焼き直しは書き直し」を器の線にする・§7 の形）。(3) **焼き直しの門（受付の門・同じ場所・停止の後）**: 同じ bead の直前の便（新しい順の先頭）の verdict が PASS でない周、その `review.json` の kind と at の各項目に「対応する差分」が在るかを **kind ごとに 1 関数**（`review.rs`・閉じた型 FindingKind〔§22〕の網羅 match）で測る: teeth-outside-write-set → at の各 path が今回の write-set（弁別済み・dir 項目はその配下）に在る／literal-mismatch → at の各識別子が今回の契約 file と節の本文に無い、または base に解ける（`NameUnresolved` の名指しの読み手と同じ 1 本）／section-material-missing → 節の本文が直前の便の `design.txt` と異なる。対応の無い項目が 1 つでも在る周は `Refuse` の新 variant `FindingUnaddressed { kind, at }`（名 `finding-unaddressed`・rc 1・at = 対応の無かった項目だけ・辞書順）で断る。goal-done-contradiction / vacuous-assert / other / unparsed と at の空な周は測れない＝通す（判断を要する型は planner に残す・裁定の (2) の線）。(4) 断りは §21 の preflight にもそのまま出る（judge の側に置く＝run を作らずに撃てる・planner が edit time に測る）。
- 触らない: 審査の段（§4）と lens の起動・§22 の kind / at の書き方と `review.json` の既存 key・verdict の 3 値と rc・回数の値（manifest だけが持つ）・交差と余地の判定・`.394`（§21）の judge / create の割り方（先に Landed なら judge の中に置き、後なら `exclude_overlap` の隣に置いて `.394` が寄せる）・台帳（受付は run を作らないので QUESTION event の宿主が無い＝断りの 1 行と preflight で planner に届く）。
- 歯（`pipe_intake_repeat_` 接頭辞・`tests/e2e/pipe/intake.rs`・偽 lens が最終行の JSON に kind と at を書く §22 の fixture・toy repo の設計 doc と契約 file を歯が書き換えて commit する）: 同じ kind の FAIL 2 便の後、契約 file と節の本文がともに不変の 3 便目は `same-kind-repeated` と 2 便の id を名指す／節の本文か契約 file のどちらかを変えると通る／kind が違う 2 便は通る／unparsed 2 便は通る／PASS を挟むと数え直す／teeth-outside-write-set at=path の便の後、write-set に path の無い契約は `finding-unaddressed` と path・在れば通る／literal-mismatch at=識別子 の便の後、識別子を書いたままで base に無い契約は断られ・消すか base に足すと通る／section-material-missing の便の後、節の本文が不変の契約は断られ・変えると通る／行の無い manifest は rc 2 で行を名指す。rules 行は `rules_review_same_kind_` 接頭辞（`crates/scribe2/tests/e2e/rules.rs`・値と kind と裁定 id と宣言順の pin・行と variant を対で足させる）で、**外形**は既存の歯が受け行の verify が完全名 `rules_external_form`（同 file・外形 snapshot `e2e__rules__rules_external_form.snap` の `rows=` / `kinds=` が 1 つ増える）で撃つ。
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
  - (b) は **e2e**（`crates/scribe2/tests/e2e/pipe/intake.rs`）。`usages(` は `crates/scribe2/src/pipe/closure.rs` の私有で、`crates/scribe2/src/pipe/closure.rs` は R-C4-2 の余地が薄い（§29 の実測）ため in-file の歯を増やさず、`surfaces` を宣言した契約を受付に通して外形 pin の閉包の結果で測る。
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

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "契約表の parser と検査（contracts check）・台帳の pointer 形"
req = ["FR47", "FR48", "FR54", "FR55"]
section = "2"
touches = ["crate::pipe::refuse::Refuse", "crate::polarity::Guard"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/polarity.rs", "contracts/schema.toml", "crates/xtask/src/check.rs", "crates/xtask/src/check_facts.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/", "crates/scribe2/src/snapshots/", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table/check.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_"]
size = "M"
done = "contracts check が本 doc の区間を全件通し、閉包が足りない fixture を名指す"

[[contract]]
id = "b"
title = "intake の生成（--design）と --contract の廃止"
req = ["FR53", "FR54", "FR48", "FR39"]
section = "2"
touches = ["crate::pipe::refuse::Refuse"]
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs"]
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
write-set = ["crates/scribe2/src/fleet/mod.rs", "+crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2/src/pipe/regate.rs", "+crates/scribe2/src/pipe/follow_step.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/health.rs", "+crates/scribe2/src/ledger/memo.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "+crates/scribe2/tests/e2e/pipe/review.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "docs/design/contract-source.md"]
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
write-set = ["rules/manifest.toml", ".vessel.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2/src/pipe/stop.rs", "+crates/scribe2/src/pipe/health.rs", "+crates/scribe2/src/pipe/land/finish.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_land_"]
size = "M"
done = "偽 remote + 偽 CI + 偽 adapter で Landed → close の 3 event・failure は close しない・consumer の要件面の path を宣言で受ける（拡張子で読み手分岐・無ければ既定・既定も無ければ断る）"
depends = ["b"]

[[contract]]
id = "e"
title = "台帳 lint（doctor の項目 1 行）— open の bead から pointer の解けない契約・pointer 無しの memo・本文を持つ契約を件数と母集団と id で名指す"
req = ["FR51"]
section = "6"
write-set = ["crates/scribe2/src/ledger/mod.rs", "+crates/scribe2/src/ledger/lint.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/seat/ledger.rs", "+crates/scribe2/tests/e2e/ledger.rs", "crates/scribe2/tests/e2e/main.rs", "crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap", "+crates/scribe2/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap"]
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli.rs", "contracts/schema.toml", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table/check.rs"]
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/contract.rs", "contracts/schema.toml", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure/names.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_derive_"]
size = "M"
done = "write-set 無しの fixture 行が導出値で intake を通り契約 file に導出値が載る・手書きが導出値とずれた fixture 行は missing / extra を名指して断られる・also の .rs / tests の非歯 file / 解けない filter は typed に断られる・schema.toml に creates / tests / also が載り write-set が任意・現物の契約表の intake 済みの行は断られない（受付だけ・CI は従来の閉包）"
depends = ["g"]

[[contract]]
id = "i"
title = "契約表の + は land すると解けなくなる — 契約表の検査では tracked な + を実在 file と読む（intake は断ったまま・閉じた型の値 1 つ）"
req = ["FR48", "FR39"]
section = "3"
write-set = ["crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_table_landed_plus_"]
size = "S"
done = "land 済みの + 項目を持つ行が contracts check で 0 件・intake は同じ行を断ったまま"
depends = ["a"]

[[contract]]
id = "j"
title = "閉包の 4 形が同名の型の file へ広がる — sees() 1 関数で「その file から型が見えているか」を判定してから数える"
req = ["FR48", "FR39"]
section = "3"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/prop.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_same_name_"]
size = "S"
done = "別 module の同名の型を持つ toy で閉包が広がらず、見えている file だけが導出値に入る"
depends = ["a"]

[[contract]]
id = "k"
title = "要件本文の読み手を要件面の形ごとに — yaml の id + text と md の見出しを lens の材料に載せ、consumer の審査が要件面を読めないで終端しない"
req = ["FR49", "FR2"]
section = "4"
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_check_reads_requirements_ pipe_review_reads_requirements_"]
size = "S"
done = "yaml / md の要件面を宣言した toy で contracts check の id 検査と lens の要件本文が同じ読み手を通り、本文の無い id は理由付きで材料に載る"

[[contract]]
id = "l"
title = "受付の門の判定式の検出線の生存 9 本に歯を足す — §31 の (a) 余地の段の除外の否定 1 本・(b) 外形の usage 行の名 2 本・(c) 余地の境界 1 本・(d) core の名の切り出し 1 本・(e) 節の切り出し 4 本の計 9 本を in-file と e2e で赤にする（歯だけ・門の判定は動かさない）"
req = ["FR48", "FR47"]
section = "31"
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
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
verify = ["cargo nextest run -p scribe2 --no-tests=fail declaration_"]
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_surfaces_"]
size = "S"
done = "外形を触る契約の導出値に導出の実装 file が入らず、閉包の便との偽の交差が消える"

[[contract]]
id = "q"
title = "write-set の導出に (vi) creates の親 mod の宣言 file と (vii) subcommand の閉じた enum（SeatCommand / PipeCommand）を足す"
req = ["FR48"]
section = "17"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_derive_creates_parent_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_derive_subcommand_enum_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_command_all_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_command_all_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "M"
done = "creates を宣言した行の導出値に §17 (vi) の 3 形とも親の宣言 file が入る＝(vi-1) <dir>/mod.rs の周と (vi-2) <dir>.rs の周に加えて、(vi-3) 項目が crates/<c>/src の直下である周は同じ dir の lib.rs と main.rs のうち base の tracked に在るものが全部入り（両方 tracked の crate は 2 面・main.rs だけの crate は 1 面）、親の候補がどれも base に無い周と .rs でない項目と dir を持たない項目は 1 面も足さない。seat と pipe の cli の既知の verb が閉じた enum と別名の const slice になって件数と宣言順が型と一致し往復し未知の token は parse が None を返し、口座 label の短い形の腕と各腕の rc は不変で、cli の型を touches に宣言した行の導出値に cli.rs と自分の件数 pin の歯の file だけが入り（もう一方の cli の歯の file は入らない）、usage の字面と 2 つの外形 snapshot は 1 字も変わらない"

[[contract]]
id = "r"
title = "write-set の導出に fn 形の touches（crate::module::snake_ident）を足し、その fn を宣言する file を閉包に入れる"
req = ["FR48"]
section = "18"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_derive_fn_"]
size = "S"
done = "touches に fn 形を書いた契約の導出値にその fn を宣言する file が入り、宣言する file が無い周は typed に断られ、型形の閉包は不変"

[[contract]]
id = "s"
title = "write-set の導出に enum の variant 構築（<Type>::<Variant> { / ( を => の右辺や Err(…) の中で作る file）の第 6 形を足す"
req = ["FR48"]
section = "19"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail closure_variant_construction_", "cargo nextest run -p scribe2 --no-tests=fail closure_picks_each_of_the_four_forms_from_its_own_file", "cargo nextest run -p scribe2 --no-tests=fail contract_closure_ext_real_table_has_zero_findings"]
size = "S"
done = "(1) variant 構築だけを持つ file が導出値に入り、(2) Self:: の構築だけの file・小文字始まりの項目だけの file・型が見えていない同名の file は入らず => の左のパターン側の出現を述語が数えず、(3) 既存 4 形の導出値は不変で、(4) 第 6 形で閉包が広がる本 doc の行 a / b / d / g / h / w の write-set に 19 項目を同じ便で追記して contracts check が findings 0 のまま"

[[contract]]
id = "t"
title = "Declared 行にも歯の置き場の門を撃つ — verify の filter 語が base で解く歯の file が write-set に無ければ file を名指して断る"
req = ["FR48"]
section = "20"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_declared_teeth_"]
size = "S"
done = "Declared 行の verify の歯の file が write-set の外に在る契約を受付が file を全部名指して断り、中に在る契約と nextest 形でない verify の契約は従来どおり通る"

[[contract]]
id = "u"
title = "pipe preflight — 受付の判定を judge / create に割り、judge だけを run を作らず撃って断りと事実を全部 1 行 1 事実で出す口"
req = ["FR48"]
section = "21"
write-set = ["+crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "-crates/scribe2/src/pipe/closure.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_preflight_", "cargo nextest run -p scribe2 --no-tests=fail pipe_external_form"]
size = "M"
done = "preflight が run dir も event も作らずに受付と同じ断りを全部列挙して rc 0 / 1 / 2 を返し、intake の断りの先頭 1 件と一致し、usage に preflight が載る"

[[contract]]
id = "v"
title = "審査の理由を閉じた型 FindingKind で review.json と event に残し、pipe report が by_kind で数える"
req = ["FR49"]
section = "22"
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/pipe/report.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_kind_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_lens_contract_prompt_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_report_counts_human_events", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_report_counts_landed_runs_not_landed_events"]
size = "M"
done = "FAIL / INCONCLUSIVE の周の review.json が閉じた 6 語の kind と at を任意 field で持ち、同じ周の event の detail が verdict:<V> kind:<k> の 2 語になり（at は event に載せない）、PASS の周は review.json に kind も at も持たず detail が verdict:PASS のままで、kind の無い・語でない・JSON が読めない周と器が作る INCONCLUSIVE 5 形は 7 語目 unparsed に倒れて verdict は lens の値のまま、report の 1 行に review_fail= が母集団つきで出て by_kind= が宣言順に 7 語とも（0 も）出て古い event は unparsed に数えられ、lens の雛形の外形 snapshot に kind と at の穴が写り、report の既存 token と verdict の 3 値と rc は不変"

[[contract]]
id = "w"
title = "同型の審査 FAIL が rules 行 review.same_kind_stop の回数に達した bead の材料不変の run N+1 を same-kind-repeated で断り、直前の at に対応する差分の無い焼き直しを finding-unaddressed で断る"
req = ["FR49"]
section = "23"
touches = ["crate::pipe::refuse::Refuse", "crate::rules::RuleKind"]
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/rules-manifest.md", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs"]
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
tests = ["crates/scribe2/tests/e2e/pipe/intake.rs"]
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
tests = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_impl_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_names_impl_"]
size = "S"
done = "impl する file が宣言する fn の型の path 形が解け、fn の無い項目と impl 行の無い file の同名 fn は name-unresolved のまま・現物の契約表は違反 0 のまま"
depends = ["y"]

[[contract]]
id = "aa"
title = "（消した・s2-07l.476）受付が契約の散文（goal / done）の歯の名指しを verify の filter と write-set に突合し、判定行 token の pin（第 7 形）を閉包に足す"
req = ["FR48"]
section = "27"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
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
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "contracts/schema.toml", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_derive_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_render_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_need_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_names_are_pinned_in_declaration_order", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_promise_"]
size = "M"
done = "(1) 約束の行を 1 つでも持つ行は WriteSet の 3 値目 Promised に弁別され、symbols の閉じた型が touches に・+ の file が creates に・.rs でない file が also に・place が tests に・_external_form の歯と名付き snapshot の歯が surfaces に写り、write-set が §3 の derive_write_set の値と一致して契約 file と runner の allowlist に載る (2) verify は teeth を（crate・scope）で束ねた nextest 行（filter は完全名を空白で並べる）・done は n の順の (n) expect の 1 文として契約 file に生成され、設計 doc には書き戻らない (3) Promised の行が write-set / touches / surfaces / tests / also / creates / done のどれかを持つ・symbols の + 無しの名が base に無い・+ 付きの名が base に在る の 3 形は Refuse の値（新しい 2 値）で断られ run dir が 0・REFUSALS の長さを pin する歯が新しい母集団で緑 (4) verify を持つ Promised の行は生成値と集合一致なら受付を通り、不一致は §3 と同じ drift の断り (5) FIELDS の done と verify が Need の 3 値目 Conditional になり、約束の行を持つ行はその 2 欄が無くても parse を通り、持たない行は TableError の必須 key の欠けで名指され、contracts schema の生成物に conditional が 2 欄で載って cargo xtask check の contracts-schema が緑"

[[contract]]
id = "ah"
title = "Promised の行の審査 — lens の雛形に約束の 4 欄を渡し、verdict の kind を VacuousAssert / GoalDoneContradiction / Other の 3 値に限り、焼き直しの門は契約 file の sha だけを見る"
req = ["FR49"]
section = "33"
depends = ["ag"]
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "+crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_promise_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_review_ pipe_review_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_lens_ pipe_intake_repeat_ pipe_intake_promise_"]
size = "S"
done = "(1) Promised の行の lens の雛形に約束の行の写し（n / text / fixture / expect の 4 欄・n の順）が載り、約束の行を持たない行の雛形は 1 字も変わらない（外形 snapshot） (2) Promised の行の review.json の kind が 3 値の外なら verdict が INCONCLUSIVE に倒れ、3 値の中ならそのまま (3) Promised の行の焼き直しの門は契約 file の sha が変わった周だけ通し、at の path 照合を撃たない〔歯 pipe_intake_promise_rework_gate_reads_only_the_contract_sha〕 (4) 約束の行を持たない行の審査と門が不変＝既存の headless_lens_ の歯と外形 snapshot・review.rs の pipe_review_ の歯・焼き直しの門の pipe_intake_repeat_ の歯が 1 字も変わらず緑（verify の 2 行がそのまま走らせる）"

[[contract]]
id = "ai"
title = "約束の行の files の + 無しの .rs を write-set にそのまま写し（Fields の 7 つ目・derive_write_set の (vi)）、symbols の crate:: で始まる型の path 形を受付が module の型の宣言で解く（閉じた型の読み手と同じ 1 本）"
req = ["FR47", "FR48", "FR39"]
section = "34"
write-set = ["crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_promise_files_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_promise_files_"]
size = "S"
done = "(1) files の + 無しの .rs（base に実在）が導出の write-set にそのまま載り、base に無い .rs は ItemUnresolved で断られる (2) symbols の crate::<module>::<Type> は module の enum / struct の宣言で解けて touches に写り、+ 付きは宣言が在れば断られ、末尾 2 節の型::項目の形は従来どおり (3) 既存の contract_promise_ / pipe_intake_promise_ / pipe_intake_repeat_ の歯と外形 snapshot が 1 字も変わらず緑"

[[contract]]
id = "aj"
title = "焼き直しの門の teeth-outside-write-set の物差しは at のうち path の形に解ける項目だけを測り、path でない項目（歯の接頭辞・§ の番号）は測れないとして断りの理由から外す"
req = ["FR49", "NFR4"]
section = "35"
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
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
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_names_declared_"]
size = "S"
done = "(1) 検査の文脈が宣言済みの新規 file の列を持ち、tracked な設計 doc の区間の全行から印つきの write-set の項目と creates の欄を集める 1 本で組まれる (2) 別の doc の行が宣言した新規 file を名指した行に name-unresolved が出ず、どの行も宣言していない名は従来どおり 1 件出る (3) 同じ母集団を CI の駆動と受付の材料の両方が同じ 1 本から受け、受付でも同じ行が通る (4) 区間を読めない doc が在る周は母集団を縮めたまま通さず従来の読めなさの 1 件が出る (5) name-unresolved の字面と在り処の形・型の path 形と fn 形の解き方・write-set の項目の実在・depends の母集団・findings の順と rc が不変で、現物の契約表は findings 0・rc 0"
[[contract]]
id = "ao"
title = "審査の材料に write-set の各項目の base の要約（行数の 2 面・本体の宣言の名・歯の名）を 1 file として足し、lens の雛形の穴 1 つを器が埋める。観点 3 つと理由の型と既存の 3 材料は不変で、cap を越える周は要約の段だけ落として本数を残す"
req = ["FR49", "NFR1"]
section = "40"
write-set = ["crates/scribe2/src/pipe/review.rs", "+crates/scribe2/src/pipe/review/base.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_base_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_base_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_base_"]
size = "M"
done = "(1) 審査の材料の dir に要約の file が 1 本増え、置く側は既存の 3 本と同じ 1 か所で、組む側は 1 回だけ呼ばれる (2) 要約 1 本が項目の path と行数の 2 面を持ち、.rs は本体の宣言の名の列と歯の名の列を別の列として持ち、.rs でない項目は行数だけ・+ の項目は新設の 1 行・読めない項目は読めなさの 1 行になる (3) 材料の写しが在る周は雛形の穴が本文で埋まり、無い周は雛形が 1 字も変わらず、契約の本文の中の穴の字面は展開されない (4) 要約を足すと cap を越える周は要約の段だけが落ちて落とした項目の本数の 1 行が残り、既存の 4 材料だけで越える周は claude を呼ばず INCONCLUSIVE のまま (5) 歯の区間と本体の区間と行数の 2 面の読み手は既存の 3 本のままで、新しい読み手を作らない (6) 審査の観点 3 つの本文・理由の型の 6 語・判定の JSON の形・diff の審査の極性・審査の rc が不変"

[[contract]]
id = "ap"
title = "Declared 行の歯の置き場の門の断りが、write-set に無い歯の file と、それを解いた verify 行の filter 語を対で名乗る。照合の 1 本と門の条件と断りの語と rc は不変"
req = ["FR48"]
section = "41"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_teeth_origin_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_teeth_origin_"]
size = "S"
done = "(1) 行ごとに解いた置き場が file と filter 語の対として畳まれ、同じ file を 2 行が解いた周は verify の先の行の語が付き、対は file の辞書順 (2) 照合の 1 本が対を受けて write-set に無い分を対のまま断りの payload にし、正規化と dir 項目の扱いは不変 (3) 断りの理由の 1 行が file と filter 語の両方を名乗り、受付の側の同じ名の型も同じ欄を持ち理由は導出の側の 1 本を写すだけ (4) 置き場を解く関数の signature と Promised の導出・門を撃つ条件と順・解けない filter の断りの字面・断りの語・rc・run dir を作らないことが不変 (5) 受付の stderr が file と filter 語を両方名乗り rc 1 で run dir を作らない"

[[contract]]
id = "aq"
title = "Declared 行の歯の置き場の逃がしが、write-set の + の新規 .rs も置き場と読む（宣言済みの新規 file を path だけで認める Promised 形と同じ下界）。断りの字面と型・行ごとに解く形・他の理由の断りは不変"
req = ["FR48"]
section = "42"
write-set = ["crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/tests/e2e/pipe/contracts.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_declared_place_new_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_declared_place_new_"]
size = "S"
done = "(1) 逃がしの条件が「base に在る歯の file が write-set に在る」か「write-set の + の項目に .rs が在る」のどちらかになり、base 側だけの周の挙動は不変 (2) + の項目は本文を見ず path だけで置き場と認められ、置き場の欄の項目を creates で照合する既存の弁別と同じ 1 つの規則のまま (3) 逃がしが効くのは解けない filter の断りだけで、読めない source と置き場の欄の項目の不整合はそのまま断る (4) base にも + にも歯の置き場が無い write-set は従来と同じ字面と rc 1 で断られ run dir を作らない (5) 契約表の行が + の新規の歯の file と新しい filter 語だけを持つ周に受付が rc 0 で run dir を作る (6) 既存の負例の歯 2 本が本文も期待も変わらず緑"
[[contract]]
id = "ar"
title = "write-set に「中身を変えない・verify の置き場として載せただけ」を表す項目の印を 1 つ足し、上限の余地の検査と core の見積の本数から外す（縮む面と消える file と同じ腕）。交差・guard・gate の照合は素の path のまま"
req = ["FR48", "FR39"]
section = "43"
touches = ["crate::pipe::declaration::WriteSetItem"]
write-set = ["crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/contracts.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_place_only_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_place_only_"]
size = "S"
done = "(1) 接頭辞の字面の定数が既存 3 本の隣に 1 本増える (2) 閉じた列挙の変種が 1 つ増え、その接頭辞は base に在る file にだけ解け、受付と契約表の検査で読みが変わらない (3) 接頭辞を剥がす 1 か所が新しい字面も剥がし、剥がす規則が 2 か所に増えない (4) 網羅 match 2 か所で新しい変種が余地を求めず core の見積の本数にも数えられない (5) 交差・guard・gate の write-set 照合・契約表の検査が素の path のまま 1 字も変わらず、断りの字面と型と rc が不変 (6) 余地の足りない file を印つきで載せた行が受付を通って run dir が出来、印だけを外すと従来の余地不足の字面で rc 1・run dir を作らない"

[[contract]]
id = "as"
title = "verify 行の -- の後ろの --exact を完全一致の filter として読む — 名の全体の末尾の段を fn 名と等値で照合し、-- の後ろの旗（--skip は引数 1 語）は閉じた列で読み飛ばす。-- の前の読みと宣言の形の門（括弧・引用符を断る）は不変"
req = ["FR48", "FR55"]
section = "43"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/tests/e2e/pipe/contracts.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_teeth_exact_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_teeth_exact_"]
size = "M"
done = "(1) closure.rs に -- の後ろの libtest の旗の閉じた 2 列（引数を取る --skip・取らない --exact / --include-ignored / --nocapture / --no-capture）が増える (2) nextest_filter が -- を境に読みを切り替え、後ろの旗を読み飛ばして裸の語を filter 語にし、--exact が在れば完全一致・無ければ部分一致になる (3) 完全一致は :: で割った末尾の段を fn 名と等値で照合し、同じ段を substring に持つ別 fn の file を置き場に取らない (4) -- の後ろに --exact も裸の語も無い行は従来どおり None で、断りの型と字面と rc が不変 (5) 検出線の語が末尾の段になる (6) -- の前の読み・METACHARS と宣言の形の門・tests 欄の弁別が不変で、現物の契約表（verify 行 208 本・-- を持つ行 0 本）の判定は 1 本も変わらず契約表の歯は緑のまま"

[[contract]]
id = "at"
title = "契約表の検査が未追跡の設計 doc を 1 行知らせ、判定行に未追跡の本数の欄を足す（findings にも rc にも数えない検出線・git が答えない周は ? で 0 に化けさせない）"
req = ["FR55", "NFR4"]
section = "43"
write-set = ["crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/tests/e2e/pipe/contracts.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contracts_untracked_doc_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contracts_untracked_doc_"]
size = "S"
done = "(1) 検査が未追跡の設計 doc を既存の git の 1 本で 1 回引く (2) 判定行に未追跡の本数の欄が 1 つ増え、現物の repo では 0 で出る（判定行を完全一致で読む contracts.rs と intake.rs の既存の歯は新しい欄を含む字面へ更新し、他の期待は変えない） (3) 1 本以上の周は判定行の前に 1 件 1 行で path を名乗り、findings の数も rc も変わらない (4) git が答えない周はその欄が ? になる (5) 追随の口の戻りが 1 つも変わらない (6) 検査の母集団は tracked な設計 doc のままで、findings の順と rc と doc 数と行数の数え方が不変"
<!-- contracts:end -->


## 28. 歯の置き場が verify 行の scope（-p / --test <name> / --lib）を読む — その行が撃てない file を write-set に要求しない（契約表の行 ab・`s2-07l.451`）

- 何が起きているか: 歯の置き場の読み手（§3 (ii) の `teeth_places` と、§20 が同じ 1 関数で通す Declared 行の門 `declared_teeth`）は、verify の nextest 行から crate（`-p` の値）と filter 語だけを取り、`nextest_filter` が scope の旗（`--test <name>` / `--lib`）を落とす。置き場はその crate の **全 file** から「`#[test]` の直下の `fn` の名が filter 語を含む file」を集めるので、`--test e2e` の行（統合 test の target だけを撃つ行）でも `src` の in-file の歯の file を write-set に要求して断る＝**その行が実際には走らせない file** を書く権利ごと要求している。実測（2026-09-17・母集団 = 設計 doc の契約表の nextest 行 220 本）: `s2-07l.447` run 1 が `seat_account_` で src の歯 4 本に当たって断られ（接頭辞を 2 本に割って回避）・`s2-07l.340` が `confine_reasons_` で同型・純移動の `s2-07l.351` は 12 の接頭辞のうち 6 本が src の歯の file を要求し、**歯の名を変えられない純移動では回避できず run が 1 本も起きない**。
- 形: nextest 行の読み手に scope を足す。scope は **閉じた 3 値の enum**（宣言順 = 旗なし / `--lib` / `--test <name>`）で、行の語から 1 関数で解き、置き場の母集団を `in_crate` の後段で 1 述語に畳む: 旗なし = その crate の全 file（従来どおり）／`--lib` = `crates/<crate>/src/` 配下／`--test <name>` = `crates/<crate>/tests/<name>.rs` とその配下。**読めない旗（`--bin` / `--benches` / `-E` ほか）と、scope の旗が 2 つ以上在る行は旗なしと同じ広い側へ倒す**（fail-closed・緩める側は狭く取るの対）。読み手は 1 本のまま（Derived の導出 (ii) と Declared の門 §20 は同じ関数を通る）で、断りの型も字面も増やさない（`TeethPlaceUnresolved` / `TeethOutsideWriteSet` のまま）。
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
- **審査の分担（行 ah）**: 前提（着地済・verified・2026-09-21）: 行 ag（`s2-07l.512`・約束の行の parse / 導出 / render・`WriteSet::Promised`・`Need::Conditional`・`promises_of`）は base に在り、行 ai（`s2-07l.528`・files の既存 .rs と crate:: の型）と行 aj（`s2-07l.529`・§35 の物差し）も着地済み＝行 ah の write-set の外の前提は全部 main に在る。既存の `e2e__headless__lens_contract_prompt_external_form.snap` が行 ah の write-set に在るのは受付が外形 pin の file を要求するためで、本行は 1 byte も変えない（pipeline.md §43 の `polarity.rs` と同じ型・done (1) 後半と (4) は verify の `headless_lens_` の既存の歯がその snapshot と照合して測る）。Promised の行では `FindingKind` のうち `TeethOutsideWriteSet` / `LiteralMismatch` / `SectionMaterialMissing` は器が受付で測り終えているので、lens の雛形（`lens-contract.txt`）に約束の行の写し（`{promises}` の穴・`n` / `text` / `fixture` / `expect` の 4 欄）を渡し、verdict の kind を `VacuousAssert` / `GoalDoneContradiction` / `Other` の 3 値に限る（Promised でない行は従来の雛形の 6 語のまま・7 語目の `Unparsed` は器が倒す側で雛形には無い）。所在（行 ah の write-set の中・`Review` の欄と `step.rs` は触らない）: (i) 審査段が「行が Promised か」を知る経路は `crates/scribe2/src/pipe/review.rs` の `review(` が `entry.repo` と契約 file の `design` の pointer から設計 doc の契約表を読み（`read_table` → `promises_of`・受付と同じ読み手）約束の列を得る 1 か所——契約 file の 9 欄は不変（pipeline.md §3）で、Promised の印は契約 file に書かない。(ii) `{promises}` の穴を埋めるのは `crates/scribe2/src/headless/lens.rs`（既存の穴と同じ置換の口・約束の列が空なら穴は空文字＝Promised でない行の雛形は 1 字も変わらない）。(iii) kind の絞りは `review.rs` の verdict の読み（`FindingKind::parse` の直後）で、約束の列が非空 ∧ kind が 3 値の外なら INCONCLUSIVE に倒す（fail-closed・`FindingKind` の 7 値と `FINDING_KINDS` は不変）。(iv) §23 の焼き直しの門は Promised の行では「契約 file の sha が変わったか」だけを見る＝`crates/scribe2/src/pipe/cli/intake.rs` の `exclude_unaddressed` が `today.row` の `WriteSet::Promised` の周は `at` の物差し（`review::unaddressed`）を撃たずに通り、`exclude_same_kind`（材料不変の N 回目）だけが残る（`at` の path 照合は器が測った項目に対しては起きない・§35 の物差しは Promised でない行のまま）。
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
- 現物（orchestrator が grep と正規化行数で実測・main ca9bb75）: 責務は 6 群（定数 51–77・判定の語彙と読み手 79–267・焼き直しの門 269–362・材料の組み立て 364–559・**要件本文の読み手 561–768**・lens の駆動と決着 770–935）で、in-file の歯は 937 行から（`#[cfg(test)]` の次の非空行が `mod tests {`＝札は `mod tests {` の直後に置ける・[pipeline.md](./pipeline.md) §45 の `#[path]` 形の罠には当たらない）。**要件本文の読み手の群は閉じている**: item は 13 個（`Found` / `requirements_text` / `requirement_row` / `requirement_md` / `md_heading` / `requirement_yaml` / `BODY_KEYS` / `BODY_JOIN` / `yaml_entry` / `unquote` / `Member` / `yaml_member` / `strip_tags`・561–768 行・正規化 **209** 行）で、親の本体から裸で呼ばれるのは **`requirements_text` の 1 site だけ**（438 行・`materials`）、他 module（`crates/scribe2/src/**`・`crates/scribe2/tests/**`）からの参照は **0 site**（`review::` の 31 site を全数確認・`table/check.rs` の 2 件は doc comment の字面）、群が親から引くのは `table::read`（1 site・579 行）と `std::path::Path` だけで、親の const・型・`Contract` は 1 つも引かない。群に struct は無く（`Found` / `Member` は enum）、field を歯が構築する型も無い＝§45 の「親に残す型」の判断は要らない。歯の `use super::{…}`（939–943 行）が名指す群の名は 6 つ（`requirement_md` / `requirement_row` / `requirement_yaml` / `requirements_text` / `strip_tags` / `Found`）。
- 名前解決の形（§45 と同じ・可視性は名前解決をしない）: 親に `use` を置く。解く名は 6 つで、うち**親の本体に site が在るのは `requirements_text` の 1 つだけ**、残り 5 つ（`requirement_row` / `requirement_md` / `requirement_yaml` / `strip_tags` / `Found`）は歯だけが読む。5 つを素の `use` に入れると通常 build で `unused_imports` → `-D warnings` で rc 101 になるので、`use` は**本体用（1 名・素）と歯用（5 名・`#[cfg(test)]` 付き）の 2 文**に割る（属性は別の行・`residual_allowed` が許す残差は `#[cfg(test)]` だけの 1 行）。群の中だけで呼ばれる 7 名（`md_heading` / `BODY_KEYS` / `BODY_JOIN` / `yaml_entry` / `unquote` / `Member` / `yaml_member`）は可視性を 1 語も変えない。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 13 item（561–768 行・正規化 209 行）を、行 al の write-set の `+` の file へ名・本文・順序・doc comment を変えずにそのまま移す（doc comment は item の一部＝1 字も書き換えない・`[`table::requirement_ids`]` の link は子の `use super::table;` で解ける）。子の頭は module doc と `use super::table;` / `use std::path::Path;` の 2 行だけ。
  2. 親に増えるのは **4 行だけ**——`mod` 宣言 1 行（file 頭の `use` 群〔37–49 行〕の直前・親に既存の `mod` 宣言は無い）、本体用の素の `use` 1 行（`requirements_text`・`pub` は付けない・`use` 群の隣）、歯用の `#[cfg(test)]` だけの 1 行と `use` 1 行（5 名・**既存の行頭 `#[cfg(test)]`〔937 行〕の直上**＝file 頭に置くと xtask の src / test の切れ目が最初の行頭 `#[cfg(test)]` へ動き、本体が丸ごと歯の区間に落ちる）。4 行とも 120 桁に収まる。
  3. 歯は 1 本も足さず 1 本も変えない: in-file の `mod tests` の本文と `use super::{…}` は 1 byte も変えない（その `use` は親の `use` 2 文が解く）。e2e（`crates/scribe2/tests/e2e/pipe/intake.rs` の `pipe_review_reads_requirements_` 4 本）は binary 越しで名を引かず、write-set の外。
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
- 歯（接頭辞 `contract_names_declared_`・`crates/scribe2/tests/e2e/pipe/contracts.rs` の既存の `contract_names_impl_` / `contract_closure_ext_` の歯の隣。`crates/` 全体で 0 件＝衝突なし）: (a) doc を 2 本持つ toy repo で、doc A の行が新規 file を宣言し doc B の行の done がその名を backtick で名指す → findings 0・rc 0（**base で RED**: name-unresolved が 1 件）。(b) どの行も宣言していない名を名指した行は従来どおり 1 件（負例・母集団が無条件に広がらない）。(c) 同じ doc の別の行の宣言でも解ける。(d) 受付でも同じ: doc B の行を pointer に受付を撃つと run dir と event が作られる（base は断られる）。(e) 現物の契約表が findings 0・rc 0。
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
  - `pipe_review_base_`（行 ao の write-set の `+` の file の in-file の歯と、`crates/scribe2/tests/e2e/pipe/review.rs` の e2e）: (a) fixture の `.rs` 1 本で、本体の宣言の名と歯の名が**別の列**に出る（母集団 = fixture の宣言の本数と歯の本数を同じ assert で数える）・(b) `.rs` でない項目は path と行数だけ・(c) `+` の項目は新設の 1 行・(d) 読めない項目は読めなさの 1 行・(e) cap を越える周は段が落ちて落とした本数の 1 行が残る・(f) e2e は受付から審査まで通した run の材料の dir に要約の file が在り、write-set の各項目の path を持つ。
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
  - e2e（`crates/scribe2/tests/e2e/pipe/contracts.rs`）: 受付の stderr が file と filter 語を両方名乗り、rc 1 で run dir を作らない。既存の歯 `contract_declared_teeth_outside_write_set_is_refused`（:951）は file だけを測っているので、本行が filter 語の期待を足す（rc と run dir の期待は不変）。

## 42. Declared 行の歯の置き場が write-set の `+` の新規 .rs も置き場と読む（契約表の行 aq・`s2-07l.481`・§20 の逃がしの 1 点）

- 出所（consumer の報告 2026-09-19・逐語は台帳 `s2-07l.481` の notes）: base に 0 本の filter 語と、新設の歯の file と、既存の歯の file を触らない契約が、Declared 行に置き場の欄が無いので受付できない。回避は「本文を変えない既存の歯の file を write-set に載せる」＝要らない file を write-set に足す作法である。
- 何が起きているか（現物・main f25084c・verified）: memo の一部は `s2-07l.391`（§20）で塞がった。`crates/scribe2/src/pipe/closure/derive.rs` の `declared_teeth` は :119 の 1 行で逃がしを立てるが、その条件は「write-set に **base に在る**歯の file（歯の区間が空でない `.rs`）が 1 つでも在る」で、母集団は base の source の本文の列だけである。write-set の `+` の項目は base に無いので本文の列に入らず、**新規の歯の file だけを足す Declared 行は今も解けない filter の断りで落ちる**（既存の歯 `contract_declared_teeth_new_filter_needs_a_teeth_file_in_write_set`・`crates/scribe2/tests/e2e/pipe/contracts.rs` :991 の (c) がこの負例を pin している）。別経路は開いている: Promised 形は `crates/scribe2/src/pipe/closure/derive.rs` の `teeth_file`（:284）が置き場の欄の項目を `creates` の側で照合し、宣言済みの新規 file を**本文を見ずに path だけで**置き場と認める。
- 形（逃がしの条件を 1 つ広げる・下界は Promised 形に揃える）:
  1. :119 の逃がしの条件に、「write-set の `+` の項目に `.rs` が 1 つでも在る」を**または**で足す。base に在る歯の file が在る周の挙動は 1 字も変えない。
  2. 下界は Promised 形と同じにする: 宣言済みの新規 file は本文が無いので path だけで置き場と認める（`teeth_file` が `creates` の項目を照合するのと同じ弁別・2 本目の規則を作らない・C2）。
  3. 逃がしが効く範囲は**解けない filter の断りだけ**で、他の理由（読めない source・置き場の欄の項目の不整合）はそのまま断る＝:125-126 の弁別を 1 字も変えない。
  4. base にも `+` にも歯の置き場が 1 つも無い write-set は従来どおり同じ字面で断る（負例が残る）。
- 触らない: 解けない filter の断りの字面と型・行ごとに解く形（先に在る新しい接頭辞の行で止まらない）・置き場を解く関数と Promised の導出・write-set に無い歯の file の断り・受付の rc と run dir の扱い・§20 の門そのもの。
- 却下: `+` の項目を `tests/` 配下に限る（Promised 形の宣言済みの新規 file は path の位置を見ない＝規則が 2 本に割れる。src の中に歯を置く便を断る根拠も無い）／Declared 行にも置き場の欄を開く（欄の意味が Promised 形と割れ、§33 の導出と衝突する）／base で 0 本の filter 語の行を無条件に読み飛ばす（§20 の門の下界を失い、置き場の検査が空洞化する）／consumer の回避（要らない既存の歯の file を write-set に足す）を作法のまま運ぶ（N2・write-set が事実と食い違う）。
- 歯（接頭辞 `contract_declared_place_new_`・`crates/` 全体で 0 件＝衝突なし。既存の `contract_declared_teeth_` の歯の隣）:
  - in-file（`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・pure な 1 本を直に呼ぶ）: base に 0 本の filter 語 1 つを持つ 3 通の write-set で rc の型を測る（母集団 = 3 通）——`+` の新規 `.rs` を 1 つ持てば通る／`+` が `.rs` でない項目だけなら従来の断り／base の歯の file も `+` の `.rs` も無ければ従来の断り。
  - e2e（`crates/scribe2/tests/e2e/pipe/contracts.rs`）: 契約表の行が `+` の新規の歯の file と新しい filter 語だけを持つ周に受付が rc 0 で run dir を作り、同じ行から `+` の項目を外すと従来の字面で rc 1・run dir を作らない（母集団 = 2 回の受付の rc）。
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
  - 歯（接頭辞 `contract_place_only_`・`crates/` 全体で 0 件）: in-file（`crates/scribe2/src/pipe/declaration/write_set.rs` の歯の区間・pure な 1 本を直に呼ぶ）で、印の項目が base に在れば解け・無ければ従来の未解決・余地の母集団に入らず・core の見積の本数に数えない（母集団 = 同じ fixture の項目の本数と対で数え、素の path の項目は従来どおり入ることを同じ assert で見る）。in-file（`crates/scribe2/src/pipe/refuse.rs` の歯の区間）で `normalize` が新しい接頭辞を剥がす（既存 3 形の隣）。e2e（`crates/scribe2/tests/e2e/pipe/contracts.rs`）で、余地の足りない file を印つきで載せた行が受付を通って run dir が出来、同じ行から印だけを外すと従来の余地不足の字面で rc 1・run dir を作らない。
- (2) verify 行の `--` の後ろの `--exact` を完全一致の filter として読む（行 as）
  - 前提（.550 の便 113148Z の問い・verified）: verify 行は `sh -c` で撃たれるので、宣言の形の門（`crates/scribe2/src/pipe/declaration.rs` の `METACHARS`・ADR-0010 §2.3）が `(` `)` と引用符を断る。nextest の式（`-E`）は括弧と引用符を要るので verify 行には**書けない**——式を読む形は採らない。nextest には括弧の要らない完全一致の口が別に在る: `--` の後ろに `--exact` を置くと、`--` の後ろの裸の語を **test の名の全体（module path 込み・`pipe::closure::derive::tests::<fn>` の形）** と完全一致で読む（実測 2026-09-22・nextest 0.9.143: 名の全体は 1 本に解け、fn 名だけでは 0 本）。
  - 何が起きているか（現物・`nextest_filter`）: `--` と `--exact` は `-` で始まる読めない語として読み飛ばされ、名の全体は末尾の裸の語として filter 語になるが、置き場の照合は fn 名が filter 語を**含む**かなので 0 本に倒れ、`tests` 欄が無ければ `TeethPlaceUnresolved` で断られる（静かに通りはしない・fail-closed は既に在る）。つまり狭く書く道具は在るが受付が読めない。
  - 形: 1. `crates/scribe2/src/pipe/closure.rs` に `--` の後ろの libtest の旗の閉じた 2 列を足す——引数を取る旗（`--skip`）と引数を取らない旗（`--exact` / `--include-ignored` / `--nocapture` / `--no-capture`）。2. `nextest_filter` が `--` を境に読みを切り替える: `--` の前は従来どおり、`--` の後ろは 1 の 2 列で旗を読み飛ばし（`--skip` は次の 1 語も消費）、裸の語を filter 語にする。`--exact` が在れば filter 語の一致の型を**完全一致**、無ければ従来の**部分一致**にする。3. 完全一致の filter 語は `::` で割った**末尾の段**を fn 名と等値で照合する（module path の段は照合しない＝同じ fn 名を持つ別 file も置き場に入る側へ倒す・閉包は広い側で fail-closed）。`::` を持たない語に `--exact` が付いた周は末尾の段 = 語そのもの。4. 1 行に filter 語は 1 つ（`--` の前後で 2 つ在る周は後ろが正本・従来の「末尾の裸の語」の規則と同じ）。5. 照合の述語に一致の型を渡す。`teeth_words` は末尾の段を出す（検出線の語の導出は 1 本のまま・§34 約束 5）。6. `--` の後ろに `--exact` も裸の語も無い行は従来どおり filter 語なし（`None`）。
  - 読みの細部（実装・s2-07l.550）: `--` の後ろに裸の語が無く前に在る行は前の語が filter 語のまま（`--` の前だけの行の読みを変えない＝4 の「後ろが正本」は両方に在る周だけ）。`--exact` は `--` の後ろのどこに在っても一致の型を完全一致にする。`--skip` が行末の行は引数を取る読めない旗と同じ極性で `None`。
  - 触らない: `--` の前の読み（`-p` / `--lib` / `--test` / 引数を取る読めない旗の倒し方）・`METACHARS` と宣言の形の門・`tests` 欄の置き場の弁別・断りの型と字面と rc・Promised 形の導出・gate が行を撃つ形（`sh -c`）。
  - 母集団（main 3ddfd94・verified）: tracked な設計 doc の verify 行 208 本のうち `--` を持つ行は 0 本・式の旗を持つ行も 0 本。既存の行の判定は 1 本も変わらず、`contract_closure_ext_real_table_has_zero_findings` は緑のままで、write-set は本行の 4 file で足りる。
  - 却下: nextest の式（`-E`）を読む（本 § の前の版）——宣言の形の門が括弧と引用符を断り、門を緩めると `sh -c` で撃つ行に包みと連結の口を開く（ADR-0010 §2.3・C6）。／宣言の形の門に `-E` の次の 1 語だけの逃がしを作る（引用符なしの括弧は `sh` が subshell に読む＝撃てない行を受け付ける）。／module path の段まで照合する（file の path から module path を組む 2 本目の読み手が要る・閉包は広い側に倒す方が安全）。／同じ門を契約表の検査の段でも回す——現物で測ると、Declared 行（write-set と verify を持つ行・192 行）のうち**少なくとも 5 行**（`docs/design/gate-cost.md` の g・`docs/design/pipeline.md` の a と j・`docs/design/rules-manifest.md` の h・`docs/design/seat-roles.md` の m）が、いま main で歯の file を write-set の外に持つ（2 通りの走査で共に当たった行・粗い走査では 18 行・memo `s2-07l.552`）。そのまま findings にすると現物の契約表が 0 件でなくなり既存の歯が赤になる。／`tests` 欄を Declared 行にも開く（§42 で却下した形）。
  - 歯（接頭辞 `contract_teeth_exact_`・`crates/` 全体で 0 件）: in-file（`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・pure な 1 本を直に呼ぶ）で、`-- --exact <名の全体>` の行が末尾の段と等値の fn を持つ file だけを置き場に取り、同じ段を substring に持つ別 fn の file を取らないこと（母集団 = fixture の 3 file と当たる 1 file を同じ assert で数える）・`--` の後ろの裸の語に `--exact` が無い周は従来の部分一致になること・`--skip` の次の 1 語が filter 語に数えられないこと・`--` の後ろに語が無い行が `None` になること・検出線の語が末尾の段になること。e2e（`crates/scribe2/tests/e2e/pipe/contracts.rs`）で、`-- --exact` の行を verify に持つ行が、部分一致なら要求された他の file を write-set に持たないまま受付を通ること（verify 行は括弧も引用符も持たないので宣言の形の門をそのまま通る）。
- (3) 未追跡の設計 doc を知らせる（行 at）
  - 形: 1. `judge_repo` が未追跡の設計 doc（設計 doc の dir 直下の `.md`）を 1 回引く。読む口は既存の git の 1 本を使う（2 本目の読み手を作らない）。2. 判定行に未追跡の本数の欄を 1 つ足す（現物は 0）。判定行の字面を完全一致で読む既存の歯は `crates/scribe2/tests/e2e/pipe/contracts.rs` の中と `crates/scribe2/tests/e2e/pipe/intake.rs` の 1 か所（受付の前に契約表の検査を撃つ歯）で、どちらも新しい欄を含む字面へ更新する（write-set に両 file を持つ・期待の他の部分は変えない）。3. 1 本以上の周は判定行の前に 1 件 1 行で path を名乗る（findings には数えない＝rc も findings の数も不変）。4. git が答えない周はその欄を `?` にする（0 に化けさせない・NFR4）。5. 追随の口（`repo_findings`）の戻りは 1 つも変えない（便の判定材料を変えない）。
  - 触らない: 検査の母集団（tracked な設計 doc だけ）・findings の順と rc・doc 数と行数の数え方・契約表の検査そのもの・受付の経路。
  - 却下: 未追跡 doc を検査の母集団に入れる（tracked でない行から契約を作る口を開く＝FR47 に反する）／findings に数えて rc 1 にする（下書きが 1 本在るだけで CI と受付が落ちる）／作業木の状態を読む別の git の口を足す（tracked の一覧と 2 本目の読み手になる）。
  - 歯（接頭辞 `contracts_untracked_doc_`・`crates/` 全体で 0 件）: e2e（`crates/scribe2/tests/e2e/pipe/contracts.rs`）で、未追跡の `.md` を 1 本置いた木の判定行が未追跡 1 を持ち知らせの 1 行が出て rc 0・findings 0 のまま、その file を追跡すると未追跡 0 になり doc 数が 1 増える（母集団 = 同じ木の 2 回の判定行を対で見る）。in-file（`crates/scribe2/src/pipe/table/check.rs` の歯の区間）で、未追跡の列から知らせの行を組む純関数を 0 本・1 本・2 本で測る。
