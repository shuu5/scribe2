# 設計: 行の審査 — 契約表の行を merge の前に審査し、落ちた行の順番を起動の列が守る

- 出所: 持ち主の問い 2026-09-30T11:3xZ〜11:5xZ（逐語は台帳 s2-07l.736.31 の notes）→ epic s2-07l.736.33 → [ADR-0103](../../design-intent/decisions/ADR-0103-contract-rows-pass-row-review-before-merge-and-failed-rows-keep-their-place.html)。材料は 2026-09-28〜09-30 の便 165 本の FAIL / INCONCLUSIVE 76 件の分析（host の file・tracked でない）。
- 要件（今の字）: [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR68](../../design-intent/spec/srs.html#FR68) 起動の列 / [FR92](../../design-intent/spec/srs.html#FR92) merge の門 / [FR39](../../design-intent/spec/srs.html#FR39) 交差の排他 / [FR55](../../design-intent/spec/srs.html#FR55) 契約表の CI の検査。FR49 と FR68 と FR92 の字を直す SRS の追加 round が先に要る（§11）。
- 前提: 審査役に読みだけの道具を渡し、作業場所を base の写しにする設計（epic s2-07l.736.33 の打ち手 0・別の設計）。行の審査の lens はその形で撃つ。
- この設計から出る契約: §11 の 7 行。契約表の行は SRS の round の後に足す（FR49 と FR68 の意味が変わるので、今の要件 id を指す行にできない）。

## 1. 何を解くか

契約の審査（FR49）は便の中に在る: 設計の PR を merge → 行が起動の列に入る → 起こされた便の Reviewed の段で審査 → runner → gate。設計の欠陥は便の中の審査か gate で初めて見つかる。

- 2026-09-28〜09-30 の FAIL / INCONCLUSIVE 76 件（Reviewed 46・gate 30）のうち 73 件（96%）は設計の側が根だった。runner が主因は 2 件、環境 1 件、審査役の誤りは 0 件。
- 審査で落ちた行は列の外へ出る（同じ契約の中身で終端に着いた便の行は `Settled`）。直しの PR が merge されるまで、その行の write-set は誰のものでもなくなる。同じ file を触る後ろの行が先に起き、直った行はその行の終わりを待つ側に回る（行 a1 が 2 回落ちる間に同じ P2 の行 ak が起き、直った a1 は ak の終わり待ち）。
- 同じ欠陥を持つ兄弟の行を止める口も無い（行 b1 は a1 が同じ指示で落ちた 4 秒後に起こされ、同じ理由で落ちた。直しは 53 分後）。
- 先撃ち（[dispatcher.md](./dispatcher.md) §27 形 aa）は審査の層になっていない。Reviewed の FAIL 46 件のうち事前に撃たれていたのは 9 件で、そのうち 6 件は先撃ちが FAIL / INCONCLUSIVE を出したまま起動した。使い回し（形 ac）は全期間で 0 件。

やさしく言うと: 設計の穴は「列に並んだ後」に見つかるので、落ちた行は列から外れ、後ろの行に file を取られる。審査を「列に並ぶ前」（設計の PR を main に入れる前）へ移し、それでも落ちた行は自分の触る file を予約して順番を守り、同じ設計から出た兄弟の行は直しが入るまで待たせる。

## 2. 何が起きているか（main eb377622・verified）

- **審査の時点**: `crates/scribe2/src/pipe/review.rs` の `review`（:306）が便の置き場の材料の dir に `stage`（:343）で材料を組み、`decide`（:535）が lens を撃つ。lens の cwd は `entry.repo`（anchor の repo）である。先撃ちの判定の使い回しは `crates/scribe2/src/pipe/dispatch/prelens.rs` の `reusable`（:317）で、同じ model に解ける周だけ引く。
- **使い回しが当たらない理由**（先撃ちの分析・host の file の §9・verified）: 使い回しの口が在った窓の Reviewed 61 回で使い回しは 0 回。
  - 先撃ちの材料には予想の印の行（「予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた」）が必ず入る（114 本中 114 本・Reviewed は 163 本中 0 本）。
  - 宣言だけの祖先は、着地で変える既存の file の中身を予想に写さず、新しい file を空で置く。祖先が着地すると base の要約と外の材料が変わる（例: write-set の 1 file が 521 行 → 899 行）。
  - lens の cmd の字に binary の置き場の path が入り、置き場は 56 時間で 18 回変わった。
  - 印を外して材料の 5 区画が全部同じだったのは 20 回のうち 1 回。使い回すと、先撃ちの見逃し（同じ版の PASS 11 回中 2 回を本番の審査が FAIL にした）がそのまま素通りする。
- **列の順と終端の行**: `crates/scribe2/src/pipe/dispatch/candidates.rs` の `entry_of`（:43）は依存 → hold → 起こした印 → 設計 pointer → 契約の生成 → `settled`（:240）の順に理由を付ける。`settled` は直前の便が終端に着き契約 file が同じ行を `Settled` にする（Reviewed の終端だけは設計の節の本文の写しも鍵に入る・`section_keyed` :274）。`settle`（:87）は `order`（`crates/scribe2/src/pipe/dispatch.rs` の :818）の順に候補を見て、`blocker`（:127）が live な便と同じ周に起こした便の write-set とだけ交差を測る。`Settled` の行の write-set はどこにも数えない。
- **待ちの理由**: `WaitReason`（dispatch.rs の :99）は閉じた型で、名の列は `WAIT_REASONS`（:94）。
- **merge の門**: `crates/scribe2/src/hook/merge_gate.rs` の `decide`（:245）は本文の file だけを読み（台帳・git・event log・置き場を読まない）、断りの語は宣言順の 5 語（`REASONS` :65）。gh 2.45.0 の `pr merge` は `--match-head-commit SHA`（PR の head がその sha でなければ merge しない）を持つ（この host の help で実測 2026-09-30）。
- **先撃ちの rules 行**: `pipe.precheck_lens_per_round`（値 1・値 0 は撃たない）と `pipe.precheck_lens_model`（sonnet）。契約の審査と gate の審査の model は `lens.model`（opus）。
- **vessel 宣言の任意 key**: `crates/scribe2/src/pipe/declaration/optional_keys.rs` が `ruling-check` / `floor-check` / `remote` を持つ。repo ごとに器の門へ加わる形はこの 3 つと同じ。

## 3. 行の審査（設計の PR の head で、変わった行ごとに撃つ）

- 口: `<NAME> pipe review --ref <sha> --repo R --state-dir S --lens CMD [--rules PATH]`。orchestrator の席が設計の PR を push した後に撃つ。merge の門の断り（§4）が同じ argv を名指すので、撃ち忘れは merge の時点で見える。
- 形（番号は §11 の行 a の done と 1:1 にする予定）:
  1. **変わった行**: `--ref` の木と、その commit と main の先端の merge-base の木の両方で、契約表を持つ file（受付と同じ表の読み手が行を返す file）の行を読み、行の digest（形 2）が merge-base と違う行と merge-base に無い行を「変わった行」とする。台帳を読みだけで読み（`read_ledger`）、設計 pointer がその行を指す bead が 1 本以上在って全部 closed の行は外す（着地済み）。変わった行が 0 本なら ref の記録（§9）だけを書き、lens は撃たない。
  2. **行の digest**: 行の欄から受付と同じ生成（`generated`）で作った契約 file の字の hash（`Settled` の鍵と同じ git の object の hash）と、実装する設計の節の本文（審査の材料と同じ読み手 `design_material`）の hash の対。FR49 が契約の中身の同一性に使う 2 つと同じものである。
  3. **祖先の扱い**: 行の祖先は、表の depends を推移でたどった行（同じ doc）と、行を指す bead が在ればその台帳の blocks の祖先（事前審査の到達と同じ読み）。祖先ごとに状態を決める: 着地（祖先を指す bead が全部 closed）／実物（祖先の便が Gated で判定 PASS・事前審査の実物の層と同じ）／宣言（それ以外・bead の無い行を含む）。宣言の祖先を 1 つでも持つ行の審査は basis が forecast、持たない行は actual である。宣言の祖先の `+` の file は空で置かない（先撃ちの予想の形を採らない）。代わりに材料の dir に祖先の材料の file を 1 つ置き、宣言の祖先ごとに、行の TOML の写し（`find_row` が返す行の字をそのまま）と実装する設計の節の本文（`design_material`）を並べる。lens には「この file は未着地の祖先が作る・変える」と材料の形で渡る。
  4. **機械の検査**: 変わった行ごとに受付と同じ `generated` → `judge`（置き場なし・`pipe preflight` と同じ 1 本）を撃つ。宣言の祖先を持つ行は事前審査の宣言の予想（[dispatcher.md](./dispatcher.md) §27 形 x の 1）の上で撃ち、確定と暫定の弁別（同 形 x の 3）を借りる。確定の finding を持つ行は lens を撃たずに判定 FAIL とし、理由の型に断りの名を置く。暫定の finding は lens の材料の末尾に写す。
  5. **lens**: 機械の検査を通った行ごとに、`--ref` の detached な一時の worktree を置き場の tree の dir に作り、実物の祖先の差分を重ね（事前審査の実物の層と同じ当て方）、Reviewed と同じ組み手（`stage`）で材料を組み、契約の審査と同じ lens（`--stage` なし＝rules 行 `lens.model`・同じ雛形）を打ち手 0 の読みだけの形で撃つ。行ごとに 1 回で、同時に起こす本数は受付の host の memory の枠（受付札・[gate-cost.md](./gate-cost.md) §3.2）で絞る。材料を組み終えて lens が終わったら worktree を外す。落ちた周の残りは、次に口を撃った周の頭に外す（事前審査の tree の片付けと同じ形）。
  6. **裏で撃つ**: 口は自分を process group を分けて起こし（`spawn_self` と同じ起こし方）、`[ROW-REVIEW] ref=<sha> rows=<n> result=pending` の 1 行を返してすぐ終わる。子は全部の行を撃ち終えた後に ref の記録の result を書き、[dispatcher.md](./dispatcher.md) §19 と同じ宛先へ同じ送達の 1 関数（`crates/scribe2/src/pipe/notify.rs` の `send`）で `scribe2 pipe: row-review ref=<12 桁> rows=<n> pass=<k> fail=<j> file=<ref の記録の path>` の 1 行を送る。
  7. **同じ digest は撃ち直さない**: 同じ行の digest の記録が既に在り、判定が unparsed でなく、lens の版（model の値・effort の値・lens の雛形の digest）が今と同じ行は撃たず、ref の記録にその記録を名指す。PR の直しの commit で変わらなかった行の審査は 1 回で済む。
  8. **ref の結果**: 行ごとの判定（lens の JSON を Reviewed と同じ読み手で読み、done の対応の表の倒し〔[contract-source.md](./contract-source.md) §64 形 4〕を通す）から ref の結果を 1 語に決める: 全行が PASS か、INCONCLUSIVE の行が全部 basis=forecast で理由の型が unparsed でない → pass／FAIL の行か、basis=actual の INCONCLUSIVE の行か、unparsed の行が 1 本でも在る → fail／撃ち終えていない行が在る → pending。
- basis=forecast の INCONCLUSIVE を pass に数える理由: 祖先の本文は祖先が着地するまで存在しない。止めると依存を持つ行の設計の PR が祖先の着地を待って直列になる。その行は祖先の着地の後の Reviewed の段で実物の base で審査し直す（§5・使い回さない）。FAIL は basis に依らず止める。

## 4. merge の門の 2 つ目の判定（vessel 宣言の任意 key row-review）

- 当たる repo: vessel 宣言が任意 key `row-review = true` を持つ repo だけ。key の無い repo の merge の門は今のまま（消費側の repo は自分で名乗るまで変わらない）。
- 判定（既存の trailer の 5 語の判定の後・当たる segment ごとに）:
  1. `--match-head-commit` の値が 40 桁の 16 進で無い → no-head-pin（head を固定しない merge は、審査した commit と merge される commit が同じだと言えない）。
  2. 置き場の ref の記録（§9）がその sha に無い・読めない・schema が違う → row-review-missing。
  3. ref の結果が pending → row-review-pending。
  4. ref の結果が fail → row-review-failed。
- 断りの語は既存の 5 語の後ろに宣言順で 4 語を足す（判定の順と同じ）。断りの 1 行は次の一手として `pipe review --ref <sha>` の argv か ref の記録の path を名指す。
- 門が新しく読むのは vessel 宣言の file と置き場の ref の記録 1 file だけで、台帳・git・event log は読まない。置き場は hook の入口が他の門（anchor の門）と同じく受けた state dir を使う。

## 5. Reviewed の段の使い回し（先撃ちの使い回しの読み口を置き換える）

- Reviewed の段は、実物の base で組んだ材料の鍵が、同じ行の digest の記録の材料の鍵と同じで、記録が basis=actual ∧ 判定 PASS ∧ lens の版が今と同じ時だけ、lens を撃たず記録の判定を写す。段の detail の末尾の語は ` row-review:reused`（`read_detail` は語で読むので report は変わらない）。違えば今どおり撃つ。
- 材料の鍵は、材料の dir の全 file の名と本文から名の順に決まる 1 つの digest（今の `crates/scribe2/src/pipe/dispatch/prelens.rs` の `digest` と同じ 1 関数を review の側へ移す）。鍵に lens の cmd の字を入れない（binary の置き場の path が入り、入れ替えのたびに外れる）。lens の同一性は lens の版（model の値・effort の値・雛形の digest）で比べる。
- basis=forecast の記録は写さない（祖先の実物が材料に無かった審査である）。
- 使い回した周は審査の消費を書かない（lens を撃っていない）。行の審査の lens の消費は行の記録に残す（§9・先撃ちの消費が便の記録に載らなかった穴を繰り返さない）。

## 6. 先撃ちの退役（事前審査の機械の予想と束は残す）

- 段 1（§11 の行 d）: rules 行 `pipe.precheck_lens_per_round` の値を 0 にする（値 0 は撃たない＝今の実装のまま・値の変更は裁定 id が要る）。同じ行で本 repo の vessel 宣言に `row-review = true` を足す。事前審査の機械の予想（形 x）と束（形 y）は残す（lens を撃たず安い・依存待ちの確定の誤りを束ねる役は行の審査と重ならない）。
- 段 2（§11 の行 e）: 先撃ちの code を外す: 先撃ちの子 module と、その宣言の行、lens の `--stage prelens`、rules 行 `pipe.precheck_lens_per_round` と `pipe.precheck_lens_model`、歯の接頭辞 `pipe_prelens_` と `pipe_review_reuse_` の歯、`[DISPATCH-PRECHECK]` の行の ` prelens=` の字面。外すのは git の履歴に残る code の退役（憲法 N1 の可逆な形）で、使い回しの読み口は行 c が行の審査の記録へ移した後に外す。

## 7. 行の予約（落ちた行が write-set を持ち続ける）

- 予約を持つ行 B: 台帳で open ∧ 起動の列の入力に在る ∧ B の直前の便が Landed でない終端（Reviewed の判定が PASS でない・Gated の判定が FAIL・Failed・Stopped）∧ その便の後に同じ bead の便が起きていない（RunCreated が無い）∧ B の最新の介入の印が hold でない ∧ その終端の記帳から rules 行 `pipe.reserve_h` の時間が過ぎていない。
- 予約する file: B の直前の便の契約の写し（便の置き場の契約 file）の write-set。
- 効き: `order` の順で B より後ろの候補のうち、write-set が予約と交差する（FR39 と同じ判定・`crossings` の交差の読み）ものは、待ちの理由の新しい variant（名 reserved・値は B の bead と交差した file の本数）で待つ。B より前の候補（介入 first・高い priority・若い起票順）は待たない。
- 直った行が最初に起きる: 直しの PR で B の契約の中身が変わると B は `Settled` を抜けて候補に戻り、元の順位（first → priority → 起票順）のまま、予約で待たせていた後ろの行より先に評価される。B が受付で断られて候補のまま待つ周も、B が新しい便を起こすまで予約は続く。
- 予約が解ける契機（どれか 1 つ）: B の新しい便の RunCreated・B の bead の close・B の memo か問いの label・B への hold の印（orchestrator が B を置いて後ろを先に通すと決めた印・今の口のまま）・期限。
- 期限の行: rules 行 `pipe.reserve_h`（kind は新しい 1 つ・Int・時間・値は user の裁定 id が要る・推奨 24）。値 0 は期限なし。行が無い・読めない周は期限なしで予約を掛け、`dispatch ls` の reserved の行の末尾に ` reserve=unset` を足す（測れない期限を「予約しない」に畳まない・C10）。
- 観測: `dispatch ls` の reason は `reserved:<B の bead>/<file の本数>`。予約の期限が切れた周は、その周の idle の知らせの末尾に既存の未処置の終端の語（[dispatcher.md](./dispatcher.md) §29）と同じ所で名指される（新しい送達を足さない）。

## 8. 兄弟の待ち（同じ設計から出た行を、直しが入るまで待たせる）

- 待たせる元 B: 直前の便が設計の側の終端に着いた行＝Reviewed の判定が FAIL か INCONCLUSIVE（理由の型が unparsed でない）、または Gated の判定が FAIL。Failed（起動の失敗・環境）と Stopped（人の停止）は元にしない。
- 兄弟: B と同じ設計 doc の行のうち、(a) B と同じ section を実装する行、または (b) B と同じ ref の記録（同じ設計の PR の行の審査）で審査された行。起動の列の候補（live でない）だけが対象である。
- 効き: 兄弟の候補は待ちの理由の新しい variant（名 sibling・値は B の bead）で待つ。介入 first の印を持つ候補は待たない（orchestrator が名指して起こす口を残す・床の検査と同じ扱い）。
- 解ける契機（どれか 1 つ）: B の行の digest が B の直前の便の記録の写し（契約 file と設計の節の本文の写し）と違う周（直しが main に入った）・B への release か hold の印・兄弟自身の行の digest の行の審査の記録が B の終端より後に書かれた（兄弟も直されて審査を通った）・B の bead の close・期限（`pipe.reserve_h` を共用）。
- 行 a1 と b1 の例では、b1 は (b)（同じ設計の PR）で a1 の兄弟になり、a1 の終端の 4 秒後の起動は起きない。

## 9. 置き場の形と跨版の約束（event kind は足さない）

- 予約と兄弟の待ちは記帳しない。列の周ごとに、既存の記録（便の段の event・便の置き場の契約 file と材料の写し・介入の印・RunCreated）と rules 行と行の審査の記録から導く（新しい event kind を足さない・C17.1）。待ちの理由の閉じた型に variant を 2 つ足す（`WAIT_REASONS` の名の列の末尾に reserved と sibling）。
- 行の審査の置き場: state dir の pipe の下の row-review の dir。
  - 行の記録の dir: 名は `<doc>#<行 id>@<契約 file の hash>/<節の本文の hash>` の字の FNV-1a 64 の 16 桁（束の id と同じ形）。中に record（1 行 1 key の `key=value`・1 行目は `schema=1`）・材料の dir（Reviewed と同じ file 名）・lens の `rc` と `out`。
  - record の key: row・contract・section・basis（actual / forecast）・ancestors（`<行>:<landed|tree|declared>` の列か `-`）・mech（clean か `firm:<断りの名>`）・verdict・kind（`-` か理由の型）・materials（材料の鍵）・model・effort・prompt（雛形の digest）・ref・at（UTC の秒）・usage（lens の消費の 6 値か `-`）。
  - ref の記録: ref の dir の下に 40 桁の sha の名で 1 file。1 行目は `schema=1`、変わった行ごとに `row=<doc>#<行 id> id=<行の記録の dir の名> verdict=<V> basis=<B>` の 1 行、最後の行は `result=<pass|fail|pending>`。
  - 書きは一時 file → rename。読み手は schema=1 だけを読み、読めない・schema が違う file は無いと同じに扱う（merge の門は row-review-missing・Reviewed は lens を撃つ）。跨版で読める約束はこの 1 形だけで、形を変える版は schema を上げて古い記録を読まない（撃ち直せば作り直せる）。

## 10. 費用（実測と推定を分ける）

- 実測（2026-09-28〜09-30・CLI の total_cost_usd・list 単価）: Reviewed の審査 163 本で $133.4（1 本あたり約 $0.82）、gate の lens 128 本で $54.4、先撃ち 114 本で $54.5（Reviewed の 41%・便の消費の記録に載らない）。gate の FAIL 1 件は起動から判定まで中央値 17.7 分（30 件で計 10.5 時間）。
- 推定（未実測）: 行の審査は行の版（行の digest）ごとに 1 回で、読みの道具を持つ lens は 1 turn の今の審査より重い（1 本 $2〜4 と置く・打ち手 0 の turn の上限で上から押さえる）。先撃ちの退役で $54.5 相当が浮き、Reviewed は材料の鍵が合う周だけ撃たずに済む（main が行の触る file を動かさなかった周・当たる割合は未実測）。増える費用は「行の版の数 × 行の審査 1 本」から先撃ちの分と使い回した Reviewed の分を引いたもので、減る費用は審査と gate の FAIL の周回（76 件のうち設計の側の 73 件の多く）と、落ちた行の順番の崩れで後ろの行が作り直す分である。着地の周の後に実測して ADR-0103 の見直しの材料にする。

## 11. SRS の round の後の行（粒度・順序・write-set の見込み）

SRS の追加 round（依頼の文は host の drafts・§11 の直しは FR49 / FR68 / FR92 と AC22 / AC38 / AC62 と新しい受け入れ基準）の後に、次の 7 行をこの doc の契約表に足す。write-set は見込みで、起票の前に `pipe preflight` と行の審査そのもので測り直す。

| 行 | 中身 | 順 | write-set の見込み |
|---|---|---|---|
| a | 行の審査の口（§3 の形 1〜8・行の記録と ref の記録の書き手と読み手・裏の起動と知らせ） | 最初 | pipe の新しい子 module 1 つ（`+`）・`crates/scribe2/src/pipe/cli.rs`（口）・`crates/scribe2/src/pipe/review.rs`（祖先の材料の file と材料の鍵の 1 関数）・`crates/scribe2/src/pipe/dispatch/precheck.rs`（予想と弁別の可視性）・e2e の既存の `crates/scribe2-boundary/tests/e2e/pipe/review.rs`（歯）・usage の外形 snapshot |
| b | merge の門の 2 つ目の判定（§4）と vessel 宣言の任意 key row-review | a の後 | `crates/scribe2/src/hook/merge_gate.rs`・`crates/scribe2/src/pipe/declaration/optional_keys.rs`・hook の入口の state dir の受け渡し・[vessel-hook.md](./vessel-hook.md) §21 の断りの語の表 |
| c | Reviewed の使い回しを行の審査の記録へ移す（§5） | a の後 | `crates/scribe2/src/pipe/review.rs`・行 a の子 module |
| d | 先撃ちの退役の段 1（§6・rules 行の値 0 と宣言の key） | b・c の後 | `rules/manifest.toml`・`.vessel.toml`・rules の歯 |
| e | 先撃ちの code の退役（§6 段 2） | d の後 | 先撃ちの子 module（`-`）・`crates/scribe2/src/pipe/dispatch.rs`・`crates/scribe2/src/headless/lens.rs`・`crates/scribe2/src/rules/mod.rs`・`rules/manifest.toml`・`crates/scribe2/src/pipe/dispatch/precheck.rs`・歯の file 2 つ |
| f | 行の予約（§7）と rules 行 `pipe.reserve_h` | a〜e と独立 | `crates/scribe2/src/pipe/dispatch.rs`（variant と名の列）・`crates/scribe2/src/pipe/dispatch/candidates.rs`（予約の集合と交差）・`crates/scribe2/src/rules/mod.rs`・`rules/manifest.toml`・e2e の既存の dispatch の歯の file |
| g | 兄弟の待ち（§8） | f と a の後 | f と同じ 2 file・行 a の子 module（ref の記録の読み） |

- 行 a の歯は、偽 lens と toy repo で (1) 変わった行だけが撃たれ、変わらない行と着地済みの行は撃たれない (2) 宣言の祖先を持つ行の材料に祖先の材料の file が在り空の `+` の file が無い (3) 確定の finding を持つ行は lens を撃たず FAIL (4) 同じ digest の 2 回目は撃たない (5) ref の結果の 3 語、を base で RED（口が無い）にする。
- 行 f の歯は、落ちた行 B（P2・起票が若い）と同じ file を触る後ろの行 C で、B の終端の後の 1 周に C が reserved で待ち、B の契約を変えた周に B が C より先に起き、B に hold を付けた周に C が起きることを base で RED にする。

## 12. 限界

- GitHub の画面など、席の道具の呼び出しでない merge は門の外に在る（今の merge の門と同じ）。
- 行の審査の記録は host の state dir に在る。別の host から merge すると記録が無く row-review-missing で断られ、その host で口を撃ち直す（lens の費用が 2 回かかる）。
- Reviewed の使い回しは、行の審査の後に main が行の材料に入る file を動かすと外れる（外の材料は名の宣言の行番号を持つので、祖先でない着地でも外れうる）。外れた周は今どおり撃つ（費用が減らないだけで、判定は古くならない）。
- basis=forecast の INCONCLUSIVE は merge を通る。その行の穴は祖先の着地の後の Reviewed で初めて止まり、そこで落ちた行は §7 と §8 が順番を守る。
- 兄弟は同じ設計 doc の中だけで、別の doc の行が同じ指示を写した形は拾わない。逆に、新しい doc を 1 本の PR で起こした時は (b) がその doc の全部の行を兄弟にするので、欠陥と関係の無い行も直しが入るまで待つ（行 a1 の例では直しまで 53 分・orchestrator は first で名指して起こせる）。
- 予約する write-set は落ちた便の契約の写しで、直しの PR が write-set を広げた分は B の次の便が起きるまで予約に入らない。
- 読みの道具を持つ lens の費用と所要は未実測（§10）。

## 13. 却下

- 先撃ちを Opus に戻すだけ: 使い回しは材料の鍵で外れる（使い回しの口が在った窓で Opus の先撃ちの後の Reviewed 8 回のうち 0 回・予想の印を外しても 20 回中 1 回）。当たっても先撃ちの見逃しが素通りする。母集団も依存を待つ行だけで、Reviewed の FAIL 46 件のうち 34 件（依存の無い行と再受付の便）には原理的に届かない。
- 先撃ちの判定で起動を止めるだけ: 流れた 6 件は止まるが、届く母集団は同じく依存待ちの行だけで、審査が列の後ろに在る形は変わらない（落ちた行が列から外れて順番が崩れる）。
- CI で lens を撃つ: 本 repo は PUBLIC で、CI に lens の口座と model を持たせることになる。口座の選定・封じ込め・rules 行は host の器が持つ。CI は台帳に届かず、変わった行が着地済みかを判じられない。PR の push ごとに撃つと費用が PR の数に比例する。
- 行の審査を merge の後（列に入った直後）に撃つ（旧 (r) の契約が出来た直後の審査・[dispatcher.md](./dispatcher.md) §2 が ADR-0045 の後に持たないとした形）: 行は既に main に在り、落ちた行の直しにもう 1 本の PR が要る。行の字が変わるのは merge の時点だけなので、門はそこに置く。
- 予約を起動の列の全部の待ちの候補に広げる（順位の高い待ちの行が常に file を持つ）: 依存を待つ行が長い間、関係の無い後ろの行を止める。落ちた行だけに絞る。
- 兄弟を同じ doc の全部の行にする: 何度も行を足してきた doc（行が数十本在る doc）で、別の時に別の前提で書いた行まで 1 行の欠陥で止める。同じ節と同じ設計の PR（同じ時に同じ前提で書いた行）に絞る。
- 兄弟を「同じ指示を持つ行」で選ぶ: 指示の同一は散文の読みで、機械で判じられない（C3.3）。同じ設計の PR を代わりの印にする。
- 予約と兄弟の待ちを event に記帳する: 既存の記録から毎周導けるので、新しい event kind（C17.1）は要らない。
