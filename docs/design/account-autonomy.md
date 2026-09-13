# 設計: 口座の自律制御 — 上限で止まった便は段として残し別口座で続け、口座は 1 つの純関数が選び、席は上限の手前で退避して別口座で立て直す

- 要件: [FR35](../../design-intent/spec/srs.html#FR35) 上限停止の検知 / [FR36](../../design-intent/spec/srs.html#FR36) 口座選定 / [FR37](../../design-intent/spec/srs.html#FR37) 別口座での途中再開 / [FR38](../../design-intent/spec/srs.html#FR38) 開発 session の上限前の退避 / [AC12](../../design-intent/spec/srs.html#AC12) [AC13](../../design-intent/spec/srs.html#AC13)・既存 [FR33](../../design-intent/spec/srs.html#FR33) / [FR14](../../design-intent/spec/srs.html#FR14) / [FR22](../../design-intent/spec/srs.html#FR22) / [FR28](../../design-intent/spec/srs.html#FR28) / [FR29](../../design-intent/spec/srs.html#FR29) / [FR32](../../design-intent/spec/srs.html#FR32)
- 決定: [ADR-0020](../../design-intent/decisions/ADR-0020-account-autonomy-limit-stop-resume-and-selection.html)（本 doc の決定の正本・§2.1〜§2.6）/ [ADR-0012](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html)（上限 record）/ [ADR-0017](../../design-intent/decisions/ADR-0017-account-allowance-measured-into-event-log.html)（実測行・label・credential の場所）/ [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html) §2.1（登録 row）
- 土台: [fleet-usage.md](./fleet-usage.md)（計測）・[pipeline.md](./pipeline.md) §4（段）/ §5.7（resume）・[pipeline-question.md](./pipeline-question.md)（回答節）・[pipeline-conflict.md](./pipeline-conflict.md) §3（起こし直しの形）・[seat-autonomy.md](./seat-autonomy.md)（tick）・[seat-roles.md](./seat-roles.md) §2（登録 row の口座と起動の雛形）・[rules-manifest.md](./rules-manifest.md)
- 位置づけ: 口座は「上限を避ける」対象ではなく「使い切る弾」。便は当たるまで走り、当たったら別口座で同じ worktree から続く。席は当たる前に退避し、別口座で同じ target に立て直す。

## 1. 何を解くか

上限に当たった便は Failed（現物の detail は `runner-rc:<上限の rc>,commits:<n>`・[pipeline.md](./pipeline.md) §5.2・ADR-0020 §1 の `rate-limit` は誤記）で終端し、worktree と回答が捨てられて新しい便でしか続けられない。口座は席の起動時に人が選び、便は席の口座を継承するので席の口座が食い潰される。席はモデル別 7 日窓だけに当たって止まった（記録時点の事故型）。器は口座残量（FR33・実測行）を持つが選定に使っていない。

## 2. 上限停止の段（ADR-0020 §2.1・FR35）

- **`Stage::RateLimited`**（variant 1 つ・宣言順は `Questioned` の直後・`as_str` は `RateLimited`）。runner が上限の rc（`headless::RC_RATE_LIMIT`・値は現物・ADR-0012 §2.1）で終わった周は `RunStage stage=RateLimited detail=rc:<rc>,status:<観測した上限 status>` を記帳する（status は runner の停止行と同じ出所・ADR-0012 §2.1 末尾。停止行の形と読み手の純関数は同じ module `headless/runner.rs` に並べる〔質問 record の形と読み手が並ぶのと同型〕・pipe は rc が上限の周だけ読む）。口座の label は pipe が (c) の `--account-dir` の配線で初めて知るので、(c) が detail に `account:<label>` を足す。窓の種別は上限 record が運ばない（記録時点）ので段には書かず、選定は実測行（FR33）から読む。上限の判定入力は上限 record の status のみ（ADR-0012・本 doc は判定を足さない）。
- Failed に倒さない。worktree・base・commit・質問と回答の event を保つ（N1・C9）。終端ではない＝live な便として intake の排他の母集団に残る（ADR-0019 §2.1）。schema 1 のまま。lens が上限で止まった周は FR9 の既存極性（INCONCLUSIVE）のまま。
- `pipe show` はこの段を名で出す。`pipe stop --run` は本段の便も止められる（終端手段は stop だけ・§4）。

## 3. 選定の純関数（ADR-0020 §2.2・FR36）

- **入力**（2 種 + 用途）: manifest の `[[account]]` 行の label 列（宣言値）・口座 × 窓ごとの**最新の実測行**（`AllowanceMeasured` / `AllowanceUnmeasured`・実測値・reset 時刻付き）・用途（`Purpose::{Run, Session}`・closed enum）・使う model（`Option`・display name・与えられた周はその model のモデル別 7 日窓だけを数え、与えられない周はモデル別窓すべての最大を数える〔保守側〕。記録時点は両用途とも与えられない: runner の起動形は model を渡さず〔claude の既定〕、登録 row に model の項目は無い〔§11〕）・除外する label の集合（席の登録 row が持つ口座・[seat-roles.md](./seat-roles.md) §2）・rules 行の値（session 用の閾値）。**席の判断・pane の字面・前の版の道具の出力は入力にしない**。
- **出力**: `Selection::{Chosen(label), None(NoCandidate)}`。`NoCandidate` は typed な理由（全口座が当たっている / 測れない / 除外で空）と**最も早い reset 時刻**（§4 の待ちに渡す）を運ぶ。
- **逼迫度** = その口座の窓のうち最大の使用率（5 時間・7 日・model が与えられた周はその model のモデル別 7 日窓だけ、与えられない周はモデル別窓すべての最大）。**当たっている** = 最新の実測行の使用率が 100（窓の全量・規則値ではない）に達している。
- **便用**: 当たっていない口座のうち逼迫度が最大の口座（使い切る側・C9.2）。席の登録 row が持つ口座は候補から外す（席の生死を問わず登録が在る限り）。
- **session 用**: 逼迫度が最小で、かつ **R-C9-1 の値**（session 用の閾値・使用率の百分率）未満の口座（余裕を残す側）。
- **候補から外す**: 実測行が無い・`AllowanceUnmeasured`・reset 時刻を過ぎた古い行しか無い口座（測れない口座を選ばない・C10）。選定の直前に器は FR33 の計測を 1 回撃つ（実測行の鮮度を選定側が保証する・既存の `fleet usage` の 1 コマンド）。同点は label の辞書順。
- **R-C9-1**: kind は既存の `AccountSelection` のまま、値を散文（`Policy`・enabled = false）から **Int**（session 用の閾値・百分率）に改めて enabled = true。値と裁定 id は契約 (b) が manifest に書く（**user 裁定**・§9）。便用の規則は閾値を持たない。
- 純関数は `fleet/select.rs`（新 module・in-file の歯・入力は値だけ・I/O なし）。読み手は §4 の再開・§5 の tick・`fleet select --state-dir S --purpose run|session [--model M] [--exclude L…]`（1 行で結果と理由を出す subcommand・席と歯の口）。

## 4. 別口座での途中再開（ADR-0020 §2.3・FR37）

- `pipe run` / `pipe resume` が `RateLimited` の段を見た周: 計測 → §3 の便用の規則で口座を選ぶ → 同じ worktree・同じ契約・同じ base で runner を起こし直す（`--account-dir <state_dir>/accounts/<label>`・ADR-0017 §2.3・FR5 の口のまま）。起こし直しの経路は [pipeline-conflict.md](./pipeline-conflict.md) §3 の `spawn_turn` と同じ 1 本（C6 の 1 つの spawn 口）。
- **prompt**: 契約（再読）+「回答」節（FR32・Questioned の再開と同じ）+「**途中再開**」節（base からの commit の一覧・止まった時刻・`headless/runner.txt` に節の雛形を足す・順序は 契約 → 回答 → 途中再開）。
- **候補なしの周**: `Completion::AccountFree { reset_at, state_dir }`（variant 1 つ・運ぶ値は pid でなく最も早い reset 時刻と実測行の置き場〔`is_met` が最新の実測行を読んで §3 を再評価する・`SlotFree` が `slots_dir` を運ぶのと同型〕＝現物の完了 enum の `pid()` は 0 を返す形）を足して唯一の wait 実装を通す。deadline は reset 時刻から計算した値（rules 行ではない・縮退を持たない）。Timeout（C11.3 の Result）を受けた周は計測して選び直し、候補なしなら次の reset 時刻で待ち直す。起こし直しの回数に上限を持たない（窓を跨いで続く）。待ちの間も便は live。終端手段は `pipe stop --run` だけ（器は自動では終端しない）。
- **記帳**: 再開のたびに `RunStage stage=Spawned detail=account:<label>,resume:rate-limit`（既存の段の event・detail で弁別）。人由来の event は 0（FR22）。

## 5. 席の退避と立て直し（ADR-0020 §2.4・FR38）

- **tick の判定に軸を 1 つ足す**（[seat-autonomy.md](./seat-autonomy.md) §3 の `judge`・inject / noop の判定であって guard ではない・極性一覧に載せない）: 登録済みの席ごとに、(1) 実測行の鮮度が既存の rules 行 `seat.tick_stale_s` の値より古い周は FR33 の計測を先に撃つ（定期計測はこの 1 形に限る）→ (2) 席の登録 row が持つ口座の逼迫度（§3 の定義・席が使う model の窓を含む）を最新の実測行から読む → (3) **R-C9-1 の値以上**である間は、FR29 と同じ除外（退避物が在る周・cycle が走っている周は注入しない）の下で idle を待たずに退避の合図を注入する。実測行が無い・測れない周は注入せず `NoopReason` に理由 1 つ（縮退・止めない）。FR27 の打刻の合図はこの周は出ない（FR27 の条件に「使用率が閾値未満」が在る）。
- **立て直し**: 席が退避して止まった周（打刻が Stop の後に session の終了を示す・[seat-state.md](./seat-state.md)）、tick は §3 の session 用の規則で口座を選び、登録 row が持つ**起動の雛形**（穴 `{account_dir}` 1 つ）の穴を選んだ label の credential dir で埋めて同じ target の shell へ注入し、続けて復元の command（FR28・`seat rebrief`）を注入する。登録 row の口座 label を選んだ label に更新する（同じ鍵で再登録 = `SeatRegistered` 1 件・[seat-roles.md](./seat-roles.md) §2）。この更新は `seat register` の subcommand を経由せず、器の内部の同じ 1 関数が event を積み、`target` / `sid` / `launch` は既存 row から写す（打刻の条件は席の session が撃つ登録に課すもので、器自身の更新には課さない・`sid` は登録時の証拠であって現在の session の識別子ではない）。候補なしの周は立て直さず typed な理由を記帳し、次の tick で選び直す（0 口座で起こさない・止めない）。立て直しは既存の登録 row を持つ席に限る。
- 起動の雛形は env を読まず、器は穴を埋めて注入するだけ（host 名や絶対 path を解釈しない）。

## 6. 極性

本設計は guard を足さない（極性一覧は不変）。境界の型: `NoCandidate`（選定・FailOpen: 待つか記帳するだけで止めない）・`RateLimited` の段（終端でない）・計測の失敗は [fleet-usage.md](./fleet-usage.md) §6 のまま（`UnmeasuredReason` FailOpen）。

## 7. 歯（`crates/<NAME>/tests/e2e/` に `pipe_ratelimit_` / `fleet_select_` / `seat_account_` 接頭辞・名前の列は現物が SSOT）

- 段: 偽 runner が上限 record を最終行に書いて上限の rc で終わる fixture で `RateLimited` の段・detail に rc と観測した status・Failed でない・worktree と commit が残る・`pipe show` が段名を出す・intake の排他が本段の便を live に数える・`pipe stop --run` で止められる。
- 選定（in-file・純関数）: 便用は逼迫度最大の当たっていない口座・席の口座は除外・session 用は逼迫度最小かつ閾値未満・当たっている口座（100）は選ばない・Unmeasured / 古い行の口座は候補外・同点は辞書順・全部当たっているときは `NoCandidate` に最も早い reset・model が与えられた周はその model の窓だけ数え、与えられない周はモデル別窓すべての最大を数える。
- 再開: 偽 runner を 2 回起こす fixture（1 回目は上限の rc・2 回目で commit）で別 label の `--account-dir` が渡り（stub が argv を写す）・段の detail に `account:<label>` が足される・stdin に「途中再開」節と順序（契約 → 回答 → 途中再開）・`Spawned detail=account:…,resume:rate-limit`・Landed まで通る（AC12）／全口座が当たっている fixture で `AccountFree` の待ち → 実測行を新しくすると待ちが解ける（deadline を fixture で短く）／`pipe stop --run` で待ちの便を終端できる。
- rules: R-C9-1 の値の型（Int）と enabled = true・欠落は `RuleError`・rules 外形 snapshot。完了 enum の網羅 match（compile）。
- tick（seat.rs・偽 tmux）: 2 席の fixture のうち 1 席の口座の使用率が閾値以上 → 退避の合図 1 件（FR29 と同じ除外が効く）・閾値未満 → 打刻の合図・実測行なし → noop に理由／Stop の後の周に起動の雛形の穴が別 label の dir で埋まった command と復元の command が順に注入され、登録 row の口座が更新される（AC13）／候補なし → 立て直さず記帳。
- 実地（done の一部・歯にしない）: live 口座 2 つ以上の host で、席 1 つの口座の使用率が閾値以上のとき退避 → 別口座で立て直し → 復元が 1 回通ること（AC13 の D）と、実便で上限に当たった記録が 1 件得られたら別口座で Landed になった事実を記録（AC12 の後半）。

## 8. 憲法・制約との整合

C1 / C5（R-C9-1 は行・裁定 id）・C2（`Purpose` / `Selection` / `Stage` は closed enum・選定は 1 関数）・C2.2（`--account-dir` の引数・env を読まない）・C3 / C3.4（段は event log・待ちは完了 enum の 1 variant と 1 実装）・C6（起こし直しは 1 つの spawn 口）・C9 / C9.2（人手なしの再開・使い切る）・C10（実測行を通してだけ選ぶ）・C11.2 / C11.3（`NoCandidate` の極性・Timeout は Result）・N1（worktree を捨てない）。

## 9. 契約（4 便・この順・実装は pipeline）

- **(a) 上限停止の段**（S）: `Stage::RateLimited`・runner の上限の rc → 段の記帳（rc / status）・`pipe show`・排他の母集団・stop。write-set = fleet/mod.rs（`Stage` の variant・`STAGES`・`as_str`）・headless/runner.rs（停止行の読み手・in-file の歯）・pipe/mod.rs・pipe/spawn.rs・pipe/cli.rs（段名の表示・live の集合・stop）・tests/e2e/{pipe,fleet}.rs・snapshot。依存: なし（上限の rc は現物・label は (c) が足す）。
- **(b) 選定の純関数と R-C9-1**（M）: `fleet/select.rs`・`Purpose` / `Selection` / `NoCandidate`・R-C9-1 の値の型変更（**値と裁定 id は user 裁定**）・`fleet select` subcommand・選定前の計測の呼出し。write-set = fleet/select.rs（新規）・fleet/mod.rs・fleet/cli.rs・rules/mod.rs（値の型）・rules/manifest.toml・tests/e2e/{fleet,rules}.rs・snapshot。依存: s2-07l.187（実測行の値の形）の land 後。
- **(c) 別口座での途中再開**（M）: `Completion::AccountFree`・`pipe run` / `resume` の RateLimited の経路・「途中再開」節・`--account-dir` の配線・記帳。write-set = pipe/follow.rs・pipe/cli.rs・pipe/spawn.rs（段の detail に `account:<label>`）・fleet/mod.rs・headless/runner.txt・tests/e2e/pipe.rs。依存: (a)(b)・s2-07l.147（land の順序制御・pipe/ と fleet/mod.rs を触るので直列）・[seat-roles.md](./seat-roles.md) 契約 (a)（便用の除外集合 = 登録 row の口座の読み手）。
- **(d) 席の退避と立て直し**（M）: tick の軸・鮮度で計測・登録 row の口座と雛形・Stop 後の立て直し・登録 row の更新。write-set = seat/tick.rs・seat/cycle.rs・seat/role.rs（登録 row の読み手）・tests/e2e/seat.rs。依存: (b)・[seat-roles.md](./seat-roles.md) 契約 (a)（登録 row）。

## 10. 却下案（ADR-0020 §5 の写しは持たない・設計固有のもの）

- 上限停止の段を `Questioned` の detail で表す。却下: 質問の再開と再開の契機が違う（回答が要らない）・C2「新しい理由は新 variant 1 つ」。
- 選定を tick の中に書く。却下: 便の再開と席の立て直しで 2 本になる。純関数 1 本を両方が呼ぶ。
- 待ちの上限を rules 行で持つ（`SlotFree` と同型）。却下: ADR-0020 §2.3（reset 時刻が上限・縮退を持たない・便を Failed にしない）。
- 登録 row の口座を tick が env から読む。却下: C2.2。row の項目だけ。

## 11. 後続

席の起動と初回の口座選択（s2-07l.38）・token の refresh と再 login（user の手番）・上限停止の便の一覧画面（v3）・モデル別窓の display name の一致の細部（ADR-0017 §2.5）・SQLite 化（v3・A3）。
