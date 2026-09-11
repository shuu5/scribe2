# 設計: 席の状態（busy / idle）を hook の打刻で typed に持つ — pane の字面を判定入力から外す

- 要件: [FR27](../../design-intent/spec/srs.html#FR27) 打刻の合図 / [FR28](../../design-intent/spec/srs.html#FR28) cycle / [FR29](../../design-intent/spec/srs.html#FR29) 退避の合図 / [FR21](../../design-intent/spec/srs.html#FR21) 注入の記録
- 憲法: [C3.3](../../design-intent/spec/constitution.html#c3) 席の状態は typed enum・端末描画や自由文を判定入力にしない / [C2.2](../../design-intent/spec/constitution.html#c2) env を読まない / [C10](../../design-intent/spec/constitution.html#c10) 測定値は出所付き / [C11.2](../../design-intent/spec/constitution.html#c11) 極性
- 決定: [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html)（状態の出所は hook の打刻・target は生成 hooks.json の shell 行が渡す pane id から解く・打刻が無い周は注入しない）。[seat-autonomy.md](./seat-autonomy.md) §3 の裁定 (e)（pane 字面の idle 判定）を supersede する。
- この設計から出る契約: `s2-07l.95`（打刻 hook 2 本・状態 file・tick の一次ソース差替え）→ `s2-07l.112`（cycle の作り直しの証拠と inject の消費の証拠を打刻から取る・§6）。

## 1. 何を解くか

管理 tick は席が idle のときだけ 1 行を注入し、cycle は idle の席にだけ `/clear` を送る。いまの idle 判定（裁定 (e)）は pane の字面（prompt 行の右が空 ∧ 探索域に spinner の形が無い）で、Claude Code の版で印が変わるたびに壊れる（2026-09-11: `esc to interrupt` が消えて走行中の席を idle と読み `/clear` を送った・`s2-07l.94`）。字面の絞り込み（`.94` の (A)・`.96`）は露出を減らしたが、本文が印の形を持つ席を永久に busy と読む残余が残り、憲法 C3.3「端末描画や自由文を判定入力にしない」との緊張は解けていない。

やさしく言うと: 「席が手を動かしているか」を画面の見た目から推測するのをやめ、席の側が「いま始めた」「いま終わった」と自分で判子を押す。tick と cycle はその判子だけを見る。

## 2. 状態の型と出所

- `SeatState`（閉じた enum・2 値）: `Busy`（user の入力を受けて turn が走っている）/ `Idle`（turn が終わった・または session が始まった直後）。
- 出所 = Claude Code の hook event（席の中で器の binary が呼ばれる）:
  - `UserPromptSubmit` → `Busy`（注入された 1 行も user の入力として submit されるので、tick の pointer が消費された周もここで Busy になる）
  - `Stop` → `Idle`（turn の終端。`stop_hook_active` が真の再入は打刻しない）
  - `SessionStart` → `Idle`（作り直し・再開・/clear の後。既存の `session-start` hook に打刻を足す）
- 打刻の形: `<state_dir>/seat/<target>/state.jsonl` へ **1 行 JSON を append**（`schema` / `state` / `event` / `ts`〔UTC〕/ `sid`）。tick と cycle は**最終行**を読む。append-only は heartbeat / tick.jsonl と同じ store（`fleet::store::append_line`・lock 込み）を通す。
- 出所付き（C10）: 行の `event` が「どの hook から来た値か」を名乗る。tick の判定行は `state=<busy|idle|missing|unreadable|stale> event=<SessionStart|UserPromptSubmit|Stop|none>` を出す（`source=` は置き場の出所〔flag|git-config〕の既存 token ゆえ流用しない・planner 裁定 2026-09-11）。

## 3. target の解決（hook の中で自席を知る）

- hook は stdin JSON（`session_id` / `transcript_path` / `cwd`）で自分の session を知るが、**tmux の target（`session:window`）は知らない**。tick は target しか知らない。
- 解決 = 生成される `hooks/hooks.json` の shell 行で `--pane "$TMUX_PANE"` を渡し、core が `tmux display-message -p -t <pane> '#{session_name}:#{window_name}'` で target を解く。**core は env を読まない**（C2.2）——env に触れるのは生成された shell 行だけで、既存の `"${<NAME_UPPER>_BIN:-<NAME>}"` と同じ場所・同じ生成器（`cargo xtask gen-manifest`）である。`--pane` が空（tmux の外・`$TMUX_PANE` 未設定）なら打刻しない（黙る）。
- `--tmux-socket` は tick と同じ flag（歯は独立 socket で撃つ）。
- **運用の契約（実測 2026-09-11・tmux 3.6b）**: tick / cycle の `--target` は打刻が解く `session:window` と同じ字面でなければ同じ dir を見ない（両側とも `sanitize_target` で潰す）。window は **`-n` で明示して名付ける**（明示名は automatic-rename を off にする）。名無しの window は前景 process の名を取り、打刻の dir が席の一生の間に散る＝tick は永久に `state-missing`（fail-closed で静か）。writer と reader の dir の突合は doctor の主題（§6）。

## 4. tick / cycle の判定（typed 状態が一次・pane は判定入力にしない）

- tick の順序（[seat-autonomy.md §3](./seat-autonomy.md) の (c) を差し替える）: 自席の未 consumed 退避物の走査 → **状態**（state.jsonl の最終行・typed）→ context → 未 consumed WM → cycle lock → 打刻の合図の brake（tick-stamp・`pointer-recent`）。heartbeat の鮮度 gate は持たない（`s2-07l.109`・`.105` の「退避物が在る周は鮮度を飛ばす」特例も不要になった）。pane は **inject の送達確認**（prompt 行が在るか・目印が消費されたか）にだけ使い、idle の判定には使わない。
- **退避の合図は状態の門の外**（SRS FR29「idle を待たずに」> ADR-0015 §2.3・planner 裁定 2026-09-11）: context が cap 以上で自席の退避物が無く cycle が走っていない周は、打刻が Busy / missing / unreadable / stale でも退避の合図を送る（busy な席へは queue の形で届き次 turn で消費される・cap 以上の事実は打刻と独立に測れる）。状態の門が掛かるのは打刻の合図（pointer）と cycle だけ。
- 極性（fail-closed・注入しない側へ倒す）: 最終行が `Busy` → `noop reason=busy`／file が無い → `noop reason=state-missing`（hook が載っていない席・v1 の席）／読めない → `noop reason=state-unreadable`／`Busy` の `ts` が `seat.tick_stale_s` より古い → `noop reason=state-stale`（hook が死んだ疑い・**busy とも idle とも言わない**）。`Idle` だけが注入へ進む。`Idle` は鮮度を持たない（turn が終わった席は何時間経っても idle・hook が `Stop` の直後に死んだ席は門が開いたまま＝doctor の主題）。
- cycle も同じ 1 本の読み口（`SeatState` を返す関数 1 つ）を通す。字面判定の関数（`is_idle` と印の集合）は削除し、探索域・印の集合の記述は設計 doc から消す（列挙は機械が持たない側へ＝もう持たない）。
- **作り直しの証拠**（cycle・`s2-07l.112`）: `/clear` を送る前に打刻 file の行数（基線）と送達 ts を取り、基線より後ろに足された `SessionStart` の打刻（`ts >= 送達 ts`・同じ秒の hook を落とさない）だけを作り直しの証拠にする。pane の echo は読まない。窓（30 s）の内に来なければ `clear-unconfirmed`（復元を送らない・既存の語）。
- **送達の証拠**（inject・`s2-07l.112`）: 目印が pane に現れた＝送達（`.90` 不変・rc 0）。`consumed=` は送達 ts 以後の `UserPromptSubmit` の打刻で決める: 在れば `true`／file は読めるが窓の内に無ければ `false`（queue・次の submit で消費）／file が無い・読めない・置き場が解けない周は `unknown` に `reason=` を添える（測れない・消費と読み替えない・C10）。

## 5. 極性一覧との関係

- 打刻 hook（UserPromptSubmit / Stop / SessionStart の打刻）は**行為を止めない**ので guard ではない（[polarity.md §2](./polarity.md) の定義）＝極性一覧に載せない。打刻に失敗しても席は止めない（stdout 0 byte・rc 0）。
- tick の「状態が無い・読めない・stale なら注入しない」は tick の判定（行為を止める側）で、既存の `noop` 語彙の内側。

## 6. 証拠の出所（`s2-07l.112` で本文化）と後続

- **作り直しの証拠**: cycle は `/clear` 送達 ts の後に足された `SessionStart` の打刻が在ることを作り直しの証拠にする（`.96` の残余 (1)〔前の echo が見えたまま今回の `/clear` が消費されない周〕を畳む）。読み口は `state::evidence_after`（基線 + 送達 ts）の 1 本で、inject と共用する（§4）。
- **送達の証拠**: inject の `consumed=` は「入力欄が空になった」でなく「送達 ts の後に `UserPromptSubmit` の打刻が在る」で決める（`.97` lens MEDIUM-2 の残余）。値は**閉じた 3 値の enum**（planner 裁定 2026-09-12・文字列で持たない）: `true` = 送達 ts 以後の `UserPromptSubmit` の打刻が在る（消費した）／`false` = 打刻 file は読めるが窓の内に新しい打刻が無い（queue・次の submit で消費される・**送達の成功であって失敗ではない**＝tick は自打刻し再送しない）／`unknown` = 測れない（`reason=state-missing`〔file が無い＝hook 不在〕/ `state-unreadable`〔読めない〕/ `state-dir`〔置き場が解けない〕を添える・消費と読み替えない）。送達そのもの（rc 0）は目印の出現で決め、`.90` の裁定は不変。`false` は「窓の内に打刻が来なかった」であって「消費されなかった」ではない（既定 2 s の窓は hook の遅さも測る・cycle は 30 s）。基線を読めない周は証拠を採らない（`unknown reason=state-unreadable`・0 行に潰さない）。cycle の復元が queue（`false`）のままの周は `restore-unconfirmed` と記録する——これは「まだ確認できない」の typed な語であって失敗ではなく（planner 裁定 2026-09-12）、再送は cycle-stamp の back-off（`s2-07l.110`）が塞ぐ。
- **後続（別契約）**: 打刻の `sid` と heartbeat の突合（同じ target に別 sid の打刻が混ざる周の検出）は doctor の主題。

## 7. 却下案

- **字面の絞り込みを続ける**（`.94` (A)・`.96`）: 版ごとに再発し、C3.3 の緊張が残る。
- **`tool_input.command` / prompt 文から target を読む**: 自由文を判定入力にする（C3.3）。
- **`/proc/<pid>/fd` から transcript を辿って sid を得る**: OS 依存・fd が開いている保証が無い。
- **transcript の mtime で busy を推定**: 時間依存の ad-hoc 判定（憲法の順位 1 位・ad-hoc 禁止）。

## 8. 歯（契約 `s2-07l.95`・接頭辞 `seat_state_` / 既存 `seat_tick_` の更新）

- hook: 3 event の打刻が state.jsonl に typed で残る（fixture の stdin JSON と `--pane`・独立 socket の tmux で target を解く）／`--pane` 無しは黙る／`stop_hook_active` の再入は打刻しない。
- tick: 最終行 Busy → busy／Idle → 注入／missing・unreadable・stale の 3 形は注入しない（fail-closed・理由が typed）／pane の spinner 字面だけを置いた席は **判定に効かない**（負例＝字面を読んでいない証拠）。
- gen-manifest: hooks.json に 2 entry が増え、SessionStart の command 行に `--pane "$TMUX_PANE"` が付く（idempotent の歯と外形が動く）。
