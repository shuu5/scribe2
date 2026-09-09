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
| `run` | string | 必須 | run id（`<bead>-<UTC stamp>`） |
| `bead` | string | 必須 | 契約の bead id（台帳は読まない・文字列として持つだけ） |
| `host` | string | 必須 | host 名（C3 の host 列）。`/etc/hostname` → `hostname` コマンド → `"unknown"` の順。env は読まない |
| `actor` | string | 必須 | `"machine"` / `"human"`。人由来 event を数える面（FR22）。`ApprovalReceived` だけが `human` |
| `stage` | string | 任意 | `Stage` の variant 名 |
| `seat` | string | 任意 | 席 id（MVP は run id と同じ） |
| `pid` | u64 | 任意 | runner の pid |
| `detail` | string | 任意 | 自由文（verdict 名・runner rc・承認の逐語 等） |

- `pub enum EventKind`（閉じた 8 variant）: `RunCreated` / `RunStage` / `RunDone` / `RunStopped` / `SeatSpawned` / `SeatStopped` / `ApprovalRequested` / `ApprovalReceived`。
- `pub enum Stage`（閉じた 8 variant）: `Intake` / `Blocked` / `Spawned` / `Implemented` / `Gated` / `Landed` / `Stopped` / `Failed`。遷移は pipeline 側（[pipeline.md §4](./pipeline.md)）。
- `pub enum SeatState { Live, Stopped }`（C3.3: 席の状態は typed enum・bool で持たない）。
- 字面変換は **wildcard 無しの `match`** 1 箇所ずつ。
- JSON は flat object（値は string / u64 / bool / null）だけを扱う std の writer / reader（`json_lite`）。`"` `\` 制御文字は escape する。それ以外の形は error。

## 4. replay・store・待機

- `pub fn replay(events: &[Event]) -> State`。`State { runs: BTreeMap<String, Run>, seats: BTreeMap<String, Seat> }`、`Run { id, bead, stage: Stage, updated, detail, approved: bool }`、`Seat { id, run, pid, state: SeatState, updated }`。run ごとに物理順で最後の `stage` が現在地。`SeatStopped` で `SeatState::Stopped`。`ApprovalReceived` で `approved = true`（`approved` は「承認 event が在るか」の導出値であって状態 enum ではない）。
- `append(dir, &Event) -> Result<Warnings, StoreError>`: lock file を `create_new` で取り（再試行の上限は rules 行 `fleet.lock_retry_ms`）、`O_APPEND` で 1 行書いて flush し、lock を外す。**rules 行 `fleet.lock_stale_ms` より古い lock は stale として除去し** `Warning::StaleLockRemoved` を返り値に載せる（黙って消さない）。
- `read_all(dir) -> Result<Vec<Event>, Vec<StoreError>>`: **malformed 行（parse 不能・`schema` が 1 以外）は skip せず `line=<N>` 付きの error に全件集めて `Err`**（NFR4）。file 不在は `Ok(vec![])`。
- **待機は 1 実装**（C3.4）: `pub enum Completion { RunnerExited(pid), SeatGone(pid) }` と `pub fn wait(c: Completion, deadline: Duration) -> Result<(), Timeout>` の 1 本。任意の述語を受ける口は作らない。pipeline の「runner の終了待ち」「TERM 後の消滅待ち」はこの 2 値で表す。
- 失敗は境界ごとの enum（`StoreError` / `Timeout`）で持ち、極性は `FailClosed`（C11.2）。

## 5. CLI（`<NAME> fleet …`・`--state-dir D` 必須・出力は `emit` / `emit_err` 経由のみ）

- `fleet record --kind <k> --run <id> --bead <b> [--stage <s>] [--seat <id>] [--pid <n>] [--actor machine|human] [--detail <text>]` → rc 0・stdout 1 行 `fleet: recorded <kind> run=<id>`。
- `fleet show --run <id>` → 1 行 `run=<id> bead=<b> stage=<s> approved=<bool> updated=<ts>`。無ければ `fleet: no such run` + rc 1。store が読めなければ rc 2。
- `fleet export`（**跨版 面 2**）: stdout 1 行目 = `{"schema":1,"kind":"export","host":"<host>","runs":<N>,"seats":<N>}`、以降 run 1 件 1 行・seat 1 件 1 行。**read-only**（store の file を 1 byte も変えない・lock も取らない）。malformed なら error 行 + rc 2。
  - run 行 = `{"kind":"run","id":"<run id>","bead":"<bead>","stage":"<Stage>","approved":<bool>,"updated":"<ts>"}`（key はこの並び）。
  - seat 行 = `{"kind":"seat","id":"<seat id>","run":"<run id>","state":"<SeatState>","updated":"<ts>"}`（key はこの並び）。

## 6. 歯（契約 `s2-07d` の検証・`tests/e2e/fleet.rs` module・`fleet_` 接頭辞・tmp dir を `--state-dir` で指す）

`fleet_replay_rebuilds_state_from_events` / `fleet_replay_marks_run_approved_on_approval_received` / `fleet_replay_seat_state_is_stopped_after_seat_stopped` / `fleet_read_rejects_malformed_line_with_line_number`（2 行目を壊す → `Err` 1 件・`line=2`・3 行目は返らない）/ `fleet_read_rejects_unknown_schema` / `fleet_append_serializes_concurrent_writers`（8 thread × 50 append → 400 行・全行 parse 可・interleave 0）/ `fleet_state_survives_process_restart`（process A が `record`・process B が `show` → 同じ stage）/ `fleet_export_first_line_is_schema_header` / `fleet_export_is_read_only`（rc 0 ∧ stdout が 1 + runs + seats 行、**その上で**前後の file 名・size・mtime 同一・lock 残らず）/ `fleet_state_dir_flag_is_required`（無ければ rc 1・stdout 0 byte）/ `fleet_json_roundtrip_escapes` / `fleet_stale_lock_is_removed_after_threshold`（`File::set_modified` で lock の mtime を戻す・std のみ）/ `fleet_wait_times_out_with_typed_error` / `fleet_export_rc2_on_malformed_store` / `fleet_external_form`（snapshot）。

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
