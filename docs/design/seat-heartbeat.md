# 設計: seat heartbeat — 管理 tick を backoff つきで戻す（合図は 40 分黙った席にだけ・無変化なら間隔を倍々に伸ばし・次の待ちが 24 時間を超える段では送らない）

- 要件: [FR27](../../design-intent/spec/srs.html#FR27) 管理 tick（heartbeat）/ [FR43](../../design-intent/spec/srs.html#FR43) 席の継続 / [FR44](../../design-intent/spec/srs.html#FR44) 席間の連絡（廃止・差し込みは 3 つだけ）/ [FR64](../../design-intent/spec/srs.html#FR64) 管理 tick の駆動 / [FR38](../../design-intent/spec/srs.html#FR38) 群の逼迫（tick は測らない）/ [FR40](../../design-intent/spec/srs.html#FR40) 席の登録 row / [AC18](../../design-intent/spec/srs.html#AC18) / [NFR4](../../design-intent/spec/srs.html#NFR4)。制約: CON2（PUBLIC repo・unit は repo に入れない）
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) 規則は manifest の行 / [C2](../../design-intent/spec/constitution.html#c2) env を直読しない・読み手は 1 本 / [C3](../../design-intent/spec/constitution.html#c3) 状態は 1 つの置き場・自由文を判定入力にしない / [C9](../../design-intent/spec/constitution.html#c9) 席の停止の検知と再開 / [C10](../../design-intent/spec/constitution.html#c10) 測れないを成功に倒さない / [C11](../../design-intent/spec/constitution.html#c11) 極性は型で / [C17](../../design-intent/spec/constitution.html#c17) 足す前に消すものを名指す
- 決定: [ADR-0058](../../design-intent/decisions/ADR-0058-heartbeat-returns-with-backoff-and-stops-at-24h.html) §2（判定の列・digest・梯子・rules 行 4 本・unit）/ [ADR-0030](../../design-intent/decisions/ADR-0030-tick-units-are-written-and-enabled-by-the-vessel.html) §2.1〜§2.5（unit の導出と書き）/ [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html)（状態は hook の打刻）/ [ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (2)（戻さないもの）
- crate の形は [rules-manifest.md §2](./rules-manifest.md) に従う。席の面の歯の置き場は [seat-roles.md §7](./seat-roles.md)。
- この設計から出る契約: 行 a（`seat tick`＝判定の列・変化の digest・梯子の記録・打刻の合図・rules 行 4 本・不在の歯の書き換え）・行 b（`seat tick install` / `uninstall`＝unit の導出と書き・doctor の 1 項目）。行 b は行 a に依存する。

やさしく言うと: 席（人と AI が話す session）が黙ったままになったとき、器が外から「続きを進めて」と 1 行差し入れる仕組みを戻す。ただし、席に何も変化が無ければ差し入れる間隔を 40 分 → 80 分 → … と倍々に伸ばし、次の待ちが 24 時間を超えたら送るのをやめる（合図は最大 6 本）。席が動けば最初の間隔に戻る。周期は host の systemd の timer が作り、その unit は器が書く。

## 1. 何を解くか（裁定と現物）

- 出所: user 裁定 2026-09-23T14:02Z（逐語は台帳 `s2-07l.580` の本文）と ADR-0058 §2。前の器の合図の backoff の形（user 裁定 2026-09-17T00:55Z・逐語は台帳 `s2-07l.423` の notes）は ADR-0058 CTX3 が要約する。
- 現物（verified・main 0f20808）:
  - 席の状態は hook の打刻 `<state_dir>/seat/<潰した target>/state.jsonl` が持つ（`crates/scribe2/src/seat/state.rs` の `Stamp`＝`schema` / `state` / `event` / `ts` / `sid`・SessionStart と Stop が Idle・UserPromptSubmit が Busy・書き手は `crates/scribe2/src/hook/stamp.rs`）。読み手は席の起動の送達確認（`evidence_after`）だけで、最終行を読む口は無い。
  - 席の登録 row は fleet の replay の最新（`crates/scribe2/src/seat/role.rs` の `registration_of_target`・`Registration` は `role` / `anchor` / `target` / `account` / `launch` / `model`）。
  - 注入の唯一の入口は `crates/scribe2/src/seat/inject.rs` の `deliver_within`（送達 → 消費の証拠は UserPromptSubmit の打刻・記録は同じ dir の `tick.jsonl` に `InjectionRecord`・`who` は `seat-inject`）。入力欄の門は `pass_input`（`guard_input` の 3 値 Clear / OwnQueued / Foreign・自席の文は `last_own_payload` の前方一致・OwnQueued は Enter を 1 回だけ）。呼び手は席の起動（`crates/scribe2/src/seat/cycle/relaunch.rs`）と dispatcher の通知（`crates/scribe2/src/pipe/notify.rs`・窓は rules 行 `pipe.stop_grace_ms`）の 2 つ。
  - 口座の計測の記録は `crates/scribe2/src/fleet/usage.rs` の `fresh_rows`（鮮度 `fleet.usage_fresh_s` の内側の最新の回だけ・計測は起こさない）と `latest_of`。逼迫の判定は `crates/scribe2/src/hook/group.rs` の `pressed`（pure・`Caps` は `fleet.group_pressure_5h_pct` / `7d_pct` / `model_pct` の 3 行）。
  - rules 行の読み手は `crates/scribe2/src/rules/mod.rs` の `int_row`。行の種類は `RuleKind`（閉じた enum・`ALL` の宣言順・外形 snapshot `rules_external_form` は rows=63）。`seat.tick_*` / `seat.pointer_backoff_*` の行は無い。
  - `seat` の使い方は `register` / `launch` / `ruling` / 短い形だけ（`crates/scribe2/src/seat/cli.rs`）。不在を測る歯 3 本（`tests/e2e/seat.rs` の `seat_autonomy_subcommands_are_gone_from_the_usage` / `seat_inject_subcommand_is_gone_from_the_usage` / `seat_working_memory_subcommands_are_gone_from_the_usage`）が `tick` の不在と、席の口が `--rules` を 1 つも受けないことを pin する。
  - `systemctl` を撃つ口は `crates/scribe2/src/pipe/confine.rs` の定数（private）だけ。
- 戻すのは 3 つ（ADR-0058・C17.2）: 管理 tick の口・打刻の合図・tick の unit。戻さないもの: 席の context 計測と退避の合図・cycle・tick-stamp の file・席が自分で打つ生存の判子・hook 集合の食い違いの判定・`seat inject` の口・作業記憶の口。

## 2. 管理 tick `seat tick`（契約表の行 a・ADR-0058 §2）

- 口: `seat tick --state-dir S --target S:W [--rules F] [--tmux-socket PATH] [--capture-file PATH]`。stdout に判定行 1 行・rc は 0（inject / noop）か 1（error）。`--rules` は歯の seam（行 4 本の写しを差し替える・`rules` の口と同じ読み）で、席の口のうち `tick` と `tick install` だけが受ける（不在の歯の `--rules` の assert は形 7 で書き換える）。env・home・自分の実行 file の場所は読まない（C2.2）。
- 形 1 **判定の列**（順序固定の AND・最初に立たなかった条件を理由にする・判定は閉じた enum TickDecision の 3 値 Inject / Noop(NoopReason) / Error(TickError)・bool で持たない・C11）。理由の値の名は次の宣言順（`as_str` の字面＝判定行の `reason=` の語）:
  1. **登録 row の門** `no-row`: `registration_of_target` が target の row を返さない周は注入しない（登録の無い席は器の管理外・FR40）。
  2. **状態の打刻** `state-missing` / `state-unreadable` / `busy` / `state-stale`: `state.jsonl` の最終行（読めた行のうち最後）を読む。file が無い（hook が載っていない席）・読めない・最終行が Busy（turn の途中）は注入しない。Busy が `seat.tick_stale_s` より古い周は `state-stale`（Stop の打刻を失った席＝人が見る）。Idle だけが進む。
  3. **変化の digest の比較**（settle）`settling` / `record-unreadable`: 形 2。梯子の記録が無い周は段 0 として進む。記録が在って基準が未確定の周は settle を試み、確定できなければ `settling`。記録が在るのに読めない周は `record-unreadable`（0 件に潰さない・fail-closed）。
  4. **黙りの門** `stamp-recent`: 最終行の `ts` から `seat.tick_stale_s` 未満なら送らない（席は最近まで動いていた＝黙っていない）。境界は未満・時計は `state.jsonl` と同じ UTC 秒。
  5. **上限の判定** `stopped`: 段の候補の待ち（形 2）が `seat.pointer_backoff_max_s` を超える段は送らない（判定行 `pointer=stopped`）。
  6. **床** `wait`: 記録の `sent_at` から段の候補の待ちが経っていない周は送らない（`pointer=wait:<残り秒>`）。記録が無い周は床を通る（初段）。
  7. **口座の門** `account-pressed`: 形 4。
  8. **pane と入力欄の門** `pane-missing` / `input-busy` / `input-unknown` / `input-own-queued`: pane を取れない周は注入しない。`pass_input`（`own` は `last_own_payload`）を通し、Foreign は `input-busy`・prompt 行を特定できなければ `input-unknown`・自席の文が Enter 1 回の後も残れば `input-own-queued`。
  9. **記録** `record-unwritable`: 形 2 の記録を一時 file → rename で書く。書けない周は 1 key も送らない（fail-closed・ADR-0058）。
  10. **注入**: `deliver_within`（窓は rules 行 `pipe.stop_grace_ms`＝dispatcher の通知と同じ行・行を増やさない）。送達の結果（消費 / queue / 断り / 未確認）は判定行に載せ、**落ちても送ったと数える**（記録は残す・次の段で再送・best-effort）。
  - 実行系が回らない周は `decision=error`・rc 1（TickError の閉じた値: `state-dir`＝置き場を解けない / `no-rule`＝行 4 本か `pipe.stop_grace_ms` か群の閾値の行が読めない・不発効・整数でない / `store`＝fleet の replay が読めない）。noop の語彙を汚さない（席が静かなのか器が壊れているのかを記録から読める）。
- 形 2 **変化の digest と梯子の記録**:
  - digest は `state.jsonl` の最終行の `ts` の 1 値（event の種類は問わない）。fleet の event log・context・台帳・pane の字面は材料にしない（ADR-0058）。
  - 記録は席の置き場の file 1 つ `<state_dir>/seat/<潰した target>/pointer-ladder`（1 行 JSON・`schema`=1・`sent_at`〔送った UTC 秒〕・`step`〔段・0 始まり〕・`digest`〔基準の ts・未確定は null〕）。書き手は tick だけ・一時 file → rename・`tick.jsonl` とは別 file（注入の記録は `InjectionRecord` のまま増やさない）。
  - **settle**（基準の確定）: 記録の `digest` が null の周、`state.jsonl` に `sent_at` より後の Stop の打刻が在ればその `ts` を基準に書く（合図に応えた turn の終わり）。無い周は、`sent_at` から `seat.tick_stale_s` を過ぎていれば今の digest を基準に書く（応えない席＝梯子が登る側に倒す）。どちらでもなければ `settling`（比べない・送らない）。
  - **段の候補**: 記録が無い → 0。基準あり ∧ 今の digest ≠ 基準 → 0（変化）。同じ → 記録の `step` + 1。段 n の待ち = `seat.tick_stale_s` × `seat.pointer_backoff_factor` ^ n（秒・飽和演算）。初期値では 40 分 → 80 → 160 → 320 → 640 → 1280 分（段 5・21.3 時間）、段 6 の待ち 2560 分は上限 24 時間を超えるので `stopped`＝合図は最大 6 本・最後は初回から約 41 時間後。
  - **送る周**: 記録を `sent_at`=今・`step`=段の候補・`digest`=null で書いてから注入する（段を記録に進めるのは送った周だけ）。
  - **打ち切りの後**: 毎周 digest を測り、変化すれば段 0 へ戻る。次の合図は黙りの門を通ってから（席がまた 40 分黙ってから）で、床は段 0 の待ち（`sent_at` から 40 分・とうに過ぎている）を通る。
  - 席の応答は Stop の打刻で測る（settle）。合図に応えた turn の打刻（UserPromptSubmit と Stop）は基準を作る側で、「変化」には数えない（基準を送出時に取ると毎回「変化あり」になり梯子が登らない・ADR-0058 CTX3）。
- 形 3 **打刻の合図**: 文面は 1 行 `<NAME> tick: heartbeat step=<段> — 台帳の現在地（bd --readonly ready --limit 0）から続きを進める（変化が無ければ次の合図は <次の段の待ち> 秒後・上限で打ち切り）`。器自身の目印は先頭の `<NAME> tick:`（`InjectionRecord` の `what` は先頭 80 byte＝OwnQueued の照合はこの頭で当たる）。文面の正本は code の 1 定数（規則は持たない・散文の指示は載せない・N2）。席の側の応答は通常の turn（台帳を読んで続きを進める）で、器は応答の中身を読まない。
- 形 4 **口座の門**（FR27・FR38 の「tick は測らない」）: 登録 row の `account` label について `fresh_rows`（鮮度の内側の最新の回だけ・**計測は起こさない**）を読み、`pressed`（`Caps` は FR38 の閾値の rules 行 3 本）が Some なら `account-pressed`。記録が無い・鮮度の外・読めない周は通す（門は正の証拠でだけ閉じる）。閾値の行が読めない周は `no-rule`（error）。歯は tick の周に偽 client の呼出が 0 件であることを pin する（AC18「計測の起動が 0 件」）。
- 形 5 **rules 行 4 本**（C1・値は manifest・kind は `RuleKind` の variant 4 つを `ALL` の末尾に宣言順で足す・裁定 id = user 2026-09-23T14:02Z・ruled_at 2026-09-23・値の出所は 2026-09-17T00:55Z の裁定〔台帳 `s2-07l.423`〕）: `seat.tick_interval_s`（60・timer の周期・行 b が読む）/ `seat.tick_stale_s`（2400・初段の待ち・黙りの閾値・Busy の古さの 3 役）/ `seat.pointer_backoff_factor`（2）/ `seat.pointer_backoff_max_s`（86400）。読めない周は `no-rule` で断る（既定値に倒さない）。`rules_external_form` の rows= は 68 に、kinds= は 66 になる（base は rows=64 kinds=62・設計の起草の後に s2-07l.585 が 1 行 1 kind を足した）。**base の母集団と末尾を測る歯の書き換え**（`tests/e2e/rules.rs`・便 223004Z の審査の根）: `rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all` は `ALL` を逆順に 3 つ取った並び（絶対の末尾）が `LedgerDeniedWrites` / `HostGuardDeniedCommands` / `HostGuardRmProtected` であることを測るので、4 kind を末尾に足すと赤になる＝名を変えずに「`LedgerDeniedWrites` の位置から 3 つが同じ 3 kind で、その直後に足す 4 kind が宣言順で続いて `ALL` が終わる（逆順に 4 つ取った並びが足す 4 kind）」を測る形に書き換える。`rules_embedded_manifest_declares_one_capability_row_per_role` の kind の母集団 62 は 66 に、`rules_embedded_manifest_is_valid_and_covers_all_kinds` の埋め込み manifest の行数 64 は 68 に書き換える（assert の文言の内訳に本便の +4 を足す）。`rules_review_same_kind_stop_kind_is_last_in_declaration_order_and_paired_with_the_row` は `ReviewSameKindStop` の位置から 3 つの並びと `RoleEffort` の直後だけを測る相対の順なので書き換えない（末尾に足しても緑のまま・名の last は assert に無い）。
- 形 6 **判定行と記録**: 判定行は stdout 1 行 `decision=<inject|noop|error> target=<潰した target> reason=<語|-> pointer=<sent|settling|wait:<残り秒>|stopped|-> step=<段|-> consumed=<true|false|unknown[:理由]|->`。`pointer=` と `step=` は梯子を評価した周（形 1 の 3 以後）に載り、それより前で止まった周は `-`（評価していない印・0 に化けない・C10）。梯子を評価した周の `pointer=` は梯子の評価そのもの: `settling` の周は `step=` に記録の段・`record-unreadable` の周は評価できないので `-`／黙りの門・口座の門・入力欄の門・記録の書きで止まった周は、上限を超えれば `stopped`・床の内なら `wait:<残り秒>`・床を過ぎていれば残り 0 秒の `wait:0`（`sent` は送った周だけ）で、`step=` は段の候補。`consumed=` は注入した周だけ（`Settled` の既存の字面）。注入の記録は `deliver_within` が `tick.jsonl` に書く従来の 1 行（`who`=`seat-inject`・tick 専用の `who` は足さない＝`last_own_payload` の照合が同じ 1 本で効く）。
- 形 7 **使い方と不在の歯の書き換え**: `seat` の使い方の 1 行に `tick --state-dir S --target S:W [--rules F]` を足し（`seat_usage_external_form` の snapshot を同じ便で更新）、不在の歯 3 本は `tick` を「在る側」に移し（`meter` / `heartbeat` / `cycle` / `inject` / `externalize` / `rebrief` / `consume` は不在のまま）、「席の口は `--rules` を 1 つも受けない」の assert を「`tick` だけが受ける」に書き換える。実装は行 a の write-set の `+` の file（seat 配下の新 module）が持ち、`crates/scribe2/src/seat/mod.rs` は `pub mod` 1 行、`cli.rs` は dispatch の 1 arm と flag の許容列 1 つ。
- 触らない: 状態の打刻の書き手（hook）・`InjectionRecord` の schema・`deliver_within` / `pass_input` / `guard_input` の中身・dispatcher の通知の経路と `pipe.stop_grace_ms` の値・群の逼迫の通知と移動（ADR-0055）・便の起動の契機（FR68）・`fleet usage` の口・極性一覧（tick は境界の行を足さない）・event の種類の閉じた一覧（tick は event を記さない・記録は席 dir の file と `tick.jsonl` だけ）。
- 却下: 席（AI）に「変化が無ければ heartbeat を打たない」と判断させる（席を起こすこと自体が消費・自由文入力・C3.3）／固定の「N 回無変化で中断」（席が応えない周に永久停止しうる・上限で必ず打ち切る梯子の方が両端を機械で守れる）／digest に fleet の event log を含める（他の席の便の終端で digest が動き無関係の変化で梯子が戻る・ADR-0058 v2）／tick-stamp の file を戻す（床の出所は梯子の記録の `sent_at` の 1 つで足りる・C17.2）／dispatcher の周に相乗りする（黙った席では便も流れず契機が来ない・ADR-0058 OPT2）／合図の窓の rules 行を新設する（dispatcher の通知と同じ行で足りる）。
- 歯（`tests/e2e/seat.rs` に `seat_tick_` 接頭辞・fixture は PATH の偽 tmux〔pane を file で持ち `capture-pane` / `send-keys` の呼出を 1 行ずつ file に残す＝送った key を呼出の行で数える・tmux の server を立てない＝nextest の tmux の group の外〕 + 席 dir の `state.jsonl` を手で書く + rules の写し〔`--rules`〕・時刻は記録の `sent_at` と打刻の `ts` を過去に書いて進める〔偽の時計〕・偽 client は PATH の script が呼出を file に残す）:
  - (a) 登録 row ∧ 最終行 Idle で 40 分前 ∧ 入力欄が空 → `decision=inject … pointer=sent step=0`・pane に合図 1 行・`pointer-ladder` 1 行（`step`=0・`digest`=null）・`tick.jsonl` に `who`=`seat-inject` 1 行。
  - (b) 登録 row 無し → `no-row`／打刻無し → `state-missing`／読めない → `state-unreadable`／最終行 Busy → `busy`／Busy が 40 分より古い → `state-stale`（各 0 key・記録 0）。
  - (c) 最終行 Idle が 40 分未満 → `stamp-recent`・0 key。
  - (d) settle と梯子: 送った後に `sent_at` より後の Stop を 1 行足すと次の周で記録に `digest` が入り `settling` を抜ける／無変化の周は `pointer=wait:<s> step=1` で 0 key／`sent_at` を待ちの分だけ過去に書くと `inject … step=1`／段 5 まで合図 6 本・段 6 は `pointer=stopped step=6` で 0 key（打ち切り）。
  - (e) 変化: 基準確定の後に最終行の `ts` を進める（Stop）と段 0 へ戻り、最終行から 40 分経った周に `inject … step=0`。打ち切りの後も同じ（stopped → 変化 → 40 分 → inject）。
  - (f) 応えない席: `sent_at` から 40 分過ぎても Stop が無い周はその周の digest で基準が入り、次の周は段 + 1。
  - (g) 口座の門: 鮮度の内側の記録が閾値以上 → `account-pressed`・0 key／記録無し・鮮度の外 → 通る／どの周も偽 client の呼出 0 件。
  - (h) 入力欄に人の文字 → `input-busy`／prompt 行が無い pane → `input-unknown`／自席の前の合図が残る → Enter 1 回の後に `input-own-queued`（text の再送 0）。`input-busy` と `input-unknown` は 0 key、`input-own-queued` は `pass_input` が送る Enter の 1 key だけで合図の text は 0 key（`pass_input` は触らない側・便 220535Z の審査の根: 「各 0 key」は Enter を数えない空虚な歯になる）。どの周も記録は増えない。
  - (i) 記録が dir（読めない）→ `record-unreadable`／席 dir が読み取り専用 → `record-unwritable`（0 key・pane 不変）。
  - (j) rules: 4 行が埋め込み manifest に値と裁定 id つきで在り `RuleKind` の `ALL` と `rules validate` の外形に載る（`tests/e2e/rules.rs` に `rules_embedded_manifest_declares_tick_` 接頭辞・kind の対の parity は既存の `rules_kind_parity_every_kind_has_sample` が数える）／`--rules` で 4 行を欠く写し → `decision=error reason=no-rule` rc 1・0 key／`rules_external_form` の snapshot（rows=68 kinds=66）／base の母集団と末尾を測る歯 3 本の書き換え（形 5・名は変えない）と、相対の順だけを測る 1 本が書き換えずに緑のまま在ること。
  - (k) 使い方: `seat_usage_external_form` の snapshot と不在の歯 3 本の書き換え（形 7）。不在の歯 3 本（`seat_autonomy_subcommands_are_gone_from_the_usage` / `seat_inject_subcommand_is_gone_from_the_usage` / `seat_working_memory_subcommands_are_gone_from_the_usage`）は名を変えずに tick を在る側・`--rules` は tick だけが受ける形へ書き換える＝行 a の verify の filter 語 `gone_from_the_usage` が書き換えた 3 本を名指す（便 215536Z の審査の根: done (7) の書き換えを名指す verify 行が無かった）。
  - lib（行 a の `+` の file の中・`seat_tick_` 接頭辞）: 段 → 待ちの pure 関数（飽和・上限の判定）・段の候補（変化 / 同じ / 記録なし）・記録の 1 行の round-trip・理由の `as_str` が宣言順で重複しない。

## 3. tick の unit を器が導出して書く（契約表の行 b・ADR-0030 §2.1〜§2.5・FR64）

- 口: `seat tick install --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]` と `seat tick uninstall --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]`（撤去も同じ引数で導出し直して比べる）。置き場・binary・unit dir は全部引数（器は env・home・`current_exe` を読まない・C2.2）。母集団は登録 row（row の無い target は `no-row` で断る・FR40）。
- 導出（pure な 1 関数・入力 = NAME・target・置き場・binary・rules の写し・周期）: file 名は `<NAME>-seat-tick-<潰した target>.service` / `.timer`（潰し方は席 dir と同じ 1 関数＝`crates/scribe2/src/seat/mod.rs` の pub の `sanitize_target`〔触らない〕・template unit と `%i` は使わない）。service = `[Unit] Description=` + `[Service] Type=oneshot` + `ExecStart=<binary> seat tick --state-dir <S> --target <S:W>`（`--rules` を受けた周だけ末尾に `--rules <F>`）。timer = `[Timer] OnBootSec=<n>s` + `OnUnitActiveSec=<n>s` + `Persistent=false` + `[Install] WantedBy=timers.target`（n = `seat.tick_interval_s`・単調時計・`OnCalendar` は使わない）。`Environment=` / `WorkingDirectory=` / `%h` を持たない。2 file の先頭行に器の印（`# <NAME> tick-install schema=1` の 1 行）を置く。外形は snapshot で pin する（C12.5）。
- 書き: 一時 file → rename。既存 file は導出の bytes と比べ、一致 → `unchanged`（有効化だけ撃つ）・不一致 → `unit-exists` で断る（人の手書きを上書きしない・N1）。有効化 = 子 process `systemctl --user daemon-reload` → `systemctl --user enable --now <timer>`（順序固定・`systemctl` の綴りは `crates/scribe2/src/pipe/confine.rs` の定数を pub(crate) にして共有・撃つ口は 1 本・失敗は `reload-failed` / `enable-failed` に rc を添える）。2 file とも一致の周は書かないので `daemon-reload` を撃たず `enable --now` だけ。記録は `tick.jsonl` に `crates/scribe2/src/hook/mod.rs` の `InjectionRecord`（`who` / `what` は文字列の欄・閉じた列ではない）を 1 行書く（`crates/scribe2/src/seat/cycle/relaunch.rs` の再起動の記録と同じ形＝`inject::tick_path` の file へ `store::append_line`・同 file の `append` は置き場の `inject.jsonl` を書く口なので使わない・`who`=`seat-tick-install`・`what`=timer の unit 名・hook/mod.rs は触らない）。
- 結果の行（実装は行 b の write-set の `+` の file）: 成功は stdout に `seat tick <install|uninstall>: <installed|unchanged|retired|absent> timer=<unit 名> …`（rc 0・`absent` は撤去で 2 file とも無い周＝何も撃たない）、断りは stderr に `seat tick <verb>: refused reason=<語> [unit=<file 名>|rc=<rc>] target=<S:W>`（rc 1）。理由の語は閉じた列 `path` / `no-rule` / `store` / `no-row` / `unit-exists` / `unit-foreign` / `unit-unwritable` / `reload-failed` / `enable-failed` / `disable-failed`。門の順は rules（周期の行）→ 置き場の replay → 登録 row → 導出 → 2 file の照合（1 つでも断れば 1 file も書かず `systemctl` も撃たない）。`ExecStart=` の語は `%` を `%%` にし、空白と引用符を含む path は二重引用符で包む（specifier と語の割れを防ぐ）。
- 撤去: `systemctl --user disable --now <timer>` → 2 file を `<unit dir>/.retired/<name>.<UTC 秒>` へ mv（N1.2・削除しない）。器の印の無い file は `unit-foreign` で断り、印は在るが同じ引数で導出し直した bytes と違う file は `unit-exists` と同じ理由で断る（動かさない・比べる bytes の出所は記録でなく導出＝記録は `who` / `what` のまま）。
- doctor: `doctor --state-dir S --unit-dir U --binary PATH [--rules F]` の周だけ、登録 row の 1 行ごとに `tick-unit=<present|absent|foreign>` を足す（`present` = 2 file が在り印と bytes が同じ引数の導出と一致・`foreign` = 在るが印が無いか bytes が違う・`absent` = 無い・片方だけ在る周は `foreign`・`systemctl` は呼ばない）。周期の行が読めない周は導出できないので 3 値のどれも名乗らず `tick-unit=no-rule:<理由>`（在るとも無いとも書かない・C10）。`--unit-dir` 無しは項目を足さず、登録 row の行は 1 byte も変わらない（評価していない印は持たない＝「触らない」の doctor の既存の行の形と同じ）。`--unit-dir` と `--binary` の flag は `crates/scribe2-boundary/src/main.rs` の doctor の flag の列に足す（値欠け・重複・`--unit-dir` だけで `--binary` が無い周は使い方の誤り）。
- 承認: 有効化・撤去は 3 クラス（消す / 出す / 使う）のいずれにも当たらない（ADR-0030 §2.4）＝器が撃つ。人が手で置いた unit は器の管理物でない（`unit-foreign`）。
- 触らない: tick の判定の列（§2）・`seat.tick_interval_s` 以外の rules 行・`confine.rs` の systemd scope の中身（定数の可視性だけ）・`doctor` の既存の行の形（`--unit-dir` 無しの外形 snapshot は 1 byte も動かない）。
- 却下: template unit と `%i`（target の潰し方が unit 名の規則と二重になる）／雛形 file を repo に置いて写す（host 固有の値が PUBLIC repo の tracked に入る・ADR-0030 §5 (A)）／周期を引数の既定値で持つ（規則が code に散る・C1）／`systemctl` の結果で unit の有無を判じる（doctor は子 process を起こさない・bytes で判じる）／記録（`tick.jsonl`）に導出の bytes の sha を持って撤去と doctor がそれと比べる（record の形が変わる = on-disk 形式の変更で ADR が要る・導出が pure なら同じ引数で導出し直せば足りる）。
- 歯（`tests/e2e/seat.rs` に `seat_unit_` 接頭辞・unit dir と binary は tmp・`systemctl` は PATH の偽 script が引数を file に残す）:
  - (a) install → 2 file の bytes が導出と一致（snapshot）・`Environment` / `WorkingDirectory` / `%h` を含まない・timer の `OnUnitActiveSec` が rules 行の値・偽 systemctl の呼出が `daemon-reload` → `enable --now <timer>` の順で 2 回・`tick.jsonl` に `who`=`seat-tick-install` 1 行・rc 0。
  - (b) 同じ bytes で再 install → `unchanged`・file の mtime 不変・enable だけ 1 回／1 byte 違う file を置いて install → `unit-exists`・file 不変・systemctl 0 回・rc 1／登録 row 無し → `no-row`・file 0・rc 1／rules の写しに `seat.tick_interval_s` が無い → `no-rule`・file 0。
  - (c) uninstall（install と同じ `--binary` / `--rules`）→ `disable --now` 1 回 → 2 file が `.retired/` に同じ bytes で在り元の場所に無い／印の無い file → `unit-foreign`・動かない／bytes 違い（導出し直した bytes と 1 byte 違う file）→ 断り・動かない／`--binary` を欠く → 使い方の誤り・動かない。
  - (d) doctor `--unit-dir` + `--binary`: install 後 `tick-unit=present`・撤去後 `absent`・印の無い file を置いて `foreign`・`--unit-dir` だけで `--binary` 無しは使い方の誤り・`--unit-dir` 無しは項目が無く既存の外形 snapshot（`seat_doctor_external_form`）が 1 byte も動かない（(d) の歯は `tests/e2e/seat.rs` に置き、`seat/` の sub-file は触らない）。
  - (e) 使い方の 1 行に `tick install …` / `tick uninstall …` が増え `seat_usage_external_form` が動く。
  - lib（行 b の `+` の file の中・`seat_unit_` 接頭辞）: 導出の pure 関数の出力を fixture の逐語との `assert_eq` で測る（insta の snapshot は使わない＝`crates/scribe2/src` に snapshot file は 0 のまま・dev-dependency は増えない）・印の判定（在る / 無い / bytes 違い）の 3 値。
- write-set の注: 行 a が `+` で足す seat 配下の file は、行 a の着地前は base に無いので本行も `+` で宣言する。行 a の着地後に素の path へ直す（受付は着地済みの file の `+` を断る）。

## 4. tick が群の移動の続きを撃つ（契約表の行 c・§2 の判定の列に移動の門を足す・ADR-0058 §2・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html) §2・[ADR-0055](../../design-intent/decisions/ADR-0055-group-pressure-is-measured-at-run-ends-and-seat-turns-without-a-timer.html) OPT1 の「席自身の仕組み」の側・`s2-07l.616`）

やさしく言うと: 群の移動（[account-lifecycle.md](./account-lifecycle.md) §20〜§22）の続き（古い席に /exit を送り、shell に戻った窓に新しい口座の席を起こす）は dispatch の 1 周でしか撃たれず、周は便の終端か手動でしか起きない。移動が決まった後に便も周も無ければ、席は「移動中」のまま何時間でも止まる。さらに群の段は自分の置き場の登録しか読まないので、同じ群でも別の置き場に登録された席（別 project の席）は退避の対象にならない。管理 tick（§2）は席ごとに周期で回り、席の状態の打刻と入力欄の門を既に持つ。tick が「自席の登録 row の口座 ≠ 群の記録の口座」の周に、自席の退避と起こし直しを 1 手ずつ撃てば、どの置き場の席も、便が無くても、移る。

- 出所: 台帳 `s2-07l.616`（memo・2026-09-24 の実測: 移動が決まった後、退避の対象は列の置き場の席 1 つだけ・その席への /exit は 2 回とも席が作業中の周に当たり input-unknown で保留・以後 8 時間 event 0 件のまま）と user 裁定 2026-09-24T22:09Z（逐語は同じ memo の本文・heartbeat で解く）。
- 現物（verified・main 564b4cd）:
  - tick の判定の列は `crates/scribe2/src/seat/tick.rs` の `judge`（`front` = 登録 row → 状態の打刻 → 梯子・`back` = 黙りの門 → 上限 → 床 → 口座の門 → 入力欄の門 → 記録 → 注入）。群は読まない（§2 の「触らない: 群の逼迫の通知と移動」）。判定は閉じた enum `TickDecision`（Inject / Noop / Error）・理由は `NoopReason`（`as_str` の宣言順の列 `NOOP_REASONS`）・判定行は `render`。
  - 群の記録の解決は `crates/scribe2/src/hook/group.rs` の `current_of`（記録 > 種・読めなければ `RecordError`）・anchor の群は `group_of`・記録の置き場は `crates/scribe2/src/seat/mod.rs` の `host_groups_dir`（置き場の親の下＝置き場を跨いで同じ 1 file）。tick の manifest は tracked の面だけで渡る（`seat tick` の口は `--rules` か埋め込み）ので、移動の門が `crates/scribe2/src/rules/mod.rs` の `with_state_dir` で host の面を合わせてから群の宣言と口座を読む（host の面が読めない周は `group-unreadable`・面が無い host は群 0 のまま）。
  - 群の段の lock・/exit の字面・dialog の既定の行と Enter の記録の語・起こし直しは `crates/scribe2/src/pipe/dispatch/group.rs`（`Lock`・`EXIT`・`EXIT_DIALOG`・`relaunch`）で全部 private。`behind` は列に渡された置き場の登録 row だけを読む＝別の置き場の席は対象外。
  - 席の起動は `crates/scribe2/src/seat/cycle/launch.rs` の `launch`（`Launch` の引数に target・anchor・account・state_dir・settle / step・manifest・rules）。settle / step は `crates/scribe2/src/seat/cycle.rs` の `pace_of`。
  - 登録 row（`crates/scribe2/src/seat/role.rs` の `Registration`）は anchor と account を持つ＝tick は自席の anchor と口座を知っている。tick の unit は §3 が置き場と target ごとに書く＝tick は自分の置き場も知っている。
- 形（1 つずつ歯が測る・行 c の done と 1:1）:
  1. **移動の門の位置**: `front`（登録 row・打刻が Idle・梯子）の直後、黙りの門の前。登録 row の anchor が群に属し（`group_of`）、群の今の口座（`current_of`）が row の口座と違う周は**移動の周**: 以後の列（黙り・上限・床・口座の門・合図の注入）は撃たず、梯子の記録も触らない（移動中の席に heartbeat を送らない）。群に属さない anchor・群 0 の host・記録と row が一致する席（記録が無く種と一致する席を含む）は今の列のまま 1 字も変わらない。記録が在るのに読めない周は `noop` の `group-unreadable`（種に読み替えない・C10）。
  2. **lock**: 移動の周は群の段と同じ lock（host の群用 dir の 1 file・`create_new`）の内側で撃つ。取れない周は `noop` の `group-locked`（1 key も送らない＝dispatch の 1 周と同じ target を二重に起こさない）。lock の実装は 1 本を共有する（`crates/scribe2/src/pipe/dispatch/group.rs` の `Lock` を記録の読み手と同じ `crates/scribe2/src/hook/group.rs` へ移して群の段と tick が呼ぶ・二重に書かない・C17）。
  3. **pane が shell の周は起こす**: `pane_is_shell` の周は `launch` の 1 本で同じ target に群の今の口座の席を起こす（anchor と置き場は自分の row と自分の置き場・settle / step は rules・登録 row は起動が書き直す・会話は運ばない＝§20 形 6 と同じ 1 本。行 f 以後は §7 形 3 のとおり打刻の sid を `--resume` で運ぶ）。判定行は `decision=move move=launch launched=<起動の結果の語>`。起こせない周（断り・失敗・候補なし）も語を載せて次の周にまた判じる（冪等・保留の event は tick が記さない）。
  4. **pane が shell でない周は退避を 1 手**: 入力欄の門（§2 形 1 の 8 と同じ `pass_input`）を通し、空なら `/exit` の 1 行を `deliver_within`（窓は `pipe.stop_grace_ms`）で送る（`decision=move move=exit`・記録は `tick.jsonl` に `who` が `seat-tick-move`・`what` が `/exit` の 1 行・送達が未確認でも残す＝account-lifecycle.md §22 形 1 と同じ）。門が Foreign で、その tail（畳んだ字面）が dialog の既定の行の literal（§22 形 2 の `1. Exit and stop tasks`）に等しい周は /exit を送らず Enter を 1 回だけ（`move=enter`・`what` は `enter:exit-dialog`・Enter は消費の証拠を持たないので `consumed=unknown:exit-dialog`）。送りは `crates/scribe2/src/seat/inject.rs` の `deliver_or_confirm` の 1 本（`deliver_within` と同じ門・送り・settle）で撃つ。それ以外の Foreign / UnknownInput / OwnQueued は今の語（`input-busy` / `input-unknown` / `input-own-queued`）で 0 key（OwnQueued の Enter 1 回は `pass_input` のまま）。Busy の打刻は `front` で止まる（作業中の席に /exit を送らない＝§20 形 6 の「作業記憶を残す番」は打刻で守る）。
  5. **/exit と dialog の字面は 1 か所**: `EXIT` / `EXIT_DIALOG` の値は `crates/scribe2/src/hook/group.rs` へ移し、群の段と tick が同じ値を読む（記録の `who` は呼び手ごと）。
  6. **群の段（dispatch の周）は残る**: `relaunch` の続きの周（`Wait::Once`）は今のまま（便の終端でも進む・同じ lock で排他）。§20 形 6 の移動の周（合図 → settle → 起動）も不変。同じ target を 2 つの手が同じ周に撃つことは lock が防ぐ。
  7. **event は記さない**（§2 の「tick は event を記さない」のまま）: 移動の周の記録は `tick.jsonl` と判定行だけ。`GroupMovePending` は群の段が記す側のまま。
  8. **判定行の形**: `decision=move` の周は `reason=-`・`pointer=-`・`step=-`（梯子を評価していない印・C10）・`move=<launch|exit|enter>`・`launched=<語|->`（`move=launch` の周だけ語）・`consumed=` は送りの結果（§2 形 6 と同じ語・`move=launch` の周は `-`）。`decision=inject|noop|error` の周は `move=-` `launched=-` を末尾に足す（列は固定・省かない）。`NoopReason` に `group-unreadable` / `group-locked` の 2 値を宣言順の末尾に足す。`TickDecision` に閉じた 3 値の手（launch / exit / enter）を持つ `Move` を足す（bool で持たない・C11）。起こした周の `launched=` は起こせた周が `done`・他は起動の断り・失敗の理由の語。
  9. **群 0 の host・群に属さない anchor・記録と一致する席は 1 字も変わらない**（§2 の歯は判定行の末尾 2 欄以外そのまま）。
- 触らない: 梯子（形 2）・合図の文面・rules 行（行は足さない: settle / step は `seat.cycle_settle_s` / `seat.cycle_step_s`、窓は `pipe.stop_grace_ms`）・`pass_input` / `input_tail`・群の判定（移り先の 3 条件・記録の形・移動を頼む記録）・§3 の install・hooks.json・event の種類の列・`Launched` の variant。
- 却下: 席自身の hook（UserPromptSubmit / SessionStart）が続きを撃つ（hook の席は作業中＝入力欄の門を通らない・他席を撃たせると群の段が 2 系統になる）／dispatch の 1 周を timer で撃つ（ADR-0055 OPT2 の据え置き・tick が既に周期を持つ＝C17.2 で足すものが無い）／群の段が別の置き場の登録を読んで起こす（置き場ごとに tick が居るので要らない・列に無い置き場の席を列が起こすと登録 row の書き手が置き場の外に増える）／移動の周にも heartbeat の合図を送る（席は退避中・合図は消費）／tick が移り先を決める（判定は群の段の 1 回・ADR-0055 OPT3 の殺到の柵）／Busy の席に /exit（作業記憶が残らない・N1）／process を kill（§20 の却下のまま）。
- 歯（`tests/e2e/seat.rs` に `seat_tick_move_` 接頭辞・§2 の fixture〔PATH の偽 tmux・手書きの `state.jsonl`・`--rules` の写し〕に、偽 tmux の `list-panes` の `pane_current_command` と可視域の字面を作り分ける口〔`tests/e2e/pipe/dispatch.rs` の群の fixture と同じ形〕と host の群用 dir の記録〔置き場の親の下・`tests/e2e/hook.rs` の `hook_group_current_` の fixture と同じ形〕を足す。settle は `--rules` の写しで 1 秒に縮める）:
  - (a) 登録 row（口座 A・anchor は群 g）∧ 記録は口座 B ∧ 最終行 Idle ∧ pane が claude ∧ 入力欄が空 → `decision=move move=exit`・`send-keys` に `/exit` の payload 1 行・`tick.jsonl` に `who` が `seat-tick-move` で `what` が `/exit` の 1 行・pointer-ladder は書かれない・合図の text は 0 key（base では合図の注入か `stamp-recent` ＝ RED）。
  - (b) 同じ席で pane の最後の `❯` 行が dialog の既定の行 → Enter 1 key・`/exit` 0・`what` が `enter:exit-dialog`／tail が別の字面 → `input-busy`・0 key／prompt 行なし → `input-unknown`・0 key（記録 0）。
  - (c) pane が shell → `move=launch`・`send-keys` に起動行 1 行（口座 B の設定 dir を持つ）・fleet に口座 B の登録 row が 1 件増える・/exit 0（settle 1 秒で `launched=` に未確認の語）。
  - (d) 最終行 Busy → `busy`・0 key（移動の門より前で止まる）／記録が dir（読めない）→ `group-unreadable`・0 key／lock の file が在る → `group-locked`・0 key・記録 0・起動行 0。
  - (e) 記録の口座 = row の口座（移動済み）／群に属さない anchor／記録なしで種 = row → §2 の列のまま（`inject` か `stamp-recent`・`move=-`）。
  - (f) 親を共有する 2 つの置き場に 1 席ずつ・記録は親の下の 1 file → 両方の tick が `move=exit`（置き場を跨いで同じ記録を読む＝別 project の席も移る）。
  - lib（`seat_tick_move_` 接頭辞・`crates/scribe2/src/seat/tick.rs` の中）: `NOOP_REASONS` が宣言順で重複しない（既存の歯の母集団が 2 増える）・判定行の `render` に `move=` / `launched=` が載る周と `-` の周。
- 後続: 席の起動が tick の unit を入れる（unit dir と binary は host の面が持つ＝面の表が増えるので ADR・別の行）／移動の周に席の hook の 1 行を「tick が /exit を送る」に揃える（字面だけ・account-lifecycle.md §21 形 2 (a)）。

## 5. 席の起動が tick の unit を入れる（契約表の行 d・host の面の表 `[[tick]]`・§3 の続き・ADR-0064・[ADR-0030](../../design-intent/decisions/ADR-0030-tick-units-are-written-and-enabled-by-the-vessel.html) §2・`s2-07l.616`）

やさしく言うと: §3 の `seat tick install` は unit dir と binary の場所を引数で受ける（器は env・home・自分の実行 file の場所を読まない）。だから今は人が席ごとに 1 回打たないと tick は動かず、打ち忘れた席は黙ったままになる（2026-09-24 の実測: この host に器の unit は 1 本も無かった）。unit dir と binary の場所は host に 1 組しか無いので、host の面（`host.toml`）に 1 行で宣言し、席を起こす口がその宣言で unit を入れる。人が打つ command は増えない（`host init` の雛形から `init` が写す）。

- 出所: 台帳 `s2-07l.616`（候補 2）と §4 の後続。同じ日の実測: 人の手書きの template unit が別の引数で落ち続けていた（器の unit は 1 本も無い）。
- 現物（verified・main 65419fb）:
  - host の面の読み手は `crates/scribe2/src/rules/manifest.rs` の `HostManifest`（`[[account]]` / `[[plugin]]` / `[[launch-arg]]` / `[[vessel]]` / `[[account-group]]`）。`[[vessel]]` は `VesselRepo`（`repo` の 1 欄・最大 1 行）＝path 1 つを持つ 0 か 1 行の表の先例。`Manifest` は `vessel: Option<VesselRepo>` で持つ。
  - `seat tick install` は `crates/scribe2/src/seat/tick/install.rs` の `run`（`Flags` = `--state-dir` / `--target` / `--unit-dir` / `--binary` / `--rules`）。導出は pure の `derive`・書きと有効化は `install`（`daemon-reload` → `enable --now`）。unit dir と binary は引数だけ（C2.2）。
  - 席の起動は `crates/scribe2/src/seat/cycle/launch.rs` の `launch`（`prepare` が登録 row を書く → 起動行を注入 → 立ち上がりを確かめて `Launched::Done`）。短い形と長い形の呼び手は `crates/scribe2/src/seat/cli.rs`（`render_launched` の 1 行）。
  - doctor の `tick-unit=` は `--unit-dir` と `--binary` を渡した周だけ足す（`crates/scribe2-boundary/src/main.rs` の flag の読み・§3）。
  - doctor の host の面の行は `crates/scribe2/src/account/mod.rs` の `render_host_manifest`（`host-manifest=<present|absent|unreadable>` の 1 行・読むだけ）が描き、呼び手は `crates/scribe2-boundary/src/main.rs` の doctor（flag を読んで行を組む）と `crates/scribe2/src/account/mod.rs` の口座の行の組み立て（`render_host_manifest` を先頭に置く）の 2 つ。形 3 の `tick=declared` はこの行の末尾の欄＝`render_host_manifest` に欄を足し、呼び手 2 つが面の有無を渡す（`crates/scribe2/src/account/mod.rs` は行 d の write-set）。
  - `init`（host-init.md §4 段 3）は雛形の host の面の 4 表（`[[plugin]]` `[[launch-arg]]` `[[account]]` `[[vessel]]`）を写す（行 b・未着地）。
- 形（1 つずつ歯が測る・行 d の done と 1:1）:
  1. **host の面の表 `[[tick]]`**: 欄は `unit-dir`（unit を置く dir の絶対 path）と `binary`（unit が撃つ器の絶対 path）の 2 つ・0 か 1 行（2 行目は loader が断る・`[[vessel]]` と同じ形）・相対 path と欠けた欄は loader が行番号つきで断る。`Manifest` は `tick: Option<TickUnit>` で持ち、読み手は `tick()` の 1 つ。表の無い host は今のまま（群 0 と同じく 1 語も変わらない）。tracked の manifest に置いた表は `[[account-group]]` と同じく 1 表 1 件で断る（unit の置き場と binary は host 固有の path・CON2）。
  2. **席の起動が入れる**: `launch` が `Launched::Done` を返す周（登録 row を書き、立ち上がりを確かめた後）に、面に `[[tick]]` が在れば §3 の install と同じ 1 本（導出 → 照合 → 書き → `daemon-reload` → `enable --now`）を、置き場 = 起動の置き場・target = 起動の target・unit dir と binary = 面の値・rules = 起動に渡された写し（無ければ埋め込み）で撃つ。結果は起動の 1 行の末尾（置き場の 2 語の後ろ）に `tick-unit=<installed|unchanged|refused:<理由の語>>` を足す（表の在る host だけ・表の無い host の起動の行は 1 字も変わらない）。install の断り・失敗は起動の rc を変えない（席は立っている・doctor が名指す）。`Refused` / `Failed` / `None` の周は撃たない（席が立っていない）。短い形と長い形の両方（呼び手は 1 か所）。
  3. **doctor は面の値を既定にする**: `--unit-dir` / `--binary` が無く面に `[[tick]]` が在る周は面の値で §3 の `tick-unit=` を足す。flag が在れば flag が勝つ。面にも flag にも無い周は今のまま項目を足さない（既存の外形 snapshot は 1 byte も動かない）。表の在る host だけ doctor の host の行に `tick=declared` を 1 項目足す（値は書かない・表の無い host は既存の外形 snapshot が 1 byte も動かない）。
  4. **`init` が写す**: host-init.md §4 段 3 の「4 表」は `[[tick]]` を含む 5 表になる（雛形に在れば写し、無ければ写さない・行 b の done (3) に `[[tick]]` を足す）。`host init` の雛形は人が 1 度だけ `[[tick]]` を書く（machine-local の面・PUBLIC repo には入らない）。
  5. **撤去は今のまま**: 席を退役する口は無い（account-lifecycle.md §20 の後続）ので、unit の撤去は §3 の `seat tick uninstall` のまま（本行は足さない）。
  6. **群 0 の host・表の無い host・§2 / §4 の判定の列は 1 字も変わらない。**
- 触らない: §3 の導出（bytes は同じ引数で同じ）・unit の file 名・`seat tick install` / `uninstall` の口と引数・§2 / §4 の tick の判定・`[[vessel]]` 以下の既存の表の形・hooks.json・rules 行（周期は `seat.tick_interval_s` のまま・行は足さない）。
- 却下: env の `HOME` / `XDG_CONFIG_HOME` から unit dir を解く（C2.2・器は env と home を読まない）／`current_exe` で binary を解く（C2.2）／install 帳簿の `InstallRecorded` の path を binary にする（記録は scribe2 の置き場にしか無く、他の置き場の席が読めない）／`systemd-path` や `systemd-analyze unit-paths` の子 process で unit dir を解く（spawn の口が増え、env を子に読ませるだけ）／template unit と `%i`（§3 の却下のまま）／`init` が unit を入れる（席を起こすのは `seat launch` の 1 本・入れる場所は起動の直後の 1 か所に置く）／起動の失敗にする（席は立っている・unit の欠けは doctor が名指す）。
- 歯:
  - `tests/e2e/rules.rs`（`rules_host_tick_` 接頭辞・host.toml の fixture）: 1 行の `[[tick]]` が読める（2 欄が絶対 path）／2 行は断る／相対 path は行番号つきで断る／欄の欠けは断る／表の無い host は既存の外形 snapshot が 1 byte も動かない（base では表が未知の key で断られる ＝ RED）。
  - `tests/e2e/seat/launch.rs`（`seat_launch_tick_` 接頭辞・§3 の fixture〔PATH の偽 systemctl・unit dir は tmp〕を起動の歯に載せる）: `[[tick]]` の在る面で起動 → 2 file が導出の bytes で在り・偽 systemctl が `daemon-reload` → `enable --now` の順・行の末尾に `tick-unit=installed`／同じ席を起動し直す → `unchanged`・file の mtime 不変／表の無い面 → 行に `tick-unit=` が無く file 0・systemctl 0 回（起動の行の既存の形が 1 byte も動かない）／起動が断られる周（`not-a-shell` 等）→ file 0・systemctl 0 回／偽 systemctl が落ちる → `tick-unit=refused:enable-failed`・rc は起動の rc のまま（base では行に `tick-unit=` が無い ＝ RED）。
  - `tests/e2e/seat.rs`（`seat_doctor_tick_` 接頭辞）: flag 無し + 面に `[[tick]]` → 登録 row の行に `tick-unit=<present|absent>`・host の行に `tick=declared`／flag が面と違う値 → flag の値で判じる／面にも flag にも無し → 既存の外形 snapshot が 1 byte も動かない（`tick=` の項目は無い）。
  - lib（`crates/scribe2/src/rules/manifest.rs` の中・`host_tick_` 接頭辞）: 表の 1 行の round-trip・2 行と相対 path と欠けた欄の断りの行番号。
- 後続: 席の退役の口（登録 row の退役 + `seat tick uninstall`）／`init` の 8 段目（host-init 行 c）が起こす席は本行の形で unit が入る＝人が打つ command は増えない。

## 6. pane が shell かの判定は子 process まで見る（契約表の行 e・§4 形 3 / 4 の前提・`s2-07l.624`）

やさしく言うと: 器は「窓の前面が shell なら席は終わっている」と読む。ところが host の再起動の後、tmux の復元が席を `sh -c 'cd … && claude …'` の形で立て直すと、claude は sh の子として同じ process group に居て leader は sh のまま＝tmux の `pane_current_command` は `sh` を返す。器はこの窓を「shell」と読み、生きた席へ起動行を送ろうとして入力欄の門で `input-unknown` を毎周返す。前面が shell でも、その shell が子 process を持つ周は「席が中で動いている」と読む。

- 出所: 台帳 `s2-07l.624`（2026-09-25 の実測: 8 席のうち 6 席がこの形で起き、tick の移動が 1 席も進まなかった）。
- 現物（verified・main 503a703）:
  - 判定は `crates/scribe2/src/seat/mod.rs` の `pane_is_shell`（`list-panes -F #{pane_current_command}` の各行が `SHELLS` の語か）。呼び手は 4 つ: tick の移動の門（`crates/scribe2/src/seat/tick.rs`）・群の段の起こし直し（`crates/scribe2/src/pipe/dispatch/group.rs`）・席の起動の前提の門（`crates/scribe2/src/seat/cycle/launch.rs` の `prepare`・`not-a-shell`）・口座の墓標の判定（`crates/scribe2/src/account/mod.rs`）。
  - 偽 tmux の fixture は歯ごとに在り（`crates/scribe2-boundary/tests/e2e/seat.rs`・`crates/scribe2-boundary/tests/e2e/seat/launch.rs`・`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`・`crates/scribe2-boundary/tests/e2e/notify.rs`）、`list-panes` は前面の語だけを返す。
- 形（1 つずつ歯が測る・行 e の done と 1:1）:
  1. **子 process の有無を 1 回見る**: 前面の語が `SHELLS` に在る周だけ、`display-message -p -t <target> '#{pane_pid}'` で pane の pid を取り、`/proc/<pid>/task/<pid>/children` が空でない周は shell でない（席が中で動いている）と読む。前面が shell でない周は今のまま（2 回目の tmux を撃たない）。
  2. **pid が取れない周は今のまま**: 出力が 10 進の整数でない・`/proc` の file が読めない周は子の有無を「不明」とし、判定は前面の語だけで決める（既存の fixture が pid を返さなくても今の歯が 1 本も動かない・不明を「子が居る」に倒さない＝再起動の後の空の shell を永久に「動いている」と読まない）。
  3. **呼び手 4 つは 1 字も変わらない**（判定は 1 本・引数も戻りも同じ）。
- 触らない: `SHELLS` の語（`sh` を外さない＝人が sh で開いた窓を shell でなくしない）・入力欄の門・4 つの呼び手の断りの語。
- 却下: `SHELLS` から `sh` を外す（偽陰性）／process group の leader を `ps` で辿る（子 process を 1 本増やす・`/proc` の 1 file で足りる）／復元 script の側だけを直す（器の外・N3・別 host の復元でまた踏む）／tmux の `pane_current_command` の代わりに常に `/proc` を読む（前面が claude の周に 2 本目の呼び出しが増えるだけ）。
- 歯（`crates/scribe2-boundary/tests/e2e/seat.rs` に `seat_pane_shell_` 接頭辞・§4 の偽 tmux の fixture に `display-message` の口〔`{pid}` の file を cat〕を足し、pid は歯が起こした実 process〔`sh -c 'sleep 60; :'` の親・末尾の `:` は dash が単独の command を exec して子を持たない形を避ける〕のもの）: (a) 前面 `sh` ∧ pid の shell が子を持つ → shell でない（tick の移動の周は `move=exit`・base では `move=launch` ＝ RED）(b) 前面 `sh` ∧ 子なし → shell（今のまま `move=launch`）(c) 前面 `sh` ∧ pid の file が無い / 整数でない → shell（今のまま）(d) 前面 `claude` → tmux は 1 回だけ（`display-message` の呼び出し 0）。
- 後続: 復元 script（host 側・tracked に書かない）から claude の送りを外し、席の立ち上げは §7 の tick に任せる。

## 7. tick が死んだ席を起こし、移動の門を打刻の前に置き、起こし直しは会話を運ぶ（契約表の行 f / 行 h・§4 の改め・`s2-07l.626` / `s2-07l.628` / `s2-07l.629`）

やさしく言うと: 今の tick は「群の記録と row の口座が違う周」だけ席を起こす。再起動や /exit で窓が shell に戻った席は、口座が合っていれば誰も起こさない。さらに /exit を送ると claude の UserPromptSubmit hook が打刻を busy に残すので、tick は「作業中」と読んで移動の門に届かない。打刻より先に「窓が shell か」を見て、shell なら口座（群なら記録の口座・群の外なら row の口座）で起こし、そのとき直前の会話（打刻の sid）を `--resume` で運ぶ。持ち主の裁定 2026-09-25T03:58Z（会話を reset する意味が無い）と 04:13Z（口座の移動を含めて完全自律）。

- 出所: 台帳 `s2-07l.626`（/exit の後の `noop busy`）・`s2-07l.628`（resume の既定化・裁定の逐語は台帳の notes）・`s2-07l.629`（死んだ席を起こさない・state-stale は人が見る）。
- 現物（verified・main 503a703）:
  - 判定の列は `crates/scribe2/src/seat/tick.rs` の `judge` → `front`（登録 row → 打刻の読み〔`stamps_of`・最終行 Busy は `busy`・古い Busy は `state-stale`〕→ 梯子）→ `moving`（移動の門・§4 形 1）→ `back`。`wake` は `launch` を `carry` 空・`restore` 無しで撃つ（§4 形 3「会話は運ばない」）。
  - 打刻の行は `crates/scribe2/src/seat/state.rs` の `Stamp`（`state` / `event` / `ts` / `sid`・sid は claude の session id）。SessionStart / UserPromptSubmit / Stop が書く。
  - 起動の入力 `crates/scribe2/src/seat/cycle/launch.rs` の `Launch` は `carry`（起動行の末尾に足す語・短い形の `-c` / `-r` が使う・account-lifecycle.md §18）を持つ。
- 形（1 つずつ歯が測る・行 f の done と 1:1）:
  1. **門の順を変える**: `front` は登録 row を読んだ直後に（打刻を読む前に）「窓が shell か」（§6 の判定）を見る。shell の周は打刻・梯子を読まず**起こす周**へ進む（打刻は席が居ない間の値で意味を持たない）。shell でない周は今の列のまま（打刻 → 梯子 → 移動の門 → 黙り → …）。
  2. **起こす周の口座**: anchor が群に属せば群の今の口座（`current_of`・記録 > 種・読めない周は `group-unreadable`）、属さなければ自分の row の口座。lock は群の周だけ今のまま（`group-locked`）。
  3. **起こし直しは会話を運ぶ**: 打刻の最終行に sid が在れば `carry` = `--resume <sid>`（値は打刻の字面・会話 id の形〔UUID〕でない周は運ばない）、無ければ空のまま。row の `launch`（雛形）には載せない（§18 と同じ）。要件は SRS v0.23 の FR38 / FR59（起こし直しは打刻の最終行の会話 id を `--resume` で運び、対話の記録の file は複写しない）で、ADR-0049 の「対話の記録は運ばない」は ADR-0067 がその節だけを supersede した（運ぶのは id 1 つ・復帰の道は機械の復帰の 1 本のまま）。
  4. **判定行**: 起こす周は `decision=move move=launch launched=<語>`（今の形 3 と同じ）。群の外の席を起こした周も同じ行（`move=launch`・`reason=-`）。
  5. **移動の周で窓が claude の席**（§4 形 4 の退避）は今のまま: 入力欄が空なら /exit・dialog なら Enter。/exit を送った次の周は 1 の門で shell と読めるので、打刻 busy に止められない（`s2-07l.626` の穴が閉じる）。
  6. **row の無い窓と、窓が claude で記録と一致する席（群 0 の host の席を含む）は 1 字も変わらない**（`no-row` / 今の列。群 0 の host でも row を持ち窓が shell に戻った席は 2 の「row の口座」で起こす＝1 / 2 と両立する）。
- 形（行 h・state-stale の再判定・`s2-07l.629` 候補 3）:
  7. 打刻の最終行が Busy で `seat.tick_stale_s` の 2 倍より古い周（Stop の打刻を失った席）は、窓が claude ∧ 入力欄の門が空なら Busy を無視して今の列（黙りの門以後）へ進む（打刻は書き換えない・判定行の `reason=` は今の `state-stale` でなく列の先の語）。入力欄が空でない周は `state-stale` のまま（人が見る）。`seat.tick_stale_s` より古く 2 倍以内の周は入力欄が空でも `state-stale` のまま（係数の両側を歯が測る＝係数を 1 にする変異はここで落ちる）。閾値は rules 行を足さず既存の `seat.tick_stale_s` × 2（値は行に書かない・係数は歯が pin する）。
- 触らない: 梯子・合図の文面・入力欄の門・`Launched` の variant・群の判定（移り先・記録の書き手）・event の種類・§3 / §5 の install・rules 行（足さない）。
- 却下: /exit を送る手が打刻へ idle を書く（打刻の書き手が hook の外に増える・実測でない値・C10）／UserPromptSubmit が `/exit` を打刻しない（hook が入力の字面で分岐する）／`--continue` で運ぶ（`projects/<cwd>` の最新の小さな session に外れる・id を名指す）／復元 script が席を起こす（器の外・§6 の後続と同じ）／dead な席を dispatch の周が起こす（置き場ごとに tick が居る・§4 の却下と同じ）。
- 歯（`crates/scribe2-boundary/tests/e2e/seat.rs`・§4 の fixture〔PATH の偽 tmux・手書きの `state.jsonl`・`--rules` の写し〕に §6 の `display-message` の口を足す）:
  - 行 f（`seat_tick_wake_` 接頭辞）: (a) 最終行 busy ∧ 前面 `bash` ∧ 群の外の row → `move=launch`・起動行 1 行が row の口座を持つ・末尾に `--resume <打刻の sid>`（base では `noop busy` ＝ RED）(b) 同じ席で最終行に sid が無い → 末尾に `--resume` 無し (c) 最終行 busy ∧ 前面 `bash` ∧ 群の row ∧ 記録 = 別口座 → 記録の口座で起動・`--resume` 付き (d) 前面 `claude` ∧ 最終行 busy → `noop busy`（今のまま・0 key）(e) 前面 `bash` ∧ row 無し → `no-row`・0 key。§2 の fixture の偽 tmux は `list-panes` に前面 `claude` を返す（形 1 で登録 row の在る周は毎回前面を引く・pane の file は触らない＝§2 の歯の判定行は 1 字も変わらない）。
  - 行 h（`seat_tick_stale_` 接頭辞）: (f) busy が stale の 2 倍より古い ∧ 前面 `claude` ∧ 入力欄が空 → 列の先の語（`stamp-recent` か `inject`・0 key か合図 1 行）／入力欄に字が在る → `state-stale`・0 key（base では両方 `state-stale` ＝ RED）(g) busy が stale より古く 2 倍以内 ∧ 前面 `claude` ∧ 入力欄が空 → `state-stale`・0 key（base でも `state-stale` ＝ GREEN・係数を 1 にする変異を落とす側の歯＝(f) と対で係数 2 を pin する。便 092346Z の審査の根: stale と 2 倍の間 ∧ 空の入力欄の周が無く係数 ×1 の変異が生き残る）。
  - lib（`crates/scribe2/src/seat/state.rs` の中・`stamp_sid_` 接頭辞・`seat_tick_` を含まない名にする＝行 a の verify の filter `seat_tick_` に当たらない〔run 4 の Gated FAIL: state.rs の `seat_tick_wake_*` が行 a の歯の file の外と数えられた〕）: 打刻の最終行の sid の読み手が UUID の形だけを返す・sid 無し / 形違いで空（base では読み手が無い ＝ RED）。`tick.rs` には歯を足さない（run 4 の flip-check: tick.rs の lib の歯が base で GREEN ＝ FAIL・`NOOP_REASONS` の母集団と `render` の `move=launch` は e2e の判定行の字面で測る）。
- 後続: 移動の判定（逼迫の読み・移り先・記録の書き換え）を dispatch の周から 1 本の関数に切り出し tick が撃つ形は ADR-0055 の契機を変えるので ADR を先に land する（`s2-07l.629` 候補 2・本 § の外）。

## 8. 群の段の起こし直しも会話を運ぶ（契約表の行 g・account-lifecycle.md §20 形 6 の改め・`s2-07l.628`）

やさしく言うと: dispatch の 1 周の群の段（退避の合図 → settle → 起動）も §7 と同じく、起こす席の打刻の sid を `--resume` で運ぶ。tick と群の段が同じ 1 本を読む。

- 現物（verified・main 503a703）: `crates/scribe2/src/pipe/dispatch/group.rs` の `relaunch` は `launch` を `carry` 空・`restore` 無しで撃つ（`replace_own` は false）。打刻は `crates/scribe2/src/seat/state.rs` の `Stamp`（`sid`）。
- 形（1 つずつ歯が測る・行 g の done と 1:1）:
  1. **carry の読み手は 1 本**: 「置き場の `seat/<target>` の打刻の最終行の sid が会話 id の形なら `--resume <sid>`・無ければ空」を `crates/scribe2/src/seat/tick.rs` でなく打刻の側（`crates/scribe2/src/seat/state.rs`）に 1 本置き、§7 の tick と本 § の群の段が同じ 1 本を呼ぶ（C2・二重に書かない）。
  2. **群の段の起こし直しは carry を渡す**: `relaunch` が起こす席ごとに 1 の値を `carry` に渡す。row の `launch` は雛形のまま。
  3. **通知・退避・記録・承認 event は 1 字も変わらない**。
- 読む時点: 席ごとに pane が shell に戻ったと判じた直後・起こす直前に 1 回読む（退避の合図の後に席が足した打刻まで含めた最終行）。移動の周（settle の窓）と続きの周（1 回だけ見る）で同じ。
- 触らない: 群の判定・lock・退避の合図・`replace_own`・event の種類。
- 却下: 群の段だけ `--continue`（§7 の却下と同じ）／carry を event に記す（会話 id は 1 回きりの値・row にも event にも載せない・§18）。
- 歯（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` に `pipe_dispatch_group_carry_` 接頭辞・§20 の fixture〔偽 tmux と偽 usage〕に打刻の file を足す）: (a) 起こす席の打刻に sid が在る → 起動行の末尾が `--resume <sid>`（base では末尾に無い ＝ RED）(b) 打刻が無い / sid の形でない → 末尾に無し (c) row の `launch` に `--resume` が無い。偽 tmux は退避の合図の Enter に sid の無い打刻を足すので、歯は §21 の移動の周（2 つ目の席が shell に戻らず保留 1）の後に 2 つ目の席の打刻 file を書き直し、前面を shell に戻した続きの周で起動行を測る（承認・断り・保留の event の数が変わらないことも同じ周で測る）。

## 9. tick が群の移動の判定を撃つ（契約表の行 i・§4 / §7 の続き・[ADR-0066](../../design-intent/decisions/ADR-0066-the-management-tick-fires-the-group-move-judgement.html)・[ADR-0055](../../design-intent/decisions/ADR-0055-group-pressure-is-measured-at-run-ends-and-seat-turns-without-a-timer.html) の契機の supersede・`s2-07l.629` 候補 2）

やさしく言うと: 群の移動の判定（逼迫を読み、移り先を決め、記録を書き換える）は今 dispatch の 1 周の群の段にしか無い。便の無い host / project では周が来ないので、席の hook が「移してほしい」と記録を置いても誰も判定しない（2026-09-25 の実測: 群の記録が動いたのは scribe2 の便の周だけ）。席ごとに周期で回る管理 tick が同じ 1 本の判定を lock の内側で撃てば、便の無い host でも群が移る。持ち主の裁定 2026-09-25T04:13Z（口座の移動を含めて完全自律）。**要件 FR38 の契機（便の終端の周と席の hook の 2 系統）に tick の周期を足し、FR27 / AC18 の「tick は測らない（計測の起動 0 件）」を「判定の周だけ鮮度の外を測る」に改める改訂を伴う**（SRS の改訂は持ち主の裁定・本行の受付の前提）。

- 出所: 台帳 `s2-07l.629`（壁 3）・ADR-0066（決定の逐語と却下案）。
- 現物（verified・main 67e74ff）:
  - 群の段は `crates/scribe2/src/pipe/dispatch/group.rs`（`fire` の群の段: 測る集合 `measured_set` → 鮮度の外の口座だけ計測〔`crates/scribe2/src/fleet/usage.rs` の `run_fresh`・頼みの記録が在れば `run`〕→ 判定 `step` / `target_of`〔逼迫は `pressed`・移り先は宣言の候補の順で他の群の今の口座でない ∧ live 便が使っていない ∧ 3 窓とも閾値未満〕→ 執行 `execute`〔記録 `write_current`・承認 event・頼みの記録を `to_history`・退避の合図 → `relaunch`〕・全部 private）。lock と記録の読み手・書き手は `crates/scribe2/src/hook/group.rs`（`Lock` / `current_of` / `write_current` / `put_request` / `pressed` / `group_of`）。
  - tick（`crates/scribe2/src/seat/tick.rs`）は群を読むのは移動の門（§4 `moving`）だけで、判定は撃たず計測もしない（§2「測らない」）。
- 形（1 つずつ歯が測る・行 i の done と 1:1）:
  1. **判定は 1 本**: 群の段の「測る集合 → 鮮度の外の計測 → 判定 → 記録と承認 event / 断りの event と頼みの履歴化」を `crates/scribe2/src/hook/group.rs` の 1 本の関数（入力 = 置き場〔state dir〕・群・他の群の今の口座の集合・鮮度に依らず測る口座の集合・この周で既に測った口座の集合〔1 本が測った口座を足して返す〕・rules の閾値と鮮度・計測の口・出力 = 閉じた enum〔移った〈移り先〉／移らない〈逼迫でない・測る集合のうち逼迫の口座と窓〉／候補なし〈断りの event を記した周・同じ実測に既に断った周の 2 値〉／読めない〉）へ移し、dispatch の群の段はそれを呼ぶ（判定の順・記録の形・event の kind と detail は 1 字も変わらない）。**群の段に残るもの**（1 本は席の pane に触らない・形 3）: 周の頭の続きの周（記録が row より先に動いた席の `relaunch`）→ 1 本 → 1 本の出力から送る §19 の通知（逼迫でない群の逼迫の口座ごとの通知とその記録）と断りの通知（断りの event を記した周だけ）と退避の合図と `relaunch`＝通知は消さず 1 本にも移さず、群の段が出力の値で送る。計測の順（**計測の口は 1 本の中だけ・周の頭は 0 回**）: 群の段の周の頭は計測を 1 回も撃たず、口座の 2 つの集合を組むだけ＝「鮮度に依らず測る口座」（頼みの記録が在る群の測る集合の和・`run` の口で測る）と「この周で既に測った口座」（空で始める）。1 本の入力にこの 2 集合を足し（既に測った口座は 1 本が測った口座を足して返す＝dispatch は群をまたいで持ち回り・tick は両方とも空で渡す）、1 本は測る集合 ∖ 既に測った口座を、強いる集合の口座は `run`・他は `run_fresh` で 1 回ずつ測ってから判定する（今の周の頭の loop と同じ規則＝口座ごとに 1 周 1 回・頼みの強制も同じ・頭と 1 本の 2 か所で測らない・失敗した口座も 2 回目は無い）。移り先の候補の計測（宣言の候補を鮮度の外なら 1 回）は今の形のまま 1 本の中。計測の回数は今の群の段と同じ（成功する計測は口座ごとに 1 周 1 回・候補の計測での再試行も今と同じ）。tick の周は 1 本だけが測る（強いる集合も既に測った口座も空＝鮮度の外だけ）。便 101707Z / 105513Z の gate の根: 周の頭に計測（鮮度の外・頼みの強制のどちらでも）を残すと、頭の計測が失敗した口座を 1 本が run_fresh でもう一度測り 1 周に 2 回になる。測る集合の「群の置き場の席の登録 row の口座」と候補の規則の「live 便が使っていない」の live 便の口座は、外から渡さず 1 本の中で置き場の fleet の event log（`crates/scribe2/src/fleet/store.rs` の `read_all` → `crates/scribe2/src/fleet/replay.rs` の `inflight_by_account`）から読む＝今の群の段と同じ出所（fleet の file は読むだけ・write-set は広げない）。live 便は「その置き場の便」の意味のまま（§20 形 5「1 周の置き場の live 便」）: tick は自席の置き場を渡すので、便の無い置き場の tick は live 便 0 で判定し、他の置き場の便は dispatch の周と同じく見ない（置き場ごとに state dir が別・host 横断の live 便の集合は持たない＝候補の規則を破らず、同じ口座を別の置き場の便が使っている周は移り先の席の口座の門〔§2〕と便の側の候補の規則が受ける）。
  2. **tick が呼ぶ周**: `front` の後・移動の門の前に、自席の anchor が群に属し ∧ その群の判定の打刻（host の群用 dir の `<群>.judged`・ts の 1 行）が `fleet.usage_fresh_s` より古い（か無い）周だけ、群の段と同じ lock の内側で 1 を撃ち（他の群の今の口座の集合は host の群用 dir の記録を `current_of` で読んだ値 ∪ 記録を読めない群の候補＝群の段が周の頭に組む値と同じ規則・tick は 1 群だけ撃つので周の中の更新は無い）、打刻を今の ts で書く。lock を取れない周は撃たず（`group-locked` にはしない・今の列へ進む）。判定の周だけ計測の子 process を起こしてよい（1 と同じ口・鮮度の内側は測らない）。打刻の合図の列（§2）は今まで通り測らない。
  3. **判定の後は各席の tick の移動の門が続きを撃つ**（§4 形 3 / 4・§7）: 判定の側（1 本）は他の置き場の席に触らず退避の合図も通知も送らない。tick の周の出力の受け皿は **自席だけ**: 「候補なし〈断りの event を記した周〉」は断りの 1 行（群の段の断りの通知と同じ字面）を §2 の注入の経路（入力欄の門を通った周だけ・通らない周は落とす＝群の段の通知の門と同じ）で自席にだけ送る（FR38「候補が無い周は移らず user へ通知」の tick の周の受け皿。同じ実測に既に断った周は送らない＝群の段と同じ規則）。「移らない〈逼迫の口座と窓〉」は tick からは送らない（自席の hook の口座の面が自分の番に出す・§19 の通知は群の段のまま）。移った周は同じ周の移動の門（§4 形 4）が自席に /exit を送る（判定の側は送らない＝合図は 1 行）。他の席は各自の tick の周期が来た順に /exit → 起こし直し。
  4. **判定行**: 判定を撃った周は `decision=` の末尾に `judged=<moved:<label>|stay|none|error:<語>>` を足し、撃たない周は `judged=-`（列は固定・省かない・C10）。event は判定の 1 本が記す（tick 自身は今まで通り event を記さない）。
  5. **群 0 の host と群に属さない anchor の tick は判定行の `judged=-`（形 4・列は固定）以外 1 字も変わらず、便の終端の 1 周の群の段の外形（通知・退避・起こし直し・event・計測の回数）は 1 字も変わらない**（通知と退避と起こし直しは群の段が 1 本の出力から送る＝形 1・event は 1 本が記す＝形 4・断りを繰り返さない規則〔前の断りより後に今の口座の新しい実測が無い周は断らない〕は同じで、tick が記した断りの event も前の断りに数える＝同じ実測に群の段が 2 度目の断りを送らない周は、その実測の断りが tick の自席の 1 行で user に届いている）。
- 触らない: 閾値の rules 行（足さない・`fleet.usage_fresh_s` を打刻の間隔に流用）・記録の形・承認 event と断りの event の kind と detail・§19 の通知の字面と記録と送り先（群の段に残る）・退避の字面・`relaunch`・頼みの強制の規則（強いる集合は `run`）・§2 の梯子。
- 却下: timer で dispatch の 1 周を撃つ（ADR-0055 OPT2・周は pipeline と repo に結びつく）／席の hook が判定して起こし直す（ADR-0055 OPT3・置き場ごとに判断が割れる・席は自分を起こせない）／tick が判定だけして退避も自分で全席に送る（他の置き場の席の pane に触る＝§4 の却下と同じ）／判定の間隔の rules 行を足す（鮮度の行で足りる・C17）／tick を 1 席だけ「判定の席」にする（席の生死に依存・どの席の周期でも同じ 1 本が lock で排他される方が簡単）。
- 歯（`crates/scribe2-boundary/tests/e2e/seat.rs` に `seat_tick_judge_` 接頭辞・§4 の fixture〔偽 tmux・host の群用 dir・`--rules` の写し〕に §20 の偽 usage〔`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` の群の fixture と同じ形〕を足す）: (a) 群の今の口座が逼迫 ∧ 候補あり ∧ 判定の打刻なし ∧ 前面 `claude` ∧ 入力欄が空 → 記録が候補へ動き・承認 event 1・`judged=moved:<label>`・自席への /exit は同じ周の移動の門の 1 行だけ（2 行にならない＝判定の側は送らない）・他の席へ 0 key・通知 0 行（done (3) の証拠・base では記録不変 ＝ RED）(b) 打刻が鮮度の内側 → 計測 0・`judged=-`（c）候補なし ∧ 入力欄が空 → `judged=none`・断りの event 1・記録不変・自席へ断りの 1 行（群の段の断りの字面）・他の席へ 0 key（done (3)・base では 0 行 ＝ RED）(c2) 候補なし ∧ 同じ実測に断りの event が既に在る → `judged=none`・event 0・自席へ 0 行 (d) lock が在る → 判定 0・列は今のまま (e) 群に属さない anchor → `judged=-`・0 key。歯の列は done の歯の文と 1:1（便 100121Z の審査の根: done の歯の文から (a) の /exit 0・(c) の記録不変・(e) の 0 key が落ちて done (3) を測る歯が無かった）。既存の `pipe_dispatch_group_` の歯（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`・移動〔`move_`〕だけでなく §19 の通知〔`seat_account_pressure_is_notified` / `same_measurement_is_not_notified_twice` 等〕・計測の回数〔`fresh_account_is_not_remeasured_but_stale_is_once` / `move_request_forces_one_measurement_and_moves_to_history`〕・退避〔`exit_`〕・carry の全部）は 1 字も変えず GREEN のまま（判定・通知・計測の回数の外形が不変の証拠＝行 i の verify は接頭辞 `pipe_dispatch_group_` で全部を撃つ。便 095254Z の審査の根: `move_` だけでは通知と計測の回数の歯が verify に無かった）。
- 後続: 要件 FR38 の改訂（持ち主が /folio-architect で撃つ）／復元 script から claude の送りを外す（§6 の後続）。

## 10. 起こし直しの初手・梯子の列・移動の周の退避（契約表の行 j / 行 k / 行 m・§2 / §4 / §7 の改め・`s2-07l.635`）

やさしく言うと: 2026-09-25 の移動で 3 つが見えた。(1) 起こし直した席は会話が戻るだけで、次の合図（40 分後）まで黙る。(2) 合図の間隔は「初段 × 2^n」の等比で、持ち主が望む切りのよい列（30 分 → 1 時間 → 3 時間 → 6 時間 → 12 時間 → 24 時間 → 停止）が書けない。(3) turn が usage の上限の error で終わると Stop の打刻が無く Busy が残り、tick は打刻を移動の門より先に読むので /exit を送れず席が 2 時間半移らなかった（memo `s2-07l.635`）。持ち主の裁定 2026-09-25T14:32Z（初手は起動行に積む）/ 14:39Z（梯子は列を明示する rules 行 1 本・tick の周期は 15 秒）/ 14:4xZ（上限で終わった席にも /exit は届くべき＝台帳 `s2-07l.635` の notes に逐語）。

- 出所: 台帳 `s2-07l.635`（事象と原因の連鎖）・§7 形 3（起こし直しの carry）・§2 形 5（rules 行 4 本）・§4 形 4（移動の門）。
- 現物（verified・main 087f1df）:
  - 起こし直しの起動行は §7 形 3 の 1 本（`crates/scribe2/src/seat/state.rs` の `resume_carry`・`--resume <sid>` の 2 語か空）を tick の `wake` と群の段の `relaunch` が `carry` に運び、起動の 1 本（`crates/scribe2/src/seat/cycle/launch.rs` の `with_tail`）が行の末尾に語を空白で足して pane へ打つ（shell が読む 1 行＝引用は呼び手の責任）。
  - 梯子の待ちは `crates/scribe2/src/seat/tick.rs` の `Pace`（`stale_s` × `factor` ^ 段・上限 `max_s` を超える段は送らない）で、rules 行は `seat.tick_interval_s`（60）/ `seat.tick_stale_s`（2400・初段の待ち・黙りの閾値・Busy の古さの 3 役）/ `seat.pointer_backoff_factor`（2）/ `seat.pointer_backoff_max_s`（86400）。埋め込み manifest は rows=70 kinds=68。tick 1 回の実測は wall 0.05 秒・CPU 0.04 秒・RSS 29 MB（fleet の event log 6 MB・2.7 万行の replay が主）。
  - 判定の列は `front`（登録 row → 窓が shell か → 打刻 → 梯子）→ `moving`（移動の門）→ `back`。Busy / state-stale は `front` で止まり移動の門に届かない。
- 形（行 j・起こし直しの初手・1 つずつ歯が測る・done と 1:1）:
  1. **起動行の末尾に初手の合図を 1 語積む**: carry の読み手（§7 形 3 の 1 本）を「`--resume <sid>`（在れば）＋ 初手の 1 語」を返す形に広げ、tick の `wake` と群の段の `relaunch` は今まで通りその 1 本を呼ぶ（呼び手を増やさない）。sid が無い・形違いの周も初手だけは積む（新しい会話に最初の 1 文として届く）。
  2. **初手の文面は 1 関数が正本**（§2 の合図の文面と同じ置き方・tick.rs の `signal` の隣）: 先頭は `<NAME> seat: relaunch` で、続きは「台帳の現在地（bd --readonly ready --limit 0）から続きを進める（会話は直前から続く・合図の梯子は段 0 から）」。字面に `'`（単引用）と改行を含めない。
  3. **1 語の形**: 起動行は shell が読むので初手は単引用で括った 1 語（`'<文面>'`）。`with_tail` は引用しない（今のまま）＝括るのは carry の読み手。
  4. **row の launch には載せない**（§18 と同じ・`--resume` と同じ扱い）。梯子には数えない（pointer-ladder に書かない・段 0 は起こし直しの後の最初の合図のまま）。
- 形（行 k・梯子の列と周期・rules の改め）:
  5. **rules 行 `seat.pointer_ladder_s`（値の形は文字列の列・秒・非空・狭義に昇順）を足し、`seat.pointer_backoff_factor` と `seat.pointer_backoff_max_s` を退役する**（行と `RuleKind` の variant を消す＝可逆な移動は git の歴史）。値は `["1800", "3600", "10800", "21600", "43200", "86400"]`（裁定 id = user 2026-09-25T14:39Z・ruled_at 2026-09-25）。`Pace` は `stale_s` と列を持ち、段 n の待ちは列の n 番目・列を越えた段は stopped（送らない＝今の「上限で打ち切り」と同じ極性・合図は列の長さの本数）。列の要素が数でない・空・昇順でない周は今の `no-rule` と同じ断り（rc 1・0 key・既定に倒さない）。
  6. **`seat.tick_stale_s` を 1800、`seat.tick_interval_s` を 15 に**（裁定 id は同じ 14:39Z）。stale_s の役は「黙りの閾値（settle の基準）・Busy の古さ」の 2 役になり、初段の待ちは列の先頭が持つ。周期 15 秒の根拠は実測（席 8 つで毎分 32 回・CPU 1.3 秒/分・journal 4.6 万行/日）。
  7. **合図の文面の「次の合図は N 秒後」は列から引く**。最後の段は「次は無い（打ち切り）」と書く。埋め込み manifest は rows=69 kinds=67（−2 +1）。`rules_external_form` の snapshot と行数・kind 数の pin の歯（§2 形 5 の 3 本）を書き換える（名は変えない・assert の内訳に本便の −2 +1 を足す）。tick の unit（§3・行 b）は周期を読むだけなので code は不変、host 側は着地後に `seat tick install` を撃ち直して timer を焼き直す（手順・契約の外）。
- 形（行 m・移動の周の退避は打刻に依らない・`s2-07l.635` 候補 (a)）:
  8. **移動の周は打刻を読まずに退避する**: `front` は登録 row → 窓が shell か（§7 形 1）→ **移動の周か**（anchor が群に属し ∧ 群の記録の口座 ≠ row の口座・記録が読めない周は今の `group-unreadable`）を見て、移動の周 ∧ 窓が claude なら打刻と梯子を読まずに移動の門（§4 形 4 の規則そのまま: 入力欄が空なら /exit 1 行・dialog なら Enter・字が在れば `input-busy`・読めなければ `input-unknown`）へ進む。移動の周でない席は今の列（打刻 → 梯子 → …）。移動の門の関数は 1 本のまま（呼ぶ場所が `front` の中へ移る・行 i の判定で移った周の呼び出しは §9 形 3 のまま）。
  9. **/exit は tick の周期ごとに送ってよい**（Busy の周も送る）: turn の途中の /exit は claude の入力の列に積まれ turn の終わりで実行される（2026-09-25T14:20Z の実測）。上限の error で終わった turn は列が消えるが入力欄は空に戻るので、次の周の /exit が届く（memo `.635` の穴が閉じる）。積まれた重複は害が無い（最初の 1 つで席が終わる）。tick.jsonl の記録は今まで通り 1 送信 1 行。
  10. **判定行**: 退避の周は今の `decision=move move=exit`（形は不変）。Busy を読まないので `reason=busy` / `state-stale` は移動の周には出ない。
- 触らない: 合図の梯子の記録の形・移動の門の合図の字面と Enter の規則・awake（shell の周・§7）・群 0 の host と群の外の席の列・§19 の通知・rules validate の外形（行数以外）・`with_tail` の無引用。
- 却下: 初段を 5 分にする（busy の打刻も 5 分で stale 扱いになり長い turn の席に合図が割り込む）／初手を tick の最初の周に送る（起動から最初の周まで最大 1 周期黙り、起こし直しの周と平常の周で梯子の意味が割れる）／等比のまま値だけ変える（30m, 1h, 2h, 4h, 8h, 16h・持ち主の列が書けない）／退避の重複を打刻の ts で 1 回に絞る（上限で終わった turn は ts が動かず再送されない＝memo の穴が閉じない）／tick が会話の記録を読んで上限の文を探す（ADR-0067 の原則: 器は会話の記録を読まない）。
- 歯（`crates/scribe2-boundary/tests/e2e/seat.rs`・§4 / §7 の fixture）:
  - 行 j（`seat_tick_wake_` の既存 5 本の起動行の assert を広げる ＋ lib は `crates/scribe2/src/seat/state.rs` の `relaunch_carry_` 接頭辞〔`seat_tick_` を含まない名〕・`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` の `pipe_dispatch_group_carry_` の既存 3 本）: (a) 最終行 busy ∧ 前面 bash ∧ 群の外の row → 起動行の末尾が `--resume <sid> '<NAME> seat: relaunch …'`（base では `--resume <sid>` で終わる ＝ RED）(b) sid 無し → 末尾が初手の 1 語だけ（`--resume` 無し）(c) 群の段の relaunch も同じ末尾 (d) lib: carry の読み手が sid あり / なし / 形違いで `[--resume, sid, 初手]` / `[初手]` / `[初手]` を返し、初手は単引用で始まり終わり中に `'` と改行が無い (e) row の launch の event に初手が無い。
  - 行 k（`seat_tick_ladder_` / `seat_tick_change_returns_to_step_zero` / `seat_tick_missing_rule_rows` の既存を列の値で書き換え ＋ `tests/e2e/rules.rs` の `rules_embedded_manifest_declares_tick_` と pin の歯 3 本）: (f) 列 [1800, 3600, 10800, 21600, 43200, 86400] の `--rules` で 6 本の合図が列の待ちで出て 7 段目は stopped（base では factor / max の行が無い写しは no-rule ＝ RED）(g) 列を欠く・要素が数でない・昇順でない写し → `no-rule` rc 1・0 key (h) 変化で段 0 に戻り 1800 秒黙った周に送る (i) 埋め込み manifest に `seat.pointer_ladder_s` が裁定 id つきで在り factor / max の行と kind が無い・rows=69 kinds=67・`rules_external_form` の snapshot (j) `seat.tick_stale_s` = 1800・`seat.tick_interval_s` = 15。lib（`crates/scribe2/src/seat/tick.rs` の既存の `seat_tick_` の歯・`Pace` の待ちと合図の文面）は列で書き換える。
  - 行 m（`seat_tick_evacuate_` 接頭辞）: (k) 記録 ≠ row ∧ 前面 claude ∧ 最終行 Busy（新しい）∧ 入力欄が空 → /exit 1 行・`decision=move move=exit`（base では `noop busy` ＝ RED）(l) 同じで Busy が stale より古い → /exit 1 行（base では `state-stale` ＝ RED）(m) 記録 = row ∧ Busy → `noop busy`・0 key（不変）(n) 記録 ≠ row ∧ 入力欄に字 → `input-busy`・0 key (o) 2 周続けて撃つと /exit が 2 行（積む・止めない・記録も 2 行）。
- 後続: 上限で終わった turn を hook が打刻する口（memo `.635` 候補 (b)・Notification の payload の実測が先）／journal の判定行の量（周期 15 秒で 4.6 万行/日・必要なら noop の行を間引く rules 行）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "管理 tick を戻す — seat tick の判定の列（登録 row → 状態の打刻 → digest の比較 → 黙りの門 → 上限 → 床 → 口座の門 → 入力欄の門 → 記録 → 注入）・変化の digest は状態の打刻の最終行の ts・梯子の記録 1 file で backoff（初段 40 分・係数 2・次の待ちが 24 時間を超える段は送らない・変化で段 0）・rules 行 4 本・打刻の合図は既存の注入の経路・不在の歯の書き換え"
req = ["FR27", "FR43", "FR44", "FR38", "FR40", "AC18", "NFR4"]
section = "2"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "+crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail seat_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail gone_from_the_usage", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_review_same_kind_stop_kind_is_last_in_declaration_order_and_paired_with_the_row"]
size = "L"
growth = ["crates/scribe2/src/rules/mod.rs:60", "crates/scribe2/src/seat/mod.rs:10", "crates/scribe2/src/seat/cli.rs:120"]
done = "(1) seat tick --state-dir S --target S:W [--rules F] が判定行 1 行を出し、登録 row ∧ 最終行 Idle が seat.tick_stale_s 以上前 ∧ 入力欄が空の席にだけ合図 1 行を注入して pointer-ladder に 1 行（step=0・digest=null）を書き tick.jsonl に who=seat-inject の 1 行が増える (2) no-row / state-missing / state-unreadable / busy / state-stale / stamp-recent / input-busy / input-unknown の各周は 1 key も送らず、input-own-queued の周は pass_input の Enter 1 key だけを送って合図の text は 0 key で、どの周も記録も増えず理由が判定行に出て、pointer= と step= は梯子を評価した周だけに載る (3) settle は sent_at より後の Stop の ts か、応えない席では seat.tick_stale_s を過ぎた周の digest を基準にし、無変化の周は wait:<s> step=n で送らず、待ちの分だけ過去に書いた sent_at で段 1〜5 の合図が出て段 6 は stopped で送らず（合図 6 本で打ち切り）、基準の後に最終行の ts が動くと段 0 に戻って 40 分黙った周に送る（打ち切りの後も同じ） (4) 口座の門は fresh_rows と pressed だけを読み、鮮度の内側の記録が閾値以上の席には account-pressed で送らず、記録無し・鮮度の外は通し、tick の周の偽 client の呼出は 0 件 (5) 記録が読めない周は record-unreadable・書けない周は record-unwritable で送らず、注入が落ちた周も記録は残る (6) rules 行 4 本（seat.tick_interval_s 60 / seat.tick_stale_s 2400 / seat.pointer_backoff_factor 2 / seat.pointer_backoff_max_s 86400）が裁定 id つきで埋め込み manifest に在り RuleKind の ALL と rules validate の外形（rows=68 kinds=66）に載り、行を欠く --rules は decision=error reason=no-rule rc 1 (7) 使い方の 1 行に tick が増えて seat_usage_external_form の snapshot が動き、不在の歯 3 本は tick を在る側・--rules は tick だけが受ける形に書き換わって meter / heartbeat / cycle / inject / externalize / rebrief / consume の不在は不変 (8) 極性一覧・event の種類・InjectionRecord の schema・hook の打刻の書き手は 1 byte も変わらない (9) rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all は LedgerDeniedWrites の位置から 3 つが LedgerDeniedWrites / HostGuardDeniedCommands / HostGuardRmProtected で、その直後に足す 4 kind が宣言順で続いて ALL が終わる形を測り、rules_embedded_manifest_declares_one_capability_row_per_role は kind の母集団 66 を、rules_embedded_manifest_is_valid_and_covers_all_kinds は埋め込み manifest の行数 68 を測って緑で、rules_review_same_kind_stop_kind_is_last_in_declaration_order_and_paired_with_the_row は assert を変えずに緑"

[[contract]]
id = "b"
title = "tick の unit を器が導出して host へ書く — seat tick install / uninstall（導出は pure な 1 関数・一時 file → rename・bytes 一致は unchanged・不一致は unit-exists・有効化は daemon-reload → enable --now・撤去は disable --now → 退役 dir へ mv・印の無い file は unit-foreign）と doctor の tick-unit= の 1 項目"
req = ["FR64", "FR40", "AC18", "NFR4"]
section = "3"
write-set = ["+crates/scribe2/src/seat/tick/install.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_unit_external_form.snap", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail seat_unit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_unit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_doctor_external_form"]
size = "M"
depends = ["a"]
growth = ["crates/scribe2/src/seat/cli.rs:120", "crates/scribe2/src/seat/role.rs:60", "crates/scribe2/src/pipe/confine.rs:10", "crates/scribe2-boundary/src/main.rs:40"]
done = "(1) seat tick install が登録 row の席の service と timer の 2 file を導出の bytes で unit dir に書き（Environment / WorkingDirectory / %h 無し・OnUnitActiveSec は seat.tick_interval_s・先頭行に器の印）、偽 systemctl が daemon-reload → enable --now <timer> の順で 2 回呼ばれ、tick.jsonl に who=seat-tick-install の 1 行が増える (2) 同じ bytes は unchanged で file 不変・enable だけ、1 byte 違う既存 file は unit-exists で file 不変・systemctl 0 回・rc 1、登録 row 無しは no-row、seat.tick_interval_s を欠く --rules は no-rule で file 0 (3) uninstall は install と同じ --binary / --rules で導出し直し、disable --now の後に 2 file を .retired/<name>.<ts> へ同じ bytes で移し、印の無い file と導出の bytes と違う file は動かさず理由で断り、--binary を欠く周は使い方の誤りで動かない (4) doctor --unit-dir U --binary PATH [--rules F] が登録 row の行ごとに tick-unit=present|absent|foreign（present = 印と bytes が同じ引数の導出と一致）を足し、--unit-dir だけで --binary 無しは使い方の誤り、--unit-dir 無しは項目を足さず seat_doctor_external_form の snapshot が 1 byte も動かない (5) 使い方の 1 行に tick install / tick uninstall が増えて seat_usage_external_form が動く (6) systemctl の綴りは confine.rs の定数 1 つを共有し、器は env・home・current_exe を読まない"
[[contract]]
id = "c"
title = "tick が群の移動の続きを撃つ — 判定の列の front の直後に移動の門（自席の登録 row の口座 ≠ 群の記録の口座）を足し、群の段と同じ lock の内側で、pane が shell なら同じ target に記録の口座の席を起こし、shell でなければ入力欄の門を通して /exit（dialog の既定の行なら Enter）を 1 手・移動の周は heartbeat を送らず梯子を触らない（§4・ADR-0058 §2・ADR-0055 OPT1・s2-07l.616）"
req = ["FR38", "FR27", "FR36", "NFR4"]
section = "4"
write-set = ["crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail seat_tick_move_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_move_"]
size = "M"
depends = ["a"]
growth = ["crates/scribe2/src/hook/group.rs:80", "crates/scribe2/src/pipe/dispatch/group.rs:20"]
done = "(1) 登録 row の anchor が群に属し群の今の口座（current_of）が row の口座と違う周は、front の直後で移動の周になり、黙り・上限・床・口座の門・合図の注入を撃たず pointer-ladder を書かず、記録が在るのに読めない周は noop の group-unreadable、群に属さない anchor と記録が row と一致する席は 1 字も変わらない (2) 移動の周は群の段と同じ lock（host の群用 dir の 1 file・実装は hook/group.rs の 1 本を群の段と共有）の内側で撃ち、取れない周は noop の group-locked で 1 key も送らない (3) pane が shell の周は launch の 1 本で同じ target に記録の口座の席を起こし（anchor と置き場は自席・settle / step は rules・登録 row は起動が書き直す）、判定行は decision=move move=launch launched=<語> で、起こせない周も語を載せて次の周にまた判じる (4) pane が shell でない周は pass_input を通し、空なら /exit の 1 行を deliver_within（窓は pipe.stop_grace_ms）で送って tick.jsonl に who=seat-tick-move what=/exit の 1 行を未確認でも残し、Foreign で tail が dialog の既定の行の literal に等しい周は Enter 1 回だけで what=enter:exit-dialog、それ以外の Foreign / UnknownInput / OwnQueued は今の語で 0 key（OwnQueued の Enter は pass_input のまま）、Busy の打刻は front で止まる (5) /exit と dialog の既定の行の値は hook/group.rs の 1 か所を群の段と tick が読み、群の段の続きの周と移動の周は不変 (6) 判定行は decision=move の周に reason=- pointer=- step=- move=<launch|exit|enter> launched=<語|-> を持ち、他の周は末尾に move=- launched=- を持ち、NoopReason は末尾に group-unreadable / group-locked の 2 値、TickDecision は閉じた Move を持つ (7) tick は event を記さず、群 0 の host は 1 字も変わらない 歯: seat_tick_move_ の歯が (a)〜(f) と lib の 2 本を測る（base では移動の周に合図の注入か stamp-recent が出て move= の欄が無い ＝ RED）"
[[contract]]
id = "d"
title = "席の起動が tick の unit を入れる — host の面の表 [[tick]]（unit-dir / binary・0 か 1 行）を loader が読み、launch が Done の周に §3 の install の 1 本を面の値で撃って行の末尾に tick-unit=<installed|unchanged|refused:<語>> を足し、doctor は flag が無ければ面の値を既定にして表の在る host の行に tick=declared を足す・表の無い host は 1 字も変わらない（§5・ADR-0064・s2-07l.616）"
req = ["FR64", "FR59", "FR61", "FR40", "NFR4"]
section = "5"
write-set = ["crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/tick/install.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail host_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_host_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_doctor_tick_"]
size = "M"
depends = ["b"]
growth = ["crates/scribe2/src/rules/manifest.rs:120", "crates/scribe2/src/seat/cycle/launch.rs:80", "crates/scribe2/src/seat/tick/install.rs:60", "crates/scribe2/src/seat/cli.rs:40", "crates/scribe2/src/seat/role.rs:40", "crates/scribe2-boundary/src/main.rs:40"]
done = "(1) host の面の [[tick]]（unit-dir / binary の 2 欄・絶対 path・0 か 1 行）を loader が読んで Manifest が tick() で返し、2 行目・相対 path・欠けた欄は行番号つきで断り、表の無い host は既存の外形 snapshot が 1 byte も動かない (2) launch が Done を返す周に面に [[tick]] が在れば §3 の install と同じ 1 本（導出 → 照合 → 書き → daemon-reload → enable --now）を起動の置き場と target・面の unit dir と binary・起動に渡された rules で撃ち、起動の 1 行の末尾に tick-unit=<installed|unchanged|refused:<語>> を足し、表の無い host は起動の行が 1 字も変わらず、install の断りは起動の rc を変えず、Refused / Failed / None の周は撃たない（短い形と長い形の両方） (3) doctor は --unit-dir / --binary が無く面に [[tick]] が在る周は面の値で tick-unit= を足し、flag が在れば flag が勝ち、どちらも無い周は項目を足さず既存の外形 snapshot が 1 byte も動かず、表の在る host だけ host の行に tick=declared を足す (4) §3 の導出・unit の file 名・seat tick install / uninstall の口・§2 / §4 の判定の列・既存の表の形は 1 字も変わらない 歯: rules_host_tick_ / seat_launch_tick_ / seat_doctor_tick_ と lib の host_tick_ が §5 の歯の各項を測る（base では [[tick]] が未知の表で断られ、起動の行に tick-unit= が無い ＝ RED）"
[[contract]]
id = "e"
title = "pane が shell かの判定は子 process まで見る — 前面が SHELLS の語の周だけ pane_pid を取り /proc の children が空でなければ shell でないと読み、pid が取れない周は今のまま（§6・s2-07l.624）"
req = ["FR59", "FR38", "NFR4"]
section = "6"
write-set = ["crates/scribe2/src/seat/mod.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_pane_shell_"]
size = "S"
growth = ["crates/scribe2/src/seat/mod.rs:40"]
done = "(1) 前面の語が SHELLS に在る周だけ display-message -p -t <target> の pane_pid を取り、/proc/<pid>/task/<pid>/children が空でなければ shell でないと読む (2) pid が整数でない・/proc の file が読めない周は前面の語だけで決める（既存の歯が 1 本も動かない） (3) 呼び手 4 つは引数も戻りも断りの語も 1 字も変わらない 歯: seat_pane_shell_ の歯が、子を持つ sh の前面で tick の移動の周が move=exit になること（base では move=launch ＝ RED）・子なしと pid 不明は move=launch のまま・前面 claude では display-message を 0 回撃つことを偽 tmux の呼び出し記録で測る"

[[contract]]
id = "f"
title = "tick が死んだ席を起こし、移動の門を打刻の前に置き、起こし直しは打刻の sid を --resume で運ぶ — front は row の直後に窓が shell かを見て、shell なら群の記録か row の口座で launch（§7 形 1〜6・s2-07l.626 / .628 / .629）"
req = ["FR59", "FR38", "FR40", "NFR4"]
section = "7"
write-set = ["crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/seat/state.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_wake_", "cargo nextest run -p scribe2 --lib --no-tests=fail stamp_sid_"]
size = "M"
growth = ["crates/scribe2/src/seat/tick.rs:120", "crates/scribe2/src/seat/state.rs:30"]
depends = ["e"]
done = "(1) front は登録 row を読んだ直後・打刻を読む前に窓が shell かを見て、shell の周は打刻と梯子を読まず起こす周へ進み、shell でない周は今の列のまま (2) 起こす周の口座は anchor が群に属せば current_of の解決値（読めない周は group-unreadable・lock は今のまま group-locked）、属さなければ row の口座 (3) 打刻の最終行の sid が会話 id の形なら carry に --resume <sid> を渡し、無ければ空で、row の launch には載せない（読み手は state.rs に 1 本） (4) 判定行は群の外の起こしでも decision=move move=launch launched=<語> reason=- (5) 移動の周で窓が claude の席の退避（/exit・Enter）は今のまま (6) row の無い窓と、窓が claude で記録と一致する席（群 0 の host の席を含む）は 1 字も変わらず NOOP_REASONS の母集団も変わらない（群 0 の host でも row を持ち窓が shell に戻った席は (2) の row の口座で起こす） 歯: seat_tick_wake_ の歯が、最終行 busy ∧ 前面 bash ∧ 群の外の row で move=launch と起動行の末尾の --resume <sid>（base では noop busy ＝ RED）・sid 無しで末尾に無し・群の row では記録の口座・前面 claude ∧ busy は noop busy・row 無しは no-row を測り、lib の歯は state.rs の stamp_sid_ 接頭辞だけ（読み手が UUID の形だけを返す・base では読み手が無い ＝ RED）で tick.rs には歯を足さない（seat_tick_ を含む名の歯を write-set の外の file に置かない）"

[[contract]]
id = "g"
title = "群の段の起こし直しも会話を運ぶ — relaunch が打刻の sid の読み手（行 f の state.rs の 1 本）を carry に渡し、row の launch と event は変えない（§8・s2-07l.628）"
req = ["FR59", "FR40", "NFR4"]
section = "8"
write-set = ["crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_carry_"]
size = "S"
growth = ["crates/scribe2/src/pipe/dispatch/group.rs:20"]
depends = ["f"]
done = "(1) relaunch は起こす席ごとに打刻の最終行の sid の読み手（行 f が state.rs に置く 1 本）を呼び、会話 id の形なら carry に --resume <sid> を渡す (2) row の launch は雛形のまま・通知・退避・記録・承認 event は 1 字も変わらない 歯: pipe_dispatch_group_carry_ の歯が、打刻に sid の在る席の起動行の末尾が --resume <sid>（base では末尾に無い ＝ RED）・打刻無しは末尾に無し・row の launch に --resume が無いことを測る"

[[contract]]
id = "h"
title = "state-stale の再判定 — Busy が seat.tick_stale_s の 2 倍より古く窓が claude で入力欄が空なら Busy を無視して列の先へ進み、字が在れば state-stale のまま（§7 形 7・s2-07l.629 候補 3）"
req = ["FR38", "FR43", "NFR4"]
section = "7"
write-set = ["crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_stale_"]
size = "S"
growth = ["crates/scribe2/src/seat/tick.rs:40"]
depends = ["f"]
done = "(1) 打刻の最終行が Busy で seat.tick_stale_s の 2 倍より古い周は、窓が claude ∧ 入力欄の門が空なら Busy を無視して黙りの門以後の列へ進む（打刻は書き換えず rules 行も足さず係数 2 は歯が pin） (2) 入力欄に字が在る周と 2 倍以内の周（seat.tick_stale_s より古く 2 倍以内の周を含む）は state-stale / busy のまま 歯: seat_tick_stale_ の歯が、2 倍より古い busy ∧ 空の入力欄で列の先の語（base では state-stale ＝ RED）・2 倍より古い busy ∧ 字の在る入力欄で state-stale・0 key・seat.tick_stale_s より古く 2 倍以内の busy ∧ 空の入力欄で state-stale・0 key（係数を 1 にする変異が落ちる）を測る"
[[contract]]
id = "i"
title = "tick が群の移動の判定を撃つ — 群の段の判定（測る集合 → 鮮度の外の計測 → 判定 → 記録と承認 event）を hook/group.rs の 1 本に移して dispatch の周と tick が同じ 1 本を呼び、tick は群ごとに usage_fresh_s に 1 回・lock の内側で撃つ（§9・ADR-0066）"
req = ["FR38", "FR27", "NFR4"]
section = "9"
write-set = ["crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_judge_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_"]
size = "L"
growth = ["crates/scribe2/src/seat/tick.rs:100", "crates/scribe2/src/hook/group.rs:180", "crates/scribe2/src/pipe/dispatch/group.rs:0"]
depends = ["f"]
done = "(1) 群の段の測る集合・鮮度の外の計測・判定・記録と承認 event / 断りの event と頼みの履歴化を hook/group.rs の 1 本の関数（入力は置き場・群・他の群の今の口座・鮮度に依らず測る口座・この周で既に測った口座〔測った口座を足して返す〕・rules の閾値と鮮度・計測の口・出力は閉じた enum の 4 変種〈移った〈移り先〉／移らない〈逼迫の口座と窓〉／候補なし〈断りの event を記した周・同じ実測に既に断った周の 2 値〉／読めない〉・測る集合の登録 row の口座と候補の規則の live 便の口座は 1 本の中で置き場の fleet の event log から読み、tick は自席の置き場を渡す）へ移し、dispatch の群の段は続きの周の relaunch → 1 本 → 1 本の出力から送る §19 の通知と断りの通知と退避の合図と relaunch の順で残し（通知は 1 本に移さず消さない・計測の口は 1 本の中だけで周の頭は 0 回＝頭は「鮮度に依らず測る口座」と「この周で既に測った口座」の 2 集合を組んで 1 本に渡し、1 本が測る集合 ∖ 既に測った口座を強いる集合なら run・他は run_fresh で 1 回ずつ測って測った口座を返す）、判定の順と記録の形と event の kind と detail は 1 字も変わらず、成功する計測の回数は口座ごとに 1 周 1 回のまま (2) tick は front の後・移動の門の前に、自席の anchor が群に属し ∧ 群用 dir の <群>.judged の ts が fleet.usage_fresh_s より古いか無い周だけ、同じ lock の内側で 1 本を撃って打刻を書き、lock を取れない周は撃たずに今の列へ進み、判定の周だけ計測の子 process を起こす (3) 判定の側（1 本）は他の置き場の席に触らず退避の合図も通知も送らず、tick の周の受け皿は自席だけ＝候補なしで断りの event を記した周は断りの 1 行（群の段の断りの字面）を §2 の注入の経路で自席にだけ送り（入力欄の門を通らない周は落とす・同じ実測に既に断った周は送らない）、移らないの周は送らず、移った周の /exit は同じ周の移動の門の 1 行だけ (4) 判定行の末尾に judged=<moved:<label>|stay|none|error:<語>|-> を足し、承認 event と断りの event は判定の 1 本だけが記す（tick も群の段も自分では記さない＝記す者は 1 か所・断りを繰り返さない規則も 1 本の中） (5) 群 0 の host と群の外の anchor の tick は判定行の judged=- 以外 1 字も変わらず、群の段の外形（通知・退避・起こし直し・event・計測の回数・断りを繰り返さない規則）は 1 字も変わらず tick が記した断りの event も前の断りに数える 歯: seat_tick_judge_ の歯が、(a) 逼迫 ∧ 候補あり ∧ 判定の打刻なし ∧ 前面 claude ∧ 入力欄が空で記録が候補へ動き・承認 event 1・judged=moved:<label>・自席への /exit は移動の門の 1 行だけ・他の席へ 0 key・通知 0 行（base では記録不変 ＝ RED）(b) 打刻が鮮度の内側で計測 0・judged=- (c) 候補なし ∧ 入力欄が空で judged=none・断りの event 1・記録不変・自席へ断りの 1 行・他の席へ 0 key（base では 0 行 ＝ RED）(c2) 同じ実測に断りの event が既に在れば judged=none・event 0・自席へ 0 行 (d) lock ありで判定 0・列は今のまま (e) 群の外で judged=-・0 key を測り、pipe_dispatch_group_ の既存の歯（移動・通知・計測の回数・退避・carry）は 1 字も変えず GREEN"
[[contract]]
id = "j"
title = "起こし直しの起動行に初手の合図を積む — carry の 1 本が --resume <sid>（在れば）の後ろに単引用の 1 語（<NAME> seat: relaunch …）を足し、tick の wake と群の段の relaunch が同じ 1 本を呼ぶ（§10 形 1〜4・s2-07l.635）"
req = ["FR59", "FR38", "FR27", "NFR4"]
section = "10"
write-set = ["crates/scribe2/src/seat/state.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail relaunch_carry_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_wake_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_carry_"]
size = "S"
growth = ["crates/scribe2/src/seat/state.rs:25", "crates/scribe2/src/seat/tick.rs:15", "crates/scribe2/src/pipe/dispatch/group.rs:5"]
depends = ["g"]
done = "(1) carry の読み手（§7 形 3 の 1 本）が --resume <sid>（打刻の最終行の sid が会話 id の形のとき）の後ろに初手の 1 語を足して返し、sid が無い・形違いの周は初手の 1 語だけを返す（空を返す周は無い） (2) 初手は単引用で括った 1 語で、文面は tick.rs の 1 関数が正本、先頭が <NAME> seat: relaunch で、字面に単引用と改行が無い (3) tick の wake と群の段の relaunch は同じ 1 本を呼び起動行の末尾に語を足すだけで、row の launch と登録の event には初手が載らない (4) 梯子（pointer-ladder）は触らず初手を段に数えない (5) 起動行の他の語（--model / --effort / 雛形）は 1 字も変わらない 歯: seat_tick_wake_ の既存 5 本の起動行の末尾の assert を (a) 群の外の row で --resume <sid> '<NAME> seat: relaunch …'（base では --resume <sid> で終わる ＝ RED）(b) sid 無しで初手の 1 語だけ (c) 群の row で記録の口座 ∧ 同じ末尾 に広げ、pipe_dispatch_group_carry_ の既存 3 本も同じ末尾に広げ、lib の relaunch_carry_ の歯が sid あり / なし / 形違いで [--resume, sid, 初手] / [初手] / [初手] と初手の引用の形を測る"

[[contract]]
id = "k"
title = "梯子の列と周期 — rules 行 seat.pointer_ladder_s（秒の文字列の列・非空・昇順）を足し factor / max の行を退役、tick_stale_s を 1800・tick_interval_s を 15 に（§10 形 5〜7・裁定 2026-09-25T14:39Z）"
req = ["FR27", "FR43", "FR44", "AC18", "NFR4"]
section = "10"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail seat_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_ladder_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_change_returns_to_step_zero", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_missing_rule_rows", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all"]
size = "M"
growth = ["crates/scribe2/src/rules/mod.rs:10", "crates/scribe2/src/seat/tick.rs:30"]
depends = ["a"]
done = "(1) 埋め込み manifest に seat.pointer_ladder_s（値 [\"1800\", \"3600\", \"10800\", \"21600\", \"43200\", \"86400\"]・裁定 id user 2026-09-25T14:39Z）が在り seat.pointer_backoff_factor と seat.pointer_backoff_max_s の行と RuleKind の variant が無く、seat.tick_stale_s が 1800・seat.tick_interval_s が 15 で、rows=69 kinds=67 が rules validate の外形と snapshot に載る (2) Pace は stale_s と列を持ち、段 n の待ちは列の n 番目、列を越えた段は stopped で送らず、合図の本数は列の長さ (3) 列を欠く・要素が数でない・空・昇順でない --rules の写しは decision=error reason=no-rule rc 1・0 key (4) 合図の文面の次の待ちは列から引き、最後の段は次が無いと書く (5) 変化で段 0 に戻り列の先頭の秒だけ黙った周に送る・settle の基準と Busy の古さは stale_s のまま (6) tick の unit の導出（行 b）と極性一覧・event の種類・打刻の書き手は 1 byte も変わらない (7) pin の歯 3 本（行数・kind の母集団・ALL の末尾の並び）は名を変えず内訳に −2 +1 を足して緑 歯: seat_tick_ladder_ の既存の歯が列の待ち [1800, 3600, 10800, 21600, 43200, 86400] で 6 本出て 7 段目 stopped（base では factor / max を欠く写しが no-rule ＝ RED）・seat_tick_missing_rule_rows が列を欠く / 数でない / 昇順でない写しで no-rule・seat_tick_change_returns_to_step_zero が 1800 秒の黙りで段 0・rules_embedded_manifest_declares_tick_ が列の行と factor / max の不在と 1800 / 15 を測り、lib の seat_tick_ の歯が Pace の待ちと文面を列で測る"

[[contract]]
id = "m"
title = "移動の周の退避は打刻に依らない — front が登録 row → 窓が shell か → 移動の周か の順で見て、移動の周 ∧ 窓が claude は打刻と梯子を読まずに移動の門へ進み /exit を周期ごとに送る（§10 形 8〜10・s2-07l.635 候補 (a)）"
req = ["FR38", "FR27", "FR43", "NFR4"]
section = "10"
write-set = ["crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "docs/design/seat-heartbeat.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_evacuate_"]
size = "S"
growth = ["crates/scribe2/src/seat/tick.rs:30"]
depends = ["i"]
done = "(1) front は登録 row → 窓が shell か → 移動の周か（anchor が群 ∧ 記録の口座 ≠ row の口座・記録が読めない周は group-unreadable）の順で見て、移動の周 ∧ 窓が claude なら打刻と梯子を読まずに移動の門（§4 形 4 の規則そのまま）へ進む (2) 移動の門の関数は 1 本のままで呼ぶ場所が front の中へ移り、行 i の判定で移った周の呼び出しは §9 形 3 のまま (3) /exit は周期ごとに送ってよく Busy の周も止めず、記録は 1 送信 1 行 (4) 判定行は decision=move move=exit のままで、移動の周に reason=busy / state-stale は出ない (5) 移動の周でない席・群 0 の host・群の外の席・shell の周（awake）は 1 字も変わらない 歯: seat_tick_evacuate_ の歯が (k) 記録 ≠ row ∧ 前面 claude ∧ 新しい Busy ∧ 空の入力欄で /exit 1 行と move=exit（base では noop busy ＝ RED）(l) Busy が stale より古い同じ席で /exit 1 行（base では state-stale ＝ RED）(m) 記録 = row ∧ Busy で noop busy・0 key (n) 記録 ≠ row ∧ 字の在る入力欄で input-busy・0 key (o) 2 周で /exit 2 行を測る"
<!-- contracts:end -->
