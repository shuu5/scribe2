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
| `run` | string | kind ごと | run id（`<bead>-<UTC stamp>`）。便の kind では必須・run を持たない kind（口座残量 2・`SeatRegistered`・`AccountRetired` / `AccountRestored`・`RulingReceived`〔§9〕）では在れば malformed |
| `bead` | string | kind ごと | 契約の bead id（台帳は読まない・文字列として持つだけ）。便の kind では必須・run を持たない kind では禁止（`RulingReceived` だけ任意） |
| `host` | string | 必須 | host 名（C3 の host 列）。`/etc/hostname` → `hostname` コマンド → `"unknown"` の順。env は読まない |
| `actor` | string | 必須 | `"machine"` / `"human"`。人由来 event を数える面（FR22）。`human` は `ApprovalReceived` と `RulingReceived`（§9）の 2 kind |
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

- `fleet record --kind <k> --run <id> --bead <b> [--stage <s>] [--seat <id>] [--pid <n>] [--actor machine|human] [--detail <text>]` → rc 0・stdout 1 行 `fleet: recorded <kind> run=<id>`。
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
- 形: (1) `EventKind` の末尾に variant 1 つ `RulingReceived`（actor = `human`・`run` を持たない kind〔行に在れば malformed〕・`bead` は任意・新しい key `rule`〔rules 行の id・任意・文字列〕・`detail` = user の逐語で**空なら 1 byte も書かない**〔`pipe approve` と同じ `record_words` の型〕・schema 版 1 のまま）。読み手（`event.rs` の kind ごとの `forbid` の arm・`replay` はこの kind で run を作らない）。(2) 口 = `<NAME> seat ruling add --state-dir S --target T --words "<逐語>" [--bead B] [--rule ID]`（1 件書く・ts は器が打つ＝**裁定 id**）/ `seat ruling ls --state-dir S`（ts・bead・rule・逐語を 1 件 1 行）。`T` の登録 row（[seat-roles.md](./seat-roles.md) §2 の解決）の役割が rules 行 `R-C7-1` の値でない周は `not-dialogue-surface` で断る（row が無い・読めない周も断る・FailClosed）。書き手はこの口だけ（`fleet record` はこの kind を従来どおり断る＝`run` を要る口）。(3) `pipe report` の行に `rulings=<n>` を足し、`human_events_other_than_approval` の「承認」の kind を `ApprovalReceived` と `RulingReceived` の 2 つにする（FR22）。(4) doctor（`doctor --state-dir S`）の 1 行 `rulings=<n> rule-rulings=<matched>/<of> unmatched=<id,…>`: manifest（tracked + `--rules`）の `[[rule]]` のうち `ruling` 欄が `user <UTC の分までの ts>` の形の行を母集団とし、同じ分の `ts` を持つ `RulingReceived` が在る行を matched・無い行を id で名指す（読むだけ・判定しない・C10.2・分が曖昧な形〔`4xZ`〕は母集団に入らず `skipped=<n>` で数える・同じ分に event が複数在る行は matched に数え件数を `matched=<n>` の内訳に持たない＝1 行が 1 件を一意に指すことは保証しない）。
- 触らない: `ApprovalRequested` / `ApprovalReceived` / `QuestionRaised` / `QuestionAnswered` の形と読み手・`pipe approve` / `pipe answer`・resume の経路・直命の表（[working-memory.md](./working-memory.md) §12.1・別 kind）・schema 版。
- 歯（`fleet_ruling_` 接頭辞・`tests/e2e/fleet.rs` と `tests/e2e/seat/ruling.rs`〔新規〕・`prop.rs` の生成器は variant を足すだけ）: 対話面の役割の登録 row を持つ target で `seat ruling add` が `RulingReceived` を 1 件（`actor=human`・`run` 無し・逐語が detail に逐語で）書く／逐語が空なら書かず rc 1／登録 row の役割が R-C7-1 の値でない target は `not-dialogue-surface` で書かない／`fleet record --kind RulingReceived` は断る／`pipe report` の `rulings=` が数え `human_events_other_than_approval` が増えない／doctor が manifest の `user <ts>` 行に対して同じ分の event の有無で matched / unmatched を出す／外形 snapshot（seat usage・doctor・pipe）が更新される。
- 却下: ADR-0037 §3（写しは持たない）。

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
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/report.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/seat/state.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/mod.rs", "+crates/scribe2/src/seat/ruling.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/src/main.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/seat.rs", "+crates/scribe2/tests/e2e/seat/ruling.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_ruling_"]
size = "M"
done = "対話面の席の口が RulingReceived を逐語付き run 無しで 1 件書き、空の逐語と対話面でない席は typed に断られ、fleet record はこの kind を断り、pipe report が rulings を数えて approval 以外の人由来が増えず、doctor が manifest の user <ts> の裁定 id ごとに同じ分の event の有無を matched / unmatched で出し、既存の承認と質問の event は不変"

[[contract]]
id = "c"
title = "lock の回収を 1 手にする — 回収用の token を create_new で取れた 1 本だけが死んだ / 古い lock を外す（追記・受付の入口・受付札・driver の札の 4 面共通）"
req = ["FR3", "FR68", "NFR4"]
section = "4"
write-set = ["crates/scribe2/src/fleet/store.rs", "crates/scribe2/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_lock_reclaim_"]
size = "S"
done = "同じ死んだ lock を観測した 2 本のうち reclaim で外せるのは 1 本（逐次 2 回で true, false）、stale の回収も同じ 1 手を通り、生きている所有者の lock は Stale でも DeadOnly でも retry_ms まで待ち、追記の lock の既存の warning の歯は不変"
<!-- contracts:end -->
