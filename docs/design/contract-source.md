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

[[contract]]
id = "ab"
title = "歯の置き場が verify 行の scope を閉じた 3 値で読む — --test <name> の行は統合 test の file だけ、--lib の行は src の file だけを置き場に数え、読めない旗と複数の旗は従来どおり crate 全体"
req = ["FR48"]
section = "28"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail closure_scope_"]
size = "S"
done = "--test の行が統合 test の file だけを、--lib の行が src の file だけを置き場に返し、旗なしと読めない旗と 2 つ以上の旗の行は crate 全体のまま、0 本の行は従来の字面で断られ、現物の契約表は findings 0"
<!-- contracts:end -->`（CLAUDE.md の憲法区間と同じ marker 形・行走査で区間を抜いて同じ parser に渡す・設計 doc 1 本に区間は 0 か 1 つ）(ii) 後続 = folio2 が設計ノート（YAML 正本）から導出する tracked な `.toml` 1 file（全文を同じ parser に渡す・folio2 planner との擦り合わせ 2026-09-13・scribe2 側は path を差し替えるだけ）。読み手は path の拡張子（`.md` = 区間 / `.toml` = 全文）で形を決め、それ以外は typed に断る。
- **契約 id** = `<doc id>#<row id>`（doc id = file 名の stem・row id = 行の `id`・folio2 の設計ノートと同じ形）。doc id は **append-only**（file を改名しても id は変えない＝改名は新 id + 旧 id の廃止・folio2 へ移すとき設計ノートの meta.id に同じ文字列を写す）。
- **行の field**（**正本は core の型** = `pipe/table.rs` の const・`<NAME> contracts schema` が tracked な生成物 `contracts/schema.toml` へ描き〔hooks.json / 極性一覧と同型・xtask check が render と tracked の差分 0 を測る〕・本節はその pointer・folio2 M1 はその file を「外部 schema 参照型」として読む＝欄の追加は scribe2 の版上げで folio2 の ADR は要らない。現物の契約 file の REQUIRED 9 欄との共通は 5 欄〔req / write-set / verify / size / done〕で 1:1 ではない）: `id`（doc 内で一意・`a` `b` …）/ `title` / `req`（要件 id の列）/ `section`（本 doc の節 anchor・生成時に節の本文を `goal` へ写す＝説明文を二重に書かない）/ `touches`（閉じた型の宣言の列・`crate::fleet::Stage` の形・空可・§3）/ `write-set`（path の列）/ `verify`（positional filter 形の列・`(` を含まない）/ `size` / `done`（1 行）/ `depends`（同 doc の契約 id の列・順序・床が解決と輪の無さを数える）/ `classes`（optional・既存）/ `opens`（optional・[seat-roles.md](./seat-roles.md) 契約 (b) が足す印・(b) の land までは未知 key として断る）。**散文の欄は `title` と `done` の 2 つだけ**（他は id / path / 型名 / 命令の識別子・folio2 の床〔語彙に無い裸の英字語 0〕はこの 2 欄に掛かる・括弧の中は免除。数 + 単位の検査は散文一般には掛けず「規範の印を持つ文」にだけ〔§12〕）。`section` は同じ doc の **節番号（`§N` の N・append-only・見出しの字面ではない）** で、`contracts check` は `## N.` の見出しが在り本文が非空であることを見る。`goal` は optional: (ii) の導出 file は各行に `goal`（節の本文の逐語・正本が YAML へ移ると節の本文の在り処が YAML になる）を運び、(i) の区間では書かず生成時に section の本文を写す。写す形は**単一行**（各行を trim し空行を落とし空白 1 つで繋ぐ・`"` と `\` は escape しない）＝契約 file の読み手は共有の scalar の読み手で、escape を解かず複数行の値も扱わない（`.209` run 10 の実測 2026-09-16）。審査の材料 `{design}` は行の `section` から節の本文をそのまま読むので、契約 file の `goal` の単一行化は審査の材料を薄めない。`schema = 1` は両形とも先頭に置く（rules manifest と同じ parser の前提）。**契約表の意味検査（id / req / section / depends / 閉包）は scribe2 の 1 関数（編集時・fail-closed）だけが持つ**。folio2 が持つのは器・導出・導出物の差分 0（post の drift 検出・C16 の代替ではない）。`owner` / `disposition` は生成時に固定値（現物の contract.rs が要求する field を埋める）。
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
- **write-set の項目の実在と展開**: write-set の各項目は base に実在する file か、末尾 `/` の dir（base に実在）か、新規 file（`+` 接頭辞で宣言・base に無いことを検査）か、**縮む面**（`-` 接頭辞で宣言・base に実在する file・「この便でその file の増分は 0 以下」という見積の符号を項目が運ぶ＝挙動不変の module 分割で歯と本文を外へ出す元の file の形・s2-07l.279 が受付で断られた 2026-09-14 の型）のいずれかで、それ以外は `Refuse::WriteSetItemUnresolved { item }`（`-` の先が base に無い項目も同じ理由）。契約表の検査では、`+` の項目が tracked に在れば land 済みの実在 file として読む（intake は在れば断る・場面は閉じた型 `NewFilePolicy` の値 1 つで渡し、読む関数は 1 本・s2-07l.346）。接頭辞は受付の宣言であって path の一部ではない＝交差の照合（FR39・ADR-0019）・runner の guard へ渡す write-set・便の worktree の allowlist は接頭辞を剥がした素の path で持つ（spawn が剥がす・管理席が契約 file で剥がして写す手順を要らなくする）。交差と guard の照合は dir 項目を base の file 一覧に**展開してから**数える（dir で書いた snapshot の置き場が、配下 1 file を持つ別便と偽の交差を起こした 2026-09-14 の型・s2-07l.206 × .243）。
- **上限の余地**: write-set の `.rs` file のうち **R-C4-2 の測定範囲と同じ `crates/<c>/src/` 配下のもの**（xtask check の file-lines が数える file・`tests/` の歯と `xtask` の外の file は門の対象外ゆえ余地を求めない＝受付が門より広くならない・s2-07l.249 run 7 の受付が 2000 行級の test file で .206 / .245 を断った 2026-09-14 の型）について、base の行数と R-C4-2 の値の差（余地）を測り、行の `size` の見積が余地を超える file が在れば `Refuse::CapHeadroom { file, headroom, size }` で断る。core の合計（R-C4-1）も同じ式で 1 回（core の見積 = write-set の `.rs` 本数 × 同じ値）。**縮む面（`-` 接頭辞）は余地を求めず、core の見積の本数にも数えない**（`size` は「1 file あたりの増分の見積」で、縮む面の増分は 0 以下と宣言されている＝余地 96 の file を 3 module へ割る便が、割る前の file に +S を当てられて断られる形〔s2-07l.279・受付が満杯の file を割る便そのものを断る＝満杯が固定される〕を作らない）。宣言を信じるのは受付の先読みだけで、上限そのものは gate の `cargo xtask check`（R-C4-2 の deny 線・測定値）が守る＝宣言が偽でも上限は超えられない（C10・宣言値は測定を経ずに有効値にならない）。縮む面が本当に縮んだかを gate が測る（head の行数 ≤ base の行数・fail-closed）のは後続（gate.rs の分割 s2-07l.286 の後）。**S / M / L と行数の対応は rules 行 3 本**（`pipe.size_s_lines` / `pipe.size_m_lines` / `pipe.size_l_lines`・1 file あたりの増分の見積・[rules-manifest.md](./rules-manifest.md) §4・user 裁定 2026-09-14）で、値は rules の読み手から取る（数を code に書かない）。**行の数え方**は xtask check の file-lines / core-lines と同じ式（幅で正規化した行数・R-C4.line-width・rules-manifest.md §4）＝1 行に詰め込んでも余地は増えない。core と xtask は互いに依存しないので式は 2 か所に在り、同じ fixture で両方を突合する歯が一致を守る。出所 = s2-07l.189 run 1（land.rs 1495/1500）・s2-07l.208 run 4（core 上限）・s2-07l.249 run 2（cli.rs / land.rs 1498/1500・満杯の file に M を当てる契約表）。**撃つ場所は受付だけ**（`pipe intake` が起こす 1 行に対して・`contracts check`〔CI〕は撃たない）: 余地は「その便を今の base に当てたら入るか」という**受付時点の事実**で、契約表の行は履歴を持つ（landed 済みの行 a / f の write-set の file はその後も育つ・表は状態を持たない〔C15・状態は台帳〕）ので、表の全行に当てると landed 行が上限で永久に違反になる（s2-07l.249 run 3・check.rs 1499 / fleet/mod.rs 1287）。§3 の他の 3 つ（外形 pin・項目の実在と展開・名指しの実在）は base に対する静的な事実なので受付と CI の両方が撃つ。
- **名指しの実在**: 行の `title` / `done` と設計 doc の当該 `section` の本文で backtick に囲まれた字面のうち、**中身全体が**次の 3 形のどれかに一致するものだけを名指しと読む（一致しない字面〔glob・属性・散文〕は名指しではない。構造体の literal・field 付きの variant・引数付きの呼出しは**先頭の token** を 3 形に当てて読む＝§25）: (1) path 形 = 英数字と「_ . / -」だけで拡張子が .rs（例: pipe/closure.rs）→ base の tracked file の path がその字面と等しいか、その字面を「/」区切りの末尾に持てば解ける（例: declaration.rs は pipe/declaration.rs に解ける）(2) 型の path 形 = 「::」で結んだ識別子の列（例: crate::fleet::Stage・Guard::Intake）→ 末尾の 2 節「型::項目」が base の `.rs` に現れれば解ける（§26 が impl の block 経由の第 2 経路を足す＝method / 関連 fn は字面が無くても解ける）。**その行の `touches` に宣言した型の variant は名指しと読まない**（`touches` は「その型に variant を足す契約」の宣言そのものなので、未来の variant〔§4 の審査の段の名・§5 の照合の完了条件の名〕は行 (c) / (d) の `touches` が説明する）(3) fn 形 = 識別子 + 「(」(+「)」)（例: overlaps(）→ base の `.rs` に `fn` 識別子 の宣言が在れば解ける。解けないものを `Refuse::NameUnresolved { name, at }` で全件名指す（字面走査の下界・別名や generic は見ない・新規 file は write-set の `+` 接頭辞で除く）。出所 = s2-07l.243 run 2 の「Guard::Rules」（実在しない variant を設計 doc と契約が名指した）・s2-07l.249 run 1 の QUESTION（本節の例示と未来の variant を検査が拾い、現物の契約表が違反 0 にならなかった＝形の定義を精密にし、例示は backtick に置かない）。
- 上の 4 つは閉包と同じ関数の列（1 実装・受付と CI・C2）で、Guard は増えない（`Guard::Intake` の断りの理由が増えるだけ・極性一覧の行数は不変）。
- **write-set の導出（契約 (h)・user 指摘 2026-09-14「恒久的に write-set が適切に分配される質の高い計画を書けるような修正」）**: write-set を planner が手で列挙する形は、不足（閉包の file が無い＝(g) が断る）と余分（触らない file を挙げて別の便と偽の交差を起こす＝直列化の根）の両方を人の注意に頼る。行は **何を触るか（`touches`）・何で測るか（`verify`）・新設する file（`creates`）・歯の新しい置き場（`tests`）・Rust の外で触る file（`also`）** を宣言し、write-set は器が**導出値**として作る。導出 = 次の和集合（各項は 1 関数・宣言順・**置き場は closure の子 module derive**〔file は契約表の行 m の新規項目・`weighted_lines` / `Fields` / `Base` / `derive_write_set` / `check_drift` / `teeth_places` と in-file の歯・純移動 `s2-07l.363`・呼び手は `pipe/closure.rs` の `pub use` で不変〕。型の閉包の字面走査〔上の 4 形・`sees`〕は `pipe/closure.rs` のまま）: (i) 閉包 = `touches` の型を構造として持つ `.rs`（上の 4 形）(ii) **歯の置き場（base の実測）** = `verify` 行のうち `cargo nextest run` の行から crate（`-p` の値・無ければ core crate）と末尾の filter 語を取り、その crate の `tests/` 配下と `src/` の test 区間で **`#[test]` の直下の `fn` の名が filter 語を含む** file の全部（helper の fn は数えない・nextest の positional filter と同じ「含む」で解く）。新しい接頭辞は base で必ず 0 本なので、そのときは行の `tests` 欄の file〔base に実在するか `creates` に在る〕が置き場（`tests` 欄はこの周に必須）で、`tests` も無ければ `Refuse::TeethPlaceUnresolved { filter }`。(iii) 外形 = `surfaces` の snapshot と pin の file（上の第 5 形）(iv) `creates` = 新規 file の列（項目は `+` を付けずに書く・`+` 付きは `Refuse::WriteSetItemUnresolved` の形で断る・base に無いことを検査＝write-set の `+` 接頭辞と同じ意味を欄で表す）(v) `also` = base に実在する **非 `.rs`** の file（`rules/manifest.toml`・`.github/workflows/ci.yml`・`docs/design/*.md`・`contracts/schema.toml`）。`also` に `.rs` を書いた行は `Refuse::AlsoNamesRust { item }`（Rust の面は `touches` と `tests` から導く＝「どの型を触るか・歯をどこに置くか」を宣言させる側に倒す）。導出値と手書きは型で区別する（閉じた enum `WriteSet` の variant `Derived` / `Declared`・C10 の派生値）。
- **手書きの write-set の扱いと撃つ場所**: 行に `write-set` が在れば導出値と**集合として一致**しなければ断る（`Refuse::WriteSetDrift { missing, extra }`・不足も余分も全部名指す）。`write-set` の無い行は導出値を write-set とし、契約 file（(b) の生成・(b) の前は admin が写す file）と runner の allowlist に書く。**撃つのは受付だけ**（上限の余地と同じ構図: 表は履歴で landed 行は導出値と離れていく＝CI の `contracts check` は従来の「閉包 ⊆ write-set」だけを撃ち drift を撃たない）。**弁別は欄の有無（typed・散文の免除を持たない）**: `creates` / `tests` / `also` のどれも持たず `write-set` を持つ行は `Declared`＝導出も drift も撃たず (g) までの検査だけで通る（(h) の前に書かれた行〔a〜g・他 doc の表〕の通常形・行を新形へ移すのは各行の planner の手番）。新欄を 1 つでも持つ行と、新欄も `write-set` も無い行（`touches` と `verify` だけの行）は `Derived`＝導出の対象で、`write-set` が在れば drift を撃ち、無ければ導出値を write-set にする（4 象限を 2 値で閉じる・C2）。drift の集合比較は `+x`（write-set）と `x`（`creates`）を同じ項目に正規化して行う。(h) 自身は `Declared` の形で intake される。`--contract` の形（契約 file を直接渡す・(b) で廃止）は `touches` を持たないので導出できず、従来の閉包検査だけを撃つ。契約 file に載る `write-set` の新欄（`creates` / `tests` / `also`）は生成した契約 file には写さない（runner が読むのは導出済みの write-set だけ）。
- **限界（残す側）**: (ii) は base の歯の名で置き場を決めるので、名が filter 語を含まない歯（別の接頭辞で書かれた歯）は置き場に入らず、実装役が置いた歯が write-set 外に出れば FR20 の guard が止め質問（FR31）で出る。閉包の下界（別名・generic）は上と同じ。`tests` 欄は歯の file だけを受ける（`tests/` 配下か `src/` の test 区間を持つ file・それ以外は `Refuse::TestsNotATeethFile { item }`）。**write-set の `+` の 2 つの場面**（`s2-07l.346`）: `+path` は「新規 file の宣言」で、intake は base に無いことを要求し（`MustBeAbsent`）、契約表の検査（CI・land 後の main）は tracked に在れば実在の file と読む（`MayBeLanded`）——場面は閉じた型の値 1 つで渡し、読む関数は 1 本。**閉包の同名衝突**（`s2-07l.347`）: 4 形（literal 構築・match の arm・件数 pin・const slice）はいずれも「その file から `touches` の型が見えているか」を 1 関数 `sees(path, text, touched, names)`（`closure.rs`・pure・path を受けるのは (a) の判定に要るから）で判定してから数え、別 module の同名の型（hook の vessel と seat の rebrief がそれぞれ持つ `Marker`）を拾わない。見えている = 次の 3 条件のいずれか: (a) **宣言** = その file が型を宣言し（`enum <Name>` / `struct <Name>` の宣言行）、path が `touches` の module に当たる（`src/` からの相対で `<module>.rs` か `<module>/` の直下・`module` は `touches` の型名の直前の 1 段＝多段 module は最後の段で弁別し親 dir は見ない〔`crate::seat::rebrief::Marker` → `src/seat/rebrief.rs`〕・`crate::Type` は `lib.rs` / `main.rs`・`crates/<c>/` の接頭辞は任意）(b) **import** = `use <module>::Name` で同名に取り込む（`as` の別名は下界の外）(c) **修飾** = 本文に `<module>::Name` の path 修飾が在る。(b)(c) の `<module>` の段は、その file が `<module>/` の直下の子 file（`mod.rs` を除く）なら `super` も同じ module と読む（`src/fleet/cli.rs` の `use super::Stage` は `crate::fleet::Stage`・re-export 先の子 file〔`src/fleet/wait.rs` の `Completion`〕は (a) で自 module）。const slice の名（`crate::paint::HUES.len()` の件数 pin）は (b)(c) で型名と同じに読む＝型を名指さずに型の構造を持つ file を落とさない。const slice の宣言 file も `sees` を通す（別 module の同名 const slice を拾わない）。件数 pin の 1 出現の解決（修飾か取り込み済みか）も同じ (b)(c) の照合で、2 実装にしない。glob（`use m::*` / `use super::*`）越しの取り込みは下界の外（別名・generic と同じ・見落とした file は FR20 の guard が止め質問で出る）。schema の面: 契約表の field に `creates` / `tests` / `also`（任意・list）を足し `write-set` を必須から任意へ（生成物 `contracts/schema.toml` が写す・(a) の schema 版は変えない＝任意 field の追加）。intake の判定行に `write-set=<derived|declared> files=<N>` を足す（`pipe` の外形 snapshot は intake の判定行を pin していないので動かない・ok 行の等値 assert が動く＝`tests/e2e/pipe.rs` の側で write-set 済み）。

## 4. 契約の審査の段（FR49）

- **段**: `Stage::Reviewed`（宣言順は `Intake` の直後・`as_str` = `Reviewed`）。`pipe intake` の直後に器が lens を 1 回撃ち、verdict を run dir の `review.json`（gate の `verdict.json` と同型・tmp → rename の atomic 書き）と `RunStage stage=Reviewed detail=verdict:<PASS|FAIL|INCONCLUSIVE>` に残す。
- **lens の口**: 既存の `<NAME> lens` に雛形を 1 枚足す（`headless/lens-contract.txt`・穴 = `{contract}`〔生成した契約 file〕/ `{design}`〔`section` の本文〕/ `{requirements}`〔`req` の要件本文・SRS から抜く〕・diff は無い）。観点は 3 つ（契約と設計の節の適合・設計が名指す状態遷移の一周〔段・完了 enum・列の所属〕・write-set の連鎖〔§3 の閉包に無い構造の落ち〕）。verdict は既存の 3 値。予算は NFR1 の cap をそのまま使う（契約 + 節 + 要件で cap を超えたら INCONCLUSIVE＝FR9 の極性）。要件本文の読み手は要件面の形ごとに 1 関数（html の anchor / yaml の `id` + `text` / md の見出し・`s2-07l.354`・契約表の行 k・id の集合の読み手 `requirement_ids` と同じ拡張子の 1 match）で、読めない形・本文の無い id は理由の 1 行を材料に載せる（黙って空にしない・NFR4）。
- **効き方**: `pipe run` / `pipe resume` は `Reviewed` かつ verdict PASS の run だけを spawn する（現物の `launch(.., &[Stage::Intake])` の入口を `Reviewed` に改める）。FAIL / INCONCLUSIVE は終端（`live` は false・retire 可）。直しは設計 doc の改訂 → PR → 再 intake（run 2）。
- **人の関与 0**: 審査を人が飛ばす口は無い（`--no-review` を作らない・C16）。歯の側も同じ: `tests/e2e/pipe.rs` の helper（`intake` → `spawn_with` / `implemented` / `gated_pass` / `questioned`・pipe/gate.rs・land.rs・lifecycle.rs の約 100 か所が使う）は lens 無しで intake → spawn を通しているので、helper の中で**偽 PASS の lens**（gate の `fake_lens` と同じ作り）による審査を 1 回通す形に改める（呼び出し側は不変・審査の段を飛ばす flag を歯にも作らない）。intake 直後の event 数を pin する歯（`event_count == 1`）は審査の段の event を数に入れる。行 (c) の write-set はこの 5 file を含む（.241 run 1 の QUESTION 2026-09-14・planner 裁定 (a)）。
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
- **lint**（`<NAME> doctor --state-dir S --repo R` の項目 1 行・C3.2）: open の bead を全件読み、(i) 契約（acceptance が `design =` で始まる bead）で pointer が解けない（doc が無い・区間に id が無い）(ii) memo（label `intake:memo`）で本文に機械が読む設計の見出し（固定の 1 つ・`## memo`）が在り `design =` / `research =` の pointer 行が無い (iii) 契約で acceptance が pointer の 1 行を超える本文を持つ（§2「台帳の bead」・生成 (b) の Landed 後は本文を機械が読まず、lens と実装役が読む契約は行と節だけ＝本文は写しの矛盾の置き場になる・.209 が審査で 7 周止まった型）、を名指す。件数と母集団を同じ行に出す（`ledger: open=N contracts=K unresolved=U bodied=B memos=M unpointed=P`）。管理 tick の pointer 注入（FR43）は doctor のこの行を席へ渡す。
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

- 何が起きているか: planner 実測 2026-09-15 10:4xZ で、契約 (a) の直命の表を Derived で書けなかった。write-set の導出（`pipe/closure.rs`）は Rust の面を `touches` の型の閉包と `tests` からしか導かず、subcommand を足す便が触る 2 面（`seat/cli.rs` の文字列 match `Some("rebrief") =>`・`seat/mod.rs` の `pub mod <新 module>;`）が写らない。`also` は非 `.rs` 限定・`creates` は新規のみ・`surfaces` は歯の区間だけ＝「口を 1 つ足す」契約は Declared に戻る（`.303` が 4 回 QUESTION した型）。現物（verified）: `seat/cli.rs` の分岐は `Some("…") =>` が 10 本・`seat/mod.rs` の mod 宣言 14 本・`pipe/cli.rs` / `seat/cli.rs` に subcommand の enum は無い。
- 形: (vi) `creates` の各 `.rs` について、その親 module の file（`<dir>/mod.rs` か `<dir>.rs`・base の tracked に在る方）を導出値に足す（置き場は §3 の導出の側＝closure の子 module derive・新規 file の検査 `created` の隣に 1 関数）。(vii) `seat/cli.rs` と `pipe/cli.rs` の文字列 match を閉じた enum（`SeatCommand` / `PipeCommand`・`as_str` / `parse`・宣言順・const slice `SEAT_COMMANDS` / `PIPE_COMMANDS` の件数 pin）にし、以後「口を足す」契約は `touches = ["crate::seat::cli::SeatCommand"]` で cli.rs が閉包（match の arm）に入る。const slice の名は 2 つで**別**にする（`ALL` のような同名にしない）: 2 つの cli module は最後の段が同じ `cli` なので、`sees` の (b)(c) は module `cli` の `ALL` を seat / pipe のどちらの型の件数 pin とも読み、両者の閉包が互いの歯の file を拾う（多段 module を最後の段で弁別する下界の限界・§3）＝名を分けて 2 つの閉包を素にする。件数 pin の歯は `tests/e2e/seat.rs` / `tests/e2e/pipe.rs` に置き、`vessel::seat::cli::SEAT_COMMANDS.len()` / `vessel::pipe::cli::PIPE_COMMANDS.len()` の修飾形で書く＝§3 (iii) の件数 pin（const slice の名を (c) 修飾で解く・module は型名の直前の 1 段 = `cli`）でその歯の file だけが自分の型の閉包に入る。親 module（`seat/mod.rs` / `pipe/mod.rs`）は `pub mod cli;` を既に持つ（現物）ので触らない。base には型も const slice も無いので、その歯を base に当てた周は e2e binary ごと compile error＝flip-check はこれを RED と数える（overlay 後の compile error は RED の規則・`crates/xtask/src/flipcheck.rs` の module doc）。
- 触らない: 各 subcommand の実装関数・usage の字面（`ALL` から組んで同じ字面になることを外形 snapshot で pin）。
- 却下案: 導出に「usage 行を持つ .rs」の形を足す（字面の形が増える・閉じた enum で既存の第 2 形に乗せる方が C2）／Declared のまま（`.303` の型の QUESTION が再発する）。

## 18. write-set の導出に fn 形の touches を足す（契約表の行 r・`s2-07l.358`）

- 何が起きているか: planner の `.323` の契約化（2026-09-15 16:5xZ・実測）で、Derived 形の write-set が subcommand の入口 file（`pipe/cli.rs` の `resume` の match・`ratelimit.rs` の段の分岐）に届かない。導出（`pipe/closure.rs` の `closure`）は `touches` を型の path（`crate::module::Type`・末尾が大文字始まり・`touched`）としか読まず、分岐を 1 本足す契約が触る入口の file を型で表せない（届く型は `Stage` / `Outcome` / `Verdict` で全木に広がる）＝Declared に戻るか全木へ広がるかの二択。現物（verified・main 797389f）: 名指しの検査（`unresolved_names`）は fn 形（`Form::Fn`・`declares_fn`）を既に読むが、導出の `touched` は大文字始まりの名だけを受け、fn 形は `ClosureError::TypeForm` で断る。§17（行 q）の (vi)(vii) は creates の親 mod と subcommand の閉じた enum を足す形で、fn の名指しは持たない。
- 形: 導出の第 8 項として **fn 形の touches**（`crate::<module>::<snake_case の識別子>`・末尾が小文字始まり）を足す。閉包 = その module の段（`scopes` / `in_module`・型形と同じ 1 関数を通す）で `fn <識別子>(` を宣言する file（`declares_fn`・下界のまま・呼び手は数えない）。宣言する file が 0 の周は typed に断る（`ClosureError` の variant 1 つ・空集合に潰さない・C10）。型形の 4 形・§16 の第 5 形・§17 の (vi)(vii) は不変。
- 断りの名と字面（run 3 = 審査 FAIL 2026-09-16「新 variant を既存 `TypeForm` の流用と区別できない・型形の退行 pin が空虚」の解として本文に置く）: 新 variant は **`ClosureError::FnUndeclared { module, name }`**、`reason()` の字面は **「touches の <module>::<name> を宣言する file が base に無い」**（既存 `TypeForm` の「crate::module::Type の形でない」とは別の字面）。歯は断りの字面でこの variant を名指して弁別する。型形の退行 pin の fixture は **型を持つ toy**（例: paint の module file に `pub enum Hue`・別の file に `Hue::Red =>` の arm・toy の path は歯の中で組む＝base の file ではない）で、fn 形の行と型形の行を同じ toy に置き、型形の導出値が base と同じ集合であることを assert する（fn だけの toy では pin が空虚）。
- 閉包の置き場（run 1 = 審査 INCONCLUSIVE 2026-09-16「touches は `Touched` だけを名指すが §18 は `ClosureError` の variant 追加も要求する」の解）: 行 r の `touches` は `Touched` と **`ClosureError`** の 2 つ。`ClosureError` は `pipe/closure.rs` の外で `closure/derive.rs`（`Err(ClosureError::…)` の構築）・`pipe/table.rs` と `cli/intake.rs`（`ClosureError` の variant を `=>` の左に持つ arm）・`pipe/refuse.rs`（`Refuse` → `ClosureError` の写し・`=>` の右辺で variant を構築）の 4 file が名指す（verified・main 43706fe・母集団 = `crates/scribe2/src` の grep）。**run 2 = 審査 FAIL 2026-09-16「write-set に refuse.rs が無い」の実測**: Derived の閉包（`files_of`）は「見えている」file のうち `<Type> {` の literal 構築・`<Type>::` が `=>` の左の arm・const slice の pin・宣言の 4 形しか数えず、**enum の variant 構築**（`ClosureError::WriteSetDrift { … }` を `=>` の右辺や `Err(…)` の中で作る refuse.rs / derive.rs）に当たらないため 2 file が落ちる（memo `s2-07l.387`・第 6 形の契約化は別便）。したがって行 r は **.387 の Landed まで手書きの `write-set`（6 面）で運ぶ**（`also` に `.rs` は書けない＝`AlsoNamesRust`）。新 variant を受付の断り（`Refuse`）へ写すかは実装役の判断で、写すなら `refuse.rs` は write-set の中。歯の toy repo は `tests/e2e/pipe/intake.rs` の既存の `contract_derive_` の歯と同じく **test の中で組む**（on-disk の fixture は置かない・`src/pipe/cli.rs` に `fn resume(` を書いた 1 file）。
- 触らない: `unresolved_names` の fn 形（名指しの検査は別の面）・`Form` / `Touched` の型名・契約表の schema（`touches` の値の形が 1 つ増えるだけで field は増えない）。
- 却下案: 入口の match を dispatch の閉じた enum に寄せる（§17 の (vii) が同じ向きで担う・分岐の追加が variant の追加になる大きい形）／`also` に `.rs` を許す（Rust の面を手書きに戻す＝Declared の再来）／呼び手まで閉包に入れる（上界に化ける・`Stage::` と同じ全木の広がり）。

## 19. write-set の導出に enum の variant 構築の形を足す（契約表の行 s・`s2-07l.387`）

- 何が起きているか: `.358` run 2（2026-09-16 03:45Z）が審査 FAIL。行 r（touches = `Touched` + `ClosureError`）から受付が焼いた write-set は 4 面で、`ClosureError` を名指す 5 file（母集団 = `crates/scribe2/src` の grep・main 43706fe）のうち `pipe/refuse.rs` と `pipe/closure/derive.rs` が落ちた。現物（verified・admin と planner が別々に読んで一致）: `pipe/closure.rs` の `files_of` は「見えている」file（`sees`）のうち **4 形**＝宣言（`declaring`）・literal 構築 `constructs`（needle = `<Type> {`）・match の arm `matches_arm`（`<Type>::` が `=>` の**左**）・const slice の件数 pin `pins` のどれかを持つ file だけを導出値に入れる。落ちた 2 file は **enum の variant 構築**（`refuse.rs:204` = `Self::WriteSetDrift { .. } => ClosureError::WriteSetDrift { … }.reason()`〔`=>` の右辺〕・`derive.rs:84` = `Err(ClosureError::WriteSetDrift { missing, extra })`〔戻りの中〕）しか持たず、4 形のどれにも当たらない。拾われた `table.rs` / `cli/intake.rs` は `ClosureError` の variant を `=>` の左に持つ arm を持つ。§3「閉包の同名衝突」の 4 形と §16 の第 5 形（外形 pin）はこの形を持たない＝FR48「閉じた型を構造として持つ file を含む」の穴。
- 形: `files_of` の述語に **第 6 形 = variant 構築** を 1 つ足す（C2・述語 1 つ）: 本文に `<Type>::<Variant> {` または `<Type>::<Variant>(` の出現（`<Variant>` = 大文字始まりの識別子・`{` / `(` の前の空白は任意）が在り、その出現が **`=>` の左のパターン側でない**（`matches_arm` が数える面と重ねない＝行を `=>` で割った右側・または `=>` の無い行）file。`sees` の門（型が見えている file だけ）は同じ 1 関数を通す（同名の型の衝突は §3 のまま）。`Self::<Variant> {` は数えない（`Self` は型名でない＝宣言 file は `declaring` が持つ）。
- 触らない: `constructs` / `matches_arm` / `pins` の判定・`sees` の 3 形・`Touched` / `Form` の型名・契約表の schema・fn 形（§18・.358）・外形 pin（§16）。
- 着地済み行への波及（run 1 の QUESTION 2026-09-16 06:44Z）: 第 6 形は**着地済みの Derived 行の閉包も広げる**（現物の契約表の行 a / b / d / g / h が variant 構築だけを持つ file を write-set に持たず、歯 `contract_closure_ext_real_table_has_zero_findings` が write-set-incomplete で赤になる・母集団は歯が数える）。母集団の実測（planner の走査 2026-09-17・第 6 形だけを足した binary を現物の表に撃って照合）: 歯は repo 全体の設計 doc 21 本・契約表 114 行を測り、`touches` を持つ行は 14 行（write-set も持ち閉包の検査が撃たれる行 10・write-set が空の行 4）。第 6 形で導出値が広がる行は 8、**write-set に無い file が出て赤になる行は 5 = 本 doc の行 a / b / d / g / h だけ**（追記は 15 項目 = a 2 / b 3 / d 7 / g 2 / h 1）で、**他の設計 doc 20 本の行は 0**＝波及は本 doc の中で閉じ、行 s の write-set に他の設計 doc は要らない。行 b と行 d は未着地の便の行で、追記はその便の write-set を広げる。第 6 形の字面の雑音（block の brace・`matches!` のパターン・文字列 literal）は `sees` を通った出現 126 件中 8 件で、追記を要求するのは 15 項目中 1 項目（上界側・doc コメントは 0 件）。純移動の行 n / o（§14 / §15）と同じ型＝**同じ PR で本 doc の契約表の該当行の write-set に広がった file を追記する**（行 s の write-set に本 doc を持つ理由）。追記は導出値をそのまま写す（手で選ばない・Declared に戻さない）。別便に送る案は却下（main が赤の窓を作る＝C12.6）。
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
- 形: (1) lens の雛形 `headless/lens-contract.txt` の最終行の JSON に **`kind`**（閉じた語の 1 つ: `teeth-outside-write-set` / `goal-done-contradiction` / `vacuous-assert` / `literal-mismatch` / `section-material-missing` / `other`・FAIL と INCONCLUSIVE の周は必須・PASS の周は無し）と **`at`**（指した場所の列・path か識別子か §・自由文でなく `,` 区切りの語）を足す。(2) `review.rs` に閉じた enum `FindingKind`（上の 6 語・`as_str` / `parse`・宣言順の const slice・網羅 match）を置き、`parse_lens` が `kind` を読む（FAIL / INCONCLUSIVE で `kind` が無い・読めない周は **`unparsed`** の 7 語目に倒し verdict は lens の値のまま＝理由の欠けを INCONCLUSIVE や `other` に化けさせない・C10。lens の判定に届かず器が作る INCONCLUSIVE〔`--lens` 無し・写しを読めない・起動できない・出力を読めない・scope の中で死んだ〕も同じく `unparsed`＝lens の JSON が無い周はすべて 7 語目）。`settle` は `review.json` に `kind` と `at` を任意 field で足し（schema 1 のまま・古い読み手は無視・§5 の足し方）、event の detail を `verdict:<V> kind:<k>`（PASS は従来どおり `verdict:PASS`）にする。(3) `pipe report` の 1 行に **`review_fail=<本数> by_kind=<k1>:<n1>,…`**（母集団 = `pipe report` が読む event 列の `RunStage stage=Reviewed` のうち verdict が PASS でないもの全部＝既存の `runs=` と同じ範囲で日付では絞らない・`kind:` を持たない古い event は `unparsed` に数える・kind 別の内訳を宣言順に全部・0 も出す）を足す。「潰す」= kind ごとに §21 の preflight の門が 1 つ増え、report の内訳でその kind が 0 に落ちたことを機械で見る。同型の回数で run N+1 を止める線（rules 行 `review.same_kind_stop` = **2**・user 裁定 2026-09-16T05:53Z「２論点とも推奨で進めて」）と、焼き直しが前回の指摘（`at`）に対応する差分を持たない周を受付が断る門は、この kind と `at` を入力にする**別の行**（後続・§23 予定・rules 行を足すので変異と生成物の一覧が同じ PR）。
- 触らない: verdict の 3 値と rc・`review.json` の既存 key・lens の起動の形（`{contract}` / `{design}` / `{requirements}`）・審査の観点 3 つ・gate の verdict.json（審査の段だけ）・`report` の既存 token（`runs=` / `landed=` / `human_events=`）。
- 却下案: memory / notes の散文で型を数える（N2・母集団が測れない）／`evidence` の字面を grep して型を推定する（自由文の字面判定・C3.3）／型を rules 行に置く（型は理由の語彙であって閾値でも極性でもない・閉じた enum の領分）／`kind` を PASS にも必須にする（PASS に理由の型は無い・空の値を作らない）。

## 23. 同型の審査 FAIL が N 回目で材料不変の run N+1 を受付が断り、直前の指摘（at）に対応する差分の無い焼き直しを受付が断る（契約表の行 w・`s2-07l.396`）

- 何が起きているか: §22 の後続（user 裁定 2026-09-16T05:53Z・同型の停止の回数 N = 2・逐語は台帳 `s2-07l.395` notes）。§22 が Landed すると `review.json` と event に kind と at が残るが、受付（`cli/intake.rs` の `intake_run`・§21 の judge の側）は同じ bead の前の便の判定を読まない＝同じ kind の FAIL が何周続いても run N+1 は無限に出せ、焼き直しが前回の指摘（at）に触れていなくても通る（本日の実測: `.209` の 10 周のうち 7 周が同型「字面が現物と合わない」）。現物（verified・main a620600）: 受付の断りは write-set の弁別（§3）・上限の余地・live な便との交差（`exclude_overlap`）だけで、便の履歴を読む口は無い。審査の材料は run dir の `review/` に残る（`keep`・契約の写し + `design.txt`〔行の section の本文〕+ `requirements.txt`）。lens の verdict は同じ材料でも揺れる（`.380` で PASS ↔ FAIL）ので、材料不変の再 intake を 1 回も許さない形は採らない＝回数の線は rules 行。
- 形: (1) **rules 行** `review.same_kind_stop`（`RuleKind` の variant `ReviewSameKindStop`・Int・**値 = 2**・裁定 id `user 2026-09-16T05:53Z`・C5・宣言順の末尾・値は manifest だけが持ち本節は写さない）。行の無い manifest は受付を 1 byte も動かさない（rc 2・行を名指す・`pipe.land_wait_s` と同じ極性）。(2) **同型の停止（受付の門・run dir も event も作らない・write-set の弁別の後・余地と交差の前）**: 受付は置き場の replay から同じ bead の便を id の新しい順に並べ、段が Reviewed 以降の便の `review.json` を読み（読めない便は `WriteSetUnreadable` と同じ断り＝`live` と同じ読み手・段が Intake の便は数えない）、先頭の便の kind と同じ kind が verdict PASS で途切れるまで連続する本数を数える（kind が unparsed の便は数えず連鎖も切らない＝lens の欠けを契約の型に化けさせない・C10）。本数が値に達し、かつ先頭の便の材料（`review/` の契約の写しと `design.txt`）が今回の材料（受付が写す形の契約 file〔導出値を置いた後〕と base から読む節の本文〔§4 の `design_text`・同じ 1 本〕）と両方とも同じ字面の周は、`Refuse` の新 variant `SameKindRepeated { kind, runs }`（名 `same-kind-repeated`・rc 1・runs = 数えた便 id の列・新しい順・理由の 1 行に kind と本数と行の値）で断る。契約か節のどちらかが変わっていれば通す（「焼き直しは書き直し」を器の線にする・§7 の形）。(3) **焼き直しの門（受付の門・同じ場所・停止の後）**: 同じ bead の直前の便（新しい順の先頭）の verdict が PASS でない周、その `review.json` の kind と at の各項目に「対応する差分」が在るかを **kind ごとに 1 関数**（`review.rs`・閉じた型 FindingKind〔§22〕の網羅 match）で測る: teeth-outside-write-set → at の各 path が今回の write-set（弁別済み・dir 項目はその配下）に在る／literal-mismatch → at の各識別子が今回の契約 file と節の本文に無い、または base に解ける（`NameUnresolved` の名指しの読み手と同じ 1 本）／section-material-missing → 節の本文が直前の便の `design.txt` と異なる。対応の無い項目が 1 つでも在る周は `Refuse` の新 variant `FindingUnaddressed { kind, at }`（名 `finding-unaddressed`・rc 1・at = 対応の無かった項目だけ・辞書順）で断る。goal-done-contradiction / vacuous-assert / other / unparsed と at の空な周は測れない＝通す（判断を要する型は planner に残す・裁定の (2) の線）。(4) 断りは §21 の preflight にもそのまま出る（judge の側に置く＝run を作らずに撃てる・planner が edit time に測る）。
- 触らない: 審査の段（§4）と lens の起動・§22 の kind / at の書き方と `review.json` の既存 key・verdict の 3 値と rc・回数の値（manifest だけが持つ）・交差と余地の判定・`.394`（§21）の judge / create の割り方（先に Landed なら judge の中に置き、後なら `exclude_overlap` の隣に置いて `.394` が寄せる）・台帳（受付は run を作らないので QUESTION event の宿主が無い＝断りの 1 行と preflight で planner に届く）。
- 歯（`pipe_intake_repeat_` 接頭辞・`tests/e2e/pipe/intake.rs`・偽 lens が最終行の JSON に kind と at を書く §22 の fixture・toy repo の設計 doc と契約 file を歯が書き換えて commit する）: 同じ kind の FAIL 2 便の後、契約 file と節の本文がともに不変の 3 便目は `same-kind-repeated` と 2 便の id を名指す／節の本文か契約 file のどちらかを変えると通る／kind が違う 2 便は通る／unparsed 2 便は通る／PASS を挟むと数え直す／teeth-outside-write-set at=path の便の後、write-set に path の無い契約は `finding-unaddressed` と path・在れば通る／literal-mismatch at=識別子 の便の後、識別子を書いたままで base に無い契約は断られ・消すか base に足すと通る／section-material-missing の便の後、節の本文が不変の契約は断られ・変えると通る／行の無い manifest は rc 2 で行を名指す。rules 行は `rules_review_same_kind_` 接頭辞（`tests/e2e/rules.rs`・値と kind と裁定 id と宣言順の pin・行と variant を対で足させる）。
- 却下案: 回数を散文の作法にする（N2）／at を `evidence` の自由文から grep する（C3.3）／回数に達したら台帳へ QUESTION event を書く（受付は run を作らない・宿主が無い）／全 kind に門を撃つ（測れない型を偽の「対応済み」に倒す・C10）／同型の停止を契約 file の sha の差だけで解く（節の本文を見ない＝acceptance の言い換えだけで通り §7 の線に反する）／材料不変の再 intake を 1 回目から断る（lens の揺れの再測を塞ぐ・回数の線は裁定の値）／停止を段（Stage）の variant にする（受付の断りは段の遷移でない・run が無い）。

## 24. 着地で消える file の宣言 — write-set の項目の `~` 接頭辞（契約表の行 x・`s2-07l.405`）

- 何が起きているか: 別 repo の planner の実測 2026-09-16（要旨: 着地済みの行の write-set に着地で削除された file が残ると、契約表の検査が `write-set-item-unresolved` を出して CI が永久に赤になる・4 path）。現物（verified・main a620600）: write-set の項目は `WriteSetItem` の 4 値（`File` / `Dir` / `New`〔`+`〕/ `Shrink`〔`-`〕）で、「この便で消す file」を表す形が無い。`+` は場面の閉じた型 `NewFilePolicy`（`MustBeAbsent` = 受付 / `MayBeLanded` = 契約表の検査）で 2 場面を分けるが、消える file は逆向き（受付では在り、着地の後は無い）で、`-` は「縮むが残る」を意味する（§3）。行から path を外すと契約表が「何を消したか」の履歴を保てない。
- 形: (1) 接頭辞 `~`（tilde）= **着地で消える file** の宣言。`WriteSetItem` に variant `Delete`（接頭辞を剥がした path・宣言順の末尾）を足す。受付（`MustBeAbsent` の場面）は base に**実在する file** を要し、無ければ従来の `WriteSetItemUnresolved` で断る（消す予定の file が無い＝宣言の誤り）。契約表の検査（`MayBeLanded` の場面）は tracked に**無ければ着地で消えたと読んで通し**、在れば「まだ消していない実在 file」として通す（両場面とも解ける・履歴が残る）。(2) 交差の照合（FR39）・runner の guard・worktree の allowlist・閉包（§3）は `-` と同じく**接頭辞を剥がした素の path**で読む（消す file は触る file・guard は消す操作を許す側）。**純移動（rename）の弁別**: 同じ行に `+新` と `~旧` を両方書いたものが純移動で、2 項目は独立に解ける（`+` は base に無いこと・`~` は base に在ることを受付が検査し、契約表の検査は両方とも着地の後も解ける）。`-旧` は「縮むが残る」なので純移動には使わない（`-` の先が着地で消えると契約表の検査が `write-set-item-unresolved` になる＝§3 の現行のまま）。交差の照合と gate の照合は 2 つの素の path として数える。**消す操作の境界は gate**: pre-tool-use の write-set guard が見る tool は `Edit` / `Write` / `MultiEdit` / `NotebookEdit` だけで `Bash` は通す（`hook/guard.rs` の `GUARDED`）ので、runner の `git rm` は guard の判定を受けない。消す操作の境界は gate の write-set 照合（`gate/verify.rs` の `check_write_set`・`git diff --name-only` は消えた path も列に出す・`listed` は接頭辞を剥がして当てる）で、`~` の項目に無い path を消した便は `outside-scope` で落ちる＝`~` の宣言が無いと削除は gate で止まり、在れば通る（機構の追加は無い・`normalize` の 1 本で効く）。上限の余地は求めず core の見積の本数にも数えない（増分は負・`Shrink` と同じ扱い）。(3) 名指しの実在（§3）は、その行の `~` の項目と等しい path 形の名指し（`title` / `done` / 節の本文）を解けたものと読む（`+` の新規 file と同じ除外・着地の後に本文の名指しが赤になる型を塞ぐ）。(4) Derived 行の欄（`deletes`）は本 § の外（後続・schema の欄の追加は別便）＝当面は Declared 行の `write-set` の接頭辞だけ。接頭辞を剥がす規則は `pipe/refuse.rs` の `normalize` の **1 本**（`+` / `-` を剥がす・drift の集合比較 `check_drift`・交差の照合 `overlaps`・spawn の `write_policy` が同じ 1 本を撃つ）で、`~` もそこで剥がす（剥がす規則を 2 か所に持たない）。接頭辞の const は `NEW_FILE` / `SHRINK_FILE` の隣（同じ file）。
- 閉包の置き場（verified・main d462fce・母集団 = `crates/scribe2/src` の grep）: `WriteSetItem` を構造として持つ file（§3 の 4 形）は `declaration/write_set.rs` **だけ**（`read_item` の構築と `headroom_shortfalls` の網羅 match）。`cli/intake.rs` / `pipe/refuse.rs` / `table/check.rs` の字面は `Refuse` の variant `WriteSetItemUnresolved`（別の型）で、`declaration.rs` は再輸出だけ＝型の閉包に入らない。残る 3 面は **fn 形の touches**（§18）で名指す: `pipe/refuse.rs` の `normalize`（`~` を剥がす・接頭辞の const）／`cli/intake.rs` の `exclude_cap_shortfall`（接頭辞付きで解けない項目を受付で断る分岐が `+` / `-` の列を持つ）／`pipe/closure.rs` の `unresolved_names`（(3) の除外・`+` の新規 file を除く分岐）。`closure/derive.rs`（`check_drift` は `normalize` を呼ぶだけ）・`table/check.rs`（`read_write_set` が `~` を読む・名指しの検査は行の write-set をそのまま渡す）・`pipe/spawn.rs`（`write_policy` は `normalize` を呼ぶだけ）は触らない。`also` に `.rs` は書けない（`AlsoNamesRust`）。
- 触らない: `+` / `-` の意味と検査・`NewFilePolicy` の 2 値（場面の弁別は同じ型の同じ値で足りる）・契約表の schema・runner の消す操作の許し方（guard の面は path の一致だけ）。
- 歯（`contract_closure_ext_delete_` 接頭辞・置き場は `contract_closure_ext_` の歯と同じ file・fixture の契約表）: `~` の項目が base に在る行は受付を通り契約 file の write-set は素の path／無い行は受付で `write-set-item-unresolved`／契約表の検査は tracked に無い `~` の項目を持つ行で findings 0・在る行でも 0／節の本文がその path を backtick で名指しても着地の後に `name-unresolved` にならない／`+` と `-` の既存の歯は緑のまま。
- 却下案: 着地済みの行の実在検査を撃たない（着地済みを CI が知る手段が台帳〔private〕か git log の字面〔散文〕しか無い・現在面と履歴面の弁別が typed にならない）／`-` に「無ければ消えた」を足す（縮む面と消える file は受付の意味が違う・`-` の先が無い項目は宣言の誤りとして断る現行を緩めない）／行から path を外す（履歴が消える・memo の指摘そのもの）。

## 25. 契約表の名指し検査が struct-like variant の literal 形と引数付きの呼出し形を先頭の token で読む（契約表の行 y・`s2-07l.399`）

- 何が起きているか（verified・2026-09-16 06:4xZ・#255）: 名指しの実在（§3・`pipe/closure.rs` の `unresolved_names` → `form_of`）は backtick の中身**全体**を path / 型の path / fn / 散文の 4 形に分ける。未 land の名を 2 つ backtick で書いた節で、素の 型::項目 の字面は型の path 形で `name-unresolved` になったが、型::項目 { 欄: 値 } の形（struct-like variant の literal・本節の例示は backtick を持たない＝走査に掛けない）は { を含むため散文に落ちて通った（findings=1 であって 2 ではない）。関数呼出しに引数が付く形（識別子(引数)）も同じ穴＝名指しなのに散文扱いの偽陰性で、未 land の名が検査を黙って抜ける。
- 形: `form_of` は backtick の中身の**先頭の token**（最初の `{` / `(` / 空白の手前まで・**末尾の `::` は落とさない**＝module path の字面 seat::account:: は従来どおり散文）を取り出して 3 形（path / 型の path / fn）に当て、残りは捨てる: 型::項目 { 欄: 値 } → 型の path 形の 型::項目（先頭の token）／型::項目(引数) → 型の path 形の 型::項目（関連関数の呼出しも同じ 1 規則・fn 形に割らない）／識別子(引数) → fn 形の識別子（**小文字始まりの識別子だけ**・大文字始まりの 識別子(…)〔Some(…) / Err(…) / Gated(FAIL)〕は tuple variant の構築の字面＝散文・§18 の fn 形の弁別と同じ）／先頭の token が Rust の予約語（pub(crate) / pub(super) の pub）なら散文（予約語は閉じた const の列・strict keywords）／識別子 + ( の形（末尾が ( か () ）は従来どおり fn 形／先頭の token がどの形にも合わない周だけ散文。`touches` に宣言した型の variant の除外（§3）は先頭の token に対して従来どおり効く。path 形の判定は先頭の token でなく中身全体のまま（path に空白や括弧は無い・変えない）。判定は 1 関数のまま（受付と CI が同じ関数を撃つ・C2）。
- 走査の母集団と本便後の findings（verified・2026-09-16 22:0xZ・本 doc の branch で全行の `title` / `done` と各行の § の本文を `backticked` と同じ対の取り方で走査・母集団 = 16 doc・backtick 6193 個）: 上の規則で新しく名指しに読まれる backtick は 73 個。直す前の同じ走査では 78 個のうち base（main d462fce）で解けないものが 7 個で、5 個は本 § と同じ PR で字面を直した（§18 / §19 の ClosureError::X の arm の例示・gate-cost §3 の Completion::pid() の呼出し・pipeline §28 の current_dir(…) の呼出し・dispatcher §2 の order(rows) の signature＝どれも未 land か std の名を backtick に持っていた）。残る 2 個は §20 の行 t の新 variant（`ClosureError` / `Refuse` の TeethOutsideWriteSet）で行 t の Landed で解ける＝行 y は行 t の後（`depends`）。予約語・大文字始まり・末尾 :: の 3 つの弁別を落とすと解けない backtick が 35 個増える（pub(crate) / pub(super) ×16・Some / Err / Ok / Gated / RunStage 等の構築 ×17・module path ×2）＝規則の 3 つの絞りは母集団の実測から出た。
- 触らない: 3 形の解き方（tracked の path・型::項目 の出現・fn の宣言）・`Refuse::NameUnresolved` の形と `at` の字面・backtick の対の取り方・散文の欄の語彙検査（§2）。
- 歯（`contract_name_form_` 接頭辞・`pipe/closure.rs` の in-file の歯）: fixture の未 land の型の 型::項目 { 欄: 値 } の字面が型の path 形（先頭の token）に読まれ base に無ければ `name-unresolved`／既存 fn `parse_pointer` に引数を付けた呼出し形が fn 形に読まれ base に在れば解ける／無い識別子の呼出し形は解けない／大文字始まりの 識別子(…) と pub(crate) と末尾 :: の module path は散文のまま／glob の use と属性の字面は従来どおり散文／既存の `contract_closure_ext_` の歯と現物の契約表（findings 0）は緑のまま。
- 却下案: 中身全体を正規表現で 3 形に当てる（形の数が増えるたびに regex が育つ・先頭の token の 1 規則で足りる）／`{` を含む字面を型の path 形として丸ごと解く（field 名まで base に求める・literal の中身は名指しでない）／散文の欄の語彙検査（folio2 の床）に任せる（未 land の名は語彙にも無い＝別の理由で赤になり planner が根を読めない）。

## 26. 名指しの実在の型の path 形を impl の block 経由でも解く — method / 関連 fn の偽陽性を閉じる（契約表の行 z・`s2-07l.432`）

- 何が起きているか: 別 repo の planner の報告 2026-09-17（run 前の contracts check）。§3「名指しの実在」の (2) 型の path 形は、末尾 2 節「型::項目」の**字面**が base の `.rs` に語として現れれば解ける（`unresolved_names(` の中の `holds_word(`・§25 の先頭 token の規則は分類の側で、解決の側は本 § が変える）。項目が method / 関連 fn のときは呼び手が「値.項目(」か impl の中の「Self::項目」で書くので「型::項目」の字面は現物に無く、実在する fn が name-unresolved に倒れる。現物で再現（verified・2026-09-17・fixture repo）: struct と impl の fn を持つ base で「Report::violation」（done と § 本文）と generic impl の「Wide::width」が name-unresolved になり、実在しない「Report::nope」と同じ 1 語で並ぶ＝実在と不在が判定で区別されない。memo の「file の path と読む」は不正確で、path 形 (1) は `:` を含む語を候補にしない。scribe2 側はこの偽陽性を「型::項目 を backtick に書かない」という散文の回避で避けていた（規則が散文に在る形・N2）。
- 母集団（planner の走査 2026-09-17・docs/design の backtick の型の path 形）: 全 doc 151 語（distinct 110）。行の検査対象（title / done + 行が指す § の本文）119 語・touches 除外 3・字面で解けない 0（現物の契約表は違反 0＝歯 contract_closure_ext_real_table_has_zero_findings の緑）。項目が小文字（fn / module 形）の語 43 は全部 `use` か module path の字面で解けている。検査対象外の § に字面で解けない distinct 5 語（Guard::ALL / Marker::ALL / PointerKind::ALL / LaunchError::AccountDirMissing / std::net）が在り、本 § の経路でも解けない（const slice は module 直下・variant は改名済み）＝処置は「母集団外のまま」（その § を指す行が立つ便で直す・本便は触らない）。本 § の解決は現行の上位集合なので現物の findings は 0 のまま変わらない（差が出るのは consumer repo と、散文の回避を外した後の §）。
- 形: (2) の解決を 2 経路の OR にする。(a) 現行＝「型::項目」の字面が語の境界で現れる。(b) impl 経路＝base の `.rs` のうち **`impl` で始まる行に「型」を語として持つ file**（impl 型／impl<'a> 型<'a>／impl Trait for 型 のどれも同じ照合）が、**同じ file** に「fn 項目」の宣言（`declares_fn(`）を持つ。(b) は同じ file に限る（別 file の同名 fn を拾わない・下界のまま）。`form_of(` の 3 形の分類（§25 の先頭 token を含む）・touches の型の除外・path 形 / fn 形の解決・Refuse の variant と字面・at の形は変えない。
- 触らない: `Refuse` の variant NameUnresolved の名と字面・`contracts check` の rc と判定行・§3 の (1) (3)・touches の除外・§24 (3) の `~` の除外・§25 の分類・alias（`use` の `as`）と generic の解決（下界の外のまま）・検査対象外の § の 5 語。
- 歯: in-file（closure.rs の test 区間・接頭辞 closure_names_impl_・`unresolved_names(` を既存の signature で呼ぶ＝base で compile し assert で RED）: impl の 3 形（素の impl・generic impl・trait impl）の file が fn を宣言する対で解け、impl 行の無い file の同名 fn では解けず、fn の無い項目（.243 の Guard::Rules の型）は解けないまま、variant は字面の (a) で解けたまま。e2e（intake.rs・接頭辞 contract_names_impl_）: 上の fixture repo で findings が「Report::nope」の 1 件だけ・rc 1・実在の 2 語は stdout に無い。現物の契約表の歯（違反 0）は変えない。
- 却下案: (i) `::` を含む語を候補から外す（memo 案 1）＝.243 の「Guard::Rules」（実在しない variant）を再び通し、(2) の下界を丸ごと失う。(ii) rustdoc / cargo metadata で型と項目の実在を引く＝契約表の検査に compile と外部 process を持ち込む（CI の歯が cargo を撃つ）。(iii) 「fn 項目」が任意の file に在れば解ける＝別の型の同名 method で偽陰性（impl 行で型に結ぶ (b) の方が狭い）。(iv) 散文の回避規則を続ける＝N2。

## 27. 受付が契約の散文（goal / done）と verify を歯と外形の pin まで現物で突合する（契約表の行 aa・`s2-07l.429`）

- 何が起きているか: planner の実測 2026-09-17（fleet event log・09-16 00:00Z 以降）: 便 94 のうち審査の段の非 PASS 39（41%）・runner の QuestionRaised 10。非 PASS の evidence の型（重複あり・母集団 40）= verify の filter が done の歯に当たらない 13 / write-set の閉包の外の file 18 / 生成物・snapshot・外形 11 / 既存の歯の fixture・逐語 pin の反転 9 / 字面の不一致 19。QuestionRaised 10 のうち 5 は「既存の歯の逐語 pin が write-set の外」（.320 ×2・.288・.416・.423）。現物: 受付（`cli/intake.rs` の `settle_write_set(`）が測るのは型の構造の閉包（§3 の 6 形）と verify の歯の置き場（§3 (ii)・§20）だけで、契約の散文（契約 file の `goal` / `done`）が名指す歯と外形の字面は読まない。名指しの実在（§3）も呼び手は CI の `table/check.rs` だけで受付は撃っていない。lens（散文の読み）が偶然拾うか runner が実装中に気づく＝1 周（数十分）払う。
- 形（2 つ・どちらも `closure/derive.rs` の pure 関数 1 本 `prose_closure`〔入力 = 契約の `goal` と `done` の本文・`verify`・base の test 区間と snapshot〕で、受付の `settle_write_set(` が Declared / Derived の両分岐で 1 回撃つ・字面走査の下界・C2）:
  - (a) **歯の名指しの被覆と置き場**: `goal` / `done` の backtick 字面（`backticked(`）のうち識別子形（小文字始まりの英数字と `_`・末尾の `(` / `()` は任意）で、**base の `#[test]` の fn 名に等しい**もの（`test_fns(` を `test_region` の全 file に当てる・母集団 1487 本）を「既存の歯の名指し」と読む。名指した既存の歯は (a1) いずれかの `verify` の nextest 行の filter 語を含む（nextest の positional filter と同じ「含む」・`nextest_filter(` と同じ読み）でなければ `ClosureError` の variant `TeethUncovered { names }`（名を全部・辞書順）→ `Refuse` の variant `TeethUncovered`（名 `teeth-uncovered`・rc 1・`refuse_of(` に 1 行・宣言順の末尾）。(a2) その歯の file は write-set に要る: Declared 行は `check_teeth_cover(` と同じ 1 関数で `TeethOutsideWriteSet` に倒す（字面不変）・Derived 行は導出値に足す（§3 (ii) と同じ扱い）。base に無い名（新しい歯）は判定しない（下界・`NameUnresolved` の領分とも重ねない＝`(` の無い識別子は Prose 形のまま）。
  - (b) **判定行 token の pin（第 7 形）**: `goal` / `done` の backtick 字面の中の `<key>=<value>` token（key = 小文字始まりの英数字と `_` `-`・直後に `=`）を取り、value が具体（`<` も `|` も含まない）なら token 全体を、placeholder（`<…>` / `a|b`）なら `<key>=` を literal とし、それを **test 区間か外形 snapshot に持つ file** を閉包に足す（Derived = 導出値へ・Declared = write-set に無ければ `ClosureError` の variant `PinsOutsideWriteSet { files }` → `Refuse` の variant `PinsOutsideWriteSet`〔名 `pins-outside-write-set`・rc 1・file を全部辞書順〕）。base に無い literal（新しい語・例 `pointer=`）は 0 file＝判定しない。現物の当たり方: `skipped=` 2 file（.416 が反転した `tests/e2e/pipe/gate.rs` を含む）・`decision=` 9 file（.423 が名指した 4 file を全部含む・広がる 5 file も同じ判定行を pin する歯）。
  - 判定行の token に `prose=<teeth>:<uncovered>:<pins>`（名指した既存の歯の本数・被覆されない本数・pin の file 数）を**既存の token の末尾に**足す（実測 2026-09-17・verified: intake の判定行は run= の id と弁別の 2 token だけで、`pipe` の外形 snapshot は判定行を pin しておらず、`tests/e2e/pipe.rs` は run= の接頭辞で id を取るだけ＝全文では測らない。token を読む歯は `tests/e2e/pipe/intake.rs` の `intake_tokens(` の集合読み〔`contains` で 1 token ずつ〕なので、末尾に足す token は write-set の中で閉じる）。Guard は増えない（`Guard::Intake` の断りの理由が 2 つ増えるだけ・極性一覧は不変）。
- 触らない: §3 の 6 形と `sees(`・§3 (ii) の `teeth_places(`（(a2) は同じ `check_teeth_cover(` を使う）・Declared / Derived の弁別・契約表の schema（欄を足さない＝散文から引く）・`Fields` の形（構築点 4 か所は不変・`prose_closure` は別の引数で受ける）・CI の `contracts check`（散文は契約 file にしか無い＝表の行には撃たない）・lens の観点。
- 限界（残す側）: (a) は base に在る名だけ（新しい歯の名は下界の外・実装役が置いた歯は FR20 の guard と §20 の門が拾う）。(b) は key の字面一致＝同じ key を別の判定行が使う file も入る（広がるだけ・害は交差の直列化）。生成物の連鎖（ci.yml の行 → CLAUDE.md の done 区間・rules 行 → 生成区間・.161 の型）は本 § の外＝後続の行（生成器の入出力を機械で引く表は on-disk の生成物になるので ADR が先・N4 /「ADR を書く条件」3）。
- 却下案: 審査（lens）に任せる（本日 39 便・1 周 = 数十分）／§ の本文まで走査する（§ は「触らない」に名を並べる＝偽陽性が受付の断りになる・散文は契約 file の 2 欄に限る）／write-set 内の全 pub fn の呼び手を tests/ で grep して足す（write-set の src file が持つ pub fn は数十本＝ほぼ全歯の file が入り直列化が跳ねる・.423 の型は fn でなく判定行の字面で反転した）／`surfaces` 欄に判定行を宣言させる（planner の手の宣言に戻る・散文に既に在る字面を 2 度書かせる）。

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
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
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
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "docs/design/contract-source.md"]
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
write-set = ["+crates/scribe2/src/ledger/mod.rs", "+crates/scribe2/src/ledger/lint.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/seat/rebrief.rs", "+crates/scribe2/tests/e2e/ledger.rs", "crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap"]
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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "contracts/schema.toml", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe.rs"]
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
title = "受付の門の判定式に検出線の生存 9 本を潰す歯を足す — 余地の境界・閉包の usages・core の合計の条件を in-file と e2e で赤にする（歯だけ・門は動かさない）"
req = ["FR48", "FR47"]
section = "3"
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_a_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_closure_ext_survivor_b_name_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_closure_ext_survivor_b_match_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_c_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_d_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_begin_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_end_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_inside_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_closure_ext_survivor_e_fence_"]
size = "S"
done = "生存 9 本の判定式それぞれに赤になる歯が在り、変異の A/B で撃墜される（門の判定は不変）"

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
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/pipe.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_derive_creates_parent_", "cargo nextest run -p scribe2 --no-tests=fail contract_derive_subcommand_enum_", "cargo nextest run -p scribe2 --no-tests=fail seat_command_all_", "cargo nextest run -p scribe2 --no-tests=fail pipe_command_all_", "cargo nextest run -p scribe2 --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2 --no-tests=fail pipe_external_form"]
size = "M"
done = "口を足す契約が Derived で書け、導出値に cli.rs と親 mod.rs が入る"

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
write-set = ["crates/scribe2/src/pipe/closure.rs", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail closure_variant_construction_"]
size = "S"
done = "variant 構築だけを持つ file が導出値に入り、=> の左のパターンだけの file・Self:: の構築・doc コメントの名指しは数えず、既存 4 形の導出値は不変"

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
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_review_kind_"]
size = "M"
done = "FAIL / INCONCLUSIVE の review.json と event が kind を持ち、kind の無い lens 出力は unparsed に倒れ、report の 1 行に review_fail= と by_kind= が宣言順に出る"

[[contract]]
id = "w"
title = "同型の審査 FAIL が rules 行 review.same_kind_stop の回数に達した bead の材料不変の run N+1 を same-kind-repeated で断り、直前の at に対応する差分の無い焼き直しを finding-unaddressed で断る"
req = ["FR49"]
section = "23"
touches = ["crate::pipe::refuse::Refuse", "crate::rules::RuleKind"]
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/review.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_intake_repeat_", "cargo nextest run -p scribe2 --no-tests=fail rules_review_same_kind_"]
size = "M"
done = "同じ kind の FAIL が行の値の本数続いた bead の材料不変の intake は same-kind-repeated で断られ、直前の at に対応する差分の無い契約は finding-unaddressed で断られ、材料か対応を変えた契約は通り、行の無い manifest は rc 2、rules 行が裁定 id 付きで 1 本増える"
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
title = "受付が契約の散文（goal / done）の歯の名指しを verify の filter と write-set に突合し、判定行 token の pin（第 7 形）を閉包に足す"
req = ["FR48"]
section = "27"
write-set = ["crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_prose_teeth_", "cargo nextest run -p scribe2 --lib --no-tests=fail prose_closure_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_derive_reasons_are_last_and_name_their_payload"]
size = "M"
done = "goal / done が名指す base の歯が verify の filter に当たらない契約と、その歯の file・判定行 token を pin する file が write-set に無い Declared 契約を受付が名を全部名指して断り、Derived 契約は導出値にその file が入り、新しい名と base に無い token は判定されず既存 6 形の導出値は不変"
<!-- contracts:end -->


## 28. 歯の置き場が verify 行の scope（-p / --test <name> / --lib）を読む — その行が撃てない file を write-set に要求しない（契約表の行 ab・`s2-07l.451`）

- 何が起きているか: 歯の置き場の読み手（§3 (ii) の `teeth_places` と、§20 が同じ 1 関数で通す Declared 行の門 `declared_teeth`）は、verify の nextest 行から crate（`-p` の値）と filter 語だけを取り、`nextest_filter` が scope の旗（`--test <name>` / `--lib`）を落とす。置き場はその crate の **全 file** から「`#[test]` の直下の `fn` の名が filter 語を含む file」を集めるので、`--test e2e` の行（統合 test の target だけを撃つ行）でも `src` の in-file の歯の file を write-set に要求して断る＝**その行が実際には走らせない file** を書く権利ごと要求している。実測（2026-09-17・母集団 = 設計 doc の契約表の nextest 行 220 本）: `s2-07l.447` run 1 が `seat_account_` で src の歯 4 本に当たって断られ（接頭辞を 2 本に割って回避）・`s2-07l.340` が `confine_reasons_` で同型・純移動の `s2-07l.351` は 12 の接頭辞のうち 6 本が src の歯の file を要求し、**歯の名を変えられない純移動では回避できず run が 1 本も起きない**。
- 形: nextest 行の読み手に scope を足す。scope は **閉じた 3 値の enum**（宣言順 = 旗なし / `--lib` / `--test <name>`）で、行の語から 1 関数で解き、置き場の母集団を `in_crate` の後段で 1 述語に畳む: 旗なし = その crate の全 file（従来どおり）／`--lib` = `crates/<crate>/src/` 配下／`--test <name>` = `crates/<crate>/tests/<name>.rs` とその配下。**読めない旗（`--bin` / `--benches` / `-E` ほか）と、scope の旗が 2 つ以上在る行は旗なしと同じ広い側へ倒す**（fail-closed・緩める側は狭く取るの対）。読み手は 1 本のまま（Derived の導出 (ii) と Declared の門 §20 は同じ関数を通る）で、断りの型も字面も増やさない（`TeethPlaceUnresolved` / `TeethOutsideWriteSet` のまま）。
- 触らない: filter 語の読み（`-` で始まらない最後の語）・`test_fns` と `test_region` の弁別・`tests` 欄の扱いと `teeth_file`・Declared / Derived の弁別・`check_teeth_cover` の照合と正規化・契約表の schema（欄を足さない＝行の verify から読む）・nextest 形でない verify 行を読み飛ばす規則・受付の判定行の token。
- 歯（`closure_scope_` 接頭辞・`crates/scribe2/src/pipe/closure/derive.rs` の歯の区間・fixture は同 module の `source(` の型）: (a) `--test e2e` の行が `tests/e2e/` の歯の file だけを置き場に返し、同じ filter 語に当たる src の in-file の歯の file を返さない（base では返す → RED）／(b) `--lib` の行が `src/` の歯の file だけを返す／(c) 旗なしの行・読めない旗を持つ行・旗が 2 つ在る行は crate 全体を返す（広い側のまま）／(d) scope が返す file が 0 本で `tests` 欄も無い行は従来どおり `TeethPlaceUnresolved`（字面不変）。
- 限界（残す側）: scope は**その行が走らせる target**までしか写さず、target の中の module の木は読まない（`--lib` は `src/main.rs` と `src/bin/` の歯も数える＝真の lib target より広い側。現物の契約表 220 行のうちこの差に当たる行は 0 本・実測）。`--test <name>` の `<name>` は `tests/<name>.rs` と `tests/<name>/` の字面で解き、`Cargo.toml` の `[[test]]` の `path` は読まない（本 repo は宣言を持たない）。断りの字面は不変ゆえ、scope の外に歯が在って 0 本になった行の理由は「base に無い」と読める（下界・`tests` 欄で置き場を宣言する側に倒す）。
- 却下案: 純移動の契約に別の verify の形（filter ごとの nextest list の本数が base = head）を持たせる（`s2-07l.351` だけを救い、`s2-07l.447` / `s2-07l.340` の型〔純移動でない便の scope 誤読〕が残る。本数で数える門は歯の本文の改変を通す＝flip-check の `removed_only` が名前の集合を捨てた教訓と同型）／scope を読めない行を断る（今日通っている 183 本の旗なしの行を全部断る）／crate の target を `cargo metadata` で解く（外部の口と実行時の依存を足す・字面走査の下界のままにする）。
