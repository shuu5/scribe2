# 設計: 上限の許可 — user の裁定に結んだ「この作業だけ消費の上限を上げる」記録を器が記帳し、その作業の読み手だけが置き換えて読む

- 出所: 2026-10-01 の便 `s2-07l.738.38.7-20260930T224927Z` が gate の予算の照合（diff 215431 byte が cap 150000 を超えた）で Gated INCONCLUSIVE に止まり（verify は全部緑）、user が一時の上げを許し、orchestrator が host の写しの manifest を `--rules` で渡す応急の形を撃った件。前例は manifest を直す PR → binary の入れ替え → 戻しの PR の対（`s2-07l.265` / `.267`・`.375` / `.376`）。SRS の版 0.34 と ADR-0106 の accepted の後（契約表はまだ持たない）。
- 要件（今の字）: [NFR1](../../design-intent/spec/srs.html#NFR1) lens 予算 / [FR9](../../design-intent/spec/srs.html#FR9) lens の verdict / [FR15](../../design-intent/spec/srs.html#FR15)・[FR16](../../design-intent/spec/srs.html#FR16) 承認 / [FR17](../../design-intent/spec/srs.html#FR17) rules manifest / [FR22](../../design-intent/spec/srs.html#FR22) 人手 0 の計測 / [FR41](../../design-intent/spec/srs.html#FR41) 権能の所在 / [FR82](../../design-intent/spec/srs.html#FR82) 発話の記帳と裁定の結び / [FR83](../../design-intent/spec/srs.html#FR83) 裁定 id の実在。足した要件は SRS の版 0.34 の FR110〜FR112・AC84〜AC86 で、FR9・FR17・FR41・NFR1 の字も直した（§9）。
- 前提: 発話の記帳と結び（[fleet-event-log.md](./fleet-event-log.md) §13 / §14・[ADR-0083](../../design-intent/decisions/ADR-0083-rulings-bind-ledger-questions-to-recorded-utterances.html)・[ADR-0087](../../design-intent/decisions/ADR-0087-user-utterances-are-sorted-three-ways-and-seat-questions-go-to-the-ledger.html)）、gate の費用（[gate-cost.md](./gate-cost.md) §43）、対話面（[dialogue-surface.md](./dialogue-surface.md)）。
- この設計から出る契約: §12 の 5 行の見込み。SRS の round（版 0.34）と ADR の accepted の後に契約表を書く。

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
| `gate.token_cap` | 150000 byte | lens に渡す本文の量 | `crates/scribe2/src/pipe/gate.rs` の `Limits` の `of`（呼び手は `crates/scribe2/src/pipe/cli/step.rs` の `gate_run` ほか）→ `decide`。lens 側は `crates/scribe2/src/headless/lens.rs` の `rows_of` → `prompt_of`・`memo` | gate の予算の照合・gate の lens・契約の審査の lens・先撃ちの lens・行の審査の lens・memo の lens |
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
- C10: 宣言値・実測値・導出値・実効値を型で分ける。→ 許可の値は manifest の値の宣言でなく、裁定を出所に持つ実行時の記録として別の型で持ち、manifest の値を上書きした形にしない。gate が読む効く cap は、manifest の宣言値と許可の記録から器が導く実効値の型で持つ。
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
| 消費・作業ごと（7 行） | 型で「許可の読み手を持つ行」だけを対象になりうると閉じ（読み手を持つのは `gate.token_cap` だけ・§7）、manifest の新しい行が「今対象にする」行を名指す。MVP の値は `gate.token_cap` の 1 要素。残る 6 行は、読み手と SRS FR111 の直しを足す後継の決定の後に型へ足す | 作業 1 つの消費だけが変わる。ほかの作業の上限は動かない。読み手の無い行の許可は記帳しても効かないので、記帳の前に断る |
| 消費・分け合う（18 行） | 対象外 | 1 つを上げるとほかの作業の取り分（memory・core・同時の本数・口座の窓・forge の API）が減る |
| 構造（20 行） | 対象外 | A2 の grill と裁定 id を要る閾値。CI の門は main の manifest で測るので、作業ごとの許可は CI に効かず、受付の余地と CI が食い違うだけになる |
| その他（40 行） | 対象外 | 上限でない |

型の分け（許可の読み手を持つ kind か）は `crates/scribe2/src/rules/mod.rs` の `RuleKind` に網羅の match を 1 本足して持つ（`shape` と同じ置き方・wildcard を書かない＝kind を足した周にどの分けかを決めないと compile が落ちる・C2）。`R-C6-1` は C6.2 の停止を入れる設計の後に manifest の対象の行へ足す（今は表示だけなので上げても何も変わらない）。

### 4.3 照合（器が確かめること・判定の順）

許可の口は次を上から順に照らし、1 つでも外れる周は何も書かずに理由の語で断る（C2: 1 つの関数が 1 つの閉じた enum の宣言順で判じる）。記録の在り（3・4）を、その記録の値を読む照合（5〜7）より前に置く。

1. 行が manifest の対象の行に在る（rule-not-listed）。
2. 値が manifest の値より大きい有限の整数（not-raise）。
3. 裁定 id が問い id の形で、その id の裁定 event が在る（no-ruling）。
4. 裁定 event が引く発話 event が在り actor が human（no-utterance）。
5. 裁定の発話の経路が chat（対話の session）で、発話 event の session が `--repo` の anchor の orchestrator の席の会話 id（状態の打刻の file の最後の行）と同じ（not-surface・C7）。打刻の file が無いか空でない行を持たない周と、file の最後の行の会話 id が空か会話 id の形でない周も断る（fail-closed）。NotFound 以外で file を開けない周と、file の最後の行が打刻の形（会話 id の欄の値を除いた行の形）でない周は rc 2（§5）。最後の行は読めた行のうち最後でなく file の最後の行を読む（`last_sid` の前の行へ戻る読みは使わない）。
6. 発話が問いの起票より後（before-question・裁定 event の question_ts と比べる）。古い「よい」を新しい問いへ結ぶ形を断る。
7. 期限が今より後で、発話の時刻から rules 行の期限の上限の内側（bad-until）。
8. 同じ裁定 id か同じ発話を引く許可がまだ無い（reused）。1 つの発話を複数の問いへ結べる（FR82）ので、許可の側で 1 対 1 に絞る。
9. 問いの本文が bead・行 id・値を字のまま持つ（not-stated）。user が見せられた字と許可の字を揃える。
10. bead の便がまだ Landed に達していない（landed）。

1〜4・6〜8・10 は event log と manifest だけで判じる。5 は orchestrator の席の状態の打刻（`crates/scribe2/src/seat/state.rs` の会話 id）も読み、9 だけが台帳を読む（`crates/scribe2/src/ledger/mod.rs` の `show` が今は本文を持たないので、`Bead` に本文の欄を足す）。

### 4.4 単位と切れ方

- 単位は bead（契約）。同じ契約の撃ち直し（追随・再 gate・新しい便）は同じ大きさの diff に何度も当たるので、便単位だと毎回 user を呼ぶ。
- 切れるのは次のどれか: 期限を過ぎた・bead の便が Landed に達した・同じ bead と行の新しい記帳が在る（新しい方が勝つ）・取り消しの記帳が在る。効くのは同じ bead と行の最新の記帳がその許可自身の周だけで、新しい記帳の期限が古い許可の期限より先に切れても古い許可へ戻らない（manifest の値を読む）。取り消しは締める向きなので裁定を要らない。取り消しは行が対象の列に在る周だけ書き（外れる周は rule-not-listed）、効いている許可が無い周も書く。
- 許可の値が今の manifest の値以下の周（manifest の値が後で上がった周）は、許可を読まずに manifest の値を読む（上げの記録で上限を下げない・状態の語は overtaken）。
- 上限なし（外す）は持たない。有限の値への上げだけ（§13 の論点 1）。

### 4.5 見え方

- 作成: 上限の許可の event 1 件（§6）と、口の 1 行（§5）。
- 使用: 許可の値を読んだ gate の周は、Gated の段の event の detail に `permit:<行>=<値> ruling=<裁定 id>` を足し、verdict の記録（INCONCLUSIVE の文を含む）も行・効いた値・裁定 id を名指す（例「diff N byte が cap M（上限の許可 <裁定 id>）を超えた」）。許可の値も越える diff は INCONCLUSIVE（上限なしにしない）。
- 一覧: `pipe show --run` に bead の許可の 1 行（状態の閉じた語。先に当たる語の順に landed / revoked / superseded / expired / overtaken / active。revoked と superseded は、同じ bead と行の次の記帳が取り消しか新しい許可かで分ける）。`pipe dispatch ls` に効いている許可を 1 行ずつ。
- 局面の出力（[case-lifecycle.md](./case-lifecycle.md)）へ欄を足すのは後の行（版の欄を上げる変更で、読み手の約束がある）。

## 5. 口の形

```
<NAME> pipe permit --bead <bead> --rule <行 id> --value <整数> --until <UTC の分> --ruling <裁定 id> [--state-dir D] [--repo R]
<NAME> pipe permit --bead <bead> --rule <行 id> --revoke [--state-dir D] [--repo R]
```

- 通る周（rc 0）: `permit: bead=<bead> rule=<行 id> value=<値> declared=<manifest の値> until=<UTC> ruling=<裁定 id>`。取り消しは `permit: bead=<bead> rule=<行 id> revoked`。
- 断る周（rc 1・何も書かない）: `pipe: permit refused reason=<語> <名指し>`。語は §4.3 の 10 語の閉じた列。
- 読めない周（rc 2・何も書かない）: 記帳の周は照らす前に event log・manifest・台帳・orchestrator の席の状態の打刻を読み、どれかが読めない周（manifest の読みが §6 の対象の閉じで断る周を含む）は照合の順に関わらず rc 2（fail-closed）。取り消しは rule-not-listed だけを照らし、読むのは event log と manifest だけ。
- 権能: 役割の guard の表に許可の口の行を足し、既存の権能 approve を当てる（新しい権能の名を足さない＝行 `role.orchestrator` は変えない）。
- 記帳の直後に列の 1 周を撃つかは、承認の記帳（`crates/scribe2/src/pipe/cli.rs` の `GATES`）と揃えるかを契約表の行で決める。MVP は撃ち直しを席の今の手に残す。

## 6. on-disk の形（schema・置き場・跨版）

- **置き場は event log だけ**（C3・C6.3 の 1 つの追記の store）。state dir に別の file を足さない（真実を 2 か所に置かない）。host の面（host.toml）は host 固有の値の面で、作業の id を書くと面が毎日動くので使わない。
- **event**: kind を 1 つ足す（名の案 LimitPermitted）。形も 1 つ足す（案: `bead` と `detail` が必須・`run`・`stage`・`seat`・`pid`・口座残量・登録・列の印の key を持たない＝`Pressure` の形と同じく本体は detail の 1 行）。detail は閉じた key の列 `rule=<id> value=<n> until=<ts> ruling=<id>`、取り消しは `rule=<id> revoked`。actor は machine（人の言葉は結んだ裁定 event が持ち、許可はその機械の導出）。`Event` の struct に欄を足さない（literal の site を全部触る write-set を避ける）。schema は 1 のまま kind と形を足す。
- **run dir の写し**: gate は lens を起こす直前に、契約の写しの隣へ効く cap の値と出所（manifest か裁定 id）の 1 行を書く（裁定の写しと同じ置き方・`lens.cmd` の穴は不変）。lens はこの写しが在る周だけそれを cap に読み、無い周（契約の審査・先撃ち・行の審査・memo）は今のとおり manifest。写しは毎周書き直す（許可が無い周は manifest の値を書く）。
- **rules 行を 2 本足す**（manifest の TOML subset・kind 2 つ・各行に新しい user 裁定 id・C5）: 対象の行の列（案 `pipe.permit_rows`・List・loader は許可の読み手を持つ kind（§4.2・MVP は `GateTokenCap` だけ）の行 id でない要素を断る・MVP の値は `gate.token_cap` 1 つ）と期限の上限（案 `pipe.permit_max_h`・Int・時間）。
- **跨版**: 古い binary は未知の kind を malformed として断る（NFR4）。最初の許可を書く前に PATH の binary の入れ替えが要る。許可を書かない限り log の字は 1 byte も変わらない。

## 7. 読み手の差し替え点

| 行 | 今の読み手 | 差し替え | MVP |
|---|---|---|---|
| `gate.token_cap`（gate の照合） | `crates/scribe2/src/pipe/cli/step.rs` の `gate_run` と `land_run`（追随の再 gate）が `Limits` の `of` で読み、`crates/scribe2/src/pipe/gate.rs` の `measure`（本文の削除の畳みを撃つかの閾値）と `decide`（予算の照合）が比べる | `crates/scribe2/src/pipe/gate.rs` の `gate`（`gate_run` と追随の再 gate の両方が通る）が `Limits` を受けた直後に bead の許可を読み、効く cap を `Limits` と別の型で持つ（`Gate` は既に `bead` を持つ）。`of` は manifest だけを読む 1 本のまま（`crates/scribe2/src/pipe/dispatch.rs` の遮断器と `crates/scribe2/src/pipe/cli/base_run.rs` も同じ 1 本を使うので触らない） | 入れる |
| `gate.token_cap`（gate の lens） | `crates/scribe2/src/headless/lens.rs` の `rows_of` → `prompt_of` | §6 の写しが在れば読む分岐（`rulings_of` と同じ形）。書き手は gate.rs の `keep_rulings` の隣 | 入れる |
| `gate.token_cap`（契約の審査・先撃ち・行の審査・memo の lens） | 同じ `rows_of` | 写しが無いので manifest のまま | 入れない |
| `R-C6-1` | `crates/scribe2/src/pipe/cli/show.rs` の `ceiling_of` | 停止の設計の後に対象の行へ足し、表示の limit を許可の値にする | 入れない |
| `review.same_kind_stop` | `crates/scribe2/src/pipe/cli/intake.rs` の `exclude_repeats` | 対象の行へ足す裁定の後の行 | 入れない |
| `pipe.follow_retries`・`pipe.land_wait_s`・`pipe.ci_wait_s` | step.rs の `land_run`・`terminal_input` | 同上 | 入れない |
| `gate.slot_wait_s` | `Limits` の `admission`・`breaker` | 同上（遮断器の読みは host 全体のまま） | 入れない |

後の行が呼ぶ口（この § が祖先）: 純関数 1 本（名 permitted・`pub`）。入力は行 id・manifest の値・bead・event 列・今の時刻、返りは閉じた 2 値（宣言のまま / 許可〔値・裁定 id・期限〕）。置き場の file は行 b が決め、記帳の本体の書き手と読み手・記帳の列・着地の読み・期限の述語も同じ file に `pub` で置く（§18）。event 列を読めない周は呼び手が今のとおり gate を止める（許可へ倒す枝は持たない）。

## 8. 偽れる穴と塞ぎ方

器の照合は「記帳された発話が在る」ことを根拠にする。普通の口（bind・答えの口）では席が逐語を渡せないので偽れないが、次の穴が残る。

| 穴 | 今 | 塞ぎ方（推奨） | 残り |
|---|---|---|---|
| H1 hook の口の直撃: 席の Bash が `<NAME> hook` の発話の記帳の枝へ作った payload を流す | 止める guard が grep で見当たらない | 行 a: 席の道具の呼び出しが器の hook の subcommand を撃つ segment を、答えの口と同じ形（answer_mouth.rs）で断る | script の file を経る形は字面で測れない |
| H2 event log の直書き: Edit / Write か Bash の redirect で置き場の log へ行を足す | 席の権能 edit-outside の内側 | 行 a: 置き場の log の path を名指す Edit / Write と redirect を断る | python などの子 process の書きは測れない |
| H3 無関係の発話を結ぶ | bind は席が発話を選ぶ | §4.3 の 5・7・8（同じ session・問いより後・1 対 1） | 問いの後に user が別の話題で「よい」と言った発話を結ぶ形は測れない。逐語は許可から引けるので事後に見える |
| H4 問いの本文は席が書く | — | §4.3 の 9（本文と許可の字を揃える） | user が本文を読んだかは測れない |
| H5 `--rules` の差し替え | 権能も記録も無い | 行 a: 席が `pipe gate`・`pipe land`・`pipe resume` に `--rules` を渡す形を断る。gate の Gated の detail に読んだ manifest の出所（埋め込みか path の sha）を書く | 席の手動の dispatch の 1 周が起こす便へ写す `--rules` と、その写しで起きた便の読みは断りの外で、扱いは後継の決定が持つ（ADR-0106・SRS FR111・FR112）。出所の記録は行 a の設計で足す跡で、要件には無い |
| H6 承認 event の偽造（`pipe approve`・`fleet record --actor human`） | 席が字を渡せる | 許可の根拠にしない（裁定 event と発話 event だけを読む） | 便の承認の口そのものの穴は本 doc の外 |

根の限界: 置き場は AI と同じ OS の user が書ける場所なので、改ざんを不可能にはできない（別の OS の user か署名が要る・§15）。器が保証するのは「普通の口では偽れない・偽った跡は残る・許可は一覧に出る」までである。

## 9. SRS の要件

- 要件の正本は SRS の版 0.34: [FR110](../../design-intent/spec/srs.html#FR110) 上限の許可の記帳・[FR111](../../design-intent/spec/srs.html#FR111) 上限の許可の効き・[FR112](../../design-intent/spec/srs.html#FR112) hook の subcommand と event log の書きと規則の差し替えの遮断、受入基準 AC84〜AC86。字を直した既存の要件は FR9・FR17・FR41・NFR1。
- 版 0.34 の round で本 doc の案から変えた所: 照合の順（記録の在りを先・§4.3）、not-surface の比べる値（席の会話 id）、取り消しの書き方と許可の値が manifest の値以下の周（§4.4）、FR111 の型（常時）、FR112 の権能の guard との順と子 process の門の外。Phase F の 2 周目で、効く許可を「最新の記帳がその許可自身」に絞り（§4.4）、状態の語 overtaken を足し（§4.5）、打刻の無い周（rc 1）と読めない周（rc 2）を分け（§4.3・§5）、取り消しの照合を rule-not-listed だけにし、pipe.permit_rows の閉じを FR17 の読みへ移し、手動の 1 周の `--rules` を FR112 の断りの外と書き（§8 H5）、使用の記録に裁定 id を足し（§4.5）、行の審査の lens を manifest の値の読み手に足した（§2.3・§6・§7）。3 周目で、対象の閉じを許可の読み手を持つ行に狭め（§4.2・§6）、打刻の rc の境目を file と最後の行で書き分け（§4.3）、記帳の周の rc 2 を照らす前の読みにし（§5）、手動の 1 周の写しで起きた便の読みを後継へ回し（§8 H5）、§2.5 の宣言値の語を実行時の記録に直した。案の字は git の履歴に在る。

## 10. ADR

- 決定: [ADR-0106](../../design-intent/decisions/ADR-0106-limit-permits-raise-a-named-cap-for-one-bead-on-a-bound-ruling.html)（下の決定の文の案 5 つを 1 本の決定にまとめた。SRS の版 0.34 で accepted・要件は FR110〜FR112 と AC84〜AC86・rules 行 2 本の値の裁定 id は §13）。

ADR を書く条件の 1（C1・C7 / A4.2・C10 の解釈）・3（event の kind と形・run dir の写し・rules 行の kind 2 つ・跨版）・4（却下の分岐）に当たる。実装の前に ADR を land し、同じ PR で語彙と decisions の索引を直す。決定の文の案:

1. 消費の上限の行のうち rules 行が名指す行だけを、bead 1 つに限って有限の値へ上げる上限の許可を、event log の新しい kind 1 つで持ち、manifest の値は変えない（C1 の読み: 値の宣言は manifest、許可は承認 event と同じ実行時の記録）。
2. 許可は対話面の chat の経路で記帳された user の発話に結んだ問い id の形の裁定 id を要り、器の口が発話の実在・同じ session・問いより後・未使用・問いの本文の字を照らしてから記帳する。席が字を渡す承認（`pipe approve`・`fleet record`）は根拠にしない。
3. 許可を読むのは名指された bead の便の gate の予算の照合と lens だけで、host と群で分け合う上限と構造の上限（C4・C13）は対象にしない。
4. 許可は期限（rules 行が上限を持つ）・bead の着地・同じ bead と行の新しい記帳か取り消しで切れ、作成と使用は event・`pipe show`・`pipe dispatch ls` に出る。
5. 前段として、席の道具の呼び出しが器の hook の口を直に撃つ形・event log を書く形・gate と land に `--rules` を渡す形を器の hook が断る。

## 11. 足す物・消す物（C17.2）と決定の梯子（C17）

- 足す: event の kind と形 1 つずつ・許可の口 1 つ・rules 行 2 本・`RuleKind` の分けの match 1 本・run dir の写し 1 つ・hook の断り 1 本。
- 消す: 上げの PR → binary の入れ替え → 戻しの PR の対（前例 2 組）と、席が `pipe gate`・`pipe land`・`pipe resume` に写しの manifest を `--rules` で渡す応急の形（H5 の断りで消える。列の手動の 1 周に渡す `--rules` は H5 のとおり断りの外で、扱いは後継の決定が持つ（行 a は現物の形を測るだけ）。
- 梯子: 要るか（要る・前例 2 組と今日の 1 件・`--rules` は lens に届かない）→ 既に在るか（承認・裁定の結び・`--rules` は在るが、どれも「確かめた user の許し × 作業 1 つ × 値」を持たない）→ std と既存の部品で足りるか（足りる・依存を足さない・NFR3）→ 1 行で済むか（済まない・読み手が 2 process に割れている）→ 最小の実装（MVP は `gate.token_cap` の 2 読み手だけ）。

## 12. 契約表の行の見込み（SRS の round と ADR の後に書く）

| 行 | 中身 | src の見積 | 順 |
|---|---|---|---|
| a | hook の断り（H1・H2・H5）と gate の detail の manifest の出所・列の番の読み（queue の `taken_at`） | 200〜300 行 | 先頭（単独でも価値がある） |
| b | `RuleKind` の分けの match・rules 行 2 本と kind 2 つ・event の kind と形・公開の口（記帳の本体と純関数） | 250〜350 行 | a の後 |
| c | 許可の口（照合 10 語・`Bead` の本文の欄・権能の表の行・help と snapshot） | 300〜450 行 | b の後 |
| d | 読み手（gate.rs の `gate` の 1 か所の重ね・`decide` の文・Gated の detail・run dir の写しの書き手と lens の読み手） | 150〜250 行 | b の後（c と write-set が交わらないので並べられる） |
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
- MVP は gate の 2 読み手だけ。契約の審査・先撃ち・行の審査・memo の lens と、`review.same_kind_stop` などほかの作業ごとの行は後の行（行を対象に足すには、その行の読み手と SRS FR111 の直しを足す後継の決定が要る）。
- user が問いの本文を読み、何に「よい」と言ったかは測れない（H3・H4）。
- runner の消費は縛らない（R-C6-1 は表示だけ・turn の上限は無い）。動機に一番効く守りは §13 の論点 3 の側で、同じ epic の次の設計が持つ。
- 撃ち直しは席の今の手のまま（許可の記帳が便を自動で測り直す形は持たない）。

## 16. 語彙

- 新しい語の canonical 名の案: **上限の許可**（aliases: limit permit）。定義の案: 消費の上限の rules 行 1 本を bead 1 つに限って有限の値へ上げる記録。対話面の chat の経路の発話に結んだ裁定 id を要り、期限・着地・取り消しで切れる。event の kind（案 LimitPermitted）と許可の口（`pipe permit`）はこの語の別名として書く。
- 既存の語に合わせる: 裁定 id・問い id の形・発話 event・裁定 event・裁定の行・台帳の問い・effect・発話の経路・対話面・裁定面・答えの口・承認 event・3 クラス・受付・便・検出線。`Allowance` は口座残量の型で使用済みなので許可の訳に使わない。

## 17. 行 a — hook の断り 3 形（hook の subcommand の直撃・置き場の event log の書き・pipe gate / land / resume への --rules）と、gate の Gated の detail に読んだ manifest の出所・列の番の読み（契約表の行 a・[FR112](../../design-intent/spec/srs.html#FR112) / AC86 / FR45 / NFR5・ADR-0106 の決定の前段）

やさしく言うと: 上限の許可（§4）は「記帳された user の言葉」を根拠にする。その根拠を AI の席が普通の道具で作れないよう、器の見張り（hook）が 3 つの近道を実行の前に止める。席が器の hook の口を自分で撃つ形、記録の file（event log）へ直に書く形、検査と着地と再開に写しの設定（`--rules`）を渡す形である。あわせて、検査（gate）が段の記録に「どの設定を読んだか」を 1 語残すので、止めの外に残る手動の 1 周の写しも後から見える。段の記録に語を足すので、その記録を読む列の番の読みも同じ行で直す。

- 何が起きているか（main 2fbe0c11・verified）:
  - PreToolUse の入口は `crates/scribe2/src/hook/mod.rs` の pre_tool_use で、門の順は選択式の問いの門 → 答えの口の門（`crates/scribe2/src/hook/answer_mouth.rs`・全部の門の前・役割と pane に依らない）→ write-set guard → Bash だけの command guard・起票の門・台帳の形の門・anchor の門・merge の門 → 権能の guard（`crates/scribe2/src/hook/role_guard.rs`）→ 走っている便の行の門（最後の 1 段）。門の順は `crate::polarity::Guard` の宣言順と同じで、deny 文は先の門が先。
  - H1: hook の subcommand（`<NAME> hook <event>`）を席の Bash から撃つ形を止める門は無い。event の語は `crates/scribe2/src/hook/mod.rs` の非公開の定数 6 つ（session-start・pre-tool-use・permission-request・user-prompt-submit・stop・pre-compact）で、`plugin/hooks/hooks.json` が撃つ `hook <event>` も同じ 6 つ。dispatch は知らない event を 0 byte・rc 0 で黙って返す（書かない）。
  - H2: event log の path は `crates/scribe2/src/fleet/store.rs` の `events_path`（置き場の fleet の下の events.jsonl）。置き場は anchor の repo の外に在り、席の Edit は権能の guard が path 種別 outside（権能 edit-outside・行 `role.orchestrator` が持つ）で通す。Bash の書き込みの向け先の読み手は host の見張り（`crates/scribe2/src/hook/host_guard.rs` の `own_targets`・redirect の > と >>・tee・sed -i・mv・cp・ln・rm の対象）が 1 本持つが非公開で、hook の入口からは呼ばれていない。
  - H5: 権能の guard の表 `CAPABILITY_COMMANDS` は 8 行で、pipe land は権能 merge・pipe resume は権能 launch、pipe gate と pipe dispatch は表に無い（権能の guard は素通し）。行 `role.orchestrator`（`rules/manifest.toml`）は merge も launch も持たないので、orchestrator の席の pipe land と pipe resume は `--rules` の有無に依らず権能の guard が先に断る（欠けた権能と行 id）。名指しの停止の窓 `STOP_FLAGS` は `--rules` を含む（pipe stop --run X --rules R は権能 stop で通る）。名指しの決着の窓 `SETTLE_FLAGS` は `--rules` を含まず、`--rules` を足した pipe land --terminal-only は名指しでなく権能 merge へ降りる（既存の歯が orchestrator の席で（merge）の断りを測る）。手動の 1 周は渡された `--rules` を起こす便へ写す（`crates/scribe2/src/pipe/dispatch/candidates.rs`）。
  - gate の段の event: `crates/scribe2/src/pipe/gate.rs` の settle が `RunStage stage=Gated` を 1 件書き、detail は `gated_detail` の `verdict:<V>`（器が lens の口座を選んだ周は `,account:<label>`）。どの manifest を読んだかは event にも `verdict.json` にも無い。gate を起こす経路は 2 つだけで、材料の struct `Gate` の literal は `crates/scribe2/src/pipe/cli/step.rs` の gate_run と `crates/scribe2/src/pipe/land.rs` の追随の再 gate の 2 か所。着地の材料 `Land` は land が受けた `--rules` の path を欄 `rules` に既に持つ。
  - Gated の detail の読み手: `crates/scribe2/src/pipe/queue.rs` の `taken_at` は `verdict:` の後ろの全部を PASS と字で比べるので、`verdict:PASS,account:a1` を「列を離れた周」に数えて番を消す（今の欠け・器が口座を選んだ撃ち直しで起きる）。`crates/scribe2/src/fleet/lifecycle_mark.rs` の `verdict_word` は `,` の前を読む（後ろの項に耐える）。ほかの読み手は段の位置と `verdict.json` を読み、detail の字を比べない。e2e は 9 本の歯が Gated の detail を字の完全一致で測る。
- 前提: 本行は §12 の先頭で、後の行（b〜e）に依らない。行 d（§20）は同じ Gated の detail の末尾（本行の出所の後ろ）に許可の使用の項 `,permit:` を足すので、列の番の読みの直し（約束 10）は本行が持ち、その項にも耐える形（最初の `,` の手前で切る）にする。
- 約束（番号は done と 1:1）:
  1. 門 1 本（行 a の write-set の + の file・`crates/scribe2/src/hook/mod.rs` に pub mod の 1 行と呼び出し）を、pre_tool_use が権能の guard の断らなかった周の後ろ・走っている便の行の門の前で撃つ。断りは rc 2・stdout 0 byte・stderr 1 行で、記録（inject.jsonl）の what は `bypass-deny <理由の語>`。pane の有無と役割に依らない（runner の session でも止める）。
     - 設計の線（歯を持たない・審査が読む）: 入力は tool 名・Bash の command・編集系の path・payload の cwd・hook の置き場だけで、台帳も rules も event log も読まず、子 process を撃たない（NFR5）。判定は通す値を持たない閉じた 2 値（断る・関係ない）。
     - 位置を答えの口の門と同じ前に置かないのは、FR112 が権能の guard の断る呼び出しにはその断りを出すと決めたからである。3 形を 1 つの門にまとめて権能の guard の後ろに置けば、pipe land と pipe resume だけでなく、権能 edit-outside を持たない席の event log の Edit も権能の断りが先に立つ。「答えの口と同じ形」（§8 H1）は読み方（segment・binary の名に依らない・shell の包み）の意味で取る。
  2. 理由の語は閉じた 3 語（宣言順 `hook-subcommand` / `event-log-write` / `rules-swap`）。Bash の周は 3 形を宣言順に全 segment で照らし（形の宣言順が segment の順に勝つ）、最初に当たった形の 1 行を返す。1 行は `<NAME>: deny bypass reason=<語>` で始まり、当たった字（hook と event の 2 語・書きの向け先の語か path・pipe と 2 語目と --rules）と FR112 と次の一手（発話は prompt の入口で hook が記帳し、問いへは seat ruling bind で結ぶ・event log は器の口だけが書く・--rules を外して撃つ）を持つ。
     - 設計の線（歯を持たない・審査が読む）: 語の列は全語の const slice で持つ（C2・極性の一覧の boundary と同じ置き方）。
  3. Bash の読み: 起票の門の分け方（`crates/scribe2/src/hook/ledger_guard.rs` の segments）で分けた列に、頭の語（前の VAR=… の代入を除く）の basename が sh・bash・zsh・dash・eval の segment について、その後ろの - で始まらない語を同じ分け方で分けた segment を足す（足した segment にも同じ読みを繰り返す）。語は前後の括り（ ( ) ` { } ）を外して比べる。
  4. `hook-subcommand`: segment の連続した 2 語が hook と event の 6 語の 1 つ。6 語は `crates/scribe2/src/hook/mod.rs` が既存の 6 つの定数を並べた 1 つの公開の列で持つ（dispatch の腕と同じ字・hooks.json の 6 つと集合が一致することを歯が測る）。binary の名・変数・cargo run -- の前置きに依らない。hook の後ろが event の語でない形（git hook run・host-guard の口）と、引用の中の字（grep の pattern）は当たらない。
  5. `event-log-write`: 編集系の道具（`crates/scribe2/src/hook/guard.rs` の GUARDED の 4 つ）の path と、Bash の書き込みの向け先（host の見張りの読み手 1 本を pub(crate) にして呼ぶ・写しを持たない）が、hook の置き場の event log（`events_path`）に当たる周に断る。当たりは host の見張りの自身の設定の比べ方と同じ（cwd から字句で畳んだ path か実体の path が一致するか、実体の在る log の祖先）。$・backtick・brace を含む語・~ で始まる語・cd の後ろの相対 path（host の見張りの解けない語）は、最後の要素が event log の file 名と同じなら断る（解けない書きを通さない・fail-closed）。読むだけの command（cat・tail・grep・< の入力・fleet の口）と、置き場の別の file と別の dir の同名の file への書きは当たらない。
  6. `rules-swap`: segment の連続した 2 語が pipe と gate・land・resume のどれかで、同じ segment に `--rules` の語か `--rules=` で始まる語が在る。pipe dispatch（手動の 1 周・FR112 の断りの外）・pipe stop・pipe land-window・--rules を持たない 3 つは当たらない（2 語目は語の完全一致）。
  7. 順（FR112・FR45）: 権能の guard が断る呼び出しは権能の断りだけを出し、本行の門の記録を残さない（orchestrator の席の pipe land --rules は（merge）・pipe resume --rules は（launch）と行 `role.orchestrator`）。merge と launch を持つ行の席と pane の無い session では同じ 2 つを `rules-swap` で断る（権能の席では role-allow の記録の後に本行の記録 1 行）。
  8. 極性一覧: `crate::polarity::Guard` に variant を 1 つ、Role の直後・LiveRow の直前に足す（一覧の語 `bypass-deny`・in-loop / fail-closed・boundary は判定の enum）。`ALL` と網羅の match 3 本に 1 arm ずつ。集計の guards と in-loop は便の base の数に 1 足した数、post-hoc と fail-open は base の数のまま（数の pin は便の base で数え直す）。
  9. gate の出所の跡: gate の Gated の段の event の detail の末尾に `,rules:<出所>` を足す（verdict の後・account が在ればその後）。出所は、埋め込みの manifest を読んだ周が `embedded`、`--rules` の path を読んだ周がその file の git の blob id（std の absolute で絶対にした path を `git -C <repo> hash-object --no-filters --` に渡した stdout の 1 行・16 進の字だけの周）、それ以外（rc≠0・空・16 進でない）が `unreadable`。出所は gate の入口で 1 回だけ測り、判定・rc・`verdict.json`・stdout の判定行は変えない。gate_run は `--rules` の値を、land の追随の再 gate は `Land` の欄 `rules` を、材料の struct `Gate` の新しい欄（`Land` と同じ名と型）で渡す（literal の site は 2 つで、どちらも本行の write-set）。追随の再 gate を省いた引き継ぎの記帳（regate=skipped）は manifest を読んで判じた周でないので字を変えない。
  10. 列の番の読み（`taken_at`）は `verdict:` の後ろを最初の `,` の手前で切った語として PASS と比べ、`verdict:PASS,…`（本行の出所・口座・行 d の許可の項のどれが付いても）を PASS と読む（離れた周は語が PASS でない周だけ）。`verdict_word` は変えない。
- 閉包: 本行は `crate::polarity::Guard` に variant を足すので touches に持ち、閉包（`crates/scribe2/src/polarity.rs` と e2e の極性の歯と外形 snapshot）を write-set に持つ。行 a の write-set の + の file は `Guard`・`EventKind`・`Stage`・`RuleKind` の変種を名指さない（ほかの doc の行の閉包を広げない）。`Gate` に欄を足すが `Event` の struct には足さない。
- 歯（変異の A/B は判定の順に沿って条件 1 つに歯の脚 1 本）:
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/hook/guards.rs`・接頭辞 `hook_bypass_`・6 本・file の頭の接頭辞の列に足す）。断りの脚はどれも rc 2・stdout 0 byte・stderr の 1 行が `<NAME>: deny bypass reason=<語>` で始まり FR112 と語ごとの次の一手の字を持つ・記録 1 行を測る。
    - (a) `plugin/hooks/hooks.json` の hook と event の全部（母集団 6・公開の列と集合が一致）の素の撃ちと、変数の binary・cargo run --・bash -c・eval・命令置換の中の形・前に VAR=1 の代入を持つ bash -c・bash -c の中の bash -c の形が reason=hook-subcommand（同じ歯で grep -rn の引用の中の hook stop・git hook run pre-commit・host-guard の口は通る）
    - (b) 置き場の event log の path への Edit と Write・echo の >>・tee -a・sed -i の対象・mv と cp と ln の行き先・bash -c の中の >・$ の変数で始まる同名の向け先・cwd を置き場にした字句で畳む相対 path（fleet/./events.jsonl）への >>・置き場の fleet を指す symlink の dir の下の events.jsonl への Write（実体の path）・置き場の fleet の dir への rm -r（log の祖先）が reason=event-log-write、event log への >> の segment と hook stop の segment を `;` で並べた形（書きが先の segment）が reason=hook-subcommand（形の宣言順）（同じ歯で置き場の別 file への Write と別の tmp dir の fleet の下の同名の file への > は通る）
    - (c) event log を読むだけの cat・tail -n・grep・wc -l < が通り記録 0
    - (d) pane の無い session で pipe gate・pipe land・pipe resume に --rules X と --rules=X を渡す形と bash -c で包んだ形が reason=rules-swap（同じ歯で --rules の無い 3 つは通る）
    - (e) pipe dispatch --rules・pipe stop --run r --rules・pipe land-window --rules が通る
    - (f) 偽 tmux の席（既存の stub_seat・tmux の群の外）で、`role.orchestrator` の既定の権能の行は pipe land --run r --rules X と pipe resume --run r --rules X を権能の guard の 1 行（（merge）・（launch）と行 id）で断って bypass-deny の記録が無く、merge と launch を足した fixture の行の同じ席では同じ 2 つが reason=rules-swap で断られる。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/polarity.rs`・接頭辞 `polarity_lists_bypass_deny_`・1 本）: (g) 一覧に bypass-deny の in-loop / fail-closed の 1 行が role-guard の直後・live-row-guard の直前（数は pin しない）。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_rules_source_`・2 本）: (h) 埋め込みで撃った gate の detail が verdict:PASS,rules:embedded (i) cap の狭い fixture の INCONCLUSIVE と広い fixture の撃ち直しの PASS の 2 件の detail が、それぞれの file を歯が git hash-object --no-filters で測った blob id を持ち、2 つが違う。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs`・接頭辞 `pipe_land_rules_source_`・1 本）: (j) 兄弟の便の着地で main が動いた便を --rules つきの land で追随させると、verdict: で始まる Gated の detail が最初の gate の rules:embedded と再 gate の rules:<その file の blob id> の 2 件。同じ歯で、検出線の面に触れない commit で main が動いた便の引き継ぎ（regate=skipped）の Gated の detail は verdict:PASS のまま（rules: の項を持たない）。
  - lib（`crates/scribe2/src/pipe/gate.rs` の既存の歯の区間・接頭辞 `gate_rules_word_`・1 本・記録する stub）: (k) 埋め込みは git を撃たずに embedded、path は git の hash-object の 1 回で stdout の 16 進の 1 行、rc≠0 と 16 進でない stdout は unreadable。
  - lib（`crates/scribe2/src/pipe/queue.rs` の既存の歯の区間・接頭辞 `pipe_order_taken_suffixed_`・1 本）: (l) turn:taken の後の verdict:PASS,rules:embedded と verdict:PASS,account:a1,rules:<16 進> と verdict:PASS,rules:embedded,permit:gate.token_cap=9 ruling=s2-rq9.1 は番を残し、同じ歯の verdict:INCONCLUSIVE,rules:embedded は番を消す。
  - 直す既存の歯（どれも直した期待が base で落ちるので retroactive の札は要らない）: Gated の detail を字で測る 9 本（`pipe_gate_lens_account_` の 5 本〔pipe.rs 1・pipe/gate.rs 4〕・`pipe_gate_findings_counts_are_recorded_per_category`・`pipe_gate_regates_after_inconclusive`・`pipe_gate_lens_reread_unreadable_then_readable_passes_with_two_calls`・`pipe_gate_health_busy_run_stays_gated_and_can_be_regated`）の期待に ,rules:<出所> を足す（期待の出所は e2e の pipe.rs に足す helper 1 本が歯の側で測る）。極性の数の pin（guards と in-loop）を持つ 5 本（`polarity_lists_the_three_added_guards`・`runner_question_guard_is_in_loop_fail_open`・`polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed`・`polarity_lists_contract_table_as_a_post_hoc_fail_closed_guard`・`polarity_lists_command_guard_as_in_loop_fail_closed_right_after_cap_guard`）の数を便の base の数に 1 足した数へ、隣接の pin 2 本（`polarity_lists_live_row_guard_between_role_guard_and_contract_table` の role-guard の直後 → 2 つ後・contract-table の歯の 2 つ後 → 3 つ後）と外形 snapshot を直す。
  - base で RED: 門も detail の項も無い・列の読みが後ろの項を PASS と読まない（機能不在）。
- 触らない: 答えの口の門と選択式の問いの門の位置と字・権能の guard の表と 2 つの窓・host の見張りの判定と字（読み手の可視性だけを変える）・`verdict.json` と gate の stdout の判定行・lens の cap の読み（行 d）・`verdict_word`。
- 限界:
  - 席が起こした子 process（script の file・python などの書き・別の program からの撃ち）は門の外（FR112・FR82 と同じ）。改ざんを不可能にはしない（§8 の根の限界）。
  - 書きの動詞の列は host の見張りの読み手のまま（redirect・tee・sed -i・mv・cp・ln・rm）。truncate・dd などほかの動詞と、語の途中に付いた redirect（空白を挟まない >>）は読まない。
  - 守るのは hook の置き場の event log だけ（ほかの置き場の log は、解けない語の同名の向け先の周だけ止まる）。anchor を解けない session（marker の無い repo）では hook が仕えないので門は働かない（FR24）。
  - pipe run・pipe spawn・手動の 1 周の --rules は断らない（FR112 の外・後継の決定）。手動の 1 周が写した --rules で起きた便の gate は detail の出所でだけ見える。
  - 出所の blob id は manifest の読みと別の読みで、gate の入口で測る（同じ process の中の 2 回の読みの間に file を替えた周は替えた後の id になる）。
- 却下:
  - 3 形を答えの口の門と同じ前の位置に置く: pipe land と pipe resume で権能の guard より先に断り、FR112 の順に反する。
  - --rules の断りを権能の guard の表へ足す: 表は役割の行の権能の照合で、pane の無い session では働かない。形の禁止と権能を 1 つの表に混ぜる。
  - event log の書きを host の見張りの種類に足す: hook の入口と別の口で、権能の guard との順を持てず、種類の閉じた列と行を足すことになる。
  - 出所を `--rules` の周にだけ書く（埋め込みは無印）: 本行より前の event と埋め込みの周を区別できない（C10）。
  - 出所に FNV-1a 64 を使う: 器の hash は増えないが、git の blob id（main の manifest の id）と照らせない。
  - 出所を `verdict.json` に書く: 撃ち直しで上書きされ履歴が残らない（event は追記）。
  - 列の番の読みの直しを行 d に置く: 本行の出所の項が毎周の接尾辞になるので本行が先に当たり、両方に置くと後の行の歯が base で緑になる。

## 18. 行 b — 上限の許可の型と記録の土台（rules 行 2 本・許可の読み手の分け・event の kind と形・記帳の本体と効きの純関数）（契約表の行 b・[FR17](../../design-intent/spec/srs.html#FR17) / FR110 / FR111 / NFR4・AC84・ADR-0106）

やさしく言うと: 上限の許可を記帳する口（行 c）・効かせる gate（行 d）・一覧（行 e）が共通に使う「入れ物」と「判定」を先に置く。manifest には「どの上限を許可で上げてよいか」と「期限は最長何時間か」の 2 行を足し、manifest を読む所は許可の対象にできない行を名指したら読みで断る。event log には許可の記帳の種類を 1 つ足す。そして「この bead のこの上限に、今いま効いている許可があるか」を event の列から答える純関数を置く。この行だけでは許可を書く口も読む gate も無いので、器の振る舞いは変わらない。

- 何が起きているか（main 2fbe0c11・verified・読むだけ）:
  - kind の分けの網羅の match は `crates/scribe2/src/rules/mod.rs` の `as_str` と `shape` の 2 本で、wildcard を書かない。行を跨ぐ検査は `crates/scribe2/src/rules/manifest.rs` の `check_declared` が束ね、`check_class_commands` が前例（kind で行を選び、要素ごとに行番号つきの 1 件を積む）。rules の行数と kind の数は `rules validate` の外形と `crates/scribe2-boundary/tests/e2e/rules/embedded.rs` の 2 本の歯が pin する。
  - List の行の読み手は `crates/scribe2/src/rules/mod.rs` の `list_row`（無い・不発効・列でないの 3 理由）で、例は `runner.allowed_commands`。値の形の検査は行の `validate` が先に行い、空の列は形の段で断る。
  - `crates/scribe2-boundary/tests/e2e/rules.rs` の歯 `rules_kind_parity_every_kind_has_sample` は `ALL` の全 kind を 1 行だけの manifest で読む。対象の列の kind は名指す行が同じ manifest に要るので、この歯に kind 1 つの場合分けが要る。
  - `ALL` の末尾を数で pin する歯が 5 本在り（`rules_floor_timeout_row_precedes_the_drafts_cap_rows`・`rules_drafts_cap_rows_are_the_last_two_kinds_and_rows`・`rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling`・`class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id`・`rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all`）、未着地の 3 行（ledger-form.md 行 o・reverse-index.md 行 a2・row-review.md 行 f）も rules の行を足す。`LensMaxTurns` の後ろ（`HookBudgetMs` の前）と `lens.max_turns` の行の後ろ（`hook.budget_ms` の前）を名指して pin する歯は 0 本。
  - event の網羅の match は `crates/scribe2/src/fleet/mod.rs`（`as_str`・`default_actor`・`shape`）・`crates/scribe2/src/fleet/event.rs`（行の並び `to_line`・本体の読み `Body` の `read`・`install` と `pressure` の形の match）・`crates/scribe2/src/fleet/replay.rs`（2 本）。`Event` の struct は欄を持ち足さない。本体を `detail` の 1 行に持つ前例は群の逼迫の通知（`Pressure` の `render` と `parse`・`Body` が `parse` で形を確かめる）、`bead` を持ち `run` を持たない前例は列の印（`Shape::Mark`）。`fleet record` は形が便でない kind を全部「record では書けない」で断る（`crates/scribe2/src/fleet/cli.rs`・形から導く）。
  - `KINDS` の数と末尾を pin する歯は 6 本（`crates/scribe2-boundary/tests/e2e/fleet.rs`・`fleet/account.rs`・`fleet/json.rs` の 2 本・`pipe/gate.rs` の 2 本〔段の数と kind の数の組〕）。
  - event の `ts` は `YYYY-MM-DDTHH:MM:SSZ` の文字列で、UNIX 秒への読みは `crates/scribe2/src/fleet/wait.rs` の `epoch_of`（形の外は `None`）、逆は `crates/scribe2/src/fleet/cli.rs` の `format_utc`。純関数は今の時刻を UNIX 秒の u64 で受ける形が前例（`crates/scribe2/src/pipe/dispatch/facts.rs` ほか）。
  - bead の便の着地を event の列から判じる bead 単位の関数は無い（便単位の読みは `crates/scribe2/src/pipe/land/finish.rs` の `landed_sha` と `crates/scribe2/src/pipe/retire.rs`）。着地は `RunDone` か `RunStage` の `stage` が `Stage::Landed` の行で、行は `bead` と `run` を持つ。
  - lib の crate の `pipe` は公開の module で、その下の公開の item は e2e から呼べる（e2e は既に fleet の公開の定数と関数を直に呼ぶ）。
- 約束（番号は done と 1:1）:
  1. **rules 行 2 本**を manifest の `lens.max_turns` の行の直後（`hook.budget_ms` の前）にこの順で足す: `pipe.permit_rows`（kind `PipePermitRows`・List・値 `["gate.token_cap"]`・enabled・ruling `user 2026-10-01T01:05Z 項 permit-rows`・ruled_at `2026-10-01`）と `pipe.permit_max_h`（kind `PipePermitMaxH`・Int・値 24・enabled・ruling `user 2026-10-01T01:24Z 項 permit-max-h`・ruled_at `2026-10-01`）。kind 2 つは `RuleKind` の宣言と `ALL` で `LensMaxTurns` の直後・`HookBudgetMs` の前にこの順で置く（末尾に置かない: 末尾を数で pin する 5 本の歯と未着地の 3 行が末尾を取り合うので、置き場を末尾から外せば動く歯は行数と kind 数の pin だけになる）。2 つの裁定の字は base に無く、それぞれ 1 行だけが持つ（`cargo xtask rules-diff` の new-row-reuses-ruling に当たらない）。読み手は許可の口（行 c）と gate（行 d）で、この行の後は `cargo xtask check` の rules-wired が 2 行を読み手の無い行として名指す（検出の行で rc は変えない・ledger-form.md 行 o と同じ扱い）。
  2. **許可の読み手を持つかの分け**: `crates/scribe2/src/rules/mod.rs` の `RuleKind` に網羅の match を 1 本足す（名 `has_permit_reader`・`pub`・返りは bool・wildcard を書かない＝kind を足した周に分けを決めないと compile が落ちる・`shape` と同じ置き方・C2）。true は `GateTokenCap` だけ。ほかの 6 つの作業ごとの消費の行（`RunTokenCeiling` など）は、読み手と FR111 の直しを足す後継の決定の後に true へ移す。
  3. **loader の閉じ**（FR17）: `crates/scribe2/src/rules/manifest.rs` の `check_declared` に、kind `PipePermitRows` の行の値の要素ごとの検査を 1 本足す（`check_class_commands` の隣・行は id でなく kind で選ぶ・enabled は問わない）。要素が manifest の行の id でないか、その行の kind の分け（約束 2）が false なら、行 id・要素・行番号を名指して 1 件ずつ断る。断りの字は `<行 id> の value の要素 "<要素>" が manifest の行の id でない（上限の許可の読み手を持つ kind は <kind の列>）` と `<行 id> の value の要素 "<要素>" の行の kind <kind> は上限の許可の読み手を持たない（上限の許可の読み手を持つ kind は <kind の列>）` の 2 形で、kind の列は分けが true の kind を `ALL` の順に ` / ` で継ぐ（今は `GateTokenCap`・手書きの列を持たない）。`--rules` の写しにも同じ読みが効く。
  4. **event の kind と形**: `EventKind::LimitPermitted` を `KINDS` の末尾に、形 `Shape::Permit` を `SHAPES` の末尾に足す。既定の actor は machine。行は `bead`（空でない）と `detail`（下の閉じた key の列）が必須で、`run`・`stage`・`seat`・`pid`・口座残量の key（`account` を含む）・登録の key・列の印の key は持たない（在れば key を名指して malformed）。裁定・案件・消費の key は今の形の段が既に断る。並びは `schema`・`ts`・`kind`・`bead`・`host`・`actor`・`detail`（列の印の並びから印を除いた形）。`Event` の struct に欄を足さない（本体は `detail` の 1 行・`Pressure` と同じ置き方）・schema は 1 のまま。replay は便も席も作らない（2 本の match で読み飛ばす）。`fleet record` は形が便でないので kind `LimitPermitted` を「record では書けない」で断る（今の口のまま・席が許可の event を字で書く経路を作らない・§8 H6）。
     - `detail` の閉じた key の列（空白 1 つで区切り、順は固定、余りの語は無い）: 許可は `rule=<行 id> value=<10 進の整数> until=<YYYY-MM-DDTHH:MM:SSZ> ruling=<裁定 id>`、取り消しは `rule=<行 id> revoked`。`until` は event の `ts` と同じ秒の形で、`epoch_of` が読めない字（分の形を含む）は形の外。行 id と裁定 id は空でない。形の外の `detail` を持つ行は `Event::from_line` が malformed で断る（NFR4・置き場の読みが rc 2 で止まる今の形に乗る）。だから event log を読めた周の許可の行の `detail` は全部読める（読み手が読めない detail の枝を持たなくてよい）。
  5. **後の行が呼ぶ口**（行 c・d・e はこの名で呼ぶ・置き場は行 b の write-set の + の file・`crates/scribe2/src/pipe/mod.rs` に doc 1 行と pub mod の宣言 1 行・可視性はどれも `pub`）。crate の内側でなく `pub` にするのは、後の行が呼ぶまで dead_code の札が要らず（札を外す行が行 b の file を write-set に持つと行 c と行 d の write-set が交わる）、純関数の歯を e2e に置けるからである。
     - 記帳 1 件の本体 `Record`（閉じた 2 値）: `Permit`（欄 rule: String・value: u64・until: String・ruling: String）と `Revoke`（欄 rule: String）。書き手 render(&self) -> String と読み手 parse(text: &str) -> Option<Record>（形は約束 4 の列・書いた字は読み返すと同じ値）。設計の線（歯を持たない・審査が読む）: `crates/scribe2/src/fleet/event.rs` の本体の読みはこの `parse` を呼び、形の写しを持たない（C2）。行 c が `render` で書き、行 c の reused の照合が `parse` で全件の許可を読み、行 d・e は下の口で読む。
     - records(bead: &str, rule: &str, events: &[Event]) -> Vec<Record>: kind `LimitPermitted` で `bead` が等しく、`detail` が読めて行 id が `rule` に等しい記帳を、列の順（＝記帳の順）のまま返す（許可も取り消しも）。ほかの kind の行は `detail` の字が同じでも数えない。
     - landed(bead: &str, events: &[Event]) -> Option<String>: 列のうち `bead` が等しく `stage` が `Stage::Landed` の最初の行の便 id を返し、無ければ `None`（便の着地は戻らない・記帳の前か後かを問わない）。行 c の照合 landed は返った便 id を名指し、`permitted` と行 e の状態の語は在りだけを読む。
     - expired(until: &str, now: u64) -> bool: `now` が期限（`epoch_of` で読んだ `until`）以後か、期限を読めない周に true（期限ちょうどは切れている・読めない期限は切れている）。`permitted` の判定 (c) と行 e の状態の語 expired はこの 1 本を呼ぶ（比べを 2 か所に写さない・C2）。
     - 効く cap の出所の閉じた 2 値 `Effect`: `Declared`（manifest の値のまま・値を持たない）と `Permitted`（欄 value: u64・ruling: String・until: String）。
     - permitted(rule: &str, declared: u64, bead: &str, events: &[Event], now: u64) -> Effect（`now` は UNIX 秒・`declared` は呼び手が manifest から読んだ値）。判定の順（1 つの関数が上から照らし、最初に外れた所で `Declared`）: (a) 同じ bead と行の最新の記帳（records の末尾）が許可である（無い・取り消しは外れ） (b) bead の便が着地していない（landed が無い） (c) 期限が切れていない（expired が false・期限ちょうどと読めない期限は外れ） (d) 許可の値が `declared` より大きい（等しいは外れ）。全部通る周だけ `Permitted`（その許可の値・裁定 id・期限）。最新の記帳だけを見るので、新しい許可が先に切れても古い許可へ戻らない（FR111）。event の列を読めない周の扱いは呼び手が持ち、この関数は許可へ倒す枝を持たない。
  6. 閉包: 本行は `RuleKind`・`EventKind`・`Shape` に変種を足すので 3 つを touches に持ち、write-set に閉包（3 型の宣言と網羅の match の file・`KINDS` と `ALL` の数の pin の歯の file）を持つ。閉包は `RuleKind` が 2 file・`EventKind` が 6 file・`Shape` が 4 file で、書き換えない `crates/scribe2/src/fleet/cli.rs` と `crates/scribe2/src/pipe/dispatch/candidates.rs` も閉包に入るので write-set に growth 0 で持つ。行 b の + の file は 3 型を match の腕・構築・件数の pin で名指さない（比べは `==` で書く）ので、3 型の閉包に新しい file は入らず、別の設計 doc の未着地の行（`RuleKind` を touches に持つ ledger-form.md 行 o・reverse-index.md 行 a2・row-review.md 行 f）の閉包は広がらない。分けの match は既に閉包に在る `crates/scribe2/src/rules/mod.rs` に置き、loader の検査は `==` で kind を選ぶ。
- 歯（変異の A/B は判定の順に沿って条件 1 つに歯 1 本）:
  - rules（既存の `crates/scribe2-boundary/tests/e2e/rules/embedded.rs`・接頭辞 `rules_permit_`・3 本・`crates/scribe2-boundary/tests/e2e/rules.rs` は上限の余地が小さいので既存の歯の書き換えだけに留める）: (a) 2 行の id・kind・形・値・enabled・裁定の字・裁定日、`list_row` と `int_row` で値が取れ、kind は字面から引け、`ALL` で `LensMaxTurns` → `PipePermitRows` → `PipePermitMaxH` → `HookBudgetMs`、manifest の行で `lens.max_turns` → `pipe.permit_rows` → `pipe.permit_max_h` → `hook.budget_ms`、2 つの裁定の字を持つ行はそれぞれ 1 本、形の違う値（列の行に整数・整数の行に文字列）は「形と合わない」で断られる（約束 1） (b) `ALL` のうち分けが true の kind の列がちょうど `[GateTokenCap]`（母集団は `ALL` の数を出す・`RunTokenCeiling` も false）（約束 2） (c) 名指される 5 行（`gate.token_cap`・`review.same_kind_stop`・`pipe.max_live`・`R-C4-1`・`runner.model`）と対象の列の行だけの fixture で、値が `["gate.token_cap"]` は読め、`review.same_kind_stop`（読み手の無い作業ごとの消費）・`pipe.max_live`（分け合う上限）・`R-C4-1`（構造）・`runner.model`（上限でない）・`nope.row`（行が無い）をそれぞれ `gate.token_cap` と並べた 5 つの値は、断りがちょうど 1 件で、その要素の字と kind の列の字と対象の列の行の行番号を名指し、`gate.token_cap` を名指さない（約束 3・同じ fixture の要素 1 つだけを替える）。同じ歯で対象の列の行を enabled = false にした fixture の `nope.row` も同じく断られる（enabled を問わない）。
  - fleet（既存の `crates/scribe2-boundary/tests/e2e/fleet/json.rs`・接頭辞 `fleet_limit_permitted_`・4 本）: (d) `KINDS` の末尾が `LimitPermitted` でその 1 つ前が便の base の末尾の kind、字面の往復・既定の actor は machine・形は `Shape::Permit` でほかの kind は持たない・`SHAPES` の末尾は `Shape::Permit`・口座残量の kind でない (e) 許可と取り消しの見本の 2 行が読めて（kind・bead・actor・detail・run は空・stage は無い）書き戻すと同じ行、2 行の replay は便 0・席 0 (f) bead の欠けと空・detail の欠け・`run`・`stage`・`seat`・`pid`・`account`・`rule`・`mark` を足した行は、それぞれ欠けた key か足した key を名指して読めない、形の外の detail 7 つ（key の順違い・`ruling=` の欠け・余りの語・値が数でない・値が空・`until` が分の形・`revoked` の綴り違い）は detail を名指して読めない (g) `fleet record --kind LimitPermitted` は rc 1 で「record では書けない」と断り log を作らない。
  - 口の e2e（既存の `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`・接頭辞 `limit_permit_`・7 本・行 b の + の file の中に歯を置かない〔新しい file の in-file の歯は flip-check で not-flippable〕・event は on-disk の 1 行を `Event::from_line` で読んで作り、`Event` の literal を書かない）: (h) `render` の字が約束 4 の 2 形と一致し `parse` で同じ値に戻り、(f) と同じ形の外の 7 つが `None` (i) records が別の bead・別の行・同じ detail の別の kind の行を数えず、許可・取り消し・許可を記帳の順に返す (j) 記帳が無い・最新が取り消し・別の行の許可だけの列は `Declared`、取り消しの後の許可は `Permitted`（判定 a） (k) 新しい許可の期限が古い許可の期限より先に切れた時刻は `Declared`（古い許可へ戻らない）、その前は新しい許可の値と裁定 id と期限（判定 a） (l) 同じ bead の着地の行が許可の後か前に在れば `Declared`、別の bead の着地の行は効きを変えない（判定 b）、landed は同じ列で着地の行の便 id を返し、別の bead だけの列では `None` (m) 今が期限ちょうどは `Declared`・1 秒前は `Permitted`（判定 c）、expired は期限ちょうどと分の形の期限で true・1 秒前で false (n) `declared` が許可の値と等しいか大きいと `Declared`・1 小さいと `Permitted`（判定 d）。
  - 直す既存の歯（どれも直した期待が base で落ちるので retroactive の札は要らない・数は便の base で数え直す）: rules の行数と kind 数の pin（`rules_embedded_manifest_is_valid_and_covers_all_kinds` の行数を base の数に 2 足す・`rules_embedded_manifest_declares_one_capability_row_per_role` の kind の数を base の数に 2 足す・`rules_external_form` の snapshot の `rows=` と `kinds=` の 2 か所を同じく 2 ずつ足す）、`rules_kind_parity_every_kind_has_sample`（kind `PipePermitRows` だけ値 `["gate.token_cap"]` と kind `GateTokenCap` の行 1 本を足した fixture で読む）、`KINDS` の数と末尾の pin（`fleet_kinds_follow_declaration_order` の数を base の数に 1 足し列の末尾に `LimitPermitted`・`account_cmd_kinds_are_fifteen_with_retire_and_restore_last` の数を 1 足す・`fleet_replay_seat_retired_kind_is_a_registration_shape_and_record_refuses_it` の数を 1 足し末尾を `LimitPermitted` に・`fleet_case_kind_kinds_are_appended_in_order` の数を 1 足し base の末尾の kind は末尾の 1 つ前へ・`pipe_regate_returns_gated_fail_to_implemented_on_the_same_worktree` と `pipe_follow_step_moves_gated_tree_onto_main_and_returns_to_implemented` の段と kind の数の組の kind の数を 1 足す）。
  - 書き換えない既存の歯で新しい kind を測るもの: `fleet_case_kind_record_refuses_every_non_run_shape`（形が便でない kind の全部を撃つので `LimitPermitted` を含む・verify に入れる）。
  - base で RED: kind も行も型も口も無いので、新しい歯と直した歯のある file は base の上で compile が落ちる（機能不在）。
- 触らない: gate の予算の照合と lens（`crates/scribe2/src/pipe/gate.rs` の `decide` と `Limits` の `of`・`crates/scribe2/src/headless/lens.rs`・行 d）、許可の口と台帳と役割の guard（行 c）、`pipe show` と `pipe dispatch ls`（行 e）、`Event` の struct の欄、schema の版、`fleet record` の口。
- 限界:
  - この行だけでは許可を書く口も読む gate も無い（記帳は行 c・効きは行 d）。rules-wired は行 c・d の着地まで 2 行を名指す。
  - `crates/scribe2/src/pipe/dispatch/refused.rs` の受付の断りの記帳は「その bead の最後の行」を見るので、断られている bead に許可の行が足された周の次の周に断りの行をもう 1 行書く（列の印の行と同じ今の振る舞い・log は 1 行ずつしか伸びない）。
  - 古い binary は `LimitPermitted` を未知の kind として断る（NFR4）。最初の許可の前に PATH の binary の入れ替えが要る（§6）。
- 却下:
  - rules の行と kind を末尾に足す案: 末尾を数で pin する 5 本の歯の書き換えが要り、末尾を足す未着地の 3 行と同じ歯を取り合う。
  - 対象の列の閉じを行の `validate`（行 1 つの検査）に置く案: 要素の行の kind は manifest の別の行を引かないと決まらない（`lifecycle.age_h.<語>` のように 1 kind が複数の行 id を持つ）。
  - 形を `Shape::Mark` に相乗りさせる案: 列の印の本体（`mark` の 4 値）が必須で、許可の本体と形が合わない。
  - 許可の値と期限を `Event` の欄に持つ案: literal の site を全部触る write-set になる（§6）。
  - 判定の純関数に manifest を渡す案: 宣言値の読みは呼び手（gate の `Limits`）の 1 本のまま置き、純関数は event の列と値だけを読む（C10: 宣言値と実行時の記録を別の型で持つ）。
  - 口を crate の内側の可視性にする案: 後の行が呼ぶまで dead_code の札が要り、札を外す行（c・d・e のうち最初に呼ぶ行）が行 b の file を write-set に持つので、並べられる行 c と行 d の write-set が交わる。純関数の歯も core の lib の歯になり、受付の見積の core の行を食う。

## 19. 行 c — 許可の口 pipe permit と取り消し・照合 10 語・台帳の Bead の本文の欄・権能の表の行・help と snapshot（契約表の行 c・[FR110](../../design-intent/spec/srs.html#FR110) / FR41 / FR17 / NFR4・AC84・ADR-0106）

やさしく言うと: 席が「この bead の上限をこの値まで、この時刻まで上げる。根拠はこの裁定」と器に頼む口を 1 つ足す。器は先に記録の file・設定・台帳・orchestrator の席の打刻を読み（読めなければ何も書かずに rc 2）、次に 10 の項目を順に確かめ（1 つでも外れれば何も書かずにその項目の語で rc 1）、全部通れば許可の記録を 1 件書いて 1 行を返す。取り消しは裁定を要らず、行が上げてよい列に在るかだけを見て 1 件書く。口は orchestrator の席の権能 approve で通り、役割の無い席では止まる。

- 何が起きているか（main 2fbe0c11・verified）:
  - `pipe` の subcommand は閉じた enum PipeCommand（`crates/scribe2/src/pipe/cli.rs` の宣言・const slice・`as_str` の arm、`crates/scribe2/src/pipe/cli/args.rs` の `allowed_of` の arm）で、件数と字の列は e2e の `pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none`（`crates/scribe2-boundary/tests/e2e/pipe.rs`）が pin する。直近に subcommand を足した前例は pipeline.md 行 az（anchor-sync）で、write-set は cli.rs・args.rs・`crates/scribe2/src/help.rs`・e2e の pipe.rs・使い方の snapshot だった。
  - 入口 `dispatch` は subcommand の flag の閉包を先に照らし、次に manifest を読む。manifest を読めない周は今どの subcommand も rc 1（`refused`）で、FR110 の「manifest を読めない許可の記帳の周は rc 2」と合わない。
  - 記帳の口の前例: `pipe approve` / `pipe answer`（`crates/scribe2/src/pipe/cli/step.rs` の `approve_run` / `answer_run` → `crates/scribe2/src/pipe/approve.rs`）は `pipe/mod.rs` の `emit` で 1 件書き、記帳が成った周は列を 1 周撃つ（cli.rs の `GATES`）。`seat ruling bind`（`crates/scribe2/src/seat/ruling.rs` の `bind`）は 4 語の閉じた断りを判定の順に持つ。
  - 裁定 event は kind RulingReceived・actor human・bead は問い id・本体は `crate::fleet::Case` の Ruling 形（裁定 id・発話の ts・経路・問いの起票の時刻 question_ts・asked）。裁定 id は `<問い id>:<発話の YYYYMMDDTHHMMZ>-1`（ruling.rs の `ruling_id`・形の判定は `crates/scribe2/src/ledger/close_reason.rs` の `is_ruling_id`＝問い id の形のほかに batch: と policy: の形も真）。発話 event は kind UtteranceReceived・本体は `Case` の Utterance 形（経路と、chat の行だけの session）・detail は逐語・ts はミリ秒つき。host の log の実物: question_ts は秒の形、発話の ts はミリ秒の形、session は UUID。
  - orchestrator の席は event log の登録 row（`crates/scribe2/src/seat/role.rs` の `registration_of_key` に役割と anchor を渡す・anchor は `--repo` の絶対 path の字）から target を引き、席の置き場（`crates/scribe2/src/seat/mod.rs` の `seat_dir`）の打刻の file を読む。打刻の読み手 `last_sid`（`crates/scribe2/src/seat/state.rs`）は読めた行のうち最後へ戻る（壊れた最終行を飛ばして前の行の会話 id を返す・in-file の歯がそれを pin する）ので、FR110 の「file の最後の行・前の行へ戻らない・壊れていれば rc 2」には使えない。会話 id の形の判定 `is_session_id` は state.rs の私有の関数。
  - 台帳の `crate::ledger::Bead`（`crates/scribe2/src/ledger/mod.rs`）は status・labels・created_at・asked・notes の 5 欄で本文を持たない。literal の構築は同じ file の `bead_of` の 1 か所。bd の show の JSON は本文を key description に持つ。無い bead の show は bd が rc 1 を返し、`show` は読めない（Unreadable）に倒す。
  - 権能の表 `CAPABILITY_COMMANDS`（`crates/scribe2/src/hook/role_guard.rs`・8 行）に `pipe permit` は無い＝今は役割の guard が見ない口になる。行 `role.orchestrator` は approve を持つ。
- 前提（doc の中の順は depends で表す）: 行 b（§18）が着地済みで、event の kind LimitPermitted と形、記帳 1 件の本体（書き手と読み手）・着地の読み（bead の着地した便 id）、rules 行 pipe.permit_rows（List）と pipe.permit_max_h（Int）と manifest の読みの閉じ（FR17）が在る。本行は行 b の file を write-set に入れず、行 b の口を名と可視性のまま呼ぶ（記帳 1 件の書き手と読み手・着地の読み）。効きの純関数と記帳の列の読みは本行では呼ばない（行 d・e の読み手）。
- 約束（番号は done と 1:1）:
  1. **口の形と配線**: PipeCommand の末尾に permit を足す（字は `permit`・const slice の末尾・件数は便の base の列に 1 語足した数）。受ける flag は置き場・repo・規則の 3 つと `--bead`・`--rule`・`--value`・`--until`・`--ruling`・`--bd` と値なしの `--revoke`。形は 2 つ:
     - 記帳 `<NAME> pipe permit --bead B --rule ID --value N --until YYYY-MM-DDTHH:MMZ --ruling ID [--state-dir D] [--repo R] [--rules PATH] [--bd CMD]`（`--repo` は要る＝台帳の cwd と orchestrator の席の anchor）。
     - 取り消し `<NAME> pipe permit --bead B --rule ID --revoke [--state-dir D] [--repo R] [--rules PATH]`。
     - 形の誤り（flag の欠け・`--revoke` と `--value` / `--until` / `--ruling` の併せ持ち）は今の形の rc 1（`pipe: --value が要る` など）で何も読まず書かない。照合の 10 語には数えない。
     - 使い方に 1 行を足す（`usage: <NAME> pipe permit --bead B --rule ID (--value N --until YYYY-MM-DDTHH:MMZ --ruling ID|--revoke) [--state-dir D] [--repo R（記帳の周は要る）] [--rules PATH] [--bd CMD]（…）`）。1 行目の `<…|…>` の語の群は変えない（help の pipe の頁の FORM は 1 行目の逐語なので変わらない）。help の pipe の頁の SUBCOMMANDS に `permit  Raise one cap for one bead on a bound user ruling, or revoke it.` の 1 行、SEE に `docs/design/limit-permit.md` を足す（頁の行数は上限 60 の内）。
     - 列の 1 周は撃たない（`GATES` にも `TERMINALS` にも足さない）。撃ち直しは席の今の手のまま（§5 の MVP）。
     - 入口の manifest の読みが断る周は、permit の周だけ rc 2 の `pipe: permit unreadable source=manifest <読みの理由>` を返す（ほかの subcommand の rc 1 は変えない・cli.rs の arm 1 つ）。
  2. **照らす前の読み（rc 2）**: 記帳の周は次の順に読み、最初に読めない置き場を名指して rc 2・stdout 0 byte・stderr 1 行 `pipe: permit unreadable source=<manifest|event-log|ledger|stamp> <理由>` で止まり、何も書かない（照合の順に関わらない・NFR4）。
     - manifest（約束 1 の入口）。
     - event log の全件（fleet の store の read_all・壊れた行と、形の外の detail を持つ許可の行は行 b の読みが malformed で断るので読めない）。
     - 台帳: 渡された裁定 id の裁定 event（RulingReceived で Ruling 形の裁定 id が一致）が在る周だけ、その event の bead（問い id）を `show` で 1 回読む。読めない（bd が起動できない・rc ≠ 0・JSON が壊れた）周は `source=ledger question=<問い id>`。裁定 event の無い周は読む問いが無いので読まずに照合へ進む（no-ruling が断る）。
     - 打刻: `--repo` を anchor に持つ orchestrator の登録 row が在る周だけ、その席の打刻の file の最後の行を約束 6 の読み手で読む。NotFound の外の理由で開けない周と、最後の行が打刻の形でない周は `source=stamp path=<file> why=<open|last-line>`。登録 row の無い周は読む物が無いので読まずに照合へ進む（not-surface が断る）。
     - 取り消しの周は manifest と event log だけを読み、台帳と打刻は読まない。
  3. **照合 10 語（rc 1）**: 記帳の周は閉じた enum の宣言順（＝下の順）に 1 つの関数で照らし、最初に外れた語で rc 1・stdout 0 byte・stderr 1 行 `pipe: permit refused reason=<語> <名指し>` を返し、何も書かない。名指しは語ごとに閉じる:

     | 順 | 語 | 通る条件 | 名指し |
     |---|---|---|---|
     | 1 | rule-not-listed | 行 id が rules 行 pipe.permit_rows の要素に在る（行が無い・不発効・列でない周は要素 0 として断る） | `bead=<b> rule=<id> listed=<要素を , で繋いだ字\|->` |
     | 2 | not-raise | 値が 10 進の数字だけ（先頭 0・符号・区切り無し）で u64 に収まり、manifest のその行の値（有効な Int）より大きい | `bead=<b> rule=<id> value=<渡された字> declared=<manifest の値\|->` |
     | 3 | no-ruling | 裁定 id が問い id の形（batch: と policy: の形を除く）で、その id の裁定 event が在る | `ruling=<渡された字>` |
     | 4 | no-utterance | 裁定 event の発話の ts と同じ ts の発話 event（最初の 1 件）が在り actor が human | `ruling=<id> utterance=<ts>` |
     | 5 | not-surface | 発話の経路が chat で、発話の session が打刻の最後の行の会話 id と同じ | `utterance=<ts> channel=<chat\|gui> session=<発話の session\|-> stamp=<no-seat\|absent\|blank\|unshaped\|会話 id>` |
     | 6 | before-question | 問いの起票（裁定 event の question_ts）が発話より後でない（約束 7 の判定 1 本） | `utterance=<ts> question=<問い id> question_ts=<字>` |
     | 7 | bad-until | 期限が `YYYY-MM-DDTHH:MMZ` の形で、今より後で、発話の秒に rules 行 pipe.permit_max_h の時間を足した時刻以下（行が無い・不発効の周は断る） | `until=<渡された字> utterance=<ts> max_h=<値\|->` |
     | 8 | reused | 在る LimitPermitted の許可の形のどれも、同じ裁定 id を持たず、その裁定 id の裁定 event の発話の ts が今の発話の ts と違う（bead と行と取り消しに依らない＝1 つの発話に許可 1 つ） | `ruling=<id> utterance=<ts>` |
     | 9 | not-stated | 問いの本文（約束 5 の欄）の語の列に bead・行 id・値（2 の 10 進の字）が字のまま在る。語は ASCII の英数字と `.` `_` `-` の最長の連なりで、末尾の `.` を剥いで比べる（`s2-x.1` は `s2-x.10` に当たらず、`250000` は `2500000` に当たらない）。問いが台帳に無い（show が空の配列）周は 3 つとも欠け | `question=<問い id> missing=<bead,rule,value のうち欠けた語>` |
     | 10 | landed | 行 b の着地の読みが bead の着地した便を返さない | `bead=<b> run=<着地した便 id>` |

     - 打刻の名指し: `no-seat` は登録 row が無い・`absent` は file が無い・`blank` は空白でない行を持たない・`unshaped` は最後の行の会話 id が空か会話 id の形でない（どれも rc 1）。会話 id が取れた周はその字を出す。
     - 時刻は event の字を `crates/scribe2/src/fleet/wait.rs` の epoch の読みで秒にして比べ（字面で比べない）、今は state.rs の `now_secs` で読む。読めない時刻はその語の断りに倒す（6 は before-question・7 は bad-until）。
     - 取り消しの周は 1 だけを照らす（裁定 id を要らない）。
  4. **記帳と出力（rc 0）**: 通った記帳の周は `pipe/mod.rs` の `emit` で kind LimitPermitted・bead・detail（行 b の書き手の許可の形・値は 2 の 10 進の字・until は渡された分の字の末尾 `Z` の前に `:00` を足した行 b の秒の形）の event を 1 件書き（actor は kind の既定＝machine・run を持たない）、stdout に `permit: bead=<b> rule=<id> value=<n> declared=<manifest の値> until=<detail の until の字> ruling=<id>` の 1 行、stderr 0 byte。取り消しは detail を行 b の書き手の取り消しの形で 1 件書き（効いている許可が無い周も書く）、stdout に `permit: bead=<b> rule=<id> revoked`。
     - 書けない周は store の書きの今の極性（rc 2・行は書いていない）に乗る（本行は歯を足さない）。
  5. **台帳の Bead に本文の欄**: `Bead` に description（show の JSON の key description の字・無ければ空）を足す。構築は `bead_of` の 1 か所のまま。結びの口と仕分けの口の振る舞いは変えない。
  6. **打刻の file の最後の行の読み手 1 本**（state.rs・crate の内側・`last_sid` の隣）: 席の置き場を受けて閉じた 6 値を返す（会話 id／file が無い／空白でない行を持たない／最後の行は打刻の形だが会話 id が空か形でない／最後の行が打刻の形でない／NotFound の外の理由で開けない）。最後の行は `lines` の最後の要素で、空白だけの行も最後の行として読む（前の行へ戻らない）。形は既存の `Stamp` の `from_line` と会話 id の形の判定をそのまま使う（写しを持たない）。`last_sid` と `resume_carry` は変えない。
  7. **問いの起票と発話の前後**: 起票が発話より後かは、発話を秒へ切り捨てて比べ、同じ秒は後でない（通る）。読めない時刻は before-question に倒す。doctor の `AskedAfter` の数えの字と数は変えない。
     - 設計の線（歯を持たない・審査が読む）: 判定は ruling.rs の crate の内側の 1 本（後・後でない・読めないの 3 値）に置き、doctor の `AskedAfter` と許可の口の 6 がこれを呼ぶ（比べを 2 か所に写さない・C2）。
  8. **権能の表の行**: `CAPABILITY_COMMANDS` に `pipe permit` を既存の権能 approve で 1 行足す（2 形とも同じ 2 語＝記帳と取り消しの両方が approve・FR41）。新しい権能の名・行 `role.orchestrator` の値・表のほかの行は変えない。
  9. **閉包と極性**: 本行の + の file は EventKind・Stage・RuleKind・Guard・Refuse・Capability・PipeCommand の変種を match の arm と変種の構築の形（変種の名の直後に波括弧か丸括弧を置く形）で名指さない（kind と段は `==` と構造体の欄の初期化で書く）。`Case` の Ruling 形と Utterance 形は `if let` で読む（今 `crate::fleet::Case` を touches に持つ行は 0 本）。`Polarity` の const を宣言しない（結びの口の断りと同じく極性の一覧に載せない）。
- 判定の順と変異の A/B（条件 1 つに歯の脚 1 本・歯の名は下の「歯」）:
  - 1 を常に真にすると w1 が落ちる（列に無い行で rc 0）。取り消しの 1 を外すと r1 の列に無い行の脚が落ちる。
  - 2 の比べを `≥` に緩めると w2 の等しい値の fixture が落ち、数字の読みを緩めると w2 の `250k` の fixture が落ちる。
  - 3 の event の照合を外すと w3 の event の無い id が、形の照合を外すと w3 の batch: と policy: の形が落ちる。
  - 4 の在りを外すと w4 の発話の無い ts が、actor を見ないと w4 の actor machine が落ちる。
  - 5 の経路を見ないと w5 の gui が、session を比べないと w5 の別の会話 id が、打刻の 4 形を通すと s1 が、`last_sid` の読みに替えると s2 が落ちる。
  - 6 を常に真にすると w6 の起票が後の脚が、同じ秒を断る（`<` を `≤` に）と w6 の同じ秒の脚が落ちる。7 の「今より後」を外すと w7 の過去の期限が、窓を外すと w7 の窓を 1 分越える期限が、窓を `<` に締めると w7 の窓ちょうどの脚が、形の照合を外すと w7 の形の外の脚が落ちる。
  - 8 の id の照合を外すと w8 の同じ id が、発話の照合を外すと w8 の別の id で同じ発話が落ちる。
  - 9 を contains に緩めると t1 が、3 語の 1 つを見ないと w9 のその語の欠けの fixture が、本文の欄を読まないと w9 の description の無い問いが落ちる。10 を常に真にすると w10 が落ちる。
  - 照合の順を入れ替えると o1 の該当の組が、読みを照合の後へ回すと u1 の順に関わらない周が、読みの順を入れ替えると u1 の 2 つを同時に壊した組が、裁定 event の無い周にも台帳を読むと u1 の壊れた bd と無い裁定 id の脚が落ちる。
  - 回帰の歯（単独の変異を持たない）: p1 の通る周・h1 の使い方と help・r1・既存の件数 pin と snapshot。
- 閉包: touches は PipeCommand（閉包は cli.rs〔宣言・const slice・arm〕・args.rs〔arm〕・e2e の pipe.rs〔件数 pin〕＝同じ型を touches に持つ既存の 5 行〔row-review.md・reverse-index.md・pipeline.md の 3 行〕の閉包と同じ 3 file で、本行の + の file は PipeCommand を名指さないので広げない）と `crate::ledger::Bead`（閉包は ledger/mod.rs の宣言と `bead_of` の literal の 1 file・Bead を touches に持つ行は 0 本）。約束 9 の形を守る限り、EventKind（dispatcher.md の 2 行と行 b）・Stage（contract-source.md の 1 行）の閉包に本行の + の file は入らない。verify の最後の行の contracts check が便の木で findings 0 を測る。
- 歯（e2e は既存の file に足す・新しい e2e の module を作らない・e2e の file は file-lines と受付の上限の余地の母集団の外〔`crates/*/src` だけを数える〕）:
  - 許可の口の e2e（既存の `crates/scribe2-boundary/tests/e2e/seat/ruling.rs`・接頭辞 `pipe_permit_mouth_`・偽の bd〔show の JSON に description を足す helper を 1 本足す〕と偽の event log と偽の打刻と tmp の置き場・登録 row は同じ置き場に `seat register --anchor <repo>` で積み〔登録は打刻の会話 id を要るので、打刻の形を壊す fixture は登録の後に file を書き換える〕・時刻は撃つ時点の今から組む・manifest は埋め込みで、値〔`gate.token_cap` と `pipe.permit_max_h`〕は埋め込みの manifest から読んで期待を組む〔数を焼かない〕）。各歯は rc・stdout と stderr の byte の完全一致と、event log の byte が撃つ前と同じか（断る周・rc 2 の周）を測る。
    - 通る fixture（p1 の形）: 発話 chat・session = 打刻の最後の行の会話 id（UUID）・actor human、問いの起票は発話より前、本文に bead・`gate.token_cap`・埋め込みの値 + 100000 の字を持つ問い、その組の裁定 event、期限は今から 2 時間後の分。
    - w1〜w10（語ごとに 1 本・通る fixture から条件 1 つだけを外す）: w1 rule-not-listed（列に無い行・本文はその行 id を持つ） w2 not-raise（manifest の値と等しい値と `250k` の 2 fixture） w3 no-ruling（event の無い id と batch: の形と policy: の形の 3 fixture） w4 no-utterance（発話の無い ts と actor machine の 2 fixture） w5 not-surface（gui の発話と別の会話 id の 2 fixture） w6 before-question（起票が発話より後の fixture と、同じ歯の起票と発話が同じ秒の fixture は通る） w7 bad-until（過去の期限・発話の秒 + pipe.permit_max_h 時間を 1 分越える期限・`2099-01-01` の形の外の 3 fixture と、同じ歯の発話を秒 0 に置いて期限を発話 + pipe.permit_max_h 時間ちょうどの分にした fixture は通る） w8 reused（同じ id の 2 度目と、同じ発話に結んだ別の裁定 id の 2 度目） w9 not-stated（bead・行 id・値のどれか 1 つを欠く 3 fixture と、description の key の無い問いが missing=bead,rule,value） w10 landed（bead の便に Landed の RunDone を積む・名指しの run がその便）。
    - o1（AC84 の依存の境目 3 組・1 本）: (i) 渡した id の event は無く同じ問いに起票が発話より後の別の id の裁定 event が在る → no-ruling（問いで引く実装なら before-question） (ii) 発話が無く打刻の file も無い → no-utterance (iii) 発話が無く期限が過去 → no-utterance。
    - s1（打刻の rc 1 の 4 形・1 本）: file が無い（`stamp=absent`）・空の file（`stamp=blank`）・最後の行の会話 id が空・最後の行の会話 id が `sid-1`（どちらも `stamp=unshaped`）がそれぞれ not-surface。同じ歯で登録 row の無い置き場が `stamp=no-seat`。
    - s2（壊れた最後の行・1 本）: 会話 id の合う行の後に `not json` の行を持つ打刻 → rc 2 `source=stamp`・event 0 件（前の行へ戻る実装なら rc 0）。
    - p1（通る・1 本）: event 1 件（kind LimitPermitted・bead・actor machine・run 無し・detail は `rule=gate.token_cap value=<値> until=<渡した分>:00Z ruling=<id>` の完全一致）と stdout 1 行の完全一致（列の 1 周の行が無い）・stderr 0 byte。
    - r1（取り消し・1 本）: 裁定 id 無しの取り消しが event 1 件と 1 行を書き、許可の無い置き場でも 1 件を書き、偽の bd が壊れて打刻の file が dir の置き場でも 1 件を書き（台帳と打刻を読まない）、列に無い行の取り消しが rule-not-listed で rc 1・event 0 件。
    - u1（読めない周・1 本）: 記帳の周の event log（`not json` の行）・manifest（壊れた `--rules`）・台帳（show が rc 1 の偽の bd）・打刻（state.jsonl が dir）がそれぞれ rc 2 で source を名指して event 0 件、取り消しの周の event log と manifest がそれぞれ rc 2、列に無い行と壊れた最後の打刻の行を同時に持つ周が rc 2 `source=stamp`（照合の順に関わらない）、manifest と event log・event log と台帳・台帳と打刻を同時に壊した 3 組がそれぞれ先の source を名指す（読みの順）、show が rc 1 の偽の bd と event の無い裁定 id の周が rc 1 の no-ruling（読む問いが無い）。
    - m1（対象の列の閉じ・1 本）: pipe.permit_rows の値を review.same_kind_stop・pipe.max_live・R-C4-2・role.orchestrator のどれか 1 つにした埋め込みの写しの manifest をそれぞれ `--rules` で渡す記帳が rc 2 `source=manifest` で行 id を名指して event 0 件（file の名は行 id を含まない字にする）。
    - t1（本文の語の境目・1 本）: 本文が `s2-p.70`・値の後ろに 0 を足した字・`gate.token_capx` だけを持つ問いがそれぞれ not-stated、`値 = <値>。` と `bead s2-p.7.` の書き方は通る。
    - h1（使い方と help・1 本）: `pipe` の使い方が permit の 1 行を持ち、`help pipe` の SUBCOMMANDS に `permit` の行と SEE に limit-permit.md、`--revoke` と `--value` の併せ持ちと `--until` の欠けが rc 1 の形の 1 行で event 0 件。
  - 権能の e2e（既存の `crates/scribe2-boundary/tests/e2e/hook.rs`・接頭辞 `hook_role_permit_`・1 本・偽 tmux の席〔stub の席・tmux group の外〕）: orchestrator の登録 row を持つ席から記帳と取り消しの 2 形が通り（rc 0・記録 `role-allow capability=approve`）、登録の無い席から 2 形が deny（`reason=unregistered`・記録 `role-deny capability=approve`）、approve を抜いた行の manifest では orchestrator の席の記帳も deny で approve を名指す（通したのは行の値）。
  - 直す既存の歯: e2e の `pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none` の件数を便の base の数に 1 足し字の列の末尾に permit・`pipe_external_form` の snapshot（使い方の塊 3 か所に 1 行ずつ）。どちらも直した期待が base で落ちるので retroactive の札は要らない。件数は便の base で数え直す（PipeCommand を touches に持つ未着地の行が先に着地したら base の数が上がる）。
  - 緑のまま走らせる既存の歯: lib の `help_table_`・`doctor_asked_after_`・`stamp_sid_reader_returns_only_a_uuid_shaped_last_sid`・`role_guard_capability_commands_match_the_three_word_sequence`、e2e の `cli_help_`・`doctor_asked_after_`・`seat_ruling_bind_`。
  - base で RED: base に `pipe permit` は無く、e2e の歯は使い方の rc と行で落ち（語と rc 2 の行を完全一致で測るので、rc 1 と event 0 件だけで通る負例は無い）、権能の歯は表に行が無く登録の無い席が通るので落ちる（機能不在）。
- 触らない: 行 b の file（kind・形・detail の書き手と読み手・純関数・rules 行と読みの閉じ）・gate と lens（行 d）・`pipe show` と `pipe dispatch ls`（行 e）・`seat ruling bind` と答えの口と `pipe approve` の振る舞い・列（`GATES`・`TERMINALS`）・`last_sid` と `resume_carry`・行 `role.orchestrator`・極性の一覧・ほかの subcommand の manifest の断りの rc。
- 着地の後: 最初の許可を書く前に PATH の binary を入れ替える（§6 の跨版・古い binary は kind LimitPermitted を読めない）。
- 限界:
  - 照合と書きは lock の外の読みと lock の中の追記の 2 段で、同じ裁定を同時に撃つ 2 本は両方通りうる（結びの口と同じ）。跡は log に 2 件残り、reused の後の撃ちは断られる。
  - user が答えた後に orchestrator の席が `/clear` か作り直しで会話 id を変えた周は not-surface で断られる（問いを立て直して答え直す）。
  - 台帳は裁定 event の bead の問いだけを読む。裁定 event の無い id の問いは読まない（no-ruling が先に断る）。
  - 本文の語の読みは ASCII の連なりで、`250,000` や `25 万` は値を持つと読まない（字のまま書く約束）。
  - 役割の guard は pane の無い呼び出し（runner・lens）を見ないが、発話の照合が普通の口では偽れない（§8 H1〜H3 の残りは行 a と §15）。
  - 口は `--rules` を受ける（FR17 の閉じと読めない周の rc 2 を歯が測る口）。席が写しの manifest で pipe.permit_max_h を上げて渡す形は FR112 の断りの外で、許可の期限の上限はその写しの値で照らされる（gate は本行と別の manifest を読むので、効く値は埋め込みの manifest の値より大きい許可だけ）。
- 却下:
  - 裁定 id の `:` の前の問い id で照合の前に台帳を読む案: 無い問いを bd が rc 1 で返すので、打ち間違いの裁定 id が no-ruling でなく rc 2 になる。
  - `last_sid` を使う案: 壊れた最後の行を飛ばして前の行の会話 id を返す（FR110 の「前の行へ戻らない」に反する）。
  - 本文の照合を contains にする案: `s2-x.1` が `s2-x.10` に、`250000` が `2500000` に当たる。
  - 記帳の後に列を 1 周撃つ案（`GATES` に足す）: §5 の MVP は撃ち直しを席の手に残し、道具の受け渡しの広さ（§2.1 後段）を許可の口へ持ち込まない。
  - 断りを極性の一覧に載せる案: 一覧の snapshot と `Guard` の閉包が広がり、同じ形の結びの口の断りも載っていない。
  - 打刻の読み手を本行の + の file に置き会話 id の判定を開く案: 打刻の形の読み手が 2 file に割れる（C2）。
  - 期限を分の字のまま detail に書く案: 行 b の detail の形は event の `ts` と同じ秒の形で、分の形は malformed として event log 全体を読めなくする。

## 20. 行 d — 読み手: gate の周の入口で bead の上限の許可を読み、予算の照合と gate が起こす lens に効く cap を渡す・INCONCLUSIVE の文と Gated の detail と verdict の記録・run dir の写し・event log を読めない周の rc 2（契約表の行 d・[FR111](../../design-intent/spec/srs.html#FR111) / FR9 / NFR1 / NFR4・AC85・ADR-0106）

やさしく言うと: gate は 1 回の周の始めに記録の file を 1 回読み、その作業（bead）に効いている上限の許可が在るかを行 b の関数に聞く。在れば、その周の予算の照合と、その周に起こす検査役（lens）だけが許可の値で数える。lens は別の process なので、gate が lens を起こす直前に「この周の cap はいくつで、出所は設定か許可か」の 1 行を便の置き場に書き、lens はそれを読む。許可で数えた周は、判定の記録と段の記録に「どの行を・いくつに・どの裁定で」を残す。記録の file が読めない周は、許可にも設定の値にも倒さずに止まる。

- 何が起きているか（main 2fbe0c11・verified）:
  - `crates/scribe2/src/pipe/gate.rs` の `Limits` は rules 行 8 本の宣言値の束で、`Limits::of` が manifest だけから組む。呼び手は `crates/scribe2/src/pipe/cli/step.rs` に 4 か所（`gate_run`・`land_run`・終端だけの撃ち直し・着地後の検出だけ）と、`crates/scribe2/src/pipe/dispatch.rs` の遮断器（倍率だけを読む）と `crates/scribe2/src/pipe/cli/base_run.rs`（遮断器だけを読む）。`token_cap` を読むのは gate.rs の 2 か所（main befd2372・verified）: `measure` は diff の周に、§41 の畳みの後の本文の byte が cap を超える周だけ本文の削除の run を畳み（[gate-cost.md](./gate-cost.md) §46 形 2）、畳んだ周は run dir の `verify.stderr.log` の通知の行の末尾に ` pruned=<run 数>/<行数>` を足す。`decide` は lens に渡す本文の byte が cap を超えたら lens を起こさず INCONCLUSIVE にする（文は「<入力の型> <N> byte が cap <M> を超えた」）。
  - gate.rs の `gate` を呼ぶのは 2 か所: step.rs の `gate_run`（`pipe gate` と、同じ関数を通る `pipe resume`）と、`crates/scribe2/src/pipe/land.rs` の追随（main が動いた便の rebase の後の再 gate）。追随は `land_run` が着地の入口で組んだ `Limits` をそのまま `Gate` へ渡す。着地の列（`crates/scribe2/src/pipe/train.rs`）は後続の便の着地の材料を先頭の材料から写す（`limits` も写る）が、列は gate を撃たない。
  - `land_run` の入口から追随の再 gate までは、着地の順番待ち（rules 行 `pipe.land_wait_s`）を挟みうる。
  - event log は gate の周で既に 2 回読まれる: step.rs の入口の段の検査（`crates/scribe2/src/pipe/cli/state.rs` の `resolve` が replay を読む・読めない周は rc 2）と、gate.rs の `gate` の先頭の base の読み（読めない周は rc 2）。どちらも verdict を書く前に止まる。
  - lens は `crates/scribe2/src/headless/lens.rs` で、`rows_of` が manifest（`--rules` か埋め込み）から cap を読み、`prompt_of` が diff か契約の材料の byte と比べる。lens の起動の行は審査が残した run dir の写し（`lens.toml`）の 1 行で `--rules` を持たないので、lens の cap は常に埋め込みの値になる（§2.1 の割れ）。裁定の写しは gate.rs の `keep_rulings` が契約の写しの隣（run dir の直下）へ書き、lens.rs の `rulings_of` が契約の path の隣から同じ名で読む（無ければ「裁定なし」・在るのに読めない周は claude を呼ばず rc 2）。
  - lens の契約の置き場は審査の種類ごとに違う: gate の lens は run dir の直下の契約の写し、契約の審査は run dir の下の `review`（`crates/scribe2/src/pipe/review.rs` の `keep`）、先撃ちは列の置き場（`crates/scribe2/src/pipe/dispatch/prelens.rs`）、memo の審査は `--stage memo` で lens.rs の `dispatch` が契約も裁定も読まずに `memo` へ分かれる。行の審査（row-review.md §3 の口）は main に無く、材料の置き場は row-review.md §9 の state dir の pipe の下の row-review の dir（run dir の外）である。
  - verdict の記録は run dir の `verdict.json`（gate.rs の `settle`・最後の判定で上書き）で、`Gated` の段の event の detail は `verdict:<V>` に口座の項（器が lens の口座を選んだ周）と行 a の出所の項 `,rules:<出所>` を継いだ字。
  - `Gated` の detail を読む手は 2 つ（`crates/scribe2/src/fleet/lifecycle_mark.rs` の `verdict_word` と `crates/scribe2/src/pipe/queue.rs` の `taken_at`）で、どちらも行 a の後は最初の `,` の前で切る。
- 前提（doc の中の順は depends で表す）: 行 b（§18）が着地している: 行 b の write-set の + の file の公開の 2 値の enum Effect（Declared / Permitted〔値・裁定 id・期限〕）と純関数 permitted（行 id・manifest の値・bead・event の列・今の UNIX 秒）。行 d は効くかの判定を書かない（判定は permitted の 1 本・C2）。行 a（§17）は行 b の前に着地し、Gated の detail に manifest の出所を足し、列の番の読みを接尾辞に耐えさせている（本行の `,permit:` の項もその読みに乗る）。
- 約束（番号は done と 1:1）:
  1. **重ねは gate.rs の `gate` の 1 か所**: precheck を通った後・verify を撃つ前に event log を 1 回読み、行 b の permitted を（行 `gate.token_cap`・manifest の値＝`Gate` の `limits` の `token_cap`・`Gate` の bead・読んだ event の列・`crates/scribe2/src/seat/state.rs` の `now_secs` の今）で 1 回呼ぶ。返りは wildcard の無い match で、gate.rs の中の私有の型「効く cap」（値と、許可の周だけ行と値と裁定 id）に写す: Declared は manifest の値、Permitted は許可の値。追随の再 gate も同じ 1 か所を通る。gate は `pipe.permit_rows` を読まない（効きの条件は FR111 の 4 つで、対象の列は記帳の照合〔FR110〕の側）。
     - `Limits` は宣言値の型のまま書き換えない（`Limits::of` の本文・`Limits` と `Gate` の欄・step.rs・land.rs・train.rs・dispatch.rs・base_run.rs は本行の write-set の外）。効く cap は manifest の宣言値と許可の記録から導く実効値で、宣言値の型に入れない（C10・ADR-0106 の決定の文の「manifest の値と別の型で持つ」）。
     - §7 の「`gate_run` と `land_run` で `Limits` を組んだ直後」は、2 つの経路がどちらも通る gate.rs の `gate` が `Limits` を受けた直後で読む（§7 の直しは doc の末尾）。重ねの手を 1 か所にし（C2）、`pipe resume` も `gate_run` を経て同じ所を通り、追随の再 gate も着地の順番待ちの後の再 gate の時点の許可を読み（期限と取り消しを待ちの前の読みで持ち越さない）、列の後続の便へ先頭の便の許可が写る経路を作らない。周の中で値は動かない（1 周に 1 回だけ読む）。
  2. **畳みと文**: `measure` の畳みの閾値と `decide` の cap の照合は、どちらも同じ効く cap の値と比べる（効く cap は約束 1 の読みが `measure` の前に 1 回導き、2 つの私有の関数へ引数で渡す・`Gate` と `Limits` の欄は変えない）。許可の周は manifest の値で畳まない（許可の値に収まる本文は削除の run を逐語のまま lens へ渡し、通知に ` pruned=` を足さない）。許可の無い周の畳みは今のまま。超えた周の INCONCLUSIVE の文は、許可の周だけ cap の値の直後に「（上限の許可 gate.token_cap=<値> ruling=<裁定 id>）」を持つ（例「diff 300 byte が cap 200（上限の許可 gate.token_cap=200 ruling=<裁定 id>）を超えた」）。許可の無い周の文は 1 字も変えない（既存の歯 `pipe_gate_inconclusive_when_diff_exceeds_cap` の「cap 1」を保つ）。上限なしにする枝は持たない（許可の値も越える diff は INCONCLUSIVE）。
  3. **使用の記録**: 効く cap が許可だった周は、判定（PASS・FAIL・INCONCLUSIVE）に依らず、`Gated` の detail の末尾（行 a の出所の後ろ）に `,permit:gate.token_cap=<値> ruling=<裁定 id>` を足し、`verdict.json` に key `permit`（値は同じ字 `gate.token_cap=<値> ruling=<裁定 id>`・`ts` の前）を足す。schema は 1 のまま。許可の無い周は detail も `verdict.json` の key の列も変えない（key の列を完全一致で見る既存の歯 3 本と、detail を完全一致で見る既存の歯を変えない）。
  4. **切れ方の読み**: 期限の後・bead の便の着地の後・取り消しの後・同じ bead と行の新しい記帳の後・許可の値が今の manifest の値以下の周の読み分けは permitted が持ち、gate はその返りだけを読む（新しい記帳の期限が古い許可より先に切れても古い許可へ戻らない・FR111）。
  5. **run dir の写しの書き手**: gate は lens を起こす周だけ、`keep_rulings` の直後（lens の口座を選ぶ前）に、契約の写しの隣（run dir の直下）の `cap.txt` を 1 行で書き直す: 許可の無い周は `gate.token_cap=<manifest の値> source=manifest`、許可の周は `gate.token_cap=<許可の値> source=permit ruling=<裁定 id>`。書けない周は lens を起こさず rc 2（`verdict.json` も `Gated` も書かない・`keep_rulings` と同じ極性）。lens を起こさない周（機械検証の段で止まった周・cap を超えた周・lens が無い周）は書かない（前の周の写しは残るが、読むのは同じ gate が次に起こす lens だけで、起こす直前に書き直す）。lens の起動の行・`lens.cmd` の穴・flag は変えない。
     - 設計の線（歯を持たない・審査が読む）: 名の定数と 1 行の書き方と読み方は gate.rs に 1 組だけ置き、lens.rs はその読み方を呼ぶ（C2）。
  6. **lens の読み手**: lens.rs の `dispatch` は、memo の審査の枝に分かれた後（memo は写しを読まない）、`rows_of` で manifest の cap を今のとおり先に読み（行が解けない周は今のとおり rc 2）、契約の path の隣に `cap.txt` が在ればその値を cap に置き換えて `prompt_of` へ渡す。無ければ manifest の値。在るのに読めない周（開けない・dir が置かれている・UTF-8 でない・1 行が上の 2 形のどちらでもない〔値が整数でない・`source=permit` で裁定 id が空〕）は claude を呼ばず rc 2 で「lens: cap の写しを読めない: <path>: <理由>」の 1 行。写しの値は manifest の値より小さくても大きくても写しが勝つ（gate の周の効く cap と lens の cap を 1 つにする・§2.1 の割れを閉じる）。usage と known flag は変えない。
  7. **審査の lens は manifest**: 写しを書くのは gate だけで、置くのは gate の契約の写しの隣だけなので、契約の審査（run dir の下の `review`）・先撃ち（列の置き場）・行の審査（row-review の dir）の lens の契約の隣には写しが無く manifest を読み、memo の審査は写しを読む前に分かれる（材料の隣に写しが在っても manifest）。
     - 設計の線（歯を持たない・審査が読む）: lens は event log を読まない（許可を event log から引く 2 本目の読み手を lens に作らない）。
  8. **読めない周**: 1 の読みが読めない周（event log の行の形が壊れている・開けない）は、許可の値にも manifest の値にも倒さずに rc 2 で「pipe: event log を読めない（上限の許可を照らせない）: <最初の理由>」の 1 行を出して止まり、verify を撃たず、`verdict.json` も `Gated` も `cap.txt` も書かない。今の main では同じ log を先に読む step.rs の入口（`resolve`）と gate.rs の base の読みが先に rc 2 で止めるので、1 の読みが単独で読めないのは読みの間の書き換えだけで、極性を揃える（許可へ倒す枝を持たない・§7 の祖先の約束）。
- 閉包: 本行は閉じた型に variant を足さない（`touches` を持たない）。gate.rs に行 b の Effect の wildcard の無い match を 1 本足すので、Effect に variant を足す後の行は gate.rs を write-set に持つ（今 Effect を `touches` に持つ行は無い）。gate.rs の私有の `Decision` に欄を 1 つ足す（literal は `gate` の 1 か所）。`Gate`・`Limits`・`Measured`・`Land`・`Event` の欄は変えない。本行は + の file を持たない。
- 大きさ: gate.rs の src は約 95 行（読みの私有の純関数・効く cap の型・写しの書き手と 1 行の書き方と読み方・文と detail と key）。`gate` と `decide` は R-C4-4 の 60 行の線に近いので、どちらも数行の呼び出しだけを足し、本体は私有の関数に置く（`decide` の引数は 5 つまで・`measure` は効く cap の引数を 1 つ足す）。lens.rs は約 30 行（写しの読み 1 本と `dispatch` の数行）。
- 歯（e2e は既存の file に足す・新しい module を作らない・変異の A/B は条件 1 つに歯の脚 1 本）:
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`・名は `pipe_gate_permit_` で始まる・16 本）。fixture: 既存の helper（`write_rules` で cap を小さくした manifest・`intake_bead` と spawn の 2 bead・偽 lens の marker・`verdict_pairs`・`gated_details`・`stop_run_ok`）と、行 b の形の上限の許可の event の 1 行（bead と detail・actor machine）を置き場の event log の file へ足す helper 1 本（pipe の子の module から呼べる可視性で置く・`fleet record` は便の形の kind しか書けないので使わない）。期限は未来を 2099 年・過去を 2000 年の UTC の秒の形（行 b の detail の形）で書き、裁定 id は bead id にも出力の他の字にも現れない字（例 `s2-rq9.1`）にする。許可の無い側の脚（manifest の値を読む）は base でも通るので、各歯は同じ歯の中に許可で通る脚（base は INCONCLUSIVE）を持ち、base で RED になる。
    - (a) 許可の効き（gate の照合）: manifest の cap 1・bead A に値 1000000 の許可・偽 lens で、A の便は rc 0 で PASS し lens が起きる。同じ歯で pipe.permit_rows を不発効にした写しの manifest でも A の便は PASS（gate は対象の列を読まない）。
    - (b) 許可の効き（gate が起こす lens）: gate の `--rules` と lens の行の `--rules` を、埋め込みの manifest の写しの `gate.token_cap` だけを 1 にした file にし、lens の行は器の lens と判定の封筒を返す偽 claude（既存の `max_turns_lens` と同じ作り）にする。A の便は PASS で偽 claude が 1 回起きる（lens の写しの読みを外すと lens が「diff exceeds cap」を返し INCONCLUSIVE）。
    - (c) 持たない bead の止め: A に許可を置いたまま B の便を先に撃ち、INCONCLUSIVE で文が「cap 1 を超えた」で「上限の許可」を持たず、detail と `verdict.json` に permit が無く、lens も写しも無い。B を `stop_run_ok` で外した後、同じ歯の A の便は PASS。
    - (d) 許可の値の上限: manifest の cap 1・A の許可の値 2（diff より小さい）で INCONCLUSIVE、文が「cap 2（上限の許可 gate.token_cap=2 ruling=<id>）を超えた」、`verdict.json` の `permit` と detail の末尾が `gate.token_cap=2 ruling=<id>` を持ち、lens は起きない。
    - (e) 使用の記録: A の許可で PASS した周の detail の末尾が `,permit:gate.token_cap=1000000 ruling=<id>`、`verdict.json` の `permit` が同じ字、`cap.txt` が `gate.token_cap=1000000 source=permit ruling=<id>`。同じ歯で偽 lens が FAIL を返す A の便の周も detail の末尾と `verdict.json` に同じ permit を持つ（判定に依らない）。同じ歯の許可の無い bead B を cap 1000000 の manifest で撃った周は PASS で、detail が行 a の出所の項で終わり `,permit:` を持たず、`verdict.json` に `permit` が無く、`cap.txt` が `gate.token_cap=1000000 source=manifest`。
    - (f) 切れ方・期限: 期限が過去の許可だけを持つ A の便は INCONCLUSIVE で manifest の文。同じ歯で期限が未来の新しい許可を足して撃ち直すと PASS。
    - (g) 切れ方・着地: 許可の後に A の別の便の `RunCreated` と段 Landed を `fleet record` で足した後の A の新しい便は INCONCLUSIVE で manifest の文。同じ歯の許可を持つ別の bead C の便は PASS。
    - (h) 切れ方・取り消し: 許可の後に取り消しの 1 行（`rule=gate.token_cap revoked`）を持つ A の便は INCONCLUSIVE で manifest の文。同じ歯で新しい許可を足して撃ち直すと PASS。
    - (i) 切れ方・新しい記帳: 値 1000000 の許可の後に値 2 の許可を持つ A の便は INCONCLUSIVE で文が「cap 2（上限の許可 gate.token_cap=2 ruling=<新しい id>）」（古い値と古い id を持たない）。
    - (j) 古い許可へ戻らない: 期限が未来の値 1000000 の許可の後に期限が過去の新しい許可を持つ A の便は INCONCLUSIVE で manifest の文（古い許可の id を持たない）。同じ歯で 3 つ目の許可を足して撃ち直すと PASS。
    - (k) manifest 以下の不効: manifest の cap 10・許可の値 5 の A の便は INCONCLUSIVE で文が「cap 10 を超えた」で「上限の許可」を持たず detail に permit が無い。同じ歯で値 1000000 の許可を足して撃ち直すと PASS。
    - (l) 審査の lens の不効・契約の審査: (e) と同じく A の許可で PASS した便の run dir の `review` の契約の写し（材料つき）に、器の lens を cap 1 の manifest の写しで撃つと「contract material exceeds cap」で偽 claude は起きない（run dir の直下の `cap.txt` は許可の値を持ったまま）。
    - (m) 審査の lens の不効・行の審査と先撃ち: (e) と同じく A の許可で PASS した後、置き場の下の `cap.txt` を全部数えると 1 本で、gate の契約の写しの隣（run dir の直下）だけに在り、`review` の下と run dir の外（row-review の dir と列の置き場の形の dir）に無い。
    - (n) 読めない周の rc 2: 許可を持つ A の便の event log の末尾に壊れた行を 1 行足して撃つと rc 2 で、`verdict.json` も `cap.txt` も無く、lens も起きず、log の file の `Gated` の行が増えない。同じ歯で壊れた行を除いて撃ち直すと PASS。
    - (t) 写しを書けない周: run dir の直下の `cap.txt` の場所に dir を置いた許可を持つ A の便は rc 2 で lens が起きず `verdict.json` が無い。同じ歯で dir を除いて撃ち直すと PASS。
    - (u) 畳みの閾値: 便の commit が 16 行以上の削除だけの run を持つ file の削除を含み、§41 の畳みの後の本文の byte が manifest の cap を超える diff で、値 1000000 の許可を持つ A の便は PASS し、run dir の `verify.stderr.log` の通知の行が ` pruned=` を持たない（畳みの閾値が manifest の値のままの実装は PASS でも ` pruned=` を持つので落ちる）。同じ歯の許可の無い bead B の同じ形の diff の便は通知の行が ` pruned=` を持つ。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs`・名は `pipe_follow_gate_permit_` で始まる・1 本・許可の 1 行は上の helper で置く）: (o) 追随の再 gate: 許可の無い manifest で PASS した A の便の後に検出線の面に触れる commit で main を動かし、許可を足して `pipe land --rules`（cap 1 の manifest）を撃つと、再 gate が許可で PASS し、detail に permit を持つ `Gated` が 1 件増え、便が着地する。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/headless/lens.rs`・名は `headless_lens_permit_cap_` で始まる・3 本・器の lens を直に撃つ）: (p) 契約の隣の `cap.txt`（`source=permit`・値 100）と cap 10 の manifest で 11 byte の diff は claude を呼んで判定を返し、同じ歯で `source=manifest`・値 5 の写しは「diff exceeds cap」、写しを消すと manifest の 10 で「diff exceeds cap」。(q) 写しが dir・値が整数でない・`source=permit` で裁定 id が無い の 3 形はどれも rc 2 で path を名指し claude を呼ばない。(r) memo の審査: memo の材料の file の隣に値 100 の `source=permit` の写しを置き、cap 10 の manifest で `--stage memo` を撃つと偽 claude が受けた prompt の材料は先頭 10 byte で切れている（同じ歯の diff の形で同じ dir の写しは値 100 で claude を呼ぶ）。
  - lib（既存の gate.rs の末尾の歯の区間・名は `gate_cap_read_` で始まる・1 本）: (s) 1 の読みの私有の純関数が、読めない結果を受けると「event log」を名指す `Err` を返し（manifest の値へ倒さない）、空の event の列を受けると manifest の値で許可が無い。
  - 直す既存の歯: 無い（許可の無い周の文・detail・`verdict.json` の key の列・lens の読みは変えない）。retroactive の札は要らない。
  - base で RED: base の gate は許可を読まず（許可で通る脚が INCONCLUSIVE）、`cap.txt` を書かず、base の lens は写しを読まず（(p)(q) の 1 脚目が判定を返す側・rc 0）、lib の 1 本は base に無い関数を呼ぶので overlay の後に compile が落ちる（どれも機能不在）。
- 触らない: `Limits::of` と `Limits`・`Gate`・`Land` の欄・step.rs・land.rs・train.rs・dispatch.rs の遮断器・base_run.rs・queue.rs（列の番の読みは行 a）・lens の起動の行と `lens.toml`・審査の材料の置き場・状態の語の重なり（行 e）・`pipe show` と `pipe dispatch ls`（行 e）・許可の口（行 c）。
- 限界:
  - 行の審査の口は main に無いので、その lens の不効は写しの置き場（歯 (m)）と lens の読みの形（歯 (p)・(l)）で測る。口が本行より先に着地したら、口を撃つ形の脚を (m) に足す。
  - 写しの値は gate が書いた値をそのまま信じる。席が gate の周の間に run dir の写しを書き換える形は測らない（置き場は席の権能 edit-outside の内側・§8 の根の限界と同じ）。lens を起こす直前に書き直すので、前の周の写しや席が先に置いた写しは使われない。
  - 1 の読みが単独で読めない周は、同じ周の先の 2 回の読みとの間に log が壊れた周だけで、e2e では起こせない（(n) は周の止まり方を測り、その rc 2 は先の読みでも出るので約束 8 の弁別は (s) が持つ）。
- 却下:
  - `gate_run` と `land_run` で `Limits` の `token_cap` を許可の値に書き換える（§7 の元の字どおりの形）: 宣言値の型に実効値を入れる（C10・ADR-0106）。重ねの手が 2 か所になり、`land_run` の入口の読みは着地の順番待ちの後の再 gate へ持ち越され、列の後続の便の材料に先頭の便の許可が写る。
  - lens が event log から bead の許可を引く: lens は bead を知らず、許可の読み手が 2 process に割れる（§2.1 の型）。契約の審査・行の審査・memo の lens にも許可が届きうる。
  - `verdict.json` に毎周 `cap` の key を足す（NFR1 の「照合を記録」を字どおりに）: key の列を完全一致で見る既存の歯 3 本を書き換えることになり、FR111 と AC85 が求めるのは許可の周の記録だけ。

## 21. 行 e — 見え方: pipe show --run が bead の許可を状態の語 6 つで、pipe dispatch ls が効いている許可を 1 行ずつ出す（契約表の行 e・[FR111](../../design-intent/spec/srs.html#FR111)・AC85・NFR4・ADR-0106）

やさしく言うと: 便を 1 本見る口（`pipe show --run`）に、その作業（bead）に出た上限の許可を 1 件 1 行で並べ、いま効いているか、効いていないならなぜか（着地した・取り消した・新しい許可に替わった・期限が切れた・manifest の値が追い越した）を 1 語で添える。列を見る口（`pipe dispatch ls`）には、いま効いている許可だけを並べる。どちらも見せるだけで、何も書かない。

- 何が起きているか（main 2fbe0c11・verified）:
  - `pipe show` は `crates/scribe2/src/pipe/cli/show.rs` の `show` が、便の段の 1 行・gate の検出線の写しの行（`detection_lines`）・消費の行（`cost_lines`）・便ごとの消費の検出線の判定行（`ceiling_of` → `ceiling_line`）をこの順に出す。判定行の閾値は `manifest_of`（`--rules` か埋め込み）と `int_row` で読み、消費の event が 1 件も無い便は manifest を読まずに行を出さず、閾値を読めない周は `cost-ceiling: unmeasured events=<n>` を出す（0 に畳まない）。event log を読めない周は rc 2。file は in-file の歯を持たない。
  - `pipe dispatch ls` は `crates/scribe2/src/pipe/cli.rs` の `queued` が `crates/scribe2/src/pipe/dispatch.rs` の `observe` を撃つ。`observe` は列の 1 周の読み（`measure`・台帳と event log を 1 回ずつ読む）から、候補の `[DISPATCH]` の行・依存待ちの事前審査の `[DISPATCH-PRECHECK]` と直しの束の `[DISPATCH-BUNDLE]`（件数の行の前）・`[DISPATCH-COUNT]` を出し（候補 0 は `[DISPATCH-NONE]`、台帳を読めない周は `[DISPATCH-UNMEASURED reason=…]` の 1 行だけ）、その後ろに memo の `[DISPATCH-MEMO]` の行を足す（`crates/scribe2/src/pipe/dispatch/memo.rs` の `lines`・今の時刻は `crates/scribe2/src/seat/state.rs` の `now_secs`）。読みの値 `Read` は event の列を `Option` で持つ（event log を読めない周は `None`）。
  - 出力の字を pin する既存の歯は、許可の記帳を持たない置き場だけで測る: `[DISPATCH-NONE]` の 1 行の完全一致（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` に 2 本・`crates/scribe2-boundary/tests/e2e/pipe/dispatch/group.rs` に 2 本）・`[DISPATCH-NONE]` と memo の行の列の完全一致（`crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs` に 1 本）・`pipe show` の gate 前の 1 行と gate 後の残りの snapshot（`crates/scribe2-boundary/tests/e2e/pipe/gate.rs` の `pipe_record_show_external_form`）・`cost-ceiling:` の行だけを拾う歯（`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs` と `crates/scribe2-boundary/tests/e2e/pipe/spawn/approval.rs`）。help は出力の行を書かない。
  - 隣の project の面と管理の道具（追跡外）は `pipe dispatch ls` の行頭 `[DISPATCH] ` の行の key と `pipe show` の 1 行目の `stage=` だけを読む。
- 前提（doc の中の順は depends で表す）: 行 b（§18）が置いた公開の口 — 閉じた 2 値の enum Effect・純関数 permitted・記帳の列の読み records・記帳 1 件の本体の読み手・着地の読み landed・期限の述語 expired — を名・可視性・返りを変えずに呼ぶ。行 b の file は書き換えない（本行の write-set に入れない）。行 c（§19）と行 d（§20）の後に着地する（§12 の順）。最初の許可の前の binary の入れ替え（§6）は行 b の側の約束で、本行は読むだけなので跨版の面を足さない。
- 約束（番号は done と 1:1）:
  1. 状態の語の判定を行 e の + の file に置き、着地と期限は行 b の公開の述語を呼ぶ（写しを持たない・C2）。
     - 閉じた enum 1 つ（6 値・宣言順は landed・revoked・superseded・expired・overtaken・active＝先に当たる順）と、語の字を返す関数 1 本。語の字は §4.5 と FR111 の 6 語のまま。
     - 純関数 1 本（入力は permitted と同じ 5 つ: 行 id・manifest の値・bead・event の列・今の時刻）が、records の返す記帳の列のうち許可の要素ごとに、記帳の順で（許可・語）を返す。語は enum の宣言順に照らして最初に当たる語: landed（bead の便が着地）・revoked（同じ bead と行の次の記帳が取り消し）・superseded（次の記帳が新しい許可）・expired（期限の後）・overtaken（値が manifest の値以下）・active（どれにも当たらない）。active の許可は高々 1 つ（列の最後の許可だけが後ろに記帳を持たない）で、在れば permitted が返す許可と値・裁定 id・期限が同じ。
     - 純関数をもう 1 本: 許可の記帳を持つ（bead・行 id）の組を、bead と行 id の字の順に重ねずに返す。
     - 設計の線（歯を持たない・審査が読む）: 着地は行 b の landed を、期限は行 b の expired を呼ぶ（permitted と同じ述語で、写しを持たない）。組の純関数は取り消しだけを持つ組を返さない（効いている許可を持ちえない組で、一覧に出ないので外から測れない）。
  2. `pipe show --run` は今の行（段・検出線・消費・cost-ceiling）の後ろに、便の bead の許可を行 id の字の順・記帳の順に 1 行ずつ出す: `permit: rule=<行 id> value=<値> declared=<manifest の値> until=<記帳の until の字> ruling=<裁定 id> state=<語>`。
     - manifest は `ceiling_of` と同じ読み（`manifest_of` と `int_row`・`--rules` か埋め込み）で、今の時刻は `now_secs` の 1 回の読み。
     - 行の値を読めない行 id（manifest を読めない・行が無い・不発効・整数でない）は `permit: unmeasured rule=<行 id> records=<許可の数>` の 1 行にして語を出さない（読めないのに語を言わない・C10）。
     - 許可の記帳を持たない bead の周は manifest を読まず、出力は今と 1 byte も変わらない。取り消しの記帳は単独の行にせず、前の許可の語（revoked）で見せる。
  3. `pipe dispatch ls` は memo の行の後ろに、効いている許可（組ごとに permitted が許可を返すもの）を bead と行 id の字の順に 1 行ずつ出す: `[DISPATCH-PERMIT] bead=<bead> rule=<行 id> value=<値> declared=<manifest の値> until=<記帳の until の字> ruling=<裁定 id>`。
     - event log を読めない周は `[DISPATCH-PERMIT-UNMEASURED reason=events]`、行の値を読めない行 id は `[DISPATCH-PERMIT-UNMEASURED reason=rules rule=<行 id>]` を出す（効いている許可 0 件と融合しない・C10・NFR4）。効いている許可が 0 件の周は行を足さない。
     - 台帳を読めない周（`[DISPATCH-UNMEASURED reason=…]` の 1 行の周）は memo の行と同じく許可の行も足さない（列の読みが無い周）。
     - 設計の線（歯を持たない・審査が読む）: event の列は列の 1 周の同じ 1 回の読み（`Read` の event の列）を使い、event log を読み直さない。今の時刻は `now_secs` を 1 回だけ読んで memo の行と同じ値を渡す。manifest は列の材料の manifest（`--rules` か埋め込み）を `int_row` で読む。
  4. 既存の行の字と並びは変えない: `pipe show` の 1 行目と検出線・消費・cost-ceiling の行、`pipe dispatch ls` の `[DISPATCH]`・`[DISPATCH-COUNT]`・`[DISPATCH-NONE]`・`[DISPATCH-UNMEASURED reason=…]`・事前審査・束・memo の行。新しい行の頭 `permit:` と `[DISPATCH-PERMIT`（`-UNMEASURED` を含む）は、既存の行頭とも、隣の project の道具が読む行頭 `[DISPATCH] ` の形とも当たらない。既存の行の key を変えないので、隣の project の席へ知らせる行は要らない（後で既存の行の key を変える便は 1 行知らせる）。
- 置き方（設計の線・歯を持たない・審査が読む。置き場と読む型は e2e の出力でも contracts check の閉包でも測れないので done の項目にしない）: 状態の語と dispatch ls の行の組み立ては行 e の + の file（dispatch の子 module・memo の行と同じ置き方・状態の語の口は show.rs から呼べる crate の内側の可視性）に置き、dispatch.rs の変更は module の宣言と `observe` の中の今の時刻の 1 回の読みと呼び出しだけ（dispatch.rs の余地は約 195 行）。show.rs は同じ file の中に組み立ての関数 1 本と `show` の呼び出し 1 行を足す。行 e の + の file が読む型は行 b の公開の口（記帳の本体 `Record` の許可の値・期限・裁定 id と、`Effect`・`records`・`landed`・`expired`・`permitted`）で、`EventKind`・`Stage`・`WaitReason` の match の arm と変種の構築を持たない（`pipe show` の語は効いていない許可の値・期限・裁定 id も出すので `Effect` だけでは組めない）。
- 閉包: 本行は既存の閉じた型（`EventKind`・`Stage`・`WaitReason`・`RuleKind`）に変種を足さず、touches を持たない。足す閉じた enum は行 e の + の file の中で新しく、読み手は同じ file の語の字の関数だけ。
- 歯（e2e・既存の `crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs`。pipe の下に新しい e2e の module を作らない。親 2 つの helper〔`ls`・`fake_bd`・`dispatch_rules`・`show_line`・`embedded_int`・`write_rules`〕を使う）:
  - fixture: 置き場の event log に、便の `RunCreated`・着地の `RunStage`（段 Landed）・許可と取り消しの記帳を、行 b の on-disk の形（kind・`bead`・`detail` の閉じた key の列・actor machine）の 1 行ずつで足す（`pipe permit` を撃たない＝行 c の照合を通さずに状態を作る・許可の 1 行は行 d の歯の helper を呼んでよい）。期限は未来（2099 年）か過去（2020 年）の UTC の秒の形で、時計を差し替えない。`pipe show` の manifest の値 D は埋め込みの `gate.token_cap` の値（`embedded_int` で読む・字で書かない）、`pipe dispatch ls` の manifest の値は `dispatch_rules` の写しの値。行は 1 行の完全一致で照らす（contains で照らさない）。
  - 語 6 つ（接頭辞 `pipe_show_permit_word_`・各歯は判定の順の条件 1 つを足す前と後を同じ歯で撃つ）: (a) active — 値 D+1・期限は未来の許可が上の形の 1 行で最後の行に出て、1 行目は今の形のまま。許可を持たない bead の便には `permit:` の行が無く、ほかの bead の許可も出ない (b) landed — 同じ許可が bead の便の着地の前は active・後は landed (c) revoked — 取り消しの前は active・後は revoked で、`permit:` で始まる行はどちらもちょうど 1 行（取り消しを単独の行にしない） (d) superseded — 2 つ目の許可の前は 1 つ目が active・後は 1 つ目が superseded で 2 つ目が active（記帳の順に 2 行） (e) expired — 期限が過去の許可は expired・同じ値で期限が未来の許可（別の bead）は active (f) overtaken — 値 D の許可は overtaken・値 D+1 は active、同じ歯で `--rules` の写し（`write_rules` で値 D+10）を渡すと値 D+1 の許可も overtaken。
  - 重なり 5 組（同じ接頭辞・先の条件を外すと後の語が出る形）: (g) landed と revoked — 取り消しの後は revoked、さらに着地の後は landed (h) landed と superseded — 新しい許可の後は 1 つ目が superseded、着地の後は 2 行とも landed (i) revoked と expired — 期限が過去の許可は expired、取り消しの後は revoked (j) superseded と expired — 期限が過去の 1 つ目は expired、2 つ目の許可の後は superseded (k) expired と overtaken — 値 D・期限が未来は overtaken、値 D・期限が過去（別の bead）は expired。
  - 読めない値（同じ接頭辞）: (l) manifest に無い行 id の許可は `permit: unmeasured rule=<行 id> records=1` で `state=` を含む行が無く、同じ bead の `gate.token_cap` の許可は語の行を持つ（行 id の字の順に並ぶ）。
  - 一覧（接頭辞 `pipe_dispatch_ls_permit_`・偽の台帳は空）: (m) bead 6 つ（効いている 2 つ・取り消し・期限切れ・着地・manifest の値以下）と新しい許可に替わった 1 つで、`[DISPATCH-PERMIT]` の行は効いている 2 本と替わった後の許可の 1 本だけが bead の字の順（`s2-p.10` が `s2-p.9` の前）に出て、1 行目は `[DISPATCH-NONE]` のまま。同じ置き場と同じ `--rules` の写しで各便の `pipe show` を撃つと 6 語が全部出て、`state=active` の行の（bead・値・裁定 id）の集合が `[DISPATCH-PERMIT]` の行の集合と等しい（permitted と状態の語の一致） (n) event log の位置に dir を置いた周は `[DISPATCH-PERMIT-UNMEASURED reason=events]` が最後の行で、同じ歯の読める event log（許可の記帳 0 件）の周は `[DISPATCH-PERMIT` で始まる行が無く、同じ歯の偽の bd が rc 1 の周は許可の記帳が在っても `[DISPATCH-UNMEASURED reason=…]` の 1 行だけ (o) 写しに無い行 id の許可は `[DISPATCH-PERMIT-UNMEASURED reason=rules rule=<行 id>]` を出し、同じ置き場の `gate.token_cap` の効いている許可は `[DISPATCH-PERMIT]` の行で出る。
  - 直す既存の歯: 無い（retroactive の札は要らない）。出力を pin する既存の歯（上の「何が起きているか」の 6 本と `run_cost_ceiling_`）は fixture に許可の記帳を持たないので字が変わらず緑のまま（done の定義の全 nextest で測る）。helper は `#[test]` の外なので `clippy::expect_used` の `#[expect]` を付ける。
  - base で RED: base の器は許可の行を 1 行も出さないので 15 本とも assert で落ちる（機能不在。fixture の kind は行 b の着地で base に在る）。
- 触らない: 許可の口（行 c）・gate の照合と lens と Gated の detail（行 d）・`cost-ceiling:` の行・局面の出力（case-lifecycle.md・§4.5 のとおり後の行）・help・rules 行。
- 限界:
  - 台帳を読めない周の `pipe dispatch ls` は許可も出さない（列の 1 周の読みが無い周）。許可だけを見るには `pipe show --run` を使う。
  - 取り消しの記帳そのもの（効いている許可が無い周の取り消しを含む）は 1 行にしない。event log には残る。
  - 使用（どの gate の周が許可の値を読んだか）は行 d の Gated の段の detail と verdict の記録に在り、本行の行は出さない。
  - 今の時刻は本物の時計で、期限ちょうどの境目は行 b の述語の比べに従う（本行の歯は期限の境目を測らない・行 b の歯 (m) が測る）。
- 却下:
  - 状態の語の判定を行 b の + の file に置く案: 行 e の write-set に行 b の file が入るが、行 b の着地の前は base に無い path を素で書けず（契約表の検査が解けない）、+ で書くと行 b の着地の後に受付が「base に在る file の新規宣言」で断る。期限の述語を行 b の公開の口にすれば写しは生まれない。
  - permitted の返りだけで語を出す案: 2 値なので active でない 5 語を弁別できない。
  - `[DISPATCH]` の候補の行に `permit=` の key を足す案: 隣の project の道具が読む行の字が動き、効いている許可を持つ bead が候補でない周（着地の前の Gated の待ちなど）は出せない。
  - 許可の一覧の口（`pipe permit ls`）を足す案: 口が 1 つ増える（C17.2）。FR111 は `pipe show` と `pipe dispatch ls` を名指す。
  - 効いている許可 0 件の周に 0 件の行を出す案: 許可の記帳の無い置き場の `pipe dispatch ls` の字が変わり、`[DISPATCH-NONE]` の完全一致の歯 5 本を直すことになる。読めない周の行が別に在るので 0 件と融合しない。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "器の hook の入口が席の道具の呼び出しの 3 形（hook の subcommand の直撃・置き場の event log への Edit / Write と shell の書き込み・pipe gate / land / resume への --rules）を権能の guard の後ろ・走っている便の行の門の前で形ごとの理由の語で断り（手動の 1 周の --rules は断らない）、gate の Gated の detail の末尾に読んだ manifest の出所（embedded か --rules の file の blob id）を書き、列の番の読みを判定の語の後ろの項に耐えさせる（§17・FR112・AC86・FR45）"
req = ["FR112", "AC86", "FR45", "NFR5"]
section = "17"
touches = ["crate::polarity::Guard"]
write-set = ["+crates/scribe2/src/hook/bypass_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/hook/host_guard.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2-boundary/tests/e2e/hook/guards.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_bypass_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_bypass_deny_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_rules_source_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_rules_source_", "cargo nextest run -p scribe2 --lib --no-tests=fail gate_rules_word_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_taken_suffixed_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_lens_account_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_findings_counts_are_recorded_per_category", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_regates_after_inconclusive", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_lens_reread_unreadable_then_readable_passes_with_two_calls", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_health_busy_run_stays_gated_and_can_be_regated", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_the_three_added_guards", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_question_guard_is_in_loop_fail_open", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_land_anchor_sync_and_retire_clean_as_in_loop_fail_closed", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_contract_table_as_a_post_hoc_fail_closed_guard", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_command_guard_as_in_loop_fail_closed_right_after_cap_guard", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_lists_live_row_guard_between_role_guard_and_contract_table", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail polarity_external_form"]
size = "M"
growth = ["crates/scribe2/src/hook/bypass_guard.rs:180", "crates/scribe2/src/hook/mod.rs:11", "crates/scribe2/src/hook/host_guard.rs:12", "crates/scribe2/src/polarity.rs:8", "crates/scribe2/src/pipe/gate.rs:65", "crates/scribe2/src/pipe/cli/step.rs:2", "crates/scribe2/src/pipe/land.rs:1", "crates/scribe2/src/pipe/queue.rs:30", "crates/scribe2-boundary/tests/e2e/hook/guards.rs:270", "crates/scribe2-boundary/tests/e2e/polarity.rs:25", "crates/scribe2-boundary/tests/e2e/pipe.rs:12", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs:60", "crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs:55"]
done = "(1) 門 1 本（行 a の + の file・hook/mod.rs に pub mod の 1 行と呼び出し）を pre_tool_use が権能の guard の断らなかった周の後ろ・走っている便の行の門の前で撃ち、断りは rc 2・stdout 0 byte・stderr 1 行・inject.jsonl の what が bypass-deny と理由の語の 1 行で、pane の有無と役割に依らない (2) 理由の語は閉じた 3 語（宣言順 hook-subcommand / event-log-write / rules-swap）で、Bash の周は形の宣言順に全 segment を照らして最初に当たった形の 1 行を返し（形の宣言順が segment の順に勝つ）、1 行は NAME: deny bypass reason=<語> で始まり当たった字と FR112 と語ごとの次の一手を持つ (3) Bash は起票の門の segments で分けた列に、頭の語（前の代入を除く）の basename が sh・bash・zsh・dash・eval の segment の - で始まらない後ろの語を同じ分け方で分けた segment を足して読み（足した segment にも繰り返す）、語は前後の括りを外して比べる (4) hook-subcommand は segment の連続した 2 語が hook と event の 6 語の 1 つで、6 語は hook/mod.rs の既存の 6 つの定数を並べた 1 つの公開の列で持ち、binary の名と cargo run -- に依らず、git hook run と host-guard の口と引用の中の字は当たらない (5) event-log-write は編集系の 4 つの道具の path と、host_guard.rs の書き込みの向け先の読み手（pub(crate) にした 1 本・redirect の > と >>・tee・sed -i・mv・cp・ln・rm）が hook の置き場の events_path に、字句で畳んだ path か実体の path の一致か実体の在る log の祖先で当たる周と、host の見張りの解けない語の最後の要素が log の file 名と同じ周に断り、読むだけの command と置き場の別 file と別の dir の同名の file は当たらない (6) rules-swap は segment の連続した 2 語が pipe と gate / land / resume で同じ segment に --rules か --rules= で始まる語が在る周で、pipe dispatch・pipe stop・pipe land-window・--rules を持たない 3 つは当たらない (7) 権能の guard が断る呼び出しはその断りだけで本行の記録を残さず（orchestrator の席の pipe land --rules は merge・pipe resume --rules は launch と行 role.orchestrator）、merge と launch を持つ行の席と pane の無い session では同じ 2 つを rules-swap で断る (8) Guard の variant 1 つが Role の直後・LiveRow の直前に在り、一覧に bypass-deny の in-loop / fail-closed の 1 行が role-guard と live-row-guard の間に載り、集計の guards と in-loop が便の base の数に 1 足した数で post-hoc と fail-open は base の数のまま (9) gate の Gated の detail の末尾（verdict と account の後）に ,rules:<出所> を足し、出所は埋め込みが embedded・--rules の path が std の absolute で絶対にした path の git -C <repo> hash-object --no-filters の stdout の 16 進の 1 行・それ以外が unreadable で、gate の入口で 1 回だけ測り、判定・rc・verdict.json・stdout の判定行は不変、gate_run は --rules の値を・land の追随の再 gate は Land の rules を Gate の新しい欄で渡し、regate=skipped の引き継ぎの記帳の字は不変 (10) queue.rs の taken_at は verdict: の後ろを最初の , の手前で切った語として PASS と比べ verdict:PASS,… を PASS と読む 歯: e2e の hook_bypass_ 6 本（guards.rs・断りの脚は rc 2・stdout 0 byte・stderr の 1 行の頭と FR112 と次の一手・記録 1 行を測る）が (a) hooks.json の hook と event の全部（母集団 6・公開の列と集合が一致）の素の撃ちと変数の binary・cargo run --・bash -c・eval・命令置換・前に代入を持つ bash -c・bash -c の中の bash -c の形の reason=hook-subcommand と、同じ歯の grep の引用の中の字・git hook run・host-guard の口の通過 (b) event log の Edit と Write と >> と tee -a と sed -i と mv・cp・ln の行き先と bash -c の中の > と変数で始まる同名の向け先と字句で畳む相対 path と symlink の dir の下の実体の path と log の祖先の dir への rm -r の reason=event-log-write と、log への >> と hook stop を ; で並べた形の reason=hook-subcommand（形の宣言順）と、同じ歯の置き場の別 file への Write と別の dir の同名の file への > の通過 (c) 読むだけの 4 形の通過と記録 0 (d) pane の無い session の 3 つの --rules X と --rules=X と bash -c の包みの reason=rules-swap と、同じ歯の --rules の無い 3 つの通過 (e) pipe dispatch --rules・pipe stop の名指し・pipe land-window の通過 (f) 偽 tmux の席の既定の行の pipe land --rules と pipe resume --rules の権能の断り（（merge）・（launch）と行 id・bypass-deny の記録 0）と、merge と launch を足した行の同じ席の reason=rules-swap を、polarity_lists_bypass_deny_ が (g) 一覧の 1 行と role-guard の直後・live-row-guard の直前を、pipe_gate_rules_source_ 2 本（pipe/gate.rs）が (h) 埋め込みの gate の verdict:PASS,rules:embedded (i) 狭い cap の INCONCLUSIVE と広い cap の撃ち直しの PASS の 2 件がそれぞれの file の blob id（歯が git hash-object --no-filters で測る）を持ち 2 つが違うことを、pipe_land_rules_source_ 1 本（pipe/land/rebase.rs）が (j) 兄弟の着地で main が動いた便の --rules つきの land の再 gate の detail が rules:<blob id> で最初の gate が rules:embedded であることと、同じ歯の面に触れない commit の引き継ぎ（regate=skipped）の detail が verdict:PASS のままであることを、lib の gate_rules_word_ 1 本（gate.rs の既存の歯の区間・記録する stub）が (k) 埋め込みの git 0 回の embedded と path の hash-object 1 回の 16 進と rc≠0 と 16 進でない stdout の unreadable を、lib の pipe_order_taken_suffixed_ 1 本（queue.rs の既存の歯の区間）が (l) turn:taken の後の verdict:PASS,rules:embedded と verdict:PASS,account:a1,rules:<16 進> と rules の後に permit の項を持つ PASS が番を残し verdict:INCONCLUSIVE,rules:embedded が番を消すことを測る。直す既存の歯は Gated の detail を字で測る 9 本（pipe_gate_lens_account_ の 5 本・pipe_gate_findings_counts_are_recorded_per_category・pipe_gate_regates_after_inconclusive・pipe_gate_lens_reread_unreadable_then_readable_passes_with_two_calls・pipe_gate_health_busy_run_stays_gated_and_can_be_regated・期待の出所は pipe.rs の helper 1 本が測る）と、極性の数の pin を持つ 5 本（便の base の数に 1 足す）と隣接の pin 2 本と外形 snapshot で、どれも直した期待が base で落ちるので retroactive の札は要らない。base は門も detail の項も無く列の読みが後ろの項を PASS と読まないので RED（機能不在）"

[[contract]]
id = "b"
title = "上限の許可の型と記録の土台 — rules 行 pipe.permit_rows / pipe.permit_max_h を lens.max_turns の直後に・RuleKind の許可の読み手の分けの網羅の match（GateTokenCap だけ true）と loader の閉じ・event の kind LimitPermitted と形 Permit（bead と閉じた key の detail）・公開の口（記帳の本体の書き手と読み手・記帳の列・着地の読み・期限の述語・効きの 2 値と純関数）（§18）"
req = ["FR17", "FR110", "FR111", "NFR4", "AC84"]
section = "18"
touches = ["crate::rules::RuleKind", "crate::fleet::EventKind", "crate::fleet::Shape"]
write-set = ["+crates/scribe2/src/pipe/permit.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/fleet/account.rs", "crates/scribe2-boundary/tests/e2e/fleet/json.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_permit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_limit_permitted_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail limit_permit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_kind_parity_every_kind_has_sample", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_kinds_follow_declaration_order", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail account_cmd_kinds_are_fifteen_with_retire_and_restore_last", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_replay_seat_retired_kind_is_a_registration_shape_and_record_refuses_it", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_case_kind_kinds_are_appended_in_order", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_case_kind_record_refuses_every_non_run_shape", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_regate_returns_gated_fail_to_implemented_on_the_same_worktree", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_follow_step_moves_gated_tree_onto_main_and_returns_to_implemented", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/permit.rs:150", "crates/scribe2/src/pipe/mod.rs:2", "crates/scribe2/src/rules/mod.rs:45", "crates/scribe2/src/rules/manifest.rs:30", "crates/scribe2/src/fleet/mod.rs:12", "crates/scribe2/src/fleet/event.rs:20", "crates/scribe2/src/fleet/replay.rs:2", "crates/scribe2/src/fleet/cli.rs:0", "crates/scribe2/src/pipe/dispatch/candidates.rs:0", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs:150"]
depends = ["a"]
done = "(1) manifest の lens.max_turns の行の直後（hook.budget_ms の前）に rules 行 pipe.permit_rows（kind PipePermitRows・List・値 [\"gate.token_cap\"]・enabled・ruling user 2026-10-01T01:05Z 項 permit-rows・ruled_at 2026-10-01）と pipe.permit_max_h（kind PipePermitMaxH・Int・値 24・enabled・ruling user 2026-10-01T01:24Z 項 permit-max-h・ruled_at 2026-10-01）がこの順で在り、2 kind は RuleKind の宣言と ALL で LensMaxTurns の直後・HookBudgetMs の前にこの順で在って字面から引け、形の違う値は断られ、2 つの裁定の字はそれぞれ 1 行だけが持つ (2) RuleKind に許可の読み手を持つかの網羅の match が 1 本在り（pub・bool・wildcard 無し）、ALL のうち true は GateTokenCap だけ (3) manifest の読みは kind PipePermitRows の行（enabled を問わない）の値の要素ごとに、manifest の行の id でない要素と分けが false の kind の行の id を、行 id・要素・行番号と分けが true の kind の列を名指して 1 件ずつ断り、分けが true の kind の行の id だけの値は通す (4) event の kind LimitPermitted が KINDS の末尾に、形 Permit が SHAPES の末尾に在り、既定の actor は machine、行は空でない bead と detail（許可 rule=<行 id> value=<整数> until=<YYYY-MM-DDTHH:MM:SSZ> ruling=<裁定 id> か取り消し rule=<行 id> revoked の閉じた key の列）が必須で、run・stage・seat・pid・口座残量・登録・列の印の key を持つ行と detail が形の外の行は from_line が key か detail を名指して malformed で断り、読めた行は書き戻すと同じ行で、Event の struct に欄を足さず schema は 1 のまま、replay は便も席も作らず、fleet record は kind LimitPermitted を record では書けないと rc 1 で断る (5) 行 b の + の file に pub の可視性で、記帳 1 件の本体 Record（Permit と Revoke の閉じた 2 値・書き手 render と読み手 parse）、同じ bead と行の記帳を記帳の順に返す records、bead の便が Landed に達した最初の行の便 id を返す landed、今が期限以後か期限を読めない周に true の expired、閉じた 2 値 Effect（Declared と Permitted〔値・裁定 id・期限〕）、純関数 permitted（引数は行 id・manifest の値・bead・event の列・UNIX 秒の今）が在り、permitted は同じ bead と行の最新の記帳が許可・bead の便が未着地・expired が false・値が manifest の値より大きいの順に照らして全部通る周だけ Permitted を返し、1 つでも外れる周は Declared（古い許可へ戻らない） (6) 行 b の + の file は RuleKind・EventKind・Shape を match の腕・構築・件数の pin で名指さず、verify の最終行の contracts check が便の木で findings 0 歯: rules_permit_ の e2e 3 本（既存の rules/embedded.rs）の (a) 2 行の id・kind・形・値・enabled・裁定の字・裁定日と list_row と int_row の値、ALL と manifest の行の前後の並び、裁定の字ごとに 1 行、形の違う値の断り (b) ALL のうち分けが true の kind の列がちょうど GateTokenCap 1 つ（母集団は ALL の数） (c) 名指される 5 行と対象の列の行だけの fixture で、値 [\"gate.token_cap\"] は読め、review.same_kind_stop・pipe.max_live・R-C4-1・runner.model・nope.row をそれぞれ gate.token_cap と並べた 5 つの値は断りがちょうど 1 件でその要素と kind の列と対象の列の行の行番号を名指し gate.token_cap を名指さず、対象の列の行を enabled = false にした fixture の nope.row も断られる、fleet_limit_permitted_ の e2e 4 本（既存の fleet/json.rs）の (d) KINDS の末尾 LimitPermitted でその 1 つ前が便の base の末尾の kind・字面の往復・actor machine・形 Permit はこの kind だけ・SHAPES の末尾 Permit・口座残量の kind でない (e) 許可と取り消しの 2 行の読みの欄と書き戻しの一致と replay の便 0 席 0 (f) bead の欠けと空・detail の欠け・run・stage・seat・pid・account・rule・mark を足した行はその key を名指して読めず、形の外の detail 7 つ（key の順違い・ruling の欠け・余りの語・数でない値・空の値・分の形の until・revoked の綴り違い）は detail を名指して読めない (g) fleet record --kind LimitPermitted は rc 1 で record では書けないと断り log を作らない、limit_permit_ の e2e 7 本（既存の pipe/gate.rs・+ の file に歯を置かない・event は on-disk の 1 行を from_line で読んで作り Event の literal を書かない）の (h) render の字が 2 形と一致し parse で同じ値に戻り、(f) と同じ形の外の 7 つが None (i) records が別の bead・別の行・同じ detail の別の kind を数えず許可・取り消し・許可を記帳の順に返す (j) 記帳が無い・最新が取り消し・別の行の許可だけは Declared で取り消しの後の許可は Permitted (k) 新しい許可が古い許可より先に切れた時刻は Declared でその前は新しい許可の値と裁定 id と期限 (l) 同じ bead の着地の行が許可の後か前に在れば Declared で別の bead の着地は効きを変えず、landed は着地の行の便 id を返し別の bead だけの列では None (m) 期限ちょうどは Declared で 1 秒前は Permitted、expired は期限ちょうどと分の形の期限で true・1 秒前で false (n) manifest の値が許可の値と等しいか大きいと Declared で 1 小さいと Permitted、直す既存の歯は rules_embedded_manifest_is_valid_and_covers_all_kinds（行数を便の base の数に 2 足す）・rules_embedded_manifest_declares_one_capability_row_per_role（kind の数を 2 足す）・rules_external_form の snapshot（rows と kinds を 2 ずつ足す）・rules_kind_parity_every_kind_has_sample（PipePermitRows は値 [\"gate.token_cap\"] と GateTokenCap の行 1 本を足した fixture）・fleet_kinds_follow_declaration_order（数を 1 足し列の末尾 LimitPermitted）・account_cmd_kinds_are_fifteen_with_retire_and_restore_last（数を 1 足す）・fleet_replay_seat_retired_kind_is_a_registration_shape_and_record_refuses_it（数を 1 足し末尾 LimitPermitted）・fleet_case_kind_kinds_are_appended_in_order（数を 1 足し base の末尾の kind は 1 つ前へ）・pipe_regate_returns_gated_fail_to_implemented_on_the_same_worktree と pipe_follow_step_moves_gated_tree_onto_main_and_returns_to_implemented（段と kind の数の組の kind を 1 足す）で、どれも直した期待が base で落ちるので retroactive の札は要らず、書き換えない fleet_case_kind_record_refuses_every_non_run_shape が LimitPermitted を含めて緑・base は kind も行も型も口も無く compile が落ちるので RED（機能不在）"

[[contract]]
id = "c"
title = "許可の口 pipe permit — 記帳と取り消しの 2 形・照らす前の 4 つの読み（manifest・event log・台帳・orchestrator の席の打刻の file の最後の行）と rc 2・照合 10 語の rc 1・LimitPermitted の 1 件と 1 行、台帳の Bead の本文の欄、打刻の最後の行の読み手 1 本、起票と発話の前後の判定、権能の表の行 pipe permit = approve、使い方と help と snapshot（§19）"
req = ["FR110", "FR41", "FR17", "NFR4", "AC84"]
section = "19"
touches = ["crate::pipe::cli::PipeCommand", "crate::ledger::Bead"]
write-set = ["+crates/scribe2/src/pipe/cli/permit.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/seat/state.rs", "crates/scribe2/src/seat/ruling.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "=crates/scribe2-boundary/tests/e2e/main.rs", "=crates/scribe2/src/hook/role_guard_tests.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_permit_mouth_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_role_permit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail doctor_asked_after_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_bind_", "cargo nextest run -p scribe2 --lib --no-tests=fail help_table_", "cargo nextest run -p scribe2 --lib --no-tests=fail doctor_asked_after_", "cargo nextest run -p scribe2 --lib --no-tests=fail stamp_sid_reader_returns_only_a_uuid_shaped_last_sid", "cargo nextest run -p scribe2 --lib --no-tests=fail role_guard_capability_commands_match_the_three_word_sequence", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/pipe/cli/permit.rs:330", "crates/scribe2/src/pipe/cli.rs:14", "crates/scribe2/src/pipe/cli/args.rs:6", "crates/scribe2/src/seat/state.rs:35", "crates/scribe2/src/seat/ruling.rs:8", "crates/scribe2/src/ledger/mod.rs:3", "crates/scribe2/src/hook/role_guard.rs:2", "crates/scribe2/src/help.rs:2", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs:680", "crates/scribe2-boundary/tests/e2e/hook.rs:45", "crates/scribe2-boundary/tests/e2e/pipe.rs:2"]
depends = ["b"]
done = "(1) PipeCommand の末尾に permit を足し（件数は便の base の列に 1 語足した数）、受ける flag は置き場・repo・規則と --bead・--rule・--value・--until・--ruling・--bd と値なしの --revoke で、記帳と取り消しの 2 形を持ち、形の誤り（flag の欠け・--revoke と --value / --until / --ruling の併せ持ち）は今の rc 1 の形で何も読まず、使い方に permit の 1 行（1 行目の語の群と help の FORM は不変）、help の pipe の頁に SUBCOMMANDS の permit の行と SEE の limit-permit.md、列の 1 周は撃たず（GATES と TERMINALS は不変）、入口の manifest の読みが断る周は permit だけ rc 2 の pipe: permit unreadable source=manifest の 1 行 (2) 記帳の周は manifest → event log の全件（形の外の detail の許可の行は行 b の読みが断る）→ 渡した裁定 id の裁定 event が在る周だけその問いの show → --repo を anchor に持つ orchestrator の登録 row が在る周だけその席の打刻の file の最後の行の順に照らす前に読み、最初に読めない置き場を source=<manifest|event-log|ledger|stamp> で名指して rc 2・stdout 0 byte・stderr 1 行で止まって何も書かず（照合の順に関わらない）、取り消しの周は manifest と event log だけを読む (3) 記帳の周は閉じた enum の宣言順に 1 関数で rule-not-listed・not-raise（10 進の数字だけで manifest の値より大）・no-ruling（問い id の形で batch: と policy: を除き裁定 event が在る）・no-utterance（同じ ts の発話 event が在り actor human）・not-surface（経路 chat で session が打刻の最後の行の会話 id と同じ・打刻の名指しは no-seat / absent / blank / unshaped / 会話 id）・before-question（起票が発話より後でない・発話は秒へ切り捨て同じ秒は後でない）・bad-until（分の形で今より後で発話の秒 + pipe.permit_max_h 時間以下）・reused（在る許可の同じ裁定 id か同じ発話）・not-stated（問いの本文の ASCII の語の列に bead・行 id・値が字のまま・本文の無い問いは 3 つとも欠け）・landed（行 b の着地の読みが便 id を返さない・名指しはその便 id）を照らし、最初に外れた語で rc 1・stdout 0 byte・stderr 1 行 pipe: permit refused reason=<語> <語ごとの名指し> を返して何も書かず、取り消しの周は rule-not-listed だけを照らす (4) 通る記帳は emit で kind LimitPermitted・bead・行 b の書き手の detail（until は渡された分の字に秒 :00 を足した秒の形）の 1 件（actor machine・run 無し）と stdout の permit: bead= rule= value= declared= until= ruling= の 1 行、取り消しは裁定 id 無しで行 b の取り消しの形の 1 件（効いている許可が無い周も）と permit: bead= rule= revoked の 1 行 (5) 台帳の Bead に本文の欄 description（show の JSON の description・無ければ空）を足し構築は bead_of の 1 か所のまま (6) state.rs に打刻の file の最後の行（lines の最後の要素・前の行へ戻らない）を閉じた 6 値で返す読み手 1 本を Stamp の from_line と会話 id の形の判定を使って置き、last_sid と resume_carry は不変 (7) doctor の AskedAfter の数えの字と数は不変 (8) CAPABILITY_COMMANDS に pipe permit を approve で 1 行足し、ほかの行と行 role.orchestrator は不変 (9) 本行の + の file は EventKind・Stage・RuleKind・Guard・Refuse・Capability・PipeCommand の変種を match の arm と変種の構築の形で名指さず、Polarity の const を宣言しない 歯: 既存の e2e の seat/ruling.rs の pipe_permit_mouth_ の w1〜w10（語ごとに 1 本・通る fixture から条件 1 つだけを外し、2 つ以上の部分を持つ条件は部分ごとの fixture・w3 は batch: と policy: の形を含み、w6 は同じ秒の通る脚を、w7 は形の外と窓ちょうどの通る脚を、w9 は description の無い問いを、w10 は名指しの便 id を持つ）・o1（依存の境目 3 組が先の語）・s1（打刻の 4 形と登録 row の無い周が not-surface）・s2（壊れた最後の行が rc 2 で前の行へ戻らない）・p1（event 1 件と 1 行の完全一致・列の 1 周の行が無い）・r1（取り消し 4 形）・u1（読めない記帳 4 形・読めない取り消し 2 形・順に関わらない rc 2・2 つを同時に壊した 3 組の先の source・壊れた bd と無い裁定 id の no-ruling）・m1（読み手の無い 4 行を列に持つ manifest が rc 2）・t1（本文の語の境目）・h1（使い方と help と形の誤り）、既存の e2e の hook.rs の hook_role_permit_ の 1 本（orchestrator の席の 2 形が approve で通り、登録の無い席の 2 形が deny、approve を抜いた行で deny）、直す既存の歯は pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none の件数（便の base の数に 1 足す）と字の列と pipe_external_form の snapshot で、どちらも直した期待が base で落ちるので retroactive の札は要らず、help_table_・cli_help_・doctor_asked_after_・seat_ruling_bind_・stamp_sid_reader_returns_only_a_uuid_shaped_last_sid・role_guard_capability_commands_match_the_three_word_sequence は緑のまま、verify の最後の行の contracts check が便の木で findings 0・base は pipe permit が無く表に行も無いので RED（機能不在）"

[[contract]]
id = "d"
title = "上限の許可の読み手 — gate の周の入口で event log を 1 回読み行 b の permitted で効く cap（宣言値の Limits と別の型）を導き、予算の照合と gate が起こす lens に渡す・許可の周の INCONCLUSIVE の文と Gated の detail の ,permit: と verdict.json の key permit・lens を起こす直前の run dir の cap.txt の 1 行と lens の読み・event log を読めない周の rc 2（§20）"
req = ["FR111", "FR9", "NFR1", "NFR4", "AC85"]
section = "20"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs", "crates/scribe2-boundary/tests/e2e/headless/lens.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_permit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_follow_gate_permit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_permit_cap_", "cargo nextest run -p scribe2 --lib --no-tests=fail gate_cap_read_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_inconclusive_when_diff_exceeds_cap", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_rulings_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_reads_cap_from_rules_row", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_rulings_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/gate.rs:130", "crates/scribe2/src/headless/lens.rs:35", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs:600", "crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs:45", "crates/scribe2-boundary/tests/e2e/headless/lens.rs:130"]
depends = ["b"]
done = "(1) gate.rs の gate が precheck の後・verify の前に event log を 1 回読み、行 b の permitted を行 gate.token_cap・manifest の値（Gate の limits の token_cap）・Gate の bead・読んだ event の列・now_secs の今で 1 回呼び、返りを wildcard の無い match で gate.rs の私有の型の効く cap（値と許可の周だけ行と値と裁定 id）に写し、追随の再 gate も同じ 1 か所を通り、gate は pipe.permit_rows を読まない (2) measure の畳みの閾値と decide の照合は同じ効く cap の値と比べ（効く cap は measure の前に 1 回導いて 2 つの私有の関数へ引数で渡し、許可の周は manifest の値で畳まず通知に pruned= を足さない）、超えた周の INCONCLUSIVE の文は許可の周だけ cap の値の直後に（上限の許可 gate.token_cap=<値> ruling=<裁定 id>）を持ち、許可の無い周の文は 1 字も変えず、上限なしの枝を持たない (3) 効く cap が許可の周は判定（PASS・FAIL・INCONCLUSIVE）に依らず Gated の detail の末尾（行 a の出所の後ろ）に ,permit:gate.token_cap=<値> ruling=<裁定 id> を、verdict.json の ts の前に key permit（同じ字）を書き、許可の無い周は detail も verdict.json の key の列も変えない (4) 期限・着地・取り消し・新しい記帳・manifest 以下の読み分けは permitted の返りだけで決め、新しい記帳が先に切れても古い許可へ戻らない (5) lens を起こす周だけ keep_rulings の直後に run dir の直下の cap.txt を gate.token_cap=<manifest の値> source=manifest か gate.token_cap=<許可の値> source=permit ruling=<裁定 id> の 1 行で書き直し、書けない周は lens を起こさず rc 2 で verdict.json も Gated も書かず、lens を起こさない周は書かず、lens の起動の行と flag は変えない (6) lens.rs の dispatch は memo の枝の後で manifest の cap を今のとおり先に読み、契約の隣に cap.txt が在ればその値を cap にし（小さくても大きくても写しが勝つ）、無ければ manifest、在るのに読めないか 2 形の外の周は claude を呼ばず rc 2 で lens: cap の写しを読めない: <path>: <理由> の 1 行 (7) 写しは gate の契約の写しの隣だけに置かれ、契約の審査・先撃ち・行の審査の lens は manifest を読み、memo の審査は写しを読まない (8) 1 の読みが読めない周は許可にも manifest にも倒さず rc 2 で pipe: event log を読めない（上限の許可を照らせない）: <最初の理由> の 1 行で止まり、verify を撃たず verdict.json も Gated も cap.txt も書かない 歯: pipe_gate_permit_ の e2e 16 本（既存の tests/e2e/pipe/gate.rs・2 bead と偽 lens と cap を越える diff・許可の event は行 b の形の 1 行を event log の file へ足す helper で置き・期限は秒の形・許可の無い脚を持つ歯は同じ歯の中に許可で通る脚を持つ）の (a) cap 1 の manifest と値 1000000 の許可で PASS し、同じ歯の pipe.permit_rows を不発効にした写しでも PASS (b) gate と器の lens の両方を cap 1 の manifest の写しにして許可で PASS し偽 claude が 1 回起きる (c) 許可を持たない bead が INCONCLUSIVE で manifest の文・permit 無し・lens も写しも無く同じ歯の許可の bead は PASS (d) 値 2 の許可で文が cap 2（上限の許可 gate.token_cap=2 ruling=<id>）を持ち verdict.json と detail も持ち lens は起きない (e) PASS の detail の末尾と verdict.json の permit と cap.txt の source=permit の 3 つと、同じ歯の偽 lens が FAIL を返す周の detail と verdict.json の permit と、同じ歯の許可の無い周の行 a の出所の項で終わり permit を持たない detail と permit の無い verdict.json と source=manifest の cap.txt (f) 期限の後 (g) 同じ bead の別の便の着地の後 (h) 取り消しの後がそれぞれ manifest の文で、(f)(h) は同じ歯で新しい許可を足した撃ち直しが PASS・(g) は同じ歯の別の bead の許可の便が PASS (i) 新しい記帳の値 2 と新しい裁定 id で文が出る (j) 期限の過ぎた新しい記帳の後に古い許可へ戻らず manifest の文で同じ歯の 3 つ目の許可で PASS (k) cap 10 の manifest と値 5 の許可で cap 10 の文で permit 無し・同じ歯の値 1000000 の許可で PASS (l) 許可で PASS した便の review の契約の写しに cap 1 の manifest で器の lens を撃つと contract material exceeds cap で claude が起きない (m) 許可で PASS した後の置き場の cap.txt は run dir の直下の 1 本だけで review の下と run dir の外に無い (n) event log の末尾の壊れた行で rc 2・verdict.json も cap.txt も無く lens も起きず Gated の行が増えず、同じ歯で行を除くと PASS (t) cap.txt の場所の dir で rc 2・lens が起きず verdict.json が無く、同じ歯で dir を除くと PASS (u) 16 行以上の削除だけの run を持ち畳みの後の本文が manifest の cap を超える diff で、値 1000000 の許可の便が PASS し run dir の verify.stderr.log の通知の行が pruned= を持たず、同じ歯の許可の無い bead の同じ形の便は pruned= を持つ、pipe_follow_gate_permit_ の e2e 1 本（既存の tests/e2e/pipe/land/follow.rs）の (o) main が動いた便の追随の再 gate が cap 1 の manifest の land で許可で PASS し permit の detail の Gated が 1 件増えて着地する、headless_lens_permit_cap_ の e2e 3 本（既存の tests/e2e/headless/lens.rs）の (p) 隣の source=permit 値 100 の写しで cap 10 の manifest でも claude を呼び、source=manifest 値 5 の写しと写しの無い周は diff exceeds cap (q) dir・整数でない値・裁定 id の無い source=permit の 3 形が rc 2 で path を名指し claude を呼ばない (r) memo の材料の隣に写しが在っても prompt の材料は manifest の 10 byte で切れ、同じ歯の diff の形は写しの値で claude を呼ぶ、gate_cap_read_ の lib 1 本（gate.rs の既存の歯の区間）の (s) 読めない結果は event log を名指す Err で空の列は manifest の値、直す既存の歯は無く retroactive の札は要らない・base は許可を読まず写しを書かず lens は写しを読まず lib の 1 本は無い関数を呼ぶので RED（機能不在）"

[[contract]]
id = "e"
title = "見え方 — pipe show --run が便の bead の上限の許可を記帳の順に 1 行ずつ状態の語 6 つ（先に当たる順 landed・revoked・superseded・expired・overtaken・active）で出し、pipe dispatch ls が効いている許可を bead の字の順に 1 行ずつ出す（行の値か event log を読めない周は unmeasured・§21）"
req = ["FR111", "AC85", "NFR4"]
section = "21"
write-set = ["crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/src/pipe/dispatch.rs", "+crates/scribe2/src/pipe/dispatch/permits.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_show_permit_word_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_ls_permit_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/cli/show.rs:40", "crates/scribe2/src/pipe/dispatch.rs:5", "crates/scribe2/src/pipe/dispatch/permits.rs:140", "crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs:440"]
depends = ["c", "d"]
done = "(1) 行 e の + の file に状態の語の閉じた enum（6 値・宣言順は landed・revoked・superseded・expired・overtaken・active で先に当たる順・語の字は FR111 の 6 語）と純関数 1 本（入力は permitted と同じ行 id・manifest の値・bead・event の列・今の時刻で、records の返す記帳の列の許可ごとに記帳の順で許可と語を返す）と、許可の記帳を持つ bead と行 id の組を bead と行 id の字の順に重ねずに返す純関数 1 本が在り、語は宣言順に照らして最初に当たる語（landed は bead の便が着地・revoked と superseded は同じ bead と行の次の記帳が取り消しか新しい許可・expired は期限の後・overtaken は値が manifest の値以下・active はどれにも当たらない）で、着地と期限は行 b の landed と expired を呼び、active の許可は高々 1 つで permitted の返す許可と値・裁定 id・期限が同じ (2) pipe show --run は今の行の後ろに便の bead の許可を行 id の字の順と記帳の順に permit: rule=<行 id> value=<値> declared=<manifest の値> until=<記帳の until の字> ruling=<裁定 id> state=<語> の 1 行ずつ出し、manifest は ceiling_of と同じ読み（--rules か埋め込み）、行の値を読めない行 id は permit: unmeasured rule=<行 id> records=<許可の数> の 1 行で語を出さず、取り消しは単独の行にせず、許可の記帳を持たない bead の周は出力を変えない (3) pipe dispatch ls は memo の行の後ろに permitted が許可を返す組を bead と行 id の字の順に [DISPATCH-PERMIT] bead=<bead> rule=<行 id> value=<値> declared=<manifest の値> until=<記帳の until の字> ruling=<裁定 id> の 1 行ずつ出し、event log を読めない周は [DISPATCH-PERMIT-UNMEASURED reason=events]、行の値を読めない行 id は [DISPATCH-PERMIT-UNMEASURED reason=rules rule=<行 id>] を出し、効いている許可 0 件の周と台帳を読めない周は行を足さない (4) pipe show の 1 行目と検出線・消費・cost-ceiling の行と、dispatch ls の [DISPATCH]・[DISPATCH-COUNT]・[DISPATCH-NONE]・[DISPATCH-UNMEASURED]・事前審査・束・memo の行の字と並びは変わらず、許可の記帳を持たない置き場の出力は 1 byte も変わらない (5) verify の最終行の contracts check が便の木で findings 0 歯: 既存の e2e の file（pipe/dispatch/terminal.rs）に置き、fixture は event log に便の RunCreated と段 Landed の RunStage と許可と取り消しの記帳を行 b の on-disk の形で 1 行ずつ足し（pipe permit を撃たない）、期限は 2099 年か 2020 年の UTC の秒の形、pipe show の manifest の値 D は埋め込みの gate.token_cap の値を読み、行は 1 行の完全一致で照らす。pipe_show_permit_word_ の 12 本は (a) active の 1 行の字と最後の行の位置・1 行目の不変・許可を持たない bead の便に行が無くほかの bead の許可も出ない (b) 着地の前は active で後は landed (c) 取り消しの前は active で後は revoked で permit: の行はどちらもちょうど 1 行 (d) 2 つ目の許可の前は active で後は superseded と active の 2 行 (e) 期限が過去は expired で未来は active (f) 値 D は overtaken・D+1 は active・--rules の写しで値を D+10 に上げると D+1 も overtaken (g) 取り消しの後は revoked でさらに着地の後は landed (h) 新しい許可の後は superseded で着地の後は 2 行とも landed (i) 期限が過去は expired で取り消しの後は revoked (j) 期限が過去の 1 つ目は expired で 2 つ目の許可の後は superseded (k) 値 D で期限が未来は overtaken・過去は expired (l) manifest に無い行 id の許可は permit: unmeasured rule=<行 id> records=1 で state= を含む行が無く、同じ bead の gate.token_cap の許可は語の行を持つ、pipe_dispatch_ls_permit_ の 3 本は (m) 効いている 2 つ・取り消し・期限切れ・着地・manifest の値以下・新しい許可に替わった 1 つの bead で [DISPATCH-PERMIT] の行は効いている許可だけが bead の字の順（s2-p.10 が s2-p.9 の前）に出て 1 行目は [DISPATCH-NONE] のまま、同じ写しの各便の pipe show で 6 語が全部出て state=active の行の bead・値・裁定 id の集合が [DISPATCH-PERMIT] の行の集合と等しい (n) event log の位置に dir を置いた周は [DISPATCH-PERMIT-UNMEASURED reason=events] が最後の行で、同じ歯の読める event log の周は [DISPATCH-PERMIT で始まる行が無く、同じ歯の偽の bd が rc 1 の周は許可の記帳が在っても [DISPATCH-UNMEASURED reason=…] の 1 行だけ (o) 写しに無い行 id の許可は [DISPATCH-PERMIT-UNMEASURED reason=rules rule=<行 id>] を出し同じ置き場の gate.token_cap の効いている許可は [DISPATCH-PERMIT] の行で出る。直す既存の歯は無く retroactive の札は要らない（出力を pin する既存の歯は許可の記帳を持たないので字が変わらない）。base は許可の行を出さないので 15 本とも RED（機能不在）"
done-teeth = ["1:@1", "2:@1", "3:@2", "4:@1", "4:@2", "5:@3"]
<!-- contracts:end -->
