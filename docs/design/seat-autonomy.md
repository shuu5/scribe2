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
- `seat guard` … hook `pre-tool-use` の内側に組み込む（新 hook は増やさない）。cap は manifest 行 `seat.context_cap_pct`（初期値 60・裁定 id 付き）。deny の極性は既存 guard と同じ fail-closed。**計測不能は deny しない**（context が読めないだけで編集を止めると開発 session が詰む＝理由を inject.jsonl に記録して allow）。
  - **計測の出所**: 使用 token は payload の `transcript_path` が名指す jsonl を便 2 の parse（末尾 10 MiB・最後の有効 usage 和・`seat::meter::used_from_transcript`）で読む。分母は manifest 行 `seat.context_window_tokens` の**宣言値**で、pane の statusline は読まない（hook から tmux を呼ばない・憲法 C2.2）。使用率は整数の切り捨てで、`pct >= cap` を止める。
  - **通す口は 2 つ**（`SeatDecision::{Allow, Externalize}`・bool で持たない＝憲法 C11）。`Externalize` は `<root>/.claude-session/working-memory.*.md` **ちょうど 2 段**の編集で、使用率に関わらず通す（止めると席は退避すらできない・FR23）。口は狭く取る＝同じ dir でも別名の編集は上限に掛かる。判定は**字句と実体の 2 段**で、`working-memory.*.md` という名前の symlink が口の外を指していれば通さない（通す側の口を字句 1 段で持つと link 1 本で上限を越えられる）。**まだ無い退避物は link ではありえない**ので在るときだけ実体を見る＝これから作る周は通る。`transcript_path` の空文字は「渡されていない」と同じに扱う（`unreadable` に化けさせない）。
  - **測れない理由は 4 語で弁別する**（`no-transcript-path` / `unreadable` / `no-usage` / `no-rule`）。1 語へ潰すと「transcript を渡し忘れている」のか「usage の形が変わった」のかを記録から後で分けられない。対象 tool は write-set guard と**同じ集合**（`hook::guard::GUARDED`）を参照する＝`Bash` は見ない。
- `seat heartbeat --target T` … 裁定 (a) の口。席の**中**から打刻する（`<state_dir>/seat/<潰した target>/heartbeat`）。tick が注入する 1 行がこの打刻を促す＝「席が生きている」は**席が自分で打った**ことでしか測らない。置き場を anchor 配下の `.claude-session/` に取らないのは、v1 の timer と場所を分けて併走できるようにするためである。
- `seat tick --target T --wm-dir DIR` … 裁定 (c) の 4 条件を**順序固定の AND** で評価し、成立時だけ注入する。判定は enum `TickDecision::{Inject, Noop(NoopReason), Error(String)}` で持ち bool にしない（憲法 C11）。
  - **順序** = 鮮度 → pane 取得 → **context** → idle → 未 consumed WM → cycle lock。**最初に立たなかった条件**を理由にする（`heartbeat-fresh` / `pane-missing` / `busy` / `wm-unconsumed` / `wm-unreadable` / `cycle-live`）。順序は load-bearing で、**鮮度が fresh の周は tmux を 1 度も叩かない**（生きている席を毎周 capture しない）。理由の字面は順序の証拠でもある＝入れ替えると同じ条件でも別の理由が出る。
  - **context（`s2-07l.89`・SRS FR29 退避の合図〔v0.3: 開発 session の context 使用量が rules 行の上限以上で、自 session の未 consumed の退避物が無く、cycle が走っていない間、管理 tick が idle を待たずに退避の合図 1 行を注入する（使用量が測れない周・退避物が在る周・cycle が走っている周は注入しない）。FR27 打刻の合図は上限未満か測れない周に絞られ FR29 と排他〕/ FR25 の計測 / AC9）**: pane を取得した周は、その本文から context 使用率を読み（meter の `used_from_pane_pct`＝`seat meter` と同じ 1 本の口・transcript の path を tick へ写す seam は作らない〔C10.3〕）、cap（meter の `declared_cap`＝guard と同じ 1 本の口・rules 行 `seat.context_cap_pct`）**以上なら idle を待たずに退避の pointer 1 行**（`退避 tick: context <pct>% ≥ cap <cap>%・/ready-compaction で退避してください`・`kind=externalize`）を既存の inject 経路で注入し、打刻の pointer と同じく自打刻する（次の周は fresh で撃たない＝storm 止め・busy な席へは queue の形で届く〔`.90`〕）。context を idle の**前**に置くのは、cap を超える席は busy（lens 待ち・長い cargo）であり、busy を理由に noop すると誰にも止められず auto-compact に至るためである（guard は編集の瞬間にしか効かない・実インシデント 2026-09-11）。
    - **退避済みの席には注入しない**（planner 裁定 2026-09-11・livelock の補正）: 注入するのは**自席の未 consumed 退避物が 0 件と確認できた周だけ**。退避済みの席は `/clear` 前で cap 以上のままなので、退避物を見ずに注入すると毎周 pointer を重ねて cycle に一度も落ちない。`wm-unconsumed` は次の条件（idle → 退避物 → cycle）へ、`wm-unreadable` は「0 件」と読み替えず同じく次へ（注入しない側）。
    - **測れない周は `context=unmeasured reason=<meter の 1 語>`** を判定行に載せて**次の条件へ進む**（注入も停止もしない・AC9 条 3 と同じ極性）。cap の行が読めない周は guard と同じ `no-rule`。判定行は既存の `decision=… reason=…` に `context=<pct>` を足す（pane を取得した周だけ・fresh で取得しない周は載せない＝`cycle=` と同じ「評価していない」の印）。注入の行は `decision=inject target=… consumed=… kind=<pointer|externalize>`。閾値・rules 行は動かさない（C5）。
    - **cycle lock が live な周には注入しない**（排他は cycle と同じ 1 本の lock）: 作り直しの最中に行を queue しても、届く先は消えるか作り直された席である。
    - **残る側 = 盲点は最大 `seat.tick_stale_s`**（宣言値）: fresh の周は pane を読まないので、打刻の直後に cap を超えた席は次の stale な周まで見えない。退避の pointer の打刻も同じ鮮度に数えるので、席が退避を終えて idle になっても cycle の入口（`wm-unconsumed` → cycle）は次の stale な周まで開かない（遅れは最大 `seat.tick_stale_s`・MVP はこれを受容する）。`--pointer` は打刻の促し（`kind=pointer`）だけを上書きし、実測値を運ぶ退避の行には掛からない。
  - **裁定 (d) 鮮度** = `now − max(heartbeat mtime, tick-stamp mtime)` が閾値（rules 行 `seat.tick_stale_s`・裁定 id 付き）以下なら fresh。**両 file 不在は stale**。自分の打刻（tick-stamp）も見るのは、注入した直後の周が「席がまだ打刻していない」を理由に撃ち続ける自傷 storm を塞ぐためである。
  - **裁定 (e) idle** = 最後の prompt 行の右が空 ∧ **探索域**に**走行中の印**が無い。
    - **探索域** = 最後の prompt 行より**上の非空 6 行 + 下の全行**。上を数で切り下を全部取るのは、印の位置が 2 通り（spinner は入力欄の上・版によっては statusline）あり、statusline の高さが設定で変わるため（実測 2026-09-11: 3 行）。末尾から固定行数（旧 `TAIL_LINES=6`）を取る形は statusline 3 行 + 区切り 2 行 + prompt 行で尽き、下から 8 非空行目の spinner に届かず**走行中の席を idle と読んだ**（2026-09-11・`/rebrief` 走行中の席へ `/clear`・bd `s2-07l.94`）。域を statusline の側（prompt より下）だけに狭めてもならない（実測 2026-09-10: 印が上に在る pane へ `/clear`・lens-384 C-1）。上の 6 は実測の 2〜3（区切り 1 行 + Tip / auto-update / queue の写し 0〜2 行）の 2 倍で、それ以上遡らないのは前の turn の出力に写った spinner 形の行で idle な席を busy と読み続けないため。
    - **印の集合**（上の域）= (1) spinner 行の**形** `<frame の 1 字> <語または文>…` に、括弧が在るなら `(<経過時間>` が続く（`✻ Sublimating… (19m 36s · ↓ 36.4k tokens)` / `✻ Reviewing the seat idle predicate… (2m 3s)`〔todo が走る周は語が todo の文になる〕/ `✻ Sublimating…`〔開始 16 秒未満は括弧が無い〕/ 旧版 `✻ Thinking… (23s · esc to interrupt)`）。frame の字は本体の配列を実読した有限集合 `· ✢ ✳ ✶ ✻ ✽ *`（assistant 本文の `●`・tool の `⏺ ⎿`・markdown の `-` は含まない）、括弧の中は `<n>h/m/s` を 1〜3 つ並べ直後が `)` か ` ·`。語は周ごとに変わるので字では持てない。(2) API 待ちの banner（spinner 行ごと置き換わる 4 形）と compaction 中の専用行: `Retrying in` / `will retry in` / `next try in` / `waiting up to` / `Compacting conversation`。
    - **印の集合**（下の域 = statusline）= `esc to interrupt`（旧版の印・現行版では API 再試行行にしか描かれない＝本体の文字列を実読 2026-09-11。**これだけでは走行中を読めない**のが本事故の根）。上の域では見ない: 本文がこの字を引用した idle な席を永久に busy と読むため。
    - **絞りの根拠と限界**: turn の完了行 `✻ Crunched for 10m 28s · done 12:41` は `…` を持たず、tool の走行行 `ls -la … (2m 6s)` は行頭が英数字、本文の `● 直した… (3 件)` は行頭も括弧の中も外れる（idle な席を busy と読まない側の根拠）。それでも本文の行頭が frame の字で `…` で切れる行や banner の文言の引用が上 6 非空行に残れば、idle な席を busy と読み続ける（席は出力を出さないので pane が変わらず脱出できない・lens-94 HIGH-3）。この残余は fail-closed 側（注入しない）ゆえ `/clear` の事故より軽く、字面では根絶できない＝typed 化 `s2-07l.95` へ申し送る（planner 裁定 2026-09-11・時間差の二重 capture は ad-hoc 判定ゆえ不採用）。字面での判定は暫定。
    - prompt 行を特定できない pane は idle と名乗らない（fail-closed・読めない席へ注入しない側へ倒す・不変）。
  - **裁定 (c) 自席の弁別** = 退避物の **file 名の sid ではなく frontmatter の `seat:` が target と一致**するもの（席の外から回る tick は sid を知らない）。名乗りを持たない退避物は自席のものと数えない——他席の退避物を根拠にすると、別の席の文脈で `/clear` を撃つことになる。dir が読めない周は `wm-unreadable` で、**0 件に潰さない**。
  - **裁定 (b) cycle の駆動** = `wm-unconsumed` で止まった周**に限り**、lock が空いていれば cycle を in-process で回し、判定行の末尾に `cycle=<done|failed|refused reason=…>` を足す。それ以外の周は cycle を評価しない（`cycle=` が付かないこと自体が「評価していない」の印）。退避側の自己 kick は持たない＝退避から作り直しまでの遅れは最大 1 周期で、MVP はこれを受容する。
  - 実行系が回らない周（置き場を解けない・注入を確認できない・宣言 rule が読めない）は `decision=error` と rc 1 にし、**noop の語彙を汚さない**（席が静かなのか機械が壊れているのかを記録から読めるようにする）。注入の断り（`busy` 等）は noop と字が重なるので `inject-` の前置きで分ける。
- `seat cycle --target T --wm-dir DIR [--restore CMD]` … 不可逆の口。lock は `<state_dir>/seat/<潰した target>/cycle.lock`（`O_EXCL`・中身は pid と deadline の 1 行 JSON）。手順は順序固定: lock を取る → 自席の未 consumed 退避物が在ることを確認 → pane が idle であることを確認 → `/clear` 注入 → 作り直しの確認 → 復元 command（既定 `/rebrief`）注入 → lock 解除。**lock-held / wm-missing / wm-unreadable / busy / pane-missing の周は 1 key も送らずに rc 1**（憲法 CON5 / SRS FR28）。TTL（rules 行 `seat.cycle_lock_ttl_s`）を超えた lock は residue として取り直す。
  - **`/clear` の送達確認だけは便 2 の settle を使えない**: `/clear` は pane を消すので「送った字面が現れる」形では測れず、**成功したときほど確認が落ちる**。代わりに**作り直しの正の証拠**「入力欄が空 ∧ 入力行より上に**消費済みの echo `❯ /clear`**（行頭・右は `/clear` ちょうど）が在る」を最大 30 秒（500 ms 周期）見る（便 2 の裁定「送った字面が現れた = 送達成功・入力欄が空 = 消費」と同じ形・`s2-07l.96`）。作り直された席は画面を消した後この echo を新しい prompt の直上に**必ず**残す（実測 2026-09-11: banner 3 行 → `❯ /clear` → 空の prompt → statusline）。
    - **負の形（「探索域に `/clear` の字面が無い」）は採らない**: 域を prompt より下に取ると echo が域の外に落ちて第 2 項が構造的にほぼ常に真になり（確認が 500 ms の sleep に化ける）、末尾の固定行数（旧 6 行）でも statusline 3 行 + 区切り 2 行 + prompt 行で尽きて同じ形に化ける（`s2-07l.94`）。域を prompt 基準（上の非空 6 行）にすると今度は echo が**必ず域の内に在る**ので第 2 項が偽のまま固定され、席が 6 行以上を出力するまで真にならない——その出力は復元を送った後にしか出ないので、`/clear` が通った席に復元を送らず空席のまま残す（実測 2026-09-11 admin 席・`.94` で極性が反転）。負の形は域の取り方のどちら側でも壊れる＝証拠は正の形で持つ。
    - **未 submit の `/clear` は入力行そのものに残る**（`input_tail` が非空）ので、echo と同じ字面でも弁別できる（Enter だけが落ちた周・実測 2026-09-11 planner 席）。cycle の入口の idle 判定でも同じ pane は busy と読まれ、`/clear` を重ねて送らない。本文が echo を**引用**した行は 2 桁字下げで描かれ行頭に来ないので、行頭の条件が引用を除外する。
    - **域は入力行より上の全行**（裁定 (e) の上 6 非空行に絞らない）: `/clear` は画面を消すので見えている echo は**何らかの** `/clear` の後に描かれたものに限られ、遡る数で古い echo を除外する必要が無い。6 行で切ると echo の下に描かれる行（hook の出力等）が版で増えた周に同じ行き止まりへ戻る（実測 2026-09-11: 実席は echo の周りに自動更新の告知 2 行 + hook 2 行を描いた）。
    - **残余（字面の正の形では塞げない・根治は `s2-07l.95` の typed 打刻）**: (1) 前の `/clear` の echo が見えたまま今回の `/clear` だけが消費されなかった周（字面が落ちて Enter だけが通る等）は、送る前と同じ pane を「済んだ」と読む。作り直し済みで復元の届いていない空席と形が同じなので、送る前の形で弁別すると空席を永久に回復できない＝弁別しない側に倒す（席は退避済み ∧ idle ゆえ害は「会話が生きたまま復元が走る」まで）。(2) 入力行より下に prompt の字を含む行が在ると anchor が移り未 submit の入力行が echo に見える（`input_tail` と同根・現行 statusline には無い）。(3) idle 判定と送信の間に席が turn を始めた周は `/clear` が queue され、turn の終わりに復元なしで発火する（次の tick が `wm-unconsumed` → cycle で回復する）。復元 command の側は便 2 の送達確認をそのまま使う（作り直し直後の席は hook の実行中に復元を queue するので 2 秒の窓では `Queued` に倒れうる＝`s2-07l.97`）。
  - **lock を取れなかった理由は 2 つに分ける**（`lock-held` = live な lock が在る / `state-dir` = 置き場が使えない）。置き場の位置に file が在る周は `create_dir_all` が競合と同じ error kind（`AlreadyExists`）を返すので、まとめると「他の cycle が走っていた」と「書けない」を記録から分けられない。
  - **表示の `target=` は置き場の dir 名と同じ潰した字面**（`a:b` → `a_b`）にする。便 2 の `seat inject` と語彙を 1 つにするためで、契約の `<T>` からの意図した読み替えである。
- `seat inject --target T --text …` … tmux `send-keys` + 送達確認（pane の末尾に text が現れたか）。rc は 0 / 1 の 2 値にし、v1 の偽陰性（4 / 7）を作らない。
- 駆動: systemd user timer は **repo に入れない**（起動コマンドを repo に置かない・CLAUDE.md）。unit の雛形は §8 に書き、user の host で有効化する。
- 記録: 判定と注入は `<state_dir>/seat/<target>/tick.jsonl` に 1 行ずつ（FR21 と同じ schema）。
- 計測できない理由の弁別（便 2 の実装で決めた読み）: statusline の候補が無い周は、pane 本文が空なら `no-source`・本文が在れば `pane-no-statusline` に分ける（どちらの弁別も **transcript を渡さない周だけ**の話で、明示された周は pane を読まずに jsonl で測る・s2-07l.75）。健全性を外れた候補は `pane-out-of-bound` で**不成立のまま**とし、別の出所で塗り直さない（壊れた面を他の値で隠さない）。**この 3 語はいずれも pane を読む周＝transcript を渡さない周にしか出ない**（渡した周は pane を読まないので到達しない）。

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
5. 切替便: 開発 session へ器を載せる面を揃える——`marketplace.json` を gen-manifest の生成物に足し（手書きしない・冪等）、導入と切替の手順を §9 に書く。そのうえで v1 timer と**併走 1 日**→ 判定一致を tick.jsonl で示す → user 裁定で v1 timer を stop（消す = A1・user 手番）。host での install・timer 有効化・v1 停止は**器の外**（planner と user の手番）である。

## 6. 却下案
- (i) in-session の CronCreate だけで tick を回す: respawn / 口座切替で死に、死んだことを誰も検知しない（v1 の case A 裁定と同じ理由）。
- (ii) v1 script をそのまま plugin として積む: user 直命（v1 を開発に積まない）に反し、python 実体の保守が続く。
- (iii) context 上限を契約の大きさだけで受ける（CON6 の拡大解釈）: runner には正しいが開発 session には効かない。

## 7. 裁定済みの論点
- 計測の一次ソース = **transcript が名指された周は transcript・名指されない周は pane**（guard は hook の中で pane を持てない〔C2.2〕ので、pane 一次のままだと 2 面が同じ瞬間に違う値を返す。実測 2026-09-11・s2-07l.75: 両出所は同じ量を見ており差は平均 0.6% だが、pane は statusline の 1k 刻みの階段・transcript は 1 token 粒度の連続で、境界の判定は細かい側が安全）。**NFR5 は満たす**——transcript は末尾 10 MiB しか読まず、11.1 MiB の file を渡した実測で 1 回 0.01 秒（debug binary・3 回とも同値。上限は 2.0 秒）。
- cap の初期値 = 60（user 裁定 2026-09-10・manifest 行 `seat.context_cap_pct` に裁定 id 付きで置く・C4 / C13 の閾値ではないので A2 非該当）。
- 切替の判定 = 併走 1 日 + cycle 完走 1 回（AC9）。v1 の timer を止めるのは user 手番（A1「消す」）。

## 8. 駆動（systemd user の雛形・**unit は repo に入れない**）

管理 tick は席の**外**から回る（憲法 R-E12）。host 側に template unit を 1 組置き、席ごとに tmux target を
instance 名で渡す。**host 固有の path・target 名・口座名は書かない**（本 repo は PUBLIC・CLAUDE.md
「やらないこと」）ので、以下は雛形であって設定ではない——`<binary>` / `<anchor>` / `<state dir>` は user が
自分の host で埋める。

`<NAME>-seat-tick@.service`:

```ini
[Unit]
Description=seat tick for %i

[Service]
Type=oneshot
ExecStart=<binary> seat tick --target %i --wm-dir <anchor>/.claude-session --state-dir <state dir>
```

`<NAME>-seat-tick@.timer`:

```ini
[Unit]
Description=seat tick timer for %i

[Timer]
OnCalendar=*:0/5
Persistent=false

[Install]
WantedBy=timers.target
```

- `%i` は systemd の instance 名で、tmux target をそのまま渡す（`:` を含む形の escape は host 側で解く）。
- 周期を 5 分に取るのは、裁定 (b) が cycle の駆動を tick に載せた結果、退避から作り直しまでの遅れが最大
  1 周期になるためである。短くすると遅れは縮むが、席が静かな間も capture が増える。
- 有効化と停止は **user の手番**である（憲法 A1「使う」/「消す」）。器はこの unit を書き出さないし、
  v1 の timer を止めもしない（切替は §5 の便 5 で、判定一致を tick.jsonl で示してから user が裁定する）。

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
2. plugin を **project scope で** install する。
3. `.claude/settings.json` の `enabledPlugins` に `<NAME>@<NAME>` を足す。**この編集は本設計の
   範囲外**（marketplace を host へ登録した後の別便で、admin は本便で settings に触らない）。
4. **走行中の session には `/reload-plugins` か再起動が要る**（hook は自動では反映されない）。

**(c) 管理 tick の timer を有効にする。** §8 の雛形を user が埋めて有効化する（§8 のとおり unit は
repo に入れない）。

**(d) 併走して突き合わせ、user が切り替える。** v1 の timer と 1 日併走し、判定の一致を
`tick.jsonl` と v1 の log で突き合わせる（AC9）。**v1 timer の停止は user 裁定**（憲法 A1「消す」）
であり、器は自分で止めない。切替の条件そのものは本 doc でなく **bd `s2-07l.38` の notes** が持つ
（規範を doc へ写さない＝憲法 C1 / N2）。
