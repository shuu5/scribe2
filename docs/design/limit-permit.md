# 設計: 上限の許可 — user の裁定に結んだ「この作業だけ消費の上限を上げる」記録を器が記帳し、その作業の読み手だけが置き換えて読む

- 出所: 2026-10-01 の便 `s2-07l.738.38.7-20260930T224927Z` が gate の予算の照合（diff 215431 byte が cap 150000 を超えた）で Gated INCONCLUSIVE に止まり（verify は全部緑）、user が一時の上げを許し、orchestrator が host の写しの manifest を `--rules` で渡す応急の形を撃った件。前例は manifest を直す PR → binary の入れ替え → 戻しの PR の対（`s2-07l.265` / `.267`・`.375` / `.376`）。草稿（SRS の round の前・契約表は持たない）。
- 要件（今の字）: [NFR1](../../design-intent/spec/srs.html#NFR1) lens 予算 / [FR9](../../design-intent/spec/srs.html#FR9) lens の verdict / [FR15](../../design-intent/spec/srs.html#FR15)・[FR16](../../design-intent/spec/srs.html#FR16) 承認 / [FR17](../../design-intent/spec/srs.html#FR17) rules manifest / [FR22](../../design-intent/spec/srs.html#FR22) 人手 0 の計測 / [FR41](../../design-intent/spec/srs.html#FR41) 権能の所在 / [FR82](../../design-intent/spec/srs.html#FR82) 発話の記帳と裁定の結び / [FR83](../../design-intent/spec/srs.html#FR83) 裁定 id の実在。要件を 3 つ足し FR9・NFR1 の字を直す SRS の追加 round が先に要る（§9）。
- 前提: 発話の記帳と結び（[fleet-event-log.md](./fleet-event-log.md) §13 / §14・[ADR-0083](../../design-intent/decisions/ADR-0083-rulings-bind-ledger-questions-to-recorded-utterances.html)・[ADR-0087](../../design-intent/decisions/ADR-0087-user-utterances-are-sorted-three-ways-and-seat-questions-go-to-the-ledger.html)）、gate の費用（[gate-cost.md](./gate-cost.md) §43）、対話面（[dialogue-surface.md](./dialogue-surface.md)）。
- この設計から出る契約: §12 の 5 行の見込み。契約表は SRS の round と ADR の後に書く。

## 1. 何を解くか

器の上限は、user が見ていない間に token が大量に使われないための既定の守りである。守りは残したまま、正当な作業が上限に当たったときだけ、user の許しを根拠にその作業 1 つの上限を一時的に上げたい。

- 今の手は 2 つしかない。(a) manifest の値を PR で上げ、binary を入れ替え、着地の後に戻す PR を出す（重い・上げている間はほかの便の上限も緩む）。(b) 写しの manifest を `--rules` で渡す（軽いが、誰が許したかを器が確かめず、どこにも残らず、効く範囲が command 単位で、§2.1 のとおり lens には届かない）。
- 欲しいのは、user の許しを器が確かめ、作業 1 つに紐づけ、自然に切れ、作成と使用が見える「つまみ（ノブ）」である。設定 file の書き換えも binary の作り直しも要らない形にする。

やさしく言うと: 「この 1 件だけ、上限を 15 万から 25 万に上げてよい」と user が言ったら、AI が器に記録させ、器はその 1 件の検査のときだけ 25 万で数える。ほかの作業は 15 万のまま。期限か着地で勝手に元へ戻り、いつ誰の言葉で上げたかが一覧に出る。

## 2. 何が起きているか（main ff924036・読むだけ・host の event log と run dir を数えた）

### 2.1 今日の便

- 1 回目の gate（00:07Z）は `crates/scribe2/src/pipe/gate.rs` の `decide` が `token_cap` と本文の byte を比べて INCONCLUSIVE にした。
- `pipe gate --rules`（写しの 250000）で撃ち直した 2 回目（00:23Z）も INCONCLUSIVE で、理由は「lens の verdict に findings が無い（evidence: diff exceeds cap）」だった。gate の照合は写しで通ったが、lens は別の process で cap を読み直す（`crates/scribe2/src/headless/lens.rs` の `rows_of` → `prompt_of` の `over`）。lens の cmd は run dir の写しの 1 行で `--rules` を持たないので、埋め込みの 150000 で再び断った。**同じ行の読み手が 2 つの process に割れていて、`--rules` は片方にしか届かない**（`s2-07l.272` が lens の cap を argv から rules 行へ寄せて塞いだ割れ〔実測は `.265` / `.267`〕と同じ型が、上書きの経路にだけ残っている）。
- 着地の経路（同じ日の実測）: 席は権能 merge を持たないので `pipe land --rules` を撃てず（役割の guard が断る）、着地は列の手動の 1 周に写しの `--rules` と `--lens` を渡して便を再開させる形しか無かった。列は渡された道具を、起こす便と起こし直す便の全部へそのまま写し（`crates/scribe2/src/pipe/dispatch/candidates.rs` の `tools`・道具の受け渡しは全部か皆無か）、写された便は自分の終端の周の列にも同じ道具を渡す。上げた上限がほかの便へ漏れないよう、起こされうる契約 7 本を hold にしてから回した。`--rules` の応急の形は、作業 1 つに絞れないことがここでも現れる。
- 同じ便の消費の 4 値の和は約 6,531 万（runner 約 5,826 万・審査 約 705 万）で、R-C6-1 の 2,500 万を越えている。`pipe show` は超過の 1 行を出すが、便は止まらない（gate-cost.md §43 の形 4「断らない」）。runner の turn は 362 で、turn の上限は無い（`crates/scribe2/src/headless/mod.rs` の `max_turns` は runner と lens では None）。

### 2.2 母集団の測り

- 便ごとの消費: `RunCost` を持つ便 524 本のうち 4 値の和が 2,500 万以上は 13 本（中央値 約 223 万・上位 1 割の境 約 1,346 万・最大は今日の便）。
- gate の予算の照合で止まった便: run dir 1003・verdict の写し 545 本のうち、evidence が「byte が cap を超えた」の形は 5 本（178302〜792551 byte・bead 4 つ）。撃ち直しで verdict の写しは上書きされるので下界（今日の便は lens の断りに上書きされて数に入らない）。

### 2.3 rules 行の分け（85 行・`rules/manifest.toml`）

| 分け | 行数 | ノブの対象か |
|---|---|---|
| 消費・作業ごと（便か bead 1 つの消費を縛る） | 7 | 対象（MVP は 1 行） |
| 消費・host と群で分け合う（1 つを上げるとほかの作業の取り分が減る） | 18 | 対象外（「ほかの作業の上限は変えない」に反する） |
| 構造（C4・C13・C12 の門と台帳の形） | 20 | 対象外（A2 の裁定事項・CI は main の manifest で測る） |
| その他（権能・安全の語列・時刻と待ち・model と effort） | 40 | 上限でない |

消費・作業ごとの 7 行:

| 行 | 値 | 何を縛る | 読み手 | 効く場所 |
|---|---|---|---|---|
| `gate.token_cap` | 150000 byte | lens に渡す本文の量 | `crates/scribe2/src/pipe/gate.rs` の `Limits` の `of`（呼び手は `crates/scribe2/src/pipe/cli/step.rs` の `gate_run` ほか）→ `decide`。lens 側は `crates/scribe2/src/headless/lens.rs` の `rows_of` → `prompt_of`・`memo` | gate の予算の照合・gate の lens・契約の審査の lens・先撃ちの lens・memo の lens |
| `R-C6-1` | 25000000 token | 便の消費の 4 値の和（検出線） | `crates/scribe2/src/pipe/cli/show.rs` の `ceiling_of` → `ceiling_line` | `pipe show` の表示だけ（受付・spawn・gate・land は読まない） |
| `review.same_kind_stop` | 2 便 | 同じ型の審査 FAIL の繰り返し | `crates/scribe2/src/pipe/cli/intake.rs` の `exclude_repeats`（断り文は `crates/scribe2/src/pipe/refuse.rs`） | 受付・事前審査 |
| `pipe.follow_retries` | 2 回 | 追随の撃ち直し | `crates/scribe2/src/pipe/cli/step.rs` の `land_run` | land の追随 |
| `pipe.land_wait_s` | 5400 秒 | 着地の順の待ち | 同じ `land_run` | land |
| `pipe.ci_wait_s` | 900 秒 | 終端の CI の照合の待ち | 同じ file の `terminal_input` | land の終端 |
| `gate.slot_wait_s` | 5400 秒 | 受付札と遮断器の待ち | `Limits` の `admission` と `breaker` | gate・land の主実測（列の遮断器 `crates/scribe2/src/pipe/dispatch.rs` は host 全体で同じ行を読む） |

gate-cost.md §43 は `gate.token_cap` を「diff の byte の上限で消費ではない」と書いて R-C6-1 の検出線から外した。あれは消費の**測り**でないという意味で、上限の効き（lens に渡す量 ≒ token を縛る）は消費の側なので、本 doc では消費・作業ごとに数える。

消費・分け合う 18 行（読み手 / 効く場所）: `pipe.max_live`（intake.rs / 受付）・`land.train_max`（step.rs / 着地の列）・`pipe.precheck_lens_per_round`（dispatch/prelens.rs / 列の周）・`gate.mutants_jobs`・`gate.job_memory_mb`・`host.reserve_memory_mb`（gate.rs と confine.rs と dispatch.rs / 受付札）・`gate.cpu_weight`（confine.rs / 箱）・`host.runnable_per_core`・`host.blocked_per_core`（gate.rs / 遮断器）・`gate.tmux_test_threads`（xtask の limits.rs と check_facts.rs / nextest の写しの突合）・`seat.memory_max_mb`（confine.rs / 席の箱）・`seat.drafts_cap_mb`（sweep.rs / 置き場の量）・`R-C9-1`（seat/mod.rs と fleet / 口座選定）・`fleet.group_pressure_5h_pct`・`fleet.group_pressure_7d_pct`・`fleet.group_pressure_model_pct`（hook/group.rs / 群の逼迫）・`pipe.ci_poll_s`（step.rs / forge の API の周期）・`floor.timeout_s`（dispatch/floor.rs / main の床の検査）。

構造の 20 行（読み手 / 効く場所）: `R-C4-1`・`R-C4-2`・`R-C4-3`・`R-C4-4.fn-lines`・`R-C4-4.complexity`・`R-C4-4.args`・`R-C4.line-width`・`R-C4-5`（xtask の limits.rs・check_sizes.rs・check_facts.rs・prose_gate.rs / CI の門。R-C4-1・R-C4-2・R-C4.line-width は受付の上限の余地も `crates/scribe2/src/pipe/cli/intake/refusal.rs` で読む）・`R-C13-1`・`R-C13-1.per-pr`・`R-C13-1.check-delta-ms`・`R-C13-2`・`R-C13-3`（xtask の limits.rs / CI・後の 2 行は id で読む読み手が無い）・`pipe.size_s_lines`・`pipe.size_m_lines`・`pipe.size_l_lines`（refusal.rs / 受付の余地の見積）・`flip.docs_only_faces`・`flip.marks_per_pr`（xtask の limits.rs / 入口の flip check）・`R-C12-1`（xtask の mutantsdiff.rs / 不発効の検出線）・`ledger.open_children_max`（ledger/graph.rs / doctor）。

spawn には消費の上限の読み手が今は 1 つも無い（runner の token も turn も縛らない）。user が挙げた「turn の上限」は rules 行として存在しない。

### 2.4 user の許しを器が確かめる今の仕組み

| 仕組み | 確かめること | AI が偽れるか | 作業ごとに効くか |
|---|---|---|---|
| `pipe approve`（承認 event・Blocked の段・FR15 / FR16・`crates/scribe2/src/pipe/approve.rs`） | 席が `--words` で渡した字が空でないこと。replay は actor human ∧ 逐語が非空で承認済みにする | 偽れる。逐語は席が渡す字で、発話 event と照らさない。`fleet record` も便の形の kind を `--actor human` で書ける | 便ごと |
| 発話の記帳（`crates/scribe2/src/hook/utterance.rs`・FR82）と `seat ruling bind`（`crates/scribe2/src/seat/ruling.rs` の `bind`） | 発話は hook が prompt の入口で逐語のまま書く。bind は記帳済みの発話の ts と開いた台帳の問いを結び、逐語は event から写す（席は逐語を渡せない） | 普通の口では偽れない。穴は 3 つ（§8 の H1〜H3）: hook の口を席の Bash から直に撃つ形を止める guard が grep で見当たらない・event log は置き場の file で席の権能 edit-outside の内側・どの発話を結ぶかは席が選ぶ（問いは発話の後に起こしてもよい） | 問いごと。上限の値には効かない |
| 答えの口（`seat ruling answer`・裁定面） | 逐語を標準入力で受け、経路 gui の発話として書いて結ぶ | 席の道具の呼び出しは `crates/scribe2/src/hook/answer_mouth.rs` が字面で断る（script の file を経る形は測っていない） | 問いごと。承認 event は書かない（C7・ADR-0087） |
| 裁定 id の実在（vessel 宣言の ruling-check・FR83） | 追跡された file の字面の裁定 id が台帳の裁定の行に解けること | 引用の実在だけで値は照らさない。本 repo の `.vessel.toml` は ruling-check を持たない（未 opt-in） | 文書の字面の検査で作業ごとでない |
| `pipe show` の `cost-ceiling: over` | 便の消費の 4 値の和と R-C6-1 | 許可でなく表示 | 便ごとに出すが止めない |
| `--rules` の差し替え（`crates/scribe2/src/pipe/cli/args.rs` の `manifest_of`・headless の `rules_of`） | 渡した file を丸ごと読む（loader は ruling 欄の有無だけを見る） | 完全に偽れる。席が写しを書いて渡せ、`pipe gate` は役割の guard の表（`crates/scribe2/src/hook/role_guard.rs` の `CAPABILITY_COMMANDS`）に無い。どの manifest を読んだかを event に残さない | command 単位。lens には届かない（§2.1）。列の 1 周に渡すと広がりすぎる（§2.1 の後段） |
| regate（FR77） | Gated ∧ FAIL ∧ 所有者の印 ∧ 逐語が非空 | 逐語は席が渡す | 便ごと。INCONCLUSIVE は対象外 |

### 2.5 憲法と ADR

- C1: 規則の値は manifest 1 file・実行時はそこからだけ読む。→ 許可は「値の宣言」ではなく、承認 event が便ごとに 3 クラス操作を開けるのと同じ実行時の記録として読む解釈が要る（ADR の条件 1）。
- C5: rules 行の変更は裁定 id を要る。→ 許可は行を変えない。新しく足す 2 行（§6）は新しい裁定 id を要る。
- C6.2: 便が R-C6-1 を越えたら器が止める、と書くが、現物は表示だけ（gate-cost.md §43 形 4・停止は別の裁定）。
- C7 / C7.2 / A1 / A4.2: 承認は対話面 1 つ（R-C7-1 = orchestrator）からだけ受け、逐語つきの承認 event で記録する。有料の使用は不可逆として先に聞く。→ 上げの許しは対話面の chat の経路の裁定に限る（裁定面の gui の経路は承認の受理面でない・ADR-0087）。subscription の窓の token が A1 の「使う」に入るかは解釈が要り、本 doc は保守側（入る）で読む。
- C10: 宣言値・実測値・導出値・実効値を型で分ける。→ 許可の値は「裁定を出所に持つ宣言値」として別の型で持ち、manifest の値を上書きした形にしない。
- A2: C4 と C13 の閾値の変更は grill と裁定 id を要る。→ 構造の行は作業ごとの許可の対象にしない。
- C17.2: 仕組みを足すなら消す物を先に名指す（§11）。
- ADR: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html)（event log は JSONL 1 file・跨版の面）・[ADR-0005](../../design-intent/decisions/ADR-0005-a3-approval-surface-c7.html)（承認の経路は C7 の対話面）・[ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html)（gate の費用）・[ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)（同時の便の本数は rules 行 1 本）・[ADR-0037](../../design-intent/decisions/ADR-0037-rulings-without-a-run-are-approval-events.html)（run 無しの裁定も承認 event）・[ADR-0083](../../design-intent/decisions/ADR-0083-rulings-bind-ledger-questions-to-recorded-utterances.html)（裁定は記帳した発話に結ぶ）・[ADR-0087](../../design-intent/decisions/ADR-0087-user-utterances-are-sorted-three-ways-and-seat-questions-go-to-the-ledger.html)（答えの口と C7.2 の読み）・[ADR-0093](../../design-intent/decisions/ADR-0093-publish-exclusions-sit-verbatim-in-the-host-face.html)（裁定つきの例外を host の面に置いた前例）。索引は `design-intent/decisions/README.html`。

## 3. 決めること

1. 対象の線: どの行を作業ごとに上げてよいか。
2. 照合: 器が「user が許した」をどの記録で確かめるか（どの経路・どの event）。
3. 単位と切れ方: bead か便か。期限・着地・取り消し。
4. 上限なし（外す）を許すか。
5. 置き場: event log・state dir の別 file・host の面のどれか。
6. 読み手の差し替え点と、lens（別 process）への渡し方。
7. 見え方: 作成と使用をどこに出すか。
8. 偽れる穴の塞ぎ方。

## 4. 推奨の形

### 4.1 流れ

1. gate が上限で INCONCLUSIVE に止まる（今と同じ）。
2. 席が台帳の問い（effect は operation・FR81 の 4 行）を立て、本文に bead・rules 行・上げる値・期限を字のまま書いて user に説明する。
3. user が chat で答える。発話の hook が逐語を記帳する。
4. 席が `seat ruling bind` で問いと発話を結ぶ（裁定 id が出る・今の口のまま）。
5. 席が許可の口（§5）に bead・行・値・期限・裁定 id を渡す。器が §4.3 を照らし、通れば上限の許可の event を 1 件書く。
6. 席がその便を撃ち直す（今の手のまま）。gate の 2 つの読み手（予算の照合と lens）が許可の値を読む。ほかの bead の便は manifest の値のまま。
7. 期限・bead の着地・取り消しで切れる。作成と使用は §4.5 に出る。

effect が operation の問いなので、未反映の裁定（FR84）に数えない（文書へ写す裁定でない）。

### 4.2 対象の線

| 分け | 許可の対象 | 理由 |
|---|---|---|
| 消費・作業ごと（7 行） | 型で「対象になりうる」と閉じ、manifest の新しい行が「今対象にする」行を名指す。MVP の値は `gate.token_cap` の 1 要素 | 作業 1 つの消費だけが変わる。ほかの作業の上限は動かない |
| 消費・分け合う（18 行） | 対象外 | 1 つを上げるとほかの作業の取り分（memory・core・同時の本数・口座の窓・forge の API）が減る |
| 構造（20 行） | 対象外 | A2 の grill と裁定 id を要る閾値。CI の門は main の manifest で測るので、作業ごとの許可は CI に効かず、受付の余地と CI が食い違うだけになる |
| その他（40 行） | 対象外 | 上限でない |

型の分けは `crates/scribe2/src/rules/mod.rs` の `RuleKind` に網羅の match を 1 本足して持つ（`shape` と同じ置き方・wildcard を書かない＝kind を足した周にどの分けかを決めないと compile が落ちる・C2）。`R-C6-1` は C6.2 の停止を入れる設計の後に manifest の対象の行へ足す（今は表示だけなので上げても何も変わらない）。

### 4.3 照合（器が確かめること・判定の順）

許可の口は次を上から順に照らし、1 つでも外れる周は何も書かずに理由の語で断る（C2: 1 つの関数が 1 つの閉じた enum の宣言順で判じる）。

1. 行が manifest の対象の行に在る（rule-not-listed）。
2. 値が manifest の値より大きい有限の整数（not-raise）。
3. 期限が今より後で、裁定の発話の時刻から rules 行の期限の上限の内側（bad-until）。
4. 裁定 id が問い id の形で、その id の裁定 event が在る（no-ruling）。
5. 裁定の発話の経路が chat で、発話の session が `--repo` の anchor の orchestrator の登録 row の sid と同じ（not-surface・C7）。sid を持たない row の周も断る（fail-closed）。発話 event の session と登録 row の sid が同じ字で並ぶかは行 c で実測する。
6. 発話 event が在り actor が human（no-utterance）。
7. 発話が問いの起票より後（before-question・裁定 event の question_ts と比べる）。古い「よい」を新しい問いへ結ぶ形を断る。
8. 同じ裁定 id か同じ発話を引く許可がまだ無い（reused）。1 つの発話を複数の問いへ結べる（FR82）ので、許可の側で 1 対 1 に絞る。
9. 問いの本文が bead・行 id・値を字のまま持つ（not-stated）。user が見せられた字と許可の字を揃える。
10. bead の便がまだ Landed に達していない（landed）。

1〜8 と 10 は event log と manifest だけで判じる。9 だけが台帳を読む（`crates/scribe2/src/ledger/mod.rs` の `show` が今は本文を持たないので、`Bead` に本文の欄を足す）。

### 4.4 単位と切れ方

- 単位は bead（契約）。同じ契約の撃ち直し（追随・再 gate・新しい便）は同じ大きさの diff に何度も当たるので、便単位だと毎回 user を呼ぶ。
- 切れるのは次のどれか: 期限を過ぎた・bead の便が Landed に達した・同じ bead と行の新しい記帳が在る（新しい方が勝つ）・取り消しの記帳が在る。取り消しは締める向きなので裁定を要らない。
- 上限なし（外す）は持たない。有限の値への上げだけ（§13 の論点 1）。

### 4.5 見え方

- 作成: 上限の許可の event 1 件（§6）と、口の 1 行（§5）。
- 使用: 許可の値を読んだ gate の周は、Gated の段の event の detail に `permit:<行>=<値>` を足し、INCONCLUSIVE の文は効いた値と裁定 id を名指す（例「diff N byte が cap M（上限の許可 <裁定 id>）を超えた」）。
- 一覧: `pipe show --run` に bead の許可の 1 行（状態の閉じた語 active / expired / landed / revoked / superseded）。`pipe dispatch ls` に効いている許可を 1 行ずつ。
- 局面の出力（[case-lifecycle.md](./case-lifecycle.md)）へ欄を足すのは後の行（版の欄を上げる変更で、読み手の約束がある）。

## 5. 口の形

```
<NAME> pipe permit --bead <bead> --rule <行 id> --value <整数> --until <UTC の分> --ruling <裁定 id> [--state-dir D] [--repo R]
<NAME> pipe permit --bead <bead> --rule <行 id> --revoke [--state-dir D] [--repo R]
```

- 通る周（rc 0）: `permit: bead=<bead> rule=<行 id> value=<値> declared=<manifest の値> until=<UTC> ruling=<裁定 id>`。取り消しは `permit: bead=<bead> rule=<行 id> revoked`。
- 断る周（rc 1・何も書かない）: `pipe: permit refused reason=<語> <名指し>`。語は §4.3 の 10 語の閉じた列。
- 読めない周（rc 2・何も書かない）: event log・manifest・台帳のどれかが読めない（fail-closed）。
- 権能: 役割の guard の表に許可の口の行を足し、既存の権能 approve を当てる（新しい権能の名を足さない＝行 `role.orchestrator` は変えない）。
- 記帳の直後に列の 1 周を撃つかは、承認の記帳（`crates/scribe2/src/pipe/cli.rs` の `GATES`）と揃えるかを契約表の行で決める。MVP は撃ち直しを席の今の手に残す。

## 6. on-disk の形（schema・置き場・跨版）

- **置き場は event log だけ**（C3・C6.3 の 1 つの追記の store）。state dir に別の file を足さない（真実を 2 か所に置かない）。host の面（host.toml）は host 固有の値の面で、作業の id を書くと面が毎日動くので使わない。
- **event**: kind を 1 つ足す（名の案 LimitPermitted）。形も 1 つ足す（案: `bead` と `detail` が必須・`run`・`stage`・`seat`・`pid`・口座残量・登録・列の印の key を持たない＝`Pressure` の形と同じく本体は detail の 1 行）。detail は閉じた key の列 `rule=<id> value=<n> until=<ts> ruling=<id>`、取り消しは `rule=<id> revoked`。actor は machine（人の言葉は結んだ裁定 event が持ち、許可はその機械の導出）。`Event` の struct に欄を足さない（literal の site を全部触る write-set を避ける）。schema は 1 のまま kind と形を足す。
- **run dir の写し**: gate は lens を起こす直前に、契約の写しの隣へ効く cap の値と出所（manifest か裁定 id）の 1 行を書く（裁定の写しと同じ置き方・`lens.cmd` の穴は不変）。lens はこの写しが在る周だけそれを cap に読み、無い周（契約の審査・先撃ち・memo）は今のとおり manifest。写しは毎周書き直す（許可が無い周は manifest の値を書く）。
- **rules 行を 2 本足す**（manifest の TOML subset・kind 2 つ・各行に新しい user 裁定 id・C5）: 対象の行の列（案 `pipe.permit_rows`・List・loader は消費・作業ごとに分けられた kind の行 id でない要素を断る・MVP の値は `gate.token_cap` 1 つ）と期限の上限（案 `pipe.permit_max_h`・Int・時間）。
- **跨版**: 古い binary は未知の kind を malformed として断る（NFR4）。最初の許可を書く前に PATH の binary の入れ替えが要る。許可を書かない限り log の字は 1 byte も変わらない。

## 7. 読み手の差し替え点

| 行 | 今の読み手 | 差し替え | MVP |
|---|---|---|---|
| `gate.token_cap`（gate の照合） | `crates/scribe2/src/pipe/cli/step.rs` の `gate_run` と `land_run`（追随の再 gate）が `Limits` の `of` で読み、`crates/scribe2/src/pipe/gate.rs` の `decide` が比べる | `gate_run` と `land_run` で `Limits` を組んだ直後に bead の許可を重ねる（`Gate` は既に `bead` を持つ）。`of` は manifest だけを読む 1 本のまま（`crates/scribe2/src/pipe/dispatch.rs` の遮断器と `crates/scribe2/src/pipe/cli/base_run.rs` も同じ 1 本を使うので触らない） | 入れる |
| `gate.token_cap`（gate の lens） | `crates/scribe2/src/headless/lens.rs` の `rows_of` → `prompt_of` | §6 の写しが在れば読む分岐（`rulings_of` と同じ形）。書き手は gate.rs の `keep_rulings` の隣 | 入れる |
| `gate.token_cap`（契約の審査・先撃ち・memo の lens） | 同じ `rows_of` | 写しが無いので manifest のまま | 入れない |
| `R-C6-1` | `crates/scribe2/src/pipe/cli/show.rs` の `ceiling_of` | 停止の設計の後に対象の行へ足し、表示の limit を許可の値にする | 入れない |
| `review.same_kind_stop` | `crates/scribe2/src/pipe/cli/intake.rs` の `exclude_repeats` | 対象の行へ足す裁定の後の行 | 入れない |
| `pipe.follow_retries`・`pipe.land_wait_s`・`pipe.ci_wait_s` | step.rs の `land_run`・`terminal_input` | 同上 | 入れない |
| `gate.slot_wait_s` | `Limits` の `admission`・`breaker` | 同上（遮断器の読みは host 全体のまま） | 入れない |

後の行が呼ぶ口（この § が祖先）: 純関数 1 本（名の案 permitted・crate の内側の可視性）。入力は行 id・manifest の値・bead・event 列・今の時刻、返りは閉じた 2 値（宣言のまま / 許可〔値・裁定 id・期限〕）。置き場の file は実装の行が決める。event 列を読めない周は呼び手が今のとおり gate を止める（許可へ倒す枝は持たない）。

## 8. 偽れる穴と塞ぎ方

器の照合は「記帳された発話が在る」ことを根拠にする。普通の口（bind・答えの口）では席が逐語を渡せないので偽れないが、次の穴が残る。

| 穴 | 今 | 塞ぎ方（推奨） | 残り |
|---|---|---|---|
| H1 hook の口の直撃: 席の Bash が `<NAME> hook` の発話の記帳の枝へ作った payload を流す | 止める guard が grep で見当たらない | 行 a: 席の道具の呼び出しが器の hook の subcommand を撃つ segment を、答えの口と同じ形（answer_mouth.rs）で断る | script の file を経る形は字面で測れない |
| H2 event log の直書き: Edit / Write か Bash の redirect で置き場の log へ行を足す | 席の権能 edit-outside の内側 | 行 a: 置き場の log の path を名指す Edit / Write と redirect を断る | python などの子 process の書きは測れない |
| H3 無関係の発話を結ぶ | bind は席が発話を選ぶ | §4.3 の 5・7・8（同じ session・問いより後・1 対 1） | 問いの後に user が別の話題で「よい」と言った発話を結ぶ形は測れない。逐語は許可から引けるので事後に見える |
| H4 問いの本文は席が書く | — | §4.3 の 9（本文と許可の字を揃える） | user が本文を読んだかは測れない |
| H5 `--rules` の差し替え | 権能も記録も無い | 行 a: 席が `pipe gate`・`pipe land`・`pipe resume` に `--rules` を渡す形を断る。gate の Gated の detail に読んだ manifest の出所（埋め込みか path の sha）を書く | 席の手動の dispatch の 1 周が起こす便へ写す `--rules` は断りの外（今の撃ち方を壊さないよう、行 a で現物の形を測ってから決める）。出所の記録で跡は残る |
| H6 承認 event の偽造（`pipe approve`・`fleet record --actor human`） | 席が字を渡せる | 許可の根拠にしない（裁定 event と発話 event だけを読む） | 便の承認の口そのものの穴は本 doc の外 |

根の限界: 置き場は AI と同じ OS の user が書ける場所なので、改ざんを不可能にはできない（別の OS の user か署名が要る・§15）。器が保証するのは「普通の口では偽れない・偽った跡は残る・許可は一覧に出る」までである。

## 9. SRS に足す要件の案（id は今の最大の次から）

- **FR110 上限の許可の記帳**（event・priority must・vmethod T）
  - 条件: 席が器の許可の口に bead・rules 行・値・期限・裁定 id を渡して撃ったとき、または bead と rules 行に取り消しを渡して撃ったとき
  - 応答: 器は、行が許可の対象の行の列に在り、値が manifest の値より大きい整数で、期限が今より後かつ裁定の発話から期限の上限の内側にあり、裁定 id が問い id の形で裁定 event が在り、その発話が対話の session の経路で対話面の席の session の発話 event（actor が human）で、発話が問いの起票より後で、同じ裁定 id と同じ発話を引く許可がまだ無く、問いの本文が bead と行と値を字のまま持ち、bead の便がまだ着地していない周だけ、上限の許可の event を 1 件記帳して 1 行を返す。どれかが外れる周は何も書かずに理由の語を名指して断る。取り消しは裁定 id を要らず、同じ bead と行の許可を今の時刻で切る event を 1 件書く
- **FR111 上限の許可の効き**（state・must・T）
  - 条件: bead に効いている上限の許可（期限の前・bead の便が未着地・同じ bead と行の最新の記帳が取り消しでない）が在る間
  - 応答: 器は、その bead の便の gate の予算の照合と gate の lens が許可の値を cap に読み、ほかの bead の便・host 全体の読み・CI の門は manifest の値を読む。許可の値を読んだ gate の周は段の event と判定の文に行と値と裁定 id を書き、`pipe show` と `pipe dispatch ls` は効いている許可を 1 行ずつ出す
- **FR112 発話の記帳の口と event log の書きの遮断**（unwanted・must・T）
  - 条件: 席の道具の呼び出しが器の hook の subcommand を撃とうとするとき、置き場の event log の file を書こうとするとき、または `pipe gate`・`pipe land`・`pipe resume` に `--rules` を渡そうとするとき
  - 応答: 器の hook の入口が実行の前に理由の語を名指して断る
- 字を直す要件: FR9（「予算 cap」を「効いている cap〔FR111〕」に）・NFR1（測りに許可の id を足す）・FR41（許可の口を権能 approve の口に数える）。
- 受入基準の案:
  - AC84（FR110）: 10 の断りの語の各 fixture で event が 0 件・語の名指し 10/10、通る fixture で event 1 件と 1 行、取り消しで 1 件、同じ発話の 2 度目は reused を歯が nextest で確かめる。
  - AC85（FR111）: bead 2 つの fixture で、許可を持つ bead の便は gate の照合と lens の写しの cap が許可の値、持たない bead の便は manifest の値で同じ diff が INCONCLUSIVE、期限・着地・取り消しの後は manifest の値に戻ることを歯が確かめる。
  - AC86（FR112）: hook の subcommand・log の path への Edit / Write / redirect・`--rules` の 3 形が断られ、log を読むだけの command と `--rules` を持たない gate が通ることを歯が確かめる。

## 10. ADR

- 決定: [ADR-0106](../../design-intent/decisions/ADR-0106-limit-permits-raise-a-named-cap-for-one-bead-on-a-bound-ruling.html)（proposed・下の決定の文の案 5 つを 1 本の決定にまとめた。rules 行 2 本の値の裁定 id は SRS の round で取る）。

ADR を書く条件の 1（C1・C7 / A4.2・C10 の解釈）・3（event の kind と形・run dir の写し・rules 行の kind 2 つ・跨版）・4（却下の分岐）に当たる。実装の前に ADR を land し、同じ PR で語彙と decisions の索引を直す。決定の文の案:

1. 消費の上限の行のうち rules 行が名指す行だけを、bead 1 つに限って有限の値へ上げる上限の許可を、event log の新しい kind 1 つで持ち、manifest の値は変えない（C1 の読み: 値の宣言は manifest、許可は承認 event と同じ実行時の記録）。
2. 許可は対話面の chat の経路で記帳された user の発話に結んだ問い id の形の裁定 id を要り、器の口が発話の実在・同じ session・問いより後・未使用・問いの本文の字を照らしてから記帳する。席が字を渡す承認（`pipe approve`・`fleet record`）は根拠にしない。
3. 許可を読むのは名指された bead の便の gate の予算の照合と lens だけで、host と群で分け合う上限と構造の上限（C4・C13）は対象にしない。
4. 許可は期限（rules 行が上限を持つ）・bead の着地・同じ bead と行の新しい記帳か取り消しで切れ、作成と使用は event・`pipe show`・`pipe dispatch ls` に出る。
5. 前段として、席の道具の呼び出しが器の hook の口を直に撃つ形・event log を書く形・gate と land に `--rules` を渡す形を器の hook が断る。

## 11. 足す物・消す物（C17.2）と決定の梯子（C17）

- 足す: event の kind と形 1 つずつ・許可の口 1 つ・rules 行 2 本・`RuleKind` の分けの match 1 本・run dir の写し 1 つ・hook の断り 1 本。
- 消す: 上げの PR → binary の入れ替え → 戻しの PR の対（前例 2 組）と、席が `pipe gate`・`pipe land`・`pipe resume` に写しの manifest を `--rules` で渡す応急の形（H5 の断りで消える。列の手動の 1 周に渡す `--rules` は H5 のとおり断りの外で、行 a で現物の形を測ってから決める）。
- 梯子: 要るか（要る・前例 2 組と今日の 1 件・`--rules` は lens に届かない）→ 既に在るか（承認・裁定の結び・`--rules` は在るが、どれも「確かめた user の許し × 作業 1 つ × 値」を持たない）→ std と既存の部品で足りるか（足りる・依存を足さない・NFR3）→ 1 行で済むか（済まない・読み手が 2 process に割れている）→ 最小の実装（MVP は `gate.token_cap` の 2 読み手だけ）。

## 12. 契約表の行の見込み（SRS の round と ADR の後に書く）

| 行 | 中身 | src の見積 | 順 |
|---|---|---|---|
| a | hook の断り（H1・H2・H5）と gate の detail の manifest の出所 | 200〜300 行 | 先頭（単独でも価値がある） |
| b | `RuleKind` の分けの match・rules 行 2 本と kind 2 つ・event の kind と形・純関数 1 本 | 250〜350 行 | a の後 |
| c | 許可の口（照合 10 語・`Bead` の本文の欄・権能の表の行・help と snapshot） | 300〜450 行 | b の後 |
| d | 読み手（`gate_run`・`land_run` の重ね・`decide` の文・Gated の detail・run dir の写しの書き手と lens の読み手） | 150〜250 行 | b の後（c と write-set が交わらなければ並べられる） |
| e | 見え方（`pipe show` の行・`pipe dispatch ls` の行） | 100〜200 行 | c・d の後 |

- 全行が 550 行以内の見積。b は閉じた型（`EventKind`・形・`RuleKind`）に変種を足すので、別 doc の既存の行の閉包が広がる。起票の前に `pipe preflight` で受付の断りを測る。
- rules 行 2 本の値は新しい user 裁定 id を要る（base の ruling の字は使い回せない）。最初の許可の前に binary の入れ替えが要る（§6）。

## 13. 価値観の論点と裁定

1. **上限を外す（上限なし）を許すか** — 裁定 user 2026-10-01T00:38Z: 許さない。有限の値への上げだけ。見ていない間の大量消費を防ぐという動機と真っ向からぶつかり、問いの本文に数を書かせることで user が数を見て「よい」と言える形になる。大きく上げたいときは大きい数を書く。
2. **許可の単位と期限の長さ** — 推奨どおり（推奨で進める既定の裁定 user 2026-09-28T00:54Z）: bead 単位で、期限の上限は rules 行 1 本・着地で自動に切れる。便単位だと同じ契約の撃ち直しのたびに user を呼ぶ。rules 行 2 本の値（対象の行の列と期限の上限）は C5 の新しい裁定 id で決めた: 対象の行の列は `gate.token_cap` の 1 要素（裁定 user 2026-10-01T01:05Z 項 permit-rows）、期限の上限は 24 時間（裁定 user 2026-10-01T01:24Z 項 permit-max-h・効くのは許可を出した bead の名指した上限だけ）。
3. **R-C6-1 を本当の停止にするか（C6.2 の字どおり）** — 推奨どおり（既定の裁定 user 2026-09-28T00:54Z）: ノブが着地した後に、同じ epic の別の設計で入れる。今の便の 2.5%（524 本中 13 本）が線を越え、今日の便は 2.6 倍で、止めるなら同時にノブで上げられる必要がある。動機（見ていない間の消費）に一番効くのは gate の cap でなく runner の消費の停止である（§2.1・§2.2）。

## 14. 却下した案

- **manifest を PR で上げて戻す（今の前例）**: 重い・上げている間はほかの便も緩む・binary の入れ替えが要る。
- **`--rules` の写しを正式の口にする**: user の許しを確かめず記録も残らない。読み手が 2 process に割れ、lens に届かない（今日の実測）。
- **host の面（host.toml）に作業ごとの例外の表を置く（ADR-0093 の型）**: host 固有の値の面に作業の id が毎日出入りする。期限と着地で切る読み手を面の読みに足すことになり、event log と真実が 2 か所になる。
- **state dir に許可の別 file を置く**: C3 の 1 つの store に反し、作成と使用の順序が log と食い違いうる。
- **`pipe approve` の承認 event を根拠にする**: 逐語は席が渡す字で、発話 event と照らさない（H6）。
- **裁定面（gui の経路）の答えを根拠に加える**: 答えの口は承認の受理面でない（C7・ADR-0087）。対話面を 2 つにする解釈は憲法の改訂の側。
- **構造の上限（C4・C13）も作業ごとに上げる**: A2 の裁定事項で、CI は main の manifest で測るので許可が CI に効かず、受付の余地と CI の門が食い違う。
- **user の発話に値の数字を含めることを求める**: 照合は強くなるが、「よい」だけで撃てる形を求めた要望に反する。H3 の残りを詰める次の手として残す。

## 15. 限界

- 改ざんを不可能にはできない（§8 の根の限界）。器が持つのは普通の口の閉じ・跡・一覧まで。
- MVP は gate の 2 読み手だけ。契約の審査・先撃ち・memo の lens と、`review.same_kind_stop` などほかの作業ごとの行は後の行。
- user が問いの本文を読み、何に「よい」と言ったかは測れない（H3・H4）。
- runner の消費は縛らない（R-C6-1 は表示だけ・turn の上限は無い）。動機に一番効く守りは §13 の論点 3 の側で、同じ epic の次の設計が持つ。
- 撃ち直しは席の今の手のまま（許可の記帳が便を自動で測り直す形は持たない）。

## 16. 語彙

- 新しい語の canonical 名の案: **上限の許可**（aliases: limit permit）。定義の案: 消費の上限の rules 行 1 本を bead 1 つに限って有限の値へ上げる記録。対話面の chat の経路の発話に結んだ裁定 id を要り、期限・着地・取り消しで切れる。event の kind（案 LimitPermitted）と許可の口（`pipe permit`）はこの語の別名として書く。
- 既存の語に合わせる: 裁定 id・問い id の形・発話 event・裁定 event・裁定の行・台帳の問い・effect・発話の経路・対話面・裁定面・答えの口・承認 event・3 クラス・受付・便・検出線。`Allowance` は口座残量の型で使用済みなので許可の訳に使わない。
