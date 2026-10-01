# 設計: 行の審査 — 契約表の行を merge の前に審査し、落ちた行の順番を起動の列が守る

- 出所: 持ち主の問い 2026-09-30T11:3xZ〜11:5xZ（逐語は台帳 s2-07l.736.31 の notes）→ epic s2-07l.736.33 → [ADR-0103](../../design-intent/decisions/ADR-0103-contract-rows-pass-row-review-before-merge-and-failed-rows-keep-their-place.html)。材料は 2026-09-28〜09-30 の便 165 本の FAIL / INCONCLUSIVE 76 件の分析（host の file・tracked でない）。推奨の採用は常設の裁定 user 2026-09-28T00:54Z（決めてほしいことは推奨で進める）の適用で、rules 行の値 2 つ（§7 の期限・§6 の値 0）と行の審査の消費（§10）は SRS の追加 round で裁定を取った（値は user 2026-09-30T22:13Z 項 reserve・項 precheck、消費は常設の裁定の適用）。
- 要件（SRS 0.33）: [FR100](../../design-intent/spec/srs.html#FR100) 行の審査 / [FR101](../../design-intent/spec/srs.html#FR101) merge の門の行の審査の判定 / [FR102](../../design-intent/spec/srs.html#FR102) 行の予約 / [FR103](../../design-intent/spec/srs.html#FR103) 兄弟の待ち / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR68](../../design-intent/spec/srs.html#FR68) 起動の列 / [FR92](../../design-intent/spec/srs.html#FR92) merge の門 / [FR39](../../design-intent/spec/srs.html#FR39) 交差の排他 / [FR44](../../design-intent/spec/srs.html#FR44) 席の入力欄への差し込み（変えない・§3 形 6）。受け入れ基準は AC73〜AC76 と AC22・AC62。
- 前提: 審査役に読みだけの道具を渡し、作業場所を審査の木（HEAD の commit を detach した一時の worktree）にする設計（同じ epic の別の設計・着地済み・以下「打ち手 0」）。行の審査の lens はその形で撃つ。
- この設計から出る契約: §11 の 10 行（行 a〜g と、行 a から分けた 3 行＝lens の版の flag の行 h・先撃ちの私有の 4 本の純移動の行 a0・行の審査の記録の読み手と鍵の口と祖先の層の行 a1）。契約表は doc の末尾の区間に在る。

## 1. 何を解くか

契約の審査（FR49）は便の中に在る: 設計の PR を merge → 行が起動の列に入る → 起こされた便の Reviewed の段で審査 → runner → gate。設計の欠陥は便の中の審査か gate で初めて見つかる。

- 2026-09-28〜09-30 の FAIL / INCONCLUSIVE 76 件（Reviewed 46・gate 30）のうち 73 件（96%）は設計の側が根だった。runner が主因は 2 件、環境 1 件、審査役の誤りは 0 件。
- 審査で落ちた行は列の外へ出る（同じ契約の中身で終端に着いた便の行は `Settled`）。直しの PR が merge されるまで、その行の write-set は誰のものでもなくなる。同じ file を触る後ろの行が先に起き、直った行はその行の終わりを待つ側に回る（行 a1 が 2 回落ちる間に同じ P2 の行 ak が起き、直った a1 は ak の終わり待ち）。
- 同じ欠陥を持つ兄弟の行を止める口も無い（行 b1 は a1 が同じ指示で落ちた 4 秒後に起こされ、同じ理由で落ちた。直しは 53 分後）。
- 先撃ち（[dispatcher.md](./dispatcher.md) §27 形 aa）は審査の層になっていない。Reviewed の FAIL 46 件のうち事前に撃たれていたのは 9 件で、そのうち 6 件は先撃ちが FAIL / INCONCLUSIVE を出したまま起動した。使い回し（形 ac）は全期間で 0 件。

やさしく言うと: 設計の穴は「列に並んだ後」に見つかるので、落ちた行は列から外れ、後ろの行に file を取られる。審査を「列に並ぶ前」（設計の PR を main に入れる前）へ移し、それでも落ちた行は自分の触る file を取り置いて順番を守り、同じ設計から出た兄弟の行は直しが入るまで待たせる。

## 2. 何が起きているか（main 92a5ddc8 で測り直した・verified・名は行 e の前の現物）

- **審査の時点**: `crates/scribe2/src/pipe/review.rs` の `review`（:307）が便の置き場の材料の dir に `stage`（:348）で材料を組み、`decide`（:545）が lens を撃つ。lens の cwd と `{worktree}` の穴は審査の木で、`decide` が `crates/scribe2/src/pipe/dispatch/floor.rs` の `Worktree`（:223・run dir の下の `<sha>.tree` に detach し、Drop で登録ごと外す）で作る（打ち手 0）。箱の unit 名は便の id から作る（:570）。判定は `read_outcome`（:614）→ `tip`（:624）の後に `narrow`（:415）を通し、3 本とも私有である（先撃ちが使う口は項目 0 の `outcome_of`・:655 だけ）。先撃ちの判定の使い回しは `crates/scribe2/src/pipe/dispatch/prelens.rs` の `reusable`（:351）で、同じ model に解ける周だけ引く（`Review` の組み立ては `crates/scribe2/src/pipe/cli/step.rs` の `review_run`・:237 の 1 か所）。
- **使い回しが当たらない理由**（先撃ちの分析・host の file の §9・verified）: 使い回しの口が在った窓の Reviewed 61 回で使い回しは 0 回。
  - 先撃ちの材料には予想の印の行（「予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた」）が必ず入る（114 本中 114 本・Reviewed は 163 本中 0 本）。
  - 宣言だけの祖先は、着地で変える既存の file の中身を予想に写さず、新しい file を空で置く。祖先が着地すると base の要約と外の材料が変わる（例: write-set の 1 file が 521 行 → 899 行）。
  - lens の cmd の字に binary の置き場の path が入り、置き場は 56 時間で 18 回変わった。
  - 印を外して材料の 5 区画が全部同じだったのは 20 回のうち 1 回。使い回すと、先撃ちの見逃し（同じ版の PASS 11 回中 2 回を本番の審査が FAIL にした）がそのまま素通りする。
- **先撃ちの木**: 一時の worktree の実体化・材料の組み・片付け・材料の鍵は prelens.rs の私有の `build`（:230）・`drop_tree`（:251）・`materialize`（:259・祖先の層を当てる）・`declare`（:283・宣言の祖先の `+` を空で置く）・`requirements_of`（:317）・`digest`（:327）と `prune`（:165）が持つ。事前審査の予想（`crates/scribe2/src/pipe/dispatch/precheck.rs` の `overlay`・:208）は材料の上に重ねるだけで木には当てない。予想の 1 本（`population`・:51 と `resolve`・:146・層の型 `Layer`・:95）は台帳の bead を母集団と鍵に持つ。先撃ちの `round`（prelens.rs の :114）は、上限の値に依らず材料の組み直し（`rebuild`・:199）を毎周撃つ。
- **列の順と終端の行**: `crates/scribe2/src/pipe/dispatch/candidates.rs` の `entry_of`（:43）は依存 → hold → 起こした印 → 設計 pointer → 契約の生成 → `settled`（:240）の順に理由を付ける。`settled` は直前の便が終端に着き契約 file が同じ行を `Settled` にする（Reviewed の終端だけは設計の節の本文の写しも鍵に入る・`section_keyed` :274）。`settle`（:87）は `order`（`crates/scribe2/src/pipe/dispatch.rs` の :880・鍵は私有の `key_of`）の順に候補を見て、`blocker`（:127）が live な便と同じ周に起こした便の write-set とだけ交差を測る。`Settled` の行の write-set はどこにも数えない。
- **待ちの理由**: `WaitReason`（dispatch.rs の :109）は閉じた型で、名の列は `WAIT_REASONS`（:104・10 語・末尾は着地済みの行 aj と am が足した `unreflected-ruling` と `floor`）。床の検査の不合格は launched と settled と first の印の無い候補の理由を上書きし、未反映の裁定はその後に床の待ちでない候補を上書きする（dispatch.rs の :588〜:600）。局面の表（`crates/scribe2/src/case/mod.rs` の `QUEUED_TURNS`・10 語）は `WAIT_REASONS` の全部を覆うことを 2 本の歯（同じ file の歯の区間の (d) と (i)）が測る。dispatch.rs は全体 1477 行（幅で正規化・R-C4-2 の上限 1500 との余地 23）で、variant を足す行はこの余地の中で書く（§11）。
- **merge の門**: `crates/scribe2/src/hook/merge_gate.rs` の `decide`（:245）は本文の file だけを読み、断りの語は宣言順の 5 語（`REASONS` :65）。hook の入口では anchor の門の直後に撃たれる（`crates/scribe2/src/hook/mod.rs` の :700・`decide` には command と cwd だけが渡り、root と state dir は anchor の門にだけ渡る）。器が着地させる便は git を子 process で撃ち、この門を通らない。見分けの `is_pr_merge`（`crates/scribe2/src/hook/anchor_guard.rs` の :92・anchor の門と merge の門の 2 か所が呼ぶ）は `-` で始まる語を読み飛ばすので、`gh pr merge --help` も merge と読んで断る（この host で再現 2026-09-30）。gh 2.45.0 の `pr merge` は `--match-head-commit SHA`（PR の head がその sha でなければ merge しない）を持つ（この host の help で実測）。
- **vessel 宣言の読み**: 任意 key の読み手は全部 git で HEAD か sha の commit の宣言を読み、作業ツリーの宣言は読まない（`crates/scribe2/src/pipe/declaration.rs` の私有の `head_declaration`・:375）。任意 key は `crates/scribe2/src/pipe/declaration/optional_keys.rs` の閉じた列が持ち（`close-check` は真偽だけの key・`floor_check_at`・:109 と `ruling_keys_at`・:162 は名指した commit の宣言を読む）、key を足した便は親の declaration.rs の `Declared` の欄と `parse` と、key の列を pin する既存の歯（declaration.rs の歯の区間の `declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema`）も触った（98285c92）。`git show <sha>:<file>` の読みは commit が local に無い周と file が無い周を分けず、どちらも「宣言が無い」と読む。
- **先撃ちの rules 行**: `pipe.precheck_lens_per_round`（値 1・値 0 は lens を撃たない）と `pipe.precheck_lens_model`（sonnet）。契約の審査と gate の審査の model は `lens.model`（opus）。lens の子は cap・model・effort を自分の `--rules` から読む（`crates/scribe2/src/headless/lens.rs` の `rows_of`・:181）。lens の子の flag は閉じた列 `KNOWN_FLAGS`（:100・8 語）で、列の外は未知の引数で断る。
- **受付札と口座**: 受付札の本文は `schema` / `pid` / `run` / `jobs` / `ts` の 1 行で、読み手は pid と起動時刻で生死を判じ、run の字で判じない（`crates/scribe2/src/pipe/admission.rs`・札を取る口は `admit`）。gate の lens は口座を `Pool::declared`（step.rs の :307）で選び、Reviewed の lens は親の環境を継ぐ。

## 3. 行の審査（設計の PR の head で、変わった行ごとに撃つ）

- 口: `<NAME> pipe review --ref <sha> --repo R --state-dir S --lens CMD [--rules PATH]`。orchestrator の席が設計の PR を push した後に撃つ。merge の門の断り（§4）が同じ argv を名指すので、撃ち忘れは merge の時点で見える。
- 形（行 a の done が各形を歯と対にする）:
  1. **変わった行**: `--ref` の木と、その commit と `origin/main` の先端の merge-base の木の両方で、契約表を持つ file（受付と同じ表の読み手が行を返す file）の行を読み、行の digest（形 2）が merge-base と違う行と merge-base に無い行を「変わった行」とする。台帳を読みだけで読み（`read_ledger`・bead の pointer は台帳の lint と同じ読み手 `pointer_of` で読む）、その行を指す bead が 1 本以上在って全部 closed の行は外す（着地済み）。bead の無い行は撃つ。変わった行が 0 本なら ref の記録（§9）だけを書き、lens は撃たない。台帳を読めない周は撃たずに rc 2 で理由を名指す（着地済みかを測れない行を撃つ側にも外す側にも倒さない・C10）。
  2. **行の digest**: 行の欄から受付と同じ生成（`generated`）で作った契約 file の字と、実装する設計の節の本文（審査の材料と同じ読み手 `design_material` の字）を、この順に NUL で区切って並べた byte の FNV-1a 64 の 16 桁。FR49 が契約の中身の同一性に使う 2 つ（契約 file の字と節の本文）と同じ入力で、便の置き場の写し（契約 file と材料の dir の design.txt から末尾の改行を 1 つ除いた字）からも同じ値が出る。
  3. **祖先の扱い**: 祖先の層は行（`<doc>#<行 id>`）を鍵にした 1 関数で組み、今の事前審査の予想（bead を鍵にした 1 本）はその上に載せ替える（予想の読み手を 2 本にしない・C2・行 a1・下の口 (G)）。祖先は、表の depends を推移でたどった行（同じ doc）と、行を指す bead が在ればその台帳の blocks の祖先。祖先ごとの状態の語は、着地（祖先を指す bead が全部 closed）／実物（祖先の便が Gated で判定 PASS）／宣言（それ以外）。審査の basis は次の 3 つ:
     - actual: 行を指す bead が在り、祖先が全部着地か実物。
     - forecast: 宣言の祖先を 1 つ以上持つ。宣言の祖先の `+` の file は空で置かない（先撃ちの予想の形を採らない）。代わりに材料の dir に祖先の材料の file を 1 つ置き、宣言の祖先ごとに、行の TOML の写し（`find_row` が返す行の字をそのまま）と実装する設計の節の本文（`design_material`）を並べる。lens には「この file は未着地の祖先が作る・変える」と材料の形で渡る。
     - partial: 行を指す bead が無く、同じ doc の祖先は全部着地か実物。doc を跨ぐ順は台帳の blocks だけが持つので、bead の無い行の doc を跨ぐ祖先は測れない。doc を跨ぐ未着地の祖先に由来する FAIL を避けたい行は、行の審査の前に bead と blocks を起票する（口はそれを読んで forecast にする）。
  4. **機械の検査**: 変わった行ごとに受付と同じ `generated` → `judge`（置き場なし・`pipe preflight` と同じ 1 本）を撃つ。宣言の祖先を持つ行は形 3 の祖先の層を材料に重ねた予想の上で撃ち、確定と暫定の弁別（[dispatcher.md](./dispatcher.md) §27 形 x の 3）を借りる。確定の finding を持つ行は lens を撃たずに判定 FAIL とし、記録の mech に `firm:<断りの名>`、kind に `-` を置く（理由の型の閉じた 7 語に受付の断りの名を混ぜない）。暫定の finding は lens の材料の末尾に写す。
  5. **lens**: 機械の検査を通った行ごとに、`--ref` の commit を detach した審査の木を Reviewed と同じ `Worktree`（§2）で置き場の ref の dir の下に作り、実物の祖先の差分を重ね、Reviewed と同じ組み手（`stage`）で材料を組み、契約の審査と同じ lens（`--stage` なし）を読みだけの道具で撃つ。lens の cwd と `{worktree}` の穴はその審査の木である。箱で包み（子は最後まで待つので作り手の死で殺される形にならない）、unit 名は ref の 12 桁と行 id から作る。口座は gate の lens と同じ `Pool::declared`（FR36 の便用の規則）で選ぶ。同時に起こす本数は受付の host の memory の枠で絞り、受付札（`admit`）の run の欄に `row-review-<ref の 12 桁>-<行 id>` の 1 語を書く。祖先の層を木に当てる腕（`materialize` と `declare`）・要件面の path の読み（`requirements_of`）・材料の鍵（`digest`）の先撃ちの私有の 4 本は、pipe の review の下の新しい子 module（行 a0 の write-set の `+` の file）へ中身を変えずに純移動して（下の「行 a0 の純移動の形」）先撃ちと共用する。行の審査は宣言の祖先の層を木に当てない（形 3）。先撃ちの `build` と `drop_tree` は移さない（行 e で消える）。落ちた周の残り（木の dir と登録）は、次に口を撃った周の頭に外す。
     - lens の判定は、Reviewed と同じ読みの 2 本（`read_outcome` → `narrow`・done の項目の数と約束の行の有無を渡す）を review.rs の `pub(in crate::pipe)` の口 1 つで通す（3 本の私有は保つ・読み手を 2 本にしない・C2）。
     - lens の版（形 7・§5）は、撃つ前に同じ lens の cmd の穴を埋めた行の末尾に行 h の版の flag を付けて箱の外で 1 回撃ち、stdout の 1 行目を読む。rc が 0 でない・1 行目が空の周は、その行を撃たずに判定 INCONCLUSIVE・理由の型 unparsed とする（版の分からない判定を鍵に入れない・C10）。
  6. **前面で最後まで撃つ**: 口は全部の行を撃ち終えるまで前面で走り、行ごとの判定と結果の 1 語を stdout の `[ROW-REVIEW]` の行で返す（行ごとに `[ROW-REVIEW] row=<doc>#<行 id> verdict=<V> basis=<B> record=<行の記録の dir の名>`・最後に `[ROW-REVIEW] result=<語> ref=<40 桁>`・rc は pass が 0、fail と pending と stale が 1、組めない周〔ref を解けない・台帳を読めない・置き場を書けない〕が 2）。席は Bash の background で待ち、完了の知らせを器は送らない。席の入力欄への差し込みは FR44 の閉じた列の外なので足さない（口を増やさない・C17.2）。
  7. **撃ち直さない**: 同じ行の判定の鍵（§9）の記録が既に在り、判定が unparsed でない行は撃たず、ref の記録にその記録を名指す。判定の鍵は、行の digest・材料の鍵（材料の dir の全 file。base の要約・要件の本文・外の材料・祖先の材料の file を含む）・code の木の鍵・basis・祖先ごとの状態の語・lens の版の全部で、どれか 1 つが違えば撃つ。PR の直しの commit で、行の中身も材料も code の木も変わらなかった行の審査は 1 回で済む。
  8. **撃ち中の印と重ねての撃ち**: 口は ref の dir に撃ち中の印（`<pid> <起動時刻>`・`lock_owner` で生死を判じる）を置き、撃ち終えて result の行を書いた後に外す。同じ sha に 2 本目の口を撃つと、印の持ち主が生きていれば lens を撃たずに `[ROW-REVIEW] result=pending` の 1 行と rc 1 で終わり（待たない・席は先の口の Bash の完了を待つ）、死んでいれば印を外して撃ち直す（形 7 で撃ち終えた行は撃たない）。
  9. **ref の結果**: 行ごとの判定（lens の JSON を Reviewed と同じ `read_outcome` → `narrow` の 2 本で読み、done の対応の表の倒し〔[contract-source.md](./contract-source.md) §64 形 4〕を通す）から ref の結果を 1 語に決める: 全行が PASS か、INCONCLUSIVE の行が全部 basis が forecast か partial で理由の型が unparsed でない → pass／FAIL の行か、basis が actual の INCONCLUSIVE の行か、unparsed の行が 1 本でも在る → fail／撃ち中の印の持ち主が生きている → pending／印の持ち主が死んで撃ち終えていない → stale。
- **行 a0 の純移動の形**（純移動の機械証明の残差の許容形に合わせる）:
  - 4 本は本文（`///` の doc を含む）を変えずに移し、変えるのは fn の頭の行の可視性だけにする（`materialize`・`requirements_of`・`digest` を `pub(in crate::pipe)` にし、`declare` は `materialize` だけが呼ぶので私有のまま）。
  - 4 本が引く層の型 `Layer` は事前審査の子 module の `pub(super)` の型で、その子 module は dispatch の私有の module なので、型の頭の行と、dispatch の親の事前審査の子 module の宣言の行の可視性を `pub(in crate::pipe)` に広げる（どちらも行数を変えない）。行 a1 の口 (G) と行 a の口も同じ可視性で引く。
  - 親に残す差は、移した 4 本だけが使っていた use の除き・子を引く use の 1 行・review の親の子 module の宣言の 1 行・上の 2 つの可視性・札と説明の 2 行だけにする。属性と use を 1 行に畳まない。
  - 先撃ちの子 module は in-file の歯を持たない（歯は e2e が外形で測る）。新しい子 module に歯の区間を作ると純移動の証明から外れるので、札 `// flip-check: moved <この行の bead の id>` は、説明の `//` の 1 行と合わせて、file 全体が歯の区間である e2e の review の歯の file の、先撃ちの歯の節の見出しの行の直後に置く。
  - 行 a0 の write-set の `-` の 3 file は増分 0 以下の宣言で、削除ではない（先撃ちの子 module は 4 本の分だけ縮み、事前審査の子 module と dispatch の親は可視性の字だけが変わる）。dispatch の親を `-` で持つのは、その余地（§2・23 行）を行 f と g が使い切るからである。
- **後の行が呼ぶ口**（(A)〜(F) と (H) は行 a1 が持ち、置き場は行 a1 の write-set の `+` の file で、その module は pipe の `pub mod`。(G) は行 a1 が事前審査の子 module に置く。名は実装が決め、役割・引数・閉じた返り・可視性をここで約束する）。(A)〜(F) と (H) は `pub` の関数で、hook の門・後の行・e2e の歯が同じ口を呼ぶ（歯のための 2 本目の読み手を作らない・C2）。行 a1 の歯は §9 の形で手で書いた記録を読み、行 a の書き手との形の一致は行 a の歯が測る:
  - (A) ref の結果の読み手（行 b が hook から呼ぶ）: 引数は state dir と 40 桁の sha。返りは閉じた 5 値＝pass（記録の merge-base の sha と契約表の file の列を持つ）・fail・pending・stale・missing（file が無い・読めない・1 行目が `schema=1` でない・result の行が無い〔撃ち中の印も無い〕）。撃ち中の印の生死は `lock_owner` の 1 本で判じる。
  - (B) 行の digest の口（行 a・行 c・行 g が呼ぶ・pure）: 引数は契約 file の字と設計の節の本文の字。返りは形 2 の 16 桁。
  - (C) code の木の鍵の口（行 a と行 c が呼ぶ）: 引数は repo と commit の sha。返りは §5 の鍵の 16 桁か、git を撃てない・tree を読めない理由（`Result`）。
  - (D) 写せる記録を引く口（行 c が呼ぶ）: 引数は state dir・`<doc>#<行 id>`・行の digest・材料の鍵・code の木の鍵・lens の版の 1 行。返りは、行と 4 つの鍵の材料が同じで basis が actual・祖先の状態の語が全部 landed か祖先なし・判定が PASS の行の記録が在る周だけ、その記録の dir の名（無い・読めない・PASS でない周は `None`）。
  - (E) 兄弟の読み手（行 g が呼ぶ）: 引数は state dir・`<doc>#<行 id>`・行の digest。返りは、その行をその digest で載せた ref の記録（§9 の row の行の digest の欄で照合）の全部に載る、その行以外の `<doc>#<行 id>` の列（重複なし・字の順）と、読めない ref の記録の file の数の対。
  - (F) 行の記録の一覧（行 g が呼ぶ）: 引数は state dir・`<doc>#<行 id>`・行の digest。返りは、その行とその digest の行の記録ごとの（判定・basis・理由の型・at の秒）の列（読めない記録は数だけを別に返す）。
  - (G) 祖先の層の口（行 a が呼ぶ・事前審査の子 module の `pub(in crate::pipe)` の関数・事前審査の予想はこの口の上に載る）: 引数は repo・state dir・読み済みの台帳の bead の列・`<doc>#<行 id>`。返りは、祖先ごとの（`<doc>#<行 id>`・状態の語〔landed・tree・declared〕・当てる層〔`Layer`〕）の列と basis の 1 語（actual・forecast・partial）の対か、組めない理由（`Result`・表か bead の pointer を読めない）。
  - (H) 判定の鍵の口（行 a が呼ぶ・pure）: 引数は `<doc>#<行 id>` と判定の鍵の 6 材料（§9）。返りは判定の鍵の 16 桁と行の記録の dir の名の 16 桁の対。
- forecast と partial の INCONCLUSIVE を pass に数える理由と C10 の読み: 祖先の本文は祖先が着地するまで存在しない。止めると依存を持つ行の設計の PR が祖先の着地を待って直列になる。「測れない」を merge の通過に倒すのは、その判定が便の段の判定として効かないからである: forecast と partial の記録は Reviewed の段で使い回されない（§5・材料の鍵か code の木の鍵が必ず違う）ので、その行は祖先の着地の後の Reviewed で実物の base で審査し直され、そこで測れなければ今どおり Reviewed で止まる。FAIL は basis に依らず止める。
- 索引の表（[reverse-index.md](./reverse-index.md) §7 (a)・(b)・(c)・[ADR-0105](../../design-intent/decisions/ADR-0105-code-facts-come-from-an-external-index-the-vessel-reads.html)）: vessel 宣言が code の索引を名乗る repo では、形 4 の機械の検査に索引の閉包と code の事実の欄の測りが加わり（確定の finding）、形 5 の材料に逆引きの表 index.txt が加わる。どちらも同じ epic の別の設計の行で、この設計の行 a の後に起こす。

## 4. merge の門の 2 つ目の判定（vessel 宣言の任意 key row-review）

- 読む宣言は 2 つ: anchor（hook の root）の HEAD の commit の宣言と、merge する PR の head の commit（`--match-head-commit` の値が 40 桁の 16 進の周だけ）の宣言。どちらも `ruling_keys_at` と同じ `git show <commit>:<file>` の読みで、作業ツリーの宣言は読まない。PR の head の commit が local の object db に在るかは、宣言を読む前に git の 1 本（commit の object の有無の問い）で分ける（`git show` の読みは commit が無い周と file が無い周を分けない・§2）。両方を読むのは、PR 自身が key を外して門を素通りする形と、key を足す PR が自分の門を持たない形の両方を塞ぐためである。
- 判定の枝（FR101 の条件・既存の trailer の 5 語の判定の後・当たる segment ごとに・次の順で見て最初に当たった 1 つを断る）:
  - (ii) **読めない宣言**: anchor の HEAD の宣言か、local に在る PR の head の commit の宣言が、在るのに読めない（key の行の不備を含む）→ key の有無に依らず断る（C10）。語は row-review-missing で、断りの 1 行は読めない理由の句「宣言が読めない」と、読めない commit と不備の 1 つ目と、次に撃つ口 `git fetch origin` を名指す。
  - (iii) **local に無い head**: `--match-head-commit` の値が 40 桁の 16 進で、その commit が local の object db に無い → anchor の HEAD の宣言が `row-review = true` を持つ repo に限り断る。語は row-review-missing で、断りの 1 行は読めない理由の句「head の commit が local に無い」と、local に無い sha と、次に撃つ口 `git fetch origin` を名指す。anchor の宣言が key を true で持たない repo（消費側）の merge は変えない（PR の head の宣言を読めないので anchor の宣言だけで決める）。
  - (i) **当たる repo**: anchor の HEAD の宣言か local に在る PR の head の commit の宣言のどちらかが `row-review = true` を持つ repo だけが、判定の順の 6 語で断る（key を true で持つ repo でも、pass の sha を固定した command はどの枝にも当たらずに通る）。
  - head の commit が local に無い周を断るのは、anchor の HEAD の宣言が key を true で持つ repo だけである。anchor が key を持たない repo では、key を足す PR の門は、head の commit が local に在って宣言を読める周にだけ掛かる。名乗っていない消費側の repo の merge を黙って変えないためで（ADR-0103 DR6）、「読めない周は断る」（DR7）は、宣言が在るのに読めない周（(ii)）と、key を true で持つ anchor の周（(iii)）に効く。
  - 門が FR92 の判定（trailer の 5 語）だけになる repo は、(ii) に当たらず、読める宣言（anchor の HEAD と local に在る head）のどれも key を true で持たない repo である（key が無い宣言と値が false の宣言の両方を含む）。
    1. `--match-head-commit` の値が 40 桁の 16 進でない（無い・短い・展開の字〔本文の読みと同じ `is_loose_path` の規則〕を含む）→ no-head-pin。head を固定しない merge は、審査した commit と merge される commit が同じだと言えない。
    2. 置き場の ref の記録（§9）がその sha に無い・読めない（file が dir・byte が UTF-8 でない等）・schema が違う・result の行が無く撃ち中の印も無い → row-review-missing。
    3. ref の結果が pending → row-review-pending、stale → row-review-stale。
    4. ref の結果が fail → row-review-failed（形 5 の動きが同時に在っても failed を名指す・判定の順）。
    5. ref の記録が名指す契約表の file のどれかが、記録の merge-base と今の anchor の `origin/main` の間で変わった（`git diff --quiet` の 1 本）→ row-review-moved。審査の後に main へ入った別の PR が同じ doc の行か節を変えた周に、merge の結果の行が一度も審査されていない形を止める。
- 断りの語は既存の 5 語の後ろに宣言順で 6 語を足す（(i) の判定の順と同じ・(ii) と (iii) は語を足さずに row-review-missing を使い、1 行の理由で分ける）。断りの 1 行は `<NAME>: deny merge-gate reason=<語>` で始まり、(i) の 6 語は次の一手として `pipe review --ref <sha>` の argv か ref の記録の path を、(ii) と (iii) は読めない理由の句（(ii) は「宣言が読めない」・(iii) は「head の commit が local に無い」）と次に撃つ口 `git fetch origin` を名指す（FR101）。hook の記録は既存どおり `merge-deny <語>` の 1 行。
- 門が新しく読むのは、vessel 宣言の file（2 つの commit）と PR の head の commit の有無と置き場の ref の記録の 1 file（§3 の口 (A)）と、契約表の file の差の有無だけで、台帳と event log は読まない。置き場は hook の入口が他の門（anchor の門）と同じく受けた state dir を使い、`decide` は root と state dir も受ける（hook の入口の 1 か所の呼び出しを直す）。trailer の判定の私有の関数と既存の歯は変えない（2 つ目の判定は別の関数で足す）。
- 同じ行で、`--help` と `-h` を持つ `gh pr merge` を merge と読まない 1 形を見分けに足す（今の門は help の表示を no-body で断る・§2）。見分けは anchor の門と共用の 1 関数なので、help の command は anchor の門の窓の判定も受けない。

## 5. Reviewed の段の使い回し（先撃ちの使い回しの読み口を置き換える）

- Reviewed の段は、実物の base で組んだ判定の鍵（§9・材料の鍵と code の木の鍵と lens の版を含む）と同じ鍵の行の記録が在り、その記録の判定が PASS で basis が actual の時だけ、lens を撃たずに記録の判定を写す。段の detail の末尾の語は ` row-review:reused`（`read_detail` は語で読むので report は変わらない）。違えば今どおり撃つ。forecast と partial の記録は写さない（祖先の実物か doc を跨ぐ祖先が審査の材料に無かった）。
- 鍵に lens の cmd の字を入れない（binary の置き場の path が入り、入れ替えのたびに外れる）。lens の同一性は lens の版で比べる。lens の版は lens の子自身が出す（行 h）: lens の口に、claude を撃たずに自分の `--rules` と組み込みの雛形から版の 1 行を stdout に出す flag `--print-version`（値を取らない・lens の閉じた flag の列の 9 語目で、本体の列と、本体より先に argv を照らす lens の入口の列〔`crates/scribe2/src/headless/mod.rs` の lens の `ALLOWED`〕の両方に足す）を足し、行の審査は撃つ前に、Reviewed は写す前に、同じ lens の cmd にその flag を付けて撃って読む（撃つ側の定数から推さない）。
  - 版の 1 行は `lens-version ` で始まり、`--stage` の値ごとに lens の子が claude を起こす時と同じ組み立て（`rows_of` の cap と、`call_of` が組む `Call` の欄のうち prompt の本文と path の値〔claude の実行 file・口座の dir・cwd〕を除いた全部＝model・effort・道具の列・permission の mode・出力の形式・turn の上限）と、組み込みの雛形 3 本（lens.txt・lens-contract.txt・lens-memo.txt）の本文の FNV-1a 64 を `key=value` で並べる。`call_of` の組んだ値から作るので、後の行が lens の子の `Call` に値（turn の上限など）を足せば版も自ずと変わる。
  - flag の周は `--contract` と `--worktree` を要らず、在っても読まない。`--rules` を読めない・行が欠ける周は、claude を起こす周と同じ断り（rc と 1 行）で版の行を出さない。
- code の木の鍵を入れるのは、読みの道具を持つ lens の判定の入力が材料の dir の外（審査の木の全部）に広がるためである。code の木の鍵は、審査の木の tree から契約表を持つ file を除いた全 file の path と blob の hash の列の digest で、他の PR が設計 doc を直しただけでは動かず、code を変える着地で動く。
- Reviewed の段が鍵を組む材料: 行の digest は便の契約 file の字と材料の dir の design.txt の字から §3 の口 (B) で、材料の鍵は審査の材料の dir から共用の `digest` で、code の木の鍵は審査の木の sha（`review` が材料の前に読む HEAD）から §3 の口 (C) で、lens の版は便の lens の cmd に行 h の flag を付けて撃って求め、§3 の口 (D) で引く。4 つのどれかを求められない周（版の行が出ない・木の鍵を読めない）は引かずに撃つ（使い回しを諦めるだけで判定は変えない）。便の契約の design の pointer が `<doc>#<行 id>` の形でない周も引かない。
- 先撃ちの使い回しの読み口（`reusable`）は行 e まで残し、行 c は行の審査の読み口を足して先撃ちの読み口の前に置く（先撃ちの使い回しの歯は行 e まで緑のまま）。写した周の段の detail の末尾は ` row-review:reused` で、先撃ちを写した周の ` prelens:reused` とは別の語である。
- 使い回した周は審査の消費を書かない（lens を撃っていない）。行の審査の lens の消費は行の記録に残す（先撃ちの消費が便の記録に載らなかった穴を繰り返さない）。

## 6. 先撃ちの退役（事前審査の機械の予想と束は残す）

- 段 1（§11 の行 d）: rules 行 `pipe.precheck_lens_per_round` の値を 0 にし（値の変更は裁定 id が要る）、同じ行で先撃ちの `round` が値 0 の周に材料の組み直しも撃たないようにする（今は値に依らず一時の worktree を毎周組む・§2）。同じ行で本 repo の vessel 宣言に `row-review = true` を足す。事前審査の機械の予想（形 x）と束（形 y）は残す（lens を撃たず安い・依存待ちの確定の誤りを束ねる役は行の審査と重ならない）。
- 段 2（§11 の行 e）: 先撃ちの残りの code を外す: 先撃ちの子 module の残り（行 a0 が共用の子 module へ移した 4 本は外さない）と、その宣言の行、lens の `--stage prelens`、rules 行 `pipe.precheck_lens_per_round` と `pipe.precheck_lens_model`、先撃ちを測る歯、`[DISPATCH-PRECHECK]` の行の ` prelens=` の字面、`Review` の先撃ちの model の欄と Reviewed の先撃ちの使い回しの読み口と ` prelens:reused` の語。外すのは git の履歴に残る code の退役（憲法 N1 の可逆な形）で、使い回しの読み口は行 c が行の審査の記録へ移した後に外す。共用の子 module の宣言の祖先の腕（`declare`）は層の型の match の腕として残す（行の審査は宣言の層を木に当てないので呼ばれない・層の型を分けるのはこの設計の外）。

## 7. 行の予約（落ちた行が write-set を持ち続ける）

- 行の予約を持つ行 B（条件と解ける契機の正本は [FR102](../../design-intent/spec/srs.html#FR102)・この節は実装の注）: FR102 の条件（台帳で open・memo の label も台帳の問いの label も持たない・直前の便が Landed でない終端・その後に同じ bead の便が無い・最新の介入の印が hold でない・期限の内）を、列の周ごとに既存の記録から導く。
  - 実装の注: 「直前の便」は置き場の replay の同じ bead の run id の昇順の最後、「Landed でない終端」は Reviewed の判定が PASS でない・Gated の判定が FAIL・Failed・Stopped の 4 形、「その後の便」はその run より後の同じ bead の RunCreated、期限の起点はその終端の段の event の ts。B が列の候補に居ない周（依存で待つ・pointer が無い・`Settled` で列外）も B は行の予約を持つ（FR102 の括弧）。
  - B の並びの位置は `order` と同じ鍵（`key_of`・first の印 → 台帳の priority → 起票順）で決め、B が候補に居ない周も B の台帳の priority と印で比べる。
- 行の予約に入れる file: B の直前の便の契約の写し（便の置き場の契約 file）の write-set。写しを読めない B は行の予約を持たず、その周の `dispatch ls` の B の行（候補に居れば）は既存の理由のまま（写しの読めなさは受付と同じく別の口が名指す）。
- 効き: FR68 の順序の 1 関数（`order`）で B より後ろに並ぶ候補のうち、write-set が B の行の予約と交差する（FR39 と同じ判定・`overlaps` の交差の読み・tracked は同じ周の材料）ものは、`settle` の中で交差と枠より先に、待ちの理由の新しい variant（名 reserved）で待つ。B より前の候補（介入 first・高い priority・若い起票順）は待たない。B が台帳の blocks で待つ祖先（推移）の候補は待たない（B の直しとして前提の行 C を足し B に C を blocks で付けた形で、B が依存で待ち C が行の予約で待つ輪を作らない）。1 つの候補に B が 2 つ以上当たる周は、並びの前の B を名指す。
- 直った行が最初に起きる: 直しの PR で B の契約の中身が変わると B は `Settled` を抜けて候補に戻り、元の順位（first → priority → 起票順）のまま、行の予約で待たせていた後ろの行より先に評価される。B が受付や依存で待つ周も、B が新しい便を起こすまで行の予約は続く。
- 行の予約が解ける契機: FR102 の列（B の新しい便の起動・B の close・B の memo か問いの label・B への hold の印・期限）のとおり。hold は orchestrator が B を置いて後ろを先に通すと決めた印で、今の口のまま。
- 期限の行: rules 行 `pipe.reserve_h`（kind は新しい 1 つ `PipeReserveH`・Int・時間・値 24・裁定 id `user 2026-09-30T22:13Z 項 reserve`・裁定日 2026-09-30）。値 0 は期限なし。行が無い・読めない周は期限なしで行の予約を掛け、reserved の値の末尾に `/unset` を足す（測れない期限を「行の予約を掛けない」に畳まない・C10）。
- 観測: `dispatch ls` の reason は `reserved:<B の bead>/<交差した file の本数>`（期限の行が読めない周は `reserved:<B の bead>/<本数>/unset`）。variant は値の型を行 f の新しい子 module に置いた tuple の 1 つ（`Floor` と同じ形）で、dispatch.rs に足す行を variant と名と描きの 3 か所に絞る（§2 の余地）。期限が切れた B は行の予約を持たなくなるだけで、B が `Settled` のまま処置を待つ周は既存の idle の知らせの未処置の終端の語（[dispatcher.md](./dispatcher.md) §29 の ` pending=`）が名指し続ける（新しい送達も新しい code も足さない）。
- 局面の表: 名 reserved は `crates/scribe2/src/case/mod.rs` の `QUEUED_TURNS` に手番 seat で足す（B の直し・hold・close のどれも席の手）。[case-lifecycle.md](./case-lifecycle.md) §3 の表は先に置いた。

## 8. 兄弟の待ち（同じ設計から出た行を、直しが入るまで待たせる）

- 待たせる元 B・兄弟・除く候補・解ける契機の正本は [FR103](../../design-intent/spec/srs.html#FR103)（この節は実装の注）。B は台帳で open な契約で、直前の便が設計の側の終端（Reviewed の判定が FAIL か unparsed でない INCONCLUSIVE・Gated の判定が FAIL）に着いた行。Failed（起動の失敗・環境）と Stopped（人の停止）と unparsed の INCONCLUSIVE は元にしない。§7 と同じく「直前の便」は置き場の replay の同じ bead の run id の昇順の最後で、その後に同じ bead の便が起きた B は元にしない。
- 兄弟: B と同じ設計 doc の行のうち、(a) B と同じ section を実装する行、または (b) B の直前の便の契約と節の写しから §3 の口 (B) で求めた行の digest で B の行を載せた ref の記録（1 本でも・push ごとの記録も直しの PR の記録も含む）に載る、B 以外の行（§3 の口 (E)）。起動の列の候補（live でない）だけが対象である。(a) の section は B と候補の設計 pointer の行の欄 section（受付と同じ表の読み手 `find_row` で main の先端の契約表から読む）で比べ、行を読めない候補は既存の理由のまま兄弟にしない。
- 効き: 兄弟の候補は待ちの理由の新しい variant（名 sibling・値は B の bead）で待つ。介入 first の印を持つ候補は待たない（orchestrator が名指して起こす口を残す・[dispatcher.md](./dispatcher.md) §35 の床の検査と同じ扱い）。B が台帳の blocks で待つ祖先（推移）の候補は待たない（§7 と同じく、B の直しとして同じ節に前提の行を足し B に blocks で付けた形で、B が依存で待ち前提の行が sibling で待つ輪を作らない）。候補に B が 2 つ以上当たる周は、run id の昇順の最初の B を名指す。
- 解ける契機の実装の注: 「B の digest が違う周」は B の今の行（main の先端の契約表と設計 doc）から求めた digest と B の直前の便の写しの digest の比べ。「兄弟自身の記録」は兄弟の今の digest の行の記録（§3 の口 (F)）の at が B の終端の段の event の ts より後のもの。期限は §7 の `pipe.reserve_h` を共用し、行が無い・読めない周は期限なし（値の末尾に `/unset`）。ref の記録か行の記録を読めない周はその記録を無いものとして数える（兄弟を増やさず解けも増やさない・§12 の限界）。
- 候補 1 件の理由の決め方の順: 依存 → hold → launched → 設計 pointer → 契約の生成 → settled → sibling → 床の上書き → 未反映の裁定の上書き → `settle` の中で reserved → overlap → 受付。名の列 `WAIT_REASONS` では、着地済みの `unreflected-ruling` と `floor` の後ろに reserved（行 f）と sibling（行 g）を足す。variant の値は bead id の字 1 つ（tuple）で、dispatch.rs に足す行を variant と名と描きの 3 か所に絞る（§2 の余地）。局面の表 `QUEUED_TURNS` には手番 seat で足す。
- 行 a1 と b1 の例では、b1 は (b)（同じ設計の PR）で a1 の兄弟になり、a1 の終端の 4 秒後の起動は起きない。

## 9. 置き場の形と跨版の約束（event kind は足さない）

- 行の予約と兄弟の待ちは記帳しない。列の周ごとに、既存の記録（便の段の event・便の置き場の契約 file と材料の写し・介入の印・RunCreated）と rules 行と行の審査の記録から導く（新しい event kind を足さない・C17）。待ちの理由の閉じた型に variant を 2 つ足す（§8 の位置）。台帳と event log は列の周が既に 1 回読んだものを使い、2 度読まない。
- 判定の鍵: 行の digest・材料の鍵（材料の dir の全 file の名と本文から名の順に決まる digest）・code の木の鍵（§5）・basis・祖先ごとの状態の語の列・lens の版（§5）を 1 行ずつ並べた字の digest。
- 行の審査の置き場: state dir の pipe の下の row-review の dir。
  - 行の記録の dir: 名は `<doc>#<行 id>@<判定の鍵>` の字の FNV-1a 64 の 16 桁（束の id と同じ形）。中に record（1 行 1 key の `key=value`・1 行目は `schema=1`）・材料の dir（Reviewed と同じ file 名）・lens の `rc` と `out`。
  - record の key: row・digest（行の digest）・key（判定の鍵）・basis（actual / forecast / partial）・ancestors（`<行>:<landed|tree|declared>` の列か `-`）・mech（clean か `firm:<断りの名>`）・verdict・kind（`-` か理由の型）・materials（材料の鍵）・tree（code の木の鍵）・version（lens の版の 1 行）・ref・at（UTC の秒）・usage（lens の消費の 6 値か `-`）。
  - ref の記録: ref の dir の下に 40 桁の sha の名で 1 file。1 行目は `schema=1`、2 行目は `base=<merge-base の sha>`、3 行目は `tables=<契約表の file の列>`、変わった行ごとに `row=<doc>#<行 id> digest=<行の digest> id=<行の記録の dir の名> verdict=<V> basis=<B>` の 1 行、最後の行は `result=<pass|fail>`（撃ち終えた後に書く）。撃ち中の印は同じ dir の `<sha>.pid`。審査の木は ref の dir の下に行ごとの dir を切って `Worktree` で作り、撃ち終えた行ごとに外す（dir の名は実装が決める・§3 の口の約束の外）。
  - 読み手の約束: 印が在り持ち主が生きていれば pending、印が在り持ち主が死んでいて result の行が無ければ stale、印が無く result の行が無い・読めない・schema が違う file は無いと同じ（merge の門は row-review-missing・Reviewed は lens を撃つ）。書きは一時 file → rename。跨版で読める約束はこの 1 形だけで、形を変える版は schema を上げて古い記録を読まない（撃ち直せば作り直せる）。
- 記録を畳む規則（量の上限と掃除）は持たない（限界・別の設計・消す仕組みは A1 に当たる）。

## 10. 費用（実測と推定を分ける）と足す物・消す物（C17.2）

- 実測（2026-09-28〜09-30・CLI の total_cost_usd・list 単価）: Reviewed の審査 163 本で $133.4（1 本あたり約 $0.82）、gate の lens 128 本で $54.4、先撃ち 114 本で $54.5（Reviewed の 41%・便の消費の記録に載らない）。gate の FAIL 1 件は起動から判定まで中央値 17.7 分（30 件で計 10.5 時間）。
- 推定（未実測）: 行の審査は行の判定の鍵ごとに 1 回で、読みの道具を持つ lens は 1 turn の今の審査より重い（1 本 $2〜4 と置く・打ち手 0 の turn の上限で上から押さえる）。先撃ちの退役で $54.5 相当が浮く。Reviewed の使い回しは、行の審査から便の起動までの間に code を変える着地が無い周だけ当たり、当たる割合は未実測である（低ければ費用は行の審査の分だけ増える）。減る費用は審査と gate の FAIL の周回（設計の側の 73 件の多く）と、落ちた行の順番の崩れで後ろの行が作り直す分である。着地の周の後に実測して ADR-0103 の見直しの材料にする。この消費を足すことは SRS の round で持ち主に問う（srs-B の grill の論点）。
- 足す物と消す物（C17.2 の対）:

| 足す | 消す（同じ program の中） |
|---|---|
| 口 `pipe review --ref` 1 つ | 先撃ちの子 module の残り・lens の `--stage prelens`・先撃ちの起動と置き場（行 e） |
| on-disk の形 2 つ（行の記録・ref の記録） | 先撃ちの置き場の形 1 つ（`precheck/lens/<bead>/`）（行 e） |
| 任意 key 1 つ（row-review）・merge の門の断りの語 6 つ | —（門は既存の 1 段に判定を足す） |
| 待ちの理由 2 つ（局面の表の 2 語を含む）・rules 行と kind 1 つ（`pipe.reserve_h`） | rules 行と kind 2 つ（`pipe.precheck_lens_per_round`・`pipe.precheck_lens_model`）（行 e） |
| lens の口の版の flag 1 つ | 使い回しの読み口 1 つ（`reusable`）（行 e） |

## 11. 契約表の行（粒度・順序・write-set・歯）

契約表は doc の末尾の区間の 10 行。write-set と growth は main 92a5ddc8 の現物で測った（行数は幅で正規化した R-C4-2 の数え方）。起票の前に `pipe preflight` と行の審査そのもので測り直す。

| 行 | 中身 | depends（同じ doc） | 置き場の要点 |
|---|---|---|---|
| h | lens の版の flag（§5） | — | lens.rs・lens の入口の flag の列（headless/mod.rs）・help の頁・headless の外形 snapshot |
| a0 | 先撃ちの私有の 4 本の共用の子 module への純移動（§3 の「行 a0 の純移動の形」） | — | pipe の review の新しい子 module（`+`）・review.rs・先撃ちと事前審査の子 module と dispatch の親（`-`）・e2e の review の歯の file（札） |
| a1 | 行の審査の記録の読み手と鍵の口 (A)〜(F)・(H) と、事前審査の予想の祖先の行の鍵の口 (G) への載せ替え（§3・§9） | a0 | pipe の新しい子 module（`+`）・pipe の親・precheck.rs |
| a | 行の審査の口（§3）＝撃つ側と記録の書き手 | h・a1 | pipe の新しい子 module（`+`）・subcommand の 4 面（cli.rs・args.rs・help・外形 snapshot）・review.rs |
| b | merge の門の 2 つ目の判定と任意 key row-review（§4） | a | hook の 3 file（merge の門・anchor の門・入口）・宣言の 2 file |
| c | Reviewed の段の使い回し（§5） | a | review.rs |
| d | 先撃ちの退役の段 1（§6） | b・c | rules 行の値・本 repo の vessel 宣言・prelens.rs |
| e | 先撃ちの退役の段 2（§6） | d | 先撃ちの子 module（`~`）と縮む面（`-`） |
| f | 行の予約と rules 行 `pipe.reserve_h`（§7） | — | dispatch の新しい子 module（`+`）・dispatch.rs・candidates.rs・局面の表・rules |
| g | 兄弟の待ち（§8） | a・f | dispatch.rs・candidates.rs・局面の表（行 f の子 module は触らない） |

- 行 a の割り（SRS NFR2 の見積 550 行以内・大きい便ほど runner が落ちる実測）: 元の行 a（src 約 905・和 1555）を 3 行に割った。共用の 4 本の移しは純移動の行 a0（歯を足さず札で flip-check を通る・src の増分は新しい子 module の約 90 だけ）、記録の読み手と鍵の口と祖先の層の載せ替えは行 a1（src 約 382）、口と記録の書き手は行 a（src 約 484）。祖先の層の載せ替えを行 a に残すと src が約 584 で 550 を超え、載せ替えだけの行は新しい歯を持たず flip-check の no-test-diff で落ちるので、新しい歯を持つ読み手の行 a1 に入れた。行 a1 の読み手は行 a の書き手より先に着地するので、行 a1 の歯は §9 の形で手で書いた記録を読み、書き手との形の一致は行 a の歯 (m) が測る。
- dispatch.rs の余地（§2・23 行）: 行 f の見込み 14 と行 g の見込み 9 の和がちょうど収まる。値の型と導きは dispatch の子 module に置き、dispatch.rs に足すのは子 module の宣言・variant・名・描き・列の周の 1 回の導きの呼び出しと、既存の歯の列の追記だけにする（§7・§8）。同じ余地を使う別の行が先に着地すると、行 f か g が受付の cap-headroom で止まる。その時は dispatch.rs の純移動の行を先に流す（この doc の外の設計）。
- 行 a0 と行 e の `-` の file は「増分 0 以下の宣言」で削除ではない（file は残り、行 a0 では 4 本の分と可視性の字だけ、行 e では先撃ちの部分だけが減る）。file ごと消えるのは `~` の 1 file（先撃ちの子 module）だけ。
- 歯の置き場は全部既存の歯の file（新しい e2e の module を作らない・新しい src の file の in-file の歯だけの行にしない）。接頭辞は行ごとに別で、ほかの行の歯の名の途中に当たらない: h `headless_lens_version_`・a0 は歯を足さない（札だけ）・a1 `pipe_review_record_`・a `pipe_review_ref_`・b `hook_merge_gate_row_review_`・c `pipe_review_row_reuse_`・d `pipe_prelens_off_` と `hook_merge_gate_self_declaration_`・e `headless_lens_stage_retired_` と `rules_prelens_retired_`・f `pipe_dispatch_row_reservation_` と `rules_reserve_`・g `pipe_dispatch_sibling_wait_`。
- 既存の歯の本文を直す行（数と列の pin・rules の値の pin・待ちの理由の列）は、直した歯が base で落ちる（数と列が base と違う・variant が無く compile で落ちる）ので retroactive の札を要らない。本文を直さずに挙動が変わらないことを測る既存の歯は、各行の done に「変わらない既存の歯」と名指し、歯の file が write-set の外なら `=`（置き場だけ）で持つ。
- rules 行を足す行 f と外す行 e は、ALL と manifest の末尾を pin する既存の歯を run の base の列から直す（同じ epic の別の束の行が同じ歯を直すので、受付の交差で直列になる）。
- 行 b の宣言の key の列の pin は run の base の列（契約表の行 bu が 3 key を足した後なら 20 key）の末尾に row-review を足す形で直す。

## 12. 限界

- GitHub の画面など、席の道具の呼び出しでない merge は門の外に在る（今の merge の門と同じ）。器が着地させる便は hook を通らないので、runner が設計 doc の行か節を書き換えた便（自分の行を書き換える純移動の便など）の行は行の審査を通らずに列に入る。列に入った後は今どおり Reviewed が審査する。
- 行の審査の記録は host の state dir に在る。別の host から merge すると記録が無く row-review-missing で断られ、その host で口を撃ち直す（lens の費用が 2 回かかる）。
- 門は merge の結果でなく PR の head と、契約表の file の main での動きを見る。契約表の file でない所（要件・code）の main での動きは門を止めず、列に入った後の Reviewed が測る。
- Reviewed の使い回しは、行の審査の後に code を変える着地が 1 本でも在れば外れる。外れた周は今どおり撃つ（費用が減らないだけで、判定は古くならない）。
- forecast と partial の INCONCLUSIVE は merge を通る。その行の穴は祖先の着地の後の Reviewed で初めて止まり、そこで落ちた行は §7 と §8 が順番を守る。partial の行は、doc を跨ぐ未着地の祖先に由来する FAIL で止まりうる（行の審査の前に bead と blocks を起票すれば forecast になる）。
- 兄弟は同じ設計 doc の中だけで、別の doc の行が同じ指示を写した形は拾わない。逆に、新しい doc を 1 本の PR で起こした時は (b) がその doc の全部の行を兄弟にするので、欠陥と関係の無い行も直しが入るまで待つ（行 a1 の例では直しまで 53 分・orchestrator は first で名指して起こせる）。捨てた PR の ref の記録にも B の digest が載っていれば、その記録の行も兄弟に数える。
- 行の予約に入れる write-set は落ちた便の契約の写しで、直しの PR が write-set を広げた分は B の次の便が起きるまで行の予約に入らない。
- 行の審査の記録は畳まれず、push ごとに増える（量の上限と掃除は別の設計）。
- 読みの道具を持つ lens の費用と所要は未実測（§10）。
- head を 40 桁で固定しない merge は PR の head の宣言を読めない。anchor の宣言が key を true で持たない repo では、key を足す PR の head を固定しない merge は no-head-pin にならず FR92 の判定だけで通る（§4 の (i)・門が掛かるのは head の commit が local に在って宣言を読める周だけ）。
- (ii) と (iii) は 6 語の外の語を足さず row-review-missing で断り、1 行の理由で分ける（hook の記録の語だけでは結果が無い周と読めない周を分けられない）。
- 兄弟の待ちは、読めない ref の記録と行の記録を無いものとして数える（兄弟を増やさず解けも増やさない）。読めない記録が在っても dispatch ls には出ない。

## 13. 却下

- 先撃ちを Opus に戻すだけ: 使い回しは材料の鍵で外れる（使い回しの口が在った窓で Opus の先撃ちの後の Reviewed 8 回のうち 0 回・予想の印を外しても 20 回中 1 回）。当たっても先撃ちの見逃しが素通りする。母集団も依存を待つ行だけで、Reviewed の FAIL 46 件のうち 34 件（依存の無い行と再受付の便）には原理的に届かない。
- 先撃ちの判定で起動を止めるだけ: 流れた 6 件は止まるが、届く母集団は同じく依存待ちの行だけで、審査が列の後ろに在る形は変わらない（落ちた行が列から外れて順番が崩れる）。
- CI で lens を撃つ: 本 repo は PUBLIC で、CI に lens の口座と model を持たせることになる。口座の選定・封じ込め・rules 行は host の器が持つ。CI は台帳に届かず、変わった行が着地済みかを判じられない。PR の push ごとに撃つと費用が PR の数に比例する。
- 行の審査を merge の後（列に入った直後）に撃つ（旧 (r) の契約が出来た直後の審査・[dispatcher.md](./dispatcher.md) §2 が ADR-0045 の後に持たないとした形）: 行は既に main に在り、落ちた行の直しにもう 1 本の PR が要る。行の字が変わるのは merge の時点だけなので、門はそこに置く。
- 審査の判定を記録しない（[dispatcher.md](./dispatcher.md) §16 の却下「審査の判定を § の sha に紐づけて記録する」を守る）: §16 が退けたのは、便の中の審査の判定を列外の鍵のために別の面へ写す形である。本設計の記録は merge の門の入力で、門は記録が無ければ断るので、面を足さずに merge の前の審査を器の段にする道が無い。§16 の理由（面を足す）は §10 の C17.2 の対で払う。
- 完了を席の入力欄へ知らせる: FR44 の閉じた列の外の新しい差し込みになる（ADR-0045 が消した席への差し込みの復活）。口を前面で最後まで撃ち、席が Bash で待つ。
- 行の予約を起動の列の全部の待ちの候補に広げる（順位の高い待ちの行が常に file を持つ）: 依存を待つ行が長い間、関係の無い後ろの行を止める。落ちた行だけに絞る。
- 兄弟を同じ doc の全部の行にする: 何度も行を足してきた doc（行が数十本在る doc）で、別の時に別の前提で書いた行まで 1 行の欠陥で止める。同じ節と同じ設計の PR（同じ時に同じ前提で書いた行）に絞る。
- 兄弟を「同じ指示を持つ行」で選ぶ: 指示の同一は散文の読みで、機械で判じられない（C3）。同じ設計の PR を代わりの印にする。
- 行の予約と兄弟の待ちを event に記帳する: 既存の記録から毎周導けるので、新しい event kind（C17）は要らない。
- 門が PR の head の宣言だけを読む: PR 自身が key を外すと自分の門を素通りする。anchor の HEAD の宣言だけを読むと、key を足す PR が自分の門を持たない。両方を読む。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "h"
title = "lens の子に版の 1 行を出す flag --print-version を足す — claude を起こさずに rules の cap と Call の欄（prompt の本文と path を除く）と組み込みの雛形 3 本の digest を lens-version の 1 行で stdout に出し、行の審査と Reviewed の使い回しの鍵にする（§5）"
req = ["FR100", "FR49"]
section = "5"
write-set = ["crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "=crates/scribe2-boundary/tests/e2e/headless.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_version_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_external_form"]
size = "S"
growth = ["crates/scribe2/src/headless/lens.rs:45", "crates/scribe2/src/headless/mod.rs:2", "crates/scribe2/src/help.rs:2", "crates/scribe2-boundary/tests/e2e/headless/lens.rs:140"]
done = "(1) lens --print-version --rules R は --contract と --worktree が無くても rc 0 で stdout がちょうど 1 行・lens-version で始まり、stderr は 0 byte で、--claude に渡した偽 claude（起こされた回数を file に数える）は 0 回〔headless_lens_version_ の (a)〕 (2) 1 行は model・effort・cap・tools・permission・output・turns の key と雛形 3 本（lens.txt・lens-contract.txt・lens-memo.txt）の本文の digest の key を key=value で持ち、rules の写しの lens.model・runner.effort・gate.token_cap をそれぞれ 1 つだけ変えた 3 回はどれも基準の行と違い、pipe.size_s_lines だけを変えた回と同じ写しの 2 回目は基準の行と同じ〔(b)・変えた行ごとに 1 対〕 (3) model の値は lens の子が claude を起こす時と同じ rules 行から読み、lens.model と pipe.precheck_lens_model が違う写しで --stage 無しと --stage memo は lens.model の値・--stage prelens は pipe.precheck_lens_model の値を持つ〔(c)〕 (4) --contract と --worktree に無い path を渡しても読まず、rc 0 で (1) と同じ行〔(d)〕 (5) --rules の file が無い写しと gate.token_cap の行を欠く写しは、claude を起こす周と同じ rc と同じ断りの 1 行を出し、stdout は 0 byte〔(e)・2 形〕 (6) flag は lens の閉じた flag の列の 9 語目で値を取らず、本体の KNOWN_FLAGS と、本体より先に argv を照らす lens の入口の許す列（headless/mod.rs の lens の ALLOWED）の両方に値を取らない形で載り、lens の usage と help の lens の頁が flag を 1 つずつ載せる〔外形の snapshot headless_external_form を作り直す・(f) の help lens の頁の行〕 base は --print-version が lens の入口で未知の引数（rc 2）として断られるので (a)〜(e) が RED・(f) は頁に行が無く RED・snapshot は usage の字が違うので RED"

[[contract]]
id = "a0"
title = "先撃ちの私有の 4 本（materialize・declare・requirements_of・digest）を pipe の review の下の新しい子 module へ純移動する — 本文を変えず、fn の頭の可視性と、4 本が引く層の型と事前審査の子 module の宣言の可視性だけを pipe の中へ広げ、先撃ちと後の行の審査の口が共用する（§3 形 5・行 a0 の純移動の形）"
req = ["FR100"]
section = "3"
write-set = ["+crates/scribe2/src/pipe/review/tree.rs", "crates/scribe2/src/pipe/review.rs", "-crates/scribe2/src/pipe/dispatch/prelens.rs", "-crates/scribe2/src/pipe/dispatch/precheck.rs", "-crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "=crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_prelens_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_precheck_", "cargo nextest run -p scribe2 --lib --no-tests=fail precheck_intake_label_"]
size = "S"
growth = ["crates/scribe2/src/pipe/review/tree.rs:90", "crates/scribe2/src/pipe/review.rs:1", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:2"]
done = "(1) 4 本は本文（/// の doc を含む）を変えずに新しい子 module へ移り、変わるのは fn の頭の行の可視性だけ（materialize・requirements_of・digest は pub(in crate::pipe)・declare は私有のまま）〔gate の段 ①（crates/scribe2/src/pipe/gate/verify.rs の約束の測り）が write-set の + の file の在りを測り、gate が diff の代わりに lens へ渡す純移動の機械証明（crates/scribe2/src/pipe/move_proof.rs・名と本文の hash の多重集合の一致・判定行の items= と moved= と visibility=）が 4 本の移動と本文の不変を測る・4 本を移さない実装は + の file が無く段 ① で rc 1〕 (2) 親に残る差は、移した 4 本だけが使っていた use の除き・子を引く use の 1 行・review の親の子 module の宣言の 1 行・事前審査の子 module の宣言の行と層の型 Layer の頭の行の可視性（pub(in crate::pipe)）・札と説明の 2 行だけで、ほかの item の本文は変わらない〔同じ機械証明: 残差が宣言と札とコメントと空行でない diff は純移動でない側に倒れ、gate の lens が diff のまま読む〕 (3) 札 // flip-check: moved <この行の bead の id> を、説明の // の 1 行と合わせて、e2e の review の歯の file の先撃ちの歯の節の見出しの行の直後に置く〔器がどの便でも撃つ共通 verify の 1 本目（.vessel.toml の common-verify・cargo xtask flip-check --base <便の base>）: 札の在る周は moved=1 で rc 0、src を変えて歯の file に札の無い diff は通さない〕 (4) 先撃ちの材料の組み・実体化・使い回しと事前審査の予想の挙動は変わらない〔変わらない既存の歯 pipe_prelens_・pipe_dispatch_precheck_・precheck_intake_label_〕 base と比べて歯は足さない（純移動）。verify の 3 本は base でも HEAD でも緑の不変の歯で (4) を測り、(1)〜(3) は器がどの便でも撃つ検証（gate の段 ①・純移動の機械証明・共通 verify の flip-check）が測る"

[[contract]]
id = "a1"
title = "行の審査の記録の読み手と鍵の口・行を鍵にした祖先の層 — ref の記録と行の記録（§9 の 1 形）を読む口 (A)(D)(E)(F) と、行の digest (B)・code の木の鍵 (C)・判定の鍵と記録の dir の名 (H) の口を pipe の新しい子 module に pub で置き、事前審査の予想の祖先を行の鍵の 1 関数 (G) に載せ替える（§3・§9）"
req = ["FR100", "FR49"]
section = "3"
write-set = ["+crates/scribe2/src/pipe/row_review.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "=crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_record_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_precheck_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_prelens_", "cargo nextest run -p scribe2 --lib --no-tests=fail precheck_intake_label_"]
size = "M"
growth = ["crates/scribe2/src/pipe/row_review.rs:320", "crates/scribe2/src/pipe/mod.rs:2", "crates/scribe2/src/pipe/dispatch/precheck.rs:60", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:330"]
depends = ["a0"]
done = "(1) 口 (A) は、歯が §9 の形で手で書いた ref の記録から、result=pass の記録は pass と記録の merge-base の sha と契約表の file の列を、result=fail の記録は fail を、歯の process の pid と起動時刻の生きた印は pending を、死んだ pid の印と result の行の無い記録は stale を返し、missing を file が無い・schema の違う file・dir で置いた file・result の行も印も無い file の 4 形で返す〔pipe_review_record_ の (a)・8 形〕 (2) 口 (B) は 16 桁の小文字の 16 進を返し、同じ 2 つの字の 2 回は同じ値、契約 file の字か節の本文の字の 1 byte を変えた 2 形はそれぞれ違う値、2 つの字の切れ目だけを動かした形（ab と c・a と bc）も違う値〔(b)〕 (3) 口 (C) は toy repo で、契約表の file だけを変えた commit で鍵が動かず、code の file を 1 つ変えた commit で動き、無い sha は理由を持つ Err〔(c)・3 形〕 (4) 口 (H) は 6 つの材料のどれか 1 つを変えた 6 形でそれぞれ判定の鍵が変わり、同じ材料の 2 回は同じ鍵と同じ dir の名〔(d)・6 対 + 1〕 (5) 口 (D) は、歯が (H) の dir の名で §9 の形に書いた行の記録のうち、PASS・actual・祖先が全部 landed か祖先なしで 4 つの鍵の材料が同じ記録の dir の名だけを返し、FAIL・forecast・祖先に tree を持つ記録と、行の digest・材料の鍵・code の木の鍵・lens の版の 1 つだけが違う 4 形は None〔(e)・返す 2 + 返さない 7〕 (6) 口 (E) は、その行をその digest で載せた ref の記録の他の行を字の順で重複なく返し、同じ行を違う digest で載せた ref の記録の行は返さず、読めない ref の記録の file の数を別に返す〔(f)〕 (7) 口 (F) はその行とその digest の行の記録ごとの判定・basis・理由の型・at を返し、読めない記録は数だけを返す〔(g)〕 (8) 口 (A)〜(F) と (H) は pub で e2e の歯から直に呼べ、口 (G) は事前審査の子 module の pub(in crate::pipe) の関数で、事前審査の予想（bead を鍵にした 1 本）は (G) の上に載り、予想の読み手を 2 本にしない（構造）〔変わらない既存の歯 pipe_dispatch_precheck_・precheck_intake_label_・pipe_prelens_〕 base は子 module が無く (a)〜(g) が compile で落ちるので RED"

[[contract]]
id = "a"
title = "行の審査の口 pipe review --ref — 設計の PR の head の commit と merge-base の間で digest の変わった行ごとに、受付と同じ機械の検査と、審査の木を cwd にした読みの道具の lens を撃ち、判定の鍵ごとの行の記録と head ごとの ref の記録（結果 4 語・merge-base・契約表の file の列）を §9 の形で state dir に書いて stdout に返す。記録の読み・鍵・祖先の層は行 a1 の口を、木の腕は行 a0 の共用の子 module を呼ぶ（§3・§9）"
req = ["FR100", "FR49", "FR48"]
section = "3"
touches = ["crate::pipe::cli::PipeCommand"]
write-set = ["+crates/scribe2/src/pipe/review_ref.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_ref_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_reuse_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/pipe/review_ref.rs:455", "crates/scribe2/src/pipe/mod.rs:2", "crates/scribe2/src/pipe/review.rs:12", "crates/scribe2/src/pipe/cli.rs:8", "crates/scribe2/src/pipe/cli/args.rs:4", "crates/scribe2/src/help.rs:3", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:480"]
depends = ["h", "a1"]
done = "(1) 口 pipe review は PipeCommand の 19 語目 review で、args が許す flag は --ref・--repo・--state-dir・--lens・--rules の 5 つで、usage と help の pipe の頁と外形の snapshot に載る〔直す既存の歯 pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none（18 → 19 語）・snapshot pipe_external_form・pipe_review_ref_ の (a) の help の頁の 1 行〕 (2) toy repo の main と設計の PR の commit で、契約の欄を変えた行・節の本文だけを変えた行・新しい行は撃たれ、変わらない行と指す bead が全部 closed の行は撃たれず、bead の無い行は撃たれる（偽 lens の呼び出しの記録から行 id の集合と母集団の行数を出す）〔(b)〕 (3) 変わった行が 0 本の sha は偽 lens 0 回で rc 0・ref の記録は row の行を持たず result=pass〔(c)〕 (4) 偽 bd が rc 1 の周は偽 lens 0 回・rc 2 で台帳を読めない理由を名指し、ref の記録に result の行を書かない〔(d)〕 (5) 同じ doc の depends の未着地の祖先（bead は open・便なし）を持つ行は basis forecast で、材料の dir に祖先の材料の file が在って祖先の行の TOML の写しと祖先の節の本文を持ち、祖先の write-set の + の file は審査の木に無く、Gated PASS の便を持つ祖先の add の file は審査の木に写って basis actual、bead の無い行は partial〔(e)・3 形〕 (6) 受付の歯と同じ fixture で確定の断りを 1 つ持つ行は偽 lens 0 回で verdict FAIL・mech が firm:<断りの名>・kind が -〔(f)〕 (7) 同じ sha の 2 回目の口は偽 lens 0 回で同じ記録の dir を名指し、要件の本文を 1 行変えた commit・code の file を 1 つ変えた commit・偽 lens の版の行を変えた回の 2 回目はそれぞれ撃つ（撃たない 1 + 撃つ 3）〔(g)〕 (8) 理由の型が unparsed の記録を持つ行は同じ鍵でも撃ち直される〔(h)〕 (9) 全行 PASS の sha と、forecast と partial の unparsed でない INCONCLUSIVE だけを持つ sha は pass（rc 0）、actual の INCONCLUSIVE・FAIL・unparsed の行を 1 本持つ sha は fail（rc 1）〔(i)・5 形〕 (10) 同じ sha に生きた撃ち中の印（歯の process の pid と起動時刻）を置いた周の口は偽 lens 0 回で result=pending の 1 行と rc 1 で待たずに終わり、死んだ pid の印と result の行の無い ref の記録を置いた周の口は印を外して撃ち直す〔(j)〕 (11) 偽 lens は審査の木の中で起き（偽 lens が記録する cwd の HEAD が --ref の sha）、撃たれている間に受付札の置き場に run が row-review-<ref の 12 桁>-<行 id> の札が 1 枚在り、撃ち終えた後に審査の木の worktree の登録が残らない〔(k)〕 (12) 口の stdout は行ごとの [ROW-REVIEW] row= の行と最後の [ROW-REVIEW] result= の行で、置き場の送達の記録（notify の行）は 0 行（席の入力欄へ差し込まない）〔(b) と (i)〕 (13) 行の記録は 1 行目 schema=1 と §9 の 14 key を持って dir の名が行 a1 の口 (H) の返す名で、ref の記録は schema・base（行の審査の merge-base の sha）・tables（判定した契約表の file の列）・row の行（digest の欄を含む）・result を持つ〔(l)・書いた file を読んで key の列と merge-base と file の列を比べる〕 (14) 口が書いた記録を行 a1 の口が読む: 全行 PASS の sha で口 (A) が pass と記録の merge-base の sha と契約表の file の列を返し、FAIL の行を持つ sha で fail を返し、PASS・actual の行で口 (D) がその記録の dir の名を返し、2 行を撃った sha で口 (E) が互いの行を返し、口 (F) が撃った行の記録の判定と basis を返す。口 (B) に便の置き場の写し（契約 file と design.txt から末尾の改行を 1 つ除いた字）を渡した値は記録の digest と同じ〔(m)・書き手と読み手の形の一致〕 (15) 版の flag で rc 1 を返す偽 lens の行は lens を撃たずに INCONCLUSIVE・unparsed〔(n)〕 (16) Reviewed の判定の読みの 2 本は review.rs の pub(in crate::pipe) の口 1 つで通り、Reviewed の段と先撃ちの使い回しの挙動は変わらない〔変わらない既存の歯 pipe_review_reuse_〕 (17) PipeCommand を名指す file は cli.rs・args.rs・e2e の pipe.rs のまま増やさない〔verify の最後の行の契約表の検査〕 (18) 行の digest・code の木の鍵・判定の鍵と記録の dir の名・祖先の層・木の腕は行 a1 の口 (B)(C)(G)(H) と行 a0 の共用の子 module を呼び、口の中に 2 本目を書かない（構造） base は subcommand が無いので (a)〜(n) が RED（(m) は口を撃つ所で落ちる）・snapshot と 19 語の pin は字と数が違うので RED"

[[contract]]
id = "b"
title = "merge の門の 2 つ目の判定 — anchor の HEAD か local の PR の head の commit の宣言が任意 key row-review を true で持つ repo で、head を 40 桁で固定しない・行の審査の結果が無い・読めない・pass でない・契約表が動いた merge を判定の順の 6 語で断り、在るのに読めない宣言はどの repo でも、local に無い head は anchor の宣言が key を true で持つ repo でだけ断る。help の merge は merge と読まない（§4）"
req = ["FR101", "FR92"]
section = "4"
write-set = ["crates/scribe2/src/hook/merge_gate.rs", "crates/scribe2/src/hook/anchor_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_merge_gate_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_anchor_guard_", "cargo nextest run -p scribe2 --lib --no-tests=fail hook_merge_trailer_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema"]
size = "M"
growth = ["crates/scribe2/src/hook/merge_gate.rs:110", "crates/scribe2/src/hook/anchor_guard.rs:4", "crates/scribe2/src/hook/mod.rs:2", "crates/scribe2/src/pipe/declaration.rs:6", "crates/scribe2/src/pipe/declaration/optional_keys.rs:30", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:340"]
depends = ["a"]
done = "(1) 任意 key row-review は真偽だけの key で、宣言の key の列の末尾（run の base の列の後ろ）に足し、書かない宣言は false と同じ、真偽でない値は key と行番号を名指す不備〔直す既存の歯 declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema・(g) の不備の宣言〕 (2) key を true で持つ anchor の repo で、trailer の良い本文の merge が 6 形でそれぞれ断られる: --match-head-commit の無い command と短い sha の command は no-head-pin、記録の無い sha は row-review-missing、生きた撃ち中の印は row-review-pending、死んだ印で result の行の無い記録は row-review-stale、result=fail は row-review-failed、result=pass で記録の merge-base と origin/main の間で契約表の file を変えた repo は row-review-moved〔hook_merge_gate_row_review_ の (a)〕 (3) 各断りは rc 2・stdout 0 byte・stderr 1 行で <NAME>: deny merge-gate reason=<語> で始まり、6 語の断りは pipe review --ref <sha> の argv を名指し、hook の記録に merge-deny <語> が 1 行増える〔(a)〕 (4) result=fail で契約表も動いた記録は row-review-failed を名指す（判定の順）〔(b)〕 (5) schema の違う記録と dir で置いた記録（結果の file が読めない 2 形）は row-review-missing を名指す〔(c)〕 (6) result=pass で契約表の動かない記録の sha を 40 桁で固定した command は rc 0・0 byte・記録 0 行〔(d)〕 (7) anchor の宣言が key を持たず local に在る PR の head の commit の宣言だけが true の形と、anchor だけが true で PR の head の宣言が key を外した形の両方で門が掛かる（どちらも記録の無い sha で row-review-missing）〔(e)・2 形〕 (8) key の無い宣言の repo と値が false の宣言の repo の 2 形で、(2) の 6 形の fixture の command が trailer の判定だけで決まる（trailer の良い command は通り、本文の無い command は no-body）〔(f)・2 形 × 6 形・変わらない既存の歯 hook_merge_gate_ の 4 本〕 (9) key を持たない repo でも、anchor の HEAD の宣言が在るのに読めない形と local に在る PR の head の commit の宣言が読めない形は row-review-missing で断られ、1 行は読めない commit と不備の 1 つ目を名指す（記録の語が merge-deny row-review-missing で、ほかの門の断りでないことを確かめる）〔(g)・2 形〕 (10) 40 桁の sha が local に無い merge は、anchor の宣言が key を true で持つ repo では row-review-missing で断られて 1 行がその sha を名指し、key を持たない anchor の repo では trailer の判定だけで通る〔(h)・2 形〕 (11) (ii) と (iii) の断りの 1 行は読めない理由を名指す: (9) の 2 形の 1 行は句「宣言が読めない」を持って句「head の commit が local に無い」を持たず、(10) の断りの 1 行は句「head の commit が local に無い」を持って句「宣言が読めない」を持たない〔(j)・3 形〕 (12) (ii) と (iii) の断りの 1 行は次に撃つ口 git fetch origin を名指し、(2) の 6 形の断りの 1 行は git fetch origin を持たずに pipe review --ref <sha> の argv を名指す〔(k)・3 形 + 6 形〕 (13) --help を持つ merge の command と -h を持つ command は key を true で持つ repo で rc 0・0 byte〔(i)・2 形〕 (14) 見分けは anchor の門と共用の 1 関数のままで、anchor の門の窓の既存の判定は変わらない〔変わらない既存の歯 hook_anchor_guard_〕 (15) 5 語の後ろに 6 語を宣言順（no-head-pin・row-review-missing・row-review-pending・row-review-stale・row-review-failed・row-review-moved）で足し、trailer の判定の私有の関数の署名と既存の歯は変えない〔直す既存の歯 hook_merge_trailer_deny_line_names_the_gap_and_the_next_step の語の列・変わらない hook_merge_trailer_ の残り 4 本〕 (16) 門は台帳も event log も読まない（偽 bd を PATH に置かず events の file を持たない置き場で (a) が通る）〔(a)〕 (17) ref の記録の読みは行 a1 の口 (A) を呼び、門に 2 本目の読み手を書かない（構造） base は 6 語が無く key の fixture が trailer の判定だけで通り、help の command は no-body で断られるので (a)〜(e)(g)(i)(j)(k) と (h) の 1 形目が RED・語の列の pin と宣言の key の列の pin は数が違うので RED"

[[contract]]
id = "c"
title = "Reviewed の段の使い回し — 行の審査の記録のうち basis が actual・祖先が全部着地・判定 PASS で、行の digest・材料の鍵・code の木の鍵・lens の版が便と同じものが在れば、lens を撃たずに PASS を写して段の detail の末尾に row-review:reused を付ける（先撃ちの読み口は行 e まで後ろに残す・§5）"
req = ["FR49", "FR100"]
section = "5"
write-set = ["crates/scribe2/src/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_row_reuse_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_reuse_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_ref_"]
size = "M"
growth = ["crates/scribe2/src/pipe/review.rs:45", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:280"]
depends = ["a"]
done = "(1) toy repo の設計の PR の commit で行の審査（偽 lens は PASS）を撃ち、同じ commit を main にして intake した便は、Reviewed の段で偽 lens 0 回・verdict PASS・detail の末尾が row-review:reused・review.json は記録の判定を写す〔pipe_review_row_reuse_ の (a)〕 (2) 行の審査の後に要件の本文を 1 行変えた main・code の file を 1 つ変えた main・偽 lens の版の行を変えた便・basis が forecast の記録だけを持つ行・partial の記録だけを持つ行の 5 形は、それぞれ偽 lens が 1 回撃たれる〔(b)・5 形〕 (3) FAIL の記録と、祖先が tree の actual の PASS の記録は写さない〔(c)・2 形〕 (4) 版の flag で rc 1 を返す lens の cmd の便は写さずに撃ち、判定は撃った lens の値〔(d)〕 (5) 写した周は審査の消費の記録を書かず（cost の行 0 行）、撃った周は 1 行〔(a) と (b)〕 (6) read_detail は row-review:reused の付いた detail を PASS と読み、report の段の字は写さない周と同じ〔(a)〕 (7) 行の審査の読み口は先撃ちの読み口より前に在り、先撃ちの使い回しの挙動は変わらない〔変わらない既存の歯 pipe_review_reuse_〕 (8) 鍵の 4 つは行 a1 の口 (B)(C)(D) と共用の digest と行 h の flag で求め、Reviewed に 2 本目の鍵の読み手を書かない〔変わらない既存の歯 pipe_review_ref_〕 base は読み口が無いので (a) が偽 lens 1 回で RED・(c)(d) は写しが起きない base でも緑の対照"

[[contract]]
id = "d"
title = "先撃ちの退役の段 1 — rules 行 pipe.precheck_lens_per_round を値 0（裁定 user 2026-09-30T22:13Z 項 precheck）にし、値 0 の周は先撃ちの材料の組み直しも一時の worktree も作らず、本 repo の vessel 宣言に row-review = true を足して merge の門の 2 つ目の判定を本 repo に掛ける（§6）"
req = ["FR100", "FR101"]
section = "6"
write-set = ["rules/manifest.toml", ".vessel.toml", "crates/scribe2/src/pipe/dispatch/prelens.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "=crates/scribe2-boundary/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_prelens_off_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_prelens_row_follows_the_lens_count", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_merge_gate_self_declaration_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_accepts_self_hosted_declaration_under_embedded_ceiling", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_prelens_limit_counts_the_flying_lens_and_zero_fires_nothing"]
size = "S"
growth = ["crates/scribe2/src/pipe/dispatch/prelens.rs:6", "crates/scribe2-boundary/tests/e2e/pipe/review.rs:70", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:30"]
depends = ["b", "c"]
done = "(1) 埋め込みの manifest の pipe.precheck_lens_per_round は値 0・裁定 id user 2026-09-30T22:13Z 項 precheck・裁定日 2026-09-30 で、行の位置と kind と形は変わらない〔直す既存の歯 rules_prelens_row_follows_the_lens_count の値と裁定の pin〕 (2) 値 0 の rules の写しの周の dispatch の 1 周は、clean の依存待ちの行が在っても先撃ちの置き場に一時の worktree も材料の dir も key も作らず worktree の登録も増えず、値 1 の同じ fixture の周は作る〔pipe_prelens_off_ の (a)・対照つき〕 (3) 値 0 の周も、母集団を出た bead の置き場と前の周の木は今どおり外す〔pipe_prelens_off_ の (b)〕 (4) 値 0 の周に 1 本も起こさない既存の挙動は変わらない〔変わらない既存の歯 pipe_prelens_limit_counts_the_flying_lens_and_zero_fires_nothing〕 (5) 本 repo の .vessel.toml は row-review = true を持ち、その宣言を写した tmp の anchor で、trailer の良い本文を渡す head を固定しない merge が no-head-pin で断られる〔hook_merge_gate_self_declaration_ の (a)〕 (6) 自己ホストの宣言を写した toy repo の受付は通る〔変わらない既存の歯 pipe_intake_accepts_self_hosted_declaration_under_embedded_ceiling〕 base は値 1 で、値 0 の周も材料を組み、本 repo の宣言が key を持たないので (1)(2)(5) が RED"

[[contract]]
id = "e"
title = "先撃ちの code の退役（§6 段 2）— 先撃ちの子 module・lens の --stage prelens・rules 行 pipe.precheck_lens_per_round と pipe.precheck_lens_model と kind 2 つ・Review の先撃ちの model の欄と Reviewed の先撃ちの使い回し・[DISPATCH-PRECHECK] の prelens= の字・先撃ちの歯を外す（事前審査の予想と束と行の審査の使い回しは残す）"
req = ["FR49", "FR100"]
section = "6"
touches = ["crate::rules::RuleKind"]
write-set = ["~crates/scribe2/src/pipe/dispatch/prelens.rs", "-crates/scribe2/src/pipe/dispatch.rs", "-crates/scribe2/src/pipe/dispatch/precheck.rs", "-crates/scribe2/src/pipe/review.rs", "-crates/scribe2/src/pipe/cli/step.rs", "-crates/scribe2/src/headless/lens.rs", "-crates/scribe2/src/headless/mod.rs", "-crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/help.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_stage_retired_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_prelens_retired_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_precheck_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_row_reuse_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_ref_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail model_split_embedded_rows_carry_the_three_models_in_declaration_order", "cargo nextest run -p scribe2 --lib --no-tests=fail precheck_intake_label_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/help.rs:1"]
depends = ["d"]
done = "(1) lens --stage prelens は未知の値と同じ rc と 1 行で断られ、--stage memo と --stage 無しは今どおり受ける〔headless_lens_stage_retired_ の (a)〕 (2) rules get pipe.precheck_lens_per_round と rules get pipe.precheck_lens_model は無い id の断りで rc が 0 でなく、kind の字 PipePrecheckLensPerRound と PipePrecheckLensModel は RuleKind の字面から引けない〔rules_prelens_retired_ の (a)〕 (3) 埋め込み manifest の行数と kind の数が run の base の値から 2 ずつ減り、model の行の kind の宣言順は RunnerModel → RunnerEffort → LensModel → RoleModel〔直す既存の歯 rules_embedded_manifest_is_valid_and_covers_all_kinds・rules_embedded_manifest_declares_one_capability_row_per_role・model_split_embedded_rows_carry_the_three_models_in_declaration_order と rules_external_form の snapshot〕 (4) clean の依存待ちの行を持つ dispatch の 1 周は事前審査の置き場の下に lens の dir を作らず、dispatch ls の [DISPATCH-PRECHECK] の行は base= の値で終わって prelens= の字を持たない〔直す既存の歯 pipe_dispatch_precheck_declared_new_file_makes_the_waiting_row_clean と変わらない pipe_dispatch_precheck_ の残り 4 本〕 (5) Reviewed の段は先撃ちの置き場を読まず、行の審査の使い回しと行の審査の口の挙動は変わらない〔変わらない既存の歯 pipe_review_row_reuse_・pipe_review_ref_〕 (6) lens の usage と help の lens の頁の --stage の値は memo だけ〔snapshot headless_external_form を作り直す・headless_lens_ の残り〕 (7) 先撃ちの子 module は file ごと消え（~）、共用の子 module は行 a0 の形のまま残り、- の file は先撃ちの部分だけが減る（- は増分 0 以下の宣言で削除ではない） (8) 先撃ちを測る歯（pipe_prelens_・pipe_review_reuse_・行 d の pipe_prelens_off_・headless の --stage prelens の歯と rules_prelens_row_follows_the_lens_count）と、それだけが使う helper は消え、使われない helper を残さない（clippy の -D warnings） base は --stage prelens を受け、rules の 2 行が在り、[DISPATCH-PRECHECK] の行が prelens= を持つので (a) と (3)(4) の pin と snapshot が RED"

[[contract]]
id = "f"
title = "行の予約 — 落ちた契約 B の直前の便の契約の写しの write-set を B の行の予約とし、順序で B より後ろの交差する候補を reserved で待たせ（B の blocks の祖先は待たせない）、B の新しい便・close・memo か問いの label・hold・期限（rules 行 pipe.reserve_h・24 時間・裁定 user 2026-09-30T22:13Z 項 reserve）で解く。記帳せず周ごとに導く（§7）"
req = ["FR102", "FR68", "FR39"]
section = "7"
touches = ["crate::rules::RuleKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch/reserve.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/case/mod.rs", "crates/scribe2/src/rules/mod.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_row_reservation_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_reserve_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_dispatch_wait_reasons_render_the_name_and_the_value", "cargo nextest run -p scribe2 --lib --no-tests=fail phase_table_queued_reasons_cover_the_wait_reasons", "cargo nextest run -p scribe2 --lib --no-tests=fail phase_table_turn_of_matches_every_row_of_the_table", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/dispatch/reserve.rs:200", "crates/scribe2/src/pipe/dispatch.rs:14", "crates/scribe2/src/pipe/dispatch/candidates.rs:25", "crates/scribe2/src/case/mod.rs:3", "crates/scribe2/src/rules/mod.rs:6", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs:400"]
done = "(1) 埋め込み manifest に rules 行 pipe.reserve_h（kind PipeReserveH・Int・値 24・enabled・裁定 id user 2026-09-30T22:13Z 項 reserve・裁定日 2026-09-30）が floor.timeout_s の行の直後に 1 本在り、kind は ALL の FloorTimeoutS の直後（SeatDraftsCapMb の前）で字面から引け、形は Int だけ〔rules_reserve_ の (a)・直す既存の歯 rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all と行数・kind の数の 2 本と rules_external_form の snapshot〕 (2) 偽 bd と偽の終端の便で、落ちた B（P1・直前の便が Reviewed FAIL）と同じ file を触る後ろの C（P2）が B の終端の後の周に reason=reserved:<B>/<交差した file の本数> で待ち、B より前の D（P0）は同じ file を触っても待たずに起こされる〔pipe_dispatch_row_reservation_ の (a)〕 (3) 終端 4 形（Reviewed の判定が PASS でない・Gated FAIL・Failed・Stopped）の B はどれも予約を持ち、直前の便が Landed の B は持たない〔(b)・5 形〕 (4) B の契約を変えた周に B が C より先に起こされ（起こした記録の順）、B の新しい便の RunCreated の後の周に C の reserved が解ける〔(c)〕 (5) B への hold の印・期限（値 1 の写しで 2 時間前の終端）・B の close・B の memo の label・B の問いの label のそれぞれで C の reserved が解ける〔(d)・5 形〕 (6) B が C を台帳の blocks で持つ周（C は B の祖先）に C は reserved で待たずに起こされる〔(e)〕 (7) 期限の行の無い rules の写しは期限なしで予約を掛けて reason の値の末尾が /unset になり、値 0 の写しは 100 時間前の終端でも予約を掛ける〔(f)・2 形〕 (8) B が列の候補に居ない周（B が依存で待つ）も C は reserved で待つ〔(g)〕 (9) 行の予約の判定は event log の行数を増やさない（各周の前後で events の行数が同じ・周の数と待った候補の数を出す）〔(a)〜(g)〕 (10) 名 reserved は WAIT_REASONS の floor の後ろで、描きは reserved:<bead>/<本数> と末尾 /unset の 2 形、局面の表 QUEUED_TURNS は reserved を手番 seat で持つ〔直す既存の歯 pipe_dispatch_wait_reasons_render_the_name_and_the_value・phase_table_queued_reasons_cover_the_wait_reasons・phase_table_turn_of_matches_every_row_of_the_table〕 (11) 値の型と導きは dispatch の新しい子 module に置き、dispatch.rs に足すのは子 module の宣言・variant（値の型の tuple 1 つ）・名・描き・列の周の 1 回の導きの呼び出しと既存の歯の列の追記だけ（dispatch.rs の余地 23 行の中の growth 14） base は reserved が無く C が起こされ、名と kind と rules 行が無いので (a)〜(g) と 3 本の lib の歯と rules の歯が RED（lib の歯は variant が無く compile で落ちる）"

[[contract]]
id = "g"
title = "兄弟の待ち — 直前の便が設計の側の終端（Reviewed の FAIL か unparsed でない INCONCLUSIVE・Gated の FAIL）に着いた B と同じ節の行と、B の行の digest を載せた行の審査の ref の記録に載る行を、first の印と B の blocks の祖先を除いて sibling で待たせ、B の digest の変化・release か hold・兄弟自身の後の PASS か予想の INCONCLUSIVE の記録・close・期限で解く（§8）"
req = ["FR103", "FR68", "FR100"]
section = "8"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/case/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_sibling_wait_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_row_reservation_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_dispatch_wait_reasons_render_the_name_and_the_value", "cargo nextest run -p scribe2 --lib --no-tests=fail phase_table_queued_reasons_cover_the_wait_reasons", "cargo nextest run -p scribe2 --lib --no-tests=fail phase_table_turn_of_matches_every_row_of_the_table"]
size = "M"
growth = ["crates/scribe2/src/pipe/dispatch.rs:9", "crates/scribe2/src/pipe/dispatch/candidates.rs:150", "crates/scribe2/src/case/mod.rs:2", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs:380"]
depends = ["a", "f"]
done = "(1) 偽 bd と偽の終端の便と行の審査の記録の fixture で、直前の便が Reviewed FAIL の B と同じ section を実装する行の候補が reason=sibling:<B> で待つ〔pipe_dispatch_sibling_wait_ の (a)〕 (2) B の直前の便の写しから求めた B の digest で B の行を載せた ref の記録に載る別の section の行も sibling で待ち、ref の記録は push 2 回の PR の 2 file の形と直しの PR の 1 file の形の 2 形〔(b)・2 形〕 (3) first の印を持つ候補と、B が blocks で待つ同じ section の前提の候補は待たずに起こされる〔(c)・2 形〕 (4) 直前の便が Failed・Stopped・unparsed の INCONCLUSIVE の B は兄弟を待たせず、Gated FAIL と unparsed でない INCONCLUSIVE の B は待たせる〔(d)・5 形〕 (5) B の今の digest が写しと違う周（直しを main に入れた）・兄弟自身の今の digest の PASS の記録が B の終端の後に書かれた周・B への hold の印・release の印・B の close・期限（値 1 の写しで 2 時間前の終端）・兄弟自身の forecast の unparsed でない INCONCLUSIVE の記録が B の終端の後に書かれた周の 7 形で解け、兄弟自身の FAIL の記録が B の終端の後に在っても解けず、B の終端より前の PASS の記録でも解けない〔(e)・解け 7 形と解けない 2 形〕 (6) 兄弟で依存待ちの候補は dependency のまま（sibling は settled の後で、床と未反映の裁定の上書きの前）〔(f)〕 (7) 兄弟の待ちの判定は event log の行数を増やさない〔(a)〜(e) の各周〕 (8) 名 sibling は WAIT_REASONS の reserved の後ろで、描きは sibling:<bead>、局面の表 QUEUED_TURNS は sibling を手番 seat で持つ〔直す既存の歯 pipe_dispatch_wait_reasons_render_the_name_and_the_value・phase_table_queued_reasons_cover_the_wait_reasons・phase_table_turn_of_matches_every_row_of_the_table〕 (9) 兄弟と記録の読みは行 a1 の口 (B)(E)(F) を呼んで ref の記録と行の記録の 2 本目の読み手を書かず、導きは candidates.rs に足し（行 f の子 module は触らない）、dispatch.rs に足すのは variant（bead id の tuple）・名・描きと既存の歯の列の追記だけ（growth 9） (10) 行の予約の挙動は変わらない〔変わらない既存の歯 pipe_dispatch_row_reservation_〕 base は sibling が無く兄弟が起こされ、名が無いので (a)(b)(d)(e) と 3 本の lib の歯が RED（lib の歯は variant が無く compile で落ちる）"
<!-- contracts:end -->
