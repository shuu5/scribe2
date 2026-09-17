# 設計: seat autonomy — 開発 session（planner / 管理席）の context 計測・cap guard・管理 tick・cycle を器に入れる

- 要件: [FR25](../../design-intent/spec/srs.html#FR25) 開発 session の context 計測 / [FR26](../../design-intent/spec/srs.html#FR26) cap guard / [FR27](../../design-intent/spec/srs.html#FR27) 打刻の合図 / [FR29](../../design-intent/spec/srs.html#FR29) 退避の合図 / [FR28](../../design-intent/spec/srs.html#FR28) cycle / [AC9](../../design-intent/spec/srs.html#AC9)・既存 [FR21](../../design-intent/spec/srs.html#FR21) / [FR23](../../design-intent/spec/srs.html#FR23)
- 憲法: [C3](../../design-intent/spec/constitution.html#c3) 開発 session の状態 / [C10](../../design-intent/spec/constitution.html#c10) 実測値の型 / [A1](../../design-intent/spec/constitution.html#a1) 3 クラス（v1 script の停止は「消す」）
- この設計から出る契約: SRS 便（本 doc と同 PR）→ meter + inject → cap guard → tick + cycle → 併走 1 日の切替便（後続は末尾）。
- 位置づけ: **器（runner）の機能ではなく、器の開発 session の機能**。runner は便ごとに headless claude を起動して終わる（CON6）ので不要。要るのは長生きする対話 session = planner / admin の 2 つの開発 sessionだけ。

## 1. v1 が担っているもの（実読 2026-09-10・置き換え対象の実体）
| 機能 | v1 の実体 | 判定 / 動作 |
|---|---|---|
| (a) context 計測 | `session-context-meter.py`（read-only） | tmux statusline の「NN% XXXk/YM」を parse（pane）／transcript jsonl の末尾 10MiB から算出（jsonl）。出力 1 行 `used_pct= used_tokens= window_tokens= source= sid=` |
| (b) 閾値通知 + work-tool deny | tick prompt (C) の `[fleet-cap]` 注入・`hooks/context-cap-guard.py`（PreToolUse） | 60% 超で退避を促し、編集 tool を deny（退避 skill だけ通す） |
| (c) 周期 tick | `scribe-tick-wake.timer`（systemd user・5 分）→ `scribe-tick-wake.py` | AND 4 条件（heartbeat stale ∧ pane idle ∧ 未 consumed WM 0 ∧ cycle marker 不在 or residue）で tick pointer 1 行を inject |
| (d) cycle 実行 | `scribe-cycle-kick.py` → `scribe-self-cycle-inject.py`（detach） | 退避後に lock を取り `/clear` → `/rebrief` を打つ。二重起動は `<marker>.lock` で排他 |
| (e) 退避 / 復元 | global skill ready-compaction / rebrief（実体は v1 の凍結 script） | WM = `working-memory.<sid>.md`・消費は `.consumed.md` へ mv（= SRS FR23 が既に固定） |
| 注入の口 | `scribe-inject.sh`（tmux send-keys + 送達確認） | rc 0 / 4 / 7 の 3 値（4 / 7 は偽陰性） |

v2 に既に在るもの: FR23（WM の規則）・FR21（`<state_dir>/inject.jsonl` = hook の消費記録）・C3（seat 状態は typed enum・fleet DB）。**無いもの: tmux を触る口（capture / send-keys）が 1 つも無い**（実測 grep 0）。

## 2. 要件（SRS v0.2 が正本・ここには写さない）
- FR25 開発 session の context 計測（event 型・N-3）/ FR26 cap guard（unwanted 型・N-1 / N-2）/ FR27 打刻の合図（state 型・N-1 / N-3・上限未満か測れない周 ∧ idle）/ FR29 退避の合図（state 型・N-1 / N-2・上限以上なら idle を待たない・FR27 と排他）/ FR28 cycle（event 型・N-1 / N-3）/ AC9（2 つの開発 session で 1 日同じ判定 + cycle 完走 1 回）。文面は `design-intent/spec/srs.html` の当該行を読む。
- 本設計はその HOW（subcommand・置き場・条件の評価順・lock）だけを決める。

## 3. 設計（subcommand `scribe2 seat …`・std のみ・env を読まない C2.2）
- `seat meter --target <tmux target> [--sid S]` … (a) の port。**transcript が名指された周は transcript・名指されない周は pane**（出所は入力で決まる 1 本道・s2-07l.75）。**空文字や空白だけの `--transcript` は「渡していない」と同じ**に扱う（渡し忘れを `unreadable` に化けさせない・guard の項と同じ読み）。Measured 型で返す（C10）。
- `seat statusline` … pane の経路の**入力の字面の正本**（ADR-0029 §2.1 / §2.2・台帳 `s2-07l.320`）。stdin の Claude Code の statusline payload（JSON）だけから 1 行を組んで stdout へ書く pure な関数 1 本で、file・env・子 process を読まない。先頭の segment は meter の `parse_statusline` の逆像（`<pct>% <used>[kM]/<window>[kM]`）で、続く segment は model・effort・口座の窓（`5h:` / `7d:`）の順（閉じた enum の宣言順）。値が無い segment は描かない（`0%` に化けない・C10）。payload が JSON でない周は空行 + rc 0（計測は `pane-no-statusline` に落ちる）。色は付けない。描いた行を `parse_statusline` が読むと組んだ値が戻る往復を property で pin し、行の外形は snapshot で pin する。読み手は変えない（前の版の行も読める）。口座の設定へ書くのは口座の口（[account-lifecycle.md](./account-lifecycle.md) §3）。
- `seat guard` … hook `pre-tool-use` の内側に組み込む（新 hook は増やさない）。cap は manifest 行 `seat.context_cap_pct`（初期値 60・裁定 id 付き）。deny の極性は既存 guard と同じ fail-closed。**計測不能は deny しない**（context が読めないだけで編集を止めると開発 session が詰む＝理由を inject.jsonl に記録して allow）。
  - **計測の出所**: 使用 token は payload の `transcript_path` が名指す jsonl を便 2 の parse（末尾 10 MiB・最後の有効 usage 和・`seat::meter::used_from_transcript_pct`）で読む。分母は manifest 行 `seat.context_window_tokens` の**宣言値**で、pane の statusline は読まない（hook から tmux を呼ばない・憲法 C2.2）。使用率は整数の切り捨てで、`pct >= cap` を止める。
  - **通す口は 2 つ**（`SeatDecision::{Allow, Externalize}`・bool で持たない＝憲法 C11）。`Externalize` は `<root>/.claude-session/working-memory.*.md` **ちょうど 2 段**の編集で、使用率に関わらず通す（止めると席は退避すらできない・FR23）。口は狭く取る＝同じ dir でも別名の編集は上限に掛かる。判定は**字句と実体の 2 段**で、`working-memory.*.md` という名前の symlink が口の外を指していれば通さない（通す側の口を字句 1 段で持つと link 1 本で上限を越えられる）。**まだ無い退避物は link ではありえない**ので在るときだけ実体を見る＝これから作る周は通る。`transcript_path` の空文字は「渡されていない」と同じに扱う（`unreadable` に化けさせない）。
  - **測れない理由は 4 語で弁別する**（`no-transcript-path` / `unreadable` / `no-usage` / `no-rule`）。1 語へ潰すと「transcript を渡し忘れている」のか「usage の形が変わった」のかを記録から後で分けられない。対象 tool は write-set guard と**同じ集合**（`hook::guard::GUARDED`）を参照する＝`Bash` は見ない。
- `seat heartbeat --target T` … 裁定 (a) の口。席の**中**から打刻する（`<state_dir>/seat/<潰した target>/heartbeat`）。tick が注入する 1 行がこの打刻を促す＝「席が生きている」は**席が自分で打った**ことでしか測らない。置き場を anchor 配下の `.claude-session/` に取らないのは、v1 の timer と場所を分けて併走できるようにするためである。
- `seat tick --target T --wm-dir DIR` … 裁定 (c) の 4 条件を**順序固定の AND** で評価し、成立時だけ注入する。判定は enum `TickDecision::{Inject, Noop(NoopReason), Error(String)}` で持ち bool にしない（憲法 C11）。
  - **順序** = 退避物の走査 → 状態の読み（[seat-state.md](./seat-state.md)・打刻の最終行）→ pane 取得 → **context** → 状態の門（Idle だけが進む）→ 未 consumed WM → cycle lock → 打刻の合図の brake（tick-stamp）。**最初に立たなかった条件**を理由にする（`pane-missing` / `busy` / `state-missing` / `state-unreadable` / `state-stale` / `wm-unconsumed` / `wm-unreadable` / `cycle-live` / `cycle-recent` / `cycle-stamp-unreadable` / `pointer-recent`）。順序は load-bearing で、理由の字面は順序の証拠でもある＝入れ替えると同じ条件でも別の理由が出る。**heartbeat の鮮度 gate は持たない**（`s2-07l.109`）: 鮮度 gate は走行中の席の pane を字面で読んで誤判定するのを避ける門だったが、席の状態が hook の打刻（typed・裁定 (e) の supersede）になって pane を idle の判定入力にしなくなり、理由が消えた。残していた害は「打刻の直後に cap を超えた席が最大 `seat.tick_stale_s`（40 分）の間 退避の合図を受けない」盲点で（`s2-07l.105` は退避物の在る周だけ鮮度を飛ばす特例でその一部を塞いだ・user 直命 2026-09-11「流石に長すぎだろ」）、撤去で特例も要らなくなった。heartbeat / tick-stamp の file は「席が生きている」の記録として残す（`seat heartbeat` と合図の文面は不変・判定入力ではない）。
  - **context（`s2-07l.89`・SRS FR29 退避の合図〔v0.3: 開発 session の context 使用量が rules 行の上限以上で、自 session の未 consumed の退避物が無く、cycle が走っていない間、管理 tick が idle を待たずに退避の合図 1 行を注入する（使用量が測れない周・退避物が在る周・cycle が走っている周は注入しない）。FR27 打刻の合図は上限未満か測れない周に絞られ FR29 と排他〕/ FR25 の計測 / AC9）**: pane を取得した周は、その本文から context 使用率を読み（meter の `used_from_pane_pct`＝`seat meter` と同じ 1 本の口・transcript の path を tick へ写す seam は作らない〔C10.3〕）、cap（meter の `declared_cap`＝guard と同じ 1 本の口・rules 行 `seat.context_cap_pct`）**以上なら idle を待たずに退避の pointer 1 行**（`退避 tick: context <pct>% ≥ cap <cap>%・/ready-compaction で退避してください`・`kind=externalize`）を既存の inject 経路で注入し、打刻の pointer と同じく自打刻する（自打刻は打刻の合図の brake〔`pointer-recent`〕にだけ効き、退避の合図は cap 以上の間 次の周も送る・busy な席へは queue の形で届く〔`.90`〕）。context を idle の**前**に置くのは、cap を超える席は busy（lens 待ち・長い cargo）であり、busy を理由に noop すると誰にも止められず auto-compact に至るためである（guard は編集の瞬間にしか効かない・実インシデント 2026-09-11）。
    - **退避済みの席には注入しない**（planner 裁定 2026-09-11・livelock の補正）: 注入するのは**自席の未 consumed 退避物が 0 件と確認できた周だけ**。退避済みの席は `/clear` 前で cap 以上のままなので、退避物を見ずに注入すると毎周 pointer を重ねて cycle に一度も落ちない。`wm-unconsumed` は次の条件（idle → 退避物 → cycle）へ、`wm-unreadable` は「0 件」と読み替えず同じく次へ（注入しない側）。
    - **測れない周は `context=unmeasured reason=<meter の 1 語>`** を判定行に載せて**次の条件へ進む**（注入も停止もしない・AC9 条 3 と同じ極性）。cap の行が読めない周は guard と同じ `no-rule`。判定行は既存の `decision=… reason=…` に `context=<pct>` を足す（pane を取得した周だけ・rules 行が読めず判定に入らない周や pane を取れない周は載せない＝`cycle=` と同じ「評価していない」の印）。注入の行は `decision=inject target=… consumed=… kind=<pointer|externalize>`。閾値・rules 行は動かさない（C5）。
    - **cycle lock が live な周には注入しない**（排他は cycle と同じ 1 本の lock）: 作り直しの最中に行を queue しても、届く先は消えるか作り直された席である。
    - **残る側 = 盲点は tick の周期だけ**（`s2-07l.109`）: 毎周 状態と pane を読むので、打刻の直後に cap を超えた席も次の周（最大 1 周期）で退避の合図を受ける。cycle の入口（`wm-unconsumed` → cycle）も席が idle で lock が空いていれば次の周で開く（`.110` の back-off は別）。`--pointer` は打刻の促し（`kind=pointer`）だけを上書きし、実測値を運ぶ退避の行には掛からない。
  - **裁定 (d) 鮮度** = **撤去**（`s2-07l.109`・planner 裁定 2026-09-12 案 A）: `now − max(heartbeat mtime, tick-stamp mtime)` を閾値と比べる門は持たない。残すのは**打刻の合図の brake だけ**: 合図を注入した周は tick-stamp を打ち、その mtime が `seat.tick_stale_s` 未満の周は合図を送らない（`pointer-recent`・`.110` の `cycle-recent` と同型・閾値は共用・境界は未満・不在や読めない周は送る側）。brake が掛かるのは合図だけで、退避の合図と cycle の評価はその周も行う。heartbeat の mtime は判定入力にしない（FR27 の合図は席の生存の記録でなく「続きを進めろ」の促しで、その頻度は tick 自身の打刻で決める）。注入した直後の周が撃ち続ける自傷 storm はこの brake が塞ぐ。
  - **裁定 (e) idle** = supersede（ADR-0015・[seat-state.md](./seat-state.md)）: 席の busy / idle は hook の打刻（`<state_dir>/seat/<target>/state.jsonl` の最終行）で typed に持ち、pane の字面は判定入力にしない。探索域・印の集合は機械も設計 doc も持たない。
  - **裁定 (c) 自席の弁別** = 退避物の **file 名の sid ではなく frontmatter の `seat:` が target と一致**するもの（席の外から回る tick は sid を知らない）。名乗りを持たない退避物は自席のものと数えない——他席の退避物を根拠にすると、別の席の文脈で `/clear` を撃つことになる。dir が読めない周は `wm-unreadable` で、**0 件に潰さない**。
  - **裁定 (b) cycle の駆動** = `wm-unconsumed` で止まった周**に限り**、lock が空いていれば cycle を in-process で回し、判定行の末尾に `cycle=<done|failed|refused reason=…>` を足す。それ以外の周は cycle を評価しない（`cycle=` が付かないこと自体が「評価していない」の印）。退避側の自己 kick は持たない＝退避から作り直しまでの遅れは最大 1 周期で、MVP はこれを受容する。 **back-off**（`s2-07l.110`・裁定 (a)）: cycle が lock を取れた周（結果が done / failed / refused のいずれでも・tick からでも `seat cycle` からでも）は lock の内側・`/clear` より先に `<state_dir>/seat/<潰した target>/cycle-stamp` を打ち（write-ahead・打てない周は 1 key も送らず `cycle-stamp-unwritable` で断る・lock を取れない周〔lock-held / state-dir / no-rule〕は打たない）、tick は同じ席を打刻から `seat.tick_stale_s` 未満の間 cycle を評価しない（`cycle-recent`・見送った周は打ち直さない・閾値は鮮度と共用＝新しい rules 行を足さない・経過が閾値ちょうどの周は評価する）。back-off を読むのは tick の側だけで、`seat cycle` を手で回す口は見ない（人の判断・mtime が未来の stamp は経過 0＝評価しない側）。`/clear` は不可逆の口（N1）で、復元されない退避物へ周期ごとに繰り返してはならない。stamp を読めない周は「無い」に読み替えず評価しない（`cycle-stamp-unreadable`・読めないことを理由に不可逆の側へ倒さない）。判定行には `cycle-stamp=<経過秒|none|unreadable>` を `cycle=` と状態の列（`state= event=`・先に land した `.95`）の後ろ・置き場の出所の前に足す（cycle の評価まで進んだ周だけ＝評価した・見送った・読めなかった・並びは planner 裁定 2026-09-12「先に land した側が前」）。
  - 実行系が回らない周（置き場を解けない・注入を確認できない・宣言 rule が読めない）は `decision=error` と rc 1 にし、**noop の語彙を汚さない**（席が静かなのか機械が壊れているのかを記録から読めるようにする）。注入の断り（`busy` 等）は noop と字が重なるので `inject-` の前置きで分ける。
- `seat cycle --target T --wm-dir DIR [--restore CMD]` … 不可逆の口。lock は `<state_dir>/seat/<潰した target>/cycle.lock`（`O_EXCL`・中身は pid と deadline の 1 行 JSON）。手順は順序固定: lock を取る → 自席の未 consumed 退避物が在ることを確認 → 打刻の最終行が Idle であることを確認（[seat-state.md](./seat-state.md)・同じ読み口）→ 送る直前の入力欄の門（注入と同じ `guard_input`・`input-busy` / `input-unknown`） → `/clear` 注入 → 作り直しの確認 → 復元 command（既定 `/rebrief`）注入 → lock 解除。**lock-held / wm-missing / wm-unreadable / busy / state-missing / state-unreadable / state-stale / pane-missing / input-busy / input-unknown の周は 1 key も送らずに rc 1**（憲法 CON5 / SRS FR28）。TTL（rules 行 `seat.cycle_lock_ttl_s`）を超えた lock は residue として取り直す。確認の上限と周期も rules 行（`seat.cycle_settle_s` / `seat.cycle_poll_ms`・下の bullet）で、`[--rules PATH]` はその 2 行を差し替える。
  - **`/clear` の送達確認だけは便 2 の settle を使えない**: `/clear` は pane を消すので「送った字面が現れる」形では測れず、**成功したときほど確認が落ちる**。代わりに**作り直しの正の証拠**「入力欄が空 ∧ 入力行より上に**消費済みの echo `❯ /clear`**（行頭・右は `/clear` ちょうど）が在る」を上限まで周期ごとに見る（便 2 の裁定「送った字面が現れた = 送達成功・入力欄が空 = 消費」と同じ形・`s2-07l.96`）。作り直された席は画面を消した後この echo を新しい prompt の直上に**必ず**残す（実測 2026-09-11: banner 3 行 → `❯ /clear` → 空の prompt → statusline）。
    - **確認の上限と周期は rules 行**（`seat.cycle_settle_s` = 30 / `seat.cycle_poll_ms` = 500・裁定 id 付き・`s2-07l.151`）: 値は運用値（席の hook の重さで決まる）ゆえ code に焼かず manifest が持ち、行が無い・不発効の周は `no-rule` で **1 key も送らずに**断る（fail-closed）。復元の確認（次の bullet）も同じ 1 つの値を使う。`seat cycle` / `seat tick` の `--rules PATH`（`rules` subcommand と同じ口）はこの 2 行だけを差し替える seam で、歯は短い上限の fixture で同じ分岐を測る（確認できない周の歯が 1 本 30 秒待つのを止める）。
    - **負の形（「探索域に `/clear` の字面が無い」）は採らない**: 域を prompt より下に取ると echo が域の外に落ちて第 2 項が構造的にほぼ常に真になり（確認が 500 ms の sleep に化ける）、末尾の固定行数（旧 6 行）でも statusline 3 行 + 区切り 2 行 + prompt 行で尽きて同じ形に化ける（`s2-07l.94`）。域を prompt 基準（上の非空 6 行）にすると今度は echo が**必ず域の内に在る**ので第 2 項が偽のまま固定され、席が 6 行以上を出力するまで真にならない——その出力は復元を送った後にしか出ないので、`/clear` が通った席に復元を送らず空席のまま残す（実測 2026-09-11 admin 席・`.94` で極性が反転）。負の形は域の取り方のどちら側でも壊れる＝証拠は正の形で持つ。
    - **未 submit の `/clear` は入力行そのものに残る**（`input_tail` が非空）ので、echo と同じ字面でも弁別できる（実測 2026-09-11 planner 席）。cycle の入口の入力欄の門でも同じ pane は `input-busy` と断られ、`/clear` を重ねて送らない。本文が echo を**引用**した行は 2 桁字下げで描かれ行頭に来ないので、行頭の条件が引用を除外する。
    - **Enter だけが落ちた周の修復（tick の注入・`s2-07l.150`）**: 送達した字面が入力欄に残ったまま消費の打刻が来ない周は、**queue に落ちた ∧ 打刻が Idle ∧ 入力欄が自分の目印を含む**の 3 つが同時に立つときだけ（pure な門 `inject::repair_of`）、Enter だけを 1 回送り直す（text は再送しない＝二重投函にならない・窓は送達確認と同じ既定＝新しい rules 行を作らない）。送り直しの後に消費の打刻が来れば `consumed=true`、来なければ `consumed=false reason=enter-lost` で、tick は **tick-stamp を打たない**＝次の周の判定がそれを測れる（入力欄に目印が残る席は入力欄の門が `inject-busy` で断り、`pointer-recent` の黙った noop にならない）。目印を条件に入れるのは、人が後から打ちかけた行へ Enter を押して他人の下書きを submit しないため。**入力欄の門は緩めない**（fail-closed・`inject` の極性は不変）: 修復は同じ周の中で閉じ、閉じない周は次の周が名乗る。再注入までは主張しない。cycle の `/clear` と復元は修復しない（判定は不変）。**残余**: 送信形の A/B（bracketed paste 等）は 50 回規模の実測を要し歯にできない。入力欄の門を「器自身の目印と一致する周だけ通す」形に緩める案は極性を変える（C16.2 の in-loop 一覧の再確認を要す）ので採らない。
    - **域は入力行より上の全行**（裁定 (e) の上 6 非空行に絞らない）: `/clear` は画面を消すので見えている echo は**何らかの** `/clear` の後に描かれたものに限られ、遡る数で古い echo を除外する必要が無い。6 行で切ると echo の下に描かれる行（hook の出力等）が版で増えた周に同じ行き止まりへ戻る（実測 2026-09-11: 実席は echo の周りに自動更新の告知 2 行 + hook 2 行を描いた）。
    - **残余（字面の正の形では塞げない・根治は `s2-07l.95` の typed 打刻）**: (1) 前の `/clear` の echo が見えたまま今回の `/clear` だけが消費されなかった周（字面が落ちて Enter だけが通る等）は、送る前と同じ pane を「済んだ」と読む。作り直し済みで復元の届いていない空席と形が同じなので、送る前の形で弁別すると空席を永久に回復できない＝弁別しない側に倒す（席は退避済み ∧ idle ゆえ害は「会話が生きたまま復元が走る」まで）。(2) 入力行より下に prompt の字を含む行が在ると anchor が移り未 submit の入力行が echo に見える（`input_tail` と同根・現行 statusline には無い）。(3) idle 判定と送信の間に席が turn を始めた周は `/clear` が queue され、turn の終わりに復元なしで発火する（次の tick が `wm-unconsumed` → cycle で回復する）。復元 command の側は便 2 の送達確認を**作り直しの確認と同じ上限まで延ばして**使う（次の bullet・`s2-07l.97`）。
  - **復元の送達確認は便 2 の settle を作り直しの確認と同じ上限まで延ばして使う**（`s2-07l.97`）: 成功の形は不変（送った字面が現れた ∧ 入力欄が空 = `Consumed`）で、変えるのは窓の長さだけ。作り直し直後の席は SessionStart hook（bd prime・lint・fetch＝数秒〜十数秒）の間、注入された復元を入力欄に queue したまま turn を始めないので、inject の既定 2 s の窓では**復元が正しく届く周ほど** `Queued` で終わり `restore-unconfirmed` になった（実測 2026-09-11 `.96` A/B: `/rebrief` は着地して rebrief が走ったのに記録は failed＝測った値が真の値と食い違う・C10）。席が queue を消費して入力欄が空になるまで作り直しの確認と同じ上限（rules 行 `seat.cycle_settle_s`）の内で見続け、上限の後も残っていれば従来どおり失敗（submit されなかった打鍵は上限まで入力欄に残るので lens-90 HIGH-2 の弁別は保たれる）。**上限を持つのは cycle の側 1 つ**で、inject は渡された窓で見るだけ（この面は規則を読まない・`s2-07l.151`）。`seat inject` の CLI の外形（2 s 既定・3 値・stdout 行・記録 schema）は変えない。**待つ時間が延びるのは失敗側だけ**（黙った席・置き去りの席は 2 s でなく上限の後に失敗する）。
  - **lock を取れなかった理由は 2 つに分ける**（`lock-held` = live な lock が在る / `state-dir` = 置き場が使えない）。置き場の位置に file が在る周は `create_dir_all` が競合と同じ error kind（`AlreadyExists`）を返すので、まとめると「他の cycle が走っていた」と「書けない」を記録から分けられない。
  - **表示の `target=` は置き場の dir 名と同じ潰した字面**（`a:b` → `a_b`）にする。便 2 の `seat inject` と語彙を 1 つにするためで、契約の `<T>` からの意図した読み替えである。
- `seat inject --target T --text …` … tmux `send-keys` + 送達確認（pane の末尾に text が現れたか）。rc は 0 / 1 の 2 値にし、v1 の偽陰性（4 / 7）を作らない。
- 駆動: systemd user timer は **repo に入れない**（起動コマンドを repo に置かない・CLAUDE.md）。unit の雛形は §8 に書き、user の host で有効化する。
- 記録: 判定と注入は `<state_dir>/seat/<target>/tick.jsonl` に 1 行ずつ（FR21 と同じ schema＝[vessel-hook.md §6](./vessel-hook.md) の `InjectionRecord`）。書き手は tick / inject / cycle の 3 面で、どの行も `seat`（潰した target・潰して空になる周は `null`）と `ts`（1970 年からの秒・UTC・`state.jsonl` の打刻と同じ時計）を持つ（`s2-07l.150`・schema は 1 のまま）。同じ file に 3 種の `who` が混ざるので、席と時刻は行の側で弁別できる。tick の判定行は `consumed=<値>` の直後に、測れない・消費されなかった理由を `reason=<語>` で足す（`seat inject` の行と同じ並び・`enter-lost` / `state-missing` / `state-unreadable` / `state-dir`）。
- 計測できない理由の弁別（便 2 の実装で決めた読み）: statusline の候補が無い周は、pane 本文が空なら `no-source`・本文が在れば `pane-no-statusline` に分ける（どちらの弁別も **transcript を渡さない周だけ**の話で、明示された周は pane を読まずに jsonl で測る・s2-07l.75）。健全性を外れた候補は `pane-out-of-bound` で**不成立のまま**とし、別の出所で塗り直さない（壊れた面を他の値で隠さない）。**この 3 語はいずれも pane を読む周＝transcript を渡さない周にしか出ない**（渡した周は pane を読まないので到達しない）。
- 歯の置き場（`s2-07l.327`・[seat-roles.md](./seat-roles.md) §7 が正本）: seat の外形 snapshot は**面ごとに 1 file**（usage / rebrief の DATA / doctor の末尾＝`seat_usage_external_form` / `seat_rebrief_external_form` / `seat_doctor_external_form`・1 本に連結しない）、`tests/e2e/seat/` の歯の file は**接頭辞（責務）ごと**に 1 file。面を触る契約だけがその面の file に当たる形にし、seat 面の契約が snapshot 1 file で全部交差するのを避ける（pipe の外形と同じ割り方）。

## 4. 憲法・制約との整合
- R-E12（常駐席は event 駆動で周期起動を張らない）: 管理 tick は席の**外**（host の timer）で回し、席には event（注入）としてしか届かない＝両立。R-G20（管理系の席の watchdog と別口座 respawn）は本設計の tick + cycle が受け皿。
- C3.3（状態は型で運び端末の見た目を判定に使わない）: 判定対象は seat state ではなく context 使用量で、出所（pane / transcript）は Provenance として Measured 型で運ぶ（C10）。端末描画は出所の 1 つであって seat state の判定入力ではない。
- CON4（v1 の役割席を持ち込まない）: 開発 session の**役割**は増やさない。planner / admin は既存の 2 つの開発 session で、v2 が持つのは「開発 session を生かす機械」だけ。
- CON5（不可逆の口を持たない）: `/clear` は session を作り直す＝退避済み WM が在るときだけ撃つ（FR23 の「DATA が無い周は brief を出さない」と対）。lock 無しでは撃たない。
- N（依存）: tmux は外部 process 呼出しで、crate 依存は増えない（A3 非該当）。
- C4（行数）: seat は新 module 1 本 + hook に guard 1 段。

## 5. 便の割り（契約は planner・実装は admin）
1. SRS 便（FR25〜27 + AC・user が /folio-architect を打つ）+ 本設計 doc の land。
2. `seat meter` + `seat inject`（歯: fixture の statusline 文字列 / jsonl から used_pct・send-keys は tmux 独立 socket で実測）。
3. `seat guard`（hook 内・manifest 行の追加は裁定 id 付き）。
4. `seat tick` + `seat cycle`（歯: 4 条件の真理値表・lock の排他・WM 不在では /clear を撃たない）。
5. 切替便: 開発 session へ器を載せる面を揃える——`marketplace.json` を gen-manifest の生成物に足し（手書きしない・冪等）、導入と切替の手順を §9 に書く。そのうえで v1 timer と**併走 1 日**→ 判定一致を tick.jsonl で示す → user 裁定で v1 timer を stop（消す = A1・user 手番）。host での binary の install は §9 (a)・timer の書きと有効化は器の口（§8・ADR-0030）・v1 停止は**器の外**（user の手番・消す）である。

## 6. 却下案
- (i) in-session の CronCreate だけで tick を回す: respawn / 口座切替で死に、死んだことを誰も検知しない（v1 の case A 裁定と同じ理由）。
- (ii) v1 script をそのまま plugin として積む: user 直命（v1 を開発に積まない）に反し、python 実体の保守が続く。
- (iii) context 上限を契約の大きさだけで受ける（CON6 の拡大解釈）: runner には正しいが開発 session には効かない。

## 7. 裁定済みの論点
- 計測の一次ソース = **transcript が名指された周は transcript・名指されない周は pane**（guard は hook の中で pane を持てない〔C2.2〕ので、pane 一次のままだと 2 面が同じ瞬間に違う値を返す。実測 2026-09-11・s2-07l.75: 両出所は同じ量を見ており差は平均 0.6% だが、pane は statusline の 1k 刻みの階段・transcript は 1 token 粒度の連続で、境界の判定は細かい側が安全）。**NFR5 は満たす**——transcript は末尾 10 MiB しか読まず、11.1 MiB の file を渡した実測で 1 回 0.01 秒（debug binary・3 回とも同値。上限は 2.0 秒）。
- cap の初期値 = 60（user 裁定 2026-09-10・manifest 行 `seat.context_cap_pct` に裁定 id 付きで置く・C4 / C13 の閾値ではないので A2 非該当）。
- 切替の判定 = 併走 1 日 + cycle 完走 1 回（AC9）。v1 の timer を止めるのは user 手番（A1「消す」）。

## 8. 駆動（systemd user の unit は**器が導出して host へ書く**・ADR-0030・台帳 `s2-07l.321`・**unit は repo に入れない**）

管理 tick は席の**外**から回る（憲法 R-E12）。unit は host 固有の値（path・target 名）を持つので repo には
入れない（本 repo は PUBLIC）が、その中身は**器の口が 1 関数で導出して host の user unit dir へ書く**
（ADR-0030 §2.1・手書きの雛形を写す形は ADR-0030 §5 (A) で却下）。

- **口**: `<NAME> seat tick install --state-dir S --target S:W --wm-dir D --unit-dir U --binary PATH [--rules PATH]`。
  置き場・binary・unit dir は全部引数で受ける（器は env・home・自分の実行 file の場所を読まない・C2.2・
  `current_exe` は env-reads の禁止集合）。`--unit-dir` と `--binary` は shell が解いて渡す（home の展開や `command -v`）。
- **導出**（pure な 1 関数 `derive_units`・入力 = NAME・target・state dir・wm dir・binary・間隔）: file 名は
  `<NAME>-seat-tick-<潰した target>.service` / `.timer`（潰し方は打刻の dir と同じ 1 関数・template unit と `%i` は
  使わない）。service = `Type=oneshot` + `ExecStart=<binary> seat tick --target <target> --wm-dir <wm dir> --state-dir <state dir> [--rules <path>]`
  （`Environment=` / `WorkingDirectory=` / `%h` を持たない）。timer = `OnBootSec=<n>s` + `OnUnitActiveSec=<n>s` +
  `Persistent=false` + `WantedBy=timers.target`（単調時計・`OnCalendar` の割り算は使わない）。外形は snapshot で pin する（C12.5）。
- **間隔**: rules 行 `seat.tick_interval_s`（秒・裁定 id = user 2026-09-15T02:30Z・C5）。読めない周は `no-rule` で断る（既定値に倒さない）。
  周期を短くすると退避から作り直しまでの遅れが縮む（cycle の駆動は tick に載っている）。退避の合図の再送は
  `seat.signal_backoff_s` の brake が抑える（`s2-07l.315`）ので、周期を縮めても queue に溜まらない。
- **書き**: 一時 file → rename（部分書きを残さない・[account-lifecycle.md](./account-lifecycle.md) §3 の host.toml と同じ形）。
  既存 file は導出の結果と bytes で比べ、一致 → `unchanged`（有効化だけ撃つ）・不一致 → `unit-exists` で断る
  （人の手書きを上書きしない・N1）。有効化 = 子 process `systemctl --user daemon-reload` → `systemctl --user enable --now <timer>`
  （順序固定・timeout は既存の唯一の wait・失敗は `reload-failed` / `enable-failed` に rc を添える）。
  記録は inject の記録と同じ置き場に `kind=tick-install` の 1 行。
- **撤去**: `seat tick uninstall --state-dir S --target S:W --unit-dir U` = `disable --now` → 2 file を `<unit dir>/.retired/<name>.<ts>` へ mv
  （N1.2・削除しない・器の生成物でない file は `unit-foreign` で断る＝S の `kind=tick-install` の記録から導出し直した bytes と比べる・§11）。
- **承認**: 有効化・撤去は 3 クラス（消す / 出す / 使う）のいずれにも当たらない（ADR-0030 §2.4・user 裁定 2026-09-15T03:52Z 問 3）
  ＝器が撃つ。**v1 の timer は器が止めない**（器の管理物でない・切替は §5 の便 5 で user が裁定する）。
- **doctor**: `--unit-dir U` を渡した周だけ、登録 row のある席ごとに `tick-unit=<present|absent|foreign>`（値名は §11 と同じ 3 値・旧案の 4 値は §11 に寄せた）
  （bytes の一致・`absent` に潰さない・`systemctl` は呼ばない・`--unit-dir` 無しは `tick=-`）。
- target の window は `-n` で明示して名付ける（名無しの window は automatic-rename で前景 process の名になり、
  打刻の dir が席の一生の間に散る＝tick は永久に state-missing・[seat-state.md §3](./seat-state.md)）。

## 9. 導入と切替（手順の**形**だけ・host 固有の値は書かない）

本節は「どういう順で載せるか」を記す。**host の path・tmux target 名・口座名・起動コマンドは
書かない**（本 repo は PUBLIC・CLAUDE.md「やらないこと」）ので、`<NAME>` と `<...>` は user が
自分の host で埋める。

**(a) binary を PATH に置く。** `cargo install --path crates/<NAME>` の形で入れる。hook が呼ぶ
コマンドは生成物 `hooks/hooks.json` が `${<NAME_UPPER>_BIN:-<NAME>}` で解くので、別の場所に置く
なら env を先に立てる（器そのものは env を 1 つも読まない＝解くのは hook を起動する shell）。

**(b) 開発 session へ plugin を載せる。** 順に:

1. この repo を directory marketplace として **user scope で**登録する。**project scope で登録して
   はならない**——tracked の `.claude/settings.json` に host の絶対 path が書かれ、PUBLIC 面を汚す。
2. plugin を **project scope で** install する（`claude plugin install <NAME>@<NAME> --scope project`
   の 1 発で、`.claude/settings.json` の `enabledPlugins` まで入る＝手で足す手順は無い）。
3. **hook の読込元は plugin source（この repo の checkout）の `hooks/hooks.json`** であり、install 時に作られる
   cache の写し（`plugins/cache/<NAME>/<NAME>/<version>/`）は hook の読込に使われない（Claude Code 2.1.268・
   `--debug-file` の `Read hooks.json for plugin <NAME>` 行で実測 2026-09-12・bd `s2-07l.95` / `s2-07l.122`）。
   走行中の session は起動時の snapshot を持つので、**`/reload-plugins` か process の作り直しが要る**（hook は
   自動では反映されない）。`claude plugin update` は版が同じなら no-op で、hook の反映とは無関係。

**(b′) `hooks/hooks.json` を変える便の rollout（順序が要る）。** 打刻（[seat-state.md](./seat-state.md) の
`seat/<target>/state.jsonl`）の無い席には管理 tick が pointer / cycle を送らない（fail-closed）ので、順序を誤ると
その席は cycle されないまま context を使い切る。

1. 便を main に載せる（`hooks/hooks.json` は `cargo xtask gen-manifest` の生成物・手で書かない）。
2. 各席で `/reload-plugins`（turn の走行中は入力欄に queue されるので idle を待つか、注入の後に着地を実測する）。
3. **全席**の `state.jsonl` に新しい打刻が出るのを実測する（1 席でも欠ければ 2 を繰り返す）。
4. **その後に** binary を入れ替える（(a)）。先に入れ替えると、hook が古い席は打刻が無いまま cycle されない。

cache の写しは plugin source を丸ごと写す（`target/` や `.worktrees/` を含み大きくなりうる）が、hook の読込には
使われないので動作には影響しない。軽くするには plugin root を小さな dir に分けて marketplace の `source` を変える
（再 install が要る＝user 手番）。本 doc はそれを決めない。

**(c) 管理 tick の timer を有効にする。** §8 の雛形を user が埋めて有効化する（§8 のとおり unit は
repo に入れない）。

**(d) 併走して突き合わせ、user が切り替える。** v1 の timer と 1 日併走し、判定の一致を
`tick.jsonl` と v1 の log で突き合わせる（AC9）。**v1 timer の停止は user 裁定**（憲法 A1「消す」）
であり、器は自分で止めない。切替の条件そのものは本 doc でなく **bd `s2-07l.38` の notes** が持つ
（規範を doc へ写さない＝憲法 C1 / N2）。

## 10. 入力欄の門の 3 値と cycle-stamp の位置（契約表の行 a・`s2-07l.288`）

- 何が起きているか: 入力欄の門（`seat/inject.rs` の `guard_input`）は入力欄の残りが非空なら一律 Busy で、器自身の注入文が折り返して残っている周を弁別しない（呼び手は `seat/inject.rs` の deliver・`seat/cycle.rs`・`seat/tick/exit.rs` の 3 か所）。加えて `seat/cycle.rs` の perform は lock の直後・門の前に cycle-stamp を打つので、1 key も送らずに断った周も stamp が立ち `seat.tick_stale_s` の back-off に入る＝入力欄の文が消えない限り同じ拒否を繰り返し、席が cap 超えのまま止まる（admin 席 2026-09-14 18:45Z〜・.296 の (a) は Landed 済みで本 § は残りの (b)(c)）。
- 形: (1) 門の戻りを閉じた 3 値（Clear / OwnQueued / Foreign）にする。OwnQueued = 入力欄の残り（folded で畳んだ字面）が直近の自席注入の記録（同じ席の `tick.jsonl` の最後の who=seat-inject の record の what＝`seat inject` が書く payload の先頭 80 byte・`decision=inject …` の行は inject.jsonl の別の記録で本文を持たない）を folded で畳んだ字面に前方一致する周。3 値は `InputGate` に variant を足さず `guard_input` の戻りの新しい閉じた型で持つ（`InputGate` は shell の門 `shell_input_empty` も返し、その網羅 match は本 § の write-set 外）。Foreign（人間の打ちかけ）と UnknownInput は従来どおり 1 key も送らない（判定は器自身の記録との一致だけ・pane の字面の規則を散文に持たない・C3.3 / N2）。(2) OwnQueued の周は 3 呼び手とも Enter を 1 回だけ送って再確認する（text は再送しない・残れば 1 key も送らず断る）。その断りの字面は `input-own-queued` の 1 つ（tick の記録に載る attrib は既存の写像 1 本＝`seat/tick/render.rs` が deliver の断りを `inject-<reason>` へ写す 1 か所〔本 § の write-set 外・触らない〕を通るので、既存の `inject-busy` と同じ規則で `inject-input-own-queued` になる。`REASON_INPUT_*` の列に足し、3 呼び手がそれぞれの refused の reason として自分の断りの面に載せる: cycle と exit は tick.jsonl の record の `what`（`refused reason=…`）、`seat inject` は stdout の断りの 1 行と rc（既存の `Refused` の面・tick.jsonl には書かない）＝Foreign の `input-busy` と弁別できる）。歯は cycle / exit / inject の 3 経路それぞれに 1 本以上。(3) cycle-stamp は全部の門を通り /clear を送る直前に打つ（write-ahead の意図は保つ・lock は二重投函の防止で残す・打刻は back-off の根拠にだけ使う）。門で断った周は stamp を打たない。
- 触らない: tick の判定の列（exit.rs は門の呼出 1 か所だけ）・`seat.tick_stale_s` / `seat.cycle_ttl_s` の行・nudge_enter の中身・`InputGate` の variant と shell の門（write-set 外の網羅 match）。
- 却下: 入力欄の字面を「器の注入文らしい」で判定する（字面の規則が散文に生まれる・N2）／refused の周に lock も取らない（二重投函の防止が要る）／入力欄の文を器が消す（人間の打ちかけと弁別できない周に破壊的・N1）。

## 11. 管理 tick の systemd unit を器が導出して書く（契約表の行 c・`s2-07l.321`）

- 何が起きているか: 報告書「scribe2 — マルチアカウント管理の現状と対策（2026-09-15）」§6・§7 問 3 と user 裁定 2026-09-15T03:52Z（「３．よい」＝unit を器が書く起票の承認）を承け、ADR-0030 §2.1 は unit を器が導出して host へ書くと定める（手書きの雛形は同 §5 (A) で却下済み）。前提の `.315`（`seat.signal_backoff_s`）は Landed 済み。現物（verified）: `seat` の usage に `tick install` は無く、`systemctl` を撃つ口は既に `pipe/confine.rs` の `SYSTEMCTL` 定数に在るが、rules 行 `seat.tick_interval_s` は manifest に無い。
- 形: rules 行 `seat.tick_interval_s`（裁定 id = user 2026-09-15T02:30Z・`RuleKind` に variant 1 つ・値 60）を足す。口 `seat tick install --state-dir S --target S:W --wm-dir D --unit-dir U --binary PATH [--rules PATH]` と `seat tick uninstall --state-dir S --target S:W --unit-dir U` を `seat/cli.rs` に足す。導出は pure 関数 `derive_units`（新 module `crates/scribe2/src/seat/tick/install.rs`）で service / timer の 2 file 名と中身を組む（`Environment=` / `WorkingDirectory=` / `%h` を持たない）。書きは一時 file → rename、既存 file は bytes 一致なら `unchanged`、不一致は `unit-exists` で断る。有効化は `systemctl --user daemon-reload` → `enable --now`（順序固定）。撤去は先に U の 2 file を S の `kind=tick-install` の記録から導出し直した bytes と比べ、記録が無い／不一致は `unit-foreign` で断る（1 byte も動かさない・器の生成物でない file を退避しない）。一致なら `disable --now` の後に 2 file を `.retired/` へ mv（N1.2）。doctor に `tick-unit=<present|absent|foreign>` の行を足す（`--unit-dir U` を受けた周だけ・flag は `main.rs` の doctor の分岐・行は `account/mod.rs`）。判定の入力は `derive_units` と同じ（target・wm dir・binary・間隔・rules path）で、install はそれを `kind=tick-install` の記録の 1 行（state dir S・inject の記録と同じ置き場）に写し、doctor は S のその記録から読み戻して導出し直し、U の file と bytes で比べる: 器の名前の service が U に無い → `absent`・記録から導出した bytes と一致 → `present`・記録が無い／一致しない → `foreign`（`systemctl` は呼ばない・rc は変えない）。
- 触らない: `seat tick` の判定の列・timer 間隔以外の rules 行・`pipe/confine.rs` の `systemctl` の口の実装（呼ぶだけで可視性のみ変える）。
- 却下案: template unit と `%i`（target の潰し方が unit 名の規則と二重になる）／雛形 file を repo に置いて写す（host 固有の値が PUBLIC repo の tracked に入る・ADR-0030 §5 (A)）／間隔を引数の既定値で持つ（規則が code に散る・C1）。

## 12. tmux 呼出の上限 — rules 行 `seat.tmux_timeout_ms`（契約表の行 d・`s2-07l.409`）

- 何が起きているか（planner 席 2026-09-16 10:10Z〜11:10Z・verified）: 管理 tick の注入（`send-keys`）を撃った tmux client が返らず 59 分止まり、親の `seat tick` が待ち続けて timer の次の発火も止まった（unit が activating の間は OnCalendar が発火しない）。現物: `seat/mod.rs` の `tmux_stdout` / `tmux_ok` は `Command::output()`（上限なし）で、席の tmux 呼出（6 file・17 か所）は全部この 2 関数を通る。tick の他の待ち（`deliver_within` の窓・`tries_within`）は注入後の確認にだけ掛かり、tmux client 自体が返らない周を止める線が無い（憲法 C11.3）。
- 形: (1) rules 行 `seat.tmux_timeout_ms`（kind `SeatTmuxTimeoutMs`・Int・ms・値 5000・裁定 id = user 2026-09-16T12:09Z・C5）。(2) tmux を撃つ口を `seat/mod.rs` の **1 関数**（spawn → 上限つき wait・超えた周は kill して回収）に寄せ、`tmux_stdout` / `tmux_ok` はその関数を呼ぶ。失敗は閉じた型 `TmuxFail::{Spawn, Status, Timeout}`（FailClosed・極性一覧に載る＝境界の `POLARITY` は `seat/mod.rs` の型の隣に置き、`crates/scribe2/src/polarity.rs` の `Guard` の 4 つ組〔variant・`ALL`・網羅 match 2 本〕と `as_str` に 1 つ足す。snapshot はその結果であって登録元ではない）。上限は `socket` と対で運ぶ 1 つの値（handle 1 つ・呼び手はそれを渡す＝機械的な置換・判定の列は不変）で、rules 行の読みは `int_rule_of` の 1 関数で、handle を組む場所は口ごとに 1 つ＝`seat` の CLI 入口・`doctor`（`main.rs`・`--rules` で読んだ manifest・`seat/role.rs` の `doctor_lines` へ渡す）・`account add --target`（`account/mod.rs`・label の検査と同じ manifest・`deliver` へ渡す）・hook（`hook/mod.rs`・guard の rules 行と同じ manifest・`target_of_pane` へ渡す＝hook も pane から席を解くのに tmux を撃つ）の 4 つ（読み手は 1 つ・組む場所は socket を受け取る口と同じ）。呼び手の判定は従来どおり `None` / `false` の側に倒す（1 key も送らない側）。(3) 超過の周は記録に `reason=tmux-timeout` を残す（tick.jsonl の record の `what`・既存の `tmux-failed` と同じ面・C10）＝`Timeout` を名指せるのは `Result` を返す 1 関数を**直接**呼ぶ面だけなので、注入の送達（`seat/inject.rs` の `capture` と `send-keys` の 2 か所＝`Delivery::Refused` / `Unconfirmed` の面）はその関数を直接呼び、失敗の型の `Timeout` の variant を `tmux-timeout`・他の variant を既存の `tmux-failed` へ写す。`meter.rs` / `tick/exit.rs` / `cycle/*` が通る `tmux_stdout` / `tmux_ok` / `capture` / `pane_is_shell` は `Option` / `bool` のまま（判定の列は不変・上限は掛かる・理由は名指さない）。tick の呼出順（capture → 門 → send-keys）ゆえ、全部返らない tmux の下では注入より前の capture が先に上限で `None` に倒れる＝記録の reason は既存の字面（`pane-unreadable` 等）で、`tmux-timeout` が出るのは注入の面が最初に上限を踏んだ周。
- 触らない: tick の判定の列・注入の確認の窓（`seat.cycle_settle_s` / `seat.cycle_poll_ms`）・systemd unit の `TimeoutStartSec`（§11 の領分・別の線）・跨 host の relay（`.408`）。
- 歯（`seat_tmux_timeout_` 接頭辞・`tests/e2e/seat/tick.rs`・rules fixture の上限は短い値〔ms〕）: (a) PATH の先頭に置いた偽 `tmux`（`send-keys` だけ読んで返らず、`capture-pane` / `list-panes` / `display-message` は席の pane を模した固定の出力を返す script）の下で `seat tick` が返り、rc は従来の失敗側で記録に `reason=tmux-timeout` が在る（base は返らない → 歯側の deadline で RED）／(b) 全部返らない偽 tmux の下でも `seat tick` が返り、記録の reason は最初に上限を踏んだ面の既存の字面（`tmux-timeout` ではない）／(c) 上限内に返る偽 tmux は従来どおり通る／(d) rules 行は kind 件数の pin と外形の歯（`seat.tick_interval_s` と同型）。
- 却下: 呼び手 17 か所に個別の timeout（線が散る・C2）／thread で `output()` を包んで放置（器が自分の子を回収しない）／systemd の `TimeoutStartSec` だけで止める（unit を入れない環境で線が無い）。

## 13. e2e の fixture が自分の残骸を畳む — tmp dir を guard で消し、死んだ process の隔離 server と dir を次の fixture が掃く（契約表の行 e・`s2-07l.402`）

- 何が起きているか（planner の実測 2026-09-16 08:0xZ・verified）: e2e の fixture が立てる隔離 socket の tmux server が 31 本・`/tmp/e2e-*` の dir が 31,185 本残存し、user の tmux-continuum が「他の server が居る」と判定して自動保存を止めた。現物: `tests/e2e/main.rs` の `make_tmp_dir` は裸の `PathBuf` を返し、消すのは各歯の成功経路の `remove_dir_all` だけ（panic / timeout の経路は飛ぶ）。`tests/e2e/seat.rs` の `IsolatedSeat` は `Drop` で `kill-session` を撃つ（panic 経路は畳める）が、nextest の timeout（SIGKILL）で process ごと落ちた周は `Drop` 自体が走らず server も dir も残る。
- 形: (1) **死んだ持ち主の残骸を次の fixture が掃く**: `make_tmp_dir` は dir を作る前に `<temp_dir>/e2e-<pid>-*` を列挙し、`<pid>` が生きていない（`kill -0` 相当＝`/proc/<pid>` の有無・std だけ）entry だけを対象に、中に socket が在れば `tmux -S <sock> kill-server`（隔離 server・live server の socket ではない）を撃ってから `remove_dir_all` する。生きている pid の dir と自分の dir は触らない（並列の歯と競合しない）。隔離 socket は `tests/e2e/seat.rs` の `socket_of` が dir 直下の `sock` に置く（呼び手 4 か所とも `make_tmp_dir` の dir）ので、掃きは dir の中の `sock` で server に届く。(2) 対象は **fixture 自身が作った scratch**（`e2e-<pid>-` の接頭辞・`temp_dir` 直下）だけで、器の管理物（state dir・退避先）ではない＝N1 の対象外・A1 の「消す」にも当たらない（歯の一時物を歯が畳む）。(3) `make_tmp_dir` の戻りを `Drop` 付きの guard にする案（panic 経路でも自分の dir を消す）は、戻りの `PathBuf` を `tmp()` の包み 4 本（`seat.rs` / `hook.rs` / `headless.rs` / `pipe.rs`）と sub module 9 file の struct 欄・`(PathBuf, PathBuf)` の戻りが受けており閉包が 17 file に及ぶので、本 § から外して別の行にする（掃きが在れば残骸は次の歯で消える＝先に効く側を小さく通す）。
- 触らない: `make_tmp_dir` の戻りの型と呼び手（guard 化は別の行）・`IsolatedSeat` の `Drop`（kill-session）・fixture の socket の置き方（`socket_of`）・nextest の timeout の値・器の本体。
- 歯（`e2e_fixture_sweep_` 接頭辞・`tests/e2e/main.rs` の隣の新 file `tests/e2e/fixture.rs`）: (a) 死んだ pid（`sh -c true` を spawn して wait した pid）名義の `e2e-<pid>-x` dir に隔離 server を立て、**掃く前に server の pid**（`tmux -S <sock> display-message -p '#{pid}'`）を取っておき、`make_tmp_dir` を呼ぶと dir が無く **その pid が `/proc` に無い**（socket の消滅は dir の削除と融合するので server の生死は pid で測る・`kill-server` を撃たない実装は dir だけ消えて pid が残り落ちる・base は dir も server も残る → RED）／(b) 自分の pid 名義の dir は掃かれず、その server の pid は `/proc` に在る。
- 却下: `tempfile` crate（直接依存の追加・A3・`make_tmp_dir` の doc comment に既に却下の記録）／nextest の `leak-timeout` / 外部の掃除 script（歯の外の散文運用・C12）／fixture 作成時に全 `e2e-*` を消す（並列の歯の dir を壊す）／`kill-server` を生きている pid の server にも撃つ（同上）。

## 14. 打刻の合図の backoff — 無変化の席には合図の間隔を伸ばして最後は止め、変化で 40 分に戻す（契約表の行 f・`s2-07l.423`）

- 何が起きているか（folio2 planner の実測 2026-09-17・verified・`s2-07l.423`）: 打刻の合図（idle の席へ「管理 tick: heartbeat を撃ち、続きを進めてください」を注入する口・§3 (d)）の brake は **時間だけ**（tick-stamp の mtime が rules 行 `seat.tick_stale_s` 未満なら送らない・`pointer_recent`）で、席の状態が前回の合図から変わったかを見ない。承認待ちで 6 時間無変化の席に同じ合図が 40 分ごとに約 30 回届き、各回が 1 turn（heartbeat + 確認 + park の報告）を消費した。tick の周期（rules 行 `seat.tick_interval_s`・§11）は刻みが細かくても token を消費しない＝消費するのは合図に応える席の turn だけ。user 裁定 2026-09-17T00:55Z（逐語は台帳 `s2-07l.423` notes）: tick は 1 分・合図は基本 40 分・無変化なら間隔を伸ばして最後は止める。
- 形（判定入力は typed な状態だけ・席の描画や自由文は見ない・C3.3）:
  1. **変化の digest**: tick は合図を送る前に席の digest を測る。材料は 2 つ = 席の状態 log（`seat/<target>/state.jsonl`）の最終行の `ts`・置き場の fleet の event log（`fleet/events.jsonl`）の最終行の `ts`。context は入れない（合図に応える turn ごとに増えるので無変化の席でも毎回変わる・閾値は退避の合図が別に見る）。台帳は読まない（毎周 `bd` を子 process で撃つ費用を tick に持ち込まない・台帳の動きは席の turn になって状態 log に現れる）。digest は 2 値の並びの 1 行（順は宣言順・hash にしない＝読める形で残す・C10）。
  2. **梯子の記録**: 合図を送った周は `seat/<target>/pointer-digest` に 1 行 JSON（`sent_at` / `step` / `digest`）を書く（tick-stamp の隣・tick-stamp は「合図を送った」の印として残す＝読み手と極性は不変）。ただし送った周の `digest` は**基準にしない**（合図に応える席の turn〔heartbeat + 報告〕が状態 log を必ず 1 行進めるので、送出時の digest と比べると毎回「変化あり」になり梯子が登らない）。基準は **席が合図に応えて idle に戻った最初の周**（状態 log の最終行が `sent_at` より後の Stop）に取り、記録の `digest` に書く（settle）。settle の前の周は比べない（`pointer=settling`）。`sent_at` から `seat.tick_stale_s` を過ぎても settle しない周（席が応えなかった＝入力欄の門など）は、その周の digest を基準にする（席が応えない席へは梯子が登る側に倒す）。settle 後の各周は基準と今の digest を比べ、**違えば `step` = 0**（待ち = `seat.tick_stale_s`・40 分）、**同じなら `step` + 1** で待ち = `seat.tick_stale_s × seat.pointer_backoff_factor ^ step`（`sent_at` からの経過で測る）。待ちが `seat.pointer_backoff_max_s` を超える段は**送らない**（停止・`pointer=stopped`）。記録の読みは閉じた 3 値（記録なし／settle 前＝基準が無い／基準あり）で、記録が無い・読めない周は「記録なし」＝step = 0 で、**従来の brake（tick-stamp の mtime が `seat.tick_stale_s` 未満なら送らない）だけで判定する**（settle 待ちに落とさない・記録を書けない席へ tick の周期で合図が重ならない・fail-open は 40 分に合図 1 本だけ・N1 の面は無い）。この周の待ちの起点は tick-stamp の mtime（記録の `sent_at` は無い）。
  3. **止めるのは合図だけ**: 停止中も tick は毎周 digest を測り、変化した周に step = 0 へ戻して合図を送る。退避の合図（context の閾値・FR29）と cycle の評価（§3 (b)）は従来どおり毎周で、本節の梯子に掛からない（brake が掛かるのは打刻の合図だけ＝`s2-07l.109` の形は不変）。
  4. **判定行**: `pointer=<sent|settling|wait:<残り秒>|stopped> step=<n>` を tick の判定行に 1 語ずつ足す（既存の `reason=pointer-recent` は「待ち」と「settling」の両方に残し、停止は `NoopReason` の variant 1 つ `PointerStopped` を足す・C2）。2 語は **brake に届いた周の全部**に載る（記録の 3 値のどれでも・先に返る周〔busy・閾値以上・cycle・rules 行が読めない・退避物が残る〕には載らない）。`wait:<残り秒>` の起点は、基準ありの周 = 記録の `sent_at`・記録なしの周 = tick-stamp の mtime（待ちは `seat.tick_stale_s`・`step=0`）。記録なしで送った周は `pointer=sent step=0`。
  5. **rules 行**（C1・値は manifest・裁定 id = user 2026-09-17T00:55Z）: `seat.pointer_backoff_factor`（2）と `seat.pointer_backoff_max_s`（86400）。初段は既存の `seat.tick_stale_s`（2400）を流用し行を増やさない。梯子の実値 = 40 分 → 80 → 160 → 320（5.3 時間）→ 640（10.7 時間）→ 1280 分（21.3 時間）→ 停止（次段 2560 分 > 24 時間）。
- 触らない: 合図の文面（`default_pointer`）・注入の経路（`inject_line`・入力欄の門）・`seat.tick_stale_s` / `seat.signal_backoff_s` の値・退避の合図と cycle の順序・tick の周期（§11）。
- 歯（`seat_pointer_backoff_` 接頭辞・`tests/e2e/seat/tick.rs`・fixture は既存の tick の歯と同じ〔偽 tmux + 席 dir + rules の写し〕で、時刻は記録の `sent_at` を過去に書いて進める）: (a) 1 周目で合図が出て記録が書かれ、席の応答（状態 log に `sent_at` より後の Stop を 1 行）の後の周で settle し、その後の無変化の周は待ちが factor 倍になって合図が出ない（判定行 `pointer=wait:<s> step=1`・応答の turn が「変化」に数えられない）／(b) settle 後に digest の材料（状態 log の最終行）を変えると step = 0 に戻って合図が出る／(c) 待ちが max を超える段は `pointer=stopped` で合図が出ず、digest を変えると再開する／(d) rules 行 2 本が `RuleKind` の `ALL` と `rules validate` の外形に載る（`tests/e2e/rules.rs` の pin）／(e) 注入の周の判定行の全文を pin する既存の歯 7 箇所（`tests/e2e/seat/register.rs` 2・`tests/e2e/seat/launch.rs` 3・`tests/e2e/hook.rs` 1・`tests/e2e/seat/account.rs` 1）の期待に `pointer=sent step=<n>` を写し、**送った直後の周の判定行（`reason=pointer-recent`）を全文 pin する既存の歯 2 箇所**（`tests/e2e/seat/tick.rs`・1 周目が記録を書くので 2 周目は settle 前）の期待に `pointer=settling step=0` を写す（どちらも意味不変・期待の字面だけ）。
- 却下: 席（AI）に「変化が無ければ heartbeat を打たない」と判断させる（席を起こす＝それ自体が合図の消費・C3.3 の自由文入力）／固定の「3 回無変化で中断」（変化の検知が席の応答に依存する周に永久停止しうる・上限で必ず 1 回撃つ梯子の方が両端を機械で守れる）／tick 自体を止める（退避の合図と cycle が止まる・folio2 の 2026-09-17 の事故）／既定の 40 分を伸ばすだけ（撃ちすぎも遅すぎも残る）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "管理 tick の入力欄の門を 3 値（Clear / OwnQueued / Foreign）にし、自席の注入文は Enter 1 回で着地させ、cycle-stamp は /clear の直前に打つ — 断った周に back-off を課さない"
req = ["FR38", "FR28"]
section = "10"
write-set = ["crates/scribe2/src/seat/inject.rs", "crates/scribe2/src/seat/cycle.rs", "crates/scribe2/src/seat/tick/exit.rs", "crates/scribe2/tests/e2e/seat/cycle.rs", "crates/scribe2/tests/e2e/seat/launch.rs", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_cycle_own_queued_"]
size = "S"
done = "自席の注入文が入力欄に残った周は Enter 1 回で着地して cycle が進み、人間の文の周は断って stamp が立たず次の tick が再評価する"

[[contract]]
id = "b"
title = "statusline を器の口に — seat statusline（stdin JSON → 1 行）を account add が口座 dir の settings.json に書き、account statusline で置換し、口座行に statusline= の語を足す"
req = ["FR63", "FR25", "FR29", "FR59"]
section = "3"
write-set = ["+crates/scribe2/src/seat/statusline.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/meter.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/account/cli.rs", "crates/scribe2/src/fleet/json_tree.rs", "crates/scribe2/tests/e2e/seat.rs", "+crates/scribe2/tests/e2e/seat/statusline.rs", "crates/scribe2/tests/e2e/seat/account.rs", "crates/scribe2/tests/e2e/seat/rules.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "+crates/scribe2/tests/e2e/snapshots/e2e__seat__statusline__seat_statusline_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_statusline_", "cargo nextest run -p scribe2 --no-tests=fail statusline_round_trips", "cargo nextest run -p scribe2 --no-tests=fail account_cmd_", "cargo nextest run -p scribe2 --no-tests=fail json_tree_render_"]
size = "M"
done = "描画は pure 関数 1 本で segment は閉じた enum の宣言順・無い値の segment は描かず非 JSON は空行 rc 0・口座の settings.json に statusLine が書かれ置換の口は他の key を保ち・口座行に statusline=vessel|other|absent|unreadable の語が出て、描いた行を tick の読み手が往復で読める"

[[contract]]
id = "c"
title = "管理 tick の systemd unit を derive_units が導出して書く — seat tick install / uninstall・rules 行 seat.tick_interval_s・doctor の tick-unit 行"
req = ["FR29"]
section = "11"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "docs/design/rules-manifest.md", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/seat/cli.rs", "+crates/scribe2/src/seat/tick/install.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/main.rs", "crates/scribe2/tests/e2e/seat/tick.rs", "+crates/scribe2/tests/e2e/snapshots/e2e__seat__tick__seat_tick_install_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_tick_install_", "cargo nextest run -p scribe2 --no-tests=fail rules_manifest_carries_seat_tick_interval"]
size = "S"
done = "tmp の unit dir で install → unchanged → uninstall が往復し、偽 systemctl の呼出順が reload → enable"

[[contract]]
id = "d"
title = "tmux 呼出の上限 — rules 行 seat.tmux_timeout_ms（5000・裁定 id user 2026-09-16T12:09Z）を席の tmux 呼出 1 関数に通し、超過は kill → TmuxFail::Timeout（FailClosed）で reason=tmux-timeout を記録する"
req = ["FR29", "FR38"]
section = "12"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "docs/design/rules-manifest.md", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/inject.rs", "crates/scribe2/src/seat/meter.rs", "crates/scribe2/src/seat/cycle.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/cycle/relaunch.rs", "crates/scribe2/src/seat/cycle/stop.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/seat/tick/exit.rs", "crates/scribe2/src/seat/tick/render.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/account/cli.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/seat/tick.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/seat/cycle.rs", "crates/scribe2/tests/e2e/seat/account.rs", "crates/scribe2/tests/e2e/seat/wm.rs", "crates/scribe2/tests/e2e/seat/launch.rs", "crates/scribe2/tests/e2e/seat/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_tmux_timeout_"]
size = "M"
done = "返らない偽 tmux の下で seat tick が rules 行の上限で戻り reason=tmux-timeout を記録し、返る偽 tmux は従来どおり通り、rules 行が裁定 id 付きで 1 本増える"

[[contract]]
id = "e"
title = "e2e の fixture が自分の残骸を畳む — 死んだ pid 名義の e2e-<pid>-* の隔離 server と dir を次の make_tmp_dir が掃く（歯の一時物を歯が畳む・器の管理物は触らない・戻りの型は不変）"
req = ["NFR6"]
section = "13"
write-set = ["crates/scribe2/tests/e2e/main.rs", "+crates/scribe2/tests/e2e/fixture.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail e2e_fixture_sweep_"]
size = "S"
done = "死んだ pid 名義の隔離 server と dir が次の make_tmp_dir で消え、生きている pid の dir と server は残り、make_tmp_dir の戻りの型と呼び手は不変"

[[contract]]
id = "f"
title = "打刻の合図の backoff — tick が席の digest（状態 log・fleet event の最終行）を席が合図に応えた後に測り、無変化なら合図の間隔を factor 倍ずつ伸ばして max で止め、変化で seat.tick_stale_s に戻す（rules 行 seat.pointer_backoff_factor / seat.pointer_backoff_max_s・裁定 user 2026-09-17T00:55Z）"
req = ["FR29", "FR27"]
section = "14"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "docs/design/rules-manifest.md", "crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/seat/tick/render.rs", "+crates/scribe2/src/seat/tick/pointer.rs", "crates/scribe2/tests/e2e/seat/tick.rs", "crates/scribe2/tests/e2e/seat/register.rs", "crates/scribe2/tests/e2e/seat/account.rs", "crates/scribe2/tests/e2e/seat/launch.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_pointer_backoff_"]
size = "S"
done = "無変化の席への合図が 40 分 → 80 → 160 → 320 → 640 → 1280 分で止まり、席が合図に応えた後の状態 log か fleet event が変わった周に 40 分へ戻って再開し、退避の合図と cycle は毎周のまま"
<!-- contracts:end -->
