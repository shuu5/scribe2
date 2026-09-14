# 設計: 契約の正本と席の作業の境界 — 契約は設計 doc の契約表 1 行、器が契約 file を生成し、審査の段を通してから実装役を起こし、着地の終端（push・CI・台帳の close）まで器が持つ

- 要件: [FR47](../../design-intent/spec/srs.html#FR47) 契約の正本 / [FR53](../../design-intent/spec/srs.html#FR53) 契約 file の生成 / [FR54](../../design-intent/spec/srs.html#FR54) 設計 pointer の拒否 / [FR48](../../design-intent/spec/srs.html#FR48) write-set の閉包 / [FR55](../../design-intent/spec/srs.html#FR55) 契約表の検査 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査の段 / [FR50](../../design-intent/spec/srs.html#FR50) land の終端 / [FR51](../../design-intent/spec/srs.html#FR51) 台帳の lint / [AC21](../../design-intent/spec/srs.html#AC21) [AC22](../../design-intent/spec/srs.html#AC22) [AC23](../../design-intent/spec/srs.html#AC23) [AC24](../../design-intent/spec/srs.html#AC24) [AC26](../../design-intent/spec/srs.html#AC26)・既存 [FR2](../../design-intent/spec/srs.html#FR2) / [FR4](../../design-intent/spec/srs.html#FR4) / [FR9](../../design-intent/spec/srs.html#FR9) / [FR12](../../design-intent/spec/srs.html#FR12) / [FR31](../../design-intent/spec/srs.html#FR31) / [FR32](../../design-intent/spec/srs.html#FR32) / [FR39](../../design-intent/spec/srs.html#FR39) / [NFR1](../../design-intent/spec/srs.html#NFR1) / [NFR4](../../design-intent/spec/srs.html#NFR4)
- 決定: [ADR-0023](../../design-intent/decisions/ADR-0023-contract-source-is-the-design-document-table.html)（本 doc の決定の正本・§2.1〜§2.8）/ [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.3（契約 file = TOML subset・本 doc では生成物）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.2（閉じた enum の充足形・字面走査）/ [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html) §2.1（intake の排他）/ [ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html) §2.6（record の任意 field）
- 土台: [pipeline.md](./pipeline.md) §3（契約 file）/ §5.1（intake）/ §5.4（land）・[pipeline-question.md](./pipeline-question.md)（質問と回答）・[pipeline-conflict.md](./pipeline-conflict.md) §3（起こし直し）・[gate-cost.md](./gate-cost.md) §6（着地の順序）・[rules-manifest.md](./rules-manifest.md)（TOML subset の parser）・[fleet-event-log.md](./fleet-event-log.md)・[seat-roles.md](./seat-roles.md)（権能・本 doc はその外側）
- 位置づけ: 席の産物（契約）と手順（写し・回答の適用・着地の終端・台帳）のうち**器が持つ範囲**を決める。権能（seat-roles.md）は「誰がしてよいか」、本 doc は「器が代わりに持つもの」。裁定 = user 2026-09-13（Q1〜Q3・台帳 s2-07l.197 notes に逐語）。

## 1. 何を解くか

便の一周（要望 → 要件 → 設計 → 契約 → 受付 → 実装 → gate → land → 終端 → 台帳）のうち、器が構造で持つのは受付の排他（FR39）・実装役の write-set guard（FR20）・gate（FR8 / FR9）・land の CAS と順序（FR10〜12・gate-cost.md §6）だけで、残りは席の判断・散文・手写しである（2026-09-13 の実測: 15 手順中 5 つ）。特に契約は **3 段の写し**（設計 doc の契約の割り〔散文〕→ 台帳の acceptance〔散文〕→ 契約 file〔管理席の手写し〕）で、各段で write-set の落ち（5 例）と設計の inline（gate で初めて見つかる穴 2 つ）が起きた。台帳には設計本文を持つ memo が 40 本溜まる（設計 doc に置き場が無い）。

本設計は (1) 契約の正本を設計 doc の**契約表**（機械が読む 1 区間）に置き、器が契約 file を**生成**する (2) 契約表を CI と受付が**検査**する（閉包・実在・形）(3) 実装役を起こす前に**審査の段**を置く (4) land の**終端**（push・CI の照合・台帳の close）を器が持つ (5) 台帳を doctor の項目で lint する。planner の産物に門が付き、管理席の手写しが消える。

## 2. 契約表（FR47）

- **本文の形**: **TOML subset の `[[contract]]` の表**（rules manifest と同じ parser・ADR-0004 §2.3。現物の parser は `[[rule]]` / `[[account]]` の 2 種だけを受け**空の配列を拒む**ので、array-of-tables の種類に `[[contract]]` を 1 つ足し〔C2・variant 1 つ〕、**空の列は key を省いて表す**〔`touches` / `depends` が無い行 = 空・空配列の拒否は緩めない〕）。**置き場は 2 形を同じ読み手で受ける**: (i) 記録時点 = 設計 doc（`docs/design/<題>.md`）の末尾の機械が読む区間 `<!-- contracts:begin -->` … `<!-- contracts:end -->`（CLAUDE.md の憲法区間と同じ marker 形・行走査で区間を抜いて同じ parser に渡す・設計 doc 1 本に区間は 0 か 1 つ）(ii) 後続 = folio2 が設計ノート（YAML 正本）から導出する tracked な `.toml` 1 file（全文を同じ parser に渡す・folio2 planner との擦り合わせ 2026-09-13・scribe2 側は path を差し替えるだけ）。読み手は path の拡張子（`.md` = 区間 / `.toml` = 全文）で形を決め、それ以外は typed に断る。
- **契約 id** = `<doc id>#<row id>`（doc id = file 名の stem・row id = 行の `id`・folio2 の設計ノートと同じ形）。doc id は **append-only**（file を改名しても id は変えない＝改名は新 id + 旧 id の廃止・folio2 へ移すとき設計ノートの meta.id に同じ文字列を写す）。
- **行の field**（**正本は core の型** = `pipe/table.rs` の const・`<NAME> contracts schema` が tracked な生成物 `contracts/schema.toml` へ描き〔hooks.json / 極性一覧と同型・xtask check が render と tracked の差分 0 を測る〕・本節はその pointer・folio2 M1 はその file を「外部 schema 参照型」として読む＝欄の追加は scribe2 の版上げで folio2 の ADR は要らない。現物の契約 file の REQUIRED 9 欄との共通は 5 欄〔req / write-set / verify / size / done〕で 1:1 ではない）: `id`（doc 内で一意・`a` `b` …）/ `title` / `req`（要件 id の列）/ `section`（本 doc の節 anchor・生成時に節の本文を `goal` へ写す＝説明文を二重に書かない）/ `touches`（閉じた型の宣言の列・`crate::fleet::Stage` の形・空可・§3）/ `write-set`（path の列）/ `verify`（positional filter 形の列・`(` を含まない）/ `size` / `done`（1 行）/ `depends`（同 doc の契約 id の列・順序・床が解決と輪の無さを数える）/ `classes`（optional・既存）/ `opens`（optional・[seat-roles.md](./seat-roles.md) 契約 (b) が足す印・(b) の land までは未知 key として断る）。**散文の欄は `title` と `done` の 2 つだけ**（他は id / path / 型名 / 命令の識別子・folio2 の床〔語彙に無い裸の英字語 0〕はこの 2 欄に掛かる・括弧の中は免除。数 + 単位の検査は散文一般には掛けず「規範の印を持つ文」にだけ〔§12〕）。`section` は同じ doc の **節番号（`§N` の N・append-only・見出しの字面ではない）** で、`contracts check` は `## N.` の見出しが在り本文が非空であることを見る。`goal` は optional: (ii) の導出 file は各行に `goal`（節の本文の逐語・正本が YAML へ移ると節の本文の在り処が YAML になる）を運び、(i) の区間では書かず生成時に section の本文を写す。`schema = 1` は両形とも先頭に置く（rules manifest と同じ parser の前提）。**契約表の意味検査（id / req / section / depends / 閉包）は scribe2 の 1 関数（編集時・fail-closed）だけが持つ**。folio2 が持つのは器・導出・導出物の差分 0（post の drift 検出・C16 の代替ではない）。`owner` / `disposition` は生成時に固定値（現物の contract.rs が要求する field を埋める）。
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
- **write-set の項目の実在と展開**: write-set の各項目は base に実在する file か、末尾 `/` の dir（base に実在）か、新規 file（`+` 接頭辞で宣言・base に無いことを検査）のいずれかで、それ以外は `Refuse::WriteSetItemUnresolved { item }`。交差（FR39・ADR-0019）と guard の照合は dir 項目を base の file 一覧に**展開してから**数える（dir で書いた snapshot の置き場が、配下 1 file を持つ別便と偽の交差を起こした 2026-09-14 の型・s2-07l.206 × .243）。
- **上限の余地**: write-set の各 `.rs` file について base の行数と R-C4-2 の値の差（余地）を測り、行の `size` の見積が余地を超える file が在れば `Refuse::CapHeadroom { file, headroom, size }` で断る。core の合計（R-C4-1）も同じ式で 1 回（core の見積 = write-set の `.rs` 本数 × 同じ値）。**S / M / L と行数の対応は rules 行 3 本**（`pipe.size_s_lines` / `pipe.size_m_lines` / `pipe.size_l_lines`・1 file あたりの増分の見積・[rules-manifest.md](./rules-manifest.md) §4・user 裁定 2026-09-14）で、値は rules の読み手から取る（数を code に書かない）。**行の数え方**は xtask check の file-lines / core-lines と同じ式（幅で正規化した行数・R-C4.line-width・rules-manifest.md §4）＝1 行に詰め込んでも余地は増えない。core と xtask は互いに依存しないので式は 2 か所に在り、同じ fixture で両方を突合する歯が一致を守る。出所 = s2-07l.189 run 1（land.rs 1495/1500）・s2-07l.208 run 4（core 上限）・s2-07l.249 run 2（cli.rs / land.rs 1498/1500・満杯の file に M を当てる契約表）。**撃つ場所は受付だけ**（`pipe intake` が起こす 1 行に対して・`contracts check`〔CI〕は撃たない）: 余地は「その便を今の base に当てたら入るか」という**受付時点の事実**で、契約表の行は履歴を持つ（landed 済みの行 a / f の write-set の file はその後も育つ・表は状態を持たない〔C15・状態は台帳〕）ので、表の全行に当てると landed 行が上限で永久に違反になる（s2-07l.249 run 3・check.rs 1499 / fleet/mod.rs 1287）。§3 の他の 3 つ（外形 pin・項目の実在と展開・名指しの実在）は base に対する静的な事実なので受付と CI の両方が撃つ。
- **名指しの実在**: 行の `title` / `done` と設計 doc の当該 `section` の本文で backtick に囲まれた字面のうち、**中身全体が**次の 3 形のどれかに一致するものだけを名指しと読む（一致しない字面〔構造体の literal・field 付きの variant・glob・属性・散文〕は名指しではない）: (1) path 形 = 英数字と「_ . / -」だけで拡張子が .rs（例: pipe/closure.rs）→ base の tracked file の path がその字面と等しいか、その字面を「/」区切りの末尾に持てば解ける（例: declaration.rs は pipe/declaration.rs に解ける）(2) 型の path 形 = 「::」で結んだ識別子の列（例: crate::fleet::Stage・Guard::Intake）→ 末尾の 2 節「型::項目」が base の `.rs` に現れれば解ける。**その行の `touches` に宣言した型の variant は名指しと読まない**（`touches` は「その型に variant を足す契約」の宣言そのものなので、未来の variant〔§4 の審査の段の名・§5 の照合の完了条件の名〕は行 (c) / (d) の `touches` が説明する）(3) fn 形 = 識別子 + 「(」(+「)」)（例: overlaps(）→ base の `.rs` に `fn` 識別子 の宣言が在れば解ける。解けないものを `Refuse::NameUnresolved { name, at }` で全件名指す（字面走査の下界・別名や generic は見ない・新規 file は write-set の `+` 接頭辞で除く）。出所 = s2-07l.243 run 2 の「Guard::Rules」（実在しない variant を設計 doc と契約が名指した）・s2-07l.249 run 1 の QUESTION（本節の例示と未来の variant を検査が拾い、現物の契約表が違反 0 にならなかった＝形の定義を精密にし、例示は backtick に置かない）。
- 上の 4 つは閉包と同じ関数の列（1 実装・受付と CI・C2）で、Guard は増えない（`Guard::Intake` の断りの理由が増えるだけ・極性一覧の行数は不変）。
- **write-set の導出（契約 (h)・user 指摘 2026-09-14「恒久的に write-set が適切に分配される質の高い計画を書けるような修正」）**: write-set を planner が手で列挙する形は、不足（閉包の file が無い＝(g) が断る）と余分（触らない file を挙げて別の便と偽の交差を起こす＝直列化の根）の両方を人の注意に頼る。行は **何を触るか（`touches`）・何で測るか（`verify`）・新設する file（`creates`）・歯の新しい置き場（`tests`）・Rust の外で触る file（`also`）** を宣言し、write-set は器が**導出値**として作る。導出 = 次の和集合（各項は 1 関数・宣言順・`pipe/closure.rs`）: (i) 閉包 = `touches` の型を構造として持つ `.rs`（上の 4 形）(ii) **歯の置き場（base の実測）** = `verify` 行のうち `cargo nextest run` の行から crate（`-p` の値・無ければ core crate）と末尾の filter 語を取り、その crate の `tests/` 配下と `src/` の test 区間で **`#[test]` の直下の `fn` の名が filter 語を含む** file の全部（helper の fn は数えない・nextest の positional filter と同じ「含む」で解く）。新しい接頭辞は base で必ず 0 本なので、そのときは行の `tests` 欄の file〔base に実在するか `creates` に在る〕が置き場（`tests` 欄はこの周に必須）で、`tests` も無ければ `Refuse::TeethPlaceUnresolved { filter }`。(iii) 外形 = `surfaces` の snapshot と pin の file（上の第 5 形）(iv) `creates` = 新規 file の列（項目は `+` を付けずに書く・`+` 付きは `Refuse::WriteSetItemUnresolved` の形で断る・base に無いことを検査＝write-set の `+` 接頭辞と同じ意味を欄で表す）(v) `also` = base に実在する **非 `.rs`** の file（`rules/manifest.toml`・`.github/workflows/ci.yml`・`docs/design/*.md`・`contracts/schema.toml`）。`also` に `.rs` を書いた行は `Refuse::AlsoNamesRust { item }`（Rust の面は `touches` と `tests` から導く＝「どの型を触るか・歯をどこに置くか」を宣言させる側に倒す）。導出値と手書きは型で区別する（閉じた enum `WriteSet` の variant `Derived` / `Declared`・C10 の派生値）。
- **手書きの write-set の扱いと撃つ場所**: 行に `write-set` が在れば導出値と**集合として一致**しなければ断る（`Refuse::WriteSetDrift { missing, extra }`・不足も余分も全部名指す）。`write-set` の無い行は導出値を write-set とし、契約 file（(b) の生成・(b) の前は admin が写す file）と runner の allowlist に書く。**撃つのは受付だけ**（上限の余地と同じ構図: 表は履歴で landed 行は導出値と離れていく＝CI の `contracts check` は従来の「閉包 ⊆ write-set」だけを撃ち drift を撃たない）。**弁別は欄の有無（typed・散文の免除を持たない）**: `creates` / `tests` / `also` のどれも持たず `write-set` を持つ行は `Declared`＝導出も drift も撃たず (g) までの検査だけで通る（(h) の前に書かれた行〔a〜g・他 doc の表〕の通常形・行を新形へ移すのは各行の planner の手番）。新欄を 1 つでも持つ行と、新欄も `write-set` も無い行（`touches` と `verify` だけの行）は `Derived`＝導出の対象で、`write-set` が在れば drift を撃ち、無ければ導出値を write-set にする（4 象限を 2 値で閉じる・C2）。drift の集合比較は `+x`（write-set）と `x`（`creates`）を同じ項目に正規化して行う。(h) 自身は `Declared` の形で intake される。`--contract` の形（契約 file を直接渡す・(b) で廃止）は `touches` を持たないので導出できず、従来の閉包検査だけを撃つ。契約 file に載る `write-set` の新欄（`creates` / `tests` / `also`）は生成した契約 file には写さない（runner が読むのは導出済みの write-set だけ）。
- **限界（残す側）**: (ii) は base の歯の名で置き場を決めるので、名が filter 語を含まない歯（別の接頭辞で書かれた歯）は置き場に入らず、実装役が置いた歯が write-set 外に出れば FR20 の guard が止め質問（FR31）で出る。閉包の下界（別名・generic）は上と同じ。`tests` 欄は歯の file だけを受ける（`tests/` 配下か `src/` の test 区間を持つ file・それ以外は `Refuse::TestsNotATeethFile { item }`）。schema の面: 契約表の field に `creates` / `tests` / `also`（任意・list）を足し `write-set` を必須から任意へ（生成物 `contracts/schema.toml` が写す・(a) の schema 版は変えない＝任意 field の追加）。intake の判定行に `write-set=<derived|declared> files=<N>` を足す（`pipe` の外形 snapshot は intake の判定行を pin していないので動かない・ok 行の等値 assert が動く＝`tests/e2e/pipe.rs` の側で write-set 済み）。

## 4. 契約の審査の段（FR49）

- **段**: `Stage::Reviewed`（宣言順は `Intake` の直後・`as_str` = `Reviewed`）。`pipe intake` の直後に器が lens を 1 回撃ち、verdict を run dir の `review.json`（gate の `verdict.json` と同型・tmp → rename の atomic 書き）と `RunStage stage=Reviewed detail=verdict:<PASS|FAIL|INCONCLUSIVE>` に残す。
- **lens の口**: 既存の `<NAME> lens` に雛形を 1 枚足す（`headless/lens-contract.txt`・穴 = `{contract}`〔生成した契約 file〕/ `{design}`〔`section` の本文〕/ `{requirements}`〔`req` の要件本文・SRS から抜く〕・diff は無い）。観点は 3 つ（契約と設計の節の適合・設計が名指す状態遷移の一周〔段・完了 enum・列の所属〕・write-set の連鎖〔§3 の閉包に無い構造の落ち〕）。verdict は既存の 3 値。予算は NFR1 の cap をそのまま使う（契約 + 節 + 要件で cap を超えたら INCONCLUSIVE＝FR9 の極性）。
- **効き方**: `pipe run` / `pipe resume` は `Reviewed` かつ verdict PASS の run だけを spawn する（現物の `launch(.., &[Stage::Intake])` の入口を `Reviewed` に改める）。FAIL / INCONCLUSIVE は終端（`live` は false・retire 可）。直しは設計 doc の改訂 → PR → 再 intake（run 2）。
- **人の関与 0**: 審査を人が飛ばす口は無い（`--no-review` を作らない・C16）。
- **順序**: 契約 (c) は (b) より先に流す（user 裁定 2026-09-14・台帳 s2-07l.197 notes）。生成 (b) が無い間は、受付が読んだ契約 file（設計 pointer 付き・(a) が検査済み）の `section` の節を設計 doc から読んで審査する＝`{design}` の穴の出所は (b) の前後で変わらない（行の pointer）。理由: 契約の不備が入口で止まらず runner と gate の周を費やした実測（2026-09-13〜14: .208 run 1 / 2・.235・.238・.222 の 5 件が (c) の観点で止まる種類）。

## 5. land の終端（FR50）

Landed（gate-cost.md §6 の CAS の後）に続けて器が行う。各段は typed な event（`RunDone` の `detail` で弁別・schema 1 のまま）。

1. **push**: `git push <remote> main:main`（子 process・remote 名は `.vessel.toml` の宣言 `remote`〔optional・無ければ `origin`〕）。失敗は `RunDone detail=push:failed:<reason>` で止める（close しない・rc 1）。
2. **CI の照合**: 唯一の wait 実装に `Completion::CiResult { repo, sha }` を足し、forge の CLI（`gh run list --commit <sha> --json status,conclusion`・子 process・`.vessel.toml` の `ci-cmd`〔optional・無ければ既定の 1 行〕）を deadline（rules 行 `pipe.ci_wait_s`・Int・裁定 id）まで待つ。結果は 3 値（success / failure / unmeasurable）。**success 以外は close しない**（FailClosed）・記帳して rc 1。
3. **台帳の close**: 台帳 adapter（§6）で `close <bead> --reason "landed <sha> ci=success"`。adapter が撃てない・rc ≠ 0 なら `RunDone detail=close:failed` で止める（着地は成立している＝やり直しは `pipe land --terminal-only <run>` で終端だけ再実行・冪等）。
4. **binary の世代**: record（verdicts.jsonl の行）に `generation=<landed sha>` を足す。自分の版が landed sha より古い周に起動を断るかは後続（§12）。
5. **commit の trailer**: squash commit の本文末尾に `<Name>-Contract: <doc id>#<row id>` と `<Name>-Requirements: <req の列>` の trailer を書く（名は NAME 定数から導出・C2.2・他の道具の trailer と衝突しない）（既存の `run:` trailer と 1 組に統合・**着地の正本は record〔面 5・event log〕で、trailer は器が squash message に同時に書く導出面**・folio2 の RTM は trailer だけを読み、無ければ「まだ分からない」と出す〔「未着地」とは言わない〕・`--pr-cmd` 形は trailer も record も無い恒久の穴として RTM に出る）。

`--pr-cmd` の形（自 repo への PR）は終端を持たない（従来どおり）。

## 6. 台帳 adapter（FR50 / FR51）

- **置き場**: 新 module `ledger/`（core）。読み = `bd --readonly show <id> --json` / `bd --readonly list --status open --limit 0 --json`（子 process・git / tmux と同型・crate 依存なし・出力は既存の `json_lite` で読む）。書き = `bd close <id> --reason <text>` の **1 種だけ**（起票・acceptance・裁定は席）。binary の名は const・path は PATH 解決（env を読まない・C2.2）。
- **lint**（`<NAME> doctor --state-dir S --repo R` の項目 1 行・C3.2）: open の bead を全件読み、(i) 契約（acceptance が `design =` で始まる bead）で pointer が解けない（doc が無い・区間に id が無い）(ii) memo（label `intake:memo`）で本文に機械が読む設計の見出し（固定の 1 つ・`## memo`）が在り `design =` / `research =` の pointer 行が無い、を名指す。件数と母集団を同じ行に出す（`ledger: open=N contracts=K unresolved=U memos=M unpointed=P`）。管理 tick の pointer 注入（FR43）は doctor のこの行を席へ渡す。
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

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "契約表の parser と検査（contracts check）・台帳の pointer 形"
req = ["FR47", "FR48", "FR54", "FR55"]
section = "2"
touches = ["crate::pipe::refuse::Refuse", "crate::polarity::Guard"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/polarity.rs", "contracts/schema.toml", "crates/xtask/src/check.rs", "crates/xtask/src/check_facts.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/", "crates/scribe2/src/snapshots/"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_"]
size = "M"
done = "contracts check が本 doc の区間を全件通し、閉包が足りない fixture を名指す"

[[contract]]
id = "b"
title = "intake の生成（--design）と --contract の廃止"
req = ["FR53", "FR54", "FR48", "FR39"]
section = "2"
touches = ["crate::pipe::refuse::Refuse"]
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/snapshots/"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_intake_design_"]
size = "M"
done = "toy repo の設計 doc から契約 file が生成され、--contract は usage で断られる"
depends = ["a", "c"]

[[contract]]
id = "c"
title = "契約の審査の段（Stage::Reviewed・lens-contract.txt・review.json）"
req = ["FR49", "FR9"]
section = "4"
touches = ["crate::fleet::Stage", "crate::polarity::Guard"]
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/headless/lens.rs", "+crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/snapshots/"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_review_"]
size = "M"
done = "偽 lens FAIL で構築点の呼出 0・PASS で Spawned"
depends = ["a"]

[[contract]]
id = "d"
title = "land の終端（push・CI の照合・台帳の close）と rules 行 pipe.ci_wait_s"
req = ["FR50", "FR12"]
section = "5"
touches = ["crate::fleet::Completion", "crate::rules::RuleKind", "crate::polarity::Guard"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_"]
size = "M"
done = "偽 remote + 偽 CI + 偽 adapter で Landed → close の 3 event・failure は close しない・consumer の要件面の path を宣言で受ける（拡張子で読み手分岐・無ければ既定・既定も無ければ断る）"
depends = ["b"]

[[contract]]
id = "e"
title = "台帳 lint（doctor の項目）"
req = ["FR51"]
section = "6"
write-set = ["+crates/scribe2/src/ledger/mod.rs", "+crates/scribe2/src/ledger/lint.rs", "crates/scribe2/src/main.rs", "+crates/scribe2/tests/e2e/ledger.rs", "crates/scribe2/src/snapshots/"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail ledger_lint_"]
size = "S"
done = "偽 adapter の出力で doctor の行が件数と母集団を出す"
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli.rs", "contracts/schema.toml", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/cli.rs", "contracts/schema.toml", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_derive_"]
size = "M"
done = "write-set 無しの fixture 行が導出値で intake を通り契約 file に導出値が載る・手書きが導出値とずれた fixture 行は missing / extra を名指して断られる・also の .rs / tests の非歯 file / 解けない filter は typed に断られる・schema.toml に creates / tests / also が載り write-set が任意・現物の契約表の intake 済みの行は断られない（受付だけ・CI は従来の閉包）"
depends = ["g"]
<!-- contracts:end -->
