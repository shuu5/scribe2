# 設計: 行の審査 — 契約表の行を merge の前に審査し、落ちた行の順番を起動の列が守る

- 出所: 持ち主の問い 2026-09-30T11:3xZ〜11:5xZ（逐語は台帳 s2-07l.736.31 の notes）→ epic s2-07l.736.33 → [ADR-0103](../../design-intent/decisions/ADR-0103-contract-rows-pass-row-review-before-merge-and-failed-rows-keep-their-place.html)。材料は 2026-09-28〜09-30 の便 165 本の FAIL / INCONCLUSIVE 76 件の分析（host の file・tracked でない）。推奨の採用は常設の裁定 user 2026-09-28T00:54Z（決めてほしいことは推奨で進める）の適用で、rules 行の値 2 つ（§7 の期限・§6 の値 0）と行の審査の消費（§10）は SRS の追加 round で別に裁定を取る。
- 要件（今の字）: [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR68](../../design-intent/spec/srs.html#FR68) 起動の列 / [FR92](../../design-intent/spec/srs.html#FR92) merge の門 / [FR39](../../design-intent/spec/srs.html#FR39) 交差の排他 / [FR44](../../design-intent/spec/srs.html#FR44) 席の入力欄への差し込み（変えない・§3 形 6）。FR49・FR68・FR92 の字を直す SRS の追加 round が先に要る（§11）。
- 前提: 審査役に読みだけの道具を渡し、作業場所を審査の木（HEAD の commit を detach した一時の worktree）にする設計（同じ epic の別の設計の PR・未着地・以下「打ち手 0」）。行の審査の lens はその形で撃つ（§11 の行 a はその行の後）。
- この設計から出る契約: §11 の 7 行。契約表の行は SRS の round の後に足す（FR49 と FR68 の意味が変わるので、今の要件 id を指す行にできない）。

## 1. 何を解くか

契約の審査（FR49）は便の中に在る: 設計の PR を merge → 行が起動の列に入る → 起こされた便の Reviewed の段で審査 → runner → gate。設計の欠陥は便の中の審査か gate で初めて見つかる。

- 2026-09-28〜09-30 の FAIL / INCONCLUSIVE 76 件（Reviewed 46・gate 30）のうち 73 件（96%）は設計の側が根だった。runner が主因は 2 件、環境 1 件、審査役の誤りは 0 件。
- 審査で落ちた行は列の外へ出る（同じ契約の中身で終端に着いた便の行は `Settled`）。直しの PR が merge されるまで、その行の write-set は誰のものでもなくなる。同じ file を触る後ろの行が先に起き、直った行はその行の終わりを待つ側に回る（行 a1 が 2 回落ちる間に同じ P2 の行 ak が起き、直った a1 は ak の終わり待ち）。
- 同じ欠陥を持つ兄弟の行を止める口も無い（行 b1 は a1 が同じ指示で落ちた 4 秒後に起こされ、同じ理由で落ちた。直しは 53 分後）。
- 先撃ち（[dispatcher.md](./dispatcher.md) §27 形 aa）は審査の層になっていない。Reviewed の FAIL 46 件のうち事前に撃たれていたのは 9 件で、そのうち 6 件は先撃ちが FAIL / INCONCLUSIVE を出したまま起動した。使い回し（形 ac）は全期間で 0 件。

やさしく言うと: 設計の穴は「列に並んだ後」に見つかるので、落ちた行は列から外れ、後ろの行に file を取られる。審査を「列に並ぶ前」（設計の PR を main に入れる前）へ移し、それでも落ちた行は自分の触る file を取り置いて順番を守り、同じ設計から出た兄弟の行は直しが入るまで待たせる。

## 2. 何が起きているか（main eb377622・verified・名は行 e の前の現物）

- **審査の時点**: `crates/scribe2/src/pipe/review.rs` の `review`（:306）が便の置き場の材料の dir に `stage`（:343）で材料を組み、`decide`（:535）が lens を撃つ。lens の cwd は `entry.repo`（anchor の repo）で、箱の unit 名は便の id から作る（:546）。判定は `read_outcome` → `tip` の後に `narrow`（:410）を通す。先撃ちの判定の使い回しは `crates/scribe2/src/pipe/dispatch/prelens.rs` の `reusable`（:317）で、同じ model に解ける周だけ引く（`Review` の組み立ては `crates/scribe2/src/pipe/cli/step.rs` の `review_run`・:237）。
- **使い回しが当たらない理由**（先撃ちの分析・host の file の §9・verified）: 使い回しの口が在った窓の Reviewed 61 回で使い回しは 0 回。
  - 先撃ちの材料には予想の印の行（「予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた」）が必ず入る（114 本中 114 本・Reviewed は 163 本中 0 本）。
  - 宣言だけの祖先は、着地で変える既存の file の中身を予想に写さず、新しい file を空で置く。祖先が着地すると base の要約と外の材料が変わる（例: write-set の 1 file が 521 行 → 899 行）。
  - lens の cmd の字に binary の置き場の path が入り、置き場は 56 時間で 18 回変わった。
  - 印を外して材料の 5 区画が全部同じだったのは 20 回のうち 1 回。使い回すと、先撃ちの見逃し（同じ版の PASS 11 回中 2 回を本番の審査が FAIL にした）がそのまま素通りする。
- **先撃ちの木**: 一時の worktree の実体化・材料の組み・片付け・材料の鍵は prelens.rs の私有の `materialize`（:227）・`build`（:205）・`drop_tree`（:219）・`prune`（:148）・`digest`（:295）が持つ。事前審査の予想（`crates/scribe2/src/pipe/dispatch/precheck.rs` の `overlay`・:208）は材料の上に重ねるだけで木には当てない。予想の 1 本（`population`・:51 と `resolve`・:146）は台帳の bead を母集団と鍵に持つ。先撃ちの `round`（prelens.rs の :104）は、上限の値に依らず材料の組み直し（`rebuild`）を毎周撃つ。
- **列の順と終端の行**: `crates/scribe2/src/pipe/dispatch/candidates.rs` の `entry_of`（:43）は依存 → hold → 起こした印 → 設計 pointer → 契約の生成 → `settled`（:240）の順に理由を付ける。`settled` は直前の便が終端に着き契約 file が同じ行を `Settled` にする（Reviewed の終端だけは設計の節の本文の写しも鍵に入る・`section_keyed` :274）。`settle`（:87）は `order`（`crates/scribe2/src/pipe/dispatch.rs` の :818）の順に候補を見て、`blocker`（:127）が live な便と同じ周に起こした便の write-set とだけ交差を測る。`Settled` の行の write-set はどこにも数えない。
- **待ちの理由**: `WaitReason`（dispatch.rs の :99）は閉じた型で、名の列は `WAIT_REASONS`（:94）。open の行 aj（[dispatcher.md](./dispatcher.md) §35）と am（同 §38）が、この列の末尾に `unreflected-ruling` と `floor` を足す予定で、床の検査の不合格は launched と settled 以外の候補の理由を上書きする（同 §35 形 2・未着地）。
- **merge の門**: `crates/scribe2/src/hook/merge_gate.rs` の `decide`（:245）は本文の file だけを読み、断りの語は宣言順の 5 語（`REASONS` :65）。hook の入口では anchor の門の直後に撃たれる（`crates/scribe2/src/hook/mod.rs` の :693）。器が着地させる便は git を子 process で撃ち、この門を通らない。見分けの `is_pr_merge`（`crates/scribe2/src/hook/anchor_guard.rs` の :92）は `-` で始まる語を読み飛ばすので、`gh pr merge --help` も merge と読んで断る（この host で再現 2026-09-30）。gh 2.45.0 の `pr merge` は `--match-head-commit SHA`（PR の head がその sha でなければ merge しない）を持つ（この host の help で実測）。
- **vessel 宣言の読み**: 任意 key の読み手は全部 git で HEAD か sha の commit の宣言を読み、作業ツリーの宣言は読まない（`crates/scribe2/src/pipe/declaration.rs` の `head_declaration`・:375）。任意 key は `crates/scribe2/src/pipe/declaration/optional_keys.rs` の閉じた列が持ち（`close-check` は真偽だけの key・`floor_check_at` は名指した sha の宣言を読む）、key を足した便は親の declaration.rs の `Declared` の欄と `parse` も触った（98285c92）。`floor_check_at` は宣言を読めない commit を「宣言が無い」と読む。
- **先撃ちの rules 行**: `pipe.precheck_lens_per_round`（値 1・値 0 は lens を撃たない）と `pipe.precheck_lens_model`（sonnet）。契約の審査と gate の審査の model は `lens.model`（opus）。lens の子は cap・model・effort を自分の `--rules` から読む（`crates/scribe2/src/headless/lens.rs` の `rows_of`・:164）。
- **受付札と口座**: 受付札の本文は `schema` / `pid` / `run` / `jobs` / `ts` の 1 行で、読み手は pid と起動時刻で生死を判じ、run の字で判じない（`crates/scribe2/src/pipe/admission.rs`）。gate の lens は口座を `Pool::declared`（step.rs の :307）で選び、Reviewed の lens は親の環境を継ぐ。

## 3. 行の審査（設計の PR の head で、変わった行ごとに撃つ）

- 口: `<NAME> pipe review --ref <sha> --repo R --state-dir S --lens CMD [--rules PATH]`。orchestrator の席が設計の PR を push した後に撃つ。merge の門の断り（§4）が同じ argv を名指すので、撃ち忘れは merge の時点で見える。
- 形（番号は §11 の行 a の done と 1:1 にする予定）:
  1. **変わった行**: `--ref` の木と、その commit と `origin/main` の先端の merge-base の木の両方で、契約表を持つ file（受付と同じ表の読み手が行を返す file）の行を読み、行の digest（形 2）が merge-base と違う行と merge-base に無い行を「変わった行」とする。台帳を読みだけで読み（`read_ledger`）、その行を指す bead が 1 本以上在って全部 closed の行は外す（着地済み）。bead の無い行は撃つ。変わった行が 0 本なら ref の記録（§9）だけを書き、lens は撃たない。
  2. **行の digest**: 行の欄から受付と同じ生成（`generated`）で作った契約 file の字の hash（`Settled` の鍵と同じ git の object の hash）と、実装する設計の節の本文（審査の材料と同じ読み手 `design_material`）の hash の対。FR49 が契約の中身の同一性に使う 2 つと同じものである。
  3. **祖先の扱い**: 祖先の層は行（`<doc>#<行 id>`）を鍵にした 1 関数で組み、今の事前審査の予想（bead を鍵にした 1 本）はその上に載せ替える（予想の読み手を 2 本にしない・C2）。祖先は、表の depends を推移でたどった行（同じ doc）と、行を指す bead が在ればその台帳の blocks の祖先。祖先ごとの状態の語は、着地（祖先を指す bead が全部 closed）／実物（祖先の便が Gated で判定 PASS）／宣言（それ以外）。審査の basis は次の 3 つ:
     - actual: 行を指す bead が在り、祖先が全部着地か実物。
     - forecast: 宣言の祖先を 1 つ以上持つ。宣言の祖先の `+` の file は空で置かない（先撃ちの予想の形を採らない）。代わりに材料の dir に祖先の材料の file を 1 つ置き、宣言の祖先ごとに、行の TOML の写し（`find_row` が返す行の字をそのまま）と実装する設計の節の本文（`design_material`）を並べる。lens には「この file は未着地の祖先が作る・変える」と材料の形で渡る。
     - partial: 行を指す bead が無く、同じ doc の祖先は全部着地か実物。doc を跨ぐ順は台帳の blocks だけが持つので、bead の無い行の doc を跨ぐ祖先は測れない。doc を跨ぐ未着地の祖先に由来する FAIL を避けたい行は、行の審査の前に bead と blocks を起票する（口はそれを読んで forecast にする）。
  4. **機械の検査**: 変わった行ごとに受付と同じ `generated` → `judge`（置き場なし・`pipe preflight` と同じ 1 本）を撃つ。宣言の祖先を持つ行は形 3 の祖先の層を材料に重ねた予想の上で撃ち、確定と暫定の弁別（[dispatcher.md](./dispatcher.md) §27 形 x の 3）を借りる。確定の finding を持つ行は lens を撃たずに判定 FAIL とし、記録の mech に `firm:<断りの名>`、kind に `-` を置く（理由の型の閉じた 7 語に受付の断りの名を混ぜない）。暫定の finding は lens の材料の末尾に写す。
  5. **lens**: 機械の検査を通った行ごとに、`--ref` の commit を detach した審査の木（打ち手 0 の設計の審査の木と同じ作り方）を置き場の tree の dir に作り、実物の祖先の差分を重ね、Reviewed と同じ組み手（`stage`）で材料を組み、契約の審査と同じ lens（`--stage` なし）を読みだけの道具で撃つ。lens の cwd と `{worktree}` の穴はその審査の木である。箱で包み（子は最後まで待つので作り手の死で殺される形にならない）、unit 名は ref の 12 桁と行 id から作る。口座は gate の lens と同じ `Pool::declared`（FR36 の便用の規則）で選ぶ。同時に起こす本数は受付の host の memory の枠で絞り、受付札の run の欄に row-review の語と ref の 12 桁と行 id を書く。木の実体化・片付け・材料の鍵は、先撃ちの私有の 5 本（§2）を pipe の新しい子 module へ移して先撃ちと共用する（行 a）。落ちた周の残りは、次に口を撃った周の頭に外す。
  6. **前面で最後まで撃つ**: 口は全部の行を撃ち終えるまで前面で走り、行ごとの判定と結果の 1 語を stdout の `[ROW-REVIEW]` の行で返す（席は Bash の background で待ち、完了の知らせを器は送らない）。席の入力欄への差し込みは FR44 の閉じた列の外なので足さない（口を増やさない・C17.2）。
  7. **撃ち直さない**: 同じ行の判定の鍵（§9）の記録が既に在り、判定が unparsed でない行は撃たず、ref の記録にその記録を名指す。判定の鍵は、行の digest・材料の鍵（材料の dir の全 file。base の要約・要件の本文・外の材料・祖先の材料の file を含む）・code の木の鍵・basis・祖先ごとの状態の語・lens の版の全部で、どれか 1 つが違えば撃つ。PR の直しの commit で、行の中身も材料も code の木も変わらなかった行の審査は 1 回で済む。
  8. **撃ち中の印と重ねての撃ち**: 口は ref の dir に撃ち中の印（`<pid> <起動時刻>`・`lock_owner` で生死を判じる）を置き、同じ sha に 2 本目の口を撃つと、印の持ち主が生きていれば撃たずに待ち、死んでいれば印を外して撃ち直す。
  9. **ref の結果**: 行ごとの判定（lens の JSON を Reviewed と同じ `read_outcome` → `narrow` の 2 本で読み、done の対応の表の倒し〔[contract-source.md](./contract-source.md) §64 形 4〕を通す）から ref の結果を 1 語に決める: 全行が PASS か、INCONCLUSIVE の行が全部 basis が forecast か partial で理由の型が unparsed でない → pass／FAIL の行か、basis が actual の INCONCLUSIVE の行か、unparsed の行が 1 本でも在る → fail／撃ち中の印の持ち主が生きている → pending／印の持ち主が死んで撃ち終えていない → stale。
- forecast と partial の INCONCLUSIVE を pass に数える理由と C10 の読み: 祖先の本文は祖先が着地するまで存在しない。止めると依存を持つ行の設計の PR が祖先の着地を待って直列になる。「測れない」を merge の通過に倒すのは、その判定が便の段の判定として効かないからである: forecast と partial の記録は Reviewed の段で使い回されない（§5・材料の鍵か code の木の鍵が必ず違う）ので、その行は祖先の着地の後の Reviewed で実物の base で審査し直され、そこで測れなければ今どおり Reviewed で止まる。FAIL は basis に依らず止める。
- 索引の表（[reverse-index.md](./reverse-index.md) §7 (a)・(b)・(c)・[ADR-0105](../../design-intent/decisions/ADR-0105-code-facts-come-from-an-external-index-the-vessel-reads.html)・proposed）: vessel 宣言が code の索引を名乗る repo では、形 4 の機械の検査に索引の閉包と code の事実の欄の測りが加わり（確定の finding）、形 5 の材料に逆引きの表 index.txt が加わる。どちらも同じ epic の別の設計の行で、この設計の行 a の後に起こす。

## 4. merge の門の 2 つ目の判定（vessel 宣言の任意 key row-review）

- 当たる repo: 次のどちらかの commit の vessel 宣言が任意 key `row-review = true` を持つ repo。
  - anchor（hook の root）の HEAD の宣言（既存の読み手 `head_declaration` の包み・任意 key の他の読み手と同じ形）。
  - merge する PR の head の commit（`--match-head-commit` の値）の宣言（同じ読み手に sha を渡す形・`floor_check_at` と同じ `<sha>:<file>` の読み）。
  - 両方を読むのは、PR 自身が key を外して門を素通りする形と、key を足す PR が自分の門を持たない形の両方を塞ぐためである。宣言が在るのに読めない周・head の commit が local の object db に無い周は断る側に倒す（C10・断り文は `git fetch origin` を名指す）。key を持たない repo の merge の門は今のまま（消費側の repo は自分で名乗るまで変わらない）。
- 判定（既存の trailer の 5 語の判定の後・当たる segment ごとに）:
  1. `--match-head-commit` の値が 40 桁の 16 進でない（無い・短い・展開の字〔本文の読みと同じ `is_loose_path` の規則〕を含む）→ no-head-pin。head を固定しない merge は、審査した commit と merge される commit が同じだと言えない。
  2. 置き場の ref の記録（§9）がその sha に無い・読めない・schema が違う → row-review-missing。
  3. ref の結果が pending → row-review-pending、stale → row-review-stale。
  4. ref の結果が fail → row-review-failed。
  5. ref の記録が名指す契約表の file のどれかが、記録の merge-base と今の anchor の `origin/main` の間で変わった（`git diff --quiet` の 1 本）→ row-review-moved。審査の後に main へ入った別の PR が同じ doc の行か節を変えた周に、merge の結果の行が一度も審査されていない形を止める。
- 断りの語は既存の 5 語の後ろに宣言順で 6 語を足す（判定の順と同じ）。断りの 1 行は次の一手として `pipe review --ref <sha>` の argv か ref の記録の path を名指す。
- 門が新しく読むのは、vessel 宣言の file（2 つの commit）と置き場の ref の記録の 1 file と、契約表の file の差の有無だけで、台帳と event log は読まない。置き場は hook の入口が他の門（anchor の門）と同じく受けた state dir を使う。
- 同じ行で、`--help` と `-h` を持つ `gh pr merge` を merge と読まない 1 形を見分けに足す（今の門は help の表示を no-body で断る・§2）。

## 5. Reviewed の段の使い回し（先撃ちの使い回しの読み口を置き換える）

- Reviewed の段は、実物の base で組んだ判定の鍵（§9・材料の鍵と code の木の鍵と lens の版を含む）と同じ鍵の行の記録が在り、その記録の判定が PASS で basis が actual の時だけ、lens を撃たずに記録の判定を写す。段の detail の末尾の語は ` row-review:reused`（`read_detail` は語で読むので report は変わらない）。違えば今どおり撃つ。forecast と partial の記録は写さない（祖先の実物か doc を跨ぐ祖先が審査の材料に無かった）。
- 鍵に lens の cmd の字を入れない（binary の置き場の path が入り、入れ替えのたびに外れる）。lens の同一性は lens の版で比べる。lens の版は lens の子自身が出す: lens の口に、claude を撃たずに自分の `--rules` と組み込みの雛形から版の 1 行（model・effort・雛形の digest・道具の列・permission の mode・`gate.token_cap`）を stdout に出す flag を足し、行の審査は撃つ前に、Reviewed は写す前に、同じ lens の cmd にその flag を付けて撃って読む（撃つ側の定数から推さない）。
- code の木の鍵を入れるのは、読みの道具を持つ lens の判定の入力が材料の dir の外（審査の木の全部）に広がるためである。code の木の鍵は、審査の木の tree から契約表を持つ file を除いた全 file の path と blob の hash の列の digest で、他の PR が設計 doc を直しただけでは動かず、code を変える着地で動く。
- 先撃ちの使い回しの読み口（`reusable`）は行 e まで残し、行 c は行の審査の読み口を足して先撃ちの読み口の前に置く（先撃ちの使い回しの歯は行 e まで緑のまま）。
- 使い回した周は審査の消費を書かない（lens を撃っていない）。行の審査の lens の消費は行の記録に残す（先撃ちの消費が便の記録に載らなかった穴を繰り返さない）。

## 6. 先撃ちの退役（事前審査の機械の予想と束は残す）

- 段 1（§11 の行 d）: rules 行 `pipe.precheck_lens_per_round` の値を 0 にし（値の変更は裁定 id が要る）、同じ行で先撃ちの `round` が値 0 の周に材料の組み直しも撃たないようにする（今は値に依らず一時の worktree を毎周組む・§2）。同じ行で本 repo の vessel 宣言に `row-review = true` を足す。事前審査の機械の予想（形 x）と束（形 y）は残す（lens を撃たず安い・依存待ちの確定の誤りを束ねる役は行の審査と重ならない）。
- 段 2（§11 の行 e）: 先撃ちの残りの code を外す: 先撃ちの子 module の残り（行 a が共用の子 module へ移した 5 本は外さない）と、その宣言の行、lens の `--stage prelens`、rules 行 `pipe.precheck_lens_per_round` と `pipe.precheck_lens_model`、先撃ちを測る歯、`[DISPATCH-PRECHECK]` の行の ` prelens=` の字面。外すのは git の履歴に残る code の退役（憲法 N1 の可逆な形）で、使い回しの読み口は行 c が行の審査の記録へ移した後に外す。

## 7. 行の予約（落ちた行が write-set を持ち続ける）

- 行の予約を持つ行 B: 台帳で open ∧ B の直前の便が Landed でない終端（Reviewed の判定が PASS でない・Gated の判定が FAIL・Failed・Stopped）∧ その便の後に同じ bead の便が起きていない（RunCreated が無い）∧ B の最新の介入の印が hold でない ∧ その終端の記帳から rules 行 `pipe.reserve_h` の時間が過ぎていない。B は起動の列の上では、直す前は列外の `Settled`、直した後は列に戻って受付や依存で待つ候補である（どちらでも、次の便が起きるまで行の予約を持つ）。
- 行の予約に入れる file: B の直前の便の契約の写し（便の置き場の契約 file）の write-set。
- 効き: FR68 の順序の 1 関数（`order`）で B より後ろに並ぶ候補のうち、write-set が B の行の予約と交差する（FR39 と同じ判定・`crossings` の交差の読み）ものは、待ちの理由の新しい variant（名 reserved・値は B の bead と交差した file の本数）で待つ。B より前の候補（介入 first・高い priority・若い起票順）は待たない。B が台帳の blocks で待つ祖先（推移）の候補は待たない（B の直しとして前提の行 C を足し B に C を blocks で付けた形で、B が依存で待ち C が行の予約で待つ輪を作らない）。
- 直った行が最初に起きる: 直しの PR で B の契約の中身が変わると B は `Settled` を抜けて候補に戻り、元の順位（first → priority → 起票順）のまま、行の予約で待たせていた後ろの行より先に評価される。B が受付や依存で待つ周も、B が新しい便を起こすまで行の予約は続く。
- 行の予約が解ける契機（閉じた列・どれか 1 つ）: B の新しい便の RunCreated・B の bead の close・B の memo か問いの label・B への hold の印（orchestrator が B を置いて後ろを先に通すと決めた印・今の口のまま）・期限。
- 期限の行: rules 行 `pipe.reserve_h`（kind は新しい 1 つ・Int・時間・値は SRS の round で user の裁定 id を取る・推奨 24）。値 0 は期限なし。行が無い・読めない周は期限なしで行の予約を掛け、`dispatch ls` の reserved の行の末尾に ` reserve=unset` を足す（測れない期限を「行の予約を掛けない」に畳まない・C10）。
- 観測: `dispatch ls` の reason は `reserved:<B の bead>/<file の本数>`。期限が切れた周は、その周の idle の知らせの末尾の既存の未処置の終端の語（[dispatcher.md](./dispatcher.md) §29）と同じ所で名指される（新しい送達を足さない）。

## 8. 兄弟の待ち（同じ設計から出た行を、直しが入るまで待たせる）

- 待たせる元 B: 直前の便が設計の側の終端に着いた行＝Reviewed の判定が FAIL か INCONCLUSIVE（理由の型が unparsed でない）、または Gated の判定が FAIL。Failed（起動の失敗・環境）と Stopped（人の停止）は元にしない。
- 兄弟: B と同じ設計 doc の行のうち、(a) B と同じ section を実装する行、または (b) B の直前の便の契約と節の写しから求めた行の digest を持つ B の行を載せた ref の記録（1 本でも・push ごとの記録も直しの PR の記録も含む）に載る、B 以外の行。どちらも置き場の file だけから 1 関数で導く。起動の列の候補（live でない）だけが対象である。
- 効き: 兄弟の候補は待ちの理由の新しい variant（名 sibling・値は B の bead）で待つ。介入 first の印を持つ候補は待たない（orchestrator が名指して起こす口を残す・[dispatcher.md](./dispatcher.md) §35 の床の検査の設計と同じ扱い・その設計は未着地）。
- 兄弟の待ちが解ける契機（閉じた列・どれか 1 つ）: B の行の digest が B の直前の便の記録の写し（契約 file と設計の節の本文の写し）と違う周（直しが main に入った）・B への release か hold の印・兄弟自身の今の行の digest の行の記録で、判定が PASS か、basis が forecast か partial で unparsed でない INCONCLUSIVE のものが、B の終端より後に書かれた（兄弟も直されて審査を通った）・B の bead の close・期限（`pipe.reserve_h` を共用）。
- 候補 1 件の理由の決め方の順: 依存 → hold → launched → 設計 pointer → 契約の生成 → settled → sibling →（行 aj の後は floor の上書き）→ `settle` の中で reserved → overlap → 受付。名の列 `WAIT_REASONS` では、行 aj と am が足す `unreflected-ruling` と `floor` の後ろに reserved と sibling を足す。
- 行 a1 と b1 の例では、b1 は (b)（同じ設計の PR）で a1 の兄弟になり、a1 の終端の 4 秒後の起動は起きない。

## 9. 置き場の形と跨版の約束（event kind は足さない）

- 行の予約と兄弟の待ちは記帳しない。列の周ごとに、既存の記録（便の段の event・便の置き場の契約 file と材料の写し・介入の印・RunCreated）と rules 行と行の審査の記録から導く（新しい event kind を足さない・C17）。待ちの理由の閉じた型に variant を 2 つ足す（§8 の位置）。
- 判定の鍵: 行の digest・材料の鍵（材料の dir の全 file の名と本文から名の順に決まる digest）・code の木の鍵（§5）・basis・祖先ごとの状態の語の列・lens の版（§5）を 1 行ずつ並べた字の digest。
- 行の審査の置き場: state dir の pipe の下の row-review の dir。
  - 行の記録の dir: 名は `<doc>#<行 id>@<判定の鍵>` の字の FNV-1a 64 の 16 桁（束の id と同じ形）。中に record（1 行 1 key の `key=value`・1 行目は `schema=1`）・材料の dir（Reviewed と同じ file 名）・lens の `rc` と `out`。
  - record の key: row・digest（行の digest）・key（判定の鍵）・basis（actual / forecast / partial）・ancestors（`<行>:<landed|tree|declared>` の列か `-`）・mech（clean か `firm:<断りの名>`）・verdict・kind（`-` か理由の型）・materials（材料の鍵）・tree（code の木の鍵）・version（lens の版の 1 行）・ref・at（UTC の秒）・usage（lens の消費の 6 値か `-`）。
  - ref の記録: ref の dir の下に 40 桁の sha の名で 1 file。1 行目は `schema=1`、2 行目は `base=<merge-base の sha>`、3 行目は `tables=<契約表の file の列>`、変わった行ごとに `row=<doc>#<行 id> id=<行の記録の dir の名> verdict=<V> basis=<B>` の 1 行、最後の行は `result=<pass|fail>`（撃ち終えた後に書く）。撃ち中の印は同じ dir の `<sha>.pid`。
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
| 待ちの理由 2 つ・rules 行と kind 1 つ（`pipe.reserve_h`） | rules 行と kind 2 つ（`pipe.precheck_lens_per_round`・`pipe.precheck_lens_model`）（行 e） |
| lens の口の版の flag 1 つ | 使い回しの読み口 1 つ（`reusable`）（行 e） |

## 11. SRS の round の後の行（粒度・順序・write-set の見込み・歯）

SRS の追加 round（直しは FR49 / FR68 / FR92 と AC22 / AC62 と新しい受け入れ基準・AC38 は変えない）の後に、次の 7 行をこの doc の契約表に足す。write-set は見込みで、起票の前に `pipe preflight` と行の審査そのもので測り直す。

| 行 | 中身 | 順（台帳の blocks） | write-set の見込み |
|---|---|---|---|
| a | 行の審査の口（§3 の形 1〜9・判定の鍵・行の記録と ref の記録の書き手と読み手・撃ち中の印）と、先撃ちの木の 5 本の共用の子 module への移し | 打ち手 0 の行（審査の木と読みの道具）の後 | pipe の新しい子 module 2 つ（`+`・口と共用の木）・`crates/scribe2/src/pipe/cli.rs`・`crates/scribe2/src/pipe/cli/args.rs`・`crates/scribe2/src/help.rs`・`crates/scribe2/src/pipe/review.rs`・`crates/scribe2/src/pipe/dispatch/prelens.rs`（5 本を移す）・`crates/scribe2/src/pipe/dispatch/precheck.rs`（祖先の層を行の鍵へ載せ替え）・`crates/scribe2/src/headless/lens.rs`（版の flag）・`crates/scribe2-boundary/tests/e2e/headless/lens.rs`（版の flag の歯）・`crates/scribe2-boundary/tests/e2e/pipe.rs`（subcommand の列の pin）・`crates/scribe2-boundary/tests/e2e/pipe/review.rs`（歯）・pipe と headless の外形 snapshot |
| b | merge の門の 2 つ目の判定（§4）と vessel 宣言の任意 key row-review | a の後 | `crates/scribe2/src/hook/merge_gate.rs`・`crates/scribe2/src/hook/anchor_guard.rs`（help の見分け）・`crates/scribe2/src/hook/mod.rs`（root と state dir の受け渡し）・`crates/scribe2/src/pipe/declaration.rs`・`crates/scribe2/src/pipe/declaration/optional_keys.rs`・`crates/scribe2-boundary/tests/e2e/hook/guards.rs`（歯）・[vessel-hook.md](./vessel-hook.md) §21 の断りの語の表 |
| c | Reviewed の使い回しの読み口を足す（§5・先撃ちの読み口は行 e まで残す） | a の後 | `crates/scribe2/src/pipe/review.rs`・`crates/scribe2/src/pipe/cli/step.rs`（`Review` の組み立て）・行 a の子 module・`crates/scribe2-boundary/tests/e2e/pipe/review.rs` |
| d | 先撃ちの退役の段 1（§6・rules 行の値 0・値 0 の周の組み直しの止め・宣言の key） | b・c の後 | `rules/manifest.toml`・`.vessel.toml`・`crates/scribe2/src/pipe/dispatch/prelens.rs`・`crates/scribe2-boundary/tests/e2e/rules.rs`・`crates/scribe2-boundary/tests/e2e/pipe/review.rs` |
| e | 先撃ちの code の退役（§6 段 2） | d の後 | 先撃ちの子 module（`-`）・`crates/scribe2/src/pipe/dispatch.rs`・`crates/scribe2/src/pipe/dispatch/precheck.rs`・`crates/scribe2/src/pipe/review.rs`・`crates/scribe2/src/pipe/cli/step.rs`・`crates/scribe2/src/headless/lens.rs`・`crates/scribe2/src/headless/mod.rs`・`crates/scribe2/src/help.rs`・`crates/scribe2/src/rules/mod.rs`・`rules/manifest.toml`・`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・`crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs`・`crates/scribe2-boundary/tests/e2e/headless.rs`・`crates/scribe2-boundary/tests/e2e/headless/lens.rs`・`crates/scribe2-boundary/tests/e2e/rules.rs`・`crates/scribe2-boundary/tests/e2e/rules/embedded.rs`・headless と rules の外形 snapshot |
| f | 行の予約（§7）と rules 行 `pipe.reserve_h` | a〜e と論理は独立・dispatch.rs で e と、dispatch.rs と candidates.rs で dispatcher.md の行 aj / am と直列（受付の交差が順を決める） | `crates/scribe2/src/pipe/dispatch.rs`（variant と名の列・既存の歯 `pipe_dispatch_wait_reasons_render_the_name_and_the_value` の本文を直すので retroactive の札）・`crates/scribe2/src/pipe/dispatch/candidates.rs`・`crates/scribe2/src/rules/mod.rs`・`rules/manifest.toml`・`crates/scribe2-boundary/tests/e2e/rules.rs`・`crates/scribe2-boundary/tests/e2e/rules/embedded.rs`・rules の外形 snapshot・e2e の既存の dispatch の歯の file |
| g | 兄弟の待ち（§8） | f と a の後 | f と同じ dispatch.rs と candidates.rs・行 a の子 module（ref の記録の読み）・e2e の既存の dispatch の歯の file |

- 歯（done の項目ごとに 1 本以上・置き場は上の歯の file・どれも base で RED の理由を書く）:
  - 行 a（接頭辞 row_review_・e2e）: (1) digest の変わった行だけが撃たれ、変わらない行と bead が全部 closed の行は撃たれず、bead の無い行は撃たれる (2) 宣言の祖先を持つ行の材料に祖先の材料の file が在り空の `+` の file が無い (3) 確定の finding を持つ行は lens 0 回で FAIL・mech が firm (4) 同じ判定の鍵の 2 回目は lens 0 回で、要件の本文か code の file を 1 つ変えた 2 回目は撃つ (5) 結果の 4 語（pass・fail・pending・stale）と、forecast / partial の INCONCLUSIVE が pass・actual の INCONCLUSIVE が fail (6) 同じ sha の 2 本目の口は印の持ち主が生きている間撃たない。base は subcommand が無いので全部 RED（機能不在）。lens の版の flag の歯（接頭辞 headless_lens_version_・e2e の headless の lens の歯の file）は flag が未知の引数で断られるので RED。
  - 行 b（接頭辞 hook_merge_row_review_・e2e の hook の門の歯の file）: 6 語の断りがそれぞれの fixture で出て、pass の sha を固定した command は通り、key を持たない repo の 6 形は FR92 の判定だけで決まり、PR の head だけが key を外した形と anchor だけが key を持たない形の両方で門が掛かり、`--help` の command は断られない。base は 6 語が無く help を no-body で断るので RED。
  - 行 c（接頭辞 pipe_review_row_reuse_・e2e）: 判定の鍵が同じ actual の PASS の記録を持つ便は偽 lens 0 回で ` row-review:reused` が付き、材料の 1 file・code の木・lens の版・basis（forecast / partial）のそれぞれが違う便は偽 lens が撃たれる（写し 1 + 撃つ 4）。base は読み口が無いので写し 1 が RED。
  - 行 d（接頭辞 rules_prelens_off_・e2e）: `rules get pipe.precheck_lens_per_round` が 0 を返し、値 0 の写しの周に先撃ちの置き場に一時の worktree も材料も組まれない。base は値 1 で、値 0 の周も材料を組むので RED。既存の `rules_prelens_` の値の pin は retroactive の札で直す。
  - 行 e（接頭辞 headless_lens_stage_retired_ と rules_prelens_retired_・e2e）: `lens --stage prelens` が未知の引数で断られ、`rules get pipe.precheck_lens_model` が no such id で断られる（行動で測る・字面の pin を使わない）。base は受け付けるので RED。
  - 行 f（接頭辞 pipe_dispatch_row_reservation_・e2e）: (1) 落ちた B（順位が前）と同じ file を触る後ろの C が B の終端の後の周に reserved で待ち、前の候補は待たない (2) B の契約を変えた周に B が C より先に起きる (3) hold・期限・B の新しい便・close のそれぞれで解ける (4) B が C を blocks で持つ周に C が起きる（輪を作らない） (5) rules 行の無い写しで ` reserve=unset` が出る。base は reserved が無いので RED。
  - 行 g（接頭辞 pipe_dispatch_sibling_wait_・e2e）: B と同じ節の行と、B の digest を載せた ref の記録（push 2 回の PR・直しの PR の 2 形）の行が sibling で待ち、first の印の行は待たず、Failed と Stopped の B は待たせず、B の digest が変わった周と兄弟自身の PASS の記録が B の終端の後に書かれた周に解け、FAIL の記録では解けない。base は sibling が無いので RED。

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
