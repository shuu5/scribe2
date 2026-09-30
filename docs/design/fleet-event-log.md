# 設計: fleet の event log — 現在地は追記だけの file を replay して読む

- 要件: [FR3](../../design-intent/spec/srs.html#FR3) 便の永続 / [FR13](../../design-intent/spec/srs.html#FR13) stop / [FR14](../../design-intent/spec/srs.html#FR14) resume / [FR22](../../design-intent/spec/srs.html#FR22) 人手 0 の計測 / [AC4](../../design-intent/spec/srs.html#AC4) / [NFR3](../../design-intent/spec/srs.html#NFR3) / [NFR4](../../design-intent/spec/srs.html#NFR4)。制約: CON2 / CON3
- 憲法: [C3](../../design-intent/spec/constitution.html#c3) fleet の状態は 1 つ（C3.3 typed 状態・C3.4 Completion 1 enum）/ [C6](../../design-intent/spec/constitution.html#c6) 計測は append-only store 1 つ / [C11](../../design-intent/spec/constitution.html#c11) 失敗は型で / [C12](../../design-intent/spec/constitution.html#c12) 外形 snapshot
- 決定: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.1（永続面 = JSONL 1 file・SQLite は v3）/ §2.2（跨版 面 2）/ §2.4（state dir の受け渡し）
- crate の形（lib + bin・`tests/e2e/main.rs` 1 target・tmp helper・snapshot）は [rules-manifest.md §2](./rules-manifest.md) に従う。
- この設計から出る契約: `s2-07d`（fleet event log 最小）。pipeline 側の利用は [pipeline.md](./pipeline.md)。

## 1. 何を解くか

便（run）と席（seat）の現在地を **process の記憶ではなく永続面から読む**。各段は event log を replay して現在地を得て、段の終わりに event を 1 件追記して終わる（FR3）。process を殺して別 process で続きを通せる（AC4）のはこの形の帰結である。

C3 は「1 つの DB file（host 列）」と言う。MVP はそれを **append-only の JSONL 1 file（各行に host 列）** と読む（ADR-0004 §2.1）。SQLite は依存 0 本の歯（NFR3）と衝突するので v3 の手番。

やさしく言うと: 起きたことを 1 行ずつ file の末尾に足していき、「今どこか」は file を頭から読み直して決める。途中で process が死んでも file は残る。

## 2. 置き場（state dir）

- **env も HOME も読まない**（C2.2・ADR-0004 §2.4）。`fleet` の subcommand は `--state-dir <dir>` を**必須**とする（既定を持たない・無ければ usage + rc 1）。
- 仕える repo に紐づく既定は、`vessel init --state-dir <dir>` が repo の **git config（local）** に `<NAME>.stateDir` として書く（[vessel-hook.md §2](./vessel-hook.md)）。`pipe` と `hook` はそれを `git config --get` で読み、`--state-dir` があれば上書きする。tracked file に path を書かない（CON2）。
- event log = `<state_dir>/fleet/events.jsonl`。lock = `<state_dir>/fleet/events.jsonl.lock`。
- verdict export（面 5）= `<state_dir>/fleet/verdicts.jsonl`（書くのは pipeline の land・本 store と同じ append 経路）。
- 注入計測 = `<state_dir>/inject.jsonl`（[vessel-hook.md §6](./vessel-hook.md)）。C6.3「消費は append-only store 1 つ」が指す消費の store はこの 1 本であり、events / verdicts は状態と審査結果であって消費ではない。

## 3. schema（`schema = 1`・flat JSON・1 行 1 event）

| field | 型 | 必須 | 意味 |
|---|---|---|---|
| `schema` | u64 | 必須 | 1。非互換な変更は版を上げる（ADR-0004 §2.5） |
| `ts` | string | 必須 | UTC `YYYY-MM-DDTHH:MM:SSZ` |
| `kind` | string | 必須 | `EventKind` の variant 名 |
| `run` | string | kind ごと | run id（`<bead>-<UTC stamp>`）。便の kind では必須・run を持たない kind（口座残量 2・`SeatRegistered`・`AccountRetired` / `AccountRestored`・`RulingReceived`〔§9〕・§12 の 5 kind）では在れば malformed |
| `bead` | string | kind ごと | 契約の bead id（台帳は読まない・文字列として持つだけ）。便の kind では必須・run を持たない kind では禁止（`RulingReceived` だけ任意・§12 の UtteranceSorted は sorting が request の行だけ必須・IntakeRefused は必須） |
| `host` | string | 必須 | host 名（C3 の host 列）。`/etc/hostname` → `hostname` コマンド → `"unknown"` の順。env は読まない |
| `actor` | string | 必須 | `"machine"` / `"human"`。人由来 event を数える面（FR22）。`human` は `ApprovalReceived`・`RulingReceived`（§9）・`SeatRetired`・UtteranceReceived（§12）の 4 kind |
| `stage` | string | 任意 | `Stage` の variant 名 |
| `seat` | string | 任意 | 席 id（MVP は run id と同じ） |
| `pid` | u64 | 任意 | runner の pid |
| `account` | string | 任意 | `SeatSpawned` だけ: 器が選んで runner に渡した口座の label（口座を渡さず親の環境を継承させた周は置かない・口座ごとの走行中の便数の出所・[ADR-0027 §2.3](../../design-intent/decisions/ADR-0027-run-account-order-earliest-reset-and-no-per-account-cap.html#s2-3-inflight)・schema 版は 1 のまま） |
| `detail` | string | 任意 | 自由文（verdict 名・runner rc・承認の逐語 等） |

- `pub enum EventKind`（閉じた enum・variant の列挙は core が持ち文書は写さない。記録時点の 8 = `RunCreated` / `RunStage` / `RunDone` / `RunStopped` / `SeatSpawned` / `SeatStopped` / `ApprovalRequested` / `ApprovalReceived`・以後は各 ADR が末尾に足す）。
- `pub enum Stage`（閉じた 8 variant）: `Intake` / `Blocked` / `Spawned` / `Implemented` / `Gated` / `Landed` / `Stopped` / `Failed`。遷移は pipeline 側（[pipeline.md §4](./pipeline.md)）。
- `pub enum SeatState { Live, Stopped }`（C3.3: 席の状態は typed enum・bool で持たない）。
- 字面変換は **wildcard 無しの `match`** 1 箇所ずつ。
- JSON は flat object（値は string / u64 / bool / null）だけを扱う std の writer / reader（`json_lite`）。`"` `\` 制御文字は escape する。それ以外の形は error。

## 4. replay・store・待機

- `pub fn replay(events: &[Event]) -> State`。`State { runs: BTreeMap<String, Run>, seats: BTreeMap<String, Seat> }`、`Run { id, bead, stage: Stage, updated, detail, approved: bool }`、`Seat { id, run, pid, state: SeatState, updated }`。run ごとに物理順で最後の `stage` が現在地。`SeatStopped` で `SeatState::Stopped`。`ApprovalReceived` で `approved = true`（`approved` は「承認 event が在るか」の導出値であって状態 enum ではない）。
- `append(dir, &Event) -> Result<Warnings, StoreError>`: lock file を `create_new` で取り（再試行の上限は rules 行 `fleet.lock_retry_ms`）、`O_APPEND` で 1 行書いて flush し、lock を外す。**rules 行 `fleet.lock_stale_ms` より古い lock は stale として除去し** `Warning::StaleLockRemoved` を返り値に載せる（黙って消さない）。**予定形**（`s2-07l.203` の land まで現物は mtime の線だけ）: lock file には**所有者の pid を 10 進 1 行**で書く（`create_new` で開いた handle にそのまま書く・第 2 の writer を作らない）。既存の lock に当たった周は中身を読み、**所有者が死んでいれば外して取り直し**、その旨を warning の 1 種として返り値に載せる（黙って消さない）。判定は純関数 1 本で所有者を 3 値に読む——死んでいる = 本文の pid の `/proc/<pid>/stat` が**無い**周だけ／生きている = 起動時刻が読めた周（pid の再利用も「生きている」）／読めない = 本文が 10 進 1 行でない周と、起動時刻の probe が「無い」以外の理由で読めない周（`/proc` が読めない環境・parse 不能。probe は `/proc/stat` を先に読み、それが読めない周は pid の有無を見ずに「読めない」＝`/proc` 自体が無い環境を「無い」に畳まない）。読めない周と生きている pid は従来どおり `fleet.lock_stale_ms` の線に従う（FailClosed の極性は変えない＝probe の読めなさを「死んだ」に畳むと生きた所有者の lock を外す側へ倒れる・C11.2）。pid の生存判定（起動時刻）の実装は受付の札（[ADR-0021 §2.3](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html#s2-3-slots)）と共有する 1 本にする（lock 実装を 1 本に保つのと同じ理由。札は probe が読めない周も回収側に読む＝ADR-0021 §2.3 の回収〔死んだ札と本文の壊れた札〕と §5 (D) の安全論〔札を失っても過剰に配る側へ倒れる〕に従う）。中身が pid だけである理由: 札の ts を持たないので pid の再利用は弁別できないが、再利用された pid は「生きている」と読んで**待つ側**へ倒れる（安全な向き）＝札と同じ 2 値を持たせると第 2 の受付が生える。新しい閾値は作らない（rules 行も裁定 id も増えない）。
- **回収は 1 手**（契約表の行 c・`s2-07l.486`）: 現物の回収は 3 手（本文を読んで死んだ／古いと判じる → `remove_file` → `create_new`）で、同じ死んだ lock を観測した 2 本が両方とも回収に入ると、後の 1 本の `remove_file` が先の 1 本が取ったばかりの生きた lock を外し、2 本が同時に lock を持つ。lock 実装は 1 本なので穴は 4 面に共通——追記（本節）・受付の入口（[dispatcher.md](./dispatcher.md) §5・`s2-07l.366`）・受付札（[gate-cost.md](./gate-cost.md) §3.2）・driver の札（dispatcher.md §5・`s2-07l.482`）。直し: 回収を関数 1 本 `reclaim(lock, observed) -> bool` に切り出し、回収用の token `<lock>.reclaim` を `create_new` で取れた 1 本だけが lock を読み直し、観測した本文と同じ周に限って `remove_file` し、token を消して `true` を返す。token を取れなかった本と読み直しが観測と違った本は `false` で、外さずに次の周の取り直しへ戻る（token の寿命は μs 単位・rename は塞がらない〔`s2-07l.482` の実測〕）。回収の途中で死んだ process が残した token は外さず、`fleet.lock_retry_ms` の後に token を名指す typed な error で落とす（FailClosed・C11.2・黙って外す側へ倒さない・恒久の直しは §8 の OS の file lock）。判定の本文（所有者の 3 値・`Reclaim::{Stale, DeadOnly}`）と rules 行（`fleet.lock_retry_ms` / `fleet.lock_stale_ms`）は不変・新しい閾値は作らない。歯は同じ死んだ lock を観測した 2 本を逐次 2 回の呼び出しで表し `true, false` を pin する（in-file・並行の e2e は置かない）。
- `read_all(dir) -> Result<Vec<Event>, Vec<StoreError>>`: **malformed 行（parse 不能・`schema` が 1 以外）は skip せず `line=<N>` 付きの error に全件集めて `Err`**（NFR4）。file 不在は `Ok(vec![])`。
- **待機は 1 実装**（C3.4）: `pub enum Completion { RunnerExited(pid), SeatGone(pid) }` と `pub fn wait(c: Completion, deadline: Duration) -> Result<(), Timeout>` の 1 本。任意の述語を受ける口は作らない。pipeline の「runner の終了待ち」「TERM 後の消滅待ち」はこの 2 値で表す。
- 失敗は境界ごとの enum（`StoreError` / `Timeout`）で持ち、極性は `FailClosed`（C11.2）。
- **着地の列の待ちの費用**（`s2-07l.300`）: `Completion::LandTurn` の 1 周の観測は event log の全行 replay で、列に並ぶ便の数だけ core を焼く。wait は列の材料の metadata の組——event log と `<state_dir>/pipe/*/verdict.json` それぞれの（長さ・mtime・inode）——を前回の観測と比べ、変わらない周は replay を省いて前回の判定を使う（周期の数値は新設しない・材料が変われば必ず読み直す・`Completion` の値と wait の 1 実装は不変・列の中身は verdict で決まるので verdict.json も材料〔[gate-cost.md](./gate-cost.md) §6.1〕・器の atomic な書き〔`.partial` → rename〕は inode を必ず変えるので mtime の粒度に賭けない・印は replay の前に取る）。

## 5. CLI（`<NAME> fleet …`・`--state-dir D` 必須・出力は `emit` / `emit_err` 経由のみ）

- `fleet record --kind <k> --run <id> --bead <b> [--stage <s>] [--seat <id>] [--pid <n>] [--actor machine|human] [--detail <text>]` → rc 0・stdout 1 行 `fleet: recorded <kind> run=<id>`。書けるのは本体の形が便の形の kind だけで、ほかの kind は断る（§12 形 5）。
- `fleet show --run <id>` → 1 行 `run=<id> bead=<b> stage=<s> approved=<bool> updated=<ts>`。無ければ `fleet: no such run` + rc 1。store が読めなければ rc 2。
- `fleet export`（**跨版 面 2**）: stdout 1 行目 = `{"schema":1,"kind":"export","host":"<host>","runs":<N>,"seats":<N>}`、以降 run 1 件 1 行・seat 1 件 1 行。**read-only**（store の file を 1 byte も変えない・lock も取らない）。malformed なら error 行 + rc 2。
  - run 行 = `{"kind":"run","id":"<run id>","bead":"<bead>","stage":"<Stage>","approved":<bool>,"updated":"<ts>"}`（key はこの並び）。
  - seat 行 = `{"kind":"seat","id":"<seat id>","run":"<run id>","state":"<SeatState>","updated":"<ts>"}`（key はこの並び）。

## 6. 歯（契約 `s2-07d` の検証・`tests/e2e/fleet.rs` module・`fleet_` 接頭辞・tmp dir を `--state-dir` で指す）

歯は `crates/<NAME>/tests/e2e/fleet.rs` module に `fleet_` 接頭辞で置く（個々の名前はここに書かない。名前の列は現物が SSOT＝`cargo nextest list -p <NAME>`・ADR-0013 §2.1・`s2-07l.78`）。外形（usage と 1 行出力）は insta snapshot 1 本で pin する。

何を測るか: replay が event から state を再構成し、承認 event で run が approved になり、席の停止 event で席が Stopped になる／壊れた行は行番号付きの `Err` 1 件になり後続の行を返さない（2 行目を壊す → `line=2`・3 行目は返らない）／未知の schema を拒む／並行 append（8 thread × 50）が 400 行・全行 parse 可・interleave 0／process を跨いで state が残る（process A が `record`・process B が `show` → 同じ stage）／export は先頭が schema header で、rc 0 ∧ stdout が 1 + runs + seats 行、**その上で**前後の file 名・size・mtime 同一・lock が残らない（読み取り専用）／`--state-dir` 無しは rc 1・stdout 0 byte／JSON の escape が round-trip する／stale lock は閾値の後に除かれる（`File::set_modified` で lock の mtime を戻す・std のみ）／wait は型付きの error で timeout する／壊れた store の export は rc 2。

## 7. 却下案

- SQLite（NFR3・cell の sandbox が crates.io を引けない）。v3 で A3 を経て再提案。
- per-run の JSON file を上書き（置換 write は先行の印を消す・記帳は追記であること）。
- lock 無しの append（同 host 並列 write の interleave）。
- malformed 行の silent skip（NFR4）。
- state dir の既定を HOME から導く（C2.2 の env 直読禁止に当たる・lens 指摘で却下）。
- `Seat.live: bool`（C3.3 の typed enum に反する・lens 指摘で却下）。
- accounts / usage / lease の表（口座選定は v3・SRS scope out）。

## 8. 後続

- doctor の fleet 面（host ごとの event 件数と schema 版の照合・C3.2）。口座・lease・退役の表（v3・C3 / C9）。
- SQLite 化は A3 を通した上で **同じ event を投影する**形にし、event log は消さない（跨版 面 2 は event log の path で固定）。
- event の圧縮・rotation（MVP は無限追記・1 便あたり 10 行程度）。cross-host の lock（MVP は同 host 内のみ）。
- lock の実装を OS の file lock（std の `File::try_lock`）へ置き換え、pid 本文と回収（§4 の行 c の token）を消す案。所有者が死ねば OS が lock を離すので、死んだ lock の回収そのものが要らなくなる（C17.2 の「消すもの」= 回収の経路と warning 2 種）。`fleet.lock_stale_ms`（と `Reclaim::Stale`）の去就は閾値行の裁定（A2）を要るので、行 c の後の候補として user に上げる。

## 9. run 無しの裁定を承認 event として持つ — kind `RulingReceived`・対話面の席の口 `seat ruling`・doctor の突合（契約表の行 b・`s2-07l.386`）

- 何が起きているか（admin の実測 2026-09-16 03:35Z・verified・母集団 = event log の全 kind の件数・決定は [ADR-0037](../../design-intent/decisions/ADR-0037-rulings-without-a-run-are-approval-events.html)）: `ApprovalRequested` / `ApprovalReceived` は 0 件・`pipe report` の human_events=0。同日の user 裁定（rules 行の値・A2 の閾値・方針）は台帳の散文にだけ在る＝憲法 C7.2 の穴。現物: `ApprovalReceived` は `run` / `bead` を要り（`pipe approve --words`・`fleet record`）、run の無い裁定を書く口が無い。run を持たない kind は既に 5 つ（口座残量 2・`SeatRegistered`・`AccountRetired` / `AccountRestored`・`event.rs` の `forbid` が kind ごとに `run` / `bead` を断る形）＝§3 の表の「`run` 必須」は kind ごとの規則に読み替える（本節で表を直す・現物に合わせる）。
- 形: (1) `EventKind` の末尾に variant 1 つ `RulingReceived`（actor = `human`・`run` を持たない kind〔行に在れば malformed〕・`bead` は任意・新しい key `rule`〔rules 行の id・任意・文字列〕・`detail` = user の逐語で**空なら 1 byte も書かない**〔`pipe approve` と同じ `record_words` の型〕・schema 版 1 のまま）。読み手（`event.rs` の kind ごとの `forbid` の arm・`replay` はこの kind で run を作らない）。`Event` に key `rule` の field を足すと `Event` の literal 構築点の全部に 1 行要る（main 2f846df の母集団 13 file・38 か所と一覧は [gate-cost.md](./gate-cost.md) §26 (c)・`pipe/stop.rs` の 1 か所は `..` の struct update で不要）＝行 b の write-set は当初の census に無かった 3 file（`crates/scribe2/src/pipe/dispatch.rs` / `crates/scribe2/src/hook/vessel.rs` / `crates/scribe2-boundary/tests/e2e/ledger_memo.rs`）を持つ。(2) 口 = `<NAME> seat ruling add --state-dir S --target T --words "<逐語>" [--bead B] [--rule ID]`（1 件書く・ts は器が打つ＝**裁定 id**）/ `seat ruling ls --state-dir S`（ts・bead・rule・逐語を 1 件 1 行）。`T` の登録 row（[seat-roles.md](./seat-roles.md) §2 の解決）の役割が rules 行 `R-C7-1` の値でない周は `not-dialogue-surface` で断る（row が無い・読めない周も断る・FailClosed）。書き手はこの口だけ（`fleet record` はこの kind を従来どおり断る＝`run` を要る口）。(3) `pipe report` の行に `rulings=<n>` を足し、`human_events_other_than_approval` の「承認」の kind を `ApprovalReceived` と `RulingReceived` の 2 つにする（FR22）。(4) doctor（`doctor --state-dir S`）の 1 行 `rulings=<n> rule-rulings=<matched>/<of> unmatched=<id,…>`: manifest（tracked + `--rules`）の `[[rule]]` のうち `ruling` 欄が `user <UTC の分までの ts>` の形の行を母集団とし、同じ分の `ts` を持つ `RulingReceived` が在る行を matched・無い行を id で名指す（読むだけ・判定しない・C10.2・分が曖昧な形〔`4xZ`〕は母集団に入らず `skipped=<n>` で数える・同じ分に event が複数在る行は matched に数え件数を `matched=<n>` の内訳に持たない＝1 行が 1 件を一意に指すことは保証しない）。
- 触らない: `ApprovalRequested` / `ApprovalReceived` / `QuestionRaised` / `QuestionAnswered` の形と読み手・`pipe approve` / `pipe answer`・resume の経路・直命の表（[working-memory.md](./working-memory.md) §12.1・別 kind）・schema 版。
- 歯（`fleet_ruling_` 接頭辞・`tests/e2e/fleet.rs` と `tests/e2e/seat/ruling.rs`〔新規〕・`prop.rs` の生成器は variant を足すだけ）: 対話面の役割の登録 row を持つ target で `seat ruling add` が `RulingReceived` を 1 件（`actor=human`・`run` 無し・逐語が detail に逐語で）書く／逐語が空なら書かず rc 1／登録 row の役割が R-C7-1 の値でない target は `not-dialogue-surface` で書かない／`fleet record --kind RulingReceived` は断る／`pipe report` の `rulings=` が数え `human_events_other_than_approval` が増えない／doctor が manifest の `user <ts>` 行に対して同じ分の event の有無で matched / unmatched を出す／外形 snapshot（seat usage・doctor・pipe）が更新される。
- 却下: ADR-0037 §3（写しは持たない）。

## 10. 追記は 1 記録を 1 回の write で撃ち、改行で終わらない末尾を先に切り離す（契約表の行 d・memo `s2-07l.711`）

- 何が起きているか（2026-09-28）:
  - verified: `crates/scribe2/src/fleet/store.rs` の `write_line` は `writeln!` で書く。fmt の adapter は本文と改行を別の write に分けうる。本文の後・改行の前に書き手が死ぬと、file は改行の無い完全な記録で終わり、次の書き手の記録が同じ行に続く。その 1 行は 2 つの記録を持つ malformed になり、`read_all` は Err を返す（NFR4・fail-closed）。置き場の replay の全部が止まり、人が手で直すまで戻らない。
  - verified: 追記の lock は `Reclaim::Stale` で回収され（rules 行 `fleet.lock_stale_ms` = 30000）、30 秒を越えた lock は持ち主が生きていても外される。deduced: 本文と改行のあいだに別の書き手が入る経路は、書き手が死んだ周に限らない。
  - verified: 再発は 0 件。本 repo と隣の project の置き場 9 つの event log で、1 行に 2 つの記録を持つ行は 0 件（本 repo の置き場は 31532 行）。
- 形（番号は done と 1:1）:
  1. `write_line` は本文と改行を 1 つの buffer に詰め、`write_all` を 1 回だけ撃つ（fmt の adapter を通さない）。
  2. lock の中で、書く前に file の末尾の 1 byte を読む。file が空でなく末尾が改行でない周は、記録の前に改行を 1 つ足す（同じ 1 回の write の頭に置く）。死んだ書き手が残した改行の無い完全な記録は、次の記録から切り離されて単独の行として読める。途中で切れた記録は単独の malformed の行として残り、`read_all` は今どおり Err を返す（直すのは人）。
  3. 効く範囲は `append_line` を通る全部の file（event log・着地と gate の記録・席の打刻）。lock の実装は 1 本のまま（C6.3）で、第 2 の writer を作らない。
  4. 変えないもの: schema・1 行 1 event・lock と回収・rules 行・`Warning` の値（改行を足しても何も消えないので warning は足さない）・`read_all` の判定。
- 歯（`fleet_torn_line_` 接頭辞・`crates/scribe2/src/fleet/store.rs` の `mod tests`・`grep -rn "fleet_torn_line" crates/` は 0 件・2026-09-28）:
  - (a) 改行の無い完全な記録で終わる event log に `append` で 1 件足すと、`read_all` が 2 件を Ok で返す。続けてもう 1 件足すと 3 件で、file は空の行を持たない（改行で終わる log には改行を余分に足さない）。base は 1 行に 2 件が並び、line=1 の Malformed の Err になる（RED・機能不在）。
  - (b) 途中で切れた記録で終わる event log に 1 件足すと、file の行数が 2 で、`read_all` の Err は line=1 の 1 件だけ。base は行数 1（RED）。
- 限界: 切れた記録そのものは直さない（fail-closed のまま人が直す）。1 記録を 1 回の write で撃つこと（形 1）は歯で数えない（書き手を差し替えられる形に変えないと数えられない・code の読みで確かめる）。
- 却下:
  - 読み手が 1 行に並んだ 2 つの記録を割って読む。1 行 1 event（§3）を読み手の側で崩し、切れた記録と並んだ記録を見分ける読みが要る。
  - 書き手が切れた末尾を消してから書く。消した記録は戻らない（N1）。
  - 1 回の write の原子性だけに頼り、形 2 を持たない。死んだ書き手が残した末尾は、それでも改行を持たない。

## 11. 本文の読めない古い lock は DeadOnly の周でも外す（契約表の行 e・memo `s2-07l.736.8`）

やさしく言うと: lock の中身（持ち主の番号）が空のまま残ると、器は「持ち主が生きているかもしれない」と読んで、永久に待っていた。中身が読めず十分に古い lock は、持ち主が居ないものとして外す。

- 何が起きているか（2026-09-29・verified）: 隣の project の置き場で、掃除の lock（[dispatcher.md](./dispatcher.md) §30）が本文 0 byte のまま 2026-09-28T10:16Z から残った。掃除は 65 回続けて `sweep: skipped=lock` で撃たれず、退役した便の build の置き場が 54 本・約 589 GB 溜まり、host の disk が 98% になった。所有者の process は生きていない。orchestrator が lock を同じ dir の別名へ移して（可逆）掃除を再開させた。
- 現物（main 1862abd0・verified）: `crates/scribe2/src/fleet/store.rs` の `acquire_in` は、create_new で開いた後に本文を書く。その間で process が死ぬと、本文の無い lock が残る。`lock_owner` は 2 形のどちらでもない本文（空を含む）を読めないと判じ、その doc は「stale の線に従う」と書く。ところが DeadOnly の周は stale の線を見ない。DeadOnly を使うのは掃除の lock（`crates/scribe2/src/pipe/sweep.rs`）と driver の札（`crates/scribe2/src/pipe/mod.rs` の `hold`）の 2 つで、どちらも本文の読めない lock が 1 つ残ると永久に取れない。
- 形（done と 1:1）:
  1. `acquire_in` の古い lock の枝: 観測した本文が 2 形のどちらでもない（空を含む）周は、Reclaim の値に依らず、mtime が `stale_ms` より古ければ回収の 1 手（`reclaim`）で外し、warning `StaleLockRemoved` を積む。
  2. 本文が読める lock の扱いは変えない: 生きている所有者の lock は DeadOnly の周に古くても奪わない。probe が読めない周も DeadOnly では外さない（fail-closed）。死んだ所有者の lock は今までどおり外す。
  3. 本文の書き方・lock の path・`reclaim` の 1 手・token の扱い・`LockPolicy` と rules 行 `fleet.lock_stale_ms` は変えない。
- なぜ安全か: 本文の読めない lock を生きている所有者が握るのは、create_new と本文の書きの間の μs の窓だけで、そのとき mtime は新しい（`stale_ms` より古くならない）。本文を書けない周は `acquire_in` が lock を消して error を返すので、本文の無い lock を握り続ける生きた所有者は居ない。
- 却下: (a) 本文を一時 file に書いてから hard link で lock の path に置く（空の lock は生まれなくなるが、既に在る空の lock と壊れた本文を回収できず、読みの側の直しが結局要る・C17 の最小の段）。(b) 掃除の lock だけを Stale の回収に変える（生きている長い掃除の lock を古さで奪う・§30 の決め）。(c) 空の lock を器の起動時に消す（所有者の判定を 2 か所に持つ）。
- 残り（memo に残す）: 掃除が `sweep: skipped=lock` を続けても、doctor と管理 tick に出ない（気づく口が無い）。生きている所有者が掃除の途中で止まった周も同じく気づけない。
- 歯（in-file・`crates/scribe2/src/fleet/store.rs` の `mod tests`・接頭辞 `fleet_lock_unparsed_`・`grep -rn fleet_lock_unparsed crates/` は 0 件・2026-09-29）:
  - `fleet_lock_unparsed_stale_body_is_reclaimed_under_dead_only`: 本文が空の lock と、2 形のどちらでもない本文（例 abc）の lock を `stale_ms` より古くした周に、DeadOnly で取れて warning が `StaleLockRemoved` の 1 件（base は `retry_ms` の後に error＝RED）。
  - `fleet_lock_unparsed_fresh_body_waits_under_dead_only`: 同じ 2 つの本文でも古くない周（`stale_ms` を大きく）は DeadOnly で取れず、lock は残る（base でも緑・非空虚の対）。
- base で RED の理由: 1 本目は、base の `acquire_in` が DeadOnly の周に古さを見ないので取れない（機能不在）。

## 12. 案件の一生の event の kind 5 つを読み書きの両側で先に足す — 書き手の行より前に閉じた型の閉包を 1 回で広げる（契約表の行 f・[ADR-0087](../../design-intent/decisions/ADR-0087-user-utterances-are-sorted-three-ways-and-seat-questions-go-to-the-ledger.html)・[ADR-0088](../../design-intent/decisions/ADR-0088-case-positions-are-computed-once-by-the-vessel-and-read-from-one-file.html)）

やさしく言うと: 後の行が書く 5 種類の記録（発話・仕分け・turn の終わりの読めない周・受付の断り・切り替えの線）を、先に「読める・書ける」ようにしておく。記録の種類の一覧を 1 回だけ広げ、書き手の行ごとに一覧を触らなくて済むようにする。

- 要件: [FR82](../../design-intent/spec/srs.html#FR82)（発話 event）/ [FR88](../../design-intent/spec/srs.html#FR88)（仕分けの event・turn の終わりの読めない周）/ [FR90](../../design-intent/spec/srs.html#FR90)（切り替えの線）/ [FR68](../../design-intent/spec/srs.html#FR68)（受付の断りの event）/ [FR22](../../design-intent/spec/srs.html#FR22)（発話は人由来に数えない）。kind と key の名は ADR-0087（発話・仕分け・turn の終わり）・ADR-0088（受付の断り・切り替えの線）・[ADR-0083](../../design-intent/decisions/ADR-0083-rulings-bind-ledger-questions-to-recorded-utterances.html)（発話の kind と actor）が決めた。kind は閉じた型の末尾に足し、key を足すだけなので schema 版は 1 のまま（ADR-0004 §2.5）。
- 何が起きているか（main 24f6ef1e・verified）:
  - `EventKind` は 24 値で、`KINDS` の末尾は `SeatRetired`。5 つの kind の名も 7 つの新しい key（channel・session・utterance・sorting・refuse・version・main）も crates の中に 0 件。今の器は 5 kind の行を 1 行でも読むと `read_all` が「未知の kind」の Err を返し、その置き場の replay の全部が止まる（NFR4・fail-closed）。
  - `EventKind` の網羅 match は 6 か所: `crates/scribe2/src/fleet/mod.rs` の `as_str`・`default_actor`・`shape`、`crates/scribe2/src/fleet/event.rs` の `read`、`crates/scribe2/src/fleet/replay.rs` の `apply_account` と `apply_seat`。`Shape` の網羅 match は `crates/scribe2/src/fleet/event.rs` の 3 か所（`to_line`・`install`・`pressure`）。`KINDS` の件数と末尾の pin は 4 file・5 か所（`crates/scribe2-boundary/tests/e2e/fleet.rs` の `fleet_kinds_follow_declaration_order`・`crates/scribe2-boundary/tests/e2e/fleet/account.rs` の `account_cmd_kinds_are_fifteen_with_retire_and_restore_last`・`crates/scribe2-boundary/tests/e2e/fleet/json.rs` の `fleet_replay_seat_retired_kind_is_a_registration_shape_and_record_refuses_it`・`crates/scribe2-boundary/tests/e2e/pipe/gate.rs` の `pipe_regate_returns_gated_fail_to_implemented_on_the_same_worktree` と `pipe_follow_step_moves_gated_tree_onto_main_and_returns_to_implemented`）。
  - `Event` の literal 構築点（field を全部並べる形）は 19 file・34 か所（src 11 file・19 か所、歯 8 file・15 か所・一覧は行 f の write-set）と、`crates/scribe2/src/fleet/event.rs` の `from_line` の 1 か所。`..` の struct update の構築点（`crates/scribe2/src/pipe/stop.rs` ほか）は field を足しても直さなくてよい。
  - `fleet record` の断りは `crates/scribe2/src/fleet/cli.rs` の `build_event` の手書きの列（口座残量と 8 kind）で、`DispatchMark` と `GroupMoved` / `GroupMoveRefused` / `GroupMovePending` を通す。通した行は本体（列の印・口座 label と detail）を持たないので読み手が malformed と読み、次の `fleet show` から rc 2 になる（PATH の binary で置き場を別に作って撃った実測: `DispatchMark` は rc 0 の後に `mark が無いか文字列でない line=1`、`GroupMoved` は `detail が無いか文字列でない line=1`）。
  - `crates/scribe2/src/pipe/report.rs` の `count` は actor が human の行を全部数える。UtteranceReceived の既定の actor は human（ADR-0083）なので、書き手の行が着地すると発話 1 つごとに `human_events_other_than_approval` が 1 増える（FR22 は発話を人由来に数えないと言う）。
  - insta の snapshot で kind の一覧を pin するものは 0 件。`fleet_external_form` の snapshot は `fleet record` の断りの 1 行（`InstallRecorded` の断りの字面）を pin する。
- 形（番号は done と 1:1）:
  1. `EventKind` の末尾に 5 variant を UtteranceReceived → UtteranceSorted → TurnEndUnjudged → IntakeRefused → LifecycleCutover の順で足し、`KINDS` も同じ順（29 種）。既定の actor は UtteranceReceived だけ human、ほかの 4 つは machine。5 kind は `Shape` の新しい値 1 つ（`SHAPES` の末尾）を共有する。replay は 5 kind の行で便も席も登録 row も口座の退役も作らない（`apply_run` は Run の形の外の行を今の枝で捨て、`apply_account` と `apply_seat` の網羅 match には何もしない arm を足す）。
  2. `Event` に field を 1 つ足し、5 kind の本体を閉じた enum 1 つ（5 variant）で持つ。発話の経路（chat / gui）と仕分け（request / chat）は、全 variant の const slice を持つ閉じた enum にする（`enum-slices` の検査が測る形）。仕分けの memo と受付の断りの契約の id は、今の `bead` の field に持つ。
  3. 行の読み書きは下の表のとおりにする。`KNOWN_KEYS` に 7 key を足し、7 key は、その key を持つ kind の外の行に在れば malformed（`COST_KEYS` / `RULING_KEYS` と同じ置き方）。TurnEndUnjudged の reason は、口座残量と共有する key（`ALLOWANCE_KEYS`）のうち reason だけを開ける。必須の欠け・閉じた値の外・空の値・経路と session の食い違い・仕分けと bead の食い違いは、key を名指す理由つきの Err にする。書き手は表の並びで書き、読んだ行を書き直すと同じ字面になる。`to_line` は今 52 行で上限 60 行に近いので、本体の key の並びは本体の enum の側の関数 1 つに置く。
  4. `ts` は今のまま文字列で読む（秒より下の桁の字面も読める）。発話の ts の字面と、一意にする手は、書き手の行 lc-a4 が決める。schema 版は 1 のまま。
  5. `fleet record` は、`Shape` が Run でない kind を全部断る（rc 1・stderr は今の `kind <k> は record では書けない`・log を作らない）。断りの手書きの列は消して、形から導く（C2）。5 kind に加えて、今は通って読めない行を残す `DispatchMark` と群の移動の 3 kind も、断る側へ移る。
  6. `pipe report` の人由来の数え（`human_events` と `human_events_other_than_approval`）は UtteranceReceived を数えない（FR22）。行の字面と rulings の数えは変えない。`crates/scribe2/src/pipe/report.rs` には `EventKind` の match の arm を書かない（`EventKind` を touches に持つ dispatcher の行 a の閉包を広げない）。
  7. `KINDS` の件数と末尾を pin する既存の歯 5 か所を、29 種と新しい末尾に書き換える。`Event` の literal 構築点 34 か所に、新しい field の空の値を 1 行ずつ足す。`fleet_external_form` の snapshot は変わらない（断りの字面が同じ）。

  kind と key の表（行の key の並びは schema・ts・kind・下の「本体の key」・host・actor・detail の順）:

  | kind | actor | 本体の key（この順） | 必須 | 任意 | 値の形 |
  |---|---|---|---|---|---|
  | UtteranceReceived（発話 event） | human | channel・session | channel・detail（逐語）・channel が chat の行の session | — | channel は chat / gui の 2 値。gui の行は session を持たない。session は空でない文字列。detail の空を断るのは書き手 |
  | UtteranceSorted（仕分けの event） | machine | utterance・sorting・bead | utterance・sorting・sorting が request の行の bead | detail | utterance は発話の ts の字面（空でない）。sorting は request / chat の 2 値。chat の行は bead を持たない。bead は開いた memo の id |
  | TurnEndUnjudged（turn の終わりの読めない周） | machine | session・reason | reason | session・detail | reason は空でない語（語の一覧は行 lc-e1b）。session は空でない文字列 |
  | IntakeRefused（受付の断りの event） | machine | bead・refuse | bead・refuse | detail | refuse は受付の断りの名（空でない・語の一覧は受付の側が持つ） |
  | LifecycleCutover（切り替えの線） | machine | version・main | version・main | detail | version は線を引いた器の版（空でない）。main は小文字の 16 進 |

  5 kind とも run・stage・seat・pid・account・口座残量・登録・列の印・消費・rule の key を持たない（在れば malformed）。bead は表で必須か任意と書いた行のほかでは持たない。

- 構築点の母集団と測り方: `grep -rn --include=*.rs -E '(^|[^A-Za-z_:])Event \{' crates/` と、`crates/scribe2-boundary/tests/e2e/prop.rs` の別名の構築。数えないのは、戻り型（`-> Event {`）、宣言（`crates/scribe2/src/fleet/event.rs`）、別の型の `Event`（`crates/scribe2/src/seat/state.rs`）、`..` の struct update。他の便の着地で増えるので、release の前に今の main で測り直し、差があれば docs PR で一覧と行の write-set を同時に直す。
- 触らない:
  - 書き手は書かない: 発話の記帳（行 lc-a4）・仕分けの口（lc-e1a）・turn の終わりの hook（lc-e1b）・受付の断りの記帳（lc-e3f）・切り替えの線（lc-e3c）。この行の後も、log に 5 kind の行は 0 件のまま。
  - `RulingReceived` の key の追加（ruling・utterance・asked・question_ts・channel）は行 lc-a5。この行は channel を UtteranceReceived にだけ開き、lc-a5 が `RulingReceived` へ広げる。
  - 既存の kind の読み方と並び・events.jsonl の path と lock・`fleet export` と `fleet show` の形・`pipe report` の行の字面（3 欄は行 lc-e22）・`DispatchMark` と群の移動の kind の書き手（pipe の口）。
- 却下:
  - kind を書き手の行ごとに足す。閉じた型の match の閉包が行ごとに広がって、別 doc の行と交差し、5 行が直列になる。
  - 5 kind の本体を field 5 つで持つ（口座残量・登録と同じ形）。literal 構築点 34 か所に 5 行ずつ（170 行）足すことになる。閉じた enum の field 1 つなら 1 行ずつで済む。
  - 本体を detail の 1 行に詰める（`InstallRecorded` と同じ形）。ADR-0087 と ADR-0088 は key の名を行の key として決めた。detail の字面を判定に読むのは C3.3 の typed payload に反する。
  - `fleet record` の断りの列に 5 kind を足すだけにする。手書きの列は `DispatchMark` と群の移動の 3 kind を漏らして、読めない行を残している（実測）。形から導けば、kind を足した周に列を直す手が要らない。
  - 5 kind に `Shape` の値を 1 つずつ持たせる。`Shape` の網羅 match 3 か所に 5 arm ずつ増えるのに、どの読み手も 5 つを形では見分けない（見分けは kind で行う）。
  - IntakeRefused の refuse を、受付の断りの名の閉じた列で読む。fleet は pipe に依存しない（層の向き）。断りの名は受付の側で増えるので、fleet で閉じると、断りを足す便ごとに fleet の読み手が動く。
- 歯（`crates/scribe2-boundary/tests/e2e/fleet/json.rs` に置き、接頭辞は fleet_case_kind_。`grep -rn fleet_case_kind crates/` は 0 件・2026-09-29。e2e の新しい file は作らない。json.rs の module doc の接頭辞の列に 1 つ足す）:
  - fleet_case_kind_lines_round_trip_through_the_log: 下の見本 7 行が、どれも `from_line` で Ok になり、`to_line` で同じ字面に戻る。`EventKind` の `parse` が 5 つの名を引ける。7 行を `append` で書き、`read_all` で 7 件が同じ値で戻る。
  - fleet_case_kind_rows_name_the_bad_key: 次の行は、どれも key か値を名指す Err になる。channel の欠け・channel が chat / gui の外・chat の行の session の欠け・gui の行の session・run を持つ発話・request の行の bead の欠け・chat の仕分けの bead・sorting が 2 値の外・utterance の欠け・reason の欠け・seat を持つ TurnEndUnjudged・refuse の空・version の欠け・main が 16 進でない。さらに `RunStage` の行の channel と `RulingReceived` の行の version も、key を名指す Err になる（こちらは base でも緑・非空虚の対）。
  - fleet_case_kind_record_refuses_every_non_run_shape: Run の形でない `KINDS` の全部と 5 つの名を `fleet record --run r1 --bead b1` で撃つと、どれも rc 1 で、stderr に「record では書けない」が出て、log の file を作らない。
  - fleet_case_kind_replay_makes_no_run_or_seat: `RunCreated` 1 行と見本 7 行の log で、replay の便は 1 つ・席 0・登録 row 0・退役 0。`fleet export` の 1 行目が runs 1・seats 0。
  - fleet_case_kind_utterance_is_not_counted_as_human: UtteranceReceived（actor human）1 行と、逐語つきの `ApprovalReceived` 1 行の列で、`count` の `human_events` が 1、`human_events_other_than_approval` が 0。
  - fleet_case_kind_kinds_are_appended_in_order: `KINDS` が 29 種で、24 番目から後ろの字面が 5 つの名の順。既定の actor は 1 つ目だけ human。5 つとも `Shape` が Run でなく、互いに同じ値。
  - 書き換える既存の歯（5 か所・上の一覧）は、29 種と新しい末尾を測る。

  見本 7 行（秒より下の桁の ts は、読み手が読めることを測るだけ。字面の決めは lc-a4）:

  ```
  {"schema":1,"ts":"2026-09-29T01:02:03.004Z","kind":"UtteranceReceived","channel":"chat","session":"s-1","host":"h","actor":"human","detail":"進めて"}
  {"schema":1,"ts":"2026-09-29T01:02:04Z","kind":"UtteranceReceived","channel":"gui","host":"h","actor":"human","detail":"A で"}
  {"schema":1,"ts":"2026-09-29T01:03:00Z","kind":"UtteranceSorted","utterance":"2026-09-29T01:02:03.004Z","sorting":"request","bead":"s2-m1","host":"h","actor":"machine"}
  {"schema":1,"ts":"2026-09-29T01:03:01Z","kind":"UtteranceSorted","utterance":"2026-09-29T01:02:04Z","sorting":"chat","host":"h","actor":"machine"}
  {"schema":1,"ts":"2026-09-29T01:04:00Z","kind":"TurnEndUnjudged","session":"s-1","reason":"log-unreadable","host":"h","actor":"machine"}
  {"schema":1,"ts":"2026-09-29T01:05:00Z","kind":"IntakeRefused","bead":"s2-c1","refuse":"cap-headroom","host":"h","actor":"machine"}
  {"schema":1,"ts":"2026-09-29T01:06:00Z","kind":"LifecycleCutover","version":"0.1.0","main":"0123456789abcdef0123456789abcdef01234567","host":"h","actor":"machine"}
  ```

- base で RED の理由:
  - 新しい歯（json.rs）は、base でも compile が通る形で書く（5 つの名は字面で持つ）。そのうえで、assert で落ちる（機能不在）: 5 kind の行は「未知の kind」で読めない。`fleet record` の断りの字面は「未知である」で「record では書けない」でない。`DispatchMark` と群の移動の 3 kind は rc 0 で通る。`KINDS` は 24 種。
  - 書き換える既存の pin: account.rs と gate.rs は、24 種の base で assert が落ちる。fleet.rs は、構築点に新しい field を足した helper が base で compile できずに落ちる（flip-check は overlay 後の compile error を RED と数える）。
  - 構築点だけを直す file（prop.rs・hook.rs・ledger_memo.rs・e2e の pipe/dispatch.rs・ratelimit.rs・seat.rs・seat/account.rs・src の in-file の helper）は、動く行が `#[test]` の fn の外だけなので、歯の本体として単独では撃たれない。

## 13. user の prompt を model が読む前に発話 event として 1 件記帳する — UserPromptSubmit の hook が、差し込みの行・別の session の包み・runner・marker の外を除いて、逐語を ts（ミリ秒・発話ごとに一意）付きで書き、ts の 1 行を返す（契約表の行 g・[ADR-0083]・[ADR-0087]・FR82 / AC52・FR88 の読む byte）

やさしく言うと: user が席に打った字を、器が先に記録に残す。後で「この答えはこの問いへ」と結ぶには、打った字が消えずに、1 つずつ見分けられる番号（時刻）で残っていることが要る。この § は、その記録の取り方と、記録しないもの（器が自分で送った合図・別の session からの便り・無人の runner）を決める。

- 何が起きているか（main 3908279b・verified）:
  - `hook user-prompt-submit` の枝（`crates/scribe2/src/hook/mod.rs` の `dispatch`）は、状態の打刻（Busy）と群の逼迫の行（`hook/group.rs` の `lines`）だけを撃つ。prompt の字を読む code は 0 件。
  - 行 f で `UtteranceReceived`（actor human・`Case::Utterance { channel, session }`・逐語は detail）の読み書きは着地済みで、書き手が 0 本。`pipe report` は発話を人由来に数えない（`pipe/report.rs` の filter）。
  - 仕える repo の判定は `anchor_of`（marker が器の NAME を言う repo だけ `Hooked` になる）。marker の無い repo と別の NAME の repo は今も 1 byte も書かない。
  - runner は `headless` の起動で `TMUX_PANE` を外し、便の plugin の写し（`crates/scribe2/src/pipe/mod.rs` の `plugin_path` の下）を `--plugin-dir` で積む。その hooks.json も UserPromptSubmit を持つので、runner の `-p` の prompt もこの hook を通る。lens の起動行は `--plugin-dir` を持たず、hook が撃たれない。
  - 器が入力欄へ差し込む行は、全部 `<NAME> <語>:` で始まる（語は pipe・seat・group・tick の 4 つ）。作り手: `pipe/notify.rs` の終端・idle・precheck の 3 行、`seat/deliver.rs` の `line`、`seat/tick/signal.rs` の `signal`（打刻の合図）、`seat/tick.rs` の `relaunch_signal`、`hook/group.rs` の `evacuate_line` と `refused_line`、`pipe/dispatch/group.rs` の逼迫の行。合わせて 9 種。
  - 時刻の字面は `fleet/cli.rs` の `format_utc`（秒まで）の 1 本で、逆の読みは `fleet/wait.rs` の `epoch_of`（秒の形だけを読み、ほかは None）。
  - payload の文字列を読む `crates/scribe2/src/hook/mod.rs` の `field` は escape を解かない（最初の `"` で切れる）。入れ子も読める JSON の読み手は `fleet/json_tree.rs` の `parse`。
  - 既存の e2e の payload（`stamp_payload`）は prompt の key を持たない。
- 約束（番号は done と 1:1）:
  1. **置き場**: 判定と記帳は hook の子 module 1 つ（行 g の write-set の `+` の file）。`user-prompt-submit` の枝で打刻の後・群の行の前に 1 回呼ぶ。
  2. **読む値**: payload を `json_tree::parse` で読み、`prompt`（逐語・escape を解いた字）と `session_id` を取る。prompt の key が無い・空白だけの周は何も書かず、何も出さない（既存の歯の payload はこの形なので、出力が変わらない）。
  3. **除外（閉じた 4 つ）**。当たる周は記帳も出力もしない。
     - (a) 差し込みの行: prompt の頭が `<NAME> <語>:` で、語が閉じた 4 語（pipe・seat・group・tick）のどれか。
     - (b) 別の session と harness からの包み: prompt の頭（先頭の空白を除く）が閉じた一覧の字（const slice 1 本・5 つ）のどれかで始まる。
       - `Another Claude session sent a message:`（別の session と teammate の message の包みの 1 行目）
       - `<cross-session-message`・`<teammate-message`（包みの 2 行目が頭に来る形）
       - `<task-notification>`（背景の task と subagent の完了の知らせ）
       - `This session is being continued from a previous conversation`（compaction の後の要約）
       - 出所: orchestrator の席の transcript 1 本（2026-09-30）で、user の turn の本文の頭を数えた実測。`<task-notification>` 297 件、1 行目が `Another Claude session sent a message:` の包み 136 件（2 行目が `<cross-session-message` 107 件・`<teammate-message` 29 件）、compaction の要約 44 件。背景の task の知らせと compaction の要約は別の session ではないが、user の発話でもないので同じ一覧で外す。
     - (c) runner: `--plugin-root` が anchor の state dir の `pipe` の dir（`pipe::run_dir` と同じ導出）の下。
     - (d) lens は hook が撃たれないので code は持たない（`headless/lens.rs` の起動の `Call` は `plugin_dir: None`・e2e の `headless.rs` の偽の claude の argv の読みで確かめられる既存の事実）。本行は lens に触らないので、この除外は歯を持たない。
  4. **ts**: 秒の下にミリ秒の 3 桁を持つ UTC の字面（`YYYY-MM-DDTHH:MM:SS.mmmZ`）。作る関数は `fleet/cli.rs` の `format_utc` の隣の 1 本、逆の読みは `fleet/wait.rs` の `epoch_of` の隣の 1 本（ミリ秒の形だけを読む）。`wait` は `fleet/mod.rs` の私有の `mod wait` なので、足した読みは同じ file の `pub use wait::{…}` の列（今は `epoch_of` などを出す 1 行）に 1 語足して crate の外にも出す。
  5. **一意の振り方**: `fleet/store.rs` に追記の関数を 1 本足す。lock の中で今の時刻を取り、log の末尾の固定の byte（64 KiB）だけを後ろから読んで、最後の発話 event の ts を探す。振る ts は「今」と「最後の ts + 1 ms」の大きい方。窓の頭で途切れた 1 行は読まずに捨てる。読む byte は log の大きさに依らない（NFR5）。既存の `append_if` は log 全体を読むので使わない。
  6. **書く行**: `UtteranceReceived`・actor human・channel chat・session は payload の値・detail は逐語そのまま（改行を含めて 1 byte も変えない）・run は無し。
  7. **返す 1 行**: 記帳できた周だけ stdout に `<NAME> utterance: ts=<ts>` を 1 行出す（群の行より前）。逐語は載せない（FR65）。席はこの ts で、同じ turn の中で仕分けか結びを撃てる。
  8. **止めない**: session_id が無い・lock が取れない・書けない周は、prompt を止めずに rc 0 で終え、stderr に `<NAME>: utterance unrecorded reason=<語>` を 1 行出す。語は閉じた 3 つ（no-session・lock・write）。
  9. **台帳を読まない**: 開いた台帳の問いの有無で記帳を変えない。bd を 1 回も撃たない。
- 歯（e2e は既存の `tests/e2e/hook/session.rs` に足す。新しい e2e の file は作らない。lib は行 g の `+` の file の歯の区間）:
  - e2e `hook_utterance_record_`:
    - (a) 開いた問いが在る時と無い時（偽の bd が呼ばれた回数を数える）の 2 本で、発話 event が 1 件ずつ・逐語は改行と非 ASCII と `"` を含めて一致・session と channel chat・actor human・run の key 無し・bd の呼び出しは 0 回・stdout は ts の 1 行。ts は `YYYY-MM-DDTHH:MM:SS.mmmZ` の字面で stdout と event で同じ。stdout に逐語の字が無い（逐語は ts と NAME に無い字で作る）。
    - (b) 差し込みの 9 種の見本と包みの 5 つの頭の見本（それぞれ後ろに本文の行を持つ）と runner の plugin-root で、どれも記帳が 0・stdout は 0 byte。
    - (b2) 除きすぎない対（約束 3 の閉じた形の外は記帳する）: `<NAME>` の後の語が閉じた 4 語の外（例 `<NAME> note:`）、`<NAME>` でない名の後に `pipe:`、包みの頭の字が 1 行目でなく 2 行目に在る prompt、`--plugin-root` が state dir の `pipe` の dir の外（隣の dir）の 4 形は、どれも 1 件ずつ記帳されて stdout が ts の 1 行。先頭に空白を置いた包みの頭の字は除かれる（0 件）。
    - (c) marker の無い repo と別の NAME の repo で 0 件。
    - (d) 同じ秒に 2 回撃つと ts が 2 つとも違い、log の順に増える（hook の経路の一意と順。+1 ms の枝は lib の (f) が決定的に測る）。
    - (e) `pipe report` の human_events が発話の前後で同じ。
    - (k) 何も書かない 2 形（約束 2）: prompt の key の無い payload と、空白だけ（空白・改行・tab）の prompt。どちらも rc 0・stdout と stderr が 0 byte・記帳 0。同じ歯の中で先に普通の prompt を 1 件記帳させる対照を置く（base で RED）。
    - (l) 止めない 3 形（約束 8）: session_id の無い payload で reason=no-session、生きた pid（test の process）を本文に持つ event log の lock を置き、rules の写しの行 fleet.lock_retry_ms を 50 にして `--rules` で渡す周で reason=lock、event log の path を dir にした置き場で reason=write。どれも rc 0・stdout 0 byte・stderr が `<NAME>: utterance unrecorded reason=<語>` の 1 行だけ・記帳 0。対照は (k) と同じ。
    - (m) 撃つ位置（約束 1・約束 7）: 既存の `tests/e2e/hook/group.rs` の群の逼迫の fixture（UserPromptSubmit の追加文脈に群の行が 1 行出る形）に prompt を持つ payload を渡すと、stdout は 2 行で、1 行目が `<NAME> utterance: ts=<ts>`・2 行目が群の行。発話 event は 1 件、席の状態の打刻は Busy。
  - lib `utterance_tail_`（(f)〜(h) は store の追記の 1 本〔約束 5〕を直接呼ぶ。log は歯が event の行を書いて作り、撃つ前と後の時刻で「今」を挟む）:
    - (f) 最後の発話の ts が今より 1 時間先の log（発話 1 件）に 1 件足すと、振られた ts がちょうど先の ts + 1 ms。今だけを振る実装と、末尾を読まない実装を落とす。
    - (g) 同じ先の発話の後に、発話でない event を合わせて 80 KiB 続けた log では、振られた ts が今（撃つ前と後の時刻の間）。読む byte が末尾の 64 KiB に閉じることを測り、log の全部や窓の外まで遡る実装を落とす。
    - (h) 同じ先の発話の後に、発話でない event を合わせて 60 KiB 続けた log では、振られた ts が先の ts + 1 ms。最後の 1 行だけを見る実装を落とす。
    - (i) 作り手の 9 種の 1 行を実物の関数で作り、(a) の判定が全部を差し込みと読む（種の数を const の表から数えて母集団として出す）。
      - 作り手のうち 3 つは私有の module の中に在る（`pipe/mod.rs` の `notify`・`pipe/dispatch.rs` の `group`・`seat/tick.rs` の `signal`）。この 3 つの mod 宣言を `pub(crate)` に広げ、作り手の関数も crate の中から呼べる可視性にする。行は増やさない。
    - (j) ミリ秒の字面の作りと読みが往復する。
    - 前の版の (f)（数える reader を通した 10 MB と 20 MB の log で読む byte が同じ）は (g) に置き換えた。(g) は同じ約束（読む byte が log の大きさに依らず末尾の窓に閉じる）を reader の seam 無しで測る。前の (f) だけだと、末尾を全く読まない実装でも緑になる。
  - base で RED の理由: 機能不在。書き手が無いので (a)(d) は 0 件で落ちる。lib の (f)〜(j) は新しい file の中なので、base では該当 0 本（rc 4）。(b)(c)(e)(k)(l) は除外や不発だけだと base で緑になる。そこで同じ歯の中で先に普通の prompt を 1 件記帳させる対照を置き、base で RED にする（負例だけの歯にしない）。
- 触らない:
  - 打刻（Busy）・群の行・Stop・SessionStart。
  - 行 f の event の型と読み手。
  - `pipe report` の字面。
  - FR44 の差し込みの作り手の字面（テストの区間を除く）。
  - 仕分けの判定と turn の終わりの止め（行 lc-e1a・lc-e1b）。
- 限界:
  - user が手で `<NAME> pipe:` で始まる字を打つと、差し込みと読んで記帳しない。
  - 1 秒に 1000 を越える発話は、ts が次の秒の字面へはみ出す。一意と順は保つ。
  - 64 KiB より前の発話は見ない（同じ ms に 64 KiB を越える追記が挟まる周だけ、一意が崩れうる）。
  - lock の待ちは store の既存の取り方のまま rules 行 fleet.lock_retry_ms（今 5000 ms）まで続く。lock が長く取られた周は、user の prompt がその間だけ待つ（rules 行 hook.budget_ms の 2000 ms を越えうる）。待ちを hook の予算に揃えるかは別 memo。
  - ミリ秒の ts は `epoch_of` では読めない。発話の年齢を測る後の行は、約束 4 の読みを使う。
  - 包みの字は CLI の版で変わりうる。変わると記帳が増える向きに倒れる（止める向きではない）。
- 却下:
  - 記帳しないで、turn の終わりに transcript から拾う（逐語が model に読まれた後になり、FR82 の「読まれる前」を破る）。
  - ts の一意を sidecar の file で持つ（on-disk の形が増える・ADR の条件 3）。
  - `append_if` で log 全体を読む（NFR5 に反する）。
  - pane の有無で runner を見分ける（tmux の外で対話の claude を開いた user を除いてしまう）。
  - ts の行を返さない（席が ts を知るのが turn の終わりの止めの後だけになり、発話ごとに 1 回ずつ余計に止まる）。

## 14. 器の結びの口 seat ruling bind — 記帳された発話と開いた台帳の問いを結び、裁定 id を発行して 5 欄の裁定の行を notes に書き、`裁定 <id>` で close して、裁定 event に 5 つの key を足す。逐語を受ける `seat ruling add` は消す（契約表の行 h・ADR-0083・ADR-0087・ADR-0089・FR82 / FR89 / AC52 / AC59）

やさしく言うと: 席が user に問いを出し、user が端末で答えたとき、その答えの字（記録済みの発話）を問いに「結ぶ」口を器に 1 つ作る。結んだ証拠として、番号（裁定 id）を配り、問いのメモ欄に 1 行書き、問いを閉じ、記録を 1 件残す。席が自分で答えの字を書き込む口（`seat ruling add`）は、打たれていない字を裁定にできてしまうので消す。

- 何が起きているか（main 3908279b・verified）:
  - `seat ruling` の口は `add`（逐語を `--words` で受けて `RulingReceived` を書く・対話面の役割だけ）と `ls` の 2 つ（`seat/ruling.rs`・`seat/cli.rs` の `ruling_of` / `ruling_add`）。bind は 0 件。
  - `RulingReceived` の key は `rule` だけ（`fleet/event.rs` の `RULING_KEYS`）。行 f の `Case` は 5 kind の本体を持つ閉じた enum で、`RulingReceived` の本体は無い。§12 は「lc-a5 が RulingReceived へ広げる」と書く。
    - `Case` を match する file は `fleet/mod.rs`・`fleet/event.rs`・`pipe/dispatch/refused.rs` の 3 つ。
  - 器が台帳へ書く口は `ledger/mod.rs` の `close` だけ（cwd を repo に固定して bd を撃つ）。notes の追記と、1 本の bead の status・label・起票の時刻・metadata を読む口は無い。
    - 席の読み（`seat/ledger.rs` の `Issue`）は起票の時刻と metadata を持たない。この型は別の係の行が field を足す見込みなので触らない。
  - 裁定 id の形の判定は `ledger/close_reason.rs` の `is_ruling_id` に在る（問い id の形・batch・policy）。
  - JSON の文字列の字面を作る `quote` は `crates/scribe2/src/fleet/json_lite.rs` の `pub fn` で、`fleet/mod.rs` の `pub mod json_lite` と lib.rs の `pub mod fleet` を通して crate の中から呼べる。約束 5 は可視性を広げない（json_lite.rs は write-set の `=` の置き場で、中身を変えない）。
  - `seat ruling add` の専用の部品は `seat/ruling.rs` の `add`・`Draft`・`surface_check`（登録 row の役割が rules 行 R-C7-1 の値かを確かめる）・const `ID_DIALOGUE_SURFACE`・`RulingRefusal` の `EmptyWords` と `NotDialogueSurface` で、同じ file の歯の区間に `surface_check` の lib の歯 `fleet_ruling_surface_check_is_fail_closed_on_every_unreadable_or_foreign_case` が在る。`surface_check` の呼び手は `add` だけで、crates の src で R-C7-1 を読むのはこの const だけ（実測）。
  - `pipe/approve.rs` は R-C7-1 を読まない。`pipe approve` の撃ち手は、PreToolUse の guard（`hook/role_guard.rs` の表の `pipe approve` → 権能 approve）が rules 行 role.orchestrator の権能で確かめる。ADR-0083 は「R-C7-1 の役割の確かめは pipe approve にだけ残る」と書くが、現物の読み手は上の const だけ。
  - 問いの label と asked の値は `ledger/form.rs` の `QUESTION_LABEL` と `ledger/question.rs` の `ASKED`（seat・user）と `ASKED_KEY`。`ASKED_KEY` は私有なので、行 h が `pub(crate)` に広げる。
  - `seat ruling add` を名指すもの:
    - code: `help.rs` の表の 1 行、`seat/cli.rs` の使い方、`fleet/mod.rs` の注。
    - 歯: e2e の `seat/ruling.rs` の 4 本と、使い方の snapshot。
    - 設計: dialogue-surface §4 と fleet-event-log §9。
    - 消費側の code はこの口を撃たない（実測 0 件）。
- 約束（番号は done と 1:1）:
  1. **口の形**: `seat ruling bind --repo R --state-dir S --question ID --utterance TS [--bd B]`。`--bd` は歯の seam で、無い周は既定の bd。
  2. **断りの順と語**: 次の順で調べ、当たった周は何も書かずに rc 1 で `seat ruling: refused reason=<語> question=<id> utterance=<ts>` の 1 行を出す。語は閉じた 4 つで、const slice を持つ。
     - (a) `no-utterance`: その ts の `UtteranceReceived` が無い。
     - (b) `bound`: 同じ発話と同じ問いの組の `RulingReceived` が在る。1 つの発話を別の問いへ結ぶのは通す。
     - (c) `closed`: 問いの status が open でない。
     - (d) `not-question`: bead が無い、または label に `intake:question` が無い。
     - 台帳の読み（約束 3）は (b) の後・(c) の前に 1 回だけ撃つ。(a)(b) で断る周は台帳を読まない。読めない周は (c)(d) を調べずに `ledger-unreadable` で断る。
  3. **台帳の読み**: `ledger/mod.rs` に 1 本足す。bd の読みだけの show で bead 1 本を読み、status・labels・起票の時刻・metadata の asked・notes を返す。読めない周は rc 1 で `reason=ledger-unreadable` と出し、何も書かない。
  4. **裁定 id**: `<問い id>:<発話の ts の UTC の年月日と時分 YYYYMMDDTHHMMZ>-1`。発話の時刻から作るので、撃ち直しても同じ id になる。`is_ruling_id` に台帳の接頭辞を渡すと真になる。
  5. **裁定の行**: 次の 1 行を notes に追記する（`ledger/mod.rs` に追記の 1 本を足す）。
     - 形: `<裁定 id> | <問い id> | <発話の ts> | <経路 chat|gui> | <逐語>`。
     - 逐語は発話 event の detail を JSON の文字列の字面（`json_lite::quote`）で最後の欄に置く。改行を含んでも 1 行になり、戻すと 1 byte も違わない。
     - 同じ行がすでに notes に在れば書かない（撃ち直しで行を重ねない）。
  6. **close**: 既存の `close` で `裁定 <裁定 id>` を理由にする。
  7. **裁定 event**: `RulingReceived` を 1 件書く。
     - actor human・bead は問い id・detail は逐語。
     - `Case` に variant を 1 つ足し、key を 5 つ持つ: ruling・utterance・channel・question_ts・asked。asked は metadata に在る周だけで、値は閉じた 2 値。
     - 読み手は、この 5 key の行を新しい形として読み、`rule` と併せ持つ行を key を名指して malformed にする。`rule` だけの古い行は今までどおり読む。
  8. **書きの順と途中の失敗**: notes → close → event の順に書く。
     - close の後で event が落ちた周は rc 1 で `partial` を出す。notes の後で close が落ちた周も同じく rc 1 で `partial` を出し、notes の行は残る。
     - 同じ組の撃ち直しは、問いが closed でも notes に同じ裁定の行が在り event が無いなら、event だけを書いて rc 0 にする（半端な結びの仕上げ・新しい結びではない）。
     - 問いが open のまま notes に同じ裁定の行が在る撃ち直し（close の前で落ちた結び）は、notes を書かずに close と event を書いて rc 0 にする（約束 5 の重ねない）。
  9. **返す 1 行**: rc 0 で stdout に `ruling: id=<裁定 id> question=<id> utterance=<ts> channel=<経路>`。逐語は載せない。
  10. **add を消す**:
      - 口・使い方・help の行と、専用の部品（`add`・`Draft`・`surface_check`・`ID_DIALOGUE_SURFACE`・`RulingRefusal` の `EmptyWords` と `NotDialogueSurface`）を消す。`RulingRefusal` の型と `Store` と `render` は、bind の断りに使うなら残し、使わないなら消す（呼び手の無い部品を残さない）。
      - `surface_check` の lib の歯 `fleet_ruling_surface_check_is_fail_closed_on_every_unreadable_or_foreign_case` は部品と一緒に消す。
      - `ls` は残し、新しい形の行では `ruling=<id>` を出す。
      - doctor の manifest の裁定 id の突合（`rule` の読み）は古い行を読むだけで、そのまま残す。
  11. **使い方と help**: seat の使い方に `ruling bind …` を足し、`ruling add …` を外す。`help.rs` の seat の表も同じにする（`help_table_` の歯が live の使い方と照らす）。
- 歯（e2e は既存の `tests/e2e/seat/ruling.rs`。lib は `fleet/event.rs` の歯の区間）:
  - e2e `seat_ruling_bind_`（偽の bd が show の JSON を返し、append-notes と close の引数を記録する）:
    - (a) 先に在った問いと、発話の後に起こした問いの 2 形で 1 回ずつ通る。
      - notes の行が 1 行（5 欄・経路は発話 event の channel）。2 形の 1 つは channel gui の発話（event log に直書き）で、notes の経路の欄と stdout の `channel=` が gui、もう 1 つは channel chat の発話。逐語は改行と `"` と非 ASCII を含む字で、最後の欄を JSON の文字列として戻すと発話 event の detail と 1 byte も違わない。
      - close の理由が `裁定 <id>`、裁定 event が 1 件。先に在った問いの形は metadata に asked=seat を持ち、event は 5 key（asked=seat）。発話の後に起こした問いの形は metadata に asked を持たず、event は asked の key を持たない 4 key。id は `<問い id>:<発話の ts の YYYYMMDDTHHMMZ>-1`。
      - 偽の bd の記録で append-notes が close より先に撃たれ、event は close の後に在る（約束 8 の順）。
      - rc 0・stdout は `ruling: id=<id> question=<id> utterance=<ts> channel=<経路>` の 1 行だけで、逐語の字を含まない（逐語は cmd の引数に無い字で作る）。stderr は 0 byte。
    - (b) 断り 4 形（無い ts・結び済み・閉じた問い・問いでない bead）がどれも rc 1・stdout 0 byte・stderr が `seat ruling: refused reason=<語> question=<id> utterance=<ts>` の 1 行（語は形の順に no-utterance・bound・closed・not-question）で、event log も偽の bd の書きも変えない。
    - (b2) 断りの順: 隣り合う語の対を全部持つ 3 形で順を 1 列に決める。無い ts ∧ 結び済み〔同じ組の `RulingReceived` だけを event log に直書きし、その ts の `UtteranceReceived` を置かない〕・結び済み ∧ 閉じた問い〔(a) の後の同じ組〕・閉じた問い ∧ 問いでない bead〔label の無い閉じた bead〕で、stderr の語はそれぞれ先の語（no-utterance・bound・closed）だけ。加えて無い ts ∧ 問いでない bead で no-utterance。
    - (c) 1 つの発話を 2 つの問いへ結べる。
    - (d) 半端な結び（notes と close だけ済み）の撃ち直しが event だけを足す。偽の bd の append-notes と close の呼び出しは 0 回（同じ行を重ねない）。
    - (d2) close の前で落ちた結び: 偽の bd の close が 1 回目だけ rc 1 を返す周は rc 1 で `partial`・notes の行が 1 行・event 0 件・問いは open。同じ組の撃ち直しは rc 0 で、偽の bd の append-notes は 0 回・close は 1 回・event は 1 件、notes の裁定の行は 1 行のまま。
    - (e) 使い方の行と `scribe2 help seat` の頁（FORM の行と SUBCOMMANDS）の両方に `ruling bind` が在り、`ruling add` が無い。
    - (e2) 消えた口（約束 10）: 対話面の役割の登録 row を持つ置き場で、今の `add` の全引数の形（`seat ruling add --state-dir S --target T --words W --bead B --rule R`）を撃つと rc 2（使い方の誤り）・stdout 0 byte・stderr が使い方の 1 行（`ruling bind` を持ち `ruling add` を持たない）で、event log に `RulingReceived` が 0 件・置き場の file の一覧が撃つ前と同じ。base は add が通って rc 0 と event 1 件で RED。
    - (f) (a) の後の `seat ruling ls` の行が `ruling=<id>` を出す（消す `fleet_ruling_add_` の ls の歯の代わり）。
    - (g) 偽の bd の show が読めない JSON を返す周は rc 1 で `reason=ledger-unreadable` を出し、event log も偽の bd の書きも変えない。同じ偽の bd で、無い ts の周と結び済みの組の周は no-utterance と bound で断り、偽の bd の show の呼び出しは 0 回（台帳は (b) の後に読む）。
    - (h) 偽の bd が close の撃ちの中で event log の path を dir に替えた周は rc 1 で `partial` を出し、notes と close の書きは残る。同じ組の撃ち直しは (d) と同じく event だけを足す。
  - lib `fleet_ruling_body_`: 新しい形の行の往復・`rule` との併せ持ちの malformed・古い `rule` だけの行の読み。
  - lib `seat_ruling_id_`（`seat/ruling.rs` の既存の歯の区間）: 裁定 id を作る関数が、問い id と発話の ts（秒とミリ秒を持つ字）から `<問い id>:<YYYYMMDDTHHMMZ>-1` を返し、同じ入力で同じ字を返し、`is_ruling_id` に台帳の接頭辞を渡すと真。
  - 既存の歯の扱い（retroactive の札が要る）:
    - `fleet_ruling_add_` の 2 本は消す。
    - lib の `fleet_ruling_surface_check_is_fail_closed_on_every_unreadable_or_foreign_case`（`seat/ruling.rs` の歯の区間）は、約束 10 の部品と一緒に消す。
    - `fleet_ruling_report_` と `fleet_ruling_doctor_` の 2 本は、fixture を `add` の撃ちから event の直書きに替える。
    - 使い方の snapshot を書き直す。
    - `tests/e2e/seat.rs` の `seat_args_unknown_flag_is_refused_with_rc_2_on_every_verb` は 3 つの verb の 3 つ目に `ruling add` を撃つ（:366）。消す口なので、3 つ目を `ruling bind`（`--repo`・`--state-dir`・`--question`・`--utterance` を持つ形）に替え、未知の flag で rc 2・置き場に何も作らない・`--help` で rc 0 の 3 つを同じに保つ。
  - base で RED の理由: 機能不在（bind が使い方の誤りで rc 2、新しい key は未知の key で malformed）。
- 触らない:
  - 発話の記帳（行 g）・仕分け（行 i）・答えの口と hook の門（行 j）・席の notes の書きの門（lc-a6）・doctor の asked の数え（lc-e2）・計測の欄（lc-e22）。
  - `pipe approve` と承認 event。
  - `seat deliver`。
  - R-C7-1 の値。
  - `seat/ledger.rs` の `Issue`。
- 限界:
  - bd の show の JSON の形は bd の版に依る。形が違う周は `ledger-unreadable` で止まり、書かない。
  - notes の追記と close は 2 回の bd の撃ちで、原子的でない。約束 8 の仕上げで回復する。
  - 同じ問いに 2 度目の結びは `closed` で断る（1 問 1 裁定）。
  - rules 行 R-C7-1 は crates の src の読み手を失う（`surface_check` が唯一の読み手）。`cargo xtask check` の rules-wired の記録（main 2dcce09c で 5/79・検出線で rc は変えない）に 1 本加わる。値は触らない。読み手を戻す話は別 memo。
- 却下:
  - `Event` に新しい field を足す（literal の構築点 34 か所が全部動く）。行 f の `Case` に variant を足せば、構築点は動かない。
  - 裁定 id の時分を bind を撃った時刻にする（撃ち直しで id が変わる）。
  - 逐語を `--words` で受ける（ADR-0087: 逐語は発話 event から写す）。
  - `seat/ledger.rs` の `Issue` を広げる（構築点が多く、他の係の行と重なる）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "着地の列の待ち（Completion::LandTurn）は event log の長さと mtime が不変の周は replay を省く"
req = ["FR50", "NFR3"]
section = "4"
write-set = ["crates/scribe2/src/fleet/wait.rs", "docs/design/gate-cost.md", "docs/design/fleet-event-log.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_wait_land_turn_"]
size = "S"
done = "log が変わらない周は replay が呼ばれず、追記のあった周は読み直して列の判定が変わる"

[[contract]]
id = "b"
title = "run 無しの裁定を承認 event として持つ — EventKind に RulingReceived を足し、対話面の席の口 seat ruling add / ls が逐語付きで書き、pipe report が rulings を数え、doctor が manifest の裁定 id と突合する"
req = ["FR41", "FR22"]
section = "9"
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/pipe/report.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/seat/state.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/mod.rs", "+crates/scribe2/src/seat/ruling.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "+crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/ledger_memo.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_ruling_"]
size = "M"
done = "対話面の席の口が RulingReceived を逐語付き run 無しで 1 件書き、空の逐語と対話面でない席は typed に断られ、fleet record はこの kind を断り、pipe report が rulings を数えて approval 以外の人由来が増えず、doctor が manifest の user <ts> の裁定 id ごとに同じ分の event の有無を matched / unmatched で出し、既存の承認と質問の event は不変"

[[contract]]
id = "c"
title = "lock の回収を 1 手にする — 回収用の token を create_new で取れた 1 本だけが死んだ / 古い lock を外す（追記・受付の入口・受付札・driver の札の 4 面共通）"
req = ["FR3", "FR68", "NFR4"]
section = "4"
write-set = ["crates/scribe2/src/fleet/store.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_lock_reclaim_"]
size = "S"
done = "同じ死んだ lock を観測した 2 本のうち reclaim で外せるのは 1 本（逐次 2 回で true, false）、stale の回収も同じ 1 手を通り、生きている所有者の lock は Stale でも DeadOnly でも retry_ms まで待ち、追記の lock の既存の warning の歯は不変"
[[contract]]
id = "d"
title = "追記は 1 記録を 1 回の write で撃ち、改行で終わらない末尾を先に切り離す — 死んだ書き手が残した改行の無い完全な記録が次の記録と 1 行に並び、置き場の replay の全部を止める穴を塞ぐ（append_line を通る全部の file・lock と schema は不変・memo s2-07l.711）"
req = ["FR3", "FR68", "NFR4"]
section = "10"
write-set = ["crates/scribe2/src/fleet/store.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail fleet_torn_line_"]
size = "S"
growth = ["crates/scribe2/src/fleet/store.rs:50"]
done = "(1) write_line が本文と改行を 1 つの buffer に詰めて write_all を 1 回だけ撃つ (2) lock の中で file の末尾の 1 byte を読み、空でなく改行でもない周だけ記録の前に改行を 1 つ足し（同じ 1 回の write）、途中で切れた記録は単独の malformed の行として残って read_all は Err のまま (3) append_line を通る全部の file に効き、lock の実装は 1 本のまま (4) schema・1 行 1 event・lock と回収・rules 行・Warning の値・read_all の判定は変わらない 歯: fleet_torn_line_ の (a) 改行の無い完全な記録で終わる log に append で 1 件足すと read_all が 2 件を Ok で返し、続けて 1 件足すと 3 件で file は空の行を持たない (b) 途中で切れた記録で終わる log に 1 件足すと行数が 2 で read_all の Err は line=1 の 1 件だけ・base は (a) が line=1 の Malformed の Err・(b) が行数 1 で RED"
[[contract]]
id = "e"
title = "本文の読めない古い lock を DeadOnly の周でも外す — 空の掃除の lock が永久に残り掃除が止まった穴を塞ぐ（本文の読める lock の扱い・本文の書き方・rules 行は不変・memo s2-07l.736.8）"
req = ["FR68", "FR30"]
section = "11"
write-set = ["crates/scribe2/src/fleet/store.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail fleet_lock_unparsed_"]
size = "S"
growth = ["crates/scribe2/src/fleet/store.rs:60"]
done = "(1) acquire_in が、観測した本文が 2 形のどちらでもない（空を含む）lock を、Reclaim の値に依らず mtime が stale_ms より古い周に回収の 1 手で外し、warning StaleLockRemoved を積む (2) 本文が読める lock は今までどおり（生きている所有者は DeadOnly の周に古くても奪わない・probe が読めない周は DeadOnly で外さない・死んだ所有者は外す）で、既存の fleet_lock_reclaim_ の歯は期待を変えずに緑 (3) 本文の書き方・lock の path・reclaim の 1 手・token・LockPolicy・rules 行 fleet.lock_stale_ms は変えない 歯: fleet_lock_unparsed_stale_body_is_reclaimed_under_dead_only（空と abc の本文の古い lock が DeadOnly で取れ warning が 1 件・base は error で RED）・fleet_lock_unparsed_fresh_body_waits_under_dead_only（古くない周は取れず lock は残る）"
[[contract]]
id = "f"
title = "案件の一生の event の kind 5 つ（UtteranceReceived・UtteranceSorted・TurnEndUnjudged・IntakeRefused・LifecycleCutover）と key を読み書きの両側で先に足す — 書き手の行の前に閉じた型の閉包を 1 回で広げ、fleet record は Run の形でない kind を全部断り、pipe report は発話を人由来に数えない（ADR-0087・ADR-0088）"
req = ["FR82", "FR88", "FR90", "FR68", "FR22"]
section = "12"
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/pipe/report.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/regate.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/fleet/json.rs", "crates/scribe2-boundary/tests/e2e/fleet/account.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/ledger_memo.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "=crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_case_kind_"]
size = "M"
growth = ["crates/scribe2/src/fleet/mod.rs:160", "crates/scribe2/src/fleet/event.rs:130", "crates/scribe2/src/fleet/replay.rs:12", "crates/scribe2/src/fleet/cli.rs:10", "crates/scribe2/src/pipe/report.rs:10", "crates/scribe2/src/account/mod.rs:1", "crates/scribe2/src/fleet/usage.rs:1", "crates/scribe2/src/hook/group.rs:1", "crates/scribe2/src/hook/vessel.rs:1", "crates/scribe2/src/pipe/dispatch.rs:3", "crates/scribe2/src/pipe/mod.rs:4", "crates/scribe2/src/pipe/queue.rs:1", "crates/scribe2/src/pipe/regate.rs:1", "crates/scribe2/src/seat/role.rs:2", "crates/scribe2/src/seat/ruling.rs:3"]
done = "(1) EventKind の末尾に UtteranceReceived・UtteranceSorted・TurnEndUnjudged・IntakeRefused・LifecycleCutover をこの順で足し、KINDS も同じ順の 29 種で、既定の actor は UtteranceReceived だけ human、5 つは Shape の新しい値 1 つを共有し、replay は 5 kind の行で便も席も登録 row も口座の退役も作らない (2) Event に 5 kind の本体を持つ閉じた enum の field を 1 つ足し、発話の経路 chat / gui と仕分け request / chat は全 variant の const slice を持つ閉じた enum で、memo と契約の id は既存の bead の field に持つ (3) 読み手は §12 の表のとおりに 7 つの新しい key を 5 kind の行でだけ受け、必須の欠け・閉じた値の外・空の値・経路と session の食い違い・仕分けと bead の食い違い・他の kind の key を key を名指す malformed にし、書き手は表の並びで書いて、読んだ行を書き直すと同じ字面になる (4) ts は文字列のまま読み（秒より下の桁も読める）、schema 版は 1 のまま (5) fleet record は Shape が Run でない kind を全部 rc 1 で断り（字面は今の record では書けない・log を作らない）、断りの手書きの列は消える (6) pipe report の human_events と human_events_other_than_approval は UtteranceReceived を数えず、行の字面と rulings の数えは変わらない (7) KINDS の件数と末尾を pin する既存の歯 5 か所を 29 種と新しい末尾に書き換え、Event の literal 構築点 34 か所に新しい field を足し、fleet_external_form の snapshot は変わらない 歯: fleet_case_kind_ の歯が、5 kind の見本 7 行が読んで書き直すと同じ字面になり log の往復でも 7 件が同じ値で戻ること、欠け・食い違い・他の kind の key の行が key を名指す malformed になること、Run の形でない kind を fleet record が全部断ること、5 kind の行で replay の便が 1・席が 0 のままのこと、発話が人由来に数えられないこと、KINDS が 29 種で末尾 5 つの字面と actor が表のとおりであることを測る。base は、5 kind が未知の kind で読めず、DispatchMark と群の移動が record を通り、KINDS が 24 種なので RED"

[[contract]]
id = "g"
title = "user の prompt を model が読む前に発話 event として記帳する — UserPromptSubmit の hook が差し込みの行・別の session の包み・runner・marker の外を除き、逐語を一意のミリ秒の ts で書いて ts の 1 行を返す（台帳を読まず・読む byte は log の大きさに依らない・ADR-0083・ADR-0087）"
req = ["FR82", "FR88", "NFR5"]
section = "13"
write-set = ["+crates/scribe2/src/hook/utterance.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/fleet/store.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/notify.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/hook/session.rs", "crates/scribe2-boundary/tests/e2e/hook/group.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_utterance_record_", "cargo nextest run -p scribe2 --lib --no-tests=fail utterance_tail_"]
size = "M"
growth = ["crates/scribe2/src/hook/utterance.rs:260", "crates/scribe2/src/hook/mod.rs:6", "crates/scribe2/src/fleet/store.rs:75", "crates/scribe2/src/fleet/cli.rs:14", "crates/scribe2/src/fleet/wait.rs:30", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/pipe/notify.rs:12", "crates/scribe2/src/pipe/dispatch/group.rs:12", "crates/scribe2/src/headless/mod.rs:10", "crates/scribe2/src/pipe/mod.rs:1", "crates/scribe2/src/pipe/dispatch.rs:1", "crates/scribe2/src/seat/tick.rs:1"]
done = "(1) hook の子 module 1 つが user-prompt-submit の枝で打刻の後・群の行の前に 1 回撃たれる (2) payload を json_tree で読み、prompt の無い・空白だけの周は何も書かず何も出さない (3) 頭が <NAME> と閉じた 4 語の差し込みの行・閉じた 5 つの頭の包み（別の session と teammate の包みの 3 形・背景の task の知らせ・compaction の要約）・plugin-root が state dir の pipe の dir の下の runner は記帳も出力もしない（lens は hook が撃たれない既存の事実で、本行は触らない） (4) ts はミリ秒 3 桁の UTC の字面で、作りと読みは fleet/cli.rs と fleet/wait.rs の各 1 本 (5) store の追記の 1 本が lock の中で末尾 64 KiB だけを読み、今と最後の発話の ts + 1 ms の大きい方を振る（窓の外の発話は見ない・窓の頭で途切れた行は捨てる） (6) 行は UtteranceReceived・human・chat・session・逐語の detail・run 無し (7) 記帳した周だけ stdout に <NAME> utterance: ts=<ts> の 1 行（逐語なし） (8) session 無し・lock・書けない周は rc 0 で stderr 1 行 reason=no-session|lock|write (9) bd を撃たない (10) 作り手の親の mod 宣言 3 つ（pipe の notify・dispatch の group・tick の signal）を pub(crate) に広げ、作り手の関数を crate の中から呼べるようにし、行を増やさない (11) ミリ秒の読みを fleet/mod.rs の pub use wait の列に 1 語足す 歯: hook_utterance_record_ が問いの有無の 2 本の記帳と逐語の一致と bd 0 回・差し込み 9 種と包みと runner の 0 件・marker の外 2 形の 0 件・同じ秒の 2 発話の ts の違いと順・pipe report の human_events の不変・prompt の無い周と空白だけの周の記帳 0 と出力 0 byte・session 無し・lock・書けない周の rc 0 と stderr の reason の 1 行と記帳 0・除きすぎない 4 形の記帳と先頭の空白の包みの除外・ts の字面と actor と run の不在と stdout の逐語の不在・群の行より前の ts の行（hook/group.rs の逼迫の fixture）を、utterance_tail_ が 1 時間先の最後の発話への追記でちょうど +1 ms・80 KiB の他の event の後ろの先の発話では今・60 KiB の後ろでは +1 ms・作り手 9 種の実物の行の判定・ミリ秒の往復を測る。base は書き手が無く記帳 0 件で RED"

[[contract]]
id = "h"
title = "器の結びの口 seat ruling bind — 記帳された発話と開いた台帳の問いを結び、裁定 id を発行して 5 欄の裁定の行を notes に書き、裁定 <id> で close し、RulingReceived に ruling・utterance・channel・question_ts・asked を足す。逐語を受ける seat ruling add は消す（ADR-0083・ADR-0087・ADR-0089）"
req = ["FR82", "FR89", "FR91"]
section = "14"
write-set = ["crates/scribe2/src/seat/ruling.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/ledger/question.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "=crates/scribe2/src/fleet/json_lite.rs", "crates/scribe2/src/pipe/dispatch/refused.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_bind_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_ruling_body_", "cargo nextest run -p scribe2 --lib --no-tests=fail seat_ruling_id_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_ruling_report_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_ruling_doctor_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_args_unknown_flag_is_refused_with_rc_2_on_every_verb"]
size = "L"
growth = ["crates/scribe2/src/ledger/question.rs:1", "crates/scribe2/src/seat/ruling.rs:170", "crates/scribe2/src/seat/cli.rs:25", "crates/scribe2/src/ledger/mod.rs:110", "crates/scribe2/src/fleet/mod.rs:45", "crates/scribe2/src/fleet/event.rs:70", "crates/scribe2/src/pipe/dispatch/refused.rs:2", "crates/scribe2/src/help.rs:2"]
done = "(1) seat ruling bind --repo --state-dir --question --utterance [--bd] が在る (2) 無い ts・結び済みの組・閉じた問い・問いでない bead を、この順に閉じた 4 語で何も書かずに rc 1 で断る (3) ledger に bead 1 本の読みを足し、読めない周は ledger-unreadable で何も書かない (4) 裁定 id は <問い id>:<発話の時分>-1 で is_ruling_id が真 (5) notes に 5 欄の 1 行（逐語は JSON の文字列の字面で最後の欄・同じ行は重ねない） (6) 裁定 <id> で close (7) RulingReceived は Case の新しい variant に 5 key を持ち、rule との併せ持ちは malformed、rule だけの古い行は読める (8) notes→close→event の順で、close か event が落ちた周は rc 1 で partial、close の後の半端な結びの撃ち直しは event だけを足し、close の前の半端な結びの撃ち直しは notes を書かずに close と event を書く (9) stdout 1 行に逐語を載せない (10) seat ruling add と専用の部品（add・Draft・surface_check・ID_DIALOGUE_SURFACE・EmptyWords・NotDialogueSurface）と surface_check の lib の歯を消し、ls は新しい形で ruling= を出す (11) 使い方と help の表が bind を持ち add を持たない (12) ledger/question.rs の ASKED_KEY を pub(crate) に広げる 歯: seat_ruling_bind_ が 2 形の通過と行（逐語の往復）・notes→close→event の順・close・event・id の形・stdout の 1 行と逐語の不在、断り 4 形の rc 1 と stderr の語と不変、隣り合う語の対の 3 形と無い ts ∧ 問いでない bead の先の語、1 発話 2 問、半端な結びの仕上げと書きの重ねの無さ（close の後と close の前の 2 形）、使い方と help の頁の bind の在と add の不在、消えた add の口を全引数で撃って rc 2 と event 0 件と置き場の不変、ls の ruling= の列、ledger-unreadable の不変、partial の rc 1 と撃ち直しを、fleet_ruling_body_ が新しい形の往復と併せ持ちの断りと古い行の読みを、seat_ruling_id_ が裁定 id の字と決定性と is_ruling_id を測る。既存の fleet_ruling_add_ の 2 本と surface_check の lib の歯 1 本は消し、2 本は fixture を event の直書きに替え、seat.rs の未知の flag の歯の 3 つ目の verb を ruling bind に替える。base は bind が使い方の誤りで RED"
<!-- contracts:end -->
