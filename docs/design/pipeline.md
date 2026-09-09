# 設計: pipeline（縦 1 本）— intake → spawn → gate → land を永続面から読んで 1 本通す

- 要件: [FR1](../../design-intent/spec/srs.html#FR1) 契約の検査 / [FR2](../../design-intent/spec/srs.html#FR2) 契約の pointer / [FR4](../../design-intent/spec/srs.html#FR4) runner の起動 / [FR5](../../design-intent/spec/srs.html#FR5) runner と lens の口 / [FR6](../../design-intent/spec/srs.html#FR6) 完了判定 / [FR7](../../design-intent/spec/srs.html#FR7) 入口の flip check / [FR8](../../design-intent/spec/srs.html#FR8) gate の機械検証 / [FR9](../../design-intent/spec/srs.html#FR9) lens の verdict / [FR10](../../design-intent/spec/srs.html#FR10) land の前提 / [FR11](../../design-intent/spec/srs.html#FR11) land / [FR12](../../design-intent/spec/srs.html#FR12) verdict export / [FR13](../../design-intent/spec/srs.html#FR13) stop / [FR14](../../design-intent/spec/srs.html#FR14) resume / [FR15](../../design-intent/spec/srs.html#FR15) 承認の停止 / [FR16](../../design-intent/spec/srs.html#FR16) 承認の再開 / [FR22](../../design-intent/spec/srs.html#FR22) 人手 0 の計測
- 受入: [AC1](../../design-intent/spec/srs.html#AC1) toy repo 5 便 / [AC2](../../design-intent/spec/srs.html#AC2) 自己ホスト 1 便 / [AC3](../../design-intent/spec/srs.html#AC3) 偽の PASS 0 / [AC4](../../design-intent/spec/srs.html#AC4) 再開で完走 / [AC5](../../design-intent/spec/srs.html#AC5) 無承認の通過 0
- 非機能・制約: [NFR1](../../design-intent/spec/srs.html#NFR1) lens 予算 / [NFR2](../../design-intent/spec/srs.html#NFR2) 契約の大きさ / CON2 PUBLIC / CON3 歯は Rust / CON5 不可逆の口を持たない / CON6 headless
- 憲法: [C3](../../design-intent/spec/constitution.html#c3) Completion 1 enum / [C6](../../design-intent/spec/constitution.html#c6) 消費は害（Budget・起動口 1 つ）/ [C7](../../design-intent/spec/constitution.html#c7) 対話面 / [A1](../../design-intent/spec/constitution.html#a1) 3 クラス / [A4](../../design-intent/spec/constitution.html#a4) merge は可逆 / [N1](../../design-intent/spec/constitution.html#n1) 削除は可逆 move だけ
- 決定: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.2（面 4 stop・面 5 verdicts）/ §2.3（契約 file の形）/ §2.4（env seam 無し）
- 前提の設計: [fleet-event-log.md](./fleet-event-log.md)（永続面・Completion）/ [vessel-hook.md](./vessel-hook.md)（guard・state dir）/ [rules-manifest.md](./rules-manifest.md)（crate の形・lens 本数・cap・猶予の値）
- この設計から出る契約（5 本・見積は NFR2 の 550 行以内）: (a) `s2-2e5` intake → spawn → stop（550 行）/ (b) `s2-41o` gate → land → export → e2e（550 行）/ (c) `s2-07l.22` 承認 Blocked と resume（300 行）/ (d) `s2-07l.23` headless runner と lens（400 行）/ (e) `s2-07l.24` 到達点の計測（400 行）。順序: (a) → (b)・(c)・(d) → (e)。

## 1. 何を解くか

契約 1 本を、人の手を借りずに intake → spawn → gate → land まで通す（GOAL 1）。壊れたまま進まず（GOAL 2）、記憶に頼らない（GOAL 3）。

やさしく言うと: 契約 file を読み込み、作業場所を切って実装役の Claude を走らせ、機械検証と審査役の Claude で審査し、合格なら main に載せる。どの段も「今どこか」は event log から読むので、途中で止めても別 process が続きを引ける。

## 2. 全体の形

```
契約 file ──intake──▶ run（Intake）
                        │ 3 クラスを名乗る契約は spawn の手前で Blocked（approval.requested）
                        │ 人の approve（逐語）→ approval.received → resume
                     spawn ──▶ Budget（Precheck から）→ worktree + runner ──▶ Implemented / Failed
                     gate  ──▶ verify 各行の rc + lens 1 本 ──▶ Gated（PASS / FAIL / INCONCLUSIVE）
                     land  ──▶ squash 1 commit（tree 同一・CAS）→ main で verify 再実走 ──▶ Landed / Failed
                              └ verdicts.jsonl に 1 行（面 5）
```

- 各 subcommand は **fleet の replay から現在 stage を読んで前提を検査し、event を 1 件以上追記して終わる**。process 間で情報を持ち越す面は event log と `<state_dir>/pipe/<run>/` だけ（FR3・AC4）。state dir は `vessel init` が repo に紐づけたもの（`--state-dir` で上書き・[vessel-hook.md §2](./vessel-hook.md)）。
- runner と lens は **seam**（`--runner <cmd>` / `--lens <cmd>`）。CI の歯は fake（`sh -c` の 1 行）で通し、実 Claude は (d) の wrapper `<NAME> runner` / `<NAME> lens` を同じ seam に渡す（FR5）。
- **不可逆の口（CON5・N1）**: force 系 git・削除の subcommand は無い（後始末は可逆 move・§5.4）。**公開**の経路は `--pr-cmd`（§5.4）だけで、run に承認 event が無ければ動かない（A1）。**課金**: runner / lens は定額の subscription 内で走り、追加課金の口を持たない（「使う」= 追加課金だけ・要件カタログ R-F4）。
- **起動口は 1 つ**（C6）: runner を起動できる関数は `fn spawn(budget: Budget, …)` の 1 本で、`Budget` は `Precheck` の実測を消費してしか作れない。CLI の `spawn` / `resume` / `run` はこの 1 関数への経路であって別の口ではない。

## 3. 契約 file（TOML subset・[ADR-0004 §2.3](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html#s2-3-contract-and-manifest)）

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `goal` | string | 必須 | 1 行の目的 |
| `done` | string | 必須 | 何ができたら終わりか |
| `size` | string | 必須 | `S` / `M` / `L`（NFR2 の見積の目安） |
| `owner` | string | 必須 | bead id（文字列として持つだけ・台帳は読まない） |
| `disposition` | string | 必須 | `A-now` / `F` |
| `write-set` | string 配列 | 必須・1 本以上 | 触ってよい file / dir（末尾 `/`） |
| `verify` | string 配列 | 必須・1 本以上 | 検証コマンド。**各要素に改行なし**・1 行で完結 |
| `req` | string 配列 | 必須・1 本以上 | SRS の要件 id（FR2） |
| `design` | string | 必須 | 設計 doc の repo 相対 path（FR2） |
| `classes` | string 配列 | 任意（既定 空） | 3 クラスの自己申告: `delete` / `publish` / `consume`（FR15） |

- `Contract::load(path) -> Result<Contract, Vec<ContractError>>`: 必須 key の欠落・`verify` 0 本 / 改行入り・`write-set` 0 本・`req` 0 本・未知 key・未知 `classes` 値を**全件集めて `Err`**（FR1・行番号付き）。
- 検査の順序と極性は 1 関数・1 enum（`ContractError`）に閉じる（C2）。

## 4. stage と event

| stage | 入る event | 出る条件 |
|---|---|---|
| `Intake` | `RunCreated` | spawn（3 クラス無し）/ Blocked（3 クラス有り・未承認） |
| `Blocked` | `ApprovalRequested` + `RunStage` | `ApprovalReceived`（`actor=human` ∧ 逐語が非空）が在れば resume → spawn（FR16） |
| `Spawned` | `RunStage` + `SeatSpawned` | runner 終了 → `SeatStopped` + Implemented / Failed（FR6） |
| `Implemented` | `RunStage` | gate |
| `Gated` | `RunStage detail=verdict:<V>` | verdict PASS → land / それ以外は止まる（FR10） |
| `Landed` | `RunDone` | 終端 |
| `Stopped` | `RunStopped` | 終端（stop --all） |
| `Failed` | `RunStage detail=<理由>` | 終端（resume は rc 1） |

前提違反は **rc 1 + stderr 1 行・何もしない**（event も追記しない）。

## 5. subcommand（`<NAME> pipe …`・全部に `[--state-dir D]` `[--rules PATH]`）

### 5.1 intake（(a)）
`pipe intake --contract <file> --bead <id> --repo <dir>` → §3 の検査 → `<state_dir>/pipe/<run>/contract.toml` へ写す → `RunCreated stage=Intake` → stdout `run=<id>`。run id = `<bead>-<UTC stamp>`。

### 5.2 spawn（(a)・FR4 / FR6・C6）
`pipe spawn --run <id> --runner <cmd>`: 前提 stage = Intake（3 クラス有りなら §5.5）。
1. **Precheck → Budget**（C6）: `Precheck::measure(contract, repo)` が write-set の本数・`verify` の本数・`size` を実測して `Budget` を作る。`Budget` はこの経路以外で作れない（private constructor）。`fn spawn(budget: Budget, run, runner) -> Outcome` が **runner を起動できる唯一の関数**。MVP の Budget は上限を効かせない（R-C6-1 が未定）が、型の形を先に置く。
2. `base = git -C <repo> rev-parse HEAD` を event に記録。
3. `git worktree add -b <NAME>/<run> <repo>/.worktrees/<NAME>/<run> <base>`（既存なら rc 1）。
4. write-set を `<worktree の git dir>/<NAME>/write-set.txt` に 1 行 1 path で書く（guard が読む形・[vessel-hook.md §5](./vessel-hook.md)・tracked 面に触れない）。
5. `RunStage stage=Spawned` → runner を `sh -c <cmd>` で **cwd = worktree** で起動し `SeatSpawned seat=<run> pid=<pid>`。cmd 中の placeholder `{run}` `{worktree}` `{contract}` `{write_set}` `{base}` を置換する。**scribe2 固有の env は 1 つも足さない**（親の env はそのまま継承・ADR-0004 §2.4）。
6. **runner の終了待ちは `Child::wait`**（rc を運ぶ）。pid の生存待ち（`stop`）は `wait(Completion::SeatGone)`（[fleet-event-log.md §4](./fleet-event-log.md)）で、`Completion::RunnerExited` は**別 process が spawn した runner を待つ resume 経路のために残す**。`SeatStopped`。**rc 0 ∧ `git rev-list --count <base>..HEAD` ≥ 1** → `Implemented`、それ以外 → `Failed detail=runner-rc:<rc>,commits:<n>`（commit 0 は完了ではない）。stdout `run=<id> stage=<s>`。

### 5.3 gate（(b)・FR8 / FR9 / NFR1）
`pipe gate --run <id> [--lens <cmd>]`: 前提 = Implemented ∧ worktree clean（`git status --porcelain` 空）∧ commits ≥ 1。**違反の扱いは 2 通りに分ける**（いずれも rc 1 で lens は起動しない）。
- **worktree の事実**（clean でない / commits 0）の違反 → `RunStage stage=Failed detail=precheck:<理由>`。実装が済んだと名乗る便の中身が前提を満たしていない＝その便はここで終わる。
- **段違い**（`Implemented` でない）→ §4 の一般則どおり **何もせず rc 1**（event を 1 件も書かない）。gate を早く叩いただけの便を `Failed` で終端させると、`resume` が引けなくなる（`Failed` からは再開しない）。
- **機械検証**: contract の `verify` 各行を worktree で `sh -c` 実行し、行ごとの rc を `<state_dir>/pipe/<run>/verify.jsonl`（`{"schema":1,"n":<i>,"rc":<rc>,"cmd":"…"}`）に逐条記録。
- **lens**: 本数 = rules 行 `gate.lens_count`（MVP は 1）、cap = `gate.token_cap`。**本数は照合する**: `gate.lens_count` が 1 でない周（0 = lens を呼ばずに通す / 2 以上 = 1 本で足りたことにする）は「lens の verdict」を得ていないので **INCONCLUSIVE**（多 lens は (b) の射程外なので、実装しない代わりに fail-closed に断る）。`--lens <cmd>` に `git diff <base>..HEAD` を stdin で渡し、stdout の JSON 1 行 `{"verdict":"PASS|FAIL|INCONCLUSIVE","evidence":"…"}` を採る。
- **予算の照合**（NFR1「diff byte と cap の照合」）: diff の byte 数を `gate.token_cap` と**直接比べる**（byte ≥ token の保守的な読み・換算係数を持たない）。diff byte > cap → **INCONCLUSIVE**（lens を起動しない）。
- **判定順**（wildcard 無しの match）: verify に rc≠0 が 1 本でも → **FAIL** ／ diff byte > cap → **INCONCLUSIVE** ／ lens が要るのに `--lens` 無し・lens rc≠0・stdout が parse 不能・verdict が 3 値外 → **INCONCLUSIVE** ／ それ以外は lens の verdict。
- 結果は `<state_dir>/pipe/<run>/verdict.json`（`{"schema":1,"run":…,"verdict":…,"evidence":…,"verify_red":<n>,"diff_bytes":<n>,"ts":…}`）と `RunStage stage=Gated detail=verdict:<V>`。stdout `run=<id> verdict=<V>`・rc は PASS=0 / FAIL=1 / INCONCLUSIVE=3。

### 5.4 land（(b)・FR10 / FR11 / FR12・N1）
`pipe land --run <id>`: 前提 = Gated ∧ verdict.json が PASS（それ以外 = rc 1・**何もしない**）∧ `git rev-parse refs/heads/main` == 記録した `base`（違えば rc 1 `stale base`）。
1. `tree = git rev-parse <worktree HEAD>^{tree}` → `new = git commit-tree <tree> -p <old> -m "<bead>: <goal>"` → `git update-ref refs/heads/main <new> <old>`（CAS）→ `git rev-parse <new>^{tree} == tree`（lossless の実測）。
2. **main 実測**: `git worktree add --detach <tmp> <new>` して `verify` 全行を再実行。1 本でも rc≠0 → `RunStage stage=Failed detail=main-red` + rc 1（auto revert は MVP 外・main は進んだまま loud）。**実測そのものができなかった周**（tmp worktree を切れない等で verify を 1 行も撃てていない）は赤と別に `RunStage stage=Failed detail=main-unmeasured` + rc 2 で残す（「測れなかった」を「赤かった」に化けさせない＝gate の極性と同じ）。
3. 全 GREEN → **verdict export（面 5）**: `<state_dir>/fleet/verdicts.jsonl` へ `{"schema":1,"run":…,"bead":…,"sha":"<new>","verdict":"PASS","evidence":"<verdict.json の path>","ts":…}` を append（fleet と同じ lock）→ `RunDone stage=Landed`。
4. **後始末は可逆 move**（N1.2）: `git worktree move <worktree> <repo>/.worktrees/<NAME>/retired/<run>`。branch は消さない（squash commit は branch の祖先でないので `branch -d` は通らず、`-D` は N1 に反する）。tmp worktree（main 実測用）は `git worktree remove --force` してよい（scribe2 が作った一時物で、成果は `new` に載っている。`--force` を許すのは `verify` の生成物で dirty になった一時 worktree を leak させないためで、**「force 系 git を書かない」の趣旨は履歴・データの破壊**＝この掃除はそれに当たらない）。失敗は stderr 1 行で rc 0 のまま（land は成立している）。stdout `run=<id> landed=<new>`。
- `--pr-cmd <cmd>`（(e)・AC2 の自己ホスト形・**公開の口**）: squash の代わりに branch を push して PR を作る seam。`{branch}` `{base}` を置換して `sh -c` する。**前提に「run に `ApprovalReceived` が在る」を足す**（無ければ rc 1・何もしない）＝seam を使う便は契約が `publish` を名乗り、spawn の手前で承認を得ている（A1「実行前」）。既定は無し（core は PR 作成の道具を知らない）。この形では main を動かさず `Landed detail=pr` で終える（merge は人が押す）。

### 5.5 承認（(c)・FR15 / FR16 / AC5・A1 / C7）
- `classes` が非空の契約は、**spawn の手前**（実行前・A1）で `ApprovalRequested detail=<classes>` + `RunStage stage=Blocked` を記帳し、**人の入力を待たずに rc 3 で process を終える**（FR15）。runner は起動しない。
- `pipe approve --run <id> --words "<user の逐語>"` → `ApprovalReceived actor=human detail=<逐語>`（C7.2）。逐語が空なら rc 1・記帳しない。**この subcommand は開発 session（R-C7-1 = user 直）が user の言葉をそのまま写して叩く**。会話の記憶を根拠にしない＝event に残った逐語だけが承認である。
- `pipe resume` は Blocked ∧ `approved` で spawn へ進む（FR16）。Blocked ∧ 未承認は rc 3。
- **`approved` を立てるのは読み手側の資格検査である**: replay は `ApprovalReceived` ∧ `actor=human` ∧ 逐語が非空（trim 後）のときだけ `approved` を立てる。書き手（`pipe approve`）の逐語検査だけだと、`fleet record` で積んだ逐語 0 字の機械 event でも関門が開く。
- 3 クラスの判定は MVP では**契約の自己申告**（`classes`）に加え、**seam の使用からの導出**を 1 つ持つ: `--pr-cmd`（公開）は承認 event 無しでは動かない（§5.4）。操作の中身から 3 クラスを判定する enforcer は次の版（A4 の機構欄）。

### 5.6 stop（(a)・FR13・面 4）
`pipe stop --all`: replay で `SeatState::Live` な seat を列挙し pid へ `kill -TERM`（std::process で `kill`）→ `wait(Completion::SeatGone(pid), 猶予)` の猶予は rules 行 `pipe.stop_grace_ms` → 残れば `-KILL` → 各 seat に `SeatStopped`・run に `RunStopped stage=Stopped`。**rc = 0: 全部止まった / 対象なし（冪等）・1: 止められない seat が残った・2: state が読めない**。stdout `stop: seats=<N> stopped=<M>`。

### 5.7 show / resume / run
- `pipe show --run <id>` → `run=<id> bead=<b> stage=<s> approved=<bool> worktree=<path>`（無ければ rc 1）。
- `pipe resume --run <id> [--runner] [--lens]`: 現在 stage から**続きの段だけ**を通す（Intake → spawn / Blocked+approved → spawn / Implemented → gate / Gated(PASS) → land）。Stopped / Failed は rc 1。
- `pipe run --contract <f> --bead <id> --repo <dir> --runner <cmd> [--lens <cmd>]` = intake → spawn → gate → land を 1 process で連続（各段は fleet を読み書きし、途中で落ちても `resume` が続きを引く）。

### 5.8 report（(e)・FR22）
`pipe report`: event log を replay し `runs=<N> landed=<N> human_events=<N> human_events_other_than_approval=<N>` の 1 行。到達点の「人由来の event が approval 以外に 0 件」を機械で示す面（AC1）。

## 6. headless runner と lens（(d)・FR5・CON6・NFR1）

- `<NAME> runner --worktree <dir> --write-set <f> --plugin-dir <dir> --permission-mode <mode> [--account-dir <dir>] [--claude <path>]`: **契約本文は stdin で受け**（FR5「stdin に契約」）、write-set と合わせて prompt に組み、`claude -p` を **cwd = worktree・`--output-format stream-json --verbose`（`-p` との併用では claude が `--verbose` を要求する）・`--permission-mode` を毎回明示・`--plugin-dir` で本 repo の plugin（hooks）を載せて** 起動する。**口座は子 process の環境変数（設定 dir）で切り替える**（FR5「口座は環境変数で切替」）。scribe2 自身は env を読まない（C2.2）＝子へ設定するのは「読む」ではない。stream-json に rate limit の error record が出たら rc 75 で止める（呼出側は `Failed detail=rate-limit`）。rc は claude の rc を写す。
- `<NAME> lens --cap <bytes> --permission-mode <mode> [--account-dir] [--claude <path>]`: stdin の diff が cap を超えたら **claude を呼ばずに** `{"verdict":"INCONCLUSIVE","evidence":"diff exceeds cap"}`。それ以外は診断 prompt（契約の verify と diff を読み PASS / FAIL / INCONCLUSIVE を JSON 1 行で返せ）で `claude -p` を **`--output-format` を渡さず既定（text）で**呼び、出力の最後の JSON 行を stdout 1 行に写す（stream-json にすると全行が JSON になり、最後の JSON 行は claude 自身の result record になって判定が取れない）。parse 不能は INCONCLUSIVE。
- 両 wrapper は `pipe` の seam にそのまま渡せる 1 行（例: `--runner "<NAME> runner --worktree {worktree} --write-set {write_set} --plugin-dir <dir> --permission-mode acceptEdits < {contract}"`）。
- `--claude <path>` は test の seam（fake の実行 file が引数と stdin を file に写す）。prompt の文面は tracked な template file（`crates/<NAME>/src/headless/*.txt`）で持ち、絶対 path・口座名を含めない。

## 7. FR7（入口の flip check）の置き場

本 repo 自身の flip check は `cargo xtask flip-check` と CI の job が担う。**CI の flip-check job は未 land**（`s2-07l.17`・現状の CI は nextest / clippy / xtask-check の 3 job）ため、AC3 の GREEN は `s2-07l.17` の land を前提に並べる。pipeline は契約の `verify` 行として flip check を撃つ（Rust repo の契約は `cargo xtask flip-check --base {base}` を verify に含める）。pipeline が Rust 固有の検査を内蔵する形は採らない（toy repo は Rust でないことがある）。

## 8. 歯（契約ごと・`tests/e2e/pipe.rs` module・tmp git repo（`.vessel` に `name=<NAME>`・`vessel init --state-dir` で tmp を紐づける）・fake runner / lens は `sh -c` 1 行）

- (a) `pipe_` 接頭辞: `pipe_intake_rejects_missing_field` / `pipe_intake_rejects_multiline_verify` / `pipe_intake_rejects_contract_without_req_or_design` / `pipe_intake_records_run_in_fleet` / `pipe_spawn_creates_worktree_and_records_implemented` / `pipe_spawn_marks_failed_when_runner_makes_no_commit` / `pipe_spawn_writes_write_set_into_git_dir` / `pipe_spawn_substitutes_placeholders_and_adds_no_env`（fake runner が env を全部 stdout に写し、test は `<NAME_UPPER>_` で始まる変数が 0 本であることと置換結果を assert）/ `pipe_spawn_refuses_wrong_stage` / `pipe_state_survives_process_restart` / `pipe_stop_all_rc0_when_nothing_to_stop` / `pipe_stop_all_terminates_live_runner` / `pipe_stop_returns_rc2_on_malformed_store` / `pipe_external_form`（snapshot）。
- (b) `pipe_gate_` / `pipe_land_` / `pipe_e2e_` / `pipe_resume_` 接頭辞: `pipe_gate_refuses_dirty_worktree` / `pipe_gate_fails_on_red_verify_line` / `pipe_gate_inconclusive_without_lens_when_required` / `pipe_gate_inconclusive_when_diff_exceeds_cap`（`--rules` で tmp manifest・`gate.token_cap = 1`）/ `pipe_gate_records_structured_verdict` / `pipe_land_refuses_without_pass` / `pipe_land_squashes_one_commit_with_identical_tree` / `pipe_land_refuses_stale_base` / `pipe_land_reruns_verify_on_main_and_fails_loud` / `pipe_land_exports_verdict_schema1` / `pipe_land_retires_worktree_by_move_and_keeps_branch` / `pipe_e2e_toy_repo_lands_one_bead_with_fake_runner` / `pipe_resume_after_kill_between_spawn_and_gate`。
  実装の便で**変異と lens review が「測れていない」と名指した経路**に足した歯（同じ 4 接頭辞）: `pipe_gate_refuses_run_without_commits` / `pipe_gate_inconclusive_on_unlisted_lens_verdict` / `pipe_gate_inconclusive_when_lens_count_is_not_one` / `pipe_gate_inconclusive_when_lens_exits_nonzero` / `pipe_gate_inconclusive_when_lens_output_is_not_json` / `pipe_gate_passes_diff_to_lens_on_stdin` / `pipe_gate_refuses_wrong_stage` / `pipe_land_reports_unmeasured_main_apart_from_red` / `pipe_land_removes_dirty_tmp_worktree`。
- (c) `pipe_approval_` 接頭辞: `pipe_approval_blocks_before_spawn_when_contract_declares_class` / `pipe_approval_records_verbatim_as_human_event` / `pipe_approval_refuses_empty_words` / `pipe_approval_resume_spawns_after_received` / `pipe_approval_resume_stays_blocked_without_received` / `pipe_approval_unlisted_class_value_is_rejected_at_intake`。
  実装の便で lens が「測れていない」と名指した経路に足した歯: `pipe_approval_blocks_in_one_shot_run`（`pipe run` の一発経路も spawn の手前で Blocked になる＝関門が段ごとの口ではなく唯一の起動口に在ることを測る）。
- (d) `headless_` 接頭辞（claude は fake の実行 file・`--claude <path>`）: `headless_runner_reads_contract_from_stdin_and_passes_permission_mode_every_time` / `headless_runner_stops_on_rate_limit_record` / `headless_lens_inconclusive_over_cap_without_calling_claude` / `headless_lens_extracts_last_json_line` / `headless_lens_inconclusive_on_unparsable_output`。
- (e) `pipe_report_` / `pipe_five_` / `pipe_land_pr_cmd_` 接頭辞: `pipe_five_contracts_land_with_fake_runner_in_toy_repo`（正常 / write-set 外編集で guard に止まる / test 追加 / gate FAIL / 承認 Blocked → approve → land の 5 便・stdin は `/dev/null`）/ `pipe_report_counts_human_events` / `pipe_land_pr_cmd_refuses_without_approval` / `pipe_land_pr_cmd_pushes_branch_without_moving_main`。

## 9. 到達点の計測（AC1 / AC2・(e)）

AC1 の条件文は「実 runner + 実 lens」なので、CI の歯（fake）は AC1 を測らない。実測は **機械が読む成果物**で残す。

- **toy repo 5 便（AC1）**: 開発 session が `<NAME> runner` / `<NAME> lens` を seam に渡して手元で 5 便を通し、`pipe report` の 1 行（`human_events_other_than_approval=0`）と `verdicts.jsonl`（5 行）を bead `s2-07l.24` の notes に**逐語で**写す。人由来の event は approval の 1 件だけ。
- **自己ホスト 1 便（AC2）**: 本 repo の root に `.vessel`（`name=<NAME>` / `version=2`）を置く PR を先に land し（この便から guard が本 repo に効く）、実 bead 1 本の契約 file（`req` と `design` と `classes = ["publish"]` を持つ）を `pipe run … --pr-cmd` で PR 作成まで通す。承認は `pipe approve` で逐語を記帳する。CI 緑・merge は人。
- AC3（偽の PASS 0 件）は (b) の `inconclusive_*` 2 本 + `fails_on_red` + `refuses_without_pass` が母集団（FR7 の面は `s2-07l.17` の CI job）。AC4 は `pipe_resume_after_kill_*` と `pipe_state_survives_*`。AC5 は (c) 6 本 + `pipe_land_pr_cmd_refuses_without_approval`。

## 10. 却下案

- state を in-memory で持つ長寿命 process（FR3・AC4 に反する）。
- runner へ scribe2 固有の env で run 情報を渡す（C2.2）。placeholder 置換で足りる。
- bd を直接 write（台帳は bead id を持つだけ・SRS scope out）。
- worktree を repo 外に置く（`.worktrees/` の運用と揃える）。
- force 系 git・auto revert（N1・CON5。main red は loud に止める）。後始末を `worktree remove` + `branch -d` で行う（削除は N1・`branch -d` は squash では通らない・lens 指摘で却下）。
- stop を「tmux 窓を kill」で実装（MVP に tmux は無い・pid で止める）。
- 承認を land の手前に置く（merge は可逆・A4.3。不可逆の実行は runner の中で起きるので spawn の手前）。
- gate に Rust 固有の flip check を内蔵（toy repo は Rust とは限らない）。
- token → byte の換算係数を code に埋める（閾値は manifest・C1。byte を cap と直接比べる保守的な読みにした）。
- PR 作成の道具を core に内蔵（seam `--pr-cmd`。道具の選定は契約側）。承認 event 無しで `--pr-cmd` を動かす（A1・lens 指摘で却下）。
- 縦 1 本を 1 契約で書く（見積 ≈1,100 行・NFR2）。

## 11. 後続

- 3 クラスの機械 enforcer（操作の中身からの判定・A4 機構欄）。R-C6-1（1 run の token 上限）の裁定が出たら `Budget` に上限を効かせる。
- 多 lens・tier・verdict 一致率（v3）。tmux / 席 / 口座選定（v3）。
- retired worktree の掃除の道具化（可逆 move の先を片付ける経路・N1.2）。
