# 設計: 口座の自律制御 — 上限で止まった便は段として残し別口座で続け、口座は 1 つの純関数が選び、席は上限の手前で退避して別口座で立て直す

- 要件: [FR35](../../design-intent/spec/srs.html#FR35) 上限停止の検知 / [FR36](../../design-intent/spec/srs.html#FR36) 口座選定 / [FR37](../../design-intent/spec/srs.html#FR37) 別口座での途中再開 / [FR38](../../design-intent/spec/srs.html#FR38) 開発 session の上限前の退避 / [AC12](../../design-intent/spec/srs.html#AC12) [AC13](../../design-intent/spec/srs.html#AC13)・既存 [FR33](../../design-intent/spec/srs.html#FR33) / [FR14](../../design-intent/spec/srs.html#FR14) / [FR22](../../design-intent/spec/srs.html#FR22) / [FR28](../../design-intent/spec/srs.html#FR28) / [FR29](../../design-intent/spec/srs.html#FR29) / [FR32](../../design-intent/spec/srs.html#FR32)
- 決定: [ADR-0020](../../design-intent/decisions/ADR-0020-account-autonomy-limit-stop-resume-and-selection.html)（本 doc の決定の正本・§2.1〜§2.6）/ [ADR-0012](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html)（上限 record）/ [ADR-0017](../../design-intent/decisions/ADR-0017-account-allowance-measured-into-event-log.html)（実測行・label・credential の場所）/ [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html) §2.1（登録 row）
- 土台: [fleet-usage.md](./fleet-usage.md)（計測）・[pipeline.md](./pipeline.md) §4（段）/ §5.7（resume）・[pipeline-question.md](./pipeline-question.md)（回答節）・[pipeline-conflict.md](./pipeline-conflict.md) §3（起こし直しの形）・[seat-autonomy.md](./seat-autonomy.md)（tick）・[seat-roles.md](./seat-roles.md) §2（登録 row の口座と起動の雛形）・[rules-manifest.md](./rules-manifest.md)
- 位置づけ: 口座は「上限を避ける」対象ではなく「使い切る弾」。便は当たるまで走り、当たったら別口座で同じ worktree から続く。席は当たる前に退避し、別口座で同じ target に立て直す。

## 1. 何を解くか

上限に当たった便は Failed（現物の detail は `runner-rc:<上限の rc>,commits:<n>`・[pipeline.md](./pipeline.md) §5.2・ADR-0020 §1 の `rate-limit` は誤記）で終端し、worktree と回答が捨てられて新しい便でしか続けられない。口座は席の起動時に人が選び、便は席の口座を継承するので席の口座が食い潰される。席はモデル別 7 日窓だけに当たって止まった（記録時点の事故型）。器は口座残量（FR33・実測行）を持つが選定に使っていない。

## 2. 上限停止の段（ADR-0020 §2.1・FR35）

- **`Stage::RateLimited`**（variant 1 つ・宣言順は `Questioned` の直後・`as_str` は `RateLimited`）。runner が上限の rc（`headless::RC_RATE_LIMIT`・値は現物・ADR-0012 §2.1）で終わった周は `RunStage stage=RateLimited detail=rc:<rc>,status:<観測した上限 status>` を記帳する（status は runner の停止行と同じ出所・ADR-0012 §2.1 末尾。**停止行は stdout に出す**〔記録時点は stderr＝pipe は stdout だけ捕らえるので届かない・(a) が stdout へ移し rc は変えない〕・停止行の形と読み手の純関数は同じ module `headless/runner.rs` に並べる〔質問 record の形と読み手が並ぶのと同型〕・pipe は rc が上限の周だけ読む）。口座の label は pipe が (c) の `--account-dir` の配線で初めて知るので、(c) が detail に `account:<label>` を足す。窓の種別は上限 record が運ばない（記録時点）ので段には書かず、選定は実測行（FR33）から読む。上限の判定入力は上限 record の status のみ（ADR-0012・本 doc は判定を足さない）。
- Failed に倒さない。worktree・base・commit・質問と回答の event を保つ（N1・C9）。終端ではない＝live な便として intake の排他の母集団に残る（ADR-0019 §2.1）。schema 1 のまま。lens が上限で止まった周は FR9 の既存極性（INCONCLUSIVE）のまま。
- `pipe show` はこの段を名で出す。`pipe stop --run` は本段の便も止められる（終端手段は stop だけ・§4）。

## 3. 選定の純関数（ADR-0020 §2.2・FR36）

- **入力**（2 種 + 用途）: manifest の `[[account]]` 行の label 列（宣言値）・口座 × 窓ごとの**最新の実測行**（`AllowanceMeasured` / `AllowanceUnmeasured`・実測値・reset 時刻付き）・用途（`Purpose::{Run, Session}`・closed enum）・使う model（`Option`・display name・与えられた周はその model のモデル別 7 日窓だけを数え、与えられない周はモデル別窓すべての最大を数える〔保守側〕。**便用は rules 行 `runner.model` の値を与える**〔runner と lens が同じ行の model を `--model` で毎回明示する（[pipeline.md](./pipeline.md) §6）ので、便が消費するのはその model の窓だけ＝他の model のモデル別窓が 100 でも便の候補から外さない。以前は「runner の起動形は model を渡さず claude の既定」を理由に与えていなかったが、実測（2026-09-14・4 便の transcript）では runner / lens は Opus で走り、便用の口座は Fable 窓 100 だけで偽に除外されていた（7 日窓 81%・5 時間窓 0% の口座が候補から消え並列が 3 便で頭打ち）。user 裁定 2026-09-14T21:59Z・契約は台帳 `s2-07l.297`〕・session 用は席の登録 row の `model` 項目（[seat-roles.md](./seat-roles.md) §2・任意・display name・無い row は None＝全 model 窓の最大を数える保守側〕を渡す。席ごとに使う model が違う〔planner の Fable 枠が当たっても管理席の口座には効かない・その逆も〕ので、席の逼迫度はその席の model の窓で数えないと退避が偽陽性になる）・除外する label の集合（席の登録 row が持つ口座・[seat-roles.md](./seat-roles.md) §2）・rules 行の値（session 用の閾値）。**席の判断・pane の字面・前の版の道具の出力は入力にしない**。
- **出力**: `Selection::{Chosen(label), None(NoCandidate)}`。`NoCandidate` は typed な理由（全口座が当たっている / 測れない / 除外で空）と**最も早い reset 時刻**（§4 の待ちに渡す）を運ぶ。
- **逼迫度** = その口座の窓のうち最大の使用率（5 時間・7 日・model が与えられた周はその model のモデル別 7 日窓だけ、与えられない周はモデル別窓すべての最大）。**当たっている** = 最新の実測行の使用率が 100（窓の全量・規則値ではない）に達している。
- **便用**（[ADR-0027](../../design-intent/decisions/ADR-0027-run-account-order-earliest-reset-and-no-per-account-cap.html) §2.1 / §2.2・ADR-0020 §2.2 の「逼迫度が最大」を supersede・user 裁定 2026-09-14T22:38Z）: 候補 = 当たっていない口座（席の登録 row が持つ口座は席の生死を問わず登録が在る限り外す・測れない口座は外す）。**走行中の便の数・口座あたりの便数では外さない**（1 口座に何便でも重ねる・上限に当たった便は §4 で別口座へ・器の外の起動手順が除外を足すのは N2 の散文規則＝器の出力が正）。順序 = 3 つの鍵の辞書順で先頭: (1) その口座の窓（5 時間・7 日・用途の model のモデル別 7 日）の最新の実測行が持つ **reset 時刻のうち最も早いもの**（昇順・使い残しは reset で消えるので近い口座から使い切る・C9.2・reset を持つ窓が 1 つも無い口座〔消費の無い窓だけ・ADR-0024〕は最後）→ (2) その口座で**走行中の便の数**（昇順・均等に寄せる・出所は spawn の event `SeatSpawned` の任意 field `account`〔器が選んで渡した label・field を持たない旧い便は 0〕を replay で数えた「非終端の段に在る便の数」）→ (3) label の辞書順。逼迫度（使用率の最大）は当たっている口座の判定と session 用の規則にだけ使う。同じ周に 2 便を続けて選ぶと同じ口座になりうる（意図した形・便数は spawn の event で増える）。純関数の入力に「口座ごとの走行中の便数」（`BTreeMap<label, usize>`・replay の導出値）が加わる。
- **session 用**: 逼迫度が最小で、かつ **R-C9-1 の値**（session 用の閾値・使用率の百分率）未満の口座（余裕を残す側）。
- **候補から外す**: 実測行が無い・`AllowanceUnmeasured`・reset 時刻を過ぎた古い行しか無い口座（測れない口座を選ばない・C10）。**reset 時刻を持たない `AllowanceMeasured` 行（消費の無い窓・ADR-0024）は古くない実測として数え**、逼迫度の最大には入るが（0）、候補なしの周の最も早い reset の導出には入れない（ADR-0024 §2.2）。選定の直前に器は FR33 の計測を 1 回撃つ（実測行の鮮度を選定側が保証する・既存の `fleet usage` の 1 コマンド）。同点は label の辞書順。
- **R-C9-1**: kind は既存の `AccountSelection` のまま、値を散文（`Policy`・enabled = false）から **Int**（session 用の閾値・百分率）に改めて enabled = true。値と裁定 id は契約 (b) が manifest に書く（**user 裁定**・§9）。便用の規則は閾値を持たない。
- 純関数は `fleet/select.rs`（新 module・in-file の歯・入力は値だけ・I/O なし）。読み手は §4 の再開・§5 の tick・`fleet select --state-dir S --purpose run|session [--model M] [--exclude L…]`（1 行で結果と理由を出す subcommand・席と歯の口）。

## 4. 別口座での途中再開（ADR-0020 §2.3・FR37）

- `pipe run` / `pipe resume` が `RateLimited` の段を見た周: 計測 → §3 の便用の規則で口座を選ぶ → 同じ worktree・同じ契約・同じ base で runner を起こし直す（`--account-dir <state_dir>/accounts/<label>`・ADR-0017 §2.3・FR5 の口のまま）。起こし直しの経路は [pipeline-conflict.md](./pipeline-conflict.md) §3 の `spawn_turn` と同じ 1 本（C6 の 1 つの spawn 口）。
- **初回の起動も同じ選定を通す**（`s2-07l.285`・便が planner 席の登録 row の口座で起きた 2026-09-14 の型＝操作役の launcher が席の口座の除外を手書きの list で運び、席の立て直しに追随しなかった）: `pipe run` / `pipe resume` が runner を**初めて**起こす周（`Intake` / 承認後の `Blocked` / 衝突の起こし直し）も、上と同じ順（計測 → §3 の便用の規則〔除外 = 登録 row の口座〕→ `spawn_turn`）で口座を選び、`Spawned detail=base:<sha>,account:<label>` を記帳して `--account-dir` を渡す。候補なしの周の待ちも上と同じ（`Completion::AccountFree`）。**manifest に口座の宣言が 1 つも無い周だけ**親の環境を継承する（`Spawned detail=base:<sha>`・従来の形＝口座を持たない toy repo の歯と、口座を器に預けていない host）。操作役に口座を選ばせる口（`--account`）は持たない（規則の写しを器の外に作らない・N2）。
- **prompt**: 契約（再読）+「回答」節（FR32・Questioned の再開と同じ）+「**途中再開**」節（base からの commit の一覧・止まった時刻・`headless/runner.txt` に節の雛形を足す・順序は 契約 → 回答 → 途中再開）。
- **候補なしの周**: `Completion::AccountFree { reset_at, state_dir }`（variant 1 つ・運ぶ値は pid でなく最も早い reset 時刻と実測行の置き場〔`is_met` が最新の実測行を読んで §3 を再評価する・`SlotFree` が `slots_dir` を運ぶのと同型〕＝現物の完了 enum の `pid()` は 0 を返す形）を足して唯一の wait 実装を通す。deadline は reset 時刻から計算した値（rules 行ではない・縮退を持たない）。Timeout（C11.3 の Result）を受けた周は計測して選び直し、候補なしなら次の reset 時刻で待ち直す。起こし直しの回数に上限を持たない（窓を跨いで続く）。待ちの間も便は live。終端手段は `pipe stop --run` だけ（器は自動では終端しない）。
- **記帳**: 再開のたびに `RunStage stage=Spawned detail=account:<label>,resume:rate-limit`（既存の段の event・detail で弁別）。人由来の event は 0（FR22）。
- **runner が死んだ便の起こし直し**（`s2-07l.323`・C9 の便版・FR37 / AC39・契約表の行 g）: `pipe resume` が `Spawned` の段を見た周は、まず最後の `SeatSpawned` の pid を唯一の wait（`Completion::SeatGone`・deadline 0＝`Timeout` が「生きている」）で測る（pane の字面は読まない・C3.3・生存の関数を新設しない）。生きていれば typed に断って runner を 2 本にしない（判定行 `run=<id> runner=alive pid=<pid>`・rc 1・event 0 件）。死んでいれば（host の再起動・OOM・kill で `SeatStopped` が書かれないまま消えた形）`SeatStopped detail=runner-dead` を記帳してから、上の 1 本（計測 → §3 の便用の規則 → `spawn_turn`）で同じ worktree・同じ契約・同じ base に runner を起こし直す（前の口座が候補ならそれで良い・候補なしの待ちも上と同じ）。「途中再開」節は上限の周と同じ場所に付き、止まった理由（`runner-dead`）と base からの commit の一覧に加えて**前の runner が残した未 commit の変更の一覧**（worktree の `git status --porcelain` の行・無ければ `なし`）を載せる（未 commit の file は消さない・N1・runner は続きから commit する）。記帳は `RunStage stage=Spawned detail=account:<label>,resume:runner-dead`（`resume:rate-limit` と同じ形・detail で弁別）。却下案: runner に定期 commit（WIP）を課す＝歯の RED→GREEN の順序と flip-check を壊し、契約の外の commit が増える／stop → run N+1 のまま＝結果を保たない（C9・2026-09-15 04:38Z の再起動で 2 便 × 45 分の実装を捨てた型）。

## 5. 席の退避と立て直し（ADR-0020 §2.4・FR38）

- **tick の判定に軸を 1 つ足す**（[seat-autonomy.md](./seat-autonomy.md) §3 の `judge`・inject / noop の判定であって guard ではない・極性一覧に載せない）: 登録済みの席ごとに、(1) 実測行の鮮度が既存の rules 行 `seat.tick_stale_s` の値より古い周は FR33 の計測を先に撃つ（定期計測はこの 1 形に限る）→ (2) 席の登録 row が持つ口座の逼迫度（§3 の定義・model = 登録 row の `model` 項目・無ければ None＝全 model 窓の最大〔保守側〕）を最新の実測行から読む → (3) **R-C9-1 の値以上**である間は、FR29 と同じ除外（退避物が在る周・cycle が走っている周は注入しない）の下で idle を待たずに退避の合図を注入する。実測行が無い・測れない周は注入せず `NoopReason` に理由 1 つ（縮退・止めない）。FR27 の打刻の合図はこの周は出ない（FR27 の条件に「使用率が閾値未満」が在る）。
- **立て直し**: 席が退避して止まった周＝次の 3 条件が同時に立つ周（(1) その席への直近の注入の記録が退避の合図〔`kind=externalize`・[vessel-hook.md](./vessel-hook.md) の `inject.jsonl`〕である (2) 打刻の最終行が `Stop` で、その ts が (1) の注入より後 (3) session が終わっている＝target の pane の前面 process が shell である〔[seat-state.md](./seat-state.md) §6・打刻に終了の event は無い〕。(1) が無い停止〔user の終了・crash〕は立て直さない〔登録 row の削除の口が無い間は、器が起こしたのでない停止を器が起こし直さない・crash の起こし直しは後続 §11〕）、tick は §3 の session 用の規則（model = 登録 row の `model` 項目・除外 = 他の席の登録 row が持つ口座）で口座を選び、登録 row が持つ**起動の雛形**（穴 `{account_dir}` 1 つ）の穴を選んだ label の credential dir で埋めて同じ target の shell へ注入し〔**shell への注入の門**: 席の注入の門（[seat-autonomy.md](./seat-autonomy.md)・入力欄は `❯` の行で特定する）は shell の pane では行を特定できない（終了した席の画面に残る古い `❯` 行で偶然通る周もある）。入口 (3) で前面 process が shell と分かっている周は、可視域の最後の非空行が shell の prompt 末尾（閉じた列の const・`$ ` `# ` `% ` `> `・字面は現物が正本）で終わり、その後に文字が無いことを門にする。特定できない・文字が在る周は 1 key も送らない（席の門と同じ極性・緩めない）〕、続けて復元の command（FR28・`seat rebrief`）を注入する。登録 row の口座 label を選んだ label に更新する（同じ鍵で再登録 = `SeatRegistered` 1 件・[seat-roles.md](./seat-roles.md) §2）。この更新は `seat register` の subcommand を経由せず、器の内部の同じ 1 関数が event を積み、`target` / `sid` / `launch` は既存 row から写す（打刻の条件は席の session が撃つ登録に課すもので、器自身の更新には課さない・`sid` は登録時の証拠であって現在の session の識別子ではない）。候補なしの周は立て直さず typed な理由を記帳し、次の tick で選び直す（0 口座で起こさない・止めない）。立て直しは既存の登録 row を持つ席に限る。
- 起動の雛形は env を読まず、器は穴を埋めて注入するだけ（host 名や絶対 path を解釈しない）。
- **復元の第 2 手**（台帳 `s2-07l.318`・planner 裁定 2026-09-15）: 立て直しと `seat launch` は復元の消費を settle の窓の内で確かめ、確かめられなければ `restore-unconfirmed` で終端する（SessionStart hook の所要が窓を食う周ほど復元が刺さらない）。終端の後は **tick の入口の 4 つ目 `Entry::Restore`** が拾う: cycle-stamp が在り、打刻の最終行が `SessionStart` でその ts が stamp 以上（起動したが 1 turn も始めていない＝復元が消費されていない typed な証拠・pane の字面は読まない C3.3）、席が idle、restore-stamp（exit-stamp と同じ形・write-ahead）が無いか rules 行 `seat.signal_backoff_s` 以上前、の順序固定 AND が立つ周に、復元の command（`--restore`・既定 `/rebrief`）を入力欄の門を通してもう一度注入する（`kind=restore`）。消費されなかった周は記録だけ残して終端せず、次の周が back-off の後にもう一度送る。立て直しの失敗の記録（`restore-unconfirmed`）は不変で、変わるのは「その次の周に手が在る」ことだけ（C9）。
- **accounts dir の定義（s2-07l.226 (1)・契約 (e)）**: `<state_dir>/accounts/<label>` は口座の**設定 dir 全体**（dir か、設定 dir への link）で、直下に `.credentials.json` を持つ。計測（[fleet-usage.md](./fleet-usage.md) §2）はその直下の credential だけを読み、立て直しの雛形の穴 `{account_dir}` にはこの dir がそのまま入る（子の `CLAUDE_CONFIG_DIR` に渡る＝立った session が skill / settings / plugins を持つ）。credential だけを置いた dir は計測は通るが、立て直しでは復元の command が unknown command で落ちる（AC13 実演 2026-09-13）。doctor（`doctor --state-dir S`）は manifest の label ごとに 1 行 `account=<label> dir=<present|missing> credential=<present|missing> config=<present|missing>` を出す（config = 直下に `settings.json` が在ること・Claude Code の設定 dir の印として読む）。doctor は判定しない（行を出すだけ・便用の口座は credential だけで足りる・verified の手書きはしない C10.2）。穴は 1 つのまま（credential と config を別の穴で受ける形は §10）。
- **trust の前提（(2)・契約 (e)）**: 対話 session は「設定 dir × anchor」の組が未 trust だと trust dialog で止まり、SessionStart の打刻が窓の内に来ないので立て直しは `relaunch-launch` で失敗する（`-p` の便には出ない・実測 2026-09-13）。器は trust を代筆しない（設定 dir 直下の `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` を器が書く形は §10）し、pane の字面で dialog に答えない（C3.3）。前提は **user が各口座の設定 dir で anchor を 1 回 trust する**こと（ADR-0017 の「1 口座 1 回の user の操作」と同じ種類の準備）。doctor は登録 row の anchor × manifest の label ごとに `trust=<accepted|missing|unreadable>` を上の行に足す（`.claude.json` の当該 key を読むだけ・書かない・file が無い・読めない周は `unreadable` で「missing」に潰さない）。
- **agent view の前提（(4)・契約 = 台帳 `s2-07l.239`・user 裁定 2026-09-14「scribe 下で動く claude code は常に disable」）**: Claude Code の agent view が有効な session は、background work（背景 Bash・agent・monitor）が残る周の `/exit` で「Background work is running」の dialog を出して止まる（.226 の実地 1 回目・`exit-absent`）。器は描画を読まない（C3.3）ので dialog に答えない。代わりに **器が起こす claude は常に agent view を切る**: 2 つの構築点（[pipeline.md](./pipeline.md) §6 の `build`〔runner / lens〕と本節の立て直し〔`relaunch` が穴を埋めた起動行〕）で子の env に `CLAUDE_CODE_DISABLE_AGENT_VIEW=1` を設定する（env を**読む**のではなく子へ**設定する**＝`CLAUDE_CONFIG_DIR` と同じ・C2.2 の内側・雛形は書き換えない＝行の先頭に前置する pure な関数 1 本）。user が手で起こす席は settings の `disableAgentView = true` が前提（trust と同じ「1 口座 1 回の user の準備」・器は書かない）で、doctor の口座行（(1) の `config=` の隣）が `agentview=<off|on|unreadable>` を読むだけで足す（契約 (e) = `s2-07l.233`）。
- **退避後の終了の手（(3)・契約 (f)）**: 立て直しの入口 (3)「前面 process が shell」は誰かが session を終えた後にしか立たない（実演では planner が `/exit` を送った）。tick に軸を 1 つ足す: **直近の注入が退避の合図（`kind=externalize`）∧ 打刻の最終行がその後の `Stop` ∧ 自席の未 consumed 退避物が在る ∧ 前面 process が shell でない**、の周に `/exit` を席の入力欄の門（cycle の `/clear` と同じ `guard_input`・`input-busy` / `input-unknown` は 1 key も送らない）を通して注入し（`kind=exit`・`InjectKind` に variant 1 つ・宣言順の末尾）、cycle-stamp を打つ（back-off は `s2-07l.110` の既存の 1 本・再送しない）。立て直しの入口 (1) は「直近の注入が `externalize` か `exit`」に広げ、(2) はその行の ts で測る。`/exit` は Claude Code の正規の終了で SessionEnd hook が走る（Ctrl-C ×2 / Ctrl-D ×2 は timing と入力欄の空に依存・§10）。退避物が無い周・cycle lock が live な周は送らない（FR28「退避物が無い session には撃たない」と同じ極性）。退避物の出現を見るのは `/clear` の cycle と同じ走査（frontmatter の `seat:`）で、終了の合図の記録は `inject.jsonl` の同じ形。
- **終了の手の stamp と送達確認（実地 3 回目・契約 = 台帳 `s2-07l.252`）**: 上の「cycle-stamp を打つ」は立て直しの back-off と stamp を共用する形で、`/exit` が成功した直後から立て直しが `seat.tick_stale_s` の間 `cycle-recent` で見送られる（実地: 終了の手の 5 分後から 6 周続けて見送り・席は user が手で起こすまで shell のまま）。2 つの back-off は目的が違う（終了の手 = `/exit` の二重投函を防ぐ／立て直し = 起こし直しの二重を防ぐ）ので stamp を分ける: 終了の手は **`exit-stamp`**（cycle-stamp と同じ形・同じ閾値 `seat.tick_stale_s`・write-ahead の極性は不変）に打ち cycle-stamp には打たない。立て直しの back-off は cycle-stamp だけを読む（＝直前の cycle / 立て直しからの経過）ので、`/exit` の次の tick で立て直しが評価される。終了の手の back-off は exit-stamp を読む（見送りの語は既存の `cycle-recent`・語彙を増やさない）。加えて `/exit` の送達確認は目印の出現数（注入の面の照合）に載せない: 席が終わると pane が shell に置き換わり目印は増えないので、成功が `absent`（rc 1）に倒れる（実地 3 回目）。終了の手の成立は入口 (3) と同じ typed な判定「前面 process が shell」で確かめる（送った後、注入の面と同じ窓・同じ刻みで待ち、shell になれば送達＝`kind=exit`・ならなければ `exit-unconfirmed`〔既存の語〕）。C3.3（描画を判定入力にしない）に沿い、他の注入の照合・極性は不変。
- **終了の手の第 2 手 = 停止（実地 2026-09-14 08:00Z・契約 = 台帳 `s2-07l.259`）**: `/exit` が窓の内に通らない周が在る——席が背景の仕事（Monitor・背景 Bash）を持つと Claude Code は終了の確認 dialog で止まり（agent view を切っても dialog は残り選択肢が減るだけ・公式 docs に dialog を消す設定は無い〔調査 2026-09-14〕）、`exit-unconfirmed` のまま人が Enter を押すまで立て直しに進まない。器は描画を読まない（C3.3）ので dialog に答えず、**終了を process の停止で確定する**: 終了の手の入口 (1)(2) が立ち、exit-stamp が在り（= `/exit` は送った）、前面 process が shell でない周は、`/exit` を再送せず、target の pane の shell の直下の子（`#{pane_pid}` の子・typed な metadata・/proc の親子で解く）の process group へ TERM → 唯一の wait 実装（`Completion::GroupGone`・猶予 = rules 行 `pipe.stop_grace_ms`）→ 残れば KILL → 同じ wait（便の停止 `pipe stop` と同じ列・C3.4）。止まれば記録は `kind=exit` と同じ形に `detail=terminated`（inject.jsonl の同じ行の形・停止の結果は closed enum `Stopped { Gone, Killed, Unstoppable }`・`InjectKind` は増やさない）、止まらなければ `exit-unstoppable`、pane の子が取れない・pid か group id が 2 未満の周は撃たず `exit-pane-missing`（`pipe stop` と `fleet usage` の停止と同じ guard・N1）。次の tick は入口 (3)「前面が shell」で立て直しへ進む。退避は済んでいる（入口 (1)(2)）ので失うものは無く、SessionEnd hook は走らない（`/exit` が通った周だけ走る＝台帳の同期は次の session の SessionStart / 席の終端で拾う）。stop-stamp は持たない（exit-stamp の back-off の内側では止まるまで毎周撃つ・TERM / KILL は冪等＝exit-stamp が `seat.tick_stale_s` より古い周は第 1 手 `/exit` からやり直す）。Guard は増やさない（終了の手 .226・立て直し .211 と同じく `Guard::Cycle` の内側・極性一覧の行数は不変）。却下: `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` を器が起こす席に前置く形（背景 Bash を丸ごと失い Monitor は残る・dialog の発生源の一部しか断てない）／席の作法として背景の仕事を持たない形（散文の規則・N2）。

## 6. 極性

本設計は guard を足さない（極性一覧は不変）。境界の型: `NoCandidate`（選定・FailOpen: 待つか記帳するだけで止めない）・`RateLimited` の段（終端でない）・計測の失敗は [fleet-usage.md](./fleet-usage.md) §6 のまま（`UnmeasuredReason` FailOpen）。

## 7. 歯（`crates/<NAME>/tests/e2e/` に `pipe_ratelimit_` / `fleet_select_` / `seat_account_` 接頭辞・名前の列は現物が SSOT）

- 段: 偽 runner が停止行を stdout に出して上限の rc で終わる fixture（実 runner と同じ形・headless の歯は実 runner の停止行が stdout に出ることを pin）で `RateLimited` の段・detail に rc と観測した status・Failed でない・worktree と commit が残る・`pipe show` が段名を出す・intake の排他が本段の便を live に数える・`pipe stop --run` で止められる。
- 選定（in-file・純関数）: 便用は逼迫度最大の当たっていない口座・席の口座は除外・session 用は逼迫度最小かつ閾値未満・当たっている口座（100）は選ばない・Unmeasured / 古い行の口座は候補外・同点は辞書順・全部当たっているときは `NoCandidate` に最も早い reset・model が与えられた周はその model の窓だけ数え、与えられない周はモデル別窓すべての最大を数える。
- 再開: 偽 runner を 2 回起こす fixture（1 回目は上限の rc・2 回目で commit）で別 label の `--account-dir` が渡り（stub が argv を写す）・段の detail に `account:<label>` が足される・stdin に「途中再開」節と順序（契約 → 回答 → 途中再開）・`Spawned detail=account:…,resume:rate-limit`・Landed まで通る（AC12）／全口座が当たっている fixture で `AccountFree` の待ち → 実測行を新しくすると待ちが解ける（deadline を fixture で短く）／`pipe stop --run` で待ちの便を終端できる。
- rules: R-C9-1 の値の型（Int）と enabled = true・欠落は `RuleError`・rules 外形 snapshot。完了 enum の網羅 match（compile）。
- tick（seat.rs・偽 tmux）: 2 席の fixture のうち 1 席の口座の使用率が閾値以上 → 退避の合図 1 件（FR29 と同じ除外が効く）・閾値未満 → 打刻の合図・実測行なし → noop に理由／Stop の後の周に起動の雛形の穴が別 label の dir で埋まった command と復元の command が順に注入され、登録 row の口座が更新される（AC13）／候補なし → 立て直さず記帳。
- 実地（done の一部・歯にしない）: live 口座 2 つ以上の host で、席 1 つの口座の使用率が閾値以上のとき退避 → 別口座で立て直し → 復元が 1 回通ること（AC13 の D）と、実便で上限に当たった記録が 1 件得られたら別口座で Landed になった事実を記録（AC12 の後半）。

## 8. 憲法・制約との整合

C1 / C5（R-C9-1 は行・裁定 id）・C2（`Purpose` / `Selection` / `Stage` は closed enum・選定は 1 関数）・C2.2（`--account-dir` の引数・env を読まない）・C3 / C3.4（段は event log・待ちは完了 enum の 1 variant と 1 実装）・C6（起こし直しは 1 つの spawn 口）・C9 / C9.2（人手なしの再開・使い切る）・C10（実測行を通してだけ選ぶ）・C11.2 / C11.3（`NoCandidate` の極性・Timeout は Result）・N1（worktree を捨てない）。

## 9. 契約（4 便・この順・実装は pipeline）

- **(a) 上限停止の段**（S）: `Stage::RateLimited`・runner の上限の rc → 段の記帳（rc / status）・`pipe show`・排他の母集団・stop。write-set = fleet/mod.rs（`Stage` の variant・`STAGES`・`as_str`）・headless/runner.rs（停止行の読み手・in-file の歯）・pipe/mod.rs・pipe/spawn.rs・pipe/cli.rs（段名の表示・live の集合・stop）・tests/e2e/{pipe,fleet,headless}.rs・snapshot。依存: なし（上限の rc は現物・label は (c) が足す）。順序: fleet/mod.rs・pipe/cli.rs・tests/e2e/pipe.rs で s2-07l.147 と交差するので .147 の land 後に直列（並列の組に入れない）。
- **(b) 選定の純関数と R-C9-1**（M）: `fleet/select.rs`・`Purpose` / `Selection` / `NoCandidate`・R-C9-1 の値の型変更（**値と裁定 id は user 裁定**）・`fleet select` subcommand・選定前の計測の呼出し。write-set = fleet/select.rs（新規）・fleet/mod.rs・fleet/cli.rs・rules/mod.rs（値の型）・rules/manifest.toml・tests/e2e/{fleet,rules}.rs・snapshot。依存: s2-07l.187（実測行の値の形）の land 後。
- **(c) 別口座での途中再開**（M）: `Completion::AccountFree`・`pipe run` / `resume` の RateLimited の経路・「途中再開」節・`--account-dir` の配線・記帳。write-set = pipe/follow.rs・pipe/cli.rs・pipe/spawn.rs（段の detail に `account:<label>`）・fleet/mod.rs・headless/runner.txt・tests/e2e/pipe.rs。依存: (a)(b)・s2-07l.147（land の順序制御・pipe/ と fleet/mod.rs を触るので直列）・[seat-roles.md](./seat-roles.md) 契約 (a)（便用の除外集合 = 登録 row の口座の読み手）。
- **(d) 席の退避と立て直し**（M）: tick の軸・鮮度で計測・登録 row の口座と雛形と `model`・立て直しの入口 3 条件（退避の合図の記録・Stop・pane の前面 process が shell）・立て直し（session 用・model = 登録 row・除外 = 他の席の口座・注入のたびに cycle-stamp を打つ＝再注入の back-off は `s2-07l.110` の既存の 1 本）・登録 row の更新。write-set = seat/tick.rs・seat/cycle.rs・seat/role.rs（登録 row の読み手）・seat/mod.rs（pane の前面 process を `list-panes` で読む関数・既存の tmux の口の隣）・tests/e2e/seat.rs。依存: (b)・[seat-roles.md](./seat-roles.md) 契約 (a)（登録 row）と契約 (e)（登録 row の `model` 項目）。
- **(e) accounts dir と trust の doctor 行**（S・fleet 側・s2-07l.226 (1)(2)・§5）: `doctor --state-dir S` の出力に manifest の label ごとの 1 行（`dir=` / `credential=` / `config=` / `trust=`）。読むだけ・書かない・判定しない。write-set = doctor の行の構築点（現物 `render_doctor_with` の隣・`main.rs`）・doctor の外形 snapshot・doctor の歯（`tests/e2e/`）。依存: なし（(d) Landed 済み）。base で RED = credential だけの dir と settings.json を持つ dir の fixture で `config=` の値が分かれる歯（機能不在）・`.claude.json` の当該 key の有無で `trust=` が分かれ、file が無い周が `unreadable` になる歯。
- **(f) 退避後の終了の合図**（S〜M・seat 側・s2-07l.226 (3)・§5）: tick の軸（4 条件の AND・順序固定）・`InjectKind::Exit`・入力欄の門・cycle-stamp・立て直しの入口 (1) の拡張。write-set = `seat/tick.rs`（軸・`InjectKind`・`relaunch_due` / `last_externalize` の拡張）・`seat/cycle.rs`（入口の読み手が cycle 側に在れば）・`tests/e2e/seat.rs`（偽 tmux の fixture: 退避の合図 → Stop → 前面 process が claude の周に `/exit` が注入され `kind=exit` が記録される／退避物が無い周・lock が live な周・入力欄に文字が在る周は 1 key も送らない／次の周で前面 process が shell になれば立て直しが立つ）。依存: (d) Landed 済み。base で RED = `kinds` の列に `exit` が在る歯（`tick.rs` の in-file `["pointer", "externalize", "relaunch"]` の pin を 4 つに）と、上の fixture の歯（機能不在）。

## 10. 却下案（ADR-0020 §5 の写しは持たない・設計固有のもの）

- 上限停止の段を `Questioned` の detail で表す。却下: 質問の再開と再開の契機が違う（回答が要らない）・C2「新しい理由は新 variant 1 つ」。
- 選定を tick の中に書く。却下: 便の再開と席の立て直しで 2 本になる。純関数 1 本を両方が呼ぶ。
- 待ちの上限を rules 行で持つ（`SlotFree` と同型）。却下: ADR-0020 §2.3（reset 時刻が上限・縮退を持たない・便を Failed にしない）。
- 登録 row の口座を tick が env から読む。却下: C2.2。row の項目だけ。
- 雛形に credential と config の穴を 2 つ持つ（§5 (1)）。却下: 穴 1 つの契約（`fill_launch`・`Holes::Many`）を壊し、計測と立て直しで口座の場所が 2 つに割れる。設定 dir 全体 1 つで両方が足りる。
- 器が設定 dir 直下の `.claude.json` に `hasTrustDialogAccepted` を書く（§5 (2)）。却下: Claude Code の私有形式への書込（ADR-0017 §2.5 と同じ fence・credential と同じ dir）で、trust は user が folder を信じる宣言＝器が代筆すると信じていない folder を信じたことになる。
- pane の trust dialog を字面で読んで答える。却下: C3.3（描画を判定入力にしない）。
- 退避の合図の文面に `/exit` まで含めて席に打たせる（§5 (3)）。却下: 散文で席を制御する（N2）・席が従わない周に立て直しが永久に立たない。器が入力欄の門を通して撃つ。
- `/exit` の代わりに Ctrl-C ×2 / Ctrl-D ×2 を送る。却下: 2 打の間隔（800 ms）と入力欄が空であることに依存し、失敗が入力欄の 1 文字削除や中断に化ける。`/exit` は 1 行の command で送達確認の形が既存の inject と同じ。
- `/exit` の dialog（agent view の「Background work is running」）へ Enter をもう 1 回送る。却下: 既定の選択肢を描画を読まずに知る手段が無い（C3.3）・cancel 側なら逆効果。dialog は agent view を切れば出ない（§5「agent view の前提」）。
- 退避の手順（ready-compaction）で席に background task を止めさせて dialog を避ける。却下: 散文で席を制御する（N2）・席が従わない周に立て直しが永久に立たない。器が起こす claude は器が env で agent view を切る。
- session を SIGTERM で終える。却下: 席の pid を器が握らない（登録 row に pid を置かない・[seat-roles.md](./seat-roles.md) §2）・打刻の外で終える。

## 11. 後続

席の起動と初回の口座選択（s2-07l.38 → [account-lifecycle.md](./account-lifecycle.md) §4・ADR-0026）・token の refresh（[fleet-usage.md](./fleet-usage.md) §3・s2-07l.229）・再 login（墓標・user の手番）・crash の起こし直し（§5 の入口 (1) が無い停止）・上限停止の便の一覧画面（v3）・モデル別窓の display name の一致の細部（ADR-0017 §2.5）・SQLite 化（v3・A3）。

## 12. usage refresh の歯の起動待ち（壁時計 5 秒）を負荷下でも足りる値へ（契約表の行 i・`s2-07l.385`）

- 何が起きているか: admin の実測 2026-09-16 01:5x〜02:05Z（母集団 = 本日 gate に到達して verify.stderr.log を持つ便 5・うち 3 = `.354` run 2 / `.247` run 1 / `.360` run 4）。歯 `fleet_usage_refresh_timeout_stops_the_child_and_its_grandchild`（`crates/scribe2/tests/e2e/fleet.rs`）が、7〜8 便が同時に gate / build を回す負荷の下で「child: 子が起動に達しない（pid file が 5s で書かれない・停止経路の失敗ではない）」で落ち、gate の n=3（workspace の歯）と n=7（検出線の baseline）の両方で便を Gated INCONCLUSIVE に倒す。anchor で単体なら 8.4 秒で PASS。
- 現物（verified・main 07310fe）: `const PID_FILE_WAIT: Duration = Duration::from_secs(5)`（fleet.rs:2765）を偽 claude の pid file 待ちと `spy_line` の poll 上限が共有する。歯は「起動に達しない」と「停止経路の失敗」を字面で弁別しており、落ちているのは fixture の起動待ちで器の停止経路ではない。
- 形: `PID_FILE_WAIT` を 60 秒にする。緑の周は pid file が書かれた時点で抜けるので費用は変わらず、赤の周だけ待ちが延びる。待ちの意味・panic の字面・停止経路の期待値（`REFRESH_TIMEOUT_S` / `STOP_MARGIN_S` / grace の算術）は不変。本体不変の歯だけの便＝base で RED を作れないので `// flip-check: retroactive` の札で通す（pipeline.md §5.3）。
- 触らない: 器の src・他の歯・並列度（並列の上限は write-set の重複と直列依存だけ・user 直命 2026-09-16）。
- 却下案: 並列度を下げる（user の許しが要る側・歯を直せば要らない）／歯を `#[ignore]` にする（停止経路の pin を失う）／CPU 負荷を測って待ちを伸縮する（fixture に測定を持ち込む・値の線が増える）。
- 同日の他の赤 2 本は本 § の対象外: `seat_tick_signal_backoff_does_not_brake_without_a_record` の赤は flip-check の base 段の「binary を起動できる: NotFound」（歯の壁時計ではなく base copy の binary 不在＝infra の側・`.380` の弁別子の対象）、`pipe_gate_move_proof_zero_moved_items_sends_diff` は verify.stderr.log に panic の字面が無く未分類。

## 13. 選定の前計測の鮮度 — 新しい実測を測り直さず、計測自身の 429 で口座を失わない（契約表の行 j・`s2-07l.407`）

- 何が起きているか（admin の実測 2026-09-16 09:57Z〜10:30Z・verified・母集団 = 第 2 陣以降の `fleet select` 全回）: §3 の「選定の直前に FR33 の計測を 1 回撃つ」は 1 回の選定で manifest の**全口座**を測り直す。短時間に選定を重ねると usage endpoint が **HTTP 429** を返し、全口座が `unmeasured reason=http_status` → `none=unmeasured` で 1 本も出せない。内訳は IP でなく **token 単位**: 走行中の runner / lens の claude（11 process）が同じ口座の token で usage を poll しており、その口座だけが選定の計測で落ちる（単発の curl は 200）＝**便を増やすほどその口座が選べなくなる**構造。上限の合図（ADR-0012）でも口座の残量でもなく、器自身の計測の burst と競合が原因。
- 形: (1) rules 行 `fleet.usage_fresh_s`（kind `UsageFreshS`・Int・秒・**値は user 裁定**・C5）。(2) `fleet select` の前計測は**鮮度つき**: 口座の最新の回（`latest_round`）の数える窓が全部 `AllowanceMeasured` で、その ts が `now − fresh_s` より新しい口座は**測り直さない**（子 process を起こさず event も書かない）。古い口座・Unmeasured の口座・行の無い口座だけを測る。(3) 測った結果が `HttpStatus` / `Timeout`（読みに届かなかった側・本文の形の失敗は含まない）で、その口座の**最新の回が実測**（数える窓が全部 `AllowanceMeasured`・古さは問わない＝(2) で「古い」と判定して測りに来た口座がここに来る）なら、その Unmeasured 行を**追記せず**その実測を最新のまま使う（stderr に `usage: account=<label> kept reason=<reason>` の 1 行・stdout は 1 行形のまま）。実測の古さの判定は選定の既存の規則（`select.rs` の `fresh_windows`＝reset を過ぎた実測だけを古いと読む）に任せる＝kept は「値の無い行で有効な実測を上書きしない」だけで、鮮度の規則を 2 か所に持たない。最新の回が Unmeasured・行なしの周は従来どおり追記する（(2) の条件と (3) の条件は重ならない: (2) は「新しい実測は測らない」・(3) は「測った古い実測を 429 で捨てない」）。(4) 計測の方針は閉じた enum（`Freshness::{Always, Within(secs)}`・`fleet usage` の口は `Always`＝挙動不変・選定の前計測だけ `Within`）で `measure` に渡す＝計測の実装は 1 本のまま。
- 触らない: 純関数 `select`（`fleet/select.rs`）と `Input`（構築点 6 か所・`prop.rs` を含む）・replay の `State`・event の形・`UnmeasuredReason` の variant・`fleet usage` / `--show` の外形・R-C9-1。
- 歯（`fleet_select_fresh_` 接頭辞・`tests/e2e/fleet.rs`）: 偽 client の呼出回数を写しで数え、新しい実測を持つ口座は選定で測り直されない（呼出 0）／古い実測の口座は測り直される（呼出 1）／429 を返す偽 client + 古い実測（reset 前・`fresh_s` より古い）の口座 → 測り直され、Unmeasured が追記されず（event の本数不変）選定がその口座を候補に残す／429 + 最新の回が Unmeasured の口座 → 追記され候補から外れる（従来）／`fleet usage` は鮮度に関わらず全口座を測る（`Always`）。rules 行は kind 件数の pin + 外形の歯（`fleet.usage_timeout_s` と同型）。
- 却下: 選定ごとに 1 口座だけ測る（候補の比較が古い値と新しい値の混在になる）／壁時計の sleep で間引く（壁時計依存・費用が増える）／replay で「最新の Measured」を別に持ち `select` に渡す（`Input` の構築点 6 か所と `prop.rs` を動かす・鮮度の規則が純関数に入り値の線が 2 か所になる）／429 の口座を admin が `--account` で名指しする（器の選定 FR36 の迂回・N2）。

## 14. 便用の除外は便の repo（anchor）の席だけ — 置き場を共有する他 vessel の席の口座は候補（契約表の行 k）

- 何が起きているか（2026-09-16 12:0xZ・verified）: 便用の選定の除外集合は `fleet/replay.rs` の `registered_accounts()`＝置き場（state dir）に在る**全部**の席の登録 row の口座。置き場は host の口座 dir ごとに 1 つで複数の vessel（repo）が共有するので、別 repo の席（同じ host で planner / admin を持つ他 project）の口座まで本 repo の便から外れる。実測: 登録 row 4 席（本 repo 2 + 他 repo 2）・宣言 7 口座のうち席で 4 が外れ、当たっていない候補が 1〜2 に痩せて並列が口座の窓で頭打ちになった。user 裁定 2026-09-16T12:09Z（逐語は台帳 `s2-07l` notes・A1「使う」）: 他 repo の席の口座を本 repo の便に使ってよい。FR40 の席の識別子は（役割, anchor）＝「席」は anchor ごとに定まるので、FR36 / ADR-0027 の「席の登録 row が持つ口座」を**便の repo の anchor の row**と読む（SRS の字面は変えない）。
- 形: (1) `registered_accounts`（`fleet/replay.rs`）は anchor を受けて**その anchor の登録 row の口座だけ**を返す（`Registration.anchor` と便の repo の一致・文字列の完全一致で比べ正規化しない＝登録が書いた値がそのまま鍵）。anchor を持たない呼び手（口座の退役の検査・`account/mod.rs`）は従来どおり全 row を見る（退役は host 全体の席を守る側・弁別の形は現物で決める）。(2) `select_for_run` は便の repo（`Turn.repo`・`pipe run --repo` の値）を受けて (1) に渡す。呼び手は `pipe/ratelimit.rs` の `choose_or_wait`（`choose_account` の 2 呼び手 = `ratelimit.rs` / `follow.rs` が `Turn.repo` を渡す）と `fleet/wait.rs` の `account_free`（`AccountFree` の観測も同じ除外で再評価する・C3.4）。(3) `fleet select --purpose run` に `--anchor DIR` を足す（無い周は従来どおり全 row を除外＝保守側・stdout は 1 行のまま）。順序の鍵・当たっている口座・測れない口座の扱い（§3 / §13）は不変。
- 触らない: 純関数 `select`（`fleet/select.rs`）と `Input`・event の形（`SeatRegistered` に列を足さない）・`--exclude` の意味・R-C9-1・session 用の規則。
- 歯（`fleet_select_anchor_` 接頭辞・`tests/e2e/fleet.rs`）: 2 anchor の登録 row を置いた置き場で `--anchor` に片方を渡すと他方の席の口座が候補に入る（`chosen` がその口座）／`--anchor` 無しは従来どおり両方外れる／`pipe run` が便の repo を渡して他 repo の席の口座を `Spawned` の account に記す 1 本。既存の歯の側: pipe e2e の helper `register_seat_account`（`tests/e2e/pipe/ratelimit.rs`）は登録 row の anchor を固定の字面で置き、(1) の一致で便の repo と食い違うので、その helper に依る既存の歯（`tests/e2e/pipe/spawn.rs` / `tests/e2e/pipe/land.rs` の便の口座を測る 2 本）は head で赤になる＝helper が便の repo の anchor を置くよう直し、helper の file と歯の file 2 つを write-set に数える（run 235030Z の QUESTION の解）。
- 却下: 除外を host の全 vessel の席へ広げる（`.328` の (a)(b)・user 裁定で却下）／launcher が `--exclude` で席を足す（ADR-0027「器の外の起動手順は除外を足さない」・N2）／登録 row を消して席の口座を候補に入れる（席の立て直しの口座が消える・N1.2）。

## 15. lens の口座も器が選ぶ — gate の lens 起動に便用の選定を通し記帳する（契約表の行 l・`s2-07l.412`）

- 何が起きているか（admin の実測 2026-09-16 11:5xZ・verified）: gate の lens 起動（`pipe/gate.rs` → `gate/lens.rs` の `ask_lens`）には `pipe/spawn.rs` の `with_account` が無く、器は lens の口座を選ばず記帳もしない。操作役の launcher（器の外）が前置きの select の口座を lens の起動行に書いて補っている（N2 の散文運用・§3「器の外の起動手順が除外を足すのは散文規則」と同型）。lens は runner と同じ `runner.model` の窓を消費する（[pipeline.md](./pipeline.md) §6）ので、選定なしでは席の口座や当たった口座で走りうる。
- 形: (1) `pipe gate` は lens を起こす直前に §3 の便用の規則で口座を選ぶ（計測 → `select_for_run`〔便の repo・§14〕）。入力は `Pool::declared`（`--rules` / `--curl` は gate の引数から・`pipe/ratelimit.rs` の既存の口）。`Gate` の構築点は 2 つ（`pipe/cli/step.rs` の `gate_run`＝`args` を持つので `Pool::declared` を解いて渡す・`pipe/land.rs` の `follow_main`＝追随の再 gate・`Land` の `runner` が持つ `Pool` を借りて渡す）で、`Gate` に選定の材料（`Pool` の参照・無ければ継承）を 1 欄足す。宣言 0 の周は従来どおり親の環境を継承する。(2) 起動行の末尾に `--account-dir <state_dir>/accounts/<label>` を足す口は `with_account`（`pipe/spawn.rs`）の **1 関数**を共有する＝引数を `Launch` 全体から「label と state_dir」に絞って gate から呼べる可視性にし、runner 側の唯一の呼び手（`launch_runner`・同 file）は絞った形で同じ結果を得る（足す字面は不変）。(3) 記帳: `RunStage stage=Gated detail=verdict:<V>` の detail に `account:<label>` を足す（`Spawned` の detail と同じ語彙・人由来の event は 0）。(4) 候補なしの周は lens を起こさず INCONCLUSIVE（理由 `account:none=<reason>`）＝gate は待ちの口を持たない（`resume` が撃ち直す・`AccountFree` の待ちは §4 の runner 側だけ）。
- 触らない: 純関数 `select` / `Pool` の形・runner の起動行と選定（§4）・lens の prompt と cap・launcher。
- 歯（`pipe_gate_lens_account_` 接頭辞・`tests/e2e/pipe/gate.rs`）: 口座の当たり / 空きは置き場へ実測行を直接置くのでなく、既存の `fake_usage_curl`（`tests/e2e/pipe/ratelimit.rs`・token ごとの応答本文）で作る（`choose_account` は毎回計測し直し、最新の 1 行が置き場の行を無条件に置き換えるので、直接置いた行は計測で上書きされ歯が空虚になる）。宣言口座 + 当たっていない応答を置いた置き場で、偽 lens が argv を写し、起動行の末尾に選んだ口座の `--account-dir` が在り `Gated` の detail に `account:<label>` が在る／宣言 0 の周は従来どおり `--account-dir` 無し／全口座が当たっている周は lens を呼ばず INCONCLUSIVE に `account:none` の理由。
- 却下: launcher が lens の起動行に口座を書く（N2・記帳と実体が食い違う）／lens を runner と同じ口座に固定する（runner の口座が当たった直後の lens が同じ窓で落ちる）／gate で `AccountFree` を待つ（gate は段の判定で待ちを持たない・resume の撃ち直しで足りる）。

## 16. 同じ flag の二重を断る — `--account-dir` が 2 つ在る起動行で器の選定が黙って無効になる穴（契約表の行 m・`s2-07l.411`）

- 何が起きているか（admin の実測 2026-09-16 11:5xZ・verified）: headless の `flag()`（`headless/mod.rs`）は**最初の出現**を採る。launcher が runner の起動行に `--account-dir` を書き、器（`with_account`）が末尾に選んだ口座を足すと 2 つ並び、器の選定が黙って無効・`Spawned` の記帳（account:<label>）と実行の口座（子 claude の設定 dir）が食い違った（C10 の実測の出所が偽になる）。
- 形: (1) `flag`（`headless/mod.rs`）は同名の flag が 2 回以上在る周を typed に断る（`Err`・理由に両方の値）＝runner / lens の全 flag に効く（読み手は 1 関数・C2）。(2) `with_account` は起動行に既に `--account-dir` が在れば足さずに spawn を断る（typed・理由に既存の値・N2 の混入を受付で止める・FailClosed）。断りの型は `pipe/spawn.rs` に新設する閉じた enum 1 つ（起動行の受付の境界・variant は「`--account-dir` が既に在る」の 1 つ・既存の値を運ぶ）で、`with_account` の戻りを `Result` にし、唯一の呼び手は同 file の `spawn`（`Spawned` の emit より前・worktree と plugin を組んだ後）へ移して既存の `refused` の面へ写す（断った周は `Spawned` も `SeatSpawned` も記帳しない・現物の `spawn` は `Spawned` を emit してから `launch_runner` を呼ぶ）。既存の `Refuse`（`pipe/refuse.rs`・契約単位の判定・呼び手は intake）は使わない（領分が違い write-set の外）。境界の `POLARITY` は同 file に置き、`crates/scribe2/src/polarity.rs` の `Guard` の 4 つ組と `as_str` に 1 つ足す（極性一覧に載る・C11.2）。(3) `Spawned` の `account:<label>` は器が足した口座だけ＝記帳と実行の一致は (1)(2) で構造的に成り、歯で pin する。
- 触らない: `flag` の値の読み方（次の token・`--` 始まりは値でない）・runner / lens の他の引数・launcher。
- 歯（`headless_flag_duplicate_` 接頭辞・`tests/e2e/headless.rs` と `tests/e2e/pipe/spawn.rs`）: runner の argv に `--account-dir` ×2 → claude を呼ばず引数不正の rc で断る（偽 claude の呼出 0）／lens も同じ／`with_account` は起動行に `--account-dir` が既に在ると足さずに断る（spawn の e2e・Spawned が記録されない）／runner と lens の argv に `--worktree` ×2 → 同じ断り（`need` は `flag` の包み・全 flag に 1 経路で効く pin）。
- 却下: 最後の出現を採る（launcher の混入を黙って上書きする＝散文運用を器が受け入れる）／runner だけ直す（lens も同じ読み手）／`with_account` が既存の値を置換する（どちらが正か器に分からない・C10）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "g"
title = "runner が死んだ便の起こし直し — pipe resume が Spawned の便の runner の生死を唯一の wait で測り、死んでいれば同じ worktree で起こし直す（途中再開の節に未 commit の一覧）"
req = ["FR37", "FR14"]
section = "4"
write-set = ["crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_resume_kill_"]
size = "M"
done = "Spawned で runner が死んだ便に resume を撃つと同じ worktree で runner が起き直り途中再開の節に未 commit の file 名が載り、生きている runner の便は typed に断られて runner が 2 本にならない"

[[contract]]
id = "h"
title = "初回 spawn の口座を器が選ぶ — pipe run / resume の初回の起動も計測 → 便用の選定 → spawn_turn の 1 本を通り、口座の宣言が無い周だけ親の環境を継承する"
req = ["FR36", "FR4"]
section = "4"
write-set = ["crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_spawn_account_"]
size = "M"
done = "初回の起動が器の選んだ口座の credential dir で起き Spawned の detail に label が載り、口座の宣言が無い toy だけが親の環境を継承する"

[[contract]]
id = "i"
title = "e2e/fleet.rs の PID_FILE_WAIT を 5 s → 60 s（負荷下で usage refresh の歯が「子が起動に達しない」で gate を落とす・歯だけの便・retroactive）"
req = ["FR36"]
section = "12"
write-set = ["crates/scribe2/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_usage_refresh_timeout_stops_the_child_and_its_grandchild"]
size = "S"
done = "PID_FILE_WAIT が 60 秒で、歯の名・panic の字面・停止経路の期待値が不変"

[[contract]]
id = "j"
title = "選定の前計測の鮮度 — rules 行 fleet.usage_fresh_s の内側の実測を測り直さず、計測自身の 429 / timeout では直前の新しい実測を保つ（fleet usage の口は不変）"
req = ["FR36", "FR33"]
section = "13"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "docs/design/rules-manifest.md", "crates/scribe2/src/fleet/select.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_select_fresh_"]
size = "S"
done = "新しい実測を持つ口座は選定で測り直されず、測り直した口座が 429 / timeout を返す周も最新の実測が上書きされず候補に残り、fleet usage は全口座を測り、rules 行が裁定 id 付きで 1 本増える"

[[contract]]
id = "k"
title = "便用の除外は便の repo（anchor）の席の登録 row の口座だけ — 置き場を共有する他 vessel の席の口座を候補に入れる（fleet select --anchor・pipe は Turn.repo を渡す）"
req = ["FR36", "FR40"]
section = "14"
write-set = ["crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_select_anchor_"]
size = "S"
done = "他 repo の席の口座を持つ置き場で本 repo の便の選定がその口座を chosen に出し、--anchor 無しの fleet select は従来どおり全 row を外す"

[[contract]]
id = "l"
title = "lens の口座も器が選ぶ — pipe gate が lens を起こす直前に便用の選定を通し、with_account の 1 関数で起動行に足し、Gated の detail に account:<label> を記帳する（候補なしは INCONCLUSIVE）"
req = ["FR36", "FR33"]
section = "15"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_lens_account_"]
size = "S"
done = "宣言口座のある置き場で gate の lens 起動行の末尾に選んだ口座の --account-dir が在り Gated の detail に account:<label> が出て、宣言 0 は従来どおり、候補なしは lens を呼ばず INCONCLUSIVE"

[[contract]]
id = "m"
title = "同じ flag の二重を断る — headless の flag（headless/mod.rs）が同名 2 回以上を typed に断り、with_account は起動行に既に --account-dir が在れば足さずに spawn を断る（記帳の口座と実行の口座の一致を歯で pin）"
req = ["NFR4", "FR36"]
section = "16"
write-set = ["crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_flag_duplicate_"]
size = "S"
done = "runner / lens の argv に同じ flag が 2 つ在ると claude を呼ばずに断り、with_account は既に --account-dir を持つ起動行を足さずに断って Spawned を記録しない"
<!-- contracts:end -->
