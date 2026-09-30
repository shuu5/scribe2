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
- **不可逆の口（CON5・N1）**: force 系 git・削除の subcommand は無い（後始末は可逆 move・§5.4）。**「出す」= public 化・外部送信**（憲法 A4 の語釈）で、core は「出す」を自分の subcommand として持たない。ただし `--pr-cmd` は任意の `sh -c` を通す seam ゆえ、外向きの道具を渡せば器は止めない（ADR-0008 §3 Negative・弁別は次版の enforcer）。自 repo への branch push と PR 作成（`--pr-cmd`・§5.4）は A4.3 で可逆ゆえ「出す」に当たらず、承認 event を前提としない（[ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html)）。**課金**: runner / lens は定額の subscription 内で走り、追加課金の口を持たない（「使う」= 追加課金だけ・要件カタログ R-F4）。
- **起動口は 1 つ**（C6）: runner を起動できる関数は `fn spawn(budget: Budget, …)` の 1 本で、`Budget` は `Precheck` の実測を消費してしか作れない。CLI の `spawn` / `resume` / `run` はこの 1 関数への経路であって別の口ではない。

## 3. 契約 file（TOML subset・[ADR-0004 §2.3](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html#s2-3-contract-and-manifest)）

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `goal` | string | 必須 | 1 行の目的 |
| `done` | string | 必須 | 何ができたら終わりか |
| `size` | string | 必須 | `S` / `M` / `L`（NFR2 の見積の目安） |
| `owner` | string | 必須 | bead id（文字列として持つだけ・台帳は読まない） |
| `disposition` | string | 必須 | `A-now` / `F` |
| `write-set` | string 配列 | 必須・1 本以上 | 触ってよい file / dir（末尾 `/`）。接頭辞 `+`（新規 file）/ `-`（縮む面）は受付の宣言で、guard と allowlist は素の path で持つ（[contract-source.md](./contract-source.md) §3） |
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
| `Spawned` | `RunStage` + `SeatSpawned` | runner 終了 → `SeatStopped` + Implemented / Failed（FR6）。包みが rc `RC_QUESTION`（76）で終わり stdout の最終行が質問 record → `SeatStopped` + `QuestionRaised(detail=逐語)` + `RunStage(Questioned)`（FR31・[pipeline-question.md §3](./pipeline-question.md)） |
| `Questioned` | `QuestionRaised` + `RunStage`（`detail=about:<key>`・任意） | **最新の質問より後**の `QuestionAnswered`（逐語が非空）が在れば resume → spawn（**同じ run・同じ worktree・記録済みの base**・FR32）。無ければ `resume` は rc 3 で何も書かない（`Blocked` と同型）。rc 76 で record が無い周・record と commit が同時の周は `Failed` |
| `Implemented` | `RunStage`（spawn の完了・または land の追随 `detail=rebase:<old>..<new>` で `Gated` から戻る周・§5.4・**予定形（ADR-0019・契約 (b) の land まで現物には無い）**: 衝突からの起こし直し待ち `detail=rebase-conflict:<base>..<main>` と起こし直し後の base 記帳 `detail=rebase:<old>..<merge-base>` も本段＝[pipeline-conflict.md](./pipeline-conflict.md) §3） | gate |
| `Gated` | `RunStage detail=verdict:<V>` | PASS → land（**base が main の祖先のまま動いていれば** land の前段で worktree の branch を main へ rebase → `RunStage stage=Implemented detail=rebase:<old>..<new>` で段を戻す → gate を同じ関数で撃ち直す → PASS なら新 base で CAS・§5.4）／ **INCONCLUSIVE → 道具を揃えて gate を撃ち直す**（`resume` は `next=gate` で rc 3）／ FAIL は終端（FR10 / FR14・**予定形**: ADR-0019 §2.4 で `pipe retire` が畳める側に入る＝契約 (b) の land まで現物は畳めない） |
| `Landed` | `RunDone` | 終端。`--pr-cmd` 形は merge の後に `pipe retire --run <id>` で worktree を畳む（`RunStage detail=retired`・段は `Landed` のまま） |
| `Stopped` | `RunStopped` | 終端（`stop --all` / `stop --run`）。worktree は `pipe retire --run <id>` で畳める（clean のときだけ・`RunStage detail=retired`・段は `Stopped` のまま・[pipeline-conflict.md](./pipeline-conflict.md) §5・`s2-07l.284`） |
| `Failed` | `RunStage detail=<理由>` | 終端（resume は rc 1）。`detail=rebase-empty` の便は `pipe retire --run <id>` で worktree を畳める（**予定形**: ADR-0019 §2.4 で `rebase-conflict` も畳める側に入る＝契約 (b) の land まで現物は `rebase-empty` だけ）（`RunStage detail=retired`・**段は `Failed` のまま**・`s2-07l.128`） |

前提違反は **rc 1 + stderr 1 行・何もしない**（event も追記しない）。

## 5. subcommand（`<NAME> pipe …`・全部に `[--state-dir D]` `[--rules PATH]`）

### 5.1 intake（(a)）
`pipe intake --contract <file> --bead <id> --repo <dir>` → §3 の検査 → **撃てない契約の拒否**（[ADR-0009 §2.2](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-2-intake-refuses)・[ADR-0010 §2.3](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-3-intake-measures)）: まず repo の **HEAD commit の tree** から `.vessel.toml`（vessel 宣言・flat TOML subset・`schema` / `allowed-commands` / `common-verify` 必須・不備は全件 行番号付き・作業ツリーは読まない＝未 commit の宣言は存在しないのと同じ）を読み、**Declared → 突合（出所 = 読んだ commit の sha・宣言 path・上限行 id）→ Effective**（C10）: (1) `allowed-commands` の各要素が manifest の上限 `runner.allowed_commands` に含まれる／(2) `common-verify` 各行が**空白区切りの argv 1 本**（先頭 command が**宣言の** `allowed-commands` に含まれ・shell の制御文字〔`;` `&` `|` `` ` `` `$` `(` `)` `<` `>` 引用符・改行〕を含まず・repo の外の path〔絶対 path・home の短縮記号〕を含む語が無く・穴は `{base}` だけ。gate は行を `sh -c` で撃つので先頭語だけでは境界にならない）／(3) 契約 `verify` 各行にも (2) と同じ検査（基準は**宣言の** allowlist・上限ではない・契約行は穴を持たない＝`{base}` 不可）／(4) `common-verify` / `detection-verify` / 契約 `verify` の各行に rules 行 `runner.denied_commands` の語列の判定（hook の command guard と同じ 1 関数・[ADR-0025 §2.3](../../design-intent/decisions/ADR-0025-denied-command-rows-and-bash-command-guard.html#s2-3-intake)・当たる行は行番号付きで断る・`Guard::Intake` の理由が 1 つ増えるだけで Guard は増えない）。宣言の不在・空・1 件でも外れは rc 1 で断る（event を書かない）＝runner が撃てない検証行・憲法の視野の外の script を便に持ち込ませない → `<state_dir>/pipe/<run>/contract.toml` と **`vessel.toml`（Effective の写し・以後の段はこれだけを読み repo / worktree の宣言を読み直さない＝便の自己拡張の閉塞）** へ写す → `RunCreated stage=Intake` → stdout `run=<id>`（`--rules <path>` で上限を差し替えて通した周は同じ行に `ceiling-overridden=<path>` を後置する＝差し替えた事実を review が拾える面。値は渡した path の字面そのもの＝quote しないので空白・改行を含む path では 1 語にならない〔seam の path は呼び手が決める〕・差し替えていない周は出さない・`s2-07l.65`）。run id = `<bead>-<UTC stamp>`。

### 5.2 spawn（(a)・FR4 / FR6・C6）
`pipe spawn --run <id> --runner <cmd>`: 前提 stage = Intake（3 クラス有りなら §5.5）。
1. **Precheck → Budget**（C6）: `Precheck::measure(contract, repo)` が write-set の本数・`verify` の本数・`size` を実測して `Budget` を作る。`Budget` はこの経路以外で作れない（private constructor）。`fn spawn(budget: Budget, run, runner) -> Outcome` が **runner を起動できる唯一の関数**。MVP の Budget は上限を効かせない（R-C6-1 が未定）が、型の形を先に置く。
2. `base = git -C <repo> rev-parse HEAD` を event に記録。
3. `git worktree add -b <NAME>/<run> <repo>/.worktrees/<NAME>/<run> <base>`（既存なら rc 1）。
4. write-set を `<worktree の git dir>/<NAME>/write-set.txt` に 1 行 1 path で書く（guard が読む形・[vessel-hook.md §5](./vessel-hook.md)・tracked 面に触れない）。
5. **plugin の root `<state_dir>/pipe/<run>/plugin/` を組む**（§6 の runner がこの root の配下を 1 dir = 1 plugin として claude の `--plugin-dir` に渡す）: (i) **器の plugin**（binary に埋め込んだ `plugin/.claude-plugin/plugin.json` と `plugin/hooks/hooks.json`＝`gen-manifest` の生成物と同じ bytes・生成 dir は [consumer-sync.md](./consumer-sync.md) §17）を `<state_dir>/pipe/<run>/plugin/<NAME>/` に**必ず**書く＝plugin を持たない consumer repo でも hook 側の in-loop guard 3 本（write-set guard・permission の deny・cap guard）が便に載る（憲法 C16 / C16.2・`s2-07l.149` 裁定 (A)・2026-09-13）。(ii) worktree の生成 dir（core の `PLUGIN_DIR`）の下に manifest と hook の manifest が**両方**在り、その plugin.json の `name` の値（既存の JSON 読み手で取る）が `NAME` と**違う**ときだけ、生成 dir の下の 2 dir を run dir の `plugin/consumer/` の直下へ写す（root 直下の旧 path にしか持たない worktree は consumer の plugin と見ない）（file だけ・**symlink は追わない**＝dir 自体が link の面も写さない・写すのは **worktree の**中身＝便の base の内容であって anchor の現在値ではない）。`name` が `NAME` と同じ周は器自身の repo＝世代がずれていても器の 1 本だけを載せ、同じ hook を 2 度走らせない。片方だけ在る周・`name` が読めない周は consumer の plugin とは見ない（写さない）。再走のため root を先に空にする。
6. `RunStage stage=Spawned` → runner を `sh -c <cmd>` で **cwd = worktree** で起動し `SeatSpawned seat=<run> pid=<pid>`。runner の **stdin には契約の写し（`contract.toml`・再読）を流し**、回答済みの質問からの再 spawn ではその末尾に「## 回答」節（`QuestionRaised.detail` と `QuestionAnswered.detail` の対）を付ける。**stdout は捕らえる**（`gate.rs::ask_lens` と同じ piped + `wait_with_output`・質問 record の読み面）。cmd 中の placeholder `{run}` `{worktree}` `{contract}` `{write_set}` `{base}` `{plugin_dir}`（= 手順 5 の plugin root）`{vessel}`（= §5.1 の Effective の写し `vessel.toml`・[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)）を置換する。**scribe2 固有の env は 1 つも足さない**（親の env はそのまま継承・ADR-0004 §2.4）。
7. **runner の終了待ちは `wait_with_output`**（rc と stdout を運ぶ・待機の実装は増えない）。rc が `RC_QUESTION`（76）の周だけ stdout の最終 JSON 行を質問 record（`question` 必須非空 1 行・`about` 任意）として読み、commit 0 なら `QuestionRaised` + `RunStage(Questioned)` を記帳して `run=<id> stage=Questioned question=<id>` を出し **rc 3** で止まる。record が無い / 読めない周は `Failed detail=question-record-missing:<理由>`、record と commit が同時の周は `Failed detail=runner-rc:76,commits:<n>`。rc が 76 でない周は最終行を読まない。捕らえた stdout は**全文を `<state_dir>/pipe/<run>/runner.stdout.log` へ見出し行（`## <ts> rc=<rc>`）付きで append する**（包みの観測行 `runner: rc=… records=… observed=…` を端末から消さない・機械は読まない診断 file・空の周は書かない）。pid の生存待ち（`stop`）は `wait(Completion::SeatGone)`（[fleet-event-log.md §4](./fleet-event-log.md)）で、`Completion::RunnerExited` は**別 process が spawn した runner を待つ resume 経路のために残す**。`SeatStopped`。**rc 0 ∧ `git rev-list --count <base>..HEAD` ≥ 1** → `Implemented`、それ以外 → `Failed detail=runner-rc:<rc>,commits:<n>`（commit 0 は完了ではない）。stdout `run=<id> stage=<s>`。

### 5.3 gate（(b)・FR8 / FR9 / NFR1）
- 資源の受付（実効 jobs）・子 process の封じ込め・`detection-verify` の段（①②③④）は [gate-cost.md](./gate-cost.md) §3〜§5（ADR-0021 §2.6 の部分 supersede）が本節を上書きする（**契約 land 後**・本節の①②③と `{base}` 唯一の穴は land 前の現物と一致）。
`pipe gate --run <id> [--lens <cmd>]`: 前提 = **`Implemented` ∨ (`Gated` ∧ verdict が INCONCLUSIVE)** ∧ worktree clean（`git status --porcelain` 空）∧ commits ≥ 1。**違反の扱いは 2 通りに分ける**（いずれも rc 1 で lens は起動しない）。
- **worktree の事実**（clean でない / commits 0）の違反 → `RunStage stage=Failed detail=precheck:<理由>`。実装が済んだと名乗る便の中身が前提を満たしていない＝その便はここで終わる。
- **段違い**（`Implemented` でも `Gated(INCONCLUSIVE)` でもない）→ §4 の一般則どおり **何もせず rc 1**（event を 1 件も書かない）。gate を早く叩いただけの便を `Failed` で終端させると、`resume` が引けなくなる（`Failed` からは再開しない）。
- **測り直し**（`Gated` ∧ INCONCLUSIVE・FR14）: INCONCLUSIVE は道具が足りず判定に届かなかった印（`--lens` 無し / diff が cap 超 / lens の不備）ゆえ、道具を揃えて**同じ便を撃ち直せる**。`verdict.json` は最後の判定で上書きし、`RunStage stage=Gated detail=verdict:<V>` は**追記**する（append-only＝1 度目の INCONCLUSIVE が残る）。**PASS / FAIL は終端**（判定に届いた周＝撃ち直す口を開けない。verify が赤い便は測り直しても赤い＝「壊れたまま進まず」GOAL 2）。**測り直しの周も worktree の事実の違反は `Failed` で終端する**（道具を揃える前に worktree を clean へ戻す）——precheck の極性を段ごとに分けると「段の検査は入口 / worktree の事実は gate」の分離が濁るためで、終端しても worktree と branch は残る（N1）＝作り直せるのは run 1 本の側である。verdict を読む関数は `pipe/land.rs` の `verdict_of` の 1 本で、land の前提・gate の入口・resume の行き先が同じ値を見る（**判定が読めない周**——file 不在 / JSON が壊れ / 3 値の外——は INCONCLUSIVE と同じ扱いにせず断る）。残るのは **3 値の履歴だけ**で、1 度目の evidence（なぜ測れなかったか）は `verdict.json` の上書きで消える。`verify.jsonl` は同じ file へ 2 周目を**追記**する（`n` は周ごとに 1 から＝file 内で一意ではない。INCONCLUSIVE の周は lens に届く前で止まっているので古い行が偽の RED を作ることはない。段①が読めない周は `verify_red > 0` の INCONCLUSIVE になりうる＝測れなかったは赤より先・`s2-07l.65`）。**同一便へ `pipe gate` を並行して撃たない**（`verdict.json` の write は event の lock の外にあり、file の最終内容と最終 event が別の周を指しうる）。
- **機械検証**（順序は [ADR-0009 §2.4](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-4-common-verify)）: ① **write-set の照合**を Rust の 1 関数で行う（`git diff --name-only <base>..HEAD` の各 path が契約 write-set のいずれか〔file 一致 or dir prefix〕に含まれる・外れが 1 件でも赤・外れた path を `verify.stderr.log` に列挙。**diff の path を読めない周は赤ではなく「測れなかった」**＝record は残し（rc は u64 の記録形 255・`verify.stderr.log` の見出しは -1）、②③ は従来どおり撃って record し（費用は ②③ 分・record を欠かさないため）、判定は lens を呼ばずに INCONCLUSIVE〔既存 3 値の内側〕へ倒す。land の `main-unmeasured` と同じく「赤ではない」側だが、land が `Failed` で終端するのに対し gate は `Gated` に留まり測り直せる（FR14）。段①の -1 は `verify_red` に数えない・`s2-07l.65`）→ ② run の写し `vessel.toml` の `common-verify` 各行（[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)・manifest の行ではない・`{base}` を便の base に置換・repo 共通の検証＝Rust repo なら flip check・test・lint・依存監査）→ ③ 写しの `detection-verify` 各行（任意 key・検出線＝変異検出・穴は②と同じ・rc 1〔R-C12-1 が deny に昇格した周だけ現れる〕は②と同じく赤・**rc 2〔測れなかった・道具の不在・baseline 落ち〕は赤に数えず判定を INCONCLUSIVE へ倒す**〔他の行の赤が 0 の周だけ・赤が在る周は FAIL が先＝[gate-cost.md](./gate-cost.md) §28・`Gated` に留まり測り直せる・FAIL にして runner をもう 1 周払わせない・`s2-07l.331`〕・木が gate と同じ main 実測では撃ち直さない・[gate-cost.md](./gate-cost.md) §5・ADR-0021 §2.4）→ ④ contract の `verify` 各行（便固有の行だけ）。**① は Rust の照合で `sh -c` を撃たない**（record の `cmd` は段の名 `write-set`）・②③④ は worktree で `sh -c` 実行し、①〜④を**通し番号 `n`** で並べて行ごとの rc を `<state_dir>/pipe/<run>/verify.jsonl`（`{"schema":1,"n":<i>,"rc":<rc>,"cmd":"…"}`）に逐条記録。**rc≠0 の行だけ、その行の stderr の末尾 20 行を `<state_dir>/pipe/<run>/verify.stderr.log` へ見出し行（`## n=<i> rc=<rc> cmd=<cmd>`）付きで append する**——rc だけでは「何がどう赤いか」が便の外から読めず、落ちるたびに人が同じ行を手で撃ち直して理由を取り直すことになる。`verify.jsonl` の record の形は変えない（跨版の契約ゆえ不変）＝`verify.stderr.log` は**機械が読まない診断 file** で、緑の行は残さない（読む理由の無い出力で埋めると赤い行の見出しが埋もれる）。同じ file に lens への入力の通知 1 行（`# lens-input=<kind> reason=<語>`・§21）だけは rc に依らず足す——段の出力ではなく**段の見出しを持たない 1 行**なので、赤い行の見出しは埋もれない。
- **lens**: 本数 = rules 行 `gate.lens_count`（MVP は 1）、cap = `gate.token_cap`。**本数は照合する**: `gate.lens_count` が 1 でない周（0 = lens を呼ばずに通す / 2 以上 = 1 本で足りたことにする）は「lens の verdict」を得ていないので **INCONCLUSIVE**（多 lens は (b) の射程外なので、実装しない代わりに fail-closed に断る）。`--lens <cmd>` に `git diff <base>..HEAD` を stdin で渡し、stdout の JSON 1 行 `{"verdict":"PASS|FAIL|INCONCLUSIVE","evidence":"…"}` を採る。**cmd の `{contract}` / `{worktree}` は run の path へ置換する**（`--runner` 側と共有するのは placeholder の語彙であって置換関数ではない・置く穴は **2 つ**〔`{contract}` / `{worktree}`〕・出所 `s2-07l.60`）。穴が 2 つ目を持つのは、lens に憲法（生成 file `docs/constitution.md`）を載せる経路が**起動 cwd 1 本**で、tracked file に絶対 path は書けない（PUBLIC repo）ため worktree を gate が埋めるほかないからである。置換は **1 走査**で行う（重ねて replace すると先に埋めた path の中の `{worktree}` まで展開されうる）——lens に問うのは「diff が**契約の**求めるものを満たすか」なので、diff だけを渡すと実 lens は「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる（実測 2026-09-10・`s2-07l.24` の実 5 便）。**渡すのは path であって本文ではない**（cmd は `sh -c` の 1 行ゆえ、本文を埋めると契約の中の引用符 1 つで cmd の構造が変わる）。**穴を持たない古い `--lens` は fail-closed に落ちる**——`{contract}` / `{worktree}` を書いていない cmd へ `<NAME> lens` を渡すと lens 自身が rc 1 で断り（`--worktree` は `--contract` と同じ必須 flag で、無ければ claude を起こさない＝憲法の載らない判定を出さない）、gate は判定順の 3 番目で INCONCLUSIVE にする（極性は正しいが、gate は lens の stderr を捨てる〔`Stdio::null()`〕ので evidence に残るのは「lens が rc 1 で終わった」だけ＝**理由は lens を手で 1 回叩いて読む**）。
- **予算の照合**（NFR1「diff byte と cap の照合」）: diff の byte 数を `gate.token_cap` と**直接比べる**（byte ≥ token の保守的な読み・換算係数を持たない）。diff byte > cap → **INCONCLUSIVE**（lens を起動しない）。
- **純移動の機械証明**（`s2-07l.266`・user 裁定 2026-09-14「それでよい。後で戻すのを忘れないで」＝分割便が cap に当たる問題の恒久解・cap を一時的に上げた `s2-07l.265` の対・戻しは `s2-07l.267`）: 予算の照合の**前**に、diff が**純移動**かを純関数（`pipe/move_proof.rs`・I/O は gate 側）で判定する。**item** = base と HEAD の「diff に現れる `.rs` file」で **列 0 から始まる宣言単位**（`fn` / `struct` / `enum` / `impl` / `trait` / `const` / `static` / `type` / `mod <name> {`・直前に連なる属性行と doc コメント〔`///` / `#[…]`〕を含む・終端は列 0 の `}` か次の item の開始＝入れ子〔`impl {}` の中の fn・inline `mod tests {}` の中の歯〕は外側の item 1 本の本文に含める）。本文の正規化は **行頭の indent の除去**と**コメント行の除外**（`s2-07l.294`・2026-09-14 の .286 = gate.rs の 3 module 分割が 2 周とも純移動と読まれなかった型: module を跨ぐ移動では doc コメントの intra-doc link の path〔`[`super::x`]` → `[`crate::…::x`]`〕の書き換えが常に要る＝コメントは挙動を持たないので、item の区間のうち `//` / `///` / `//!` で始まる行〔行頭の indent の後〕は hash に入れない。**札の字面**〔`// flip-check:` で始まる行〕だけは除外せず従来どおり残差の検査に掛ける〔`retroactive` を item の中に隠す形を作らない・`ForeignMarker`〕。除外したコメント行の差は item ごとに数えて要約に載せる〔「コメント行の差 N」・lens が読める〕）。行内の空白と文字列 literal は変えない（pane の字面を持つ歯の literal を潰さない）。**可視性の prefix**（`pub` / `pub(crate)` / `pub(super)`）は item の名の前から剥がして hash に入れず、剥がした前後を item ごとに記録する（子 module へ出した helper は必ず可視性が広がる＝`s2-07l.257` の「本文字面不変・可視性と改名のみ」と同じ扱い）。(名, 本文の hash) の**多重集合**が HEAD と base で一致（追加 0・削除 0・本文差 0）し、**移動した item が 1 つ以上**在り、**残差分**（両側の diff 行のうちどの item の区間にも入らない行）が `mod` / `use` / `pub use` / `#[path]` / `#[cfg(test)]` / `// flip-check: moved <id>` の宣言と札、**item に付かない裸のコメント行**（`//` / `//!` / `///`・module doc と区切り線）、空行だけなら**純移動**（`s2-07l.261` の diff = `pub(super)` 化 9 行 + `//!` 31 行 + 区切り 26 行がこの形に当たる＝本機構の出所の便を通す基準）。純移動の周は lens の入力を diff でなく**要約**（型 `MoveSummary`: file ごとの item の移動元 → 先と本数・行数・可視性が変わった item の一覧〔名 + 前 → 後〕・宣言と札の残差分〔逐語・小さい〕・「名 + 本文の多重集合が一致」の判定行 1 本）にし、雛形 `lens.txt` の `{diff}` の穴に**そのまま**入れる（雛形は変えない＝`lens_prompt_external_form` の snapshot は動かない・要約の先頭行が「これは diff ではなく純移動の要約である」と名乗る）。予算の照合は lens に渡す本文の byte で行い、`verdict.json` の `diff_bytes` は従来どおり diff の byte のまま（意味を変えない・要約の byte は判定行に出す）。lens への入力は閉じた型（`LensInput::Diff` / `LensInput::Summary`・C3.3 の判定入力）で運び、要約の本文は run dir に `lens-input.txt` として残す（事後に読める・NFR4）。純移動でない周は従来の diff。結果は gate の stdout の判定行に `lens-input=<diff|summary> bytes=<N>` として出す（gate の外形 snapshot が動く周は同じ便で更新）。**極性**（C11.2 / C16.2）: 純移動の誤判定は lens から diff を奪う側に倒れる（FailOpen・PostHoc）ので `Guard` に variant 1 つ（`MoveProof`）を足し、極性一覧に載せる（in-loop の本数は変わらない・行数 +1）。切り出しの立場は閉包（[contract-source.md](./contract-source.md) §3）と同じ**下界**（構文木を持たない・A3 の依存を足さない）: macro で生成する item・1 行に複数の item は純移動と判定しない（保守側に倒れ lens が diff を読む従来形になる）。item の中のコメント行だけの差は hash に入らない（上の正規化・`s2-07l.294`・要約に件数が載る）。歯: `s2-07l.261` と同型の fixture（1 file → 複数 file の移動・`pub(super)` 化・module doc・区切り線つき）で lens 入力が要約になり verdict が読める／本文を 1 行変えた fixture は純移動でなく diff が渡る／宣言と札とコメント以外の行が残る fixture も diff が渡る／移動 item 0 の fixture（宣言だけ）は純移動でない／要約の外形は snapshot／判定の純関数は in-file（可視性の剥がしと入れ子の切り出しを直接撃つ）。**持ち越しの札**（`s2-07l.362`・契約表の行 e）: base に元から在る `moved` 以外の札（`retroactive` 等）が item ごと head へ移る周は、両側で同じ字面（id まで）の札を**対にして**残差から外し、対の無い札（head だけの新規・id 違い・base だけの消えた札）だけを `ForeignMarker` にする＝移動の中に新しい札を隠せない意図は保ったまま、持ち越しを新規と読まない。要約は持ち越した札の本数を 1 行で名乗る。
- **判定順**（wildcard 無しの match）: 箱の中で殺された行（**検出線以外の行**の `oom_kill`・どの行でも包みごとの signal 死＝[gate-cost.md](./gate-cost.md) §4.2）が在る → **INCONCLUSIVE**（赤より先）／ 検出線の行（`kind=detection`）が rc 2（測れなかった）→ **INCONCLUSIVE**（赤より先・理由に行番号と「検出線が測れなかった」・`s2-07l.331`）／ verify に rc≠0 が 1 本でも（検出線の rc 2 を除く）→ **FAIL** ／ **この 2 つの順は [gate-cost.md](./gate-cost.md) §28 が入れ替える**（行 t の着地の後の形: 赤が 1 行でも在る周は検出線が rc 2 でも FAIL・検出線の rc 2 が INCONCLUSIVE に倒すのは赤が 0 の周だけ）／ diff byte > cap → **INCONCLUSIVE** ／ lens が要るのに `--lens` 無し・lens rc≠0・stdout が parse 不能・verdict が 3 値外 → **INCONCLUSIVE** ／ それ以外は lens の verdict。
- **記録の追加**（[gate-cost.md](./gate-cost.md) §5.1・**契約 land 後**）: `verify.jsonl` の `kind=detection` の行だけ `line=<stdout の末尾の非空 1 行・逐語>` を持つ（検出線の値・parse しない・無い周は欠く）。
- 結果は `<state_dir>/pipe/<run>/verdict.json`（`{"schema":1,"run":…,"verdict":…,"evidence":…,"verify_red":<n>,"diff_bytes":<n>,"ts":…}`）と `RunStage stage=Gated detail=verdict:<V>`。stdout `run=<id> verdict=<V>`・rc は PASS=0 / FAIL=1 / INCONCLUSIVE=3。
- **[gate-cost.md](./gate-cost.md) §44 が上書き**（[ADR-0060](../../design-intent/decisions/ADR-0060-detection-line-runs-after-landing-not-in-the-gate.html)・同 doc の契約表の行 ak / al / am の land 後・本節の本文は書き換えない）: gate は ③（`detection-verify`）を撃たず、判定は ①②④ と lens だけで出る。検出線の rc 2 は判定に入らない。

### 5.4 land（(b)・FR10 / FR11 / FR12・N1）
- main 実測で木の hash が gate の木と一致する周は検出線を撃ち直さず、record は `verify-main.jsonl` に書く（[gate-cost.md](./gate-cost.md) §5・ADR-0021 §2.4・**契約 land 後**）。着地の順序の原則は同 §6。
`pipe land --run <id> [--lens <cmd>]`: 前提 = Gated ∧ verdict.json が PASS（それ以外 = rc 1・**何もしない**）。`git rev-parse refs/heads/main` が記録した `base` と違う周は **追随する**（`s2-07l.119`・FR30・並行に流した便の 2 本目が先着の後に置き去りになる形）: (i) `base` が main の祖先でなければ rc 1 `stale base`（main が巻き戻った / 分岐した＝追随の形が無い・何も書かない）(ii) worktree が clean でなければ rc 1（何も書かない・汚れた木では rebase を走らせない）(iii) worktree の branch を `git rebase <main>` する——効くのは **worktree の branch だけ**で main は 1 byte も動かさず、force 系は使わない（N1）。衝突は `git rebase --abort` で木を戻す。**ADR-0019 §2.2 の形**では `RunStage stage=Implemented detail=rebase-conflict:<base>..<main>` を記帳して runner を起こし直し、回数上限で `Failed detail=rebase-conflict`（[pipeline-conflict.md](./pipeline-conflict.md) §3・契約 (b) の land まで現物は `RunStage stage=Failed detail=rebase-conflict` + rc 1 の終端）(iii′) rebase が通って便の commit が 0 本になった周（同一変更の便が先に land した）は gate を撃ち直さず `RunStage stage=Failed detail=rebase-empty` + rc 1（便の変更は既に main に在る＝close してよい合図・lens を起動しない・main 不変・`s2-07l.125`）。commit 数を読めない周は 0 に読み替えず (iv) へ進む（fail-closed の向きを変えない） (iv) `RunStage stage=Implemented detail=rebase:<old>..<new>` を追記する（段が `Gated` から `Implemented` へ戻る 1 件＝撃ち直す便の記帳。`base` の読み手〔`base_of_run` の 1 本〕はこの行の新しい側を読む）→ stdout に `run=<id> rebase=<old>..<new>` (v) §5.3 の gate を**同じ関数で**撃ち直す（機械検証 + lens・diff が変わりうる）。PASS でなければ gate の判定行と rc で止まる（FAIL は `Gated` のまま land しない・INCONCLUSIVE は測り直せる側・lens は `--lens` で渡す）(vi) 撃ち直しの間に main がさらに動いた周は `RunStage stage=Gated detail=stale:<old>..<now>` を記帳して**同じ land の中で** (iii) から追随し直す。回数は `pipe.follow_retries` の 1 つの上限に衝突（`rebase-conflict:`）と合算で数え、上限で `Failed detail=rebase-conflict` rc 1（§18）。event 列が追随の回数をそのまま語るのは同じ。追随した周も以下の手順は同じ（CAS の old は新しい base）。
- 順序制御（[gate-cost.md](./gate-cost.md) §6・`s2-07l.147`）が在る周は (vi) は起きない（前提検査の直後・追随の前に着地待ちの列で自分の番を待ち、順番が来た便は撃ち直しの間も列の先頭に残るので他の便は待つ）。land の stdout と面 5 の行に `order=<first|waited:<秒>|degraded|unmeasured>` が載る。
1. `tree = git rev-parse <worktree HEAD>^{tree}` → `new = git commit-tree <tree> -p <old> -m "<message>"` → `git update-ref refs/heads/main <new> <old>`（CAS）→ `git rev-parse <new>^{tree} == tree`（lossless の実測）。message は **3 部**（`s2-07l.130`）: 件名の要旨 = goal の**先頭の文**（最初の改行または「。」の手前まで・前後の空白と markdown の見出し記号 `#` を除く）を **72 文字**（byte でなく char）で切ったもので、切った周だけ末尾に `…` を付ける（要旨が空なら件名は `<bead>` だけ＝land を止めない）／空行／本文 = goal 全文を**逐語**（改行を保つ）+ 空行 + `run: <run id>` の 1 行（trailer・読み手が fleet の記録へ辿る鍵）。件名は要約ゆえ中身が落ちるので、**落とさない側を同じ message の本文に必ず持つ**。`--pr-cmd` 形の message は forge が組む（この形は ref を動かさない）。
2. **anchor の同期**（`s2-07l.120`・N1・**手順 3 の実測の結果に依らず**行う＝ref は既に進んでいる。`s2-07l.131`: 同期は squash の直後・実測の**前**で、`git status` に staged の逆向きが見える窓を実測の長さから秒単位へ縮める。同期が `sync-failed` でも実測は続ける）: `--repo` の checkout の HEAD が `refs/heads/main` を指し、tracked な未 commit の変更が無く（見立ては **ref を進める前**に読む）、landed tree が足す path が anchor に無ければ `git read-tree -m -u <old> <new>` で index と working tree を新 main に揃える（`update-ref` は ref しか動かさず、揃えないと `git status` に landed 変更が staged の逆向きで残り次の `commit -a` が打ち消す・`.117` 実測。`reset --keep <new>` は ref が既に new を指すため working tree を更新しない＝採らない。`read-tree -m -u` は ignored な untracked file を黙って上書きするので足す path の衝突を先に見る）。dirty / 衝突 / 別 branch・detached / 読めない / git が途中で断った周は `anchor=skipped:<dirty|collision|not-main|unreadable|sync-failed>`（not-main 以外は stderr に warning 1 行・sync-failed は部分更新の可能性を名指す）。成立は `anchor=synced`。赤 / 測れない周は stderr に `pipe: anchor=…` を足す。`--pr-cmd` 形は ref を動かさないので token を持たない。
3. **main 実測**: `git worktree add --detach <tmp> <new>` して §5.3 の機械検証を**同じ順序・同じ関数**で再実行する（① write-set 照合 → ② run の写し `vessel.toml` の `common-verify`〔`{base}` 置換〕→ ③ 契約の `verify`）——2 本の実装に割ると gate が通した行と main で撃った行の意味が静かにずれる。**材料が揃わない周**（便の base を読めない / 写しを読めない）は赤ではなく `main-unmeasured` である。1 本でも rc≠0 → `RunStage stage=Failed detail=main-red` + rc 1（auto revert は MVP 外・main は進んだまま loud）。**実測そのものができなかった周**（tmp worktree を切れない等で verify を 1 行も撃てていない）は赤と別に `RunStage stage=Failed detail=main-unmeasured` + rc 2 で残す（「測れなかった」を「赤かった」に化けさせない＝gate の極性と同じ）。**段①（write-set 照合）を読めなかった周**（Step の cmd=`write-set` ∧ rc -1・gate と同じ 1 本の判定）も rc≠0 の集計より先に `main-unmeasured` へ倒す（gate §6 の INCONCLUSIVE と同じ極性・rc -1 だけでは見ない＝signal で死んだ verify 行は走った赤・`s2-07l.103`）。
4. 全 GREEN → **verdict export（面 5）**: `<state_dir>/fleet/verdicts.jsonl` へ `{"schema":1,"run":…,"bead":…,"sha":"<new>","verdict":"PASS","evidence":"<verdict.json の path>","ts":…}` を append（fleet と同じ lock）→ `RunDone stage=Landed`。便の規模の任意 field（`size` / `files` / `lines` / `pub_symbols`・`order` の後ろ・閾値なし）は [gate-cost.md](./gate-cost.md) §5.1（**契約 land 後**）。
5. **後始末は可逆 move**（N1.2）: `git worktree move <worktree> <repo>/.worktrees/<NAME>/retired/<run>`。branch は消さない（squash commit は branch の祖先でないので `branch -d` は通らず、`-D` は N1 に反する）。tmp worktree（main 実測用）は `git worktree remove --force` してよい（scribe2 が作った一時物で、成果は `new` に載っている。`--force` を許すのは `verify` の生成物で dirty になった一時 worktree を leak させないためで、**「force 系 git を書かない」の趣旨は履歴・データの破壊**＝この掃除はそれに当たらない）。失敗は stderr 1 行で rc 0 のまま（land は成立している）。stdout `run=<id> landed=<new> anchor=<synced|skipped:<reason>>`。
- `--pr-cmd <cmd>`（(e)・AC2 の自己ホスト形・**自 repo への PR の口**）: squash の代わりに branch を push して PR を作る seam。`{branch}` `{base}` を置換して `sh -c` する。**承認 event は前提でない**（[ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html)）＝この形は main を動かさず branch も PR も閉じられるので A4.3 で可逆であり、Ask-first の「出す」に当たらない。3 クラスの判定は契約の自己申告（`classes`）だけに効く。既定は無し（core は PR 作成の道具を知らない）。この形では main を動かさず `Landed detail=pr` で終える（merge は人が押す）。以下の 4 点はこの形にだけ効く（`s2-07l.24` の実装時の裁定・planner 2026-09-10）: **stale base を見ない**（ref を 1 本も動かさないので CAS の old が要らない。逆にここで base を縛ると main が動いた瞬間に PR を出せなくなり、自己ホストの便が最も踏む）／**面 5（`verdicts.jsonl`）へ書かない**（あれは main に載った便の記録で、この形はまだ載っていない）／**worktree を畳まない**（merge は人が押すまで終わっていない）／**道具の失敗（push・PR 作成の rc≠0）で便を終端させない**＝rc 1 で何も書かず段も動かさない（network で落ちうるので `Failed` を焼くと再試行できない便が残る）。
- `pipe retire --run <id>`（`s2-07l.46`）: 上の形が残した worktree を **merge の後に**畳む段。前提 = **`Landed` ∨ (`Failed` ∧ 最後の `RunStage` の detail が `rebase-empty`)**（ADR-0019 §2.4 で `rebase-conflict` と `Gated(FAIL)` を畳める側に足す・[pipeline-conflict.md](./pipeline-conflict.md) §5）∨ (`Reviewed` ∧ 審査の verdict が PASS でない〔`s2-07l.353`・判定を読めない周は断る・段は `Reviewed` のまま〕)∧ `.worktrees/<NAME>/<run>` が在る ∧ その worktree が clean（`git status --porcelain` が空白除去で空・**読めない周は偽**＝fail-closed。move は中身ごと運ぶので、未 commit の仕事を持った worktree を畳むとその仕事の行き先が便の外から読めなくなる）。通ったら手順 5 と**同じ 1 本の関数**で `git worktree move` し（`retired/<run>` へ・**削除しない**・**branch も消さない**・N1.2）、`RunStage stage=<その便の終端の段> detail=retired` を残して stdout `run=<id> retired=<path>` rc 0。**段は終端のまま**（`Landed` なら `Landed`・`Failed(rebase-empty)` なら `Failed`）で終端を動かさない＝**残す event の段を `Landed` に決め打ちしない**。前提違反は §4 の一般則どおり **rc 1 で何も書かない**（2 度目の retire は元の場所に worktree が無いのでここで止まる＝`retired/<run>/<run>` の入れ子が生まれない）。
  - **`rebase-empty` を畳める側に数える**（`s2-07l.128`）: (iii′) で終端した便は**変更が既に main に在る**（先着の同一変更が land 済み）＝成果の行き先が確定し、残っているのは入れ物だけで、`--pr-cmd` 形の `Landed` と同じ形である。他の `Failed`（`rebase-conflict`〔ADR-0019 §2.4 で畳める側へ移る〕/ `main-red` / `main-unmeasured` / `precheck:…`）は**人がまだ現物を読む**側なので断る（rc 1・worktree 不動・event 0 増）——理由を読めない周も断る（読めなかったを `rebase-empty` に読み替えない・fail-closed）。理由は同じ `Failed` 段の中で分かれる面ゆえ、**段の検査の一部として契約より前**に弁別する（そうしないと契約が壊れた便だけ「段違いなのに rc 2」になり §4 の一般則が rc の語彙ごと崩れる）。読むのは **追記だけの log の最後の `RunStage`** で、replay の `Run::detail`（最後に見た自由文）ではない——retire 自身が書く `detail=retired` が被さって理由が消える。move そのものの失敗だけは「対象が壊れている」ので **rc 2**（その stderr を出し、event は書かない＝畳めていないのに「畳んだ」を記帳しない）。**`detail=pr` を前提にしない**——squash 形で手順 5 の move だけが落ちた便（land は rc 0 のまま stderr 1 行で終わる）を後追いで畳む口にもなる。見るのは永続面の事実だけで、**merge 済みかは人が確かめる**（forge へ問い合わせない・器は PR の状態を知らない）。契約も読まない（畳むのは入れ物だけ＝契約が壊れた便の worktree が永久に畳めなくなる形を作らない）。`git worktree remove` / `rm` は足さない（N1）。
- **[gate-cost.md](./gate-cost.md) §44 が上書き**（[ADR-0060](../../design-intent/decisions/ADR-0060-detection-line-runs-after-landing-not-in-the-gate.html)・同 doc の契約表の行 ak / al / am の land 後・本節の本文は書き換えない）: 主実測は ③ を撃たず、land は `Landed` の event の後に着地後の検出（`pipe land --detection-only`）を子 process として切り離して起こし、終わりを待たずに終端へ進む。

### 5.5 承認（(c)・FR15 / FR16 / AC5・A1 / C7）
- `classes` が非空の契約は、**spawn の手前**（実行前・A1）で `ApprovalRequested detail=<classes>` + `RunStage stage=Blocked` を記帳し、**人の入力を待たずに rc 3 で process を終える**（FR15）。runner は起動しない。
- `pipe approve --run <id> --words "<user の逐語>"` → `ApprovalReceived actor=human detail=<逐語>`（C7.2）。逐語が空なら rc 1・記帳しない。**この subcommand は開発 session（R-C7-1 = user 直）が user の言葉をそのまま写して叩く**。会話の記憶を根拠にしない＝event に残った逐語だけが承認である。
- `pipe resume` は Blocked ∧ `approved` で spawn へ進む（FR16）。Blocked ∧ 未承認は rc 3。
- **質問（FR31 / FR32・[pipeline-question.md §5](./pipeline-question.md)）**: `pipe answer --run <id> --words "<回答の逐語>"` は **`Questioned` の run にだけ**受理し `QuestionAnswered detail=<逐語>` を 1 行 append する（actor は `default_actor` の `machine` のまま＝`Emit` に actor の seam を足さない・FR22 不変。逐語が空なら rc 1・段違いは rc 3・いずれも記帳しない）。`pipe resume` は Questioned ∧ 最新の質問への回答が在る周だけ同じ run を再 spawn する（無ければ rc 3・何も書かない）。
- **`approved` を立てるのは読み手側の資格検査である**: replay は `ApprovalReceived` ∧ `actor=human` ∧ 逐語が非空（trim 後）のときだけ `approved` を立てる。書き手（`pipe approve`）の逐語検査だけだと、`fleet record` で積んだ逐語 0 字の機械 event でも関門が開く。
- 3 クラスの判定面は**契約の自己申告**（`classes`）＋ **rules 行の deny list**（憲法 A4 機構欄）で、deny list は**未着**（manifest に該当行 0）＝MVP で実際に効くのは自己申告だけである（**seam の使用からの導出は [ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html) で廃止**した——`--pr-cmd` は自 repo への PR の口ゆえ承認 event を前提としない・§5.4）。操作の中身から 3 クラスを判定する enforcer は次の版（A4 の機構欄）。

### 5.6 stop（(a)・FR13・面 4）
`pipe stop --all`: replay で `SeatState::Live` な seat を列挙し pid へ `kill -TERM`（std::process で `kill`）→ `wait(Completion::SeatGone(pid), 猶予)` の猶予は rules 行 `pipe.stop_grace_ms` → 残れば `-KILL` → 各 seat に `SeatStopped`・run に `RunStopped stage=Stopped`。**rc = 0: 全部止まった / 対象なし（冪等）・1: 止められない seat が残った・2: state が読めない**。stdout `stop: seats=<N> stopped=<M>`。**予定形（ADR-0019 §2.1・契約 (a) の land まで現物は `--all` だけ）**: `pipe stop --run <id>`（[pipeline-conflict.md](./pipeline-conflict.md) §2）は終端でない run 1 本に `RunStopped` を書いて live から外す（席が Live なら先に止める・終端の run は event を増やさず rc 1）。
**errata（s2-07l.180）**: 席は process group 宛てに止める（spawn が先頭 process を group leader にし、`kill -TERM -- -<pid>` → `wait(Completion::GroupGone(pid))` → 残れば `-KILL`・group が無い旧 record の席と pid ≤ 1 は単一 pid の経路）・席を 1 つでも止め切れなかった周は `RunStopped` を書かない（rc 1・run は live のまま・`--all` も同じ）。

### 5.7 show / resume / run
- `pipe show --run <id>` → `run=<id> bead=<b> stage=<s> approved=<bool> worktree=<path>`（無ければ rc 1）。
- `pipe resume --run <id> [--runner] [--lens]`: 現在 stage から**続きの段だけ**を通す（Intake → spawn / Blocked+approved → spawn / Implemented → gate〔**予定形**（ADR-0019 §2.2・契約 (b) の land まで現物は gate だけ）: 最後の `RunStage` の detail が `rebase-conflict:` で runner が起きていなければ起こし直し〕/ Gated(PASS) → land〔base が動いていれば §5.4 の追随を同じ経路で通す＝`--lens` を渡す〕）。**Gated(INCONCLUSIVE) は land を試さず `run=<id> next=gate` を出して rc 3**——測れていない便に land の「PASS でない」を返すのは吸収状態の言い換えでしかなく、かといって**自動で測り直さない**（道具の不足は人が直す・`--lens` を渡してあっても撃たない）。Stopped / Failed は rc 1。**途中で process が死んだ便**（runner の process group が SIGKILL で落ちた周・席の落ち）も同じ口で続く（**予定形**（`s2-07l.203` の land まで現物は lock の mtime の線だけ））: 中断点は event log の最後の `RunStage` であり、殺された周が残す物のうち**所有者の死んだ lock file と受付の札**は resume の分岐ではなく資源の側で片付ける——lock は所有者の生死で外す（[fleet-event-log.md §4](./fleet-event-log.md)・warning は store の返り値まで＝pipe の面には出ない・配線は別便）、札は pid + 起動時刻で回収する（[ADR-0021 §2.3](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html#s2-3-slots)）。残る 3 つは片付ける機構を持たず**未決**として分ける: 書きかけの event 行は §4 の規則（malformed は全件 error）ゆえ resume が `Err` で止まる（規則の改訂は A2 / C5 の裁定を要する別 bead）／生き残った孫 process は器の機構でなく歯の側が片付け（`s2-07l.203` の acceptance 1・`reap_own`）／途中の worktree（untracked file・git の作業 lock）は `s2-07l.203` の候補 2 で弁別する（同便では直さない）。resume に「殺された周」の分岐を作らない（段の関数は 1 つ・C2）。
- `pipe run --contract <f> --bead <id> --repo <dir> --runner <cmd> [--lens <cmd>]` = intake → spawn → gate → land を 1 process で連続（各段は fleet を読み書きし、途中で落ちても `resume` が続きを引く。先頭行は intake と同じ 1 行＝`--rules` の周は `ceiling-overridden=` を後置する）。spawn が質問で止まった周は判定行に `question=<id>` が載って rc 3 で止まる（席の中継の入力・[pipeline-question.md §6](./pipeline-question.md)）。

### 5.8 report（(e)・FR22）
`pipe report`: event log を replay し `runs=<N> landed=<N> human_events=<N> human_events_other_than_approval=<N>` の 1 行。到達点の「人由来の event が approval 以外に 0 件」を機械で示す面（AC1）。`landed` は **終端（`Landed`）まで通った便の数**で「main に載った数」ではない（`--pr-cmd` の便は main を動かさず終端に達する。main へ載った数は面 5 の行数で読む）。**便の数と land の数は replay から、人由来の event は生の行から**数える——replay は便ごとに最後の段しか残さないので、承認の後に手で段を動かした周が replay 上は「機械だけで進んだ便」に見える。承認だけを例外にする判定は **kind**（`ApprovalReceived`）で行う（`actor` は誰が起こしたか・`kind` は何が起きたかで、例外は後者である）。

### 5.9 `pipe/cli/` の置き場（`s2-07l.349`）
subcommand と helper は責務ごとに 1 file に置く——入口（usage / dispatch / contracts）は `cli.rs`、引数と rules 行の helper は `cli/args.rs`、便の状態の helper は `cli/state.rs`、`show` は `cli/show.rs`、`resume` は `cli/resume.rs`。`cli.rs` は `mod` 宣言と再輸出の shim だけを持ち、`intake.rs` / `run.rs` / `step.rs` の `use super::…` は shim で解く（既存の subcommand の file を触らずに割る）。材料の型（`Resolved` / `Extra`）は `cli.rs` に残す（子 module が private field を読む＝型を動かすと field の可視性が変わり純移動でなくなる）。usage の字面と歯の本数は移動の前後で変えない。歯の側（旧 lifecycle.rs → `tests/e2e/pipe/ratelimit.rs` / `tests/e2e/pipe/stop.rs`）も同じ型で、`tests/e2e/pipe/spawn.rs` / `tests/e2e/pipe/land.rs` が `super::lifecycle::` で引く口座の fixture は親 `tests/e2e/pipe.rs` の `use ratelimit as lifecycle;` の別名で解く（呼び手を触らない）。

## 6. headless runner と lens（(d)・FR5・CON6・NFR1）
- runner / lens の子 process の封じ込め（cgroup scope）は [gate-cost.md](./gate-cost.md) §4（ADR-0021 §2.2）。runner / lens / claude の包みの上限は `1 × gate.job_memory_mb`（gate-cost.md §12・契約表の行 c・裁定 id user 2026-09-15T18:2xZ）。

- **器が起こす claude は settings を 1 つも読まない**（[ADR-0011 §2.1](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-1-launch-form)）: claude の Command を組む `build` が `--setting-sources ""`（空＝user / project / local のどれも読まない）と `--strict-mcp-config` を**毎回**渡す（runner・lens 共通の唯一の構築点に置き、呼出側には置かない＝足し忘れた側が既定の全 source へ落ちる形を作らない）。`--settings`（追加読込・settings を消さない）と `--restricted`（Bash を外し人の承認へ倒す）は渡さない。checkout の `.claude/settings.json` の allow 規則は承認要求を出さず PermissionRequest hook を素通りし、`hooks` は checkout が process を起動する口になるので、**入力の段で断つ**（[ADR-0011 §1 / §2.3](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-3-no-fallback)・intake の字面検査で代替しない）。
- **model も毎回明示する**（`s2-07l.297`・user 裁定 2026-09-14T21:59Z）: 同じ `build` が `--model <値>` を runner・lens 共通に渡す（`--permission-mode` と同じ理由＝版の既定に従うと便が消費するモデル別窓が黙って変わり、便用の口座選定（[account-autonomy.md](./account-autonomy.md) §3）が数える窓とずれる）。値は rules 行 `runner.model`（lens と先撃ちの lens の行は §61・`--rules` / 埋め込み・lens の cap と同じ読み口・runner にも `--rules` の seam を足す。写しの allowlist と common-verify は従来どおり写しから＝[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers) は動かない）。行が無い / 不発効 / 文字列でない周は claude を呼ばず rc 2（cap と同じ極性）。実測（2026-09-14・4 便の transcript）: `--model` 無しの runner / lens は `claude-opus-5` で走っていた＝行の初期値はその実測値で、挙動は変えずに選定の窓だけを合わせる。
- **effort も毎回明示する**（`s2-07l.322`・user 裁定 2026-09-15T03:52Z）: 同じ `build` が `--model` の直後に `--effort <値>` を runner・lens 共通に渡す。値は rules 行 `runner.effort`（model と同じ読み口・同じ manifest・閉じた表 `Effort`〔`low` / `medium` / `high` / `xhigh`〕との完全一致）。渡さないと effort は起動口座の設定 dir の `effortLevel` で決まり口座ごとにばらつく（席の model の事故〔[account-lifecycle.md](./account-lifecycle.md) §4・`s2-07l.313`〕と同じ根因）。行が無い / 不発効 / 文字列でない / 表に無い周は claude を呼ばず rc 2（model と同じ極性・cap → model → effort の順に先に落ちた理由 1 つだけを出す）。
- **実測**（claude 2.1.267・2026-09-10・`s2-07l.64`）: checkout の `.claude/settings.json` に SessionStart hook（marker を touch する 1 行）を置いた dir で runner と lens を**実 claude**・空の口座 dir・無効な API key（401 authentication_failed で止まる＝`total_cost_usd` 0・課金なし）で 1 回ずつ撃つと、**marker は両方とも生成されず**、runner の stream の `hook_started` は **1 件**（`--plugin-dir` で載せた器の plugin の hook だけ）だった。対照として `--setting-sources` を `project` に戻した build では **marker が生成され**、`hook_started` は **2 件**（checkout の hook が 1 件増える）。**settings を切っても器の hook は載る**（[ADR-0011 §5](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s5-trace) の限界）。
- `<NAME> runner --worktree <dir> --write-set <f> --vessel <f> --plugin-dir <dir> --permission-mode <mode> [--rules PATH] [--account-dir <dir>] [--claude <path>]`（`--rules` は model の行だけを読む・上の bullet）: **契約本文は stdin で受け**（FR5「stdin に契約」）、write-set と合わせて prompt に組み、`claude -p` を **cwd = worktree・`--allowedTools`（**`--vessel` は必須**〔無ければ claude を呼ばず rc 1・lens の `--contract` と同じ極性〕で、その写しの `allowed-commands` の各要素を `Bash(<cmd>:*)` の形で・runner は manifest を読まない・[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)・[ADR-0009 §2.1](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-1-runner-permissions)＝起動口座の settings を継承せず器が権限を与える・明示 allow の外は PermissionRequest hook が deny。checkout の settings も読まないことは上の bullet・[ADR-0011 §2.2](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-2-supersede)）・`--output-format stream-json --verbose`（`-p` との併用では claude が `--verbose` を要求する）・`--permission-mode` を毎回明示・`--plugin-dir` で **plugin root**（§5.2 手順 5・`{plugin_dir}`・器の plugin + consumer の plugin）を載せて** 起動する。prompt の穴は **3 つ**（`{contract}` / `{write_set}` / `{allowed}`）で、**1 走査**で埋める（重ねて replace すると契約本文や write-set の中の `{allowed}` が次の走査で展開され、実装役が自分の権限一覧を自分で書き換えられる）。`{allowed}` の値は**便の写し**の `allowed-commands` を 1 行 1 command で並べた人が読む面で、`--allowedTools` の `Bash(<cmd>:*)` 形とは別に組む（値の出所は写しのまま・[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)）。prompt は「一覧の外は承認要求で止まり出力が返らない」「pipe / redirect / `&&` / `;` で繋がず **1 command** で撃つ」「出力を絞るときは pipe でなく command 自身の flag」を運ぶ（`s2-07l.67`）。**prompt は argv でなく claude の stdin で渡す**——argv だと Linux の 1 引数上限（128KiB）に当たり、user が裁定した `gate.token_cap = 150000` が実質 130KB へ切り下がる。stdin で渡す以上 stream には prompt が 1 行も出ないので、runner は**組んだ prompt を claude を起こす前に写しの隣 `<state_dir>/pipe/<run>/prompt.txt` へ残す**（`s2-07l.79`）——「何を渡したか」を後から読む運用の証跡で、置き場は `--vessel` の写しから解く（新しい flag も env も足さない・C2.2）。tracked な面（worktree）には置かない（契約本文と write-set が入る・PUBLIC）——置き場が解けない写し（裸の `vessel.toml`・`Path::parent` が空を返す形）では cwd〔= 便の worktree〕へ落とす代わりに**残さない**側へ倒す（lens 2026-09-11 M2 の実測: 素朴な join は cwd へ落ちた）。**証跡は判定の入力ではない**: 残せない周は stderr に 1 行を出すだけで便は続き、rc は claude のものを写す。**口座は子 process の環境変数（設定 dir）で切り替える**（FR5「口座は環境変数で切替」）。scribe2 自身は env を読まない（C2.2）＝子へ設定するのは「読む」ではない。**`--account-dir` を渡さない周は親の環境変数がそのまま子へ継承される**（消す形は、この env で口座を切っている環境で子を黙って既定口座へ落とす実害があり、消すこと自体も env への介入になるため採らない。C2.2 が禁じるのは「読むこと」と「新しい seam を導入すること」で、継承はそのどちらでもない）。stream-json の中で **`rate_limit_event` 種別の record だけ**を上限の判定に使い、その `rate_limit_info.status` を入力にする（[ADR-0012 §2.1](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html)）。**上限を表す status の閉じた集合**を持ち、集合に属する周だけ rc 75 で止める（呼出側は `Failed detail=rate-limit`）。**集合に無い status では止めない**（未知を含む）——rc 75 は判定の名札ではなく**実行の中断**（runner は待たずに kill する）で、取りこぼしは「分類が付かない」だけだが誤検出は**健全な便を殺す**からである。**限界（主張と同じ場所に置く）**: 集合には**実測で採れた値だけ**を入れるところ、止まる周の status は未採取なので**集合は空**であり、**器は当面 rc 75 を一度も立てない**（真に上限へ当たった周も claude の rc による失敗として残る）。**観測した status は記録面（runner の 1 行）へ載せる**——集合を実測で育てる唯一の口である。ただし種別の判定は**行頭の形と字面**に依るため、**別の record が上限 record を入れ子で引用した周は status を読みうる**（集合が空の現在は止まらないが、**記録の口には載る**＝集合を育てるときの入力が汚れる）。top-level の種別を読む実装は [ADR-0012 §2.2](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html) が撤去を命じたものなので、塞ぐなら**集合へ値を入れる便で決める**（そのときは現物が要る）。`utilization` は判定に使わない（閾値で自主的に止めない・憲法 C9.2）。本文への**字面照合は撤去した**（`s2-07l.77`）——語彙表と本文 field の走査は、識別子の 16 進や tool の出力で 2 度誤爆し、狭めても `system` 種別の誤爆が残った。上限の真の合図が構造で来ると現物で分かった以上、字面を併用する理由が無い（ADR-0012 §2.2）。rc は claude の rc を写す。**agent view は常に切る**: `build`（runner と lens の唯一の構築点）は子の env に `CLAUDE_CODE_DISABLE_AGENT_VIEW=1` を設定する（[account-autonomy.md](./account-autonomy.md) §5「agent view の前提」・台帳 `s2-07l.239`・`CLAUDE_CONFIG_DIR` と同じく子へ設定するだけで器は env を読まない・C2.2）。
- **`--plugin-dir <dir>` は plugin の root**（§5.2 手順 5 の `plugin/`）: runner は root の配下の dir を名前順に 1 つずつ claude の `--plugin-dir` に渡す（配下 0 なら渡さない・root 直下の file と symlink は無視）。**root が読めない、または配下に plugin の dir が 0 の周は claude を起こさず rc 2**（`RC_BROKEN` の極性・手順 5 が器の plugin を必ず書くので配下 0 は root が壊れた印＝guard 0 本で claude を起こさない）。器側で展開する理由: claude の `--plugin-dir` は folder を渡すと各 child を読む版が在るが、読めない周・0 件の周の極性と読み込み順を器が握り、版依存の挙動に寄せないため。器の plugin は root に必ず在るので、consumer の repo が plugin を持たなくても in-loop guard が便に載る（`s2-07l.149`）。hook の発火は hook command の binary 解決（`${…_BIN}` か PATH の `<NAME>`・[vessel-hook.md](./vessel-hook.md) の hooks.json の項）に依る。
- **実測**（claude 2.1.267・2026-09-11・`s2-07l.67`・**器が初めて cargo を回した周**）: toy repo（cargo crate 1 本）へ 契約を流し、**subscription の口座 dir**（`--account-dir`・API key は子 env へ渡さない）で実 run を 1 本。**Bash 呼出 5 件 / うち cargo 2 件 / 「This command requires approval」0 件**（**ただし deny は 1 件**——`git commit -m "$(printf …)"` が claude 自身の静的解析で `Contains shell syntax (string) that cannot be statically analyzed` として止まり、実装役が単純な形へ書き直して成功した＝**器の allowlist による deny ではない**。この形は prompt の禁止列挙に無かったので本便で足した）で、transcript に cargo 自身の出力（`Finished \`test\` profile` / `test result: ok. 1 passed`）が残り、実装役は commit まで到達した（過去 4 run は cargo 呼出 9〜12 件が**全件 approval 待ち・cargo 出力 0 件**）。副産物: 同じ stream に **`{"type":"rate_limit_event","rate_limit_info":{"status":…,"utilization":…}}`** が流れており、上限の真の signal は**専用の record 種別と構造化された status** で来る（字面照合ではない）＝`s2-07l.77` の証拠。**限界（当時）**: 本周は `--plugin-dir` に §5.2 の写しでなく repo 本体を渡した probe で、pipeline 経由の起動形は通っておらず、prompt も stream に出なかった——どちらも次の bullet（`s2-07l.79`）で畳んだ。`{allowed}` の寄与と `s2-07l.64` の効果の分離は歯が担い、実 run では次の bullet の 3 便目が `{allowed}` の一覧を実装役が読んで従った現物になった（A/B の対照ではない）。
- **実測**（claude 2.1.268・2026-09-11・`s2-07l.79`・**pipeline 経由で器が cargo を回した周**）: `pipe intake` → `pipe resume`（§5.2 の spawn・runner cmd の `--plugin-dir` は `{plugin_dir}` = 手順 5 の写し）で、plugin dir を tree に持つ toy repo（cargo crate 1 本）へ **3 便**、subscription の口座 dir（`--account-dir`）で撃った。fleet の event は 3 便とも `RunCreated(Intake)` → `RunStage(Spawned)` → `SeatSpawned` → `SeatStopped` → `RunStage(Implemented)`。claude の `init` record の `plugins[].path` が **run dir 配下の写し**（source `scribe2@inline`）を指し、写した `hooks.json` は repo のものとバイト同一で、その file の SessionStart hook が `hook_response exit_code=0` で走った（＝写しの hooks.json が載った**直接**の証拠）。Edit が write-set 内の file に通って commit まで到達した（repo 本体を渡した `s2-07l.39` の Failed は写しでは再現しない）。**Bash 4 件（cargo 1 / git 3）・5 件（cargo 1 / git 4）・5 件（cargo 1 / git 4）／「This command requires approval」0 件・0 件・0 件／deny 0 件・0 件・0 件**（母集団は各便の Bash 全件）で、transcript に cargo 自身の出力（`Finished \`test\` profile` / `test result: ok. 2 passed`）が残り、3 便とも `stage=Implemented`（commit 1 本ずつ）。**3 便目は負例を仕込んだ**（goal に write-set 外の `NOTES.md` の新規作成と allowlist 外の `ls` を要求）——実装役は prompt の規律を引いて両方を**自分で退け**（「write-set の外は触らない」「`ls` は allow の外なので `git ls-files` で代える」と最終応答に明記）、`src/lib.rs` だけを編集して commit した＝`{allowed}` の一覧と write-set が実 run で読まれ従われた現物。上限 record: 2 便目・3 便目の raw stream（`--claude` seam の tee）に `rate_limit_event` が各 1 件（status `allowed_warning`・`seven_day`・utilization 0.28 / 0.29）＝集合が空なので止まらず runner の記録行に載った（1 便目は raw stream 未採取で、記録行の status `allowed_warning` だけが現物・`s2-07l.77` の証拠採取と同じ・上限には当たっていない）。prompt は 3 便とも `prompt.txt` に落ち、1 便目で口座側 transcript の最初の user message と**同一**（1101 字）と実測した（mtime も最初の user message より前）＝「何を渡したか」が stream 抜きで読める。**残る限界**: PreToolUse / PermissionRequest hook は**一度も発火していない**（negative を仕込んだ 3 便目でも実装役が prompt の段で自制したため、guard に届いた write-set 外の Edit / allowlist 外の Bash は 0 件）＝写しの hook が deny を返すことの実 run での実証は無く、stream-json にも hook record は SessionStart 以外出ない。gate / land はこの 3 便では撃っていない（spawn 段までの実証・AC1 / AC2 の完走は別便）。
- **包みの質問 record**（FR31・[pipeline-question.md §3](./pipeline-question.md)・`s2-07l.115`）: claude が**正常に終わった後**（rc 0）、stream の最終 `result` record の text の**最後の `{` で始まる行**（lens の verdict と同じ読み方＝後続の散文は無視する）が `question` key を持つ JSON 1 つ（`question` 必須非空 1 行・`about` 任意）なら、同じ record を自分の stdout の**最終行にそのまま**写し（観測行 `runner: rc=… records=…` はその前）、rc `RC_QUESTION`（76・`pipe` の定数）で終える。record が無い周は claude の rc を写し、JSON らしい最終行が読めない・`question` が空・文字列でない・複数行の周も claude の rc を写して stderr に理由 1 行（未知は claude の rc へ・FailOpen・極性一覧 `runner-question`）。claude が非 0 で終わった周は最終行を読まない。prompt template は「質問は record で・commit を作らない・それ以外の形で人へ問わない」「契約末尾の『## 回答』節は前の質問への回答」を運ぶ。pipeline 経由では契約本文（+ 回答節）を `pipe` が stdin へ流す（seam に `< {contract}` を書かない・§5.2）。
- `<NAME> lens --contract <f> --worktree <dir> --permission-mode <mode> [--rules PATH] [--account-dir] [--claude <path>]`: **`--contract` と `--worktree` は必須**で、無ければ claude を呼ばずに rc 1（前提違反）・読めない契約は rc 2。起動形は runner と同じ `build` を通る＝user / project / local の settings を読まない（上の bullet・[ADR-0011 §2.1](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-1-launch-form)）。lens は `--allowedTools` も `--plugin-dir` も渡さない（権限の出所は `--permission-mode` と headless の既定だけ・[ADR-0011 §2.2](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-2-supersede)）。**cap は rules 行 `gate.token_cap`（埋め込み / `--rules`）から読む＝値の出所は manifest 1 つ・launcher は数を書かない**（`s2-07l.272`・憲法 C1・FR17。以前の argv `--cap <bytes>` は撤去し、渡された周は未知の引数として rc 1 で断る＝手書きの数が黙って効き続ける経路を構造で塞ぐ。行が無い / 不発効 / 整数でない周は claude を呼ばず rc 2）。stdin の diff が cap を超えたら **claude を呼ばずに** `{"verdict":"INCONCLUSIVE","evidence":"diff exceeds cap"}`。それ以外は診断 prompt（契約の goal / done / verify 各行 / write-set 各行を `{contract}` 穴へ差し込み、diff と併せて PASS / FAIL / INCONCLUSIVE を JSON 1 行で返せ。**契約 file を丸写ししない**——owner や disposition は判定の材料にならず、渡すほど cap を食う。穴は `{contract}` と `{diff}` を **1 走査**で埋める＝契約本文の中の `{diff}` が展開されない）で `claude -p` を **`--output-format` を渡さず既定（text）で・prompt は stdin で**呼び、出力の最後の JSON 行を stdout 1 行に写す（stream-json にすると全行が JSON になり、最後の JSON 行は claude 自身の result record になって判定が取れない）。parse 不能は INCONCLUSIVE。
- 両 wrapper は `pipe` の seam にそのまま渡せる 1 行（例: `--runner "<NAME> runner --worktree {worktree} --write-set {write_set} --vessel {vessel} --plugin-dir {plugin_dir} --permission-mode acceptEdits"`＝**`< {contract}` は書かない**: 契約本文は §5.2 手順 6 のとおり `pipe` が runner の stdin へ流す〔再 spawn では「## 回答」節付き〕。shell の redirect は piped stdin を上書きするので、seam に書くと回答節が包みへ届かない〔`s2-07l.114` lens〕。包みを単体で叩くときだけ `< contract` を使う）。**`--plugin-dir` には repo でなく `{plugin_dir}`（§5.2 の写し）を渡す**——Claude Code は読み込んだ plugin dir 配下の file を acceptEdits の自動承認から外す（sensitive）ので、repo を渡すと便の worktree（`<repo>/.worktrees/<NAME>/<run>`）はその内側になり、runner は write-set 内の 1 file も Edit / Write できない（実測 2026-09-10・`s2-07l.39` の Failed）。
- **lens の verdict は findings の閉じた category と母集団を必須 key に持つ**（`s2-07l.188`・§17・C10 / C11.2）: 出力の JSON 1 行は `verdict` / `evidence` に加えて `findings`（閉じた 8 観点〔contract-fit / teeth-nonvacuous / constitution / delete / stdlib / native / yagni / shrink〕を**宣言順で全部**・`<category>:<件数>` を `,` で並べ、**0 件の観点も 0 と書く**）と `population`（`files:<n>,lines:<n>`＝lens が読んだ母集団）を持つ。どちらかが欠けた周・表に無い名・件数が数でない周・母集団が 0 の周は gate が **INCONCLUSIVE** へ倒す——件数の無い判定は「見て 0 件だった」と「見ていない」を弁別できず、母集団 0 の PASS は「見ていない」が「穴なし」に化けた形だからである（既存の INCONCLUSIVE 経路なので便は終端せず測り直せる）。`verdict.json` に同じ 2 field が**宣言順に正規化された字面**で載り（読めた周だけ＝field の無い verdict は「測っていない」と読める）、`RunStage stage=Gated` の detail は `verdict:<V>` のまま**変えない**。判断（この抽象は要るか）は lens の領分で、器が持つのは型と件数と母集団だけである。
- `--claude <path>` は test の seam（fake の実行 file が引数と stdin を file に写す）。prompt の文面は tracked な template file（`crates/<NAME>/src/headless/*.txt`）で持ち、絶対 path・口座名を含めない。
- **prompt 本文の外形**（`s2-07l.176`）: runner と lens の prompt は fixture の契約 / write-set / diff で組んだ**全文**を外形 snapshot で pin する（`headless_runner_prompt_external_form` / `headless_lens_prompt_external_form`・lens-contract と同じ型）。本文の 1 字の変更は `.snap` の差分として PR に現れ、review の入口になる（C12.5）。

## 7. FR7（入口の flip check）の置き場

本 repo 自身の flip check は `cargo xtask flip-check` と CI の job が担う。**CI の flip-check job は `s2-07l.17` で land 済み**（CI は nextest / clippy / xtask-check / flip-check / deny / insta の 6 job・flip-check は PR のときだけ撃つ）。pipeline は **vessel 宣言 `common-verify`** の 1 行として flip check を撃つ（Rust repo の行は `cargo xtask flip-check --base {base}`・契約にも manifest にも書かない・[ADR-0010 §2.1](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-1-declaration-file)・[ADR-0009 §2.4](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-4-common-verify)）。pipeline が Rust 固有の検査を内蔵する形は採らない（toy repo は Rust でないことがある）。**非空虚性（変異検出線・C12 R-C12-1）も同じ置き場**: `cargo xtask mutants-diff --base {base}` が `cargo mutants --in-diff` を便の diff に当て `total / caught / missed / unviable / timeout / scope` の 1 行を出す。`scope` は `-p` へ**実際に渡した** package 名で、現状は **core package 固定**＝xtask 側の diff は母集団に入らない（`s2-07l.82`・行は出所から切り離されて流通するので限界は報告でなく行に載せる。diff が触った package を並べて測る形は費用を測ってから別便）。rc は **3 値**である: (i) `outcomes.json` が在る → manifest の `R-C12-1` 行の極性（`enabled=false` = 記録のみ・`enabled=true` = missed>0 で rc≠0）／(ii) 無い ∧ cargo-mutants が rc 0 → `total=0` を含む 1 行で **rc 0**（**測る対象が無い**＝core を触らない便を恒久 FAIL にしない・母集団を額面に出すので「0 件の緑」と読み違えない）／(iii) 無い ∧ cargo-mutants が非 0、または**道具の不在** → **rc 2**（測れなかったを 0 に化けさせない）。**道具の rc は捨てない**——baseline（変異を当てない木）の test が落ちた周も cargo-mutants は `outcomes.json` を書く（`total_mutants=0`）ので、rc を見ないと「suite が壊れているときほど門が緑」になる。非 0 の理由が件数から説明できる周（生存・時間切れが在る）だけを測定として受ける。**前回の出力 dir は撃つ前に掃除する**（変異 0 の周は cargo-mutants が dir へ触らないので、掃除しないと前便の `total=18 missed=6` が今便の測定を名乗る・実測 2026-09-11）。**置き場は本 repo の `.vessel.toml` の `common-verify` の末尾**（`s2-07l.58` で land・先頭語 `cargo` は上限と宣言の allowlist の内）。**歯は cargo-mutants 本体を起動しない**——fixture（`outcomes.json` の 5 種と、道具の rc の 2 値）で 1 行の形と rc の 3 値だけを測る（CI に 10 分の実行を持ち込まない）。契約に変異 script・変異 anchor を書かない（[ADR-0009 §2.3](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-3-mutation-proof)）。`--base` を取る xtask の口は flip-check / mutants-diff / rules-diff / deps-delta の 4 本で、いずれも CI の flip-check job（PR のときだけ）が同じ `base.sha` で撃つ。deps-delta の rc の 2 面（deny の面だけが rc・check-delta-ms は `-` で残す検出線）は [rules-manifest.md §4](./rules-manifest.md)。

- **判定クラス**（語彙の SSOT は `cargo xtask flip-check` の判定行そのもの＝`judge` が stdout へ出す 1 行。本節は意味だけを持ち、README は本節への pointer だけを持つ・ADR-0013 §2.1・`s2-07l.92`。判定行の書式は ADR-0013 §2.4 のとおり pin されていないので、ここが実装と食い違ったら実装が正）。判定行は 2 形: `RED-on-base ok tests_changed=N`（rc 0・免除・同梱・base 段の撃ち直し・docs-only が在るときだけ `removed-only=N` / `retroactive=N` / `moved=N` / `decl=N` / `fixture=N` / `base-retried=N` / `docs-only=N` を後置。flip が 0 本でも免除だけの便は `tests_changed=0` のこの形で通る。`docs-only=N` は `.rs` が 1 本も無く、動いた path が全部 rules 行 `flip.docs_only_faces` の面の中の便で、N は動いた path の本数・`s2-07l.170` で `skip reason=no-rust-diff` の形を置き換えた）／`FAIL reason=<理由>`（rc 1）。FAIL の理由は 6 語: `green-on-base`（overlay した歯が base で緑＝TDD の不履行。1 本ずつ撃つ周〔flip が 2 本以上、または宣言 file を同梱した周〕は `file=<rel>` で緑だった file を名指す）／`no-test-diff`（`.rs` は変わったが test 区間の差が 1 本も無く、免除の札も無い。`.rs` が 1 本も無く docs-only の面の外の path を含む便も同じ語で、stderr に `outside-docs-faces <rel>` を 1 行ずつ残す）／`bad-marker file=<rel>`（その便で足した札の bead id が閉じた形 `<接頭辞>-<段>(.<段>)*` に合わない）／`too-many-marks marks=N limit=M`（その便で足した札の本数が rules 行 `flip.marks_per_pr` を超える）／`not-flippable`（下）／`infra-error <理由>`（道具の失敗＝git / tar / cargo の spawn 失敗・base 自身の test が緑でない `base-not-green`・base で該当 test が 0 本の `no-tests-on-base`・runner が signal で死んだ `runner-killed-by-signal`。**測れなかった**であって赤ではない）。**base 段の撃ち直し**（`s2-07l.270`・負荷下の flaky の検出線）: base の素の runner が落ち、落ちた歯を runner の出力の `FAIL` 行（binary id と歯の名）から名指せる周は、**その歯だけ**を同じ base copy で **1 回だけ**撃ち直し、通れば base 緑と読んで判定行に `base-retried=N`（N = 撃ち直した歯の本数）を後置する。2 回目も落ちる・落ちた歯を 1 本も名指せない（compile error・signal・出力の形が読めない）・撃ち直しの rc が 0 でない周は従来どおり `base-not-green`（撃ち直しは緩める側なので狭く取る＝名指せない失敗を撃ち直しで緑に化けさせない）。stderr に `base-retry <binary>::<name>` を 1 行ずつ残す。**base の実体化**（`s2-07l.280`）= `git archive` の展開 + index（`git init` / `git add -A`）+ HEAD（共有 object store を alternates で読み、base の commit を `update-ref HEAD` で置く）＝base の tracked 集合を `git ls-files` で、宣言を `HEAD:<file>` で読める git repo（`contracts check` 等 tracked 集合と HEAD を読む歯が base で測れる・commit は作らない＝HEAD は base の sha そのもの・overlay は working tree にだけ書き index にも HEAD にも載せない）。引数の不正（`--base` の不在・空）だけは判定行を出さず rc 2（直上の mutants-diff の rc 2「測れなかった」とは別の意味）。stderr の行は判定ではなく、判定行を読む人のための診断である。
- **測れなかった便は「測れなかった」と言う**（`s2-07l.14`・reason 語彙を 3 つ足した）。いずれも fail-closed のままで、`skip` で rc 0 にする経路は持たない。
  - `not-flippable`: base に無い `.rs`（新規 module）の in-file 歯は、base 側に `mod` 宣言ごと存在せず compile されないので**構造的に測れない**。flip が 1 本も無く、そういう file が 1 本以上在る周は runner を撃たず `FAIL reason=not-flippable files=<rel,…>` rc 1（stderr に逃がし方 1 行）。`green-on-base`（TDD の不履行）と同じ札を貼らない。
  - `tests-removed-only`: overlay できる file で **HEAD の test 区間の行列が base の行列の部分列**（順序を保った行の削除だけで得られる）なら flip に数えず、stderr へ `not-flipped reason=tests-removed-only <rel>`。純粋な module 分割（歯の移動）が恒久 FAIL しないための門である。**`crates/*/src/**/*_tests.rs`（と tests という名の file）は名前で test file と見なし全体を写す**——`#[path]` で src 配下へ外出しした test module は `#[cfg(test)] mod` の形を持たず、名前で見なければ区間判定には src 区間だけの file に見え、そこへ足した歯が 1 本も測られない。**`#[test]` fn 名では数えない**——名前の集合で見ると本文の改変が免除される（`⊆` は「名前が同じで本文だけ変えた歯」を、真部分集合でも「1 本消して別の 1 本の本文を変えた file」を通す）。部分列なら 1 行でも足された / 書き換えられた時点で成立しない。
  - **宣言 file の同梱**（`s2-07l.41`）: flip した file のうち、test 区間の差分行が**すべて** `mod x;` 形（`pub` / `pub(crate)` 可）の file は「宣言 file」と呼び、**単独では撃たず**本体 file を撃つ木へ同梱する（判定行に `decl=N`・stderr に `decl-with-body <rel>`）。新規 module は宣言と本体が別 file に割れ、単独 overlay ではどちらの判定も意味を持たない（宣言だけ = 本体不在の `E0583` の偽 RED／本体だけ = base に宣言が無く compile 対象外の偽 GREEN・実測 2026-09-10 `s2-07l.38.2` が初発）。弁別は**差分行の字面だけ**で行い parser は足さない＝`mod` 以外の行が 1 行でも動いていれば宣言 file ではない（同梱は判定を緩める側なので狭く取る）。**宣言 file しか flip していない便は従来どおり単独で撃つ**（存在しない module を指す `E0583` は本当の RED である）。同梱は**本体 1 本を撃つ turn ごと**に、**その便が足した** `mod <name>;` 行のうちその時点の tree に本体が無いもの（同じ dir の `<name>.rs` か `<name>/mod.rs` で見る）を落として置く。`mod` 行**以外は 1 行も触らず**、**base に既に在った宣言行も落とさない**——base が緑である以上その本体は必ず在り、落とすと `#[path = "…"]` の属性行だけが孤児になって`expected item after attributes` の compile error＝**別の捏造 RED**を作る（lens-44 H1）。`#[path]` 付き module を**救う**わけではない（その便が足した `#[path]` 宣言は従来どおり測れない・M4）——便の宣言行を全部置くと、その turn ではまだ置かれていない兄弟 module の `E0583` が RED に化け、**本体がどちらも base で緑でも隠れる**（新規 module 2 本以上の便の fail-open・実測 2026-09-10・`s2-07l.44`）。絞ったうえで本体の歯が base で緑なら `green-on-base` のまま落ちる＝同梱は RED を捏造しない。
  - **歯の外の file の同梱**（`s2-07l.450`・§37）: flip した file のうち、その便で動いた行が 1 本も歯の中に無い file（fixture だけの差）は宣言 file と同じ側＝単独では撃たず本体を撃つ木へ同梱する（判定行に `fixture=N`・stderr に `not-flipped reason=outside-teeth <rel>`）。歯の中の行が動いた file と、同梱しか flip の無い便は従来どおり落ちる。
  - `moved`（`s2-07l.86`）: **歯を 1 本も足さず挙動も変えない純粋な移動**の便は、test 区間へ `// flip-check: moved <bead-id>` を 1 行置くと RED を要求されず、判定行に `moved=N` が載る。**`tests-removed-only` との弁別は「自動か明示か」**——あちらは test 区間の差が**削除だけ**（部分列）のとき機械が自動で通す門で、こちらは差が削除にならない便（歯が `check()` 越しの統合形で書かれていて、実装だけを module へ出した周）を、**書いた人が札 1 行で明示して**通す逃がしである。`retroactive` を転用しない——あの数は「後から足した歯が N 本」と読まれるので、移動の便に貼ると判定行から何を免除したのか読めなくなる（実測 2026-09-11・`s2-07l.84`）。効く条件は `retroactive` と**同じ 4 つ**（test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない）で、判定は同じ実装（`marker_beads`）を通る。
  - `retroactive`: 既に land した挙動へ**後から歯を足す**便は、歯をどこへ置いても base で緑になる（測る対象が base に在る）。test 区間内の行 `// flip-check: retroactive <bead-id>` を置いた file は RED を要求せず、判定行に `retroactive=N` が載る。**src 区間の marker は効かない**（実装の隣に 1 行足すだけで検査を外せる形にしない）。**効くのはその便で足した札だけ**である＝札の bead id が HEAD の test 区間に在り、かつ base の test 区間に無いときに限る（base に無い file は test 区間が丸ごと新しいので HEAD に在れば足りる）。札は file に残るので、在るだけで数えると一度貼った札がその file の test 区間を触る以後のすべての便を免除し、札の bead id と便が対応しなくなる。**同一性は bead id で見る**（字下げや id 前後の空白が 1 個違うだけで持ち越した札が新しい札に化けると、古い id のまま免除が効き続ける）。持ち越した札しか無い file で **test 区間が動いた便**には免除を与えず、stderr に `flip-check: stale-marker <rel>` を 1 行出す（免除を求めていない便＝src だけ触った便には出さない。札は file に残るので、出すとその file の src を触るたびに「削除しろ」と言われ、本当に効かない札を見落とす）。**限界（もう 1 つ）**: 札の bead id が実在の便を指すかは照合しない（bd を見ない）ので、**新規 file へ古い id の札を置く**形は通る——判定行の `retroactive=N` が review の入口である。N ≥ 1 は review の対象で、notes に変異 proof を要する。marker の無い後から足す歯は従来どおり落ちる。**限界**: 行が実際にコメントか文字列の中身かは **parser 無しでは弁別できない**ので、複数行文字列の中に行頭から marker が現れる file は免除される（`crates/*/tests/*.rs` は全体が test 区間なので特に当たりやすい）。塞ぐには parser が要り、それは本器の取らない道である——代わりに `retroactive=N` が判定行に必ず出るので、**事故は見える形で残る**（review が拾う）。
  - **持ち越し**（`s2-07l.362`・契約表の行 e）: 純移動で base から item ごと移る `moved` 以外の札（`retroactive` 等）は、純移動の機械証明（§5.3）が両側で同じ字面の札を対にして残差から外す＝新規の札と読まない。対の無い札だけが `ForeignMarker`。
- **免除経路の閉じ方**（契約表の行 c・`s2-07l.170`・監査 2026-09-12 塊 14）。**本節の他の段落は着地済みの現物で、行 c が作るのは下の約束 (1)〜(4) だけである。**
  - 現物（planner が grep で実測・main 0b7e0a1）: docs-only の skip は `crates/xtask/src/flipcheck.rs` の `no_flip_verdict`（私有 `fn`・`judge_into` から呼ばれる）が `.rs` の差の有無だけで決め、path の面を読まない。札の読み手は `crates/xtask/src/flipcheck.rs` の `marker_beads`（私有 `fn`・bead id の字面を検めず本数も数えない）。push(main) の出所を測る口は xtask に無い（分岐は `crates/xtask/src/main.rs`・行の上限は `crates/xtask/src/limits.rs`）。宣言の `common-verify` の行を先頭語で分類する型は `crates/scribe2/src/pipe/declaration.rs` に無い。rules 行の kind の列は `crates/scribe2/src/rules/mod.rs`、行の読み手は `crates/scribe2/src/rules/manifest.rs`（`value_field` が list / int の形を kind に依らず既に読み、kind の名を 1 つも名指さない＝本行の 2 行〔list 1・int 1〕を足しても動かないので write-set に持たない・`.170` の受付が cap-headroom で止まった根を外す・実測 main b063ba0）、外形の pin は `crates/scribe2-boundary/tests/e2e/rules.rs` の `rules_external_form`（`rules: ok rows=47 kinds=47` の 2 行）と kind の網羅の `rules_kind_parity_every_kind_has_sample`。`rows=` / `kinds=` の数を持つ file は repo 全体でこの snapshot 1 つだけ（`rows=4[0-9]` / `kinds=4[0-9]` の全数 grep・2 件とも同じ file）。
  - 約束（この行が作るもの・番号は done と 1:1）:
    1. docs-only の分類は path の面（rules 行 `flip.docs_only_faces`・kind は list）で決め、面の外の file を含む便は `.rs` の差分が無くても `no-test-diff` で落ちる（`no-rust-diff` の skip は消す＝runner を撃たずに通す経路は残らない）。
    2. 札（`retroactive` / `moved`）の bead id は閉じた形で受け、形に合わない札は `bad-marker`、便が持つ札の本数が rules 行 `flip.marks_per_pr`（kind は int）を超えれば `too-many-marks` で落ちる。
    3. push(main) の CI は HEAD が PR の squash（件名末尾の `(#N)`）か `pipe land` の trailer（`run: <run id>`）を持つことを `xtask main-provenance` で測る。口の本体は行 c の write-set の `+` の file に置き、`crates/xtask/src/main.rs` は分岐 1 本、`crates/xtask/src/limits.rs` は行の上限だけが動く。判定行は `main-provenance: ok via=pr number=<N>` / `ok via=land run=<run id>`（rc 0）・`FAIL reason=no-provenance`（rc 1）で、git を撃てない周は rc 2。CI の job は push(main) のときだけ撃ち、`run:` は block scalar で書く＝`CLAUDE.md` の done 区間（`run: cargo …` の 1 行形の写し）に載らない（着地した後の main でしか満たせない門を便の done にしない）。発端の trailer は §60（行 bc）で足す（(#N) だけの commit は `no-source` で落ちる）。
    4. 宣言の `common-verify` の各行は先頭語列で閉じた `VerifyKind` に分類され、先頭語 `cargo` の行を 1 本でも持ちながら入口の flip を撃つ行を持たない宣言は intake が `NoEntranceRed` で断る。先頭語 `cargo` の行を持たない宣言（Rust でない toy repo・`sh` / `git` だけの `common-verify`）は分類だけで断らない＝上の「Rust 固有の検査を内蔵しない」のまま。宣言 file の schema は変えない。
    5. 足す rules 行は 2 本（`flip.docs_only_faces` = list・`flip.marks_per_pr` = int）で、kind の列（`crates/scribe2/src/rules/mod.rs`）・行の読み手（`crates/scribe2/src/rules/manifest.rs`）・外形の pin（`crates/scribe2-boundary/tests/e2e/rules.rs` の snapshot）・kind の網羅の sample に同じ便で載る。値と裁定 id（C5・user 裁定 2026-09-22T06:48Z・逐語は台帳 `s2-07l.170`）: `flip.docs_only_faces` = `docs/` `design-intent/` `.beads/` `README.md` `CLAUDE.md`（直近 40 commit の非 `.rs` の面の実測: docs 33・design-intent 1。`plugin/` と `rules/` は生成物と規則で docs でない）、`flip.marks_per_pr` = 16（HEAD の札の母集団: 便ごとの最大 15〔`.222`〕・10・9・8・6・6 の上）。manifest の `ruling` 欄は `user 2026-09-22T06:48Z` を写す。既存の歯 `flipcheck_no_rust_diff_skips`（`crates/xtask/src/flipcheck_overlay_tests.rs`）は形 1 で消える `reason=no-rust-diff` の rc 0 を pin しているので、同じ便で新しい語彙（`no-test-diff`）へ書き換える＝行 c の write-set に持つ。
    6. 閾値の読み手（`crates/xtask/src/limits.rs` の `Limits::read`）は足した行を含めて欠け無く読む（要求する本数は行の本数に追随する）。
  - 歯（接頭辞と置き場・名は `crates/xtask/src/flipcheck_tests.rs` の既存の族 `flip_check_*` と `crates/scribe2/src/pipe/declaration.rs` の既存の族 `declaration_*` に合わせる）: `flip_check_docs_only_` と `flip_check_marks_` は `crates/xtask/src/flipcheck_tests.rs`／`main_provenance_` は行 c の write-set の `+` の file の `mod tests`／`declaration_kind_` は `crates/scribe2/src/pipe/declaration.rs` の `mod tests`／`rules_flip_` は `crates/scribe2-boundary/tests/e2e/rules.rs`。`crates/xtask/src/limits.rs` の `Limits::read` は manifest から **11 値**を読み、欠けを `Err` にするので、行を足す周は in-file の歯 `limits_match_rules_manifest` が動く＝検証行に完全名で置く。rules 行を 2 本足すので外形の pin `rules_external_form`（`rules: ok rows=` と `kinds=` を持つ 2 行の数がどちらも 2 増える）と kind の網羅 `rules_kind_parity_every_kind_has_sample` も同じ便で動く＝検証行に完全名で置く。
  - 触らない: 本節の他の段落が述べる着地済みの経路（`not-flippable` / `tests-removed-only` / 宣言 file の同梱 / 歯の外の file の同梱 / `moved` / `retroactive` / base 段の撃ち直し / base の実体化）の判定そのもの・`crates/xtask/src/flipcheck.rs` の nextest を撃つ群（行 am が別便で子 module へ移す）・`crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap`（doctor の外形は rules の件数も xtask の口も持たない＝本行では動かない・実測）。

## 8. 歯（契約ごと・`tests/e2e/pipe.rs` module・tmp git repo（`.vessel` に `name=<NAME>`・隣に `.vessel.toml`〔Rust を含まない宣言・`allowed-commands = ["git", "sh"]`＝契約の verify 行は `sh <script>` の argv 1 本になり、上限は `--rules` の tmp manifest 側で広げる〕を marker と一緒に commit・`vessel init --state-dir` で tmp を紐づける）・fake runner / lens は `sh -c` 1 行）

- **置き場と接頭辞の規約**（個々の名前はここに書かない。名前の列は現物が SSOT＝`cargo nextest list -p <NAME>` が出す一覧・ADR-0013 §2.1・`s2-07l.78`）: pipeline の歯は `crates/<NAME>/tests/e2e/pipe.rs` module に `pipe_` 接頭辞で置き、段の名を副接頭辞にする（`pipe_intake_` / `pipe_spawn_` / `pipe_gate_` / `pipe_land_` / `pipe_retire_` / `pipe_resume_`〔殺してから引く歯は `pipe_resume_kill_`〕 / `pipe_stop_` / `pipe_approval_` / `pipe_report_`・`--pr-cmd` 形は `pipe_land_pr_cmd_`・toy repo の通し便は `pipe_five_` / `pipe_e2e_`・guard の backstop は `pipe_guard_`）。runner / lens の包みの歯は `crates/<NAME>/tests/e2e/headless.rs` に `headless_` 接頭辞（claude は fake の実行 file・`--claude <path>`）。外形（usage と 1 行出力）は面ごとに insta snapshot 1 本で pin する。契約の便固有 verify 行は副接頭辞 1 つの filter で撃つ（例: `cargo nextest run -p <NAME> --no-tests=fail pipe_retire_`・括弧を持つ `-E` 形は宣言の制御文字禁止に当たる）。
- **何を測るか**（名前を主語にしない設計の説明。歯の本数や名前は上の現物で数える）:
  - 入口と段: intake は必須 field の欠け・複数行の verify・req / design の無い契約を断り、通った便を fleet に記帳する／spawn は worktree を切って Implemented を記帳し、runner が commit を 1 本も作らなければ Failed にし、write-set を git dir へ書き、段違いを断る／stop は対象なしを rc 0、live な runner を止め、壊れた store を rc 2 にする／state は process を跨いで残り、resume は新しい process で Implemented から続く（**予定形**（`s2-07l.203` の land まで現物 0 本）: runner の process group を SIGKILL で殺した周も同じ道を通り、所有者の死んだ lock が残っていても別 process の resume が Landed まで通る・殺す前の結果は保たれる・C9）／land は PASS 無し・stale base を断り、1 commit の squash が tree 同一で、main で verify を再実行して赤なら loud に落ち、赤と「測れなかった」を分け、dirty な tmp worktree を除き、verdict を schema 1 で export し、worktree を可逆 move で畳んで branch を残す／toy repo の通し便が fake runner で 1 便 land する。
  - spawn が env を 1 つも足さないことは、fake runner が env を全部 file に写し、`<NAME_UPPER>_` で始まる変数**名の集合が親 process と同じ**であることで測る——**0 本では測らない**。器が足したかを見る歯なので、親が既に持っていた変数と器が足した変数を弁別できない形にすると、その接頭辞の env を持つ shell から撃つ周に歯が落ちる（`s2-07l.49`）。
  - retire（`s2-07l.46`）は、`--pr-cmd` 形で land した便の live worktree が在ることを先に測ってから畳み、`retired/<id>` へ**中身ごと**運ばれ・元の場所が空き・branch が残り・event の最終行が `Landed` + `detail=retired` であることまで測る。続けて 2 度目を撃ち **rc 1 ∧ event 行数不変 ∧ 畳んだ先は在るまま**。断る側（`Gated` のままの便／`Landed` だが live worktree に untracked file が在る便）は **「断ってから前提だけを解いて通す」形で測る**——rc 1 は subcommand を持たない器でも返るので、rc だけを見る歯は空虚になる（stderr の文言は pin しない）。`rebase-empty` の便を畳む側（`pipe_retire_rebase_empty_`・`s2-07l.128`）は、同一変更の 2 便で (iii′) の終端を作ってから retire を撃ち、rc 0 ∧ `retired/<run>` へ中身ごと ∧ 元の場所が空く ∧ branch が残る ∧ main 不変 ∧ **event の最終行が `Failed` + `detail=retired`** まで測る。負例は clean な木のまま `rebase-conflict` で終端した便で、**clean 検査では断れない**位置に置く（rc 1 が終端の理由を見ていることを担保する）。
  - gate は、dirty な worktree・commit 0 の便・段違いを断り、verify の赤い行で FAIL し、lens 無し / cap 超 / lens 本数が 1 でない / lens の rc≠0 / 出力が JSON でない / 3 値の外を INCONCLUSIVE にし、diff を lens の stdin へ渡し、verdict を構造化して残す。契約 path の置換（`s2-07l.31`）は fake lens が受けた argv を写し、置換後の値が run の契約 copy の絶対 path でそこから契約を読めることまで測る。
  - 測り直し経路（`s2-07l.30`）: `--lens` 無しで INCONCLUSIVE → 揃えて撃ち直して PASS → land／FAIL は終端で rc 1・lens を起動しない／resume は rc 3 で `next=gate` を名乗り何も書かない／`verdict.json` が**不在 / 壊れ / 3 値の外**の便は測り直さない（fail-closed）／cap 超過から予算を緩めて・規則の lens 本数を直して撃ち直せる／測り直しの周も worktree の事実の違反は終端する（掃除しても引けない非対称まで測る）。後の 4 本は変異が生き延びた経路（自前 1 本・独立 lens 6 本）と planner 裁定 Q3 案B に足したもので、既存の歯 2 本（段違いの拒否・INCONCLUSIVE からの測り直し）にも Landed 再 gate と resume(FAIL) の場合を書き足した。
  - 診断 file（`s2-07l.49`・retroactive）: 赤い行の stderr 本文が `verify.stderr.log` に残り、緑の行の見出しは出ない。**cmd の字面に無い語**で測る——見出し行は `cmd=` で command をそのまま載せるので、cmd に在る語で測ると stderr の写しが空でも緑になる。
  - 承認: 契約が 3 クラスを申告した便は spawn の手前で Blocked になり、逐語を human event として記帳し、空の逐語を断り、received の後だけ resume で spawn し、未 received は Blocked のまま、未知の class 値は intake で断る。**`pipe run` の一発経路も spawn の手前で Blocked になる**（関門が段ごとの口ではなく唯一の起動口に在ることを測る）。
  - headless: 契約を stdin で受け permission mode を毎回明示する／包みが rc を作り替えない／本文の引用や識別子の字面では上限と誤爆しない／cap 超の diff では claude を呼ばず INCONCLUSIVE／最後の JSON 行を採り parse 不能は INCONCLUSIVE／`--contract` 無し（rc 1）と読めない契約（rc 2）のどちらも claude を起こさない＝「無い」と「壊れている」で極性を変えない／prompt に契約の goal / done / verify / write-set と diff が載り placeholder が残らない（`s2-07l.31`）。
  - toy repo の 5 便（正常 / **write-set と両立しない契約で compliant な runner が write-set の外を書かない**〔fake の歯は空 commit → gate の verify が赤 / 実 runner は質問の口で止まる（`Questioned`・commit 0・guard の deny 0・[pipeline-question.md](./pipeline-question.md)）・`s2-07l.204` の実測〕/ test 追加 / gate FAIL / 承認 Blocked → approve → land・stdin は `/dev/null`）は **toy repo の `.vessel` を commit してから通す**——便の worktree は base の checkout なので、marker が untracked な repo では worktree に marker が無く `served()` は `Absent`＝guard が黙る。本 repo の root へ marker を置く理由がこれである。
  - report: **読めない台帳から 0 を出さない**（到達点の 1 行は「人手 0」を主張する面ゆえ、数えられなかったを 0 に化けさせると偽の全クリアそのものになる・C11.2）／`landed` は便の数であって event の数ではない／human event を数える。`--pr-cmd` 形は**承認 event 無しでも動き**（A4.3・[ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html)）・branch を push して main を動かさず・main が動いても PR は出せ（`{base}` は便の base）・**空の seam は公開したと名乗らせない**（`sh -c ""` は rc 0 で終わるため）。
  - **guard は misbehave した runner のための backstop** であって関門ではない＝compliant な runner の便は guard の手前（gate の verify）で止まり、deny は 1 件も出ない。その極性は独立した歯が持つ（hook を直に叩いて write-set の外への Write を deny させ、便が `Failed` になり deny が 1 件残ることまで測る）。

- **toy repo の seed（`s2-07l.117` の実測・2026-09-12）**: cargo crate の toy には **`Cargo.lock` を seed の commit に含める**。gate の precheck は untracked も clean の外と数える（便が生成した file を黙って捨てない規則は正しい）ので、lock を track していない toy では `cargo test` が生成する lock で precheck に落ちる。実 repo は lock を track 済みで発現しない。

- **歯の file の置き場**（`s2-07l.351`）: `tests/e2e/pipe/` の file は接頭辞（責務）ごとに 1 file——`intake.rs` = `pipe_intake_`、`review.rs` = `pipe_review_`、`contracts.rs` = 契約表の検査の歯（`contracts_check` を使うもの）、`refuse.rs` = 残りの `pipe_refuse_`、`ratelimit.rs` / `stop.rs` = `pipe_ratelimit_` / `pipe_stop_`（`s2-07l.349`）。2 file 以上が使う helper は `pipe.rs` の `pub(super)` に置いて複製せず、外形 snapshot の歯は `pipe.rs` に残す。`contracts.rs` は `contracts_check` を使う歯に加えて、intake の口で契約の閉包・導出・宣言を撃つ歯も持つ＝置き場は**名の接頭辞**で決める（名が `contract_` で始まる歯と `pipe_contract_` の歯が `contracts.rs`。`pipe_intake_` / `pipe_refuse_` で始まり名の途中に `contract_` を持つ歯は接頭辞の file に残る＝名の途中の語では動かさない）。接頭辞が 1 本だけの歯（`pipe_state_` / `pipe_show_`）は群を成さないので外形の歯と同じく `pipe.rs` に置く。

## 9. 到達点の計測（AC1 / AC2・(e)）

AC1 の条件文は「実 runner + 実 lens」なので、CI の歯（fake）は AC1 を測らない。実測は **機械が読む成果物**で残す。

- **toy repo 5 便（AC1）**: 開発 session が `<NAME> runner` / `<NAME> lens` を seam に渡して手元で 5 便を通し、`pipe report` の 1 行（`human_events_other_than_approval=0`）と `verdicts.jsonl` を bead `s2-07l.24` の notes に**逐語で**写す。人由来の event は approval の 1 件だけ。**`verdicts.jsonl` の行数は 5 ではない**——5 便のうち **land しない便**（便②は verify が赤い〔fake〕か質問で止まる〔実 runner・`Questioned`〕 / 便④は lens が FAIL〔fake〕。compliant な実 runner では write-set と両立しない契約が質問の口で先に止まる〔`Questioned`・`s2-07l.204` の裁定 (A)〕ので、実 lens の FAIL の現物は toy でなく本番の便〔`s2-07l.147`〕が担う）は land しないので、面 5 に出るのは main へ squash した便だけである（fake の歯では 3 行）。
- **自己ホスト 1 便（AC2）**: 本 repo の root に `.vessel`（`name=<NAME>` / `version=2`）を置く PR を先に land し（この便から guard が本 repo に効く）、実 bead 1 本の契約 file（`req` と `design` を持ち **`classes` は空**——自 repo への PR は「出す」でないため・[ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html)）を `pipe run … --pr-cmd` で PR 作成まで通す。CI 緑・merge は人。
- AC3（偽の PASS 0 件）は gate の INCONCLUSIVE 経路（lens 無し・cap 超）2 本 + 赤い verify 行の FAIL + PASS 無しの land 拒否 + 測り直し経路の終端 2 本（FAIL からの再 gate・読めない verdict）と resume の rc 3（**測れなかった便を「測り直してよい便」へ化けさせない**側の歯）が母集団（FR7 の面は `s2-07l.17` の CI job）。AC4 は resume が新しい process で Implemented から続く歯と state が process を跨いで残る歯が母集団——**process を殺してから引き直す歯は未着**（`s2-07l.203` が測る・候補 2 / 4 の周は便を止めて別 bead）で、**予定形**（`s2-07l.203` の land まで現物 0 本）: runner の process group を殺してから別 process の resume で Landed まで通す歯（`pipe_resume_kill_`・所有者の死んだ lock を残した周を含む）が母集団に加わる。AC1 の実測（(e) `s2-07l.24`）は実 runner の面を担う。AC5（無承認の通過 0）は `pipe_approval_` 接頭辞の歯（本数は現物で数える）が母集団——`classes` を名乗る契約が spawn の手前で Blocked のままであることを測る側だけである（`--pr-cmd` を承認で縛る歯は [ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html) で反転したので、承認 event 無しで `--pr-cmd` が動くことを測る歯は AC5 の証人ではない）。
- **並行 2 本（FR30 の前提・`s2-07l.117`・2026-09-12 実測）**: 同じ管理席から `pipe run` を 3〜8 秒差で 2 本同時に流す（toy crate・実 runner・実 lens・席の口座を継承）。3 組 6 run で runner 2 本が同時に生きた時間帯は 15 / 17 / 16 秒、fleet の event は run ごとに整合（SeatSpawned / SeatStopped 6/6・seat と run の不一致 0・lock の待ち・拒否の判定行 0 / stderr 4 行）、main は detached worktree の `cargo test` が緑＝**spawn / runner / gate は並行に動く**。ただし **write-set が交わらなくても 2 本目は `stale base`（§5.4 の base 固定）で land できず、`resume` も同じ rc 1**（Gated PASS のまま・event なし）＝先着 1 本だけが Landed に到達する。write-set が交わる 3 本目も同じ機構で止まり、CAS（`update-ref`）の失敗まで到達しない。並列化には 2 本目の base を新しい main へ進める口（land での rebase + gate の撃ち直し／resume の base 更新）が要る（起票候補・本便は計測のみ）。
- **質問の口 1 便（AC10・`s2-07l.116`・2026-09-12 実測）**: toy crate に「verify 行が矛盾する契約」（同じ純関数に **quarter(8) == 2** と **quarter(8) == 20** を要求）を `pipe run`（実 runner・席の口座・実 lens）で流す。実装役は矛盾を見抜いて最終行に質問 record を 1 つ書き commit 0 で止まり、包みが rc 76 で名乗って `QuestionRaised`（逐語）→ `Questioned` になる。回答なしの `resume` は rc 3 で event 0 件。契約の所有者が `pipe answer --words` で回答（`QuestionAnswered`・machine・逐語）し **run の写し `contract.toml` の done を回答どおりに直してから** `resume` すると、prompt の末尾に「回答」節が載った再 spawn が回答どおりに実装し（`Implemented`）、verify 4/4 と実 lens の PASS を経て `Landed` に至る。fleet の event は全部 machine（`pipe report` = `human_events=0`）・席が手で書いた commit 0。**写しの done を直さずに `resume` した便は実 lens が「契約と違う引数で verify だけ通した」と FAIL にする**（終端・回答節は契約を上書きしない＝回答が契約を変える周は写しも直す）。runner seam に `< {contract}` を付けると pipeline が stdin に流す「契約 + 回答」節が shell の redirect で潰れる（seam は §6 の形＝redirect 無し）。**包みが実 record を拾えない周が先に在った**（`s2-07l.123`・claude 2.1.268 の `result` record は入れ子の `"type"` が top-level より前）＝AC10 は `.123` の land 後に成立。

- **本番 repo 4 便（切替便 `s2-07l.127`・ADR-0016 §2.1 の既定形・2026-09-12 実測）**: 本 repo 自身を anchor に、契約済みの bead 4 本（`s2-07l.128` / `.129` / `.131` / `.130`）を管理席の手でなく `pipe run`（実 runner・実 lens・席の口座）で Landed まで通した。4 本とも管理席の code 0・人由来の event 0・QuestionRaised 0。所要は runner 8〜18 分 / gate 5〜18 分 / land 2.5〜15 分。`.128` は途中で main が別便で動き、§5.4 の追随（`Implemented detail=rebase:<old>..<new>` → gate 撃ち直し PASS）が本番で通った。gate の FAIL 2 回はどちらも契約の側の穴（挙動不変の refactor に property 歯だけで flip-check が RED にならない／record 不在が clean に化ける歯）で、実 lens と機械検証が fail-closed に止め、契約を改訂した 2 本目が通った＝偽の PASS 0。操作役に残る手順は 3 つ: 走らせる binary を新 main で build する・Landed の後に anchor から `git push`（land は origin へ出さない）・gate FAIL の run の worktree は live のまま残る（retire は `Landed` / `rebase-empty` 限定）。現物は bead `s2-07l.127` の notes と管理席の報告 file。

## 10. 却下案

- state を in-memory で持つ長寿命 process（FR3・AC4 に反する）。
- runner へ scribe2 固有の env で run 情報を渡す（C2.2）。placeholder 置換で足りる。
- bd を直接 write（台帳は bead id を持つだけ・SRS scope out）。
- worktree を repo 外に置く（`.worktrees/` の運用と揃える）。
- force 系 git・auto revert（N1・CON5。main red は loud に止める）。後始末を `worktree remove` + `branch -d` で行う（削除は N1・`branch -d` は squash では通らない・lens 指摘で却下）。
- main が動いた便の追随を `pipe resume --rebase` の別口にする（席が明示して撃つ）／便を直列化する lock（並列を諦める）——`s2-07l.119` で却下（FR30 の向きは「main が動いた便は測り直す」を構造で持つこと。別口は撃ち忘れで Gated PASS のまま永久に land できない便を残し、lock は並列そのものを捨てる）。
- stop を「tmux 窓を kill」で実装（MVP に tmux は無い・pid で止める）。
- 承認を land の手前に置く（merge は可逆・A4.3。不可逆の実行は runner の中で起きるので spawn の手前）。
- gate に Rust 固有の flip check を内蔵（toy repo は Rust とは限らない）。
- token → byte の換算係数を code に埋める（閾値は manifest・C1。byte を cap と直接比べる保守的な読みにした）。
- PR 作成の道具を core に内蔵（seam `--pr-cmd`。道具の選定は契約側）。〔承認 event 無しで `--pr-cmd` を動かす〕は当初 A1 の読みで却下したが、[ADR-0008](../../design-intent/decisions/ADR-0008-own-repo-pr-is-not-publish.html) で**採用へ転じた**（却下の根拠だった A1 の読みが A4.3〔merge・自 repo への dispatch は可逆〕で覆った）。
- 便の worktree を `<state_dir>/worktrees/<run>` へ出して plugin dir の外にする（sensitive 判定の案 (b)）。retire-by-move・`.vessel` marker・歯の path 前提が一斉に動くので、写す側（run dir 配下の plugin）で解いた。
- runner を `--permission-mode bypassPermissions` で起こす（同 案 (c)）。§6 の acceptEdits を捨てて Claude Code 側の保護を全部失うので採らない。
- runner の Bash 権限を起動口座の settings に暗黙に委ねる／変異 proof を repo の外の sh script（実装の字面を pin）で gate に撃たせる／契約の `verify` に共通規律（flip check・done の定義・write-set 照合）を毎便手書きする——いずれも [ADR-0009](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html) §4 で却下（C1 / C2 / C12・ADR-0001 の採用理由に反する）。
- 縦 1 本を 1 契約で書く（見積 ≈1,100 行・NFR2）。

## 11. 後続

- runner の質問の口（契約の不足を typed な質問 record で返して止まり、席が planner へ中継し、回答の記帳で再開する）と既定の配送構造（planner ×1 + 管理席 ×N）: [pipeline-question.md](./pipeline-question.md)・[ADR-0016](../../design-intent/decisions/ADR-0016-default-delivery-structure-and-typed-question-record.html)（SRS 改訂は user 裁定）。
- 並列の便の受付（intake の write-set 排他・`stop --run`）と衝突の起こし直し（回数の guard・retire の拡張）: [pipeline-conflict.md](./pipeline-conflict.md)・[ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)（要件は SRS v0.6 の FR10 / FR34）。
- 3 クラスの機械 enforcer（操作の中身からの判定・A4 機構欄）。R-C6-1（1 run の token 上限）の裁定が出たら `Budget` に上限を効かせる。
- 多 lens・tier・verdict 一致率（v3）。tmux / 席 / 口座選定（v3）。
- retired worktree の掃除の道具化（可逆 move の先を片付ける経路・N1.2）。

## 12. retire の終端の列挙（契約表の行 i・`s2-07l.353`）

- 出所: 審査の段（契約の審査・FR49／[contract-source.md](./contract-source.md) §4）が足した終端 Reviewed の FAIL / INCONCLUSIVE は live を持たないが、retire の入口は Gated の FAIL と Failed の一部しか畳めない＝審査の段で終端した便が前の周の worktree を残すと畳めず、run N+1 が別 worktree で立つ（`.209` run 1 の実測 2026-09-15）。畳めない worktree は再開（FR14）の続きの段を別の worktree に割るので、終端の後始末の口が終端の列挙に追いつく必要がある。
- 現物（planner が grep で実測・main f678bd0）: 受ける段の列挙は `crates/scribe2/src/pipe/cli/step.rs` の `retire_run`（`pub(super) fn`）が持つ `allowed` = `[Stage::Landed, Stage::Failed, Stage::Gated, Stage::Stopped]`＝`Reviewed` は列に無い。段の中の弁別は `crates/scribe2/src/pipe/cli/state.rs` の `discriminate`（同 file の私有 `fn`・`resolve` から呼ばれる）が持ち、`(&Extra::Retire, Stage::Gated)` と `(&Extra::Retire, Stage::Failed)` の 2 arm だけが在って `Reviewed` は末尾の catch-all で `Ok(())` に落ちる。判定の読み手は `crates/scribe2/src/pipe/review.rs` の `ReviewCheck::judge`（`pub fn`・返り値は `Passed` / `Stopped(Verdict)` / `Unreadable` の閉じた 3 つ）で、`crates/scribe2/src/pipe/cli/state.rs` の `discriminate` が持つ `(&Extra::Spawn, Stage::Reviewed)` の arm が既に呼んでいる。断りの字面の括弧に入る語は `crates/scribe2/src/pipe/review.rs` の `ReviewCheck::as_str`（`pub fn`）が作り、`Passed` → `PASS`（`crates/scribe2/src/pipe/gate.rs` の `Verdict::as_str` が `Pass` に返す語）・`Stopped(Verdict::Fail)` → `FAIL`・`Stopped(Verdict::Inconclusive)` → `INCONCLUSIVE`・`Unreadable` → `読めない` の 4 語で閉じる＝`Verdict` を持たない `Passed` と `Unreadable` にも語が在る。畳む本体は `crates/scribe2/src/pipe/retire.rs` の `retire`（`pub fn`・在るか / clean かだけを見て段を動かさず `retired/` へ可逆 move する）。
- 現物（歯の置き場と verify の接頭辞の当たり・planner が全 e2e の `#[test]` 付きの `fn` 名 923 本を走査して実測・main d875aaf）: 接頭辞 `pipe_retire_` は base で 6 本（`pipe_retire_moves_pr_landed_worktree_and_keeps_branch` / `pipe_retire_rebase_empty_folds_failed_run_and_keeps_stage` / `pipe_retire_rebase_empty_refuses_other_failed_reasons` / `pipe_retire_refuses_unless_landed_and_clean` / `pipe_retire_stopped_folds_a_clean_stopped_run_and_keeps_stage` / `pipe_retire_stopped_refuses_a_dirty_worktree`）に当たり、6 本とも base で緑＝この接頭辞を撃つ verify 行は base で rc 0 になり、新しい歯の RED→GREEN を測れない。接頭辞を `pipe_retire_reviewed_` まで伸ばすと base の当たりは 0 本（`--no-tests=fail` の rc 4＝RED の極性が立つ）。当たる 6 本と伸ばした先の 0 本はどちらも `crates/scribe2-boundary/tests/e2e/pipe/land.rs` の 1 file に閉じる（この行の write-set の中）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `retire_run` の `allowed` に `Stage::Reviewed` を 1 つ足す（既存の 4 つと順序は不変）。
  2. `discriminate` に `(&Extra::Retire, Stage::Reviewed)` の arm を足し、`ReviewCheck::judge` が `Stopped(_)`（FAIL / INCONCLUSIVE）を返す周は `Ok(())`＝畳める。
  3. 同じ arm で `Passed` は断る（live・起こす側）。
  4. 同じ arm で `Unreadable` も断る（読めない判定を終端に読み替えない・fail-closed）。
  5. 断りの字面は `discriminate` の `(&Extra::Spawn, Stage::Reviewed)` の arm と同じ形 `run <id> の段は Reviewed である（verdict=<語>）` で、`<語>` は `ReviewCheck::as_str` が返す語＝`Passed` の周は `PASS`・`Unreadable` の周は `読めない`。段違いの一般則の字面（`run <id> の段は Reviewed である`・verdict の括弧を持たない）と区別が付く。歯はこの 2 語を逐語で pin して新 arm を測る。
  6. 畳んだ後の段は `Reviewed` のまま（`Failed` / `Gated` と同じ）＝`RunStage detail=retired` の記帳と `retired/` への可逆 move は不変（N1.2）。
  7. 新しい歯 4 本は `crates/scribe2-boundary/tests/e2e/pipe/land.rs` に置き、接頭辞 `pipe_retire_reviewed_` だけで選べる名にする（既存の `pipe_retire_*` 6 本と `pipe_follow_retire_*` 2 本は名も本数も不変＝この接頭辞は 1 本も選ばない）。
- 触らない: `retire.rs` の `retire` 本体（在るか・clean か）・`Extra::Retire` × `Gated` の arm・`Extra::Retire` × `Failed` の arm（そこは行 r が別便で触る）・worktree の無い Reviewed 終端の便（畳む物が無い＝既存の断りのまま）・`crates/scribe2-boundary/tests/e2e/pipe/land.rs` に在る既存の `pipe_retire_*` 6 本と `pipe_follow_retire_*` 2 本（行 r が `pipe_retire_rebase_empty_refuses_other_failed_reasons` を別便で反転させる面には、この行の verify の接頭辞は届かない）。
- 却下: 審査の段の中で自動で畳む（終端の後始末は go を挟む retire の 1 口に揃える）／live が false の段を全部畳める側にする（Failed の理由ごとの弁別が消える）。

## 13. xtask の flipcheck.rs の分割（契約表の行 j・純移動）

- 何が起きているか: `crates/xtask/src/flipcheck.rs`（約 1320 行・上限 1500）は R-C4-2 の余地が 177 行しか無く、size M の便（.170 の行 c）を受付が断る（admin の実測 2026-09-16: src の満杯面 6 つのうちの 1 つ）。責務は 10 群あり、git / tar で base を取り出す群（parse_base / git_stdout / changed_rs / show / load_pairs / repo_root / extract_archive / materialize_base / index_base / work_dir・231 行）は他群から独立している（呼び手は run と judge の側だけ）。
- 形（.363 の `pipe/closure.rs` → `pipe/closure/derive.rs` と同型）: flipcheck.rs は残し、同名の新規 dir に子 module flipcheck/git.rs を置いてその群をそのまま移す（名・本文・順序を変えない）。親は mod 宣言と名指しの `pub use` で呼び手（`main.rs` の run・歯の `use super::{…}` 11 個）を無傷に保つ。歯（`flipcheck_tests.rs` と子 5 file）は動かさず、`super::` で読む private item のうち移す 4 つ（parse_base / failed_tests / nextest_args / FailedTest のうち git 群に当たるもの）は pub 化 + 再輸出で解く。親に残る私有 item を子が呼ぶ周は可視性を `pub(super)` に上げる＝可視性の 1 語と mod 宣言・`pub use`・`use` の path は移動の一部（純移動の残差として許す・.363 と同じ）。移動で生じた可視性の制約を説明する doc コメント行（例: 親の private 型を引数に持つ関数を pub に上げられない理由）も移動の一部＝要約の「コメント行の差」に数えてよい（.372 run 1 の Gated INCONCLUSIVE・admin の逐語実測 2026-09-16）。札 `// flip-check: moved <bead>` は親の歯の区間（flipcheck_tests.rs の先頭）と子の歯の区間に対で置く（純移動の機械証明は §5.3）。
- 触らない: FilePair / is_test_file / split_regions（fan-out が大きい）・cargo 実行の群（後続の便で runner.rs へ）・歯の中身。
- 見積: 親 1323 → 約 1100 行・子 約 235 行。

## 14. 引数の reader を 1 本に（契約表の行 f・`s2-07l.306`）

- 出所: `pipe land --run <id> --help` が help を出さず squash 形の land を実行し、anchor の main の ref を進めて `Landed` まで完走した（実測 2026-09-15）。不正入力を黙って落とさない（NFR4）に反し、`verdicts.jsonl` へ 1 行 append して `Landed` と記帳する land の終端（FR12）が呼び手の書き間違いだけで起きる。
- 現物（planner が grep で実測・main f678bd0・`.349` の分割後）: argv の reader は 6 本で、どれも「名指しの flag を position で拾う」だけ＝argv 全体が既知の集合に閉じているかを見ないので、未知の flag と `--help` / `-h` を黙って無視する fail-open。
  - `crates/scribe2/src/pipe/cli/args.rs` の `flag`（`pub(in crate::pipe) fn`）と `need`（`pub(super) fn`）
  - `crates/scribe2/src/fleet/cli.rs` の `flag`（私有 `fn`・返り値は同 file の `Flag`）
  - `crates/scribe2/src/seat/cli.rs` の `flag`（私有 `fn`・返り値は同 file の `Flag`）
  - `crates/scribe2/src/headless/mod.rs` の `flag`（`pub fn`）と `need`（`pub fn`）
  - `crates/scribe2/src/hook/vessel.rs` の `flag_value`（私有 `fn`）
  7 本目の `crates/scribe2/src/account/cli.rs` の `flags`（私有 `fn`・`allowed: &[&str]` を取る）だけが allowed の集合で閉じるが、断りを全部 `None` に畳むので Help / Unknown / Missing / Duplicate を弁別しない（[account-lifecycle.md](./account-lifecycle.md) §4）。usage の字面は面ごとに 1 本ずつ在り（`pipe` は `crates/scribe2/src/pipe/cli.rs` の `usage`・`pub fn`）、外形は insta の名付き snapshot 5 本が pin している: `pipe_external_form`（`crates/scribe2-boundary/tests/e2e/pipe.rs`）・`fleet_external_form`（`crates/scribe2-boundary/tests/e2e/fleet.rs`）・`seat_usage_external_form`（`crates/scribe2-boundary/tests/e2e/seat.rs`）・`headless_external_form`（`crates/scribe2-boundary/tests/e2e/headless.rs`）・`vessel_external_form`（`crates/scribe2-boundary/tests/e2e/hook.rs`）。
  - reader の**呼び手**（grep の実測・main 0878dad・`.306` run 3 の問い）: pipe の `flag` / `need` は定義 file の外で `crates/scribe2/src/pipe/cli.rs`（`dispatch` の周辺・8 site）と subcommand の本体 `pipe/cli/step.rs`（7）・`pipe/cli/resume.rs`（5）・`pipe/cli/intake.rs`（3）・`pipe/cli/run.rs`（2）・`pipe/cli/state.rs`（1）・`pipe/cli/show.rs`（1）・`pipe/stop.rs`（1）・`pipe/ratelimit.rs`（1）が `use super::{…}` で引く。headless の `flag` / `need` は `headless/mod.rs` 自身（2）と本体 `headless/runner.rs`（9）・`headless/lens.rs`（6）が引く。fleet / seat / vessel / account の reader は定義 file の中でしか呼ばれない。**本体の呼び手 36 site は本行の write-set に無く、約束 5 はそれらを 1 字も変えない**（閉包の検査を入口に置く形＝下）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 共通の reader 1 本を新 module（行 f の write-set の `+` の file・`crates/scribe2/src/lib.rs` に `mod` 宣言）に置く: `parse(args, allowed) -> Result<Parsed, ArgsError>`。`Parsed` は名指しの flag の値（`value` / `need` の 2 面）と positional の列を持つ。
  2. `ArgsError` は閉じた enum（`Help` / `Unknown` / `Missing` / `Duplicate`・宣言順の `as_str`）。
  3. `--help` / `-h` は `allowed` に無くても `Help` で返し、呼び手は usage を出して rc 0 で終わる（state も ref も 1 本も動かさない）。
  4. `Unknown` は usage 1 行で rc 2。`Missing` / `Duplicate` も同じ rc 2 の口から出す。
  5. 閉包の検査は**面の入口 1 か所ずつ**で撃つ: pipe は `crates/scribe2/src/pipe/cli.rs` の `dispatch` が subcommand を選んだ直後、headless は `crates/scribe2/src/headless/mod.rs` の `runner` / `lens` の分岐の直後、fleet / seat / vessel / account は各 cli の入口（write-set の 4 file）で `parse(args, ALLOWED_<subcommand>)` を 1 回撃ち、断りは (3)(4) の口へ出す。各 subcommand の `allowed` は宣言順の const 配列で持つ。通った argv は上の 6 本の reader が従来どおり位置で読む（reader の定義と本体の呼び手は 1 字も変えない）。`account/cli.rs` の `flags` だけは allowed の集合を既に持つので `parse` の呼出に置き換える（`None` に畳む挙動が typed な 4 値へ置き換わる以外は変えない）。
  6. `pipe land` に**未知の flag** を渡した周は、偽 remote の toy repo で main の ref・event log・worktree が 1 つも動かず rc 2 で断る。
  7. `pipe land` に **`--help`** を渡した周も同じ toy repo で main の ref・event log・worktree が 1 つも動かず、usage を出して rc 0 で終わる（**2026-09-15 の実測の回帰そのもの**＝この枝は (6) と別の歯で 1 本ずつ測る。単体の reader の歯だけでは緑にならない）。
  8. `fleet` / `seat` / `headless` / `vessel` / `account` の 1 口ずつが未知の flag を rc 2 で断る（**5 口を 1 口ずつ名指して測る**＝1 口だけ直して緑にならない）。
  9. 既存の flag の意味と usage の文は不変＝上の外形 snapshot 5 本が 1 字も動かない（挙動の差は「未知の flag と `--help` を断る」だけ）。
- 触らない: subcommand の本体・rules・docs。依存を足さない。`.config/nextest.toml` は、seat の口の新しい歯が tmux の直列化の一覧（`test-groups.tmux` の filter）に載る周だけ 1 行が増える面として write-set に持つ（載らない周は触らない）。

## 15. pipe の `--repo` / `--state-dir` の cwd fallback を落とす（契約表の行 g・`s2-07l.310`）

- 出所: cargo-mutants の一時コピーは worktree の `.git`（file・本物の gitdir を指す）を持つので、コピーの中で cwd から解いた repo に便を起こすと worktree と branch が本物の repo に登録される（prunable 47 件・fixture 名の branch 44 本・実測 2026-09-15・prune は user 承認 event 02:5xZ）。base を記録して worktree を切る runner の起動（FR4）が、呼び手の指さない repo に効いてしまう＝intake の排他（FR39）が数える write-set の相手も別 repo に割れる。
- 現物（planner が grep で実測・main f678bd0・`.349` の分割後）: cwd の枝を持つのは `crates/scribe2/src/pipe/cli/args.rs` の `repo_of`（`pub(super) fn`・`--repo` が無いと cwd の repo root へ落ちる）で、呼び手は同 file の `state_dir_of`（`pub(in crate::pipe) fn`・`--state-dir` が無い周に `repo_of` 経由で置き場を解く）と `crates/scribe2/src/pipe/cli/intake.rs` の `run_repo`（`pub(super) fn`・写し面が読めない周の最終枝）の 2 つ。usage の字面は `crates/scribe2/src/pipe/cli.rs` の `usage`（`pub fn`）で、外形は `pipe_external_form`（`crates/scribe2-boundary/tests/e2e/pipe.rs`）の名付き snapshot が pin する。
- 順序: 本便は**行 v（`s2-07l.381`）の後に出す**。行 v は e2e が binary を起こす 41 site を 1 関数に集めて cwd を git repo でない temp dir に固定し、その site 数 1 を母集団付きの歯で pin する（§28 の歯 2）。ゆえに repo を要る呼出しは行 v の時点で既に `--repo` / `--state-dir` を渡しており、本便は e2e の helper と既存の呼出し site を 1 つも触らない（触るのは下の新しい歯だけ）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `repo_of` から cwd の枝を落とし、`--repo` が無ければ flag 不在の断り 1 行で rc 1（`--run が要る` と同じ作り＝`Refuse` は契約単位の拒否ゆえ variant を足さない）。
  2. `run_repo` の最終枝（写し面が無い周）も同じ断り＝cwd を読まない。
  3. `state_dir_of`（`--state-dir` も `--repo` も無い周）も同じ断り＝cwd を読まない。
  4. 断った周は worktree を 1 つも作らず event を 1 件も書かない。
  5. `usage` に `--repo` の要件を 1 句足す＝`pipe_external_form` の snapshot はこの 1 句だけ動く。
- 触らない: `crates/scribe2/src/hook/vessel.rs` の `repo_root`（cwd から解くのは hook の領分）・契約 file の schema・写し面の読み（`run_repo` の最終枝より手前）・e2e の helper（行 v の面）。
- 却下: 変異の一時コピーの `.git` を切る hook（cargo-mutants に口が無い）／歯が必ず `--repo` を渡すだけで器を変えない（規律が歯の散文に残る・N2 / C16）／写し面が無い spawn を cwd で救う（壊れた run は断る側・C10）。

## 16. runner / lens の effort を rules 行から毎回渡す（契約表の行 h・`s2-07l.322`）

- 何が起きているか: `headless/mod.rs` の build は `--model` を毎回渡すが `--effort` は渡さない＝runner / lens の effort は口座 dir の settings の値で決まり口座ごとにばらばら（席の model 事故と同じ根因）。user 裁定 2026-09-15T03:52Z = effort は high・model は既存の rules 行 runner.model のまま。
- 形: rules 行 runner.effort（kind RunnerEffort・Str・値 high・裁定 id 付き・C5）を `rules/manifest.toml` に足し `rules/mod.rs` の閉じた enum に variant 1 つ。effort の型は headless に置く（閉じた enum Effort = Low / Medium / High / Xhigh・宣言順の const slice・alias = CLI の字面・parse は完全一致）。読み口は runner_model と同じ形の runner_effort（行が無い / 不発効 / 文字列でない / 表に無いの 4 理由）。Call に effort を足し build が `--model` の直後に `--effort <値>` を毎回渡す。runner と lens は同じ manifest から読み、読めない周は claude を呼ばず rc 2（model と同じ極性・順序 = cap → model → effort）。計測の Call（`fleet/usage.rs`）は None で挙動不変。歯の fixture の manifest には effort の行を同じ helper で足す。
- 触らない: `pipe/`・`seat/`・headless の雛形 txt・`.vessel.toml`。依存を足さない。

## 17. lens の verdict に findings の閉じた category と母集団を必須にする（契約表の行 k・`s2-07l.188`）

- 出所: research ponytail §4 (1)・監査 2026-09-12 塊 21（`.175`: lens の verdict に母集団が無く「0 件」と「未測」を弁別できない）。`.175` は本便に統合する（同じ record・同じ雛形）。
- 現物: lens の雛形は `crates/scribe2/src/headless/lens.txt`（`crates/scribe2/src/headless/lens.rs` の `include_str!`）で、verdict は `pipe/gate.rs` が `verdict.json` に書き `Gated` を記帳する。`pipe/gate/lens.rs` の `parse_lens` が拾う key は verdict と evidence の 2 つだけである。
- 形: findings の観点を閉じた enum にする（`pipe/gate.rs` か新規 module `pipe/gate/findings.rs`）。variant は既存の観点 3 つ（contract-fit / teeth-nonvacuous / constitution）と過剰設計 5 種（delete / stdlib / native / yagni / shrink）の 8 つで、宣言順の一覧と字面変換を 1 箇所に閉じる。
- `parse_lens` に `findings=<category:件数,…>`（8 category を 0 も含めて全部）と `population=<files:<n>,lines:<n>>` を必須 key として足し、欠落・母集団 0 の周は `Verdict` を Inconclusive に倒す（既存の測り直し経路をそのまま使う）。`verdict.json` にも同じ field を持たせ、`Gated` の detail は変えない。
- 雛形 `lens.txt` に観点 8 行（pointer 付き・判断は lens に残す）と出力の形（2 key の字面）を足し、外形を snapshot で pin する。歯の偽 lens（`fake_lens`）は 2 key を出す形に改める。
- 触らない: verdict の 3 値・lens の本数と予算（rules 行）・runner の雛形。

## 18. land の stale base を同じ land の中で人手なしで追随し直す — resume の Gated(PASS) 受けは既在で同じ周回を通る（契約表の行 l・`s2-07l.335`）

- 出所: admin 報告（`.329` run 2）: 追随の撃ち直し中に main が動くと `pipe land` が stale base の rc 1 で抜け、`pipe run` はそこで終了する＝段は `Gated`（PASS）のまま次の land を撃つ主体が無い。user 直命: dispatcher の仕組みを最優先にし、admin が手で撃ち直す穴を器で塞ぐ。
- 現物: `pipe/land.rs` の `land` 関数が stale base を refused で返す。`pipe/cli/resume.rs` の `resume` は `Stage` の `Gated` の周を `verdict_of` の値で分けている。`pipe/follow.rs` は起こし直しの上限を rules 行 `pipe.follow_retries` で持ち、回数は replay から導く（`EXHAUSTED`）。
- 形: `pipe run` の着地の段で land が stale base を返した周は `RunStage`（`stage=Gated detail=stale:<base>..<main>`）を記帳し、同じ追随の経路（rebase → gate の撃ち直し → 順番待ち → land）へ戻る。回数は既存の `pipe.follow_retries` の判定に「起こし直し 1 回」として数え、上限に当たれば typed な `Failed` で終端する（既存の終端の型を使い新しい理由の variant は増やさない）。周回は `land` の中に置き、試行 1 回の戻りを閉じた enum（決着 / stale）にして自由文で判定しない。stale の記帳は `follow.rs` の衝突と同じ記帳の口を通し、`retried` は `rebase-conflict:` と `stale:` の行を 1 つの回数に合算する。stale の周回に `--runner` は要らない。列の鍵は最初の Gated の ts なので stale の Gated 記帳で動かない。
- `pipe resume` は `Stage` の `Gated` かつ verdict PASS の便を既に `land_run` へ流す（触らない）。周回は `land` の中に閉じるので、`pipe run` / `pipe resume` / `pipe land` のどの口から撃っても同じ追随を通る。verdict が Pass でない周は従来どおり断る。
- 着地の形（`s2-07l.335`）: `land` は前提検査と順番待ち（1 回・列の鍵は動かない）の後に試行を周回し、試行 1 回の戻りは閉じた 2 値（決着 / stale〔old と now の 2 sha〕）。stale の周は `follow.rs` の衝突と同じ記帳の口で `RunStage stage=Gated detail=stale:<old>..<now>` を 1 件記し、回数の判定も衝突と同じ 1 本（`FollowCheck`・`retried` は `rebase-conflict:` と `stale:` の行を合算 − 1）で、上限の内なら次の試行（base の読み手は `base_of_run` の 1 本＝前周の `rebase:` の新しい側）へ戻り、上限で `Failed detail=rebase-conflict` rc 1・読めない周は `Failed detail=follow-unmeasured` rc 2（衝突と同じ終端形）。stdout は周を跨いで追随・撃ち直しの判定行を捨てず、stale の周に `run=<id> stale=<old>..<now>` の 1 行を足す。`stale:` の判定（`is_stale`）は `is_conflict` と**別**で、`resume` の弁別（衝突だけを起こし直しの続きと読む）は動かない。
- 歯（`pipe_land_stale_` 接頭辞・`tests/e2e/pipe/land.rs`・既存の追随の fixture〔`gated_pass` + 面の内の別便の commit + 撃ち直しの lens の中で main を面の内へ進める偽 lens〕の型）: (a) 撃ち直しの間に main が 1 度動いた周は同じ land の中で `stale:` を 1 件記帳し、2 周目の追随（`rebase:` 2 件）→ 再 gate（偽 lens 2 回）を通って新しい main の上に squash が載り `Landed`（`--runner` 無し・base は rc 1 `stale base` で段 Gated のまま＝RED）／(b) 毎周 main が動く周は上限（fixture の rules で 1）で `Failed detail=rebase-conflict` rc 1・stale の記帳 2 件・偽 lens は 2 周分・squash は載らない／(c) 衝突の記帳を 1 件持つ便は上限 1 の下で最初の stale で終端する（合算・別々に数えると 2 周目で載る）。in-file の歯（`follow_stale_` 接頭辞・`pipe/follow.rs`）: `stale:` と `rebase-conflict:` の判定は別で接頭辞は `:` まで見る／回数は同じ便の `RunStage` の 2 種の行の合算 − 1（別の便・`RunStage` 以外・終端の理由の語だけは数えない）。
- 触らない: `land` の CAS と stale の判定・`follow_retries` の値・`pipe/queue.rs`。
- 却下: stale を state dir に記録するだけで撃ち直しは人に任せる（撃つ主体が席のまま残る）／新しい rules 行を作る（既存の `pipe.follow_retries` で足りる）。

## 19. pipeline 外の merge のための着地列の待ち口（契約表の行 m・`s2-07l.212`）

- 出所: 設計 doc や ADR の PR の squash merge が `Gated`（PASS）の便の追随を 1 周誘発する（実測）。便の着地と merge は器の dispatcher が持つ（FR30）が、**pipeline の外で行う merge はその口を通らない**＝いまは「`Gated` PASS の便が 0 の窓か `Landed` の直後に merge」を撃つ側の散文で守っていて規則ではない（N2）。器が窓を測って返す口が無い。
- 現物（planner が grep で実測・main f678bd0）: 着地の列は `crates/scribe2/src/pipe/queue.rs` の `turn_in`（`pub fn`）/ `await_turn`（`pub(super) fn`）が読む。唯一の wait は `crates/scribe2/src/fleet/wait.rs` の `Completion`（`pub enum`・`RunnerExited` / `SeatGone` / `SlotFree` / `GroupGone` / `LandTurn` / `AccountFree` / `CiResult` / `HostCalm` の 8 つで、`SlotFree` / `LandTurn` / `AccountFree` / `CiResult` / `HostCalm` が pid を見張らない側・`HostCalm` は器の健康の遮断器〔`s2-07l.504.3`〕が足した）。`Completion` の variant を網羅 match で読む site は 3 つで全部 `crates/scribe2/src/fleet/wait.rs` に在る（`impl Completion` の `pid(`〔`match *self`〕と `is_met(`〔`match self`〕・in-file の歯 `pipe_stop_group_completion_match_is_exhaustive`〔全 variant を並べて名を写す census〕）＝variant を足すと動くのはこの 3 site だけで、src の他の 9 file の参照は variant の構築であり腕を持たない（grep で実測・2026-09-21）。追随中の記帳: land の追随（§18）は `RunStage` `Implemented` に detail `rebase:<sha>` を記す（fleet の event log の実測）ので、「追随中の便」は最新の `RunStage` が `Implemented` で detail が `rebase:` で始まり、その後に `Gated` / `Landed` の記帳が無い便と読む。git の読みは `crates/scribe2/src/pipe/mod.rs` の `git_ok`（`pub fn`）と `git_line`（`pub fn`）の 2 口だけ。subcommand の verb は `crates/scribe2/src/pipe/cli.rs` の `match verb` 1 か所に並び、usage の外形は `pipe_external_form`（`crates/scribe2-boundary/tests/e2e/pipe.rs`）の snapshot が pin する。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `Completion` に pid を見張らない variant を 1 つ足す（窓の待ち）。既存の 8 つの判定と deadline の経路、唯一の wait の loop は不変。動くのは網羅 match の 3 site だけ（`pid(` に腕 1 本・`is_met(` に腕 1 本・census の歯に腕 1 本と名 1 つ）。census の歯は接頭辞 `fleet_wait_` に当たらないので、verify はその歯を名の全体（`pipe_stop_group_completion_match_is_exhaustive`）で別の 1 行として撃つ（2026-09-21 の便 2 本目の審査 FAIL vacuous-assert の再現）。
  2. 窓の判定条件は 3 つ全部: (a) 着地の列に PASS の便が 0 本 (b) 追随中の便（最新の `RunStage` が `Implemented` で detail が `rebase:` の便・現物の項）が 0 本 (c) local の `refs/heads/main` が `refs/remotes/origin/main` の祖先である（**未 push の squash が無い**＝主実測中の便が local main に積んだ squash と push 待ちの `Landed` の両方を数える・`s2-07l.449` の 1 面目: 窓が `Landed` 前の squash を数えず docs の merge で origin と分岐した・実測 2026-09-17）。
  3. (c) の読みの順は固定: **先に** local の `refs/heads/main` を読み、読めない周は origin の有無に依らず窓を**閉じる**（fail-closed）。**次に** `refs/remotes/origin/main` を読み、無い周は (c) を数えず（(a)(b) だけで判定し）行に `remote=none` を載せる。両方読めた周だけ祖先を判定する。
  4. origin の ref は読むだけで fetch しない（撃つ側が fetch する・器は網を撃たない）。git の読みは既存の `git_ok` / `git_line` の 2 口だけを使う。
  5. 新しい subcommand の口 `pipe land-window` を足す。窓が開いていれば rc 0 で `clear` の 1 行、待ちが切れれば rc 1 で列の便を名指した `busy` の 1 行を返す。pipeline の外で merge を撃つ側はこの口を前置して撃てる＝散文の窓判断が器の rc に変わる。
  6. `busy` の行は列の便の名指しの隣に `unpushed=<local main の sha|unreadable|->` を持つ（(c) で閉じた周は sha・読めない周は `unreadable`・(a)(b) だけで閉じた周は `-`）＝どの条件で閉じたかが 1 行で読める（C10）。
  7. verb が 1 つ増えるので usage に 1 行増え、`pipe_external_form` の snapshot がその 1 行だけ動く。
- 着地の形: variant は `Completion::LandWindow { state_dir, repo }`（`CiResult` と `HostCalm` の間に宣言＝census の「宣言順の末尾に HostCalm」は不変）。判定は `crates/scribe2/src/pipe/queue.rs` の `window_now` の 1 本（列の便は `turn_in` と同じ面〔終端でない ∧ `Gated` を通った ∧ worktree が実在〕で、判定を読めない便も数える＝PASS でないと測れていない便を外さない・C10）で、`queue` は `pipe` の外へ見えないので `crates/scribe2/src/pipe/cli.rs` が `window_now` だけを再輸出し、`is_met(` と `pipe land-window` が同じ 1 本を読む。口の待ちの上限は `--wait-s N`（無ければ 0＝1 周だけ観測）で、待ちの後に窓を読み直した 1 行で答える（`after_wake` と同じ再評価）。行は `land-window=clear[ remote=none]` か `land-window=busy queue=<便,…|-|unreadable> following=<便,…|-|unreadable> unpushed=<sha|unreadable|->[ remote=none]`（local main を読めない周は origin を読まないので `remote=` を載せない）。
- 触らない: `gh pr merge` 自体（器は merge を撃たない）・列の順序と鍵・`LandTurn` の判定・`turn_in` の本体。
- 却下: docs-only PR も器が `gh` を撃って merge する（外部 binary を撃つ面が増える）／運用のまま据え置く（規則が散文のまま・N2）。

## 20. runner の雛形に終端の規律を足し片付けで殺した子の数を記録する（契約表の行 n・`s2-07l.275`）

- 出所: `.270` run 1 の観測: runner が「flip-check を背景で回している・完了通知を待つ」と言って turn を閉じ（rc 0・自己申告は done）、背景の task が scope の片付けで止められた。runner の口（FR5）の自己申告が背景 task の完了を含まず、人手 0 の計測（FR22）に載る終端の 1 行が「何を殺したか」を持たない。
- 現物（planner が grep で実測・main f678bd0）: 雛形は `crates/scribe2/src/headless/runner.txt`（34 行・`crates/scribe2/src/headless/runner.rs` が `include_str!` で読む）で、「背景実行で turn を閉じない」に当たる行は 1 本も無い（`背景` / `前面` / `turn を閉じ` の 3 語とも 0 件）。片付けは `crates/scribe2/src/pipe/confine.rs` の `release_scope`（`pub fn`・引数は `&Confinement`・返り値は `Option<Released>`）が行い、`crates/scribe2/src/headless/mod.rs` がそれを呼ぶ。終端の 1 行は `crates/scribe2/src/headless/runner.rs`（`<who>: scope=<Released の語> claude_peak_bytes=<n|->`）が組んで stderr へ出す＝片付けで殺した子の数はどこにも残らない。雛形の外形は `headless_runner_prompt_external_form`（`crates/scribe2-boundary/tests/e2e/headless.rs`）の snapshot が pin する。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 雛形に「検証は前面で完走させてから turn を閉じる（背景実行を残して終えない・残した task は片付けで止められ done に数えない）」の 1 行を足す。
  2. その 1 行だけを名指す歯を別に持つ（`.snap` は入口の flip の test 区間に入らないので、雛形の RED は逐語を名指すこの歯で測る）。
  3. 雛形の外形は `headless_runner_prompt_external_form` の snapshot が引き続き全文を pin する（足した 1 行だけが snapshot に現れる）。
  4. `release_scope` が scope を止める**直前**に scope に残った process の数を読む。
  5. 終端の 1 行に `orphans=<n|->` を足す（0 も書く・読めなければ `-`＝「0 本」と「測れなかった」を融合しない・C10）。
  6. 数える関数（`crates/scribe2/src/pipe/confine.rs`）と行を組む関数（`crates/scribe2/src/headless/runner.rs`）は pure に切り、それぞれの file の in-file の歯が fixture で測る（systemd の scope を歯で起こさない）。
- 触らない: 片付けの極性（止める）・runner の権限・`Released` の語彙・`claude_peak_bytes` の読み。
- 却下: 背景 task を待ってから片付ける（turn の終端の規律が曖昧になる）／雛形だけ直す（殺した事実が記録に残らない）。
- errata（実装の現物・`s2-07l.275`）: 数える口は約束 4 の `release_scope` ではなく、runner / lens の終端の 1 行を組む `scope_line` が `release` を撃つ**直前**である。`release_scope` の中で数えると gate の verify 行ごとの片付けでも `systemctl` の呼出が `kill` の隣に 1 本増え、「行の終端で 1 回だけ、包んだ名の scope を SIGKILL で片付ける」を pin する既存の歯（`crates/scribe2-boundary/tests/e2e/pipe/gate.rs`）が落ちる——その file はこの行の write-set の外なので、数える口を終端の 1 行の側へ寄せた（約束 4 の「止める直前に読む」は不変で、読む場所だけが違う）。数える root は既定の cgroup root 固定で、runner / lens の `--cgroup-root` は走行中の peak の読みの差し替え口のまま（root を差し替えて測る歯は pure な読みを直に撃つ）。雛形は 34 行から 35 行になる。

## 21. gate の段の通知行を rc に依らず record と stderr に残す（契約表の行 o・`s2-07l.293`）

- 出所: `.286` の gate が 2 周とも lens への入力が diff だったのに理由語が残らなかった（run.stderr 0 bytes・rc 0）。gate の機械検証（FR8）が「測れなかった / 要約にならなかった」理由を捨てるので、人手 0 の計測（FR22）で後から原因を引けない（黙って落とさない・NFR4 の面）。同じ形は precheck の注意行・追随の rebase の行にも当たる。
- 現物（planner が grep で実測・main f678bd0）: 理由の 1 行は `crates/scribe2/src/pipe/move_proof.rs` の `LensInput::notice`（`pub fn`・返り値は `Option<String>`）が出し、`crates/scribe2/src/pipe/gate.rs` がそれを `Outcome.err` に載せる。`crates/scribe2/src/pipe/cli/run.rs:126` の `chain`（`pub(super) fn chain(lines: &mut Vec<String>, outcome: Outcome) -> Option<Outcome>`）は rc が `RC_OK` の周に `lines.extend(outcome.out)` して `None` を返す＝**`err` をそこで捨てる**。run dir にも書かれない（`lens-input.txt` は要約の周だけ残る）。gate の記録の行は `crates/scribe2/src/pipe/gate/record.rs` の `step_record`（`pub fn`）が組む。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `chain` が rc 0 の段の `err` を保持し、`pipe run` の stderr に段の順で出す（畳む関数は 1 本のまま・`resume` の 2 段は `notes` を持たない薄い口を通り従来の形を保つ）。
  2. stdout の判定行は 1 字も変わらない（`out` の繋ぎ方と rc の極性は不変）＝(1) の歯が「stderr に段の順で出る」と「stdout が base と同じ」を**対で**測る。
  3. gate の記録の step 行と同じ log——段の見出し `## n=<i> rc=<rc> cmd=<cmd>` を持つ診断 file（`verify.stderr.log`・§5.3）——に `lens-input=<kind> reason=<語>` を追記する（**要約の周も diff の周も**・rc に依らず・run dir に残る）。**`verify.jsonl` には書かない**: あの file は「1 行 = 1 record」で、追随の引き継ぎ（§33）が record の通し番号 `n` を**行数から**導くので、record でない行を混ぜると `n` が飛ぶ。
  4. `kind` は判定行が既に出す `LensInput` の kind（`diff` / `summary`）、`語` は `notice` の reason の値で、要約の周は `-`（0 と「測れなかった」を融合しない・C10）＝`move_proof.rs` は触らない。
  5. 歯の置き場: 記録の歯は `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`、`pipe run` の stderr の歯は `pipe run` の e2e が在る `crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`（§5.9 の分割で出来た `crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` と `crates/scribe2-boundary/tests/e2e/pipe/stop.rs` はどちらも `pipe run` の歯を持たない）。
- 触らない: 判定の極性・`notice` の語彙・lens の入力の選び方・`move_proof.rs`。
- 却下: stderr にだけ出す（run dir に残らず事後に読めない）／記録にだけ書く（撃った場で読めない）。

## 22. 撃ち直しの間も着地の番を先頭に保つ（契約表の行 p・`s2-07l.305`）

- 出所: `.294` が `.279` を追い抜き、`.279` が撃ち直し 1 周分（約 20 分）を余計に払った（現物確認 2026-09-15 00:4xZ）。着地の終端（FR50）の列が、鍵の早い便が一度離れて戻る周に先頭を 2 本作る＝配送構造（FR30）の着地が費用だけ二重になる。
- 現物（planner が grep で実測・main f678bd0）: 列の判定は `crates/scribe2/src/pipe/queue.rs` の `turn_in`（`pub fn turn_in(queue: Option<&[Queued]>, me: &str) -> Turn`・**pure**・`Turn` は `First` / `After(String)` / `Unmeasurable` の閉じた 3 値）1 本で、順序は `Queued.gated_at`（**最初の** `Gated` の ts・同時刻は run id の辞書順）だけから導く。待ちは同 file の `await_turn`（`pub(super) fn await_turn(entry: &Land<'_>) -> Order`）で、`crates/scribe2/src/pipe/land.rs` が追随の前に 1 回だけ撃ち、撃ち直しの後は番を読み直さない（読み直すのは main が動いたかだけ）。ゆえに鍵の早い便が INCONCLUSIVE で一度列を離れて戻ると、番を取った便と両方が自分を先頭と読む。`Gated` の記帳は `detail=verdict:<…>` を持つので、gate の周数を数える歯は `verdict:` の件だけを母集団にする。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `await_turn` が「自分の番」と判定した周に `RunStage`（`stage=Gated detail=turn:taken`）を 1 行追記する（既存の段の event を既存の記帳の口から書き、`detail` で弁別する＝新しい `EventKind` は足さない）。
  2. 記帳するのは `Order` が番を取った 2 値（`First` / `Waited`）の周だけ（縮退と測れなかった周は番を取っていないので書かない）。
  3. `turn_in` は、列の便のうち `turn:taken` を持つ便が在れば、**最新の** `turn:taken` の ts（同時刻は run id の辞書順）の 1 本だけを先頭とする（自分なら `First`・他なら `After`）。`turn:taken` を持つ便が 1 本も無ければ従来どおり鍵の順。
  4. 列を離れた便（終端・worktree 無し・verdict が PASS でない）の `turn:taken` は数えない＝戻ってきた便は番を持たない側から数え直す。
  5. `Queued` に導出の field を 1 つ足す＝その便の最新の `turn:taken` の ts（無ければ `None`）。`turn_in` が pure である性質と `Turn` の閉じた 3 値は不変。
- 実装の読み（行 p の着地時）: (4) の「戻ってきた便」は log から導く——`turn:taken` の後に `verdict:` が PASS でない `Gated` の記帳が在る便は field が `None`（撃ち直しの `verdict:PASS` と `stale:` は消さない）。`turn_in` は列の面（終端でない ∧ worktree 在り ∧ verdict が PASS）に居る便の field だけを数える。番待ちの間に列の先頭が自分を着地 / 終端させた便（§40）は段が終端なので記さない（`Gated` の記帳で終端の段を上書きしない）。番を取った後に断る周（stale base・汚れた木・`rebase-empty`）の「event を書かない」歯は `turn:taken` の 1 行を除いて数える。
- 触らない: 鍵（`gated_at`＝最初の `Gated` の ts）の定義・stale base の判定・`await_turn` の待ち（唯一の wait）・`Turn` の variant の数。
- 却下: 撃ち直しの後に番を読み直す（払う側が入れ替わるだけで 1 周の損失は消えない）／受容する（dispatcher で便が増えると追い抜きの頻度が上がる）。

## 23. stop 起因の終端を oom-kill に誤分類しない（契約表の行 q・`s2-07l.340`）

- 出所: admin 実測: `.336` run 2 を `pipe stop` した同じ秒に `RunStage`（`stage=Failed detail=oom-kill`）が記録され、その後 `RunStopped` で最終的に `Stopped` になった（直後の available memory は圧迫なし）。
- 現物: `pipe/spawn.rs` の `OOM_DETAIL`（"oom-kill"）・`pipe/confine.rs` の `Reason::OomKill`（終端行の `oom_kill` ≥ 1 で判定・`pipe/gate/lens.rs`）・`pipe/stop.rs` は席を止め切ってから `RunStopped` を書く。stop の終端検出が「runner が消えた」を oom-kill に倒す経路が疑われる（kernel の証拠は権限で未確認）。
- 形: `pipe stop` は signal を送る前に、その run の「停止中」の印を書く。runner の消滅を見た経路（`pipe/spawn.rs` の終端検出）は、その run が停止中なら `Failed detail=oom-kill` を書かず `RunStopped` の経路に任せる。印は `RunStage stage=<現段> detail=stopping`（kind も field も既存・字面は `pipe/mod.rs` の定数 1 本）で、「停止中」は便の**生の**最後の `RunStage` の detail が stopping であることを下の別の口 1 本で読む（停止中の spawn は段を 1 件も書かず `SeatStopped` だけを書いて rc 1 で止まる＝呼び手は次の段へ進まない）。止め切れなかった周は印が残り便は live のままで、次の `pipe stop` が同じ判定で読む（再 spawn の `RunStage` は最後の記帳を置き換えるので印は自然に読まれなくなる）。
- oom-kill は `Reason::OomKill` の既存の判定条件（終端行の `oom_kill` ≥ 1）が在る周だけに限る。`Usage` の `oom_kill` は Option（終端行が無い / 読めない周は `None`）で、包めた周の終端行が無い / 読めない signal 死は `Reason` の末尾に足す variant（`Unknown`・`as_str` = unknown）で `Failed detail=unknown` に倒す（0 と「測れない」を融合しない）。終端行が在って `oom_kill` が 0 の周は箱の中の死と読まず従来の settle（runner-rc）へ落とす。gate の分類（verify 行・lens・審査）は読みを Option に合わせるだけで `Unknown` を作らない。
- 印を書くのは便 1 本を外す口（`pipe stop` の `--run`）だけ。席の掃除の口（`--all`）は便の現段を解かない口なので印を書かず、その周は上の証拠条件だけが効く（oom-kill の証拠が無ければ unknown）。**印は既存の読み手からは見えない**: 便の最後の `RunStage` の detail を返す口（`pipe/mod.rs` の 1 本）は印の行を読み飛ばし、その手前の最後の `RunStage` の detail を返すので、衝突の記帳（`rebase-conflict:<base>..<main>`）も `Failed` の理由も印に上書きされず、既存の読み手 2 面（resume の弁別・retire の入口）は 1 字も変わらない。停止中かは同じ file の隣に置く別の口 1 本（生の最後の `RunStage` の detail が印か）で読み、`pipe stop` と spawn の終端検出だけがそれを呼ぶ。不変は歯で測る（衝突を記帳した `Implemented` の便を止めた後も resume が起こし直しの続きと読み、log に印の行が在る・`tests/e2e/pipe/stop.rs`）。他の歯は `tests/e2e/pipe/spawn.rs` に置く（理由の語彙の歯は `as_str` の語の列の完全一致で測り、production と同じ file の in-file の歯を flip の根拠にしない）。
- 触らない: stop の極性（止め切れなければ `RunStopped` を書かない）・oom の閾値。
- 却下: dmesg / journalctl を読む（権限と host 依存）／stop 後の `Failed` を後から書き換える（append-only の log を汚す）。

## 24. pipe retire が受ける終端の段を広げる（契約表の行 r・`s2-07l.132`）

- 出所: `s2-07l.127` phase 1 / 2 の実測: gate FAIL で終端した run の worktree が live のまま残り、`pipe retire` は限られた段しか受けないので操作役が畳めない。dispatcher で便が増えると FAIL 終端の worktree が積む。追随（FR34）が衝突の上限に達して `Failed` で終端した便も同じ穴に落ちる（下の `follow::EXHAUSTED`）。
- 現物（planner が grep で実測・main f678bd0・`.349` の分割後）: 畳む本体は `crates/scribe2/src/pipe/retire.rs` の `retire`（`pub fn`）で「在るか・clean か」だけを検査し、段を動かさず `retired/` へ move する（段の弁別は持たない）。受ける段の弁別は呼び手が持つ＝`crates/scribe2/src/pipe/cli/step.rs` の `retire_run`（`pub(super) fn`）の `allowed`（`Landed` / `Failed` / `Gated` / `Stopped`）と `crates/scribe2/src/pipe/cli/state.rs` の `discriminate`（同 file の私有 `fn`）の 2 arm＝`Extra::Retire` × `Gated` は verdict が `Fail` の周だけ・× `Failed` は `last_stage_detail` が `REBASE_EMPTY` か `follow::EXHAUSTED` の周だけで、他の段は末尾の catch-all で受ける。＝`Landed` / `Stopped` / `Gated(FAIL)` / `Failed(rebase-empty / rebase-conflict)` は**既に畳める**（`crates/scribe2-boundary/tests/e2e/pipe/land.rs` の `pipe_retire_*` 6 本 / `pipe_follow_retire_*` 2 本の歯が pin・母集団は同 file の歯全部）。base で断るのは **`Failed` の他の detail**（`main-red` / `main-unmeasured` / `rebase-dirty` / `precheck:…`）だけで、`pipe_retire_rebase_empty_refuses_other_failed_reasons` がその拒否を pin している。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `discriminate` の `Extra::Retire` × `Failed` の arm から detail の弁別を外す＝`Failed` は detail を問わず `Ok(())`（`last_stage_detail` の読みも断りの字面もこの arm から消える）。
  2. その結果、受ける集合は `Stage` の終端全部（`Landed` / `Stopped` / `Failed`）∧ `Gated(FAIL)` になる（`Reviewed` の非 PASS を足すのは行 i の面で、本行は触らない）。
  3. `allowed` の列・`retire.rs` の本体・clean の検査・`retired/` への可逆 move・段を動かさないこと（`RunStage detail=retired` の記帳）は不変。「人が現物を読む前に入れ物が動く」懸念は可逆 move（N1.2）と `detail=retired` の event が持つ＝読む物は消えない。
  4. 非終端（`Spawned` / `Implemented`）と `Gated(PASS)` は断る（退行の pin・`stop` の極性も不変）。
  5. `land.rs` の既存の `pipe_retire_*` / `pipe_follow_retire_*` の歯は、極性が反転する 1 本（`pipe_retire_rebase_empty_refuses_other_failed_reasons`＝`Failed` の他の detail を断る pin）を「畳める」側へ書き換える以外は不変。
- 触らない: `stop` の極性・`Extra::Retire` × `Gated` の arm・`Extra::Retire` × `Reviewed`（行 i の面）。
- 却下: 手で `git worktree remove`（pipeline の外・不可逆）／段を新しい `Retired` の variant に動かす（`.128` の裁定に反する）。

## 25. 純移動の要約にコメント行の差の逐語を載せる（契約表の行 s・`s2-07l.377`）

- 何が起きているか: `.372` run 1 が Gated INCONCLUSIVE（2026-09-16・admin の逐語実測）。純移動の要約（§5.3 の機械証明・`pipe/move_proof.rs`）は item の中のコメント行の差を `CommentDiff { file, name, lines }`（件数だけ）で `render` の「## コメント行の差（名: 行数）」に出し、逐語を持たない。lens は「契約の『移動以外 0 行』を満たすか材料から確かめられない」で判定不能にする。`.361` の要約は通った（残差が use / path だけで「## 残差分（逐語）」に逐語が載る）＝差の種類によって材料が足りなくなる。
- 形: `Item::comments`（hash から除いたコメント行の字面・区間の順）は既に在るので、`CommentDiff` に base 側と head 側の逐語（indent を落とした字面・区間の順）を持たせ、`render` が各項目の件数の行の下に base 側を `-`・head 側を `+` の接頭辞で逐語のまま並べる。件数の行は残す（母集団と対）。要約の byte は既存の予算の照合（§5.3「lens に渡す本文の byte で行う」・要約 > cap → INCONCLUSIVE で理由に kind と byte が載る）に乗り、**新しい cap を持たない**。
- 既存の歯の書き直し（run 1 = Questioned 2026-09-16・runner の逐語「本便の新外形と正面から矛盾して必ず RED になる」・admin が現物で確認）: `crates/scribe2-boundary/tests/e2e/pipe/gate.rs` の `pipe_gate_move_proof_comment_only_diff_inside_items_sends_summary` は旧外形を字面で pin する（件数の行の直後が `## 残差分（逐語）`・コメントの字面 `helper two` は要約に載らない）。本便はその歯を新外形（件数の行の直後に `-` / `+` の逐語が並び、コメントの字面が要約に載る）へ書き直す。歯の名・`lens-input=summary`・rc・stderr 空の pin は不変＝write-set はこの e2e file を含む。
- 触らない: item の切り出し・hash・多重集合の照合・残差の判定（`NotPure` の理由）・`keep` の置き場・lens の口。
- 却下案: コメント行の差を `NotPure` に倒す（doc コメントの path 書き換えは純移動の残差として許す既存の裁定・`.362`）／逐語に cap を別に持つ（数値の線が増える・既存の照合で足りる）。

## 26. lens の cmd を run の record に残し gate / land / resume が同じ 1 か所から読む（契約表の行 t・`s2-07l.378`）

- 何が起きているか: admin の実測 2026-09-16（`.372` の着地を `pipe land --run` で撃ち、rebase 後の再 gate が lens を要するのに「lens が要るのに --lens が無い」で INCONCLUSIVE・`pipe gate --run … --lens` の撃ち直しで回復）。現物（verified）: `--lens` は `pipe/cli/step.rs` の `review_run` / `gate_run` / `land_run` が毎回 flag から読み、run dir（`contract.toml` / `vessel.toml` / `review.json` …）には lens の cmd が残らない。操作役が land を手で撃つ周（Gated PASS の run を後から着地・admin の常道）は毎回同じ cmd を渡さないと必ず踏む。
- 形: `pipe review --lens <cmd>`（run の最初に lens を受ける口）が cmd を `<run_dir>/lens.toml`（`schema = 1` / `cmd = "<逐語>"`・rules manifest と同じ parser の subset・run dir の一時物で跨版契約ではない）に写す。`gate` / `land` / `resume` は `--lens` が無い周にその写しを読む（`--lens` が在れば flag が勝つ＝上書きの手は残す）。写しが**無い**周は従来どおり INCONCLUSIVE（「lens が要るのに --lens が無い」）、写しが**読めない**周は理由を変えて INCONCLUSIVE（`lens.toml` の path と読めなかった理由・「無い」に潰さない・C10）。読み書きは新 module `pipe/lens_record.rs`（pure な parse + I/O 2 関数）に置き、3 つの口は同じ 1 関数で読む（C2）。
- 分岐の置き場（run 1 = 審査 INCONCLUSIVE 2026-09-16「verdict の生成箇所に依存し文面から確定できない」の解・admin の現物実測: 「lens が要るのに --lens が無い」を出すのは `pipe/gate.rs` の `gate`（`let Some(cmd) = entry.lens else`）と `pipe/review.rs` の `decide` の **2 か所**・母集団 = `crates/scribe2/src` の grep・`pipe/land.rs` は自分では出さず再 gate に `entry.lens` を渡すだけ）: 読みの 3 値は `lens_record.rs` の閉じた型 1 つ（flag か写しから得た cmd / 無い / 読めない〔path と理由〕）で表し、`Gate` / `Review` / `Land` の `lens` field（現物は `Option<&str>`）をその型に置き換える。step.rs は `--lens` が在れば cmd を、無ければ写しを読んだ結果をその型で渡す。INCONCLUSIVE の理由の分岐は既存の 2 か所が 3 値を match して行う（無い = 字面不変・読めない = path と理由）。`pipe/land.rs` は field の型と再 gate への pass-through だけが変わり、追随の要否判定は触らない。step.rs で先回りする形（写しが読めない周に gate / land を呼ばず INCONCLUSIVE を書く）は採らない＝land の再 gate が要らない周まで INCONCLUSIVE に倒す挙動変化になる。
- 既存の歯の fixture（run 3 = QUESTION 2026-09-16「写しを読む gate が既存 5 本を PASS に変えて赤にする」の解・admin の現物実測: 「`--lens` 無しで INCONCLUSIVE」を期待する歯は `tests/e2e/pipe/gate.rs` に 4 本〔`pipe_gate_inconclusive_without_lens_when_required` / `pipe_gate_regates_after_inconclusive` / `pipe_gate_fails_regate_on_dirty_worktree` / `pipe_gate_refuses_regate_without_readable_verdict`〕・`tests/e2e/pipe/spawn.rs` に 1 本〔`pipe_resume_reports_next_gate_on_inconclusive`〕・母集団 = `tests/e2e/pipe` の grep）: これらは review を `--lens` 付きで通した run に対して gate / resume を `--lens` 無しで撃ち INCONCLUSIVE を期待するので、写しが在る世界では**設計どおり gate が lens を起動して PASS に変わる**。歯の期待（「写しも flag も無い周は INCONCLUSIVE」）を保つ形は fixture 側で `<run_dir>/lens.toml` を外す 1 行だけ（歯の名・極性・assert は不変・step.rs や gate に「写しを読まない」seam は作らない）。この 2 file は行 t の write-set に含める（歯の fixture が閉包の外に在る形を残さない）。
- `Land.lens` の型置換が届く literal 構築点（run 4 = QUESTION 2026-09-16「queue.rs の in-file の歯の helper `land()` が `Land { lens: None, .. }` を literal で組む」の解・admin の現物実測: `src/pipe/queue.rs` の `mod tests` の `fn land<'a>(…) -> Land<'a>` が `lens: None` を持つ）: `Land` の field の型を替える便は `Land {` の literal 構築点を全部持つ＝`queue.rs` も行 t の write-set に含める（変わるのは in-file の歯の helper の 1 field だけ・queue の判定は不変）。
- 触らない: lens の起動の形（`{contract}` / `{worktree}` の穴・stdin の diff）・gate の判定順・land の追随の要否判定・`pipe run` の引数（run は lens を受けない＝現物のまま）。
- 却下案: `pipe land --lens` を必須にする（毎回手で渡す seam が残る・記録に残らない）／event log の detail に cmd を書く（detail は自由文で typed に読めない・cmd に空白と引用符が入る）／`vessel.toml` の写しに足す（写しは tracked な宣言の写しで、操作役の入力を混ぜると出所が割れる）。

## 27. land の終端に main の実測 sha を写す（契約表の行 u・`s2-07l.379`）

- 何が起きているか: admin の実測 2026-09-16 00:4xZ（`.373` の着地: chain の log は `landed=9ad8b8f…` と出したが、その object は repo に無く、main に載った squash は `2d25ce3`〔親 `6519b39`〕）。現物（verified）: `pipe/land.rs` の `finish` は `new`（`commit-tree` で作り `update-ref` の CAS で main に載せた sha・§5.4 の手順 1）を verdict export（`sha`）・`RunDone stage=Landed detail=sha:<new>`・stdout `landed=<new>` に**宣言値のまま**写し、終端の時点で `refs/heads/main` を読み直さない。CAS の後に main が動いた周（追随の chain・別の便・手の操作）を land の記録から見分けられない。
- 形: `finish` の直前（export の前）に `git rev-parse refs/heads/main` を 1 回実測し、**stdout に `main=<実測>`・event の detail に `main:<実測>`（`sha:` の後ろ・空白区切り）** を足す。`new` と一致する周も書く（一致を「省略」で表さない・C10 の実測値）。読めない周は `main=unknown`（land は成立している＝落とさない・理由は stderr 1 行）。`landed=` / verdicts.jsonl の `sha`（squash の sha・key 列は跨版契約で不変）の意味は不変。
- 触らない: squash・CAS・anchor の同期・main 実測（手順 3）・retire・verdicts.jsonl の key 列・`--pr-cmd` の形（main を動かさない形は `main=` を持たない）。
- 却下案: 不一致を `Failed` に倒す（main は既に進んでいる＝終端を偽らない・記録して loud に留める）／push 後の `origin/main` を読む（land は push しない・remote は器の外）／verdicts.jsonl に key を足す（跨版契約の改訂＝ADR が要る・本便の射程外）。

## 28. e2e の歯が binary を起こす cwd を repo の外に固定する（契約表の行 v・`s2-07l.381`）

- 何が起きているか: admin の実測 2026-09-16 01:4xZ（本番の state dir の `pipe/` に e2e の fixture 便 `s2-2e5-…`〔`contract_body()` の goal・owner・`write-set = ["src/lib.rs"]`・`repo` file は tmp の toy repo・review.json は `evidence:"fake"`〕が 1 件〔全 320 件中〕・`fleet/events.jsonl` に RunCreated / RunStage Reviewed の 2 件〔2320 件中〕）。現物（verified・main d6e4e6f）: `pipe/cli.rs` の `state_dir_of` は `--state-dir` が無いと `repo_of` へ落ち、`repo_of` は `--repo` が無いと **cwd** から repo root を解いて `hook/vessel.rs` の `state_dir`（`git -C <root> config --get <NAME>.stateDir`）を読む。e2e の helper（`tests/e2e/pipe.rs` の `run_pipe` / `intake_raw` ほか）は全部の呼出しで `--state-dir` を渡している（母集団 = `run_pipe(&[` 136 箇所・grep）が、binary を **cwd を継いだまま**（nextest の子 process の cwd = crate dir・便の worktree の中）起こす。便の worktree は anchor の `.git/config` を共有し、この host の anchor は `<NAME>.stateDir` に本番を持つ。経路（inferred・時刻 01:39Z は `.349` / `.379` が Implemented → gate に入った直後）: gate の変異検査は**変異 binary で歯を回す**ので、`flag` / `state_dir_of` / `repo_of` を壊す変異の下では `--state-dir` / `--repo` が読めず cwd の fallback が本番へ届く。CI は config を持たないので露出せず、この host でだけ非 hermetic。`intake` は `--repo` を必須にするので cwd の fallback に届かず、届くのは `show` / `report` / `resume` など `--state-dir` 無しで `state_dir_of` を撃つ口である。
- site の census（planner が実測・main f678bd0・母集団 = `crates/scribe2-boundary/tests/e2e/pipe.rs` と `crates/scribe2-boundary/tests/e2e/pipe/` 配下の全 file）: binary を起こす site は **合計 41 箇所・9 file** で、字面は 2 形ある——`Command::new(bin())` が 38 箇所（`pipe.rs` 4 / `pipe/intake.rs` 13 / `pipe/land.rs` 6 / `pipe/spawn.rs` 5 / `pipe/stop.rs` 6 / `pipe/gate.rs` 2 / `pipe/ratelimit.rs` 1 / `pipe/launch_failure.rs` 1）と `Command::new(super::bin())` が 3 箇所（`pipe/dispatch.rs`）。**どの site も cwd を継ぐ**（nextest の子 process の cwd = crate dir）。例外は既に自分で `current_dir` を置いている 2 箇所だけで、`pipe/launch_failure.rs` の 1 箇所（相対 `--repo` が cwd から解けることがその歯の主題）と `pipe/dispatch.rs` の 1 箇所である。`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs` の残り 1 つの `bin()` は runner の shell 文字列に埋める path で Command を作らない（site に数えない）。親の helper のうち `pipe` の verb で binary を起こすのは 3 本＝`run_pipe` / `run_pipe_with_path` / `land_once_with_git_shim`（`intake_raw` と `land_once` は `run_pipe` 経由なので直の site ではない）。
- 形: binary を起こす口を **1 関数**（`crates/scribe2-boundary/tests/e2e/pipe.rs` に新設・`Command` を返し `current_dir` を git repo でない temp dir に付ける・**関数の名は行 v の契約が持つ**＝base に無い名を本節は名指さない）に集め、上の 41 site を全部それに通す。cwd が主題の 2 site は、返った `Command` に自分の `current_dir` を**後置**して主題を保つ（後の指定が勝つ＝歯の意味は変えない）。cwd が repo でなければ、どの変異の下でも cwd の fallback は「repo の root を解決できない」で**断る**（fail-closed）＝本番へは届かない。器の側（`state_dir_of` / `repo_of` の fallback・`vessel::state_dir`）は触らない（読みの口 `show` / `report` を anchor の cwd で撃つ常道を残す）。本便の write-set は全部 test 区間なので flip-check の overlay は HEAD と一致し base で赤くならない。歯は retroactive 札で通し、`current_dir` を外した A/B と差し替えを 1 site 戻した A/B の rc を変異 proof として notes に残す。
- 歯（2 本立て・名は行 v の契約が持つ。番号は done の (2) (3) と 1:1）:
  1. 親の helper 3 本（`run_pipe` / `run_pipe_with_path` / `land_once_with_git_shim`）の**それぞれ**で、`--state-dir` も `--repo` も無い `pipe show` が「repo の root を解決できない」の断りで rc 1 になる（3 本を**1 本ずつ名指した 3 つの歯**で測る＝1 本だけ直して緑にならない）。
  2. 置き場の pin: tracked の 9 file を読み、`Command::new(bin())` と `Command::new(super::bin())` の出現の**合計が 1**（新設の関数の中の 1 箇所）であることを、読んだ file 数と base の 41 site を母集団として同時に出して測る。cwd が主題の 2 site も新設の関数を通るのでこの合計を崩さない。
- 差し替えの残り: 差し替えで cwd の fallback に頼っていたことが露出した呼出しは、その site に `--repo` / `--state-dir` を明示して直す（歯の名・本数・assert は変えない）。この面は done の 8 門の workspace 実走が担保し、本行は検証行を置かない（**行 g（`s2-07l.310`）が器の cwd の枝を落とせるのはこの状態が前提**＝上の歯 2 が site 数を母集団付きで pin する）。
- 形 2 の改訂（契約表の行 aq・`s2-07l.547`・歯だけ・src は触らない）: 置き場の pin の「tracked の 9 file」は定数で、`pipe/` 配下に file を足す純移動の便（§42 の行 b）が定数を 12 に触ると純移動の機械証明が items-differ で落ち、lens の入力が要約にならず diff が `gate.token_cap` を超える（run 054450Z・2026-09-22・verified）。定数を**親（`crates/scribe2-boundary/tests/e2e/pipe.rs`）の列 0 の `mod` 宣言の数 + 1** に替える: 読んだ file 数と宣言の数を母集団として message に出し、宣言の無い file と file の無い宣言をどちらも赤にする。site の合計 1 と base の 41 site の pin は動かない。歯は base でも緑なので `// flip-check: retroactive s2-07l.547` の札を歯の fn の中の行頭に置き（[contract-source.md](./contract-source.md) §31 と同じ 4 条件・後から形を切り直した歯の申告）、変異の A/B（宣言を 1 本消す・`pipe/` 配下に file を 1 本足す）の撃墜を notes に残す。
- 触らない: `state_dir_of` / `repo_of` / `vessel::state_dir` の解決順・`vessel init` の呼出しの引数（`--state-dir` と root を明示済み）・`fleet` / `seat` / `hook` の e2e の helper（本便の射程外・同じ型は別便で数える）・汚れた 1 件の処分（消さず `retired/` へ移す = N1.2）・歯の名と本数（site の差し替えだけで期待は変えない＝既存の e2e が全部緑のままなのは done の 8 門が測る）。
- 却下案: 書く口（intake / run）に `--repo` を必須にして cwd の fallback を消す（変異の下では必須の検査も壊れる＝歯の側で cwd を固定しないと閉じない・admin の launcher の引数も変わる）／CI に `<NAME>.stateDir` の config を足して再現する（露出の面を増やすだけ）／本番の置き場を手で掃除する（不可逆・N1）。

## 29. 既に main に squash が在る便の land は rebase-empty の Failed でなく Landed（冪等）で終端する（契約表の行 w・`s2-07l.389`）

- 何が起きているか: admin の実測 2026-09-16 04:2xZ（`.379` run 012455Z）。器の段は `Failed detail=rebase-empty`・`verify-main.jsonl` は無いが、成果は main に在った（squash の本文に `run: <run id>` の trailer・便の worktree の HEAD の tree と squash の tree が同一・gate 3 周目 8/8 rc 0）。admin が tree の一致と gate の緑を根拠に sha 名指しで push した＝push / close / verify-main の終端の手順が器の外に落ちた。見立て（deduced）: 再起動 → resume → 着地の chain の周回で、前の周が local main に squash を載せた後（§5.4 手順 1 の CAS の後・手順 2 の実測の前）に死に、次の周の land が同じ便を rebase したので commit が 0 本になった。現物（verified・main d214c43）: `pipe/land.rs` の `rebase_onto` は `commits_after_rebase == Some(0)` を一律に `follow_failed(REBASE_EMPTY)` へ倒し、「main に既にこの便の squash が在る」（`squash_message` が本文の末尾に置く trailer `run: <run id>` で同定できる）と「本当に空の便」（先に land した別の便と同じ patch）を弁別しない。結果、台帳と event の段が実態（着地済み）と食い違う（C3・C10）。
- 形: `rebase_onto` が commit 0 本を見た周、`Failed` に倒す前に **`refs/heads/main` の log をこの便の trailer 1 行（`run: <run id>`・run id は時刻付きで一意）で 1 回だけ探す**（`git log <main> -n 1 --fixed-strings --grep=<trailer> --format=%H`・`.` を regex に読ませない・main の祖先だけが母集団）。**在れば** `Follow` に新 variant **`AlreadyLanded(sha)`**（`Stopped` / `Ready` の隣）で返し、`land` はその周に squash と CAS を撃たず（main は動かさない・anchor の同期は `old → old` の no-op）、**主実測（`verify_main`）はその sha に対して従来どおり撃ち**（前の周が実測の前に死んだ可能性が在る＝記録が無いものを緑と読まない・C10・木が gate と同じ周は検出線を撃たない §5）、緑なら `finish` を **`new = 見つけた sha`** で通す。`finish` の stdout は `landed=<見つけた sha> main=<実測> … order=<…>` に **`already-landed=1`** を後置し、`RunDone stage=Landed` の detail は `sha:<見つけた sha> main:<実測> already-landed`（§27 の `main:` の後ろ・空白区切り）、verdicts.jsonl の行は従来の key 列（`sha` = 見つけた sha）で書く（跨版契約は不変・任意 field を足さない）。**無ければ**従来どおり `rebase-empty` で終端する（本当に空の便の意味は不変）。commit 数を読めない周の扱い（0 に読み替えない）も不変。
- 触らない: squash の message の形（§5.4・trailer の字面 `run: `）・CAS・追随の要否判定と衝突の経路・§27 の `main=` の実測・`verify_main` の中身と skip の判定・verdicts.jsonl の key 列・`retire_worktree`・`--pr-cmd` の形（main を動かさないので本節は通らない）。
- 却下案: admin の chain が trailer を見て手で push / close する（器の外の運用・台帳と event の段が食い違ったまま）／`Failed` のまま notes で補う（同上・C3）／trailer でなく tree の一致で同定する（同じ tree を持つ別の便〔純移動の再 land 等〕を自分の squash と誤認する・trailer は便 1 つに 1 つ）／主実測を撃たずに `Landed` にする（前の周が実測の前に死んだ周を緑と読む・C10）。

## 30. 検出線（変異検査）は main の差分が検出線の面に触れた周だけ撃つ — 追随の再 gate と主実測の両方（契約表の行 x・`s2-07l.397`）

- 何が起きているか: admin の実測 2026-09-16 06:1xZ（本日の着地側 8 便を event log から集計）: Gated PASS 27 回・INCONCLUSIVE 12 回（§26 の穴）・rebase 15 回。rebase の 15 回は走行中の land の下で main が動いた周で、動かしたのは docs merge 26 本 + 着地 5 本。1 周ごとに再 gate（全件 + 変異検査）が走り、`.322` は Gated PASS 6 回で 2 時間 20 分未着地。user の観測（同日）「CPU に負荷がかかって温度が上がっている」の主因（1 周 ≒ 変異検査 75 分・gate-cost.md §5 の実測）。現物（verified・main e45019c）: `pipe/land.rs` の `follow_main` は rebase の後に `gate(&Gate { .. })` を撃ち、gate の `record_verify`（`pipe/gate/record.rs`）は写しの `detection_verify()` を**必ず**撃つ。主実測 `verify_main` は `same_tree`（verdict の `tree` = 便の HEAD の `^{tree}` **全体**と一致）の周だけ検出線を飛ばすので、docs だけの merge で main が動いた周も tree が変わり検出線を撃ち直す。動いた差分が変異検査の入力（source と歯・依存の pin）に触れていない周まで、変異検査 1 周を払っている。
- 形: **検出線の要否を「差分が検出線の面に触れたか」で決める 1 関数**（pure・`land.rs` か `gate` の隣・C2）: 閉じた path 集合 `DETECTION_SCOPE` = `crates/` 配下・`Cargo.toml` / `Cargo.lock`・`rules/` 配下・`.vessel.toml`（検出線の行の出所）。(i) **追随の再 gate**: `follow_main` が rebase の前に `git diff --name-only -z <base>..<main>` を取り、path が 1 つも `DETECTION_SCOPE` に無ければ `Gate` に「検出線を撃たない」印（閉じた enum の 2 値＝撃つ / 飛ばす〔理由 = 面の外〕・名は契約が持つ・`Gate` の field 1 つ・literal 構築点は `follow_main` と `gate_run` の 2 つで後者は常に「撃つ」）を渡し、`record_verify` は検出線の行を撃たず `verify.jsonl` に `kind=detection skipped=detection reason=outside-scope` の record を 1 本置く（主実測の `skip_record` と同じ形・`tree` の代わりに `reason`）。共通 verify（全件 nextest・clippy・xtask check・deny）と契約 verify は**従来どおり撃つ**（xtask check と契約表の歯は docs を読むので飛ばさない＝偽 PASS を作らない）。(ii) **主実測**: `same_tree` を「tree 全体の一致」から「`git diff-tree -r --name-only -z <gated tree> <landed tree>` の path が 1 つも `DETECTION_SCOPE` に無い」に改める（tree id どうしを diff-tree で比べる・一致の周は差分 0 で従来と同じ結果）。record は従来の `skipped=detection tree=<landed>` に `reason=outside-scope|same-tree` を足す。読めない周（diff が取れない・tree が無い・verdict に `tree` が無い）は**撃つ**（fail-closed・0 に読み替えない）。(i)(ii) は同じ 1 関数（path の列 → 要否）を通す。
- 触らない: 追随の要否判定（`old != base` なら rebase）・rebase と衝突の経路・共通 verify と契約 verify の行・検出線の行の中身と `{jobs}` の受付・verdict の 3 値と rc・`DETECTION_SCOPE` の外の変更（docs / design-intent / README / .github）が共通 verify で赤になる経路（従来どおり赤）。
- 却下案: docs だけの周は再 gate ごと飛ばす（xtask check の prose gate と契約表の歯 `contract_closure_ext_real_table_has_zero_findings` が docs を読む＝偽 PASS の経路・#249 の型）／`DETECTION_SCOPE` を rules 行にする（値でなく閉じた path の集合・variant の領分）／docs merge を止める（新契約の投入が遅れる・運用は Landed 直後に束ねる形〔planner 裁定 06:2xZ〕で別に手当て）／変異検査を着地の直前 1 回だけにする（gate の検出線を捨てる設計変更＝ADR-0021 §2.4 の改訂・本便の後に残る重さで判定）。
- **[gate-cost.md](./gate-cost.md) §44 が上書き**（[ADR-0060](../../design-intent/decisions/ADR-0060-detection-line-runs-after-landing-not-in-the-gate.html)・同 doc の契約表の行 ak / al / am の land 後・本節の本文は書き換えない）: 検出線の要否の判定は追随と主実測から消え、面の判定は §33 の再 gate の省略と着地後の検出の skip の 2 か所に残る。

## 31. flip-check の module 宣言の残し方に `<stem>/<name>.rs` の子を足す — 宣言 file の子 dir に置いた新規 module が落ちない（契約表の行 y・`s2-07l.410`）

- 何が起きているか: `.320` run 110459Z の QUESTION（2026-09-16 11:37Z・admin 実測 verified）。flip-check の base 段は、歯の diff だけを base に当てるとき宣言 file の `mod <name>;` のうち **base に本体が無い行を落とす**（§5.3・新 module の本体は head にしか無いので base では宣言だけが残り compile error になる型の回避）。その判定 `present_mods_only`（`crates/xtask/src/flipcheck.rs`）は宣言 file と同じ dir の `<name>.rs` と `<name>/mod.rs` しか探さない。Rust の規則では `tests/e2e/seat.rs` の子は `tests/e2e/seat/<name>.rs`（`<stem>/<name>.rs`）で、既存の `mod account;` 等は base に在るので carried で通るが、**新しい `mod statusline;` は本体が head の overlay に在っても落とされ**、その module の歯が base 段で走らず green-on-base で FAIL する＝`tests/e2e/<x>.rs` の子に新 module を置く便の全部に効く器の穴。
- 形: `present_mods_only` の探索に **`<宣言 file の stem>/<name>.rs`** を 3 つ目の形として足す（`dir/<stem>/<name>.rs`・stem = 宣言 file の拡張子を除いた名）。判定は「base に在る」でなく「overlay 先（`dest`）に本体が在る」の従来の意味のまま（3 形を or で見る）。`#[path]` 付きの module は従来どおり救済しない（§5.3 の M4 の記録のまま）。
- 触らない: flip の 3 段の順序・`carried` の読み（base の宣言）・`judge_each` の集合・`retroactive` の札・head 段。
- 歯（`flipcheck_declaration_nested_` 接頭辞・`crates/xtask/src/flipcheck_declaration_tests.rs`・既存の fixture〔`base_commit_with_e2e` / `red_body` / `green_body`〕の型）: 宣言 file `tests/e2e/seat.rs` の子（seat/ 配下の probe の module・新規）を足す diff で flip が RED-on-base ok を出す（base は宣言が落ちて歯が走らず green-on-base FAIL → RED）／同じ dir の `<name>.rs` と `<name>/mod.rs` の既存の 2 形は不変（既存の歯が緑のまま）／`#[path]` 付きは従来どおり測れない（既存の期待を変えない）。
- 却下: `.320` の新歯を `tests/e2e/seat.rs` の test 区間に置く（seat.rs の余地を食い、子 module の置き場を禁じる運用が散文に生まれる・N2）／`.320` の write-set に xtask を足す（seat と xtask を 1 便に混ぜる）／parser を足して `#[path]` も追う（A3 の依存・§5.3 で却下済み）。

## 32. flip-check の base-not-green に経路の弁別子を後置する — 負荷・環境・本物の赤を判定行で分ける（契約表の行 z・`s2-07l.380`）

- 何が起きているか: `.354` run 1（Gated FAIL）の verify は `cargo xtask flip-check` の 1 行だけ `FAIL reason=infra-error base-not-green` で、他の行は rc 0・main CI は同じ base で success。現物（verified）: `crates/xtask/src/flipcheck.rs` の `base_is_green` / `retry_named` は「名指せない失敗」を 3 つの経路——(i) base の nextest が rc を持たない（signal）(ii) rc≠0 で `failed_tests` が 0 本（compile error の rc 101・出力の形が読めない）(iii) 名指した歯の撃ち直しが rc≠0——で**同じ字面** `base-not-green` に倒す。撃ち直さないのは設計どおり（§5.3・C11.2）だが、操作役が負荷 / 環境 / 本物の赤を判定行から弁別できず、retire か run N+1 かを推測で決めている（C10: 測れない理由を潰さない）。
- 形: 理由を閉じた enum（3 variant・`as_str`・宣言順 = 上の (i)(ii)(iii)）で持ち、`infra` の字面に後置する: `base-not-green:signal` / `base-not-green:unnamed rc=<rc>` / `base-not-green:retry-failed rc=<rc>`。極性一覧の行（`infra-error`）は不変・判定行の先頭 `flip-check: FAIL reason=infra-error` も不変（後置だけ）。理由の出口は既存の `relay` / `sink` / 判定行で、新しい seam を足さない。
- 触らない: 撃ち直しの回数（1 回）と範囲（完全一致）・`failed_tests` の読み・head 段・`retroactive` の札・§31 の宣言の扱い。
- 歯（`flipcheck_base_reason_` 接頭辞・`crates/xtask/src/flipcheck_tests.rs`）: (i) 理由の純関数（rc の有無・名指した本数・撃ち直しの rc → variant）の 3 通りを in-file で pin（base では関数が無く RED）／(ii) 既存の toy fixture で base を compile error にした周の判定行が `base-not-green:unnamed rc=101` を含む（base の字面は `base-not-green` で終わる → RED）／signal と retry-failed は純関数の歯で足りる（実 signal の fixture は壁時計と環境に依る・§5.3 の型）。
- 却下: 経路ごとに別の `reason=` を立てる（極性一覧の行が増え infra-error の意味が割れる）／撃ち直しを 2 回に増やす（緩める側・C11.2）／stderr の relay だけに理由を書く（判定行を読む操作役に届かない・gate の evidence は判定行）。

## 33. 追随の再 gate を main の差分が検出線の面の外だけの周は省く — docs の merge ごとに先頭が 1 周払わない（契約表の行 aa・`s2-07l.416`）

- 何が起きているか（admin の実測 2026-09-16 12:50Z・13:25Z・verified）: 11:25Z 以降 85 分着地 0。列の先頭 `.389` は Gated PASS → 追随 rebase → 再 gate を 3 周し（main を動かしたのは docs-only の PR 5 本）、3 周目の再 gate で全件 nextest の 2 本 / 1490 本が負荷 flaky で落ちて Gated FAIL → retire＝実装 1 本を喪失。§30 は検出線だけを面の外で省いたが、共通 verify（全件 nextest ほか）と lens は差分の内容を見ずに毎周撃つ。同じ Rust の木に対して gate は前周で PASS 済みで、着地の直前には主実測 `verify_main` が最終の木で全行を撃つ（§5.4）＝再 gate の全件は二重。
- 形: `follow_main` は rebase の前に §30 と**同じ 1 関数**（`detection_needed`・`DETECTION_SCOPE`）で main の差分を測り、面に 1 つも触れない周は **再 gate を撃たず** `RunStage stage=Implemented detail=rebase:<old>..<new>` の直後に `RunStage stage=Gated detail=verdict:PASS` を器が記帳して着地へ進む（前周の PASS を新 base へ引き継ぐ・verdict の 3 値と detail の形は不変）。引き継いだ事実は `verify.jsonl` に §30 の `skip_record` と同じ形の record 1 本（`kind=gate skipped=regate reason=outside-scope`）で残す（C10）。`skipped=` の値は `pipe/gate/record.rs` に閉じた 2 値（`detection` / `regate`）で、`reason` は既存の `DetectionSkip::OutsideScope` のまま＝`pipe/gate.rs` の enum に variant を足さない（`Skipped` の構築点は `land.rs` の主実測と `record.rs` の gate の 2 つと `tests/e2e/pipe/land.rs` の歯＝行 aa の write-set の中に閉じる）。面に触れる周・diff を読めない周は従来どおり再 gate（fail-closed）。主実測 `verify_main` は従来どおり最終の木で全行を撃つ（push の前の唯一の全件・C12.6 の緑はここが担う）。
- 触らない: 追随の要否判定（`old != base` なら rebase）・rebase と衝突の経路（pipeline-conflict.md §3）・`DETECTION_SCOPE` の中身・gate の判定順と verdict・主実測の行・`gate_run`（`pipe gate` を人が撃つ周は常に撃つ）。
- 歯（`pipe_follow_docs_only_` 接頭辞・`tests/e2e/pipe/land.rs`・既存の追随の fixture〔`gated_pass` + 別便の commit + 偽 lens〕の型）: (a) main が docs だけの commit で進んだ周は land が偽 lens を呼ばず（写し 0）verify の record が増えず、`Gated verdict:PASS` の record と `skipped=regate reason=outside-scope` の record が在って着地する／(b) main が `crates/` の file で進んだ周は従来どおり再 gate（偽 lens 1 回・既存の歯）。diff を読めない周の fail-closed は既存の `follow_detection`（変更しない）が持ち、FR34 の前提（base が main の祖先）を通した上で diff だけを失敗させる seam が無いので歯は置かない（空虚な歯を避ける）。record の形（`Skipped` / `skip_record`）の定義は `pipe/gate/record.rs` に閉じる。既存の歯の閉包は `tests/e2e/pipe/land.rs` の「main を便の base から動かす fixture」に加えて `tests/e2e/pipe/gate.rs` の `pipe_confine_release_regate_in_one_process_uses_distinct_unit_names`（repo 直下の `other.txt` を別便の変更として置き、追随の再 gate と主実測の 2 周が別名の unit で撃たれたことを数える）も含む＝その fixture の path は面の外なので本節の形で再 gate が省かれ 1 周になって反転する。歯の趣旨（2 周の unit 名が異なる）を保つため fixture を `crates/other.txt`（面の内）に替え、期待は不変＝write-set はこの e2e file を含む（run 4 = QUESTION 2026-09-17 の解）。閉包の母集団は pipe の e2e 全 file（`tests/e2e/pipe.rs` + `tests/e2e/pipe/*.rs`）で「便の base の後に main へ commit を積んでから land を撃つ fixture」を掃いたもの＝`land.rs` の 7 本と `gate.rs` のこの 1 本だけ（他の file は main を動かさない・`gate.rs` の他の再 gate の歯は同じ base で撃ち直すだけ）。
- 却下: docs-only の周は主実測も省く（push の前に最終の木で全行を撃つ唯一の線が消える・C12.6）／共通 verify のうち docs を読む行だけ撃つ（行の意味を字面で分類する散文規則・N2）／docs merge を止める運用だけで凌ぐ（planner 裁定 12:5xZ の暫定・器に無い規則）。

## 34. 追随で入った契約表の行が便の消した path を名指す周は runner を起こし直す — Gated のまま誰も直せない穴を衝突と同じ経路で塞ぐ（契約表の行 ab・`s2-07l.400`）

- 何が起きているか（`s2-07l.349` run 010919Z・`.288`・2026-09-16・verified）: 純移動の便（e2e の lifecycle.rs を ratelimit.rs / stop.rs へ割る）が Gated PASS を 4 回通した後、追随の rebase で docs PR の行 v（`.395`）が便の木に入り、その write-set が便の消した file を名指したまま。契約表の検査の歯 `contract_closure_ext_real_table_has_zero_findings` が便の木で赤（write-set-item-unresolved）→ 変異検査の baseline が落ちて検出線 rc 2（測れない）→ Gated INCONCLUSIVE を 5 回繰り返し、待ち手の back-off が尽きた。穴は 2 つ: (1) 追随の rebase は木を動かすが runner を呼び戻さない＝行と便の食い違いは Questioned でないので answer も効かず、Gated のまま誰も直せない。(2) 検出線の rc 2 は「測れない」であって便の赤ではないのに、resume は同じ検出線だけを撃ち直す（原因は木に在る）。§33 の後は docs だけの周の再 gate が省かれるので、同じ食い違いは主実測 `verify_main`（§5.4）の赤＝main-red の記録へ移るだけで、直す手は依然無い。
- 現物（planner が grep で実測・main 0b7e0a1）: 契約表の検査の口は `crates/scribe2/src/pipe/table/check.rs` の `check_repo`（`pub(crate) fn`・`contracts check` と同じ 1 本）だが、**返すのは描画済みの `Outcome` で findings の列ではない**。findings の列を返すのは同 crate の `judge_doc`（`crates/scribe2/src/pipe/table/check.rs` の私有 `fn`）で、1 件の型 `Finding`（`crates/scribe2/src/pipe/table.rs` の `pub struct`）は `line` だけが `pub`・理由の `refuse`（`Refuse::WriteSetItemUnresolved { item }` を持つ）は**私有 field**＝呼び手から未解決の項目の path を読めない。追随の入口は `crates/scribe2/src/pipe/land.rs` の `follow_main`（私有 `fn`）で、衝突の回数と resume の弁別は `crates/scribe2/src/pipe/follow.rs`、stdin の節は `crates/scribe2/src/pipe/spawn.rs`、resume の読み手は `crates/scribe2/src/pipe/cli/resume.rs` に在る。
- 形（衝突の機械解消 [pipeline-conflict.md](./pipeline-conflict.md) §3 と同じ経路・新しい経路を持たない）: (1) `follow_main` は rebase が通った直後・§33 の省略の判定と再 gate の**前**に、契約表の検査を便の木に撃つ（`check_repo` と同じ入力・同じ検査で、未解決の項目の path を呼び手が読める形を 1 つ足す＝`Finding` の読み口 1 本。検査そのものと `contracts check` の出力の字面は変えない）。findings が 0 なら従来どおり。(2) findings が在り、そのすべてが write-set の項目の未解決で、名指された path が**便自身の diff で消えた・改名した path**（`git diff --name-status <base>..HEAD` の D / R の旧 path・写しの write-set の `-` の項目とは別の実測）に含まれる周は、`RunStage stage=Implemented detail=rebase-stale-rows:<base>..<main>` を記帳し（終端にしない・§3 の手順 2 と同型）、runner を起こし直す（同じ worktree・同じ契約・stdin の「追随」節に行の一覧〔`<doc>#<id>` と未解決の項目〕を足す・`spawn.rs` の節の出所は `follow.rs` の `section` の 1 本のまま）。回数は衝突の回数と**同じ 1 つの上限**（rules 行 `pipe.follow_retries`・`is_conflict` の読み手を `rebase-stale-rows:` の接頭辞も数える 1 本にする・resume の弁別も同じ 1 本）で、上限に達した周は `Failed detail=rebase-stale-rows`。(3) 起こし直しの turn が行を直せるよう、写しの write-set（run dir の `contract.toml`・`contract_path`）に findings の行を持つ設計 doc（`docs/design/<doc>.md`）を器が**追記する**（追記だけ・既存の項目は動かさない・追記した項目は同じ event の stderr の行に写す＝gate の照合と runner の guard が同じ写しを読むので食い違わない・`.133` の「契約の改訂を器の口で持つ」の最小形）。(4) それ以外の findings（便が消していない path・行の形の誤り）は便の責任ではない＝従来どおり再 gate へ進み、赤なら gate の判定で止まる（本行は「便が消した path を名指す行」だけを拾う・fail-closed の向きは変えない）。契約表の検査を撃てない周（repo を読めない）は従来どおり再 gate（読めないを「行なし」に読み替えない・NFR4）。
- 触らない: 契約表の検査そのものと `contracts check` の出力の字面（足すのは未解決の項目の path を呼び手が読める読み口 1 本だけ）・追随の要否判定・rebase と衝突の経路・§33 の省略の判定（本検査はその前に撃つ）・検出線の rc 2 の扱い（穴 (2) は原因を木から取り除くことで到達しなくなる・resume の形は不変）・純移動の便が契約時に他の行を直す義務（contract-source.md §15 の型・本行は契約の後に入った行だけを拾う）。
- 歯（接頭辞 `pipe_follow_stale_rows_`・置き場は `crates/scribe2-boundary/tests/e2e/pipe/land.rs`・既存の追随の fixture〔`gated_pass` + 別便の commit + 偽 runner〕の型・base の当たりは 0 本）: (a) `pipe_follow_stale_rows_restarts_` = 便が file を消した後、main が「消えた path を write-set に持つ行」を足す docs の commit で進んだ周は、land が `Implemented detail=rebase-stale-rows:` を記帳して偽 runner を 1 回起こし、写しの write-set にその設計 doc が追記され（追記は末尾 1 項目・既存の項目は順序も字面も不変）、再 gate は撃たれない（偽 lens の写し 0）／(b) `pipe_follow_stale_rows_unrelated_` = 行が便と無関係の path を名指す周は起こし直さず従来どおり再 gate へ進む（契約表の検査を撃てない周も同じ側へ倒れる）／(c) `pipe_follow_stale_rows_exhausted_` = 上限 `pipe.follow_retries` に達した周は `Failed detail=rebase-stale-rows` で終端する／(d) `pipe_follow_stale_rows_no_runner_` = `--runner` の無い land は `rebase-stale-rows:` を記帳して rc 1 で止まり resume で続けられる（§3 の手順 4 と同型）。
- 却下: 器が行を書き換えて着地する（land が write-set の外の doc を触る＝gate の照合と runner の guard の外の変更・C16）／stale な行を INCONCLUSIVE の理由の 1 つとして記帳するだけ（記帳は在っても直す手が無い・穴 (1) そのもの）／検出線の rc 2 の周に resume が全 verify を撃ち直す（原因が木に在る間は何回撃っても同じ・費用だけ増える）。

## 35. main 実測の赤に落ちた歯の名と panic の抜粋を残す — record に `failed=`・落ちた歯ごとの stderr の区間（契約表の行 ac・`s2-07l.401`）

- 何が起きているか（admin 実測 2026-09-16 07:18Z `.164` run 051333Z・14:2xZ の 3 便比較・verified）: land の主実測（§5.4・`verify_main`）で全件 nextest が rc 100 → `Failed detail=main-red`（push なし・main 無傷＝止め方は正しい）。record（`verify-main.jsonl`・gate の `verify.jsonl` と同じ `records_of` の形）は行ごとの `rc` と stderr の末尾 `STDERR_TAIL_LINES` 行（20）を持つので、落ちた歯の名（nextest の Summary の後の `FAIL [` 行）は残るが、**panic の本文（assert の文）は落ちた歯の実行位置が末尾に入る周だけ残る**（`.389` は在る・`.323` は 441/1490 と 642/1490 の位置で無い）。原因の切り分け（rebase の相互作用か flaky か）を admin が手で撃ち直して探した。
- 形（gate の verify 行と主実測の**同じ 1 本**・`pipe/gate/record.rs`）: (1) record の head に `failed=<歯の名>` を 1 つ足す（nextest の stderr の `FAIL [` 行の最初の 1 本・無い周は書かない・pure な抽出関数 1 本・in-file の歯）。(2) stderr の写しは末尾 N 行に加えて、**落ちた歯ごとの区間**（nextest は落ちた歯ごとに即時の `FAIL [ … ] <歯の名>` の進捗行の後へ小見出し `stdout ───` / `stderr ───` を出す〔0.9.143〕。区間 = その `stderr ───` の小見出しから次の進捗行（`PASS [` / `FAIL [`）か Summary の直前まで・歯の名は直前の `FAIL [` の行から取る・歯 1 本あたり `STDERR_TAIL_LINES` 行を上限・落ちた歯が複数なら順に・字面は cargo-nextest 0.9.143 の出力を gate の実 log で実測したもの）を残す＝末尾の N 行に panic が入らない位置の歯でも本文が残る。区間の切り出しは nextest の字面の閉じた 2 形（`FAIL [` の進捗行 / `stderr ───` の小見出し）だけを読む pure な関数で、他の verify 行（clippy 等）は従来どおり末尾 N 行だけ。(3) gate の `verify.jsonl` と主実測の `verify-main.jsonl` は同じ関数を通る（片側だけに足さない・C2）。写しの診断 file も対で持つ: gate は既存の `verify.stderr.log`、主実測は同じ dir に同じ形で `verify-main.stderr.log`（`verify-main.jsonl` と同じ stem・機械は読まない・人が読む）。主実測の record を書く `record_main` は `.457` の純移動で `pipe/land/verify.rs` に在る（`pipe/land.rs` は `MainCheck` の読み手だけ・2026-09-22 の現物）ので、行 ac の write-set は両方を持つ。
- 触らない: `MainCheck` の 3 値と `main-red` の極性（auto revert しない）・record の `n` / `rc` / `cmd` の形・`STDERR_TAIL_LINES` の値・stdout の扱い。
- 歯（`pipe_verify_failed_` 接頭辞・`pipe/gate/record.rs` の in-file の pure な歯 + `tests/e2e/pipe/land.rs` の既存の main-red の fixture の型）: nextest 形の stderr（Summary の後に `FAIL [` 2 本・各歯の `stderr ───` の区間・落ちた歯が末尾から遠い位置）から `failed=` が最初の 1 本を指し、区間が歯ごとに上限行数で残る／`FAIL [` の無い stderr は `failed=` を持たず末尾 N 行だけ／main-red の便の `verify-main.jsonl` に `failed=` と区間が載る（e2e）。
- 却下: 末尾の行数を増やす（歯の数に比例して膨らみ、位置の問題は残る）／nextest の JSON 出力を読む（出力形式の依存が 1 つ増え、共通 verify の行の字面を器が縛る・ADR-0010 の宣言の外）／runner の stdout に写す（主実測は runner が居ない）。

## 36. 着地の列が driver の死んだ便を先頭に数えない — 札の所有者が死んだ便を `skipped-dead` で外し、段は動かさない（契約表の行 ad・`s2-07l.388`）

- 何が起きているか（admin の実測 2026-09-16 04:0xZ・verified）: 着地の列（`pipe/queue.rs` の `turn_in`・`gated_at` 順・[gate-cost.md](./gate-cost.md) §6）の先頭の便が host の再起動で driver（`pipe run` の process）ごと死に Implemented のまま止まると（Gated PASS の判定と worktree は在る）、後続の便は `await_turn` で先頭が退くのを rules 行 `pipe.land_wait_s` の上限（90 分）まで待ち `unmeasured` に倒れる＝再起動・oom・stop の失敗のたびに「上限 × 後続の本数」を失う。現物（main a620600）: `turn_in` は自分より鍵の小さい PASS の便を「終端でない ∧ Gated を 1 度通った ∧ worktree が在る」で数え、その便を進める者が生きているかを見ない。driver の生死は [dispatcher.md](./dispatcher.md) §5（行 d・`s2-07l.352`）の**札**（`<state_dir>/pipe/<run>/driver`・pid + 起動時刻・lock の所有者と同じ probe・`Owner::Dead`）が typed に持つ＝本 § はその読みを列に足すだけで、生死の判定を 2 本にしない（C2・C3.3）。
- 現物（orchestrator が grep で実測・main 587546f）: 行 d の札の読み手は `crates/scribe2/src/pipe/mod.rs` の `driver_ticket`（`pub fn`・閉じた 4 値 `Ticket` = `Absent` / `Dead` / `Live` / `Unreadable`・lock の所有者と同じ 1 本の probe・行 d は `s2-07l.482` で Landed）で、`driver_is_dead` はその `Dead` だけを真にする薄い読み手。列の材料 `Queued` は `crates/scribe2/src/pipe/queue.rs` の `queue_from` が replay から組む（`verdict` は `may_queue` の便だけ読む）。`order=` の字面は同 file の `Order`（`pub(super)`・4 値・`Copy`）が持ち、stdout の行と面 5 の行はどちらも `crates/scribe2/src/pipe/land/finish.rs` の `finish`（`export_verdict` が `verdicts.jsonl` へ `order` の key を書く）が出す。`await_turn` の呼び手は `crates/scribe2/src/pipe/land.rs` で、`Order` の値を `finish` へ渡す。
- 形: (1) `Queued` に field を 1 つ足す（driver の札の状態・型は `crate::pipe::Ticket` を `Option` で包んだもの＝列に入りうる便〔`may_queue`〕だけ読み、他は `None`・`verdict` と同じ規則・新しい enum は作らない）。埋めるのは `queue_from` で、読み手は行 d の `driver_ticket`（`crates/scribe2/src/pipe/mod.rs`）の同じ 1 本（可視性は変えない・第 2 の probe を作らない・C6.3）。(2) `turn_in` は札が `Dead` の便だけを `ahead` の候補から外す。`Live` / `Absent` / `Unreadable` / 読んでいない（`None`）は従来どおり数える（測れないを「死んだ」に読み替えない・fail-closed・行 d の「札の無い便は触らない」と同じ極性）。自分の便の札は見ない（自分は生きている）。外した便の id の列（鍵の順）は `turn_in` と同じ 1 本の pure な選別が返す（同じ条件を 2 か所に書かない）。(3) 外した便を黙らせない（C10）: `Turn` の 3 値と `Order` の 4 値は不変で、`await_turn` は `Order` と外した便 id の列を 1 つの struct（`queue.rs` に新設・field は order と skipped_dead の 2 つ・`Copy` を持たない）で返し、`crates/scribe2/src/pipe/land.rs` の呼び手はそれを `finish` へ渡す。`finish` は面 5（`export_verdict` の行）に任意 field `skipped_dead`（外した便 id の列・鍵の順・外した周だけ・既存の key 列の後ろ・ADR-0021 §2.6 (iv)）を足し、stdout の `order=` の値の直後に `skipped-dead=<n>` を後置する（0 の周は書かない）。(4) 外すだけで段は動かさない（死んだ便は Implemented / Gated のまま・resume 可・N1）。起こし直しは行 d（dispatch の turn 関数の `pipe resume`）の領分＝本 § は列の側だけ。
- 触らない: `Turn` の 3 値と `Order` の 4 値・列の鍵（最初の `Gated` の ts）・`may_queue` の条件・`pipe.land_wait_s`・札の書き・消し・probe（行 d）・起こし直しの上限と間隔（行 d の側で rules 行か閉じた定数）・追随で Implemented に戻った便が PASS のまま列に残る規則（仕様）。
- 歯（`pipe_order_dead_` 接頭辞・`tests/e2e/pipe/land.rs` の `pipe_order_` の隣・in-file は `queue.rs` の `turn_in` の pure な歯）: 先頭の便の札の pid を死んだ process（`sh -c true` を wait した pid）にした fixture で、後続の `pipe land` が `first` で進み `order=first skipped-dead=1` と `verdicts.jsonl` の `skipped_dead` にその便 id／先頭の札が生きている周は従来どおり `After`／札の無い先頭は従来どおり待つ（`After`）／死んだ便の段と worktree は不変（`pipe show` の段が動かない）／pure: 3 値 × 鍵の順の表で `ahead` の選び方が変わらない。
- 却下: `pipe.land_wait_s` を短くする（gate の所要が長い便で偽の `unmeasured`）／席の pid や pane で生死を測る（driver は席ではない・C3.3・札 1 本で足りる）／死んだ便を列から外すと同時に Stopped へ倒す（成果を捨てる・N1・起こし直しは行 d）／admin の daemon で先頭を監視する（散文の運用・器の列の外）／札の無い便も死んだと読む（行 d の前の便や読めない周を全部外す＝fail-open）。

## 37. flip-check が歯の外の行だけ動いた test file を単独で撃たず本体の木へ同梱する（契約表の行 ae・`s2-07l.450`）

- 何が起きているか: admin の実測 2026-09-17（母集団 = 本日の Gated FAIL）で 3 便が `green-on-base` で gate 1 周を失った（`s2-07l.447` ×2・`s2-07l.412` ×1・実装完了から判明まで 26〜60 分）。flip-check は flip した file を **1 本ずつ単独で** base へ重ねて RED を求める（`flipcheck.rs` の `judge_each`）が、その file の差が **歯の外**（helper・共有の fixture・module の宣言以外の作り）の行だけのときは、単独 overlay が base の歯を 1 本も動かさず必ず緑になる＝**構造的に RED になりようがない差に RED を要求している**。`s2-07l.412` の根は `tests/e2e/pipe.rs` の fake lens の 4 行で、現物の `fake_lens` / `lens_verdict` は `pub(super) fn` の helper＝歯の外である（この file は歯を 22 本持つので「歯 0 本の file」では捕まらない・母集団 = `crates/*/tests/` 配下の `.rs` 22 本のうち歯 0 本の file は 0 本）。同じことを新規 module の宣言 file については既に言っている（§5.3「宣言 file の同梱」＝単独 overlay ではどちらの判定も意味を持たない）が、弁別が `mod x;` の字面に閉じているので helper の行には効かない。
- 形: flip した file のうち、**その便で動いた行**（`changed_lines` が返す片側にしか無い行・空白だけの行は数えない）が **1 本も歯の中に無い** file を「歯の外の file」と呼び、宣言 file と同じ側＝**単独では撃たず本体を撃つ木へ同梱**する（判定行に `fixture=N` を後置・stderr に `not-flipped reason=outside-teeth <rel>` を 1 行）。「歯の中」は **`#[test]` の直下の `fn` の宣言行から次の `fn` の宣言行の手前まで**（属性・doc・空行は跨ぐ・`test_fns` と同じ「直下の fn」の読み）で、**変更行の字面が base 側か HEAD 側のどちらかの歯の中に 1 度でも現れれば歯の中**と読む（`}` のように重複する字面は歯の中へ倒れる＝同梱は判定を緩める側なので弁別は狭く取る・brace を数える parser は足さない）。**歯の外の file しか flip していない便は従来どおり単独で撃つ**（`plan_of` が宣言 file にしている落とし方と同じ＝本体が 1 本も無ければ `green-on-base` のまま落ちる・fail-closed）。同梱は本体 1 本を撃つ turn ごとに置き、`mod` 行の絞り込み（§31）は宣言 file の側のままである。
- 触らない: 単独 overlay の骨（`judge_each` の 1 本ずつ・「どれか 1 本が赤い」へ緩めない）・`removed_only` の部分列・`retroactive` / `moved` の札と効く 4 条件・`present_mods_only` の 3 形・base 段の撃ち直しと `base-not-green` の弁別子・判定行の 3 形と FAIL の 4 語・極性一覧の行・gate の段の順序。
- 歯（`flipcheck_fixture_` 接頭辞・`crates/xtask/src/flipcheck_overlay_tests.rs`・既存 fixture〔`base_commit` / `write_at` / `head_commit` / `judge` / `assert_verdict`〕の型）: (a) 歯の外の 1 行だけを動かす file と新しい歯を足す file の 2 本を持つ diff が `RED-on-base ok` を出し判定行に `fixture=1` が載る（base は前者を単独で撃って `green-on-base file=…` で落ちる → RED）／(b) 負例 = 歯**の中**の 1 行（base でも通る前提の値）を動かす file は従来どおり単独で撃たれて `green-on-base` で落ち、判定行に `fixture` の後置が付かない／(c) 歯の外の file しか動かない便は `green-on-base` のまま落ちる（同梱の fail-closed な落とし方）。
- 却下: 便に 1 本でも RED が在れば他の file の緑を通す（`judge_each` が塞いだ当の fail-open で、flip の意味は「どれか 1 本が赤い」ではない）／file の種別を閉じた enum へ畳む（現物は述語の組で、畳むと判定の全面書き換え＝S の便が L になる・C4）／歯の中の前提の値（`s2-07l.447` の型）まで救う（前提と期待は字面で弁別できない＝解は契約の側〔弁別の歯を 1 本足す〕と、入口の測定を実装役の完了判定の前へ寄せる形〔別便〕に残る）。

## 38. 追随の形が無い便（base が main の祖先でない）を merge-base からの rebase --onto で追随する — merge-base が無い周だけ stale base で断る（契約表の行 af・`s2-07l.449`）

- 何が起きているか（admin 実測 2026-09-17 07:2xZ・verified・`s2-07l.449` の 2 面目）: 着地中の便が squash を local main に積んで主実測を回している間に docs PR の merge が origin/main へ載り、local と origin が分岐した。回復で local main を origin へ揃えた後、その未 push の squash を base に追随済みだった便（Gated PASS・base が消えた squash）の base が main の祖先でなくなり、`pipe land` が `stale base` で 8 周断り、後ろの便が着地の順番待ちで止まった（#297 と同型）。現物: `pipe/land.rs` の `follow_main` は `git merge-base --is-ancestor <base> <main>` が偽なら `stale base` の refused で何も書かない。§18 の周回はこの断りを `stale:` として記帳し同じ経路へ戻すが、祖先でない base は何周しても解けない（撃ち直しの回数だけ減る）。1 面目（窓が主実測中の squash を数えない）は §19 の窓の 3 つ目の条件で塞ぐ。
- 形: (1) `follow_main` の祖先検査を閉じた 3 値にする（宣言順 = base が main の祖先／祖先でないが `git merge-base <base> <main>` が 1 つ在る／merge-base が無い・読めない）。置き場は `follow.rs`（`follow_main` と、起こし直しの stdin に「追随」節を出す `section` が**同じ 1 関数**で読む＝`section` は 2 つ目の周にも main を返す。現物の `section` は `is-ancestor` が偽なら節を出さないので、起こし直しの runner が --onto の周に追随の指示を受け取らない穴が在る）。(2) 2 つ目の周は worktree の branch に `git rebase --onto <main> <base>` を撃つ＝便が記録した base の上に積んだ commit だけを main の上へ運ぶ（merge-base から base までの消えた commit は運ばない・main は 1 byte も動かさず force 系は使わない・N1）。衝突は既存の衝突の経路（`follow.rs` の `on_conflict`・`rebase-conflict:` の記帳・起こし直し・上限）へ合流するが、起こし直しの「追随」節は main と便の base の **2 sha** を名指し、runner への指示（`pipe/spawn.rs` が描く節の値〔`Launch` の追随の相手を main の sha から main と base の 2 sha に広げる〕と `headless/runner.txt` の雛形）も `git rebase --onto <main> <base>` の形にする（祖先である周も同じ形＝結果は `git rebase <main>` と同じ・経路を 2 本にしない・runner が消えた commit を運ばない）。成功は既存の `rebase:<base>..<main>` の記帳（`base_of_run` が新しい側を読む＝新しい base は main）・既着地の判定（§29）・再 gate の要否（§30 の検出線の面）へ**合流する**＝以後は従来の追随と同じ 1 本で、rebase の呼び方が 1 語違うだけ。(3) merge-base が無い・読めない周だけ従来の `stale base` の断り（字面不変・何も書かない・fail-closed）。(4) 記帳の detail と stdout の `rebase=` の形は不変（何を onto したかは範囲の 2 sha で読める）。
- 触らない: CAS と「撃ち直しの間に main が動いた」の断り・§18 の周回・§29 の既着地・衝突の回数の上限（`pipe.follow_retries`）・`retire` の前提・`--pr-cmd` 形（stale base を見ない）・§19 の窓。
- 歯（`pipe_land_onto_` 接頭辞・`tests/e2e/pipe/land.rs`・既存の追随の fixture〔`pipe_land_rebase_` の型〕で main を base の親から別の commit で作り直す）: (a) base が main の祖先でなく merge-base が在る便の land が rebase --onto で追随して `Landed`（squash の tree は便の commit だけを運ぶ・記帳は `rebase:<base>..<main>`・base は `stale base` の rc 1 で event 0 増 → RED）／(b) 同じ形で衝突する周は既存の `rebase-conflict:` の記帳と起こし直しの経路（字面不変）で、起こし直しの stdin の「追随」節が main と base の 2 sha を持ち、`--onto <main> <base>` で rebase を通す stub の runner は消えた commit を運ばずに `Landed`（`--runner` 無しの周は既存の rc 1）／(c) merge-base の無い main（無関係な歴史）は従来どおり `stale base` の rc 1・event 0 増（極性不変）／(d) 追随の後の `base_of_run` が main を返し、再 gate の要否は §30 の判定のまま（差分が検出線の面の外なら省く）／(e) runner の prompt の雛形（`tests/e2e/headless.rs`・外形 snapshot `headless_runner_prompt_external_form`）が `--onto` の形で追随を命じ、祖先である周の「追随」節も同じ 2 sha の形（既存の追随の歯は期待を変えない）。
- 却下: land の外で人が branch を rebase する（.432 で 8 周・撃つ主体が席に残る）／器が local main を origin へ揃える（main を動かす側・N1・回復は人の手番のまま）／merge-base を新しい base として記帳する（gate の diff に main の commit の逆向きが載る・pipeline-conflict.md §3 手順 5 と同じ穴）／`stale base` の断りを全部 --onto に置き換える（無関係な歴史へ運ぶ・fail-closed を保つ）。

## 39. pipe stop --run が段を問わず便を終端にする — Stopped の後の段の記帳を記帳の口が断り、運転手の process を札で止める（契約表の行 ag・`s2-07l.437`・[dispatcher.md](./dispatcher.md) 行 d の札の後）

- 何が起きているか（admin 実測 2026-09-17 05:5xZ・母集団 = 同じ周の stop 2 本・2 本とも再現・verified）: `pipe stop --run` は runner の席（process group）だけを止めて `RunStopped` を書き、review / gate / land の段を運んでいる運転手（`pipe run` の process・`cli/run.rs` の `run_all` が段を 1 process で連続させる）を止めない。運転手は止まった便の次の段を書く: .430 run 055209Z は Intake で stop（seats=0）した後も審査 → `Spawned` まで進み（再 stop で runner を止め、器は停止の signal を `Failed oom-kill` と記帳＝行 q の誤分類）、.289 run 052948Z は Implemented で stop した後も gate を続け `Gated` → land で `Stopped` を上書きし write-set の面を握り続けた（運転手を手で TERM）。現物: run の event を書く口は `pipe/mod.rs` の `emit` 1 本（`fleet/store.rs` の `append` が lock の中で追記する）で、段の関数は `Stopped` を読まない（読むのは `queue.rs` の `may_queue`・`cli/state.rs`・`retire` の前提だけ）。
- 形（判定は typed・段の関数に読み手を増やさない・C2）: (1) **記帳の門**: `fleet/store.rs` に「lock の中で述語を評価して偽なら書かない」条件付き append の口を 1 つ足し（述語は閉じた enum の値＝`NotStopped { run }`・自由な closure は受けない・lock の外で読んだ値との race を塞ぐ）、`pipe/mod.rs` の `emit` は kind が `RunStage` / `RunDone` / `SeatSpawned` の周だけこの口を通す＝その run の最後の run event が `RunStopped` なら書かずに `StoreError` の新しい variant `Stopped` で断る（呼び手は既存の Err の経路＝rc 2 と stderr 1 行で止まる・運転手はそこで終わる・`chain` が後段を撃たない）。読めない周は書く側に倒さない（既存の `Malformed` / `Io` の断りのまま・fail-closed）。`RunStopped` 自身と `SeatStopped` は門を通さない（停止の記帳を停止が塞がない）。所在: `StoreError` の定義と網羅 match は `crates/scribe2/src/fleet/store.rs` の 2 か所（`Display` の impl と `as_str`）だけで、他 module の呼び手（`pipe/report.rs` / `stop.rs` / `ratelimit.rs` / `cli/step.rs` / `cli/state.rs` / `cli/resume.rs` / `cli/intake.rs`）は `to_string` で文を写して rc は `RC_BROKEN` 固定＝variant を読まないので、`Stopped` を足しても write-set の外は変わらない。event の型は `crates/scribe2/src/fleet/event.rs` の `Event`（`seat` は `Option<String>`・`seat=driver` は既存の型に載る・file は触らない）。(2) **運転手の停止**: `pipe stop --run` は席を止めた後、札（dispatcher.md 行 d・`<state_dir>/pipe/<run>/driver`・pid + 起動時刻・`Owner` の probe）が生きた運転手を指す周は、その process group を席と同じ 1 関数（`terminate`・猶予は `pipe.stop_grace_ms`・TERM → wait → KILL）で止め、`SeatStopped` と同じ形の記録（`seat=driver` の 1 行・detail に `stopped-by-stop`）を残す。札の pid が**自分自身**（`pipe run` の中から stop を撃つ形）の周は止めない。札が無い・読めない・死んでいる周は止めない（測れないを「止めた」に読み替えない）。止め切れなかった周は従来どおり `RunStopped` を書かず rc 1。所在: `terminate` は `crates/scribe2/src/pipe/stop.rs`（席を止める既存の 1 関数・引数は pid と猶予 ms）、札の probe は `crates/scribe2/src/fleet/store.rs` の `Owner`（`lock_owner`・Dead / Live / Unreadable の 3 値）で、`pipe/mod.rs` の `Ticket` がそれを写す（既存の読み手を使い回す・`Owner` の値は増やさない）。(3) 段の順序は不変: 席 → 運転手 → `RunStopped`（`RunStopped` は最後＝書けた時点で live から外れる・§5.6 の極性）。
- 触らない: `pipe stop --all`（席の掃除・運転手は止めない）・`pipe.stop_grace_ms` の値・`Stopped` の便の `retire`（pipeline-conflict.md §5）・`queue.rs` の `may_queue`・oom の分類（行 q・`s2-07l.340`）・`resume`（`Stopped` は終端＝再開の口は無いまま）・札の書き・消し（行 d）。
- 歯（`pipe_stop_driver_` 接頭辞・e2e は `tests/e2e/pipe/stop.rs`・偽の運転手 = `sleep` の process group の pid を札に書いた fixture）: (a) in-file（`pipe/mod.rs` の tests）`RunStopped` の後に `RunStage stage=Gated` を `emit` すると `StoreError` の `Stopped` で断られ event log の byte 数が不変（base は書く → RED）・`RunStopped` / `SeatStopped` は書ける／(a′) e2e の race: verify を `sleep` にした gate を子 process で走らせ走行中に `pipe stop --run` を撃つと、gate は `Gated` の記帳で断られ rc 2・event log の最後は `RunStopped` のまま（札の無い gate は止められず門だけが効く形）／(b) `pipe stop --run` が札の pid の group を止め `seat=driver` の記録を残す（base は生きたまま・記録 0 → RED）／(c) 札の pid が stop を撃つ process 自身の周は止めずに `RunStopped` を書く（fixture は `sh -c` で `$$` と起動時刻を札に書いてから同じ shell で `exec` して stop を撃つ＝札の pid == stop の pid）／(d) 札が無い・死んでいる周は席の停止と `RunStopped` だけ（従来と同じ event 列）／(e) TERM を無視する偽の運転手（`trap '' TERM` の sh）は猶予の後の KILL で止まり rc 0（席の既存の歯と同型）／(f) in-file（`fleet/store.rs` の tests）: 条件付き append は lock の中で述語を評価し、偽の周は file が 1 byte も変わらない。
- 期待が変わる既存の歯（形 (1) と (2) の帰結・runner の問い 2026-09-21 を orchestrator が §39 の字面と現物で再現・行 ag の write-set に 2 file を持つ理由）: `pipe_ratelimit_resume_stop_breaks_the_wait`（`crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs`）は札を握る `resume` を test の子として起こし `pipe stop --run` の後に**自力の** rc 1 と「段は Stopped である」を assert するが、形 (2) では stop がその process group を止めるので、期待は「止められた終了（自力の終了 code を持たない）・`seat=driver` の記録 1 行・`RunStopped` 1 件・起こし直し 0」に替える。`pipe_follow_unreadable_retry_count_fails_closed_with_rc_two`（`crates/scribe2-boundary/tests/e2e/pipe/land.rs`）は壊れた行を混ぜた event log の**後に** `Failed detail=follow-unmeasured` の行が書かれることを assert するが、形 (1) の門は読めない周に書かない（fail-closed）ので、期待は「rc 2 は不変・最後の行は壊れた行のまま（`Failed` は書かれない）・main は動かない」に替える。どちらも歯の名と本数は変えない（assert の期待だけ）。
- 却下: 各段の関数の先頭で `Stopped` を読む（読み手が段の数だけ増え、新しい段を足すたびに漏れる・C2）／stop が state dir に印の file を置いて段が読む（記帳と別の状態・C3）／運転手を殺すだけで記帳の門を持たない（殺す前に書かれた event と race する・.289 の型）／記帳の門だけで運転手を殺さない（gate の verify が走り切るまで CPU と worktree を握る）。

## 40. 着地の列の先頭 N 本を候補の木 1 つに積んで検査を 1 回撃ち、patch-id 不変の便は検出線を持ち越す（契約表の行 ah / ai・`s2-07l.428`・[ADR-0039](../../design-intent/decisions/ADR-0039-landing-train-gates-one-candidate-tree.html)）

- **本節は契約表の 2 行を持つ**: 行 ah = 形 (a) と歯 (a)〜(g)、行 ai = 形 (b) と歯 (h)〜(j)。行 ai は行 ah が着地した後に始まる（行 ai の `depends`）。「触らない」と「却下」の節は 2 行に共通で、どちらの行も列の順序と鍵・`await_turn`・主実測の行・lens の判定には触らない。
- 何が起きているか（planner の実測 2026-09-17・fleet の event log 09-16 00:00Z 以降 84 便）: 便の延べ 83 時間のうち gate 36 時間（43%）・追随の再 gate 29 回（中央値 16 分・最大 151 分）・着地した 17 bead の壁時計は中央値 238 分。列の先頭が着地するたびに後続が追随して全部撃ち直す＝列の長さ N に対して再 gate が N 回・gate の時間が N の 2 乗に伸びる。29 回の再 gate はすべて `rebase:<old>..<new>` の記帳を持つ衝突無しの rebase で、便の diff は変わっていないのに検出線（mutants-diff）も撃ち直している。§30 / §33 は main の差分が検出線の面の外の周だけを省く。
- 現物: `pipe/land.rs` の `land` は便 1 本ごとに「番待ち → 追随（rebase → gate の撃ち直し）→ squash → 主実測 → finish」を通す 1 本道で、列（`pipe/queue.rs` の `turn_in`）は順番だけを決める。検出線の record（`pipe/gate/record.rs`）は便の diff の指紋を持たないので、再 gate は「同じ diff か」を測れない。
- 形 (a)（行 ah・候補の木）: 列の先頭（`turn_in` が `First` を返した便）が着地する周、**列の自分の後ろに並ぶ便**（`turn_in` と同じ条件＝終端でない ∧ Gated 済 ∧ worktree 実在 ∧ 最新 verdict PASS・鍵の順）を rules 行 `land.train_max`（列の長さの上限・kind `LandTrainMax`・Int・値 4・裁定 id = user 2026-09-17T03:28Z・C5）− 1 本まで取り、自分を先頭に並べた列を **1 つの候補の木**に積む。選ぶ関数は `pipe/queue.rs` の pure な 1 本（`turn_in` と同じ列の読みの上）で、行が無い・読めない周と上限 1 の周は自分だけ（現行と同じ）。候補の木は main の先端から切った tmp worktree（主実測の `verify` の隣・便の worktree と記録の base は触らない）に、便ごとに `git cherry-pick <base>..<HEAD>` で順に積む。積めなかった便（衝突）は `cherry-pick --abort` で候補から外し（その便の event は書かない・後続は詰める）、外れた便は自分の land で従来どおり追随して衝突を pipeline-conflict.md §3 の起こし直しへ進める。積んだ便ごとにその段の tree を覚え、便の diff に対する検出線をその場で撃つ（base = 直前の段・record はその便の `verify.jsonl`）。全部積んだ木に対して共通 verify を **1 回**（record は先頭の便の `verify.jsonl`・field `train=<N>`）と各便の契約 verify をその便の分（record はその便の `verify.jsonl`）撃つ＝lens は撃たない（各便の verdict PASS が入口の条件で、候補の木で測るのは木の緑）。**緑**なら列の順に、覚えた段の tree で `commit-tree`（親 = 直前の着地 commit）して main を CAS で N 本ぶん進め（squash の材料を「worktree の tree」から「段の tree」に広げる 1 引数）、次に主実測 `verify_main` を従来どおり先端の木で 1 回撃つ（§33・C12.6 の緑はここが担う・木は候補と同じなので検出線は `same-tree` で省かれる）。主実測が**緑**なら便ごとに列の順で `verdicts.jsonl` の 1 行（`order` の閉じた値に `train` を 1 つ足す）と `Landed` event（detail は従来の形＝`sha:` に自分の段の commit・`main:` に実測した先端）と worktree の退避を書く。主実測が**赤**なら列の便すべてに従来の `Failed` の `main-red` を書く（main は進んだまま・巻き戻さない＝既存の極性・N 本のどれが赤かは帰属しない）。FR50 の面: push と CI の照合は先端 commit 1 回で N 本ぶん（同じ push・同じ CI run）、台帳の close は便ごとの `Landed` の `sha:` で 1 本ずつ（push・照合・close の機械化は本節の外＝行 ah の write-set に無い）。**赤**（共通 / 契約 / 検出線のいずれか）なら候補の木を畳んで**列を解き**、先頭 1 本の既存の経路（追随 → 撃ち直し）にそのまま入る（どの便が赤かは帰属しない・後続の便は列に残る）。候補の木を切れない・積めない・読めない周も同じく解く（fail-closed）。列の後ろの便が自分の land に来た周は、**番待ち（`await_turn`）から戻った直後の 1 点**で段を読み、`Landed` なら **何もせず rc 0**（§29 の冪等の終端を段で先に読む・worktree の実在を要さない・待たない周も同じ点を通るので、番待ちの間に列で着地した便も追随へ進まず終端する＝`await_turn` 自身は触らない）。上限 1 と行の不在は現行の経路そのもの。stdout の 1 行に `train=<積んだ本数>`（解いた周は `train=<N> dissolved`）。train の本体は行 ah の write-set の `+` の file（新設 module）に置く。`crates/scribe2/src/pipe/land.rs` で動くのは `Landed` の早期終端と `First` の周の委譲の 2 か所だけである。squash の材料を段の tree へ広げる 1 引数と、便ごとの `Landed` / `verdicts.jsonl` / worktree の退避を train から呼ぶための可視性は、**§43（行 ak）が先に割った後の受け皿の file**（行 ak の write-set の `+` の file）の側で動く＝行 ah は行 ak の着地を前提にする。その file はまだ base に無いので、行 ah の write-set は**それを含む dir**（`crates/scribe2/src/pipe/land/`・行 aj が既に作っている）を 1 項目として持つ＝行 ak の着地の前でも後でも解ける（`+` は intake が「base に無い file」として読むので、他の行が作る file には使えない）。
- 形 (b)（行 ai・検出線の持ち越し）**行 ai の write-set の train の項目は行 ah が作る file を指すので、行 ah が着地した後に契約を焼く周は `+` を外す**（`+` は intake が「base に無い file」として読み、実在する file に付いていれば断る）: gate は検出線を撃つ周に便の diff の `git patch-id --stable`（`<base>..HEAD`）を検出線の record に `patch_id=` で残す。追随の撃ち直し（`follow_main`）と候補の木の各段は、検出線を撃つ前に同じ 1 関数で「面に触れたか（§30）→ 前周の record に `patch_id` が在り今の diff の patch-id と同じか」を順に読み、同じ周は前周の検出線の record を **`carried=<前周の n>` を付けて写し**撃たない（`Detection` の閉じた値に持ち越しを 1 つ足す・持ち越した値は record の field で実測と区別する・C10）。record が無い・`patch_id` が無い・違う・読めない周は撃つ（fail-closed）。§30 の `outside-scope` の省略は不変で先に効く。**差し込み点の現物（orchestrator が grep で実測・main ff77a0a・行 aj / ak の純移動の後の姿）**: `follow_main` は `crates/scribe2/src/pipe/land.rs`（570 行・親に残る）に在り、`pipe/land/` 配下の 2 つの子（squash と finish・主実測）には無い。`Detection` と `DetectionSkip` の閉じた値は `crates/scribe2/src/pipe/gate.rs`（222 行・231 行）、写しの `DetectionCopy` は `crates/scribe2/src/pipe/gate/record.rs`（263 行）。`Detection` の値を読む site は 4 file 10 site で、**網羅 match は `crates/scribe2/src/pipe/gate/record.rs` の 1 つだけ**（`Run` / `Skip` の 2 腕・78–80 行）、`crates/scribe2/src/pipe/land.rs` の 4 site は `if let Skip` 1 つと `Run` / `Skip` の構築 3 つ、`crates/scribe2/src/pipe/gate.rs` の 1 site は doc の参照、`crates/scribe2/src/pipe/cli/step.rs` の 1 site は `Run` の構築（腕を足さないので write-set に要らない）。ゆえに持ち越しの値を足して動く file は write-set の `gate.rs` / `gate/record.rs` / `land.rs` と候補の木の段 `train.rs` で閉じる。
- 触らない: 列の順序と鍵（`turn_in`・ADR-0021 §2.5）・`await_turn` と `pipe.land_wait_s`・stale base の判定と CAS・rebase と衝突の経路（便の worktree は候補の木の外）・主実測の行・lens の判定・verdict の 3 値・`Landed` の detail の形・`--pr-cmd` 形（列を見ない）・`DETECTION_SCOPE`。
- 却下（ADR-0039 §03）: 現行のまま（2 乗の撃ち直し）／追随の再 gate を撃たない（着地前に着地後の木を検査しない・C12.6）／楽観着地して赤なら revert（main が赤の時間を認める）／先頭 k 本ごとの木を並列に検査する（費用が N 倍・改訂 ADR で足す候補）／候補の木を便の worktree を順に rebase して作る（後続の便の記録の base が main の祖先でなくなり、解いた周に便が stale で固まる）／`Landed` の detail に train の印を足す（便ごとの記録の形は不変・train の事実は先頭の便の record と stdout に置く）。
- 歯（**行 ah**・接頭辞 `pipe_train_`〔置き場は `crates/scribe2-boundary/tests/e2e/pipe/land.rs` と、列を選ぶ pure 関数の in-file の歯だけ `crates/scribe2/src/pipe/queue.rs` の `mod tests`〕と `rules_land_train_`〔`crates/scribe2-boundary/tests/e2e/rules.rs`〕・base の当たりはどちらも 0 本・既存の `three_gated_runs` + 偽 lens + rules の tmp manifest の型）: (a) 上限 3 で先頭を land すると 3 本が列の順に着地し（親の連鎖・main の先端・`Landed` 3 件・`verdicts.jsonl` 3 行・`order=train` が後続 2 本）、後続の追随は 0 回・先頭の `verify.jsonl` に共通 verify が `train=3` で 1 組・後続の `verify.jsonl` に契約 verify と検出線だけ／着地済みの便の land は rc 0 で main 不変／番待ちで待っている 2 本目の land（子 process・`pipe.land_wait_s` の窓）と並行に先頭が列で着地すると、2 本目は起きた後に rc 0 `already-landed` で終端し event が増えない。(b) 3 本目の契約 verify が赤なら列を解いて先頭だけが着地し、後続 2 本は Gated PASS のまま列に残り、stdout に `dissolved`。(c) 2 本目が先頭と衝突する周は 2 本目を外して 1・3 本目が着地し、2 本目の worktree は clean のまま event が増えない。(d) 上限 1 と行の不在は先頭だけが着地し後続は従来どおり追随 1 回。(e) 列を選ぶ pure 関数の in-file の歯（鍵の順・PASS でない / worktree 無し / 終端の便を数えない・上限で切る）。(f) rules 行の pin（kind・enabled・裁定 id・`ALL` に在る・parse で引ける・外形 snap の `rows=` / `kinds=` が 1 増える）。(g) 先端の木の主実測が赤の周は列の便すべてが `Failed` の `main-red` で `Landed` 0 件・main は N 本ぶん進んだまま（巻き戻さない・既存の極性）。
- 歯（**行 ai**・接頭辞 `pipe_detection_carry_`・置き場は `crates/scribe2-boundary/tests/e2e/pipe/gate.rs` と `crates/scribe2-boundary/tests/e2e/pipe/land.rs`・base の当たりは 0 本）: (h) gate の検出線の record に `patch_id` が在り `git patch-id --stable` と一致する／(i) main が `crates/` で動いた追随で便の diff が不変なら検出線を撃たず `carried=<n>` の record が前周の写しになる（`Detection` の閉じた値に持ち越しが 1 つ増える）／(j) 便の diff の中身が変わる周（衝突無しでも context が動く fixture）・前周の record が無い周・`patch_id` を持たない record の周・読めない周は撃つ（fail-closed）。
- 実装の決め（**行 ah**・`s2-07l.428`・形 (a) の読みを現物で閉じた 5 点）: (i) 列に積む後続は `turn_in` の面のうち段が **`Gated` の便だけ**（追随して `Implemented` へ戻り撃ち直している便は自分の land が worktree を動かしている最中＝候補の木に積まない）・worktree が clean で契約 / base / HEAD を読める便だけ（読めない便は積まず自分の land で進む）。先頭が積めない周は解き（`dissolved why=stack`）、後続がどれも積めなかった周は `train=` を出さず先頭 1 本の既存の経路。stdout の解いた行は `train=<N> dissolved why=<read|cut|stack|verify|cas>`。(ii) 主実測は先端の木で `verify_main` と同じ 1 本を撃つが、`{base}` と write-set 照合の base は**候補の木を切った main**、照合の write-set は**列の契約の write-set の和**（先頭の記録の base からの差分は後続と main の動きを含み、先頭の契約だけでは照合が赤になる）。(iii) 着地の記帳は**後続 → 先頭の順**（後続の番待ちは先頭が列を空けた瞬間に起きて段を読む＝先頭を先に終端させると、自分の `Landed` の前に起きた後続が追随へ進む窓が開く）・stdout は列の順。(iv) 番待ちの直後の 1 点は `Landed` を rc 0（`already-landed=1 order=<…>`）、主実測の赤で列ごと `Failed` になった便を rc 1 で止める（追随へ進まない）。`pipe land` の段の前提は `Gated` に `Landed` を足し、`--pr-cmd` 形は従来どおり `Landed` を断る。(v) 検出線を持つ便の段では同じ呼出が撃つ契約 verify を捨て（先端の木で撃ち直す側を記録と判定に数える）、`verify.jsonl` の record の `n` は既存の record からの通し。
- 実装の決め（**行 ai**・形 (b) の読みを現物で閉じた 5 点）: (i) 1 関数は `land.rs` の `rerun_detection`（材料は `Rerun` の 1 struct＝main の動いた range・便の diff を測る木と range）で、(1) 面（§30 の `follow_detection`）→ (2) 持ち越し（`gate.rs` の `carry_detection`）の順。追随は面の range に `<base>..<main>`（rebase で動かない）、diff に rebase 後の `<main>..HEAD` を渡す＝面の判定は rebase の後へ移したが値は不変。候補の木の段は面の range に `<便の記録の base>..<直前の段>`、diff に `<直前の段>..<段の commit>` を渡す（列の先頭で main が動いていない段は range が空＝§30 の読みで `outside-scope` の skip record になり撃たない・gate が同じ diff を測った後である）。(ii) 「前周の record」は `verify.jsonl` の**最後の** `kind=detection` ∧ `skipped` を持たない record（撃った record か、前の写し）。持ち越すのはその `patch_id` が今の patch-id と同じで `rc` が 0 の周だけ。file を読めない・1 行でも JSON として読めない周は撃つ。(iii) 写しは前周の record の field を並びのまま写し、`n` だけをその周の通しに振り直して末尾に `carried=<前周の n>` を足す（`cmd` / `rc` / `line` / `patch_id` は前周の値・撃っていないので周ごとの写しの置き場〔gate-cost.md §15〕は作らない）。前の写しを写す周は `carried` を今の前周の `n` に置き換える。(iv) `patch_id` は撃つ周の検出線の record にだけ書く（検出線の行が無い便・diff が空 / 読めない周は field を欠く＝持ち越しの根にならない）。候補の木の段で撃った record も同じ field を持つ。(v) `Detection` は値を持つ variant が増えたので `Copy` を外す（構築点と読み手は §40 形 (b) の閉包のまま・`pipe/cli/step.rs` の `Run` の構築は不変）。
- **[gate-cost.md](./gate-cost.md) §44 が上書き**（[ADR-0060](../../design-intent/decisions/ADR-0060-detection-line-runs-after-landing-not-in-the-gate.html)・同 doc の契約表の行 ak / al / am の land 後・本節の本文は書き換えない）: 形 (b) の持ち越し（patch-id の record）は gate-cost.md の行 al で消え、候補の木の後続ごとの検出線の段は同じ doc の行 am で消える。後続の検出線は自分の squash commit の親を {base} にして着地後に撃たれる。
- **errata（着地・gate-cost.md の行 al・`s2-07l.588`・規範は上の本文のまま）**: 形 (b) の持ち越しは器から消えた——`Detection` は `Run` / `Skip` の 2 値に戻って `Copy` を持ち、`rerun_detection` の材料は面の range だけで、検出線の record は `patch_id` と `carried` の field を持たない。面に触れた追随と候補の木の段は毎回撃つ（形 (b) の歯 (h)〜(j) は同じ便で消え、gate-cost.md の接頭辞 `pipe_detection_single_shot_` の歯が撃つ側を測る）。

## 41. pipe/land.rs の主実測の群を land/verify.rs へ割る（契約表の行 aj・`s2-07l.457`・純移動）

- 何が起きているか（planner の実測 2026-09-18・main 36d9c39・`pipe preflight` で verified）: `pipe/land.rs`（1238 行・src 1160 + in-file の歯 78）は R-C4-2 の余地が 261 行しか無く、size M の便 3 本（行 ah・`s2-07l.428`／行 af・`s2-07l.449`／行 ab・`s2-07l.400`）を受付が `cap-headroom` で断る（3 本とも rc 1 を実測）。責務は 6 群（入口と番待ち／追随〔`follow_main` の群〕／squash と finish／主実測〔`verify_main` の群〕／anchor の同期／worktree の検査）で、主実測の群は §5.4 の tmp worktree の中だけを触り、追随・squash の群に依存しない閉じた集合（呼び手は `land`〔`verify_main` / `main_red` / `main_unmeasured`〕と `finish`〔`measure_main`〕の 2 か所・grep で確認・run 213227Z の Gated FAIL で 4 語目を実測）。
- 決定的な制約（実測）: `MainCheck` は極性一覧（`crates/scribe2/src/polarity.rs`・`tests/e2e/polarity.rs`・snapshot `polarity_external_form`）が境界の型名 `pipe::land::MainCheck` を pin する＝**`MainCheck` は親に残す**（`pub use` では型名が変わらない・contract-source.md §15 の `TableError` と同型）。`AnchorPlan` / `WorktreeCheck` も同じ pin を持つが移す群に無い。
- 形（contract-source.md §14 / §15 と同型）: 子 module（行 aj の write-set の `+` の file）へ主実測の群（`check_path` / `verify_main` / `materials` / `main_detection` / `record_main` / `measure_main` / `main_red` / `with_anchor` / `main_unmeasured` と const `CHECK_DIR` / `VERIFY_MAIN_FILE` / `MAIN_UNKNOWN`・約 230 行）をそのまま移す。親は `mod` 宣言と `use`（子の 3 関数を `land` が・1 関数を `finish` が呼ぶ）で `land` と `finish` の本体を不変に保つ。in-file の歯 5 本（`pipe_land_subject_` / `pipe_detection_scope_` / `mutant_in_pipe_land_next_number_`）は移す群の歯ではないので親に残す＝子に歯は無い（純移動の証明は札と既存の e2e の歯）。子は親の私有 item（`MainCheck` / `AnchorSync` / `Land` の欄 / `verdicts_path` / `with_lines` 等）を `super::` でそのまま呼べる（Rust の可視性＝子孫は祖先の私有を見る）ので親側の可視性は変えない。上げるのは**子側**の可視性＝親の `land` が呼ぶ `verify_main` / `main_red` / `main_unmeasured` と親の `finish` が呼ぶ `measure_main` の `pub(super)` の 4 つだけ。可視性の 1 語と mod 宣言・`use` の path・doc コメント行は移動の一部（純移動の残差として許す）。札 `// flip-check: moved s2-07l.457` は親の歯の区間と子の先頭に対で置く。verify は親に残る in-file の歯（`pipe_land_subject_`）と極性一覧の snapshot の歯（`polarity_external_form`・`tests/e2e/polarity.rs`）を撃つ＝後者の file は行 aj の write-set に持つが本便では触らない（受付の歯の置き場の門のため）。
- 見積: 親 約 1000 行（余地 約 500）・子 約 240 行。
- 歯: 既存の `pipe_land_` / `pipe_order_` / `pipe_follow_` / `pipe_retire_` の e2e と極性一覧の snapshot（`polarity_external_form`）が全部緑で期待を変えない。
- 却下: anchor の群を移す（88 行で M の余地に届かない）／追随の群を移す（行 af / 行 ab が同じ群を触る＝それらの write-set が 2 file に割れて交差が増える）／行 ah / af / ab を S に落とす（見積が S の 100 を超える＝size の字面だけ変える嘘）。

## 42. 受付の e2e の hub を接頭辞ごとに割る（契約表の行 b・`s2-07l.351`・純移動）

- 出所: `crates/scribe2-boundary/tests/e2e/pipe/intake.rs` は open な便の write-set に何本も現れる hub で、intake の排他（FR39）の交差の母集団を大きくし便を直列にする。接頭辞で割って母集団を構造から減らす（`.327` / `.349` / `.264` と同じ型・要件は FR30 の配送構造の面）。受付が verify 行の scope（`-p` / `--test` / `--lib`）を読むようになった `s2-07l.451` が着地した後なので、歯の置き場の門は本便の形で通る。
- 現物（orchestrator が測り直し・main b010cd4・当初 planner が main f678bd0 で測った 2943 行・97 本から hub は伸び続けている）: `crates/scribe2-boundary/tests/e2e/pipe/intake.rs` は **4159 行・`#[test]` の歯 134 本**で、接頭辞の内訳は `pipe_intake_` 55・`contract_` で始まる 45（`contract_closure_ext_` 19 / `contract_derive_` 12 / `contract_check_` 6 / `contract_declared_` 3 / `contract_table_landed_` 2 / `contract_schema_` 2 / `contract_names_` 1）・`pipe_review_` 15・`pipe_refuse_` 8・`pipe_preflight_` 4・`pipe_repo_` 3・`pipe_contract_` 1・`pipe_confine_` 1・`pipe_state_` 1・`pipe_show_` 1（母集団は同 file の `#[test]` 全数 134・数え直しは着地直前に runner がもう一度行い、本節の数と食い違えば実測を採って notes に残す）。共有 helper は親の `crates/scribe2-boundary/tests/e2e/pipe.rs` に 40 本在り、子は `use super::*;` で引く（`.264` の分割と同じ形）。親の `mod` 宣言は 8 本。
- filter の当たりの実測（`--test e2e` の scope・fn 名の substring・母集団は `cargo nextest list -p scribe2 --test e2e` の名簿 **1145 本**・main 2f846df）: 裸の `contract_` は 10 file の 80 本に当たる（`headless.rs` 7 / `hook.rs` 2 / `pipe/dispatch.rs` 5 / `pipe/gate.rs` 3 / `pipe/land.rs` 1 / `pipe/spawn.rs` 1 / `polarity.rs` 1 / `prop.rs` 1 / `rules.rs` 3 / `pipe/intake.rs` 56）＝**verify 行には使えない**（受付が teeth-outside-write-set で断る）。細かくした接頭辞のうち他 file に漏れるのは 2 つだけで、`contract_closure_ext_` が `crates/scribe2-boundary/tests/e2e/prop.rs` の 1 本に当たり（20 本中・だから行 b の write-set はこの file を持つ＝**本便では 1 字も触らない**）、`contract_table_` は `polarity.rs` 1 本と `rules.rs` 3 本に当たるので `contract_table_landed_` まで伸ばす（当たりは `pipe/intake.rs` の 2 本だけ）。`pipe_intake_` は親の `pipe.rs` の 1 本にも当たる（55 + 1 = 56・親は write-set に在る）。検証行の 3〜5 本目が使う残りの接頭辞は hub の外に 1 本も当たらない（同じ名簿 1145 本で実測）: `pipe_refuse_` 8 / `pipe_preflight_` 4 / `pipe_state_` 1 / `pipe_show_` 1 は全部 `pipe/intake.rs` の歯で、他の file は 0。`pipe_review_` だけが hub の 15 本の外に 3 本当たる（形 (1)・名の全体で書く）。
- 形（純移動・番号は done と 1:1。行 b の write-set の `+` の 3 file は**宣言順に**〔審査の面・契約の面・断りの面〕を受ける）:
  1. 審査の段の歯 `pipe_review_` **15 本**（main 26ac26f の実測・当初 12 本に `pipe_review_kind_` 3 本と要件の読みの歯が増えた）を `+` の 1 本目（審査の面）へそのまま移す。接頭辞 `pipe_review_` は hub の外の 3 本（`pipe_review_kind_lens_that_cannot_start_is_unparsed`〔`tests/e2e/pipe/ratelimit.rs`〕・`pipe_review_kind_report_counts_review_fail_by_kind_in_declaration_order`〔`spawn.rs`〕・`pipe_review_kind_lens_killed_in_scope_is_unparsed`〔`stop.rs`〕）にも当たり、受付が `teeth-outside-write-set` で断る（2026-09-22 の実測）ので、検証行は hub の 15 本を**名の全体**で名指し、外の 3 本は触らない。移す 15 本は `Stage` / `Guard` の variant を分岐する（`Stage` 6 本・`Guard` 2 本・main 88d80fc の実測）ので、`+` の 1 本目は [contract-source.md](./contract-source.md) の行 c（`touches` = `Stage` / `Guard`）の閉包に入り、行 c の write-set に `+` で先に書いてある（run 042125Z の runner の質問の根・現物の契約表の歯 `contract_closure_ext_real_table_has_zero_findings` が赤になる。本便は行 c の doc を触らない）。`+` の 2 本目と 3 本目は `touches` を持つ 17 行のどの閉包にも入らない（分割を粗く再現した scratch で `contracts check` の findings が行 c の 1 件だけ・verified）。
  2. 契約表と閉包の歯 46 本（`contract_` で始まる 45 + `pipe_contract_` 1）を `+` の 2 本目（契約の面）へそのまま移す。
  3. 断りの語彙の歯 `pipe_refuse_` 8 本を `+` の 3 本目（断りの面）へそのまま移す。
  4. 単発の `pipe_state_` 1 本と `pipe_show_` 1 本は親（`crates/scribe2-boundary/tests/e2e/pipe.rs`）へ移す。
  5. 元の file には受付の口の歯 63 本（`pipe_intake_` 55 + `pipe_preflight_` 4 + `pipe_repo_` 3 + `pipe_confine_` 1）だけが残り、4159 行は約 2000 行へ縮む。
  6. 親に `mod` 宣言を 3 本足す（宣言は既存の 8 本と合わせて名の昇順）。歯の本文・名・順序・`#[test]` の総数 134 は変えず、子は親の helper を `use super::*;` で引く。
  7. 札 `// flip-check: moved s2-07l.351` を元の file の歯の区間の先頭と `+` の 3 file の先頭に対で置く（純移動の機械証明は §5.3）。形 4 で 2 本を受ける親（`crates/scribe2-boundary/tests/e2e/pipe.rs`）にも、移した 2 本の直前に同じ札を 1 行置く（親は `tests/` 配下＝file 全体が歯の区間なので置いた位置で効く）＝札は元の file・`+` の 3 file・親の 5 file に在り、親に増える残差は `mod` 宣言 3 行と札 1 行と移した 2 本だけ（2026-09-20 の審査 FAIL「親に札の無い `#[test]` 2 本が増える」の再現）。
  8. 親の置き場の pin 歯 `pipe_hermetic_sites_stay_one`（§28 形 2 の改訂・行 aq・`s2-07l.547`）は本便では**触らない**: 行 aq が定数 9 を「親の列 0 の `mod` 宣言の数 + 1」に替えた後に走るので、`+` の 3 file と宣言 3 本で 12 = 11 + 1 のまま緑（run 054450Z は定数を 12 に触って純移動の機械証明が items-differ で落ち、lens の入力が要約にならず diff 313584 byte が cap 150000 を超えて INCONCLUSIVE になった根）。
  9. 元の file の helper（歯でない fn / const）のうち子が使うものは、逐語で親 `crates/scribe2-boundary/tests/e2e/pipe.rs` へ移して `pub(super)` に上げる（子は `use super::*` で引く・run 054450Z の実測では fn 23 本 + const 6 本）か、元の file に残して `pub(super)` に上げ子が元の module を名指す use で引く（1 つの子だけが使う helper）。どちらも純移動の機械証明の中（移動と可視性の差は許容・本文は 1 字も変えない）。
- 触らない: 歯の名・本文・本数・親の pin 歯（形 8）・親の 40 本の helper・`prop.rs` / `polarity.rs` / `rules.rs`（filter が当たるだけで中身は触らない）・受付の口の src。
- 却下: `contract_` 1 本の filter で verify を書く（9 file に当たり受付が断る）／歯の名を変えて接頭辞を揃える（純移動でなくなり機械証明が残差を出す）／割らずに据え置く（交差の母集団が減らない）。

## 43. pipe/land.rs の「squash と finish」の群を割る（契約表の行 ak・`s2-07l.457` の 2 回目・純移動）

- 出所: `crates/scribe2/src/pipe/land.rs` は幅 120 で正規化した行数が **1416**（上限 R-C4-2 = 1500）＝**余地 84** で、受付が `cap-headroom` で 3 便を断る——行 p（`s2-07l.305`・size S・見積 `pipe.size_s_lines` = 100）・行 ab（`s2-07l.400`・size M・見積 `pipe.size_m_lines` = 300）・行 ah（`s2-07l.428`・size M・300）。行 p の拒否は planner が `pipe preflight` で実測した（2026-09-20・「上限の余地が 84 行で size S の見積に足りない」）。行 aj（`s2-07l.457`・着地済み）が主実測の群を 1 回割った後の姿で、本行は同じ型の 2 回目である。
- 現物（planner が grep と正規化行数で実測・main 3926753）: 責務は 6 群（入口と番待ち／追随／squash と finish／主実測／anchor の同期／worktree の検査）で、主実測の群は行 aj が子へ出した。残る 5 群のうち **squash と finish の群**は呼び手が閉じている（親の `land` と `attempt` の 2 か所から入り、群の外を呼ぶのは `verdicts_path` と `RUN_TRAILER` と `Terminal` だけ）。群の item は **18 個**（8 + 8 + 2・2026-09-21 の便 1 本目の審査が「19」との食い違いを指摘し、数え直した）で、内訳は `open_pr` / `squash` / `squash_message` / `trailer_key` / `subject_of` / `gist_of` と const `CONTRACT_TRAILER` / `REQUIREMENTS_TRAILER`（768–897 行・**130**）、`landed_sha` / `terminal` / `note` / `finish` / `export_verdict` と const `GENERATION` / `CLOSE_REASON` / `SHA_PREFIX`（965–1154 行・**190**）、const `SUBJECT_CHARS` / `ELLIPSIS`（126–131 行・**6**）の合計 **326 行**（幅 120 で正規化した値）。
- 決定的な制約（実測）: 極性一覧（`crates/scribe2/src/polarity.rs`・`crates/scribe2-boundary/tests/e2e/polarity.rs`・snapshot `polarity_external_form`）が境界の型名 `pipe::land::Terminal` を pin し `crate::pipe::land::TERMINAL_POLARITY` を名指しで読む＝**`Terminal` と `TERMINAL_TOKENS` と `TERMINAL_POLARITY` と `impl Terminal` は親に残す**（`MainCheck` / `AnchorPlan` / `WorktreeCheck` と同じ理由・§41 と同型）。`RUN_TRAILER` は追随の群の `landed_squash_of` も読むので親に残す。`VERDICTS_FILE` と `verdicts_path` は `land::verdicts_path` の形で 11 site が引くので親に残す。`land::landed_sha` と `land::terminal` は `crates/scribe2/src/pipe/cli/step.rs` が引くので、親が名指しの `pub use` で再輸出して呼び手の字面を変えない。
- 名前解決の形（planner が呼び手を grep で数え、同じ形の最小 crate を `cargo clippy --all-targets -- -D warnings` まで通して実測・2026-09-20・§45 と同じ型）: **可視性は名前解決をしない**ので、子を上げるだけでは親の本体の裸の名が解けず、親に `use` が要る（修飾は本体を書き換える＝純移動でなくなる）。解く必要がある名は 3 群に割れる——**(i) 親の本体が裸で呼ぶ 3 名**（`open_pr` / `squash` / `finish`・それぞれ 1 site）／**(ii) `pipe` の外から `land::` で引かれる 2 名**（`landed_sha` / `terminal`・呼び手は `crates/scribe2/src/pipe/cli/step.rs`）／**(iii) in-file の歯だけが読む 7 名**（`squash_message` 4 site / `trailer_key` 7 / `subject_of` 4 / `CONTRACT_TRAILER` 5 / `REQUIREMENTS_TRAILER` 3 / `SHA_PREFIX` 4 / `SUBJECT_CHARS` 3・いずれも親の本体の site は 0）。(iii) を素の `use` に入れると `#[cfg(test)]` の無い通常 build で `unused imports` が出て **rc 101** で落ちるので、**`#[cfg(test)]` を付けた `use` 1 文に分ける**（`allow` は足さない）。`#[cfg(test)]` の束縛も私有で module とその子孫に見えるため、歯の区間の `use super::{…}` は**1 字も変わらない**。(ii) は**再輸出が可視性を広げられない**（子の item が `pub(super)` のままだと `pub(in crate::pipe) use` は `E0364`）ので、子の側で `pub(in crate::pipe)` にして同じ可視性で再輸出する。`gist_of` / `note` / `export_verdict` / `GENERATION` / `CLOSE_REASON` / `ELLIPSIS` は群の外に site が 0 で、子の中だけで呼ばれる＝`use` にも現れず `dead_code` にもならない。
- 検証行が名指す歯（planner が全 `#[test]` の fn 名で census・main b98bbee）: **4 本とも既存の歯で、新設は 0 本**である（純移動ゆえ歯の名も本数も本文も変わらない＝入口の RED は札 `moved` が担う・§7 の `moved` の逃がし）。内訳は、接頭辞 `pipe_land_subject_` が `crates/scribe2/src/pipe/land.rs` の既存 3 本（`pipe_land_subject_cuts_by_chars_not_bytes` / `pipe_land_subject_falls_back_to_bead_when_gist_is_empty` / `pipe_land_subject_keeps_multiline_goal_verbatim_in_body`）、完全名の 3 本（`pipe_terminal_land_landed_sha_reads_only_its_own_run_done` / `pipe_terminal_land_squash_message_carries_the_contract_and_requirements_trailers` / `pipe_terminal_land_outcomes_are_the_closed_seven`）も `crates/scribe2/src/pipe/land.rs` の既存 1 本ずつ、`polarity_external_form` が `crates/scribe2-boundary/tests/e2e/polarity.rs` の既存 1 本。
- **`crates/scribe2-boundary/tests/e2e/polarity.rs` が write-set に在るのは、受付が「検証行の filter が当たる歯の file は write-set に在ること」を求めるためであって、編集してよい面だからではない。本行はこの file を 1 byte も変えない。** `pipe preflight` の実測で `polarity_external_form` はこの file に解ける（`teeth=polarity_external_form:1@crates/scribe2-boundary/tests/e2e/polarity.rs`）＝write-set から外せば `teeth-outside-write-set` で断られ、入れたまま編集すれば極性一覧の pin を便が自分で書き換えられることになる。ゆえに **write-set に持ち、diff は 0 行**という形を取る（[rules-manifest.md](./rules-manifest.md) §15 の行 l が `crates/xtask/src/check.rs` で取った形と同型）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 18 item（合計 326 行）を、行 ak の write-set の `+` の file へ名・本文・順序を変えずにそのまま移す。
  2. `Terminal` / `TERMINAL_TOKENS` / `TERMINAL_POLARITY` / `impl Terminal` / `RUN_TRAILER` / `VERDICTS_FILE` / `verdicts_path` は親に残る＝`polarity_external_form` の snapshot が 1 字も動かない。
  3. 親に増えるのは `mod` 宣言 1 つと `use` 文 3 つだけ——素の `use`（(i) の 3 名）／`pub(in crate::pipe) use`（(ii) の 2 名）／`#[cfg(test)]` を付けた `use`（(iii) の 7 名・この 1 文だけ 120 桁に収まらないので `{}` の中で折る）。`land` / `attempt` の本体は **1 字も変わらない**。
  4. 上げるのは**子側**の可視性だけ＝親の `land` と `attempt` が呼ぶ item は `pub(super)`、`pipe` の外から `land::` で引かれる 2 つは `pub(in crate::pipe)`。親側の可視性は変えない（Rust の可視性＝子孫は祖先の私有を見るので、子は親の `RUN_TRAILER` / `verdicts_path` / `Land` の欄を `super::` でそのまま呼べる）。
  5. in-file の歯 8 本は**1 本も動かさない**（親の `mod tests` に残る）。歯の区間の `use super::{…}` も**1 字も変えない**——移した名は親の 3 つの `use` が親の scope に置くので `super::` のまま解ける。
  6. 札 `// flip-check: moved s2-07l.457` は既に親に在るので、本行は `// flip-check: moved <行 ak の bead>` を親の歯の区間の先頭と `+` の file の先頭に対で置く（純移動の機械証明は §5.3）。
  7. 割った後の正規化行数は親が **約 1090**（余地 **約 410**）・`+` の file が **約 340**＝余地 410 は上の 3 便の見積（100 / 300 / 300）を全部満たす。
  8. `crates/scribe2-boundary/tests/e2e/polarity.rs` の diff は **0 行**（write-set に在る理由は受付の要求だけ）＝極性一覧の pin は便の外から効いたままになる。
- 触らない: `land` / `attempt` / `follow_main` / `rebase_onto` の本体・追随の群・anchor の群・worktree の群・検出線の群・in-file の歯の名と assert・`crates/scribe2-boundary/tests/e2e/polarity.rs`（write-set に持つが 1 byte も変えない・上の 2 つ目の bullet）・他の e2e。
- 却下: 追随の群を移す（行 ab が同じ群を触る＝その write-set が 2 file に割れて交差が増える）／`Terminal` ごと移す（極性一覧の型名の pin が動く）／in-file の歯も一緒に移す（純移動の残差が歯の区間の差に広がり、機械証明の読みが難しくなる）／割らずに据え置く（3 便が受付で止まったまま）。

## 44. pipe/move_proof.rs の「diff を読んで item の列にする」群を割る（契約表の行 al・純移動）

- 出所: `crates/scribe2/src/pipe/move_proof.rs` は幅 120 で正規化した行数が **1500**＝上限 R-C4-2 ちょうどで、**余地 0**。[gate-cost.md](./gate-cost.md) の契約表の行 e（`s2-07l.292`・size S・見積 100）が `crates/scribe2/src/pipe/move_proof.rs` を write-set に持つので、受付は `cap-headroom` で断る。`.363`（`pipe/closure.rs` → `pipe/closure/derive.rs`）・行 aj・§10（[rules-manifest.md](./rules-manifest.md)）と同型の純移動で余地を作る。
- 現物（planner が grep と正規化行数で実測・main 3926753）: 責務は 3 段で、**(1) diff の字面を読む → (2) 宣言を同定する → (3) item の列に切る**の 3 段が前段、**(4) 突き合わせて要約を組む**が後段である。前段の item は 24 個で、内訳は `FileDiff` / `HEADERS` / `parse_diff` / `split_git_paths` / `hunk_start` / `hunk_line` / `header_line`（189–278 行・**91**）、`Kind` / `KINDS` / `impl Kind` / `Declaration` / `strip_visibility` / `is_scope` / `QUALIFIERS` / `declaration_of` / `named`（279–417 行・**139**）、`Item` / `Located` / `is_prefix` / `is_continuation` / `starts_item` / `items_of` / `item_end` / `build_item`（418–544 行・**127**）の合計 **357 行**（幅 120 で正規化した値）。後段（`comment_diff` / `use_spans` / `pair_items` / `matched_of` / `residual_lines` / `render` ほか・545–856 行）は前段の `Located` と `Item` を受け取るだけで、前段は後段の item を 1 つも呼ばない（呼び手の向きは片道・grep で確認）。
- 決定的な制約（実測）: `move_proof` の外から修飾付きで引かれる名は 5 つだけで（`judge` / `keep` / `LensInput` / `POLARITY` / `RULINGS_FILE`・`crates/scribe2/src` と `crates/scribe2/tests` の全数を grep）、**どれも前段に無い**＝移しても外の字面は 1 つも変わらない。極性一覧が pin する型名 `pipe::move_proof::LensInput` も親に残る側である。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 24 item（合計 357 行）を、行 al の write-set の `+` の file へ名・本文・順序を変えずにそのまま移す。
  2. 後段の 6 群と公開の 5 名（`judge` / `keep` / `LensInput` / `POLARITY` / `RULINGS_FILE`）は親に残る＝`polarity_external_form` の snapshot が 1 字も動かない。
  3. 親は `mod` 宣言 1 行と `use` だけが増え、後段の本体は不変。`pub use` は要らない（前段の名を `move_proof::` で引く呼び手が 0 件だから）。
  4. 上げるのは**子側**の可視性だけ（後段が呼ぶ item を `pub(super)`）。親側の可視性は変えない。
  5. in-file の歯 19 本は**1 本も動かさない**（親の `mod tests` に残り、移した item は `use super::` の path 1 行の差し替えだけで引く）。
  6. 札 `// flip-check: moved <行 al の bead>` を親の歯の区間の先頭と `+` の file の先頭に対で置く（純移動の機械証明は §5.3）。
  7. 割った後の正規化行数は親が **約 1143**（余地 **約 357**）・`+` の file が **約 370**＝余地 357 は待っている便の見積（100）を満たし、size M（300）の便も受けられる。
- 触らない: 後段の突き合わせと要約・`LensInput` の 3 値・`NotPure` の語彙・`keep` の書き口・歯の名と assert・e2e。
- 却下: 後段を移す（`LensInput` と `POLARITY` が極性一覧の pin を持ち、`judge` / `keep` の呼び手の字面が変わる）／in-file の歯だけを別 file へ出す（余地は空くが責務は割れず、次に足す便が同じ hub に戻る）／割らずに据え置く（余地 0 のまま 1 行も足せない）。

## 45. xtask の flipcheck.rs の「nextest を撃って出力を読む」群を割る（契約表の行 am・純移動・行 j の 2 回目）

- 出所: `crates/xtask/src/flipcheck.rs` は幅 120 で正規化した行数が **1284**（上限 R-C4-2 = 1500）＝**余地 216** で、行 c（`s2-07l.170`・size M・見積 300）が `crates/xtask/src/flipcheck.rs` を write-set に持つので受付が `cap-headroom` で断る。行 j（着地済み）が git / tar の群を 1 回割った後の姿で、本行は同じ型の 2 回目である。
- 現物（planner が grep と正規化行数で実測・main 3926753）: 責務は 9 群で、git / tar の群は行 j が子へ出した。残る群のうち **nextest を撃って出力を読む群**は閉じている（外部 process を起こす面と、その stdout を読む面だけを持ち、判定・overlay・札の面を呼ばない）。群の item は 7 個で、`trimmed` / `relay` / `nextest` / `nextest_with` / `nextest_args` / `strip_csi` / `failed_tests`（507–655 行のうち `struct FailedTest` と `impl FailedTest` の 19 行〔594–612 行〕を除く**約 130 行**・幅 120 で正規化した値）。**`struct FailedTest`（field 2 つ・`derive` 付き）と `impl FailedTest`（`filterset`）は親に残す**——歯が field ごと構築する型なので子へ出すと field と method の可視性を上げざるを得ず、純移動の証明（`crates/scribe2/src/pipe/move_proof.rs`）は item の頭の行の可視性しか剥がさない＝field の `pub(super)` が `items-differ` になる（2026-09-21 の便 3 本目の gate FAIL flip-check `lens-input=diff reason=items-differ` の再現）。子の `failed_tests` は親の私有の型を `use super::FailedTest;` の 1 行で引く（私有の item と field は子孫に見える・行 j の git.rs の `use super::{…}` と同型）。
- 決定的な制約（実測）: 歯の 6 file のうち 5 file（`crates/xtask/src/flipcheck_declaration_tests.rs` / `crates/xtask/src/flipcheck_overlay_tests.rs` / `crates/xtask/src/flipcheck_moved_tests.rs` / `crates/xtask/src/flipcheck_entrance_tests.rs` / `crates/xtask/src/flipcheck_retroactive_tests.rs`）は `use super::*;` で引き、残る `crates/xtask/src/flipcheck_tests.rs` は `use super::{…}` で 12 名を名指しする（うち移る側は `failed_tests` / `nextest_args` の 2 つ・`FailedTest` は親に残るので歯の `use super::FailedTest` はそのまま解ける）。行 c が触るのは札と面と宣言の群（`RETROACTIVE_MARK` / `MOVED_MARK` / `marker_beads` / `is_test_file` / `judge_into`）で、移す群と交差しない。
- 名前解決の形（planner が呼び手を grep で数え、同じ形の最小 crate を compile して実測・2026-09-20）: **可視性は名前解決をしない**——子の item を `pub(super)` にしても、親の本体の裸の名（`nextest(…)` 等）は解けないままで、親に `use` 1 文が要るか呼び手を修飾するかのどちらかになる。修飾は本体を書き換えるので純移動でなくなる＝**親に `use` 1 行を置く**形を採る。解く必要がある名は 6 つで、内訳は**親の本体が裸で呼ぶ 4 つ**（`relay` 4 site・`nextest` 3 site・`nextest_with` 1 site・`failed_tests` 1 site。`FailedTest` の 2 site〔`retry_named` の引数の型と `FailedTest::filterset`〕は型が親に残るので解く必要が無い）と、**歯が `use super::{…}` で名指す 2 つ**（`failed_tests` / `nextest_args`）と、**兄弟 module `crates/xtask/src/flipcheck/git.rs` が `use super::{trimmed, …}` で名指す 1 つ**（`trimmed`・4 site）の和である。`strip_csi` は群の外に site が 0 で、子の中だけで呼ばれる。`trimmed` は git.rs の `use super::trimmed` が親の私有の `use` の束縛を経て解ける（兄弟は親の子孫＝私有の束縛が見える・2026-09-21 の便 1 本目の Questioned を受けて最小 crate で `#![deny(warnings)]` の compile を実測）ので、git.rs は 1 字も触らない。`use` は `pub` を付けない私有の束縛で足りる——**私有の束縛は module とその子孫に見える**ので、`flipcheck::tests`（`#[path]` で取り込む歯の module）からの `use super::{…}` も `use super::*;` も解ける。行 j が同じ形で着地している（現物: `crates/xtask/src/flipcheck.rs` の `mod git;` と `use git::load_pairs;`）。module 名と fn 名がどちらも `nextest` になる衝突（module は型の名前空間・fn は値の名前空間）は同じ最小 crate で通ることを確かめた（`pub(super)` の struct を子孫が field ごと構築する形も compile は通るが、上記のとおり純移動の証明が通らないので採らない）。
- **`use` を 2 文に割る理由（planner が最小 crate で A/B・`cargo clippy --all-targets -- -D warnings`）**: 6 名のうち `nextest_args` だけは**親の本体に site が 0**（読み手は `#[cfg(test)]` 下の `crates/xtask/src/flipcheck_tests.rs` だけ）なので、6 名を 1 本の素の `use` に入れると `#[cfg(test)]` の無い通常 build で `warning: unused imports` が出て **rc 101** で落ちる。`#[cfg(test)]` を付けた `use` に分けると **rc 0**（`allow` は足さない・`#[cfg(test)]` の束縛も私有で子孫から解けることは in-file の `mod tests` と `#[path]` の子 module の両方で実測）。ゆえに親の `use` は **本体用（5 名・`trimmed` は git.rs の `use super::trimmed` が経由する束縛）と歯用（1 名・`#[cfg(test)]`）の 2 文**に割る。`trimmed` の束縛は git.rs が経由するので通常 build でも `unused_imports` に当たらない。**`dead_code` の側は起きない**——`nextest_args` は子の `nextest_with` が呼び、`trimmed` と `strip_csi` も子の中で呼ばれるので、通常 build でも全部使われている。`FailedTest` は親に残るので親の `use` には現れない。
- 検証行が名指す歯（planner が全 `#[test]` の fn 名で census・main b98bbee）: **4 本とも既存の歯で、新設は 0 本**である（純移動ゆえ歯の名も本数も本文も変わらない＝入口の RED は札 `moved` が担う・§7 の `moved` の逃がし）。内訳は、接頭辞 `flip_check_parses_failed_tests_` が `crates/xtask/src/flipcheck_tests.rs` の既存 2 本（`flip_check_parses_failed_tests_from_nextest_output` / `flip_check_parses_failed_tests_under_color_escapes`）、完全名の 2 本（`flip_check_child_nextest_disables_color` / `no_fail_fast_is_in_flipcheck_nextest_args`）も `crates/xtask/src/flipcheck_tests.rs` の既存 1 本ずつ。
- **`crates/xtask/src/flipcheck_tests.rs` が write-set に在るのは、検証行 2 本が撃つ既存の歯 4 本の現住所であることと、約束 5 の札 2 行の置き場であることの 2 つの理由からで、それ以外を編集してよい面だからではない。本行はこの file の歯の本文と `use super::{…}` を 1 byte も変えず、増えるのは約束 5 の札 2 行だけである**（約束 3 と同じ面で、write-set に在ることが札以外の編集の例外を作らない）。受付の導出では `teeth=flip_check_parses_failed_tests_:0@-` となり、この file は歯の file として解けない——歯の区間は「`crates/<c>/src/` の file は行頭の `#[cfg(test)]` から末尾」で切られ、`#[path]` で取り込まれる test 専用 file は行頭の `#[cfg(test)]` を持たないからである（[rules-manifest.md](./rules-manifest.md) §15 の実測と同じ性質）。**導出に現れないことは「撃たれない」ではない**——`cargo nextest` は compile した歯の名で選ぶので 4 本とも実際に走る。ゆえに **write-set に持ち、diff は 0 行**という形を取る。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 7 item（`trimmed` / `relay` / `nextest` / `nextest_with` / `nextest_args` / `strip_csi` / `failed_tests`・合計 約 130 行）を、行 am の write-set の `+` の file へ名・本文・順序を変えずにそのまま移す。`struct FailedTest` と `impl FailedTest` は親に残して 1 字も変えず、子は `use super::FailedTest;` の 1 行でそれを引く。
  2. 親に増えるのは **4 行だけ**——`mod` 宣言 1 行、本体が裸で呼ぶ 4 名と git.rs が経由する 1 名の計 5 名を列挙した素の `use` 1 行（`failed_tests` / `nextest` / `nextest_with` / `relay` / `trimmed`・`pub` は付けない）、歯だけが読む 1 名の `#[cfg(test)]` 付きの `use`（`nextest_args`・**属性は別の行に置く**＝`#[cfg(test)]` だけの 1 行と `use nextest::nextest_args;` の 1 行の 2 行）。純移動の証明（`crates/scribe2/src/pipe/move_proof.rs` の `residual_allowed`）が item の外に許す残差の行は、空行・`use` の頭・`mod` 宣言・`#[path` 始まり・**`#[cfg(test)]` だけの行**・`//` 始まり・札に限られ、`#[cfg(test)] use …;` を 1 行に畳むと `residual-line` で落ちる（2026-09-21 の便 4 本目の gate FAIL の再現）。4 行とも 120 桁に収まる。**置き場**（2026-09-21 の便 1 本目の審査が「置き場が無く札の anchor と test 差分の面が別物になる」と指摘した点）: `mod` 宣言は既存の `mod git;` の隣、素の `use` は既存の `use git::load_pairs;` の隣（file 頭の import 群）、`#[cfg(test)]` 付きの `use` は **src の本体の全 item の後・既存の `#[cfg(test)]` + `#[path]` + `mod tests;` の 3 行の直上**に置く。xtask の門（`crates/xtask/src/workspace.rs` の `split_test_src`・`env_reads.rs` / `check_sizes.rs`）と本 doc の歯の区間の切り方は「最初の行頭 `#[cfg(test)]` から末尾」なので、この `use` を file 頭に置くと src の本体が丸ごと歯の区間に落ちる（dispatcher.md §20 で同じ型を実測）。直上に置けば歯の区間の先頭がこの `#[cfg(test)]` の行になるだけで、src の本体は区間の外のまま。`judge_each` / `base_is_green` / `retry_named` / `run_on_base` を含め、親の本体は **1 字も変わらない**（裸の名はこの `use` が解く）。
  3. 歯の 6 file のうち 5 file は**1 行も触らない**（`use super::*;` で引く・write-set の外に在る）。`crates/xtask/src/flipcheck_tests.rs` は write-set に在り、diff は**約束 5 の札 2 行の追加だけ**（`//` 始まりの説明 1 行と `// flip-check: moved <行 am の bead>` の 1 行・既存の `use super::{…}` の直前）で、歯の本文と `use super::{…}` は 1 byte も変えない（その `use` は親の `use` 1 行が解く）。
  4. 上げるのは**子側**の可視性だけで、使う語は `pub(super)` の 1 種類（`pub(crate)` も `pub use` も使わない）。上げる集合は**空でなく、名指しで 6 つ**（`relay` / `nextest` / `nextest_with` / `nextest_args` / `failed_tests` / `trimmed`・全部 fn の頭の行）である。`strip_csi` は**可視性を 1 語も変えない**（子の中だけで呼ばれる）。`struct FailedTest` とその field・`impl FailedTest` の `filterset` は親に残るので可視性を 1 語も変えない。親側の可視性は変えない。
  5. 札 `// flip-check: moved <行 am の bead>` を **`crates/xtask/src/flipcheck_tests.rs` の先頭**（既存の `use super::{…}` の直前・行 j の便 `s2-07l.372` が置いた札の隣・`//` 始まりの説明 1 行と札 1 行の 2 行）と `+` の file の module doc の直後に対で置く（純移動の機械証明は §5.3）。**親 `flipcheck.rs` には札を置かない**（親に増える行は約束 2 の 4 行のまま）。理由（run 4 の gate FAIL `no-test-diff` の再現・2026-09-21）: flip-check の歯の区間は `test_offset` が「次の非空行が `mod` 宣言である行頭の `#[cfg(test)]`」から切るので、`#[path]` を挟む親の末尾 3 行は区間の始点にならず親は src 区間だけの file に見える（§7 の `moved` は歯の区間の札しか数えない・src 区間の札は効かない）。`*_tests.rs` は `is_test_file` が file 全体を歯の区間と読むので、札はそこへ置く。
  6. 割った後の正規化行数は親が **約 1154**（余地 **約 346**）・`+` の file が **約 145**＝余地 346 は行 c の見積（300）を満たす。
- 触らない: 判定の群（`judge_run` / `judge_one` / `Counts` / `ok_line` / `BaseNotGreen` / `base_not_green`）・overlay の群・札と面の群（行 c の面）・`run` と `judge` の外形・歯の 6 file の歯の本文と `use`（そのうち `crates/xtask/src/flipcheck_tests.rs` は write-set に持つが、増えるのは約束 5 の札 2 行だけ・上の 2 つ目の bullet）。
- 却下: 判定の群を移す（84 行で M の余地に届かない）／札と面の群を移す（行 c が同じ群を触る）／歯の file を割る（歯の総数は変わらず親の余地が空かない）。

## 46. 着地の CI 照合が event=schedule の run を数えない（契約表の行 an・`s2-07l.523`・約束の行の形）

やさしく言うと: 着地の最後に「CI は緑か」を聞くとき、同じ commit で cron（定期実行）の workflow も走っていると、それが終わるまで「測れない」になって bead が閉じない。定期実行の run は数えない。

- 出所: memo `s2-07l.523`（便 `s2-07l.522` の終端が `RunDone Landed detail=terminal:ci:unmeasurable`・2026-09-21T10:12Z・同じ sha で cron の `mutants` workflow が走っていた）。
- 現物（verified・main）: CI の判定を読む 1 行の既定は `crates/scribe2/src/pipe/declaration.rs` の `DEFAULT_CI_CMD`（`gh run list --commit {sha} --json status,conclusion`・宣言 `ci-cmd` が無い周に使う）。読み手は `crates/scribe2/src/fleet/wait.rs` の `ci_now(`（JSON の配列を読み、**落ちた run を先に見て** `Failure`、全部 `completed` で `Success`、それ以外は `None`＝測れない）。run の `event`（`push` / `pull_request` / `schedule` …）は読まない。本 repo の workflow は `ci`（push / pull_request）と `mutants`（`schedule`・週 1 の cron）の 2 本で、cron が同じ sha で走る周は完了まで `None` が続き `pipe.ci_wait_s` を使い切って `ci:unmeasurable` に倒れる（bead は閉じず、close は手番になる）。
- 形（読み手で外す・宣言の穴は増やさない）:
  1. 既定の 1 行に `event` を足す（`--json status,conclusion,event`）。宣言 `ci-cmd` の穴は `{sha}` の 1 つのまま。
  2. `ci_now(` は `event` が `schedule` の run を**母集団から外してから**従来の判定を行う（落ちた run 優先 → 全部 `completed` で success → それ以外は測れない）。外した後に run が 0 本なら測れない（`None`）。`event` の欄が無い run（宣言の `ci-cmd` が `event` を返さない周）は外さない＝従来と同じ。絞る処理は `ci_now(` の隣の関数 1 つ（JSON の木の列を受けて schedule でない run の列を返す pure な形）に置き、in-file の歯が欄の不在と `schedule` の値を別々に測る（約束 3・2026-09-21 の便 1 本目の審査 FAIL「欄の不在の規則が契約に無い」の再現）。
  3. workflow の**名では絞らない**（`ci` / `mutants` は repo 固有の値・N3）。外すのは forge が返す `event` の語 1 つ（`schedule`）だけで、その語は `ci_now(` の隣の `const` 1 つが持つ。
- 触らない: `CiRun` の 3 値・`pipe.ci_wait_s`・宣言の `ci-cmd` の形・終端の 3 段（push → CI → close）の順。
- 却下: workflow 名で絞る（repo 固有の値を code に持つ・N3）／cron を別 sha で走らせる運用（散文の規則・N2）／`ci-cmd` に穴を足して宣言側で絞る（宣言が無い consumer に効かない）。

## 47. 着地列の窓が止めた便・終えた便を追随中に数えない（契約表の行 ao・§19 約束 2 (b) の条件を閉じる）

やさしく言うと: 「今 merge していいか」を器に聞く口（§19）が、何日も前に止めた便を「まだ追随中」と数えて永久に「待て」と答える。止めた便・終えた便は追随中ではない。

- 出所（orchestrator の実測 2026-09-22・verified・main 5951736）: docs の PR の merge の前置に `pipe land-window` を撃ったところ `land-window=busy queue=- following=<便 2 本> unpushed=-`。名指された 2 本はどちらも最新の event が `RunStopped`（2026-09-18 と 2026-09-21 に止めた便・bead は close 済み）で、着地の列にも worktree にも居ない。窓は閉じたまま開かず、撃つ側は §19 が消したはずの散文の窓判断（「Gated と push の間の便が無いか」を event log を目で読む）へ戻った（N2）。
- 現物（verified・`crates/scribe2/src/pipe/queue.rs`）: 追随中の便を導く関数は **`RunStage` の event だけ**を便ごとに最後の 1 本まで畳み、段が `Implemented` ∧ detail が `rebase:` で始まる便を返す。`RunStopped` / `RunDone` は kind が `RunStage` でないので読まれず、`Implemented rebase:` の後に止めた・終えた便が追随中のまま残る。一方 (a) の列は `queue_from` → `replay` → `may_queue` で **終端（`Landed` / `Failed` / `Stopped`）の便を外す**＝(a) と (b) の面が非対称。根は §19 約束 2 (b) の文言（「最新の `RunStage` が `Implemented` で detail が `rebase:` の便」）が終端の条件を欠いたまま code に写されたこと＝設計の穴で、実装の逸脱ではない。
- 形:
  1. 追随中の便の条件を **3 つ全部**にする: 最新の `RunStage` が `Implemented` ∧ その detail が `rebase:` で始まる ∧ **replay した段が終端でない**（`may_queue` と同じ 3 語 `Landed` / `Failed` / `Stopped`）。窓の判定（`window_now`）は既に replay を 1 回持っている（(a) の材料）ので、(b) はその同じ 1 回の読みの段を重ねる＝log を 2 度読まない。
  2. 本節の 1 が §19 約束 2 (b) の条件を supersede する（§19 の文言は行 m の着地時の写しとして残す・書き換えない）。(a)(c) の判定・行の字面（`land-window=clear` / `busy queue= following= unpushed=`）・rc・`--wait-s` の再評価は不変。
  3. 追随中の便が「止めた・終えた」だけで外れるのであって、`Implemented rebase:` のまま runner が死んだ便（終端の記帳が無い）は従来どおり数える（死んだ便を列から外す物差しは `s2-07l.388` の行の領分・本節は触らない）。
- 歯（in-file・`queue.rs` の `mod tests`・接頭辞 `pipe_window_following_`・既存の `event` fixture〔`RunStage` の段と ts だけ〕の隣に kind と detail を取る 1 つを足し、pure な関数へ event の列を渡す）: (a) `Implemented` detail `rebase:a..b` の後に `RunStopped`（段 `Stopped`）→ 追随中に数えない（base は数える → RED）。(b) 同じ便が `RunDone` `Landed` で終えた → 数えない（base は数える → RED）。(c) `Implemented rebase:` のまま終端が無い → 数える（不変・(a)(b) が「追随中を全部外す」変異でないことを測る）。(d) `Implemented` の detail が `rebase:` で始まらない → 数えない（不変）。
- 触らない: `may_queue` / `first_gated_at` / `turn_in` / `train_in`・`FOLLOWING` の字面・`MainRead` の 4 値・`Completion::LandWindow` の判定経路・e2e の `pipe_land_window_` の歯 3 本（1 字も変わらない）。
- 却下: 撃つ側が古い便に `RunStage` を足して窓を開ける運用（散文の作法・N2・記帳の門〔§39〕が `RunStopped` の後の `RunStage` を断る）／追随中を時間で切る（「N 時間前の `Implemented` は追随中でない」＝閾値が恣意・終端の記帳が在るのに読まない）／`RunStopped` を別の読みで拾う（log を 2 度読む・replay が既に段を持つ）。

## 48. 終端の台帳の close を repo を cwd にして撃つ（契約表の行 ap・§5.4 の終端の 3 段の 3 段目）

やさしく言うと: 便が着地して CI も緑なのに、最後の「台帳を閉じる」だけが落ちることがある。台帳の道具（bd）は「今いる dir」から台帳を探すのに、器はそれを運転手が起きたときの dir のまま撃っている。起こした側の dir が消えていると台帳が見つからない。着地と同じく repo を dir にして撃つ。

- 出所（orchestrator の実測 2026-09-21T23:51Z・verified）: 便 `s2-07l.540` の終端が `terminal:push:origin` → `terminal:ci:success` の後に `terminal:close:failed:rc=1 or set BEADS_DIR to point to your .beads directory` で止まった（bead は open のまま・手で close）。運転手（driver）は dispatch の周を撃った shell の cwd を継承して起きており（`/proc/<pid>/cwd`）、その cwd が docs 便の worktree で、着地の前に `git worktree remove` されていた（`(deleted)`）。同じ周に起きた `s2-07l.502` の運転手も同じ cwd を持つ。
- 現物（verified・main e6d3134）: 台帳を書く口は `crates/scribe2/src/ledger/mod.rs` の `close`（`Command::new(bd).args([close, <bead>, --reason, <text>]).output()`・**`current_dir` を付けない**＝親 process の cwd を継承する）の 1 site だけで、呼び手は `crates/scribe2/src/pipe/land/finish.rs` の終端（3 段目・`entry.bd` と `entry.bead` を渡し `entry.repo` は渡さない）。同じ finish の push と PR の道具は `sh -c` を `current_dir` = `entry.repo` で撃つ（60–70 行）＝終端の 3 段のうち close だけが cwd を repo に固定していない。台帳を**読む**口（`crates/scribe2/src/seat/ledger.rs` の `spawn_read`）は `current_dir` を呼び手から受けた cwd で撃つ＝読みは既に固定されている。bd は `.beads` を cwd から上へ探す（断り文の字面）ので、cwd が消えた dir なら台帳を解けない。
- 形:
  1. `close` は repo を受けて `current_dir` = repo で撃つ（読みの `spawn_read` と同じ形・引数が 1 つ増える）。呼び手の終端は `entry.repo` を渡す。cwd に依らず、同じ repo なら同じ台帳を閉じる。
  2. 断りの 2 値（`Unlaunchable` / `Refused { rc, tail }`）と終端の記帳の字面（`close:ok` / `close:failed:…`）は不変。repo が読めない周は従来どおり `bd` の rc と stderr の末尾がそのまま `Refused` に運ばれる（新しい variant は足さない・C17.1）。
  3. 運転手の cwd は本行では触らない（dispatch が子を起こす cwd の固定は起こす側の別の面・§12 の `--repo` の絶対化と同じ列）。
- 歯（接頭辞 `pipe_terminal_land_close_cwd_`・in-file は `crates/scribe2/src/ledger/mod.rs` の `mod tests`（既存の `pipe_terminal_land_close_` の歯の隣・偽 bd の shell script を書く fixture の型）・e2e は `crates/scribe2-boundary/tests/e2e/pipe/land.rs` の終端の歯の隣（`fake_terminal` の偽 bd）: (a) in-file: 偽 bd が `pwd` を stdout に書いて rc 0 で終わる script で、`close` を「今の cwd とは別の dir」を repo として撃つと、書かれた path が repo に等しい（base は `close` が repo を受けないので compile が止まる＝道具不在の RED）。(b) in-file: 偽 bd が rc 1 と stderr 2 行で断る周は `Refused { rc: Some(1), tail: <末尾の 1 行> }`（不変・既存の歯と同じ形で「cwd を固定しても断りの形が変わらない」を測る）。(c) e2e: 偽 bd が cwd を record に残す fixture で、Gated PASS の便を **消える dir を cwd にした子 process** から `pipe land` すると終端が `terminal:close:ok` まで進み、record の cwd が repo（base は `close:failed`・`(deleted)` の cwd を継承）。
- 触らない: 終端の 3 段の順（push → CI → close）と CI の照合・`CloseError` の 2 値・`CLOSE_REASON` の字面・台帳を読む口・dispatch が子を起こす cwd。
- 却下: 運転手の cwd を repo に固定して close の側は触らない（起こす側が複数〔dispatch の周・手の `pipe run`・`--terminal-only`〕で、撃つ側の 1 site を直す方が小さい・C17.4）／`BEADS_DIR` を env で渡す（env の縫い目が 1 つ増える・C2.2・repo を cwd にすれば要らない）／close を `sh -c "cd <repo> && bd …"` で包む（shell を 1 段増やす・`Command` の `current_dir` で足りる）。

## 49. 契約の赤でない Gated FAIL を同じ worktree で再 gate する口 — 裁定の逐語を持つ 1 段戻しを、その FAIL につき 1 回だけ許す（契約表の行 ar・`s2-07l.240`）

- 出所（`s2-07l.240`・orchestrator の再実測 2026-09-22・verified・main f25084c）: `.208` の run 3 は器の欠陥（検出線の同名衝突）で、run 4 は上限（環境）で追随の再 gate に落ちた。どちらも実装は合格していたのに、`Gated` の FAIL は終端なので新しい取り込みで runner が一から作り直した（費用 = runner 2 周 + gate 4 周）。memo が待つとした先行（`s2-07l.335` / `.428` / `.502`）は 3 本とも close 済みで、穴だけが残っている。
- 現物（本行の base・main f25084c・verified）:
  - 段が生きているかを測る述語は 1 本（`crates/scribe2/src/pipe/cli/state.rs` の `live`・可視性は `pub(in crate::pipe)` で、行 ar の write-set の `+` の file は `crates/scribe2/src/pipe/` の配下ゆえそのまま呼べる＝`state.rs` は触らず write-set にも入れない。親 module の file は `crates/scribe2/src/pipe/mod.rs` の 1 本で `pipe.rs` 形は無い・母集団 = 段 11 個の網羅の match）。`Gated` の答えだけが判定から導かれ、**判定が FAIL の周は `Some(false)`＝終端**である。`Landed` / `Failed` / `Stopped` は段だけで終端になる。
  - 起こし直しの候補を選ぶ述語（`crates/scribe2/src/pipe/dispatch.rs` の `passed_gate`）は `Gated` ∧ 判定が PASS の周しか拾わない＝FAIL の `Gated` は候補にならない。
  - `release` の印が列へ戻す段は `Gated` / `Stopped` / `Failed` の 3 つ（同 file の `requeues`・in-file の歯が母集団 11 段で pin）だが、戻すのは **bead の印**であって便ではない＝戻り先は新しい取り込みで、worktree は作り直しになる。
  - 同じ worktree で段を戻す口は `crates/scribe2/src/pipe` の src 全数 grep で **0 件**（`pipe` の subcommand は 15 個で、字面は `crates/scribe2/src/pipe/cli.rs` の 1 か所の表が正本）。`regate` の字面は src に 12 件（`crates/scribe2/src/pipe/land.rs` の追随の後の再 gate と `crates/scribe2/src/pipe/gate/record.rs` の skipped=regate の記録）在るが、どれも追随の撃ち直しであって終端の便を戻す口ではない。
  - 裁定を運べる形は既に在る: 便の event は `RunStage` が段と `detail` を運び、`QuestionAnswered` が逐語を運ぶ（どちらも既存の種別）。本節は**種別も段も 1 つも足さない**。
- 形（番号は done と歯に 1:1 で対応する）:
  1. **口を 1 つ足す**: `pipe regate --run <id> --reason <逐語>`。本体は行 ar の write-set の `+` の file で、`pipe` の subcommand の表（`crates/scribe2/src/pipe/cli.rs`）と flag の表（`crates/scribe2/src/pipe/cli/args.rs`）にそれぞれ 1 行足す。usage の 1 行も増えるので、usage を逐語で pin する外形の snapshot（`crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap`）を同じ便で更新する（増えるのはその 1 行だけ）。
  2. **受け付けるのは 4 つ全部を満たす周だけ**: 段が `Gated` ∧ 判定が FAIL（`live` が `Some(false)` を返す面）∧ 運転手の札が無いか所有者が死んでいる ∧ `--reason` が非空。1 つでも外れたら rc 1 で**何も書かない**（測れない周〔`live` が `None`〕も断る・fail-closed）。器は「契約の赤かどうか」を自分では判定しない——それは裁定であって述語ではない（C5）。口は裁定の逐語を記帳するだけである。
  3. **戻す先は `Implemented`・記帳は 1 件**: `RunStage` を 1 件（段 = `Implemented`・`detail` = `regate:` + 逐語・actor は human）。worktree・commit・判定の file には 1 byte も触らない。段が `Implemented` に戻ると `live` は `Some(true)` を返し、既存の列（`pipe run` / `resume` / 列の 1 周）が**同じ便 id の同じ worktree**で gate をもう 1 周撃つ（新しい取り込みを通らないので worktree を作り直す経路に入らない）。列の 1 周がこの便（札が無いか所有者が死んでいる便）を拾う条件は [dispatcher.md](./dispatcher.md) §23（[FR68](../../design-intent/spec/srs.html#FR68) の 4 種目）である。
  4. **1 つの FAIL につき 1 回だけ**: 便の event を畳み、**最新の `Gated` の `RunStage` より後ろ**に `detail` が `regate:` で始まる `RunStage` が在る周は断る。もう 1 周の gate が `Gated` を書けば次の 1 回が開く＝回数の閾値を値で持たない（恣意の無い上限）。
  5. **行の字面**: rc 0 の周は `regate: run=<id> from=Gated to=Implemented` の 1 行。断る周は理由 1 行で rc 1。
- 触らない: `passed_gate`（PASS の候補）と `requeues` の 3 段と `release` の印・`live` の他の 10 段の答え・判定の file の読み書きと `Verdict` の 3 値・gate の中身と追随の形・新しい取り込みの経路・`pipe stop` の終端の極性・運転手の札の 4 値。段の種別（11 個）と event の種別（19 個）はどちらも 1 つも増えない。
- 却下:
  - **memo の (b)（新しい取り込みの runner に前 run の commit の cherry-pick を許す）**: 器を変えない代わりに、成果の保全（C9）を runner の判断に預ける。同じ事故が次の周も起こり、何が引き継がれたかが記録に残らない。
  - **memo の (c)（追随の再 gate を検出線と上限だけの軽い周にする）**: 追随で挙動が変わる可能性を測らないことになる。落ちているのは「終端だから捨てる」であって gate の重さではない。
  - **段か event の種別を 1 つ足して裁定を typed にする**（memo の (a) の素直な形）: 閉じた型が 2 つ動き、記帳の形が跨版の面になる（ADR 条件 3）。`RunStage` の `detail` で同じ逐語が運べるので、型を増やさない側を採る（C17.1）。**ゆえに本節は ADR を要さない**——遷移表に載る段の組が 1 つ増えるだけで、on-disk の形も外部依存も動かない。
  - **器が FAIL の理由を分類して自動で戻す**: 「器の欠陥か・上限か・環境か・契約の赤か」は裁定であって述語ではない。自動で戻すと、契約の赤を無限に撃ち直す形が作れてしまう。
  - **戻せる回数を rules 行の値にする**: 値の線と裁定が 1 つ増える（C5 / A2）。形 4 の「最新の `Gated` より後ろに 1 件」は値を持たずに同じ上限を作る。
  - **終端の判定そのものを緩める**（FAIL の `Gated` を live に倒す）: 着地の列・排他の母集団・`pipe stop` の断りが全部動く。戻すのは口の側の 1 件の記帳で足りる。
- 歯（接頭辞 `pipe_regate_`・`crates/` 全体の fn 名の substring として base に 0 件。in-file は行 ar の write-set の `+` の file の `mod tests`・e2e は `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`）:
  (a) 形 2 の核: 受付の pure な述語に、4 つの条件を 1 つずつ外した 4 形と全部満たす 1 形を渡す（母集団 = 5 形を assert に出す）。通るのは 1 形だけで、`live` が `None` の周は断る側である。
  (b) 形 3: 通った周に書かれる event が **1 件**で、種別が `RunStage`・段が `Implemented`・`detail` が `regate:` の後ろに逐語をそのまま持つ（逐語は入力と別の字面の fixture で測り、`detail` の出所を弁別する）。
  (c) 形 4: 同じ便に 2 度撃つと 2 度目が断られ、間に `Gated` の `RunStage` を 1 件挟めば 3 度目が通る（3 形を対で）。
  (d) 形 1 / 形 5（e2e）: 判定が FAIL の `Gated` の便に口を撃つと rc 0 で行が出て、その後の段が `Implemented` になり、便の worktree の path が 1 字も変わらない。base は subcommand の表に字面が無く使い方の誤りで断られる＝機能不在の RED。
- 閉包の連鎖: 行 ar の `+` の file は `Stage` の変種（`Gated` / `Implemented`）を名指すので、`crate::fleet::Stage` を touches に持つ [contract-source.md](./contract-source.md) の行 c の閉包に入る。同じ path を行 c の write-set に載せておく（base に無い file は行 ar の `+` の宣言で解ける・§20）。

## 50. 固定日付を持つ fixture の母集団を歯で pin する（契約表の行 as・`s2-07l.469`・歯だけ・retroactive 札）

- 出所: memo `s2-07l.469`（.468 の notes・planner 2026-09-18T01:4xZ）。.468 の直しは commit `f4eac60`（`crates/scribe2-boundary/tests/e2e/fleet.rs` の 1 file・+69 / −36）。
- **何が起きているか（母集団の実測・main f25084c・verified）**:
  1. **全体**: `crates/` の tracked な `.rs` は **176 本**。引用符に囲まれた日付の形（`YYYY-MM-DD`）の字面を持つ file は **27 本・のべ 281 件**。大半は裁定 id（`ruled_at` / `ruling` の字面）と event の `ts` と散文で、壁時計と比べる経路には渡らない（`crates/scribe2-boundary/tests/e2e/rules.rs` が 73 件・`crates/scribe2-boundary/tests/e2e/fleet.rs` が 56 件・`crates/scribe2/src/fleet/usage.rs` が 35 件）。
  2. **絞り込み（reset / 期限の欄に座る字面だけ）**: 281 件のうち、同じ行に reset か期限の key（`resets_at` / `reset_at` / `RESETS_AT` / 末尾が `_RESET` の定数名 / `expiresAt`）を持つ行は **31 行・6 file**。内訳は `crates/scribe2/src/fleet/select_tests.rs` 6 行・`crates/scribe2/src/fleet/usage.rs` 11 行・`crates/scribe2/src/fleet/wait.rs` 2 行・`crates/scribe2-boundary/tests/e2e/fleet.rs` 9 行・`crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` 2 行・`crates/scribe2-boundary/tests/e2e/seat.rs` 1 行。
  3. **(a) 壁時計と比べない側（19 行・3 file）**: `crates/scribe2/src/fleet/select_tests.rs` と `crates/scribe2/src/fleet/usage.rs` と `crates/scribe2/src/fleet/wait.rs` の in-file の歯。選定の純関数は「いま」を引数で受ける（`crates/scribe2/src/fleet/select.rs` の 188 行が `now` の欄・338 行がその欄と reset を比べる唯一の式）ので、fixture と「いま」が両方とも固定値＝時限にならない。`crates/scribe2/src/fleet/usage.rs` の 11 行は本文の読みと 1 行の組み立ての歯で、比較そのものを持たない。`crates/scribe2/src/fleet/wait.rs` の 2 行は待ちの種別の網羅 match の材料で、reset を読まない。
  4. **(b) 壁時計と比べる側（12 行・3 file）**: `crates/scribe2-boundary/tests/e2e` の下の 3 file。ここは器の binary を起こすので「いま」は器が読む（`crates/scribe2/src/fleet/cli.rs` の 482 行の `now_utc` が唯一の口・呼び手は同 file の 180 行と `crates/scribe2/src/pipe/ratelimit.rs` の 296 行 / 326 行と `crates/scribe2/src/fleet/wait.rs` の 335 行）。12 行の形は 2 つに割れる——**年が 2099 以上の番兵が 7 行**（`crates/scribe2-boundary/tests/e2e/fleet.rs` の 3561 / 3564 / 3913 / 3959・`crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` の 182 / 185・`crates/scribe2-boundary/tests/e2e/seat.rs` の 555）と、**年が 2099 未満の字面が 5 行**（`crates/scribe2-boundary/tests/e2e/fleet.rs` の 876 / 877 / 881 / 882 / 1039）。
  5. **(b) の 5 行が今日 赤くないことの根拠**: 5 行の字面はどれも過去（2026-09-12）である。過去の reset が壁時計と比べられれば選定はその口座を古いと読んで候補から落とし、.468 と同じ赤になる。main は f25084c で緑ゆえ、この 5 行は比較に届いていない——`crates/scribe2-boundary/tests/e2e/fleet.rs` の 876〜882 は木の読みの歯の本文・1039 は event の直列化と replay の歯が使う定数で、どちらも器の選定を通らない。
  6. **.468 の形は 1 か所だけ着地している**: `crates/scribe2-boundary/tests/e2e/fleet.rs` の 1850 行の遅延 static が、壁時計の今日から 5 時間窓 = 翌日 05:00Z・7 日窓 = 7 日後 00:00Z を器の `format_utc` で組む。同じ「壁時計から組む」形は `crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` の偽 curl（応答の直前に日付の道具で作る）と `crates/scribe2-boundary/tests/e2e/seat.rs` の 641 行の helper にも在る。つまり (b) の 12 行は **番兵・壁時計から組む・過去の字面** の 3 形が同居していて、どれを使うかを**機械が測っていない**。
  7. **数の欄の族は上の母集団に入らない**: 期限は数（1970 年からのミリ秒）で書かれる面も持つ（`crates/scribe2/src/fleet/usage/read.rs` の 69 行が壁時計と比べる）。fixture 側は遠い未来の 1 定数と「必ず期限切れ」の小さい値の 2 形で、日付の字面ではないので走査に当たらない。本行は日付の字面だけを的にする。
- **形**（歯だけ・器の src も fixture も 1 字も動かさない）:
  1. `crates/scribe2-boundary/tests/e2e/main.rs` の既存の歯（`e2e_fixture_` の接頭辞・13 本）の隣に**母集団を数える歯 1 本**を足す。`git ls-files` で e2e の下の tracked な `.rs`（base 29 本）を引き、各 file を読み、reset / 期限の key を持つ行のうち日付の形の字面を持つ行を数え、年が 2099 以上の本数と 2099 未満の本数に割る。
  2. assert は **3 つ組の等値 1 本**（file 数・番兵の本数・2099 未満の本数）で、message に母集団の全数と当たった行の file 名を出す（0 件を「変化なし」と読まない・[contract-source.md](./contract-source.md) §31 の柵と同じ）。base の値は (29, 7, 5)。
  3. key の字面は歯の file 自身に当たらない形で組む（`concat!` で 2 片に割って連結する・`crates/scribe2-boundary/tests/e2e/pipe.rs` の 1302 行の既存の歯と同じ手）。割らないと歯の file が母集団に自分を数えて、数が 1 本ずれる。
  4. 札 `// flip-check: retroactive s2-07l.469` を歯の fn の中の行頭に 1 行置く（base でも緑になる歯ゆえ・効く条件は test 区間内 / 行頭 / bead id / base に無い、の 4 つ）。
- **触らない**: 器の src 全部（選定の式・「いま」の口・期限の読み）・(b) の 12 行の fixture の字面・(a) の 19 行・数の欄で書かれた期限の fixture・`crates/scribe2-boundary/tests/e2e` の他の歯。
- **却下**:
  - (b) の 7 行の番兵を全部「壁時計から組む」形へ書き換える — 番兵は時限ではない（2099 年まで古くならない）ので直す理由が無く、期待の側の字面（`crates/scribe2-boundary/tests/e2e/fleet.rs` の 3743 / 3747 / 3784 / 3936 が stderr の 1 行に同じ年を持つ）まで連れて動くので、歯だけの便が fixture の書き換えに化ける。
  - (b) の 5 行を番兵へ揃える — 5 行は比較に届かない場所に在り（上の 5.）、揃えても測れる面が増えない。揃えた瞬間に「過去の reset を fixture に置けない」という測っていない規律が字面で増える（C1 / N2）。
  - 日付の字面を repo 全体で 0 件にする規則 — 母集団 281 件の大半は裁定 id と event の `ts` で、消せば裁定の出所が消える。
  - `xtask check` の門に足す — 門は src の形を測る面で、歯の fixture の形は歯で測る（新しい門を 1 つ増やさない・C17.4）。
- **歯（接頭辞 `e2e_fixture_clock_`・行の契約が持つ）**: 上の (2) の 3 つ組。`crates/` 全体で `e2e_fixture_` を名に持つ fn は `crates/scribe2-boundary/tests/e2e/main.rs` の 1 file にしかない（実測）ので、verify の filter は他の file へ広がらない。**空虚さの柵**: 番兵の本数と 2099 未満の本数を別々の欄で持ち（1 本で 2 本を兼ねない）、message に母集団の全数と当たった行を出す。**変異の A/B**（done の条件・proof は便の notes）: e2e の file に reset の欄を持つ 2026 年の行を 1 行足すと 2099 未満の欄が 5 → 6 で赤・2099 年の行を 1 行足すと番兵の欄が 7 → 8 で赤・e2e に `.rs` を 1 本足すと file 数の欄が 29 → 30 で赤。

## 51. stop の全部止めを live な便の本数で絞り、止めた便を母集団つきで返す — 便が 2 本以上の周は逐語を要り、逐語は止めた便の記帳に残る（契約表の行 at・`s2-07l.459`）

- 出所（`s2-07l.459`・admin 報告 2026-09-17T21:16Z・orchestrator の再実測 2026-09-22・verified・main b0e6e07）: 全部止めを usage の確認のつもりで撃ち、実装中の便 1 本（17 分）を不可逆に失った。停止は「消す」ではないが、走行中の便の token と時間を戻せない形で捨てる（A4.2 の隣）。
- 現物（本行の base・main b0e6e07・verified）:
  - `crates/scribe2/src/pipe/stop.rs` は 485 行で、入口（21 行）が `--run` の有無だけで 2 経路に割れる。`--run` が無い周は全部止め（175 行）へ落ち、181〜183 行が `--all` の不在を rc 1 で断る（**引数なしは何も止めない**＝memo の論点 2 は着地済み）。
  - **理由を受ける口は無い**: `--reason` の字面は `crates/scribe2` の src 全数で 1 件だけで、それは台帳を閉じる側（`crates/scribe2/src/ledger/mod.rs` の 75 行）である。`crates/scribe2/src/pipe/stop.rs` の `reason` 6 件は全部 `Err(reason)` の変数名で、flag ではない。
  - **live の本数で分岐する式は 0 件**: 全部止めが数える `live` は**席**の列（`SeatState` が Live の席）で、`live.len()` は席の数である。席が指す**便**の本数で受付を分ける式は同 file に 1 つも無い。
  - **止めた便の一覧は捨てられている**: 全部止めは記帳順の便の列（`stopped_runs`）と止め切れなかった便の列（`unstopped_runs`）を組み立てるが、stdout に出るのは `stop: seats=<席数> stopped=<席数>` の 1 行だけで、**どの便を倒したかは残らない**（memo の論点 3）。
  - 受ける flag の表は `crates/scribe2/src/pipe/cli/args.rs` の 168〜169 行（置き場 3 つ + `--run` の値 + `--all` の switch + 道具 4 つ）、usage の 1 行は `crates/scribe2/src/pipe/cli.rs` の 148 行にある。
  - 逐語を運ぶ形は既に在る: 席を止めた記帳は detail を受ける（`crates/scribe2/src/pipe/stop.rs` の 121 行が運転手の席に `stopped-by-stop` を載せる）。便の終端の記帳（363 行）だけが detail を持たない。本節は**段の種別も event の種別も 1 つも足さない**。
- 形（番号は done と歯に 1:1 で対応する）:
  1. **全部止めの受付を 2 段にする**: Live な席が指す**別々の便**の本数を先に数え、2 本以上の周は `--reason <逐語>` が無ければ rc 1 で断る（何も止めず、events は 1 件も増えない）。1 本以下の周は従来どおり `--all` だけで通る（対象なしの rc 0 の冪等も不変）。断りの 1 行は便の本数を母集団として名指し、`--run` で 1 本ずつ止める道と逐語を付ける道の両方を書く。
  2. **`--reason` を値つきで受ける**: `crates/scribe2/src/pipe/cli/args.rs` の stop の表に 1 つ足し、`crates/scribe2/src/pipe/cli.rs` の usage の 1 行に写す。`--run` と `--reason` を同時に渡す周は rc 1 で断る（1 本を外す操作に爆風は無く、逐語の行き先も無い——使い方の誤りを黙って落とさない・SRS NFR4）。
  3. **逐語は止めた便の記帳に残る**: 便を終端にする記帳に detail を 1 つ渡せるようにし（`crates/scribe2/src/pipe/stop.rs` に閉じた 2 呼び手）、全部止めの周は `reason:` の後ろに入力の逐語をそのまま載せる。`--run` の周は従来どおり detail を持たない。
  4. **止めた便を母集団つきで返す**: rc 0 の 1 行に、終端を記帳した便の id を記帳順で並べる欄と、止め切れなかった席を持つ便の id を並べる欄を足す（席数と止めた席数の 2 欄は不変・空の欄は出さない）。行は 1 本のまま増やさない。
- 触らない: `--run` の経路の段と極性（終端の便は rc 1・止め切れない周は終端にしない）・rc の 3 値（0 / 1 / 2）・停止の順（席 → 運転手 → 便の終端）・猶予の規則の行・席を選ぶ述語・pid を持たない Live 席を母集団に数える形・運転手の札の扱い・停止中の印。
- 却下:
  - **本数の線を規則の行の値にする**: 2 は調整できる閾値ではなく構造の境目である（live な便が 1 本以下なら、全部止めは `--run` 1 本と同じ爆風しか持たない＝絞る意味が無い）。値の線と裁定 id を 1 つ増やさない（C5 / A2）。
  - **対話面の承認の event を足す**（memo の後半の案）: 器に対話の口と event の種別が 1 つ増える。逐語 1 つで「誰が何のつもりで撃ったか」は同じだけ残る。
  - **全部止めを消して `--run` だけにする**: 席の掃除の冪等な 1 コマンドが無くなる（SRS FR13 が名指す形そのもの）。
  - **2 本以上の周を `--run` だけに倒す**（逐語の道を作らない）: host を畳む周に便の本数だけ撃つことになり、同じ誤操作が「連打」の形で戻る。
  - **止めた便の一覧を別の行や構造化した形で返す**: stdout の 1 行形（`stop:` で始まる 1 行）は stop の既存の面で、読み手（運転手の待ち手）が行数を前提にしている。欄を足す側に倒す。
- 歯（接頭辞 `pipe_stop_scope_`・`crates/` 全体の fn 名の substring として base に 0 件。in-file は `crates/scribe2/src/pipe/stop.rs` の `mod tests`・e2e は `crates/scribe2-boundary/tests/e2e/pipe/stop.rs`）:
  (a) 形 1 の核: 席の列から別々の便を数える純な述語に、便 0 / 1 / 2 / 3 本の 4 形を渡す（母集団 = 4 形を assert の message に出す）。同じ便を指す席が 2 つ在る形を 1 本と数えることを、席数と便数が食い違う形で測る。
  (b) 形 1 / 2 の受付: 便が 2 本 live な置き場に対し、`--all` だけ・`--all` と逐語・`--run` と逐語 の 3 形を撃つ（母集団 3 形）。通るのは 2 形目だけで、断る 2 形は events が 1 件も増えず席も生きたままである。
  (c) 形 3: 通った周に書かれた便の終端の記帳が、`reason:` の後ろに入力の逐語をそのまま持つ（逐語は fixture の他の字面と衝突しない形で渡し、detail の出所を弁別する）。`--run` で止めた便の記帳は detail を持たない。
  (d) 形 4（e2e）: 便 2 本を止めた周の rc 0 の 1 行が、席数と止めた席数に加えて 2 本の便 id を記帳順で持つ。1 本の席が止まらない周は、その便が止め切れなかった側の欄にだけ出て、止めた側の欄には出ない。base は逐語の flag が使い方の誤りで断られる＝機能不在の RED。
  (e) 形 1 の逃がし（e2e・done (2)）: 便が 0 本と 1 本 live な置き場に `--all` だけを撃つ 2 形（母集団 2 形）で、どちらも従来どおり rc 0 で通り、0 本の周は対象なしの字面のまま・1 本の周はその便を止める（逐語なしで通る側を pin する）。
  (f) 形 2 の外形（既存の歯・done (3)）: usage の 1 行は `crates/scribe2-boundary/tests/e2e/pipe.rs` の既存の snapshot 歯 `pipe_external_form` が pin する。同じ便で snapshot を更新し、verify に同じ歯を名の全体で持つ（`.rs` は触らない＝write-set には置き場だけの印で載せる。審査の要約がこの印を読めるのは [contract-source.md](./contract-source.md) §44 / 行 au の後＝本行はその便の後に出す）。
  (g) 既存の歯の不変（done (6)）: `crates/scribe2-boundary/tests/e2e/pipe/stop.rs` の既存の `pipe_stop_` の歯を verify の e2e 行で丸ごと撃つ（新設の歯と同じ file・接頭辞は既存の名の全体を含む）。
- 審査の器の世代（orchestrator 2026-09-22・便 134836Z の INCONCLUSIVE の再現・verified）: 本行の write-set の `=` の項目は、審査の材料の base の要約が印を剥がして読む世代（行 au・[contract-source.md](./contract-source.md) §44 の着地 86990e2 以後）の器でしか要約に載らない。旧世代の器で審査した便は section-material-missing で止まるので、本行は行 au 着地後の世代の器（PATH の binary を入れ替えた後）で審査する。
- usage の外形: `--all` の行の flag が増えるので `crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap`（pipe の usage 行を逐語で pin する既存の snapshot）を同じ便で更新する（行 at の write-set に含める・§49 形 1 と同じ形）。

## 52. gate の段の便が base を追随できる口 — 木だけを main の先端へ載せ替えて段を実装へ戻す 1 本を、land の外に開く（契約表の行 au・`s2-07l.470` の論点 4）

- 出所（`s2-07l.470` の notes・planner 2026-09-18T02:2xZ・orchestrator の再実測 2026-09-22・verified・main b0e6e07）: main の赤を直した便が着地した後も、旧い base の木で走る便の gate は時限の歯で赤のままである。`Gated` の FAIL は終端なので、実装 1 周を捨てて取り込みからやり直した。
- 現物（本行の base・main b0e6e07・verified）:
  - **追随の口は着地の中の 1 本だけ**: `crates/scribe2/src/pipe/land.rs` の 400 行が同 file 576 行の追随を呼ぶ唯一の呼び手で、`crates/scribe2/src/pipe/gate.rs` に「追随」の字面は **0 件**である。
  - 木を載せ替える実体は `crates/scribe2/src/pipe/land.rs` の 746 行で、着地の文脈（衝突の起こし直し・上限・便の写し）を丸ごと受けるので**着地の外から呼べない**。祖先の判定と木の clean の判定は `crates/scribe2/src/pipe/follow.rs` と同 file の 937 行が持つ既存の 1 本ずつである。
  - §33（行 aa）は追随の**後**に再 gate を省く規則で、base を進める口ではない。§49（行 ar）は判定 FAIL の便を同じ木のまま実装へ戻す口で、**木の base は動かさない**＝時限の歯はもう 1 周も赤である。
  - 段が生きているかを測る述語は `crates/scribe2/src/pipe/cli/state.rs` の `live`（§49 と同じ 1 本）。本節は**段の種別も event の種別も 1 つも足さない**。
- 形（番号は done と歯に 1:1 で対応する）:
  1. **口を 1 つ足す**: 便 1 本を名指して木だけを追随させる subcommand（字面は `pipe follow --run <id>`・受ける flag は置き場の 3 つと `--run` だけ）。本体は行 au の write-set の `+` の file で、`crates/scribe2/src/pipe/cli.rs` の subcommand の表と `crates/scribe2/src/pipe/cli/args.rs` の flag の表にそれぞれ 1 行足す。
  2. **受け付けるのは 5 つ全部を満たす周だけ**: 便が在る ∧ 段が終端でない（測れない周は断る・fail-closed） ∧ 運転手の札が無いか所有者が死んでいる ∧ 木が clean ∧ 記録した base が main の祖先。1 つでも外れたら rc 1 で**何も書かない**。
  3. **動かすのは木だけ**: main の先端へ載せ替え、段を実装へ戻す記帳を 1 件書く（detail は着地の追随と**同じ字面**）。gate は撃たない・main は 1 byte も動かさない・PR も押さない・runner も起こさない。段が `Gated` だった周も実装へ戻るので、既存の列が gate をもう 1 周撃つ。
  4. **衝突した周は木を戻して断る**: 載せ替えが衝突したら中止して木を撃つ前の姿へ戻し、rc 1 で何も書かない。着地側の衝突の経路（runner を起こし直す・上限を数える）は通さない——この口は測って戻すだけである。
  5. **載せ替えの実装は 1 本に畳む**: repo と木と 2 つの sha だけを受ける載せ替えの 1 段を行 au の write-set の `+` の file に置き、`crates/scribe2/src/pipe/land.rs` の 746 行はその 1 本を呼ぶ（追随の載せ替えの実装を 2 本にしない・C3.4）。着地側の差分は呼び出しの 1 行である。
  6. **行の字面**: rc 0 の周は `follow: run=<id> rebase=<base>..<main>` の 1 行（便 id と載せ替えの 2 つの sha）。断る周は理由 1 行で rc 1（衝突の後に木を戻せなかった周だけ rc 2）。
- 触らない: 着地の追随の全体（衝突の起こし直し・上限・stale な行・既着地の判定）・§33 の判定の引き継ぎ・§30 の検出線・gate の判定順と verdict の 3 値・主実測・§49 の口・段の種別と event の種別（どちらも増やさない）・受付と列の経路。
- 却下:
  - **gate の前に器が自動で追随する**: main が動くたびに全部の便の木が動き、gate の前提（測った木と着地する木が同じ）が人の知らない間に変わる。口は人が撃つ 1 本にする。
  - **判定 FAIL の便をこの口で実装へ戻す**: §49（行 ar）と役割が重なる。本節は**段を戻す判断を持たない**——実装へ戻すのは木が変わったからで、判定の可否は既存の列が決める。
  - **追随の回数に上限を置く**: 値の線と裁定 id が 1 つ増える（C5 / A2）。上限は main が動いた回数で自然に決まる。
  - **着地の 746 行を丸ごと `+` の file へ移す**: 衝突・stale な行・既着地の 3 経路が着地の文脈を要るので、移すと純移動でなく作り直しになる。畳むのは git の 1 段だけにする。
  - **gate の中で base の遅れを測って断る**: 遅れを名指すだけで前へ進まない。main が動くのは日常なので、断りが既定になる。
- 歯（接頭辞 `pipe_follow_step_`・`crates/` 全体の fn 名の substring として base に 0 件。in-file は行 au の write-set の `+` の file の `mod tests`・e2e は `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`）:
  (a) 形 2 の核: 受付の純な述語に、5 つの条件を 1 つずつ外した 5 形と全部満たす 1 形を渡す（母集団 = 6 形を assert の message に出す）。通るのは 1 形だけで、段を測れない周は断る側である。
  (b) 形 3: 通った周に書かれる event が **1 件**で、段が実装へ戻り、detail が着地の追随と同じ接頭のあとに 2 つの sha を持つ。木の載せ替えの前後で main の sha が 1 字も変わらない。
  (c) 形 4: 衝突する commit を main に積んだ周は rc 1 で、木の先端が撃つ前と同じ sha に戻り、events が 1 件も増えない。
  (d) 形 1 / 6（e2e）: `Gated` の便にこの口を撃つと rc 0 で 1 行が出て、その後の段が実装になり、木の base が main の先端になる。base は subcommand の表に字面が無く使い方の誤りで断られる＝機能不在の RED。
  (e) 形 5: 着地の追随の既存の歯（`crates/scribe2-boundary/tests/e2e/pipe/land.rs` の追随の群）が 1 字も変わらず緑である（載せ替えの 1 段を畳んでも着地の 3 経路が動かない）。
- usage の外形: subcommand の表に 1 語増えるので `crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap`（pipe の usage 行を逐語で pin する既存の snapshot）を同じ便で更新する（行 au の write-set に含める・§49 形 1 と同じ形）。
- 閉包の連鎖: 行 au の `+` の file は段（`Stage`）の変種を名指すので、`crate::fleet::Stage` を touches に持つ [contract-source.md](./contract-source.md) の行 c の閉包に入る。同じ path を行 c の write-set に載せておく（§51 と同じ形）。

## 53. 入口の flip-check が rename を対にして読む — 変更 file の列挙を name-status で取り、R の行は base 側を旧 path で読む（契約表の行 av・`s2-07l.198.2` の便 135324Z の問い）

- 出所（`s2-07l.198.2` の 7 便目の問い about:write-set・orchestrator が便の staged の写しを別 worktree に commit して再現・verified・base bc044ce）: 純移動 49 file（同一本文の対 47・src だけが動いた bin 本体 1・test 区間の相対 path が動いた e2e 1）を含む木に入口の flip-check を撃つと `too-many-marks marks=80 limit=16` で落ちる。札を外しても各 file は base に無い扱いで `not-flippable` か `green-on-base` に落ちるので、純移動の便が gate を通る経路が無い。[core-boundary.md](./core-boundary.md) §5 の表は flip-check の判定を対象外と置いたので、これは行 b の write-set の外の設計の穴である。
- 現物（main bc044ce・verified）: `crates/xtask/src/flipcheck/git.rs` の変更 file の列挙は `git diff --name-only` で、rename された file は HEAD の path でしか現れない。`load_pairs` は同じ path で base と HEAD を読むので、移した file は base が無い「新規」の対になり、test 区間に在る既存の札（他の便の `retroactive` / `moved`）が全部この便の札に数えられる（持ち越し〔§7〕は base の test 区間に同じ札が在ることで測るので、base が無いと 1 本も持ち越せない）。
- 形:
  1. 列挙を `git diff --name-status -M <base>...HEAD` で取り、R の行は（旧 path・新 path）の対、A / M / D の行は従来どおり 1 path の対にする。対は base 側の path を持ち（`rel` は HEAD の path のまま）、base の本文は旧 path で読む。
  2. 対の test 区間が同一なら test-diff 無し＝flip に数えず、札は base の test 区間に在るので持ち越し（この便の札に数えない）。src だけが動いた対も同じ。test 区間に差が在る対は従来の規則（RED の要求・`moved` の札の免除・`tests-removed-only`・宣言 file と歯の外の file の同梱）がそのまま当たり、overlay の書き先は新 path。新 path の親（crate・mod 宣言）が base に無い周は既存の `not-flippable` の判定がそのまま当たる。
  3. docs-only の面の判定（便が動かした全 path）は旧 path と新 path の両方を数える。
  4. 対が rename かどうかは **base 側の path と HEAD 側の path が異なること（name-status の R の行）だけ**で決め、本文の同一で決めない。同じ path で本文が同一の対（mode だけが動いた M の行）は従来どおり test-diff 無しの側に落ち、便に test の差が無ければ `no-test-diff` で落ちる（便 145423Z の Gated FAIL・orchestrator が runner の木に mode だけの変更を足して rc 0 を再現・verified）。
  5. 実装の置き方（s2-07l.555 の便 152342Z）: base 側の path は列挙の列（旧 path と新 path の組）が持ち、対の型には欄を足さない（対の型の構築点が行 av の write-set の外の歯の file に在るため）。base の本文を旧 path で読むのは列挙から対を組む段で、区間の切り分けは従来どおり新 path で行う。R の行のうち base と HEAD の本文が 1 字も違わない対の本数を判定行の後置 renamed=N に載せ、flip 0 の便はこの本数が 1 以上なら removed-only / retroactive / moved と同じく rc 0 で通す（本文が動いた R の行は数えない＝rename に src の差を混ぜた便を no-test-diff の免除に使わせない）。列挙は -z で path の quote を外し、-M を明示して diff.renames の設定に依らない。
- verify の形: xtask は bin だけの package で lib target を持たない（`--lib` は rc 101）ので、verify 行は他の xtask の行と同じ `-p xtask --no-tests=fail <接頭辞>` の形。
- 大きさ（受付の cap の実測・main 974dc47）: `crates/xtask/src/flipcheck.rs` の上限の余地は 258 行なので size は S。flipcheck.rs 側の差分は renamed=N の数え口と load の呼び手だけに留め（base 側の path は形 5 のとおり列挙の列が持つ）、列挙と対の組み立ては `crates/xtask/src/flipcheck/git.rs`、歯は `crates/xtask/src/flipcheck_tests.rs` に置く。
- 触らない: 札の形と上限（rules 行 `flip.marks_per_pr`）・`not-flippable` / `tests-removed-only` / 同梱 / `moved` / `retroactive` の判定そのもの・判定行の書式・nextest を撃つ群。
- 却下: (i) 行 b の write-set に flipcheck を足して純移動の便の中で直す（純移動に判定の変更を混ぜる・§5 が対象外と置いた面）。(ii) 上限を上げる（札の門が空洞化・値は裁定 id 付き）。(iii) 移す file から過去の札を消す（免除の記録を失う・§7 の持ち越しに反する）。
- 歯（`crates/xtask/src/flipcheck_tests.rs` の既存の族 `flip_check_` に接頭辞 `flip_check_rename_`・tmp の git repo に base と HEAD を commit して撃つ既存の形）: (a) rename だけの commit（同一本文）に撃つと対の base が旧 path の本文で、flip 0・この便の札 0・rc 0 (b) rename + test 区間 1 行の差は flip に数え、overlay の書き先が新 path (c) 旧 path の test 区間に札 2 本を持つ file を rename すると 2 本とも持ち越しで、新しく置いた札 1 本だけがこの便の札 (d) A / M / D だけの便の対と判定が変わらない（既存の `flip_check_` の歯が全部緑のまま）。 (e) 同じ path で本文が同一の M の行（mode だけの変更）を 1 本だけ持つ便は rename に数えず、判定が `no-test-diff` の FAIL のまま（rename の対を本文の同一で決めていないことを撃つ）。 (f) docs-only の面の中の file を面の外へ rename しただけの便（`.rs` の差なし・例: docs/ の .md を面の外の dir へ）は docs-only にならず `no-test-diff` で落ち、面の外から面の中へ rename しただけの便も同じ（旧 path と新 path の両方を面の判定に数えることを撃つ・名は接頭辞 `flip_check_rename_` の下に置き verify の filter が当たる）。

## 54. 宣言の任意 key で「入口の flip は測らない」と名乗った Rust の消費側を、受付と契約表の検査が通す（契約表の行 aw・`s2-07l.554`・[ADR-0054](../../design-intent/decisions/ADR-0054-entrance-flip-may-be-declared-unmeasured-in-the-vessel-declaration.html)）

- 出所: 消費側 1 号の席の報告 2026-09-22（逐語は台帳 `s2-07l.554` の notes）。§7 の約束 4（`NoEntranceRed`・`s2-07l.170`）は common-verify に先頭語 `cargo` の行を持つ宣言に入口の flip の行を要求するが、xtask を持たない Rust の消費側はその行を書けず、契約表の検査が rc 2・preflight と便の起動も通らない。消費側は古い写しの器に固定して運んでいる。裁定 = user 2026-09-22（3 案のうち名乗りの欄を先に・ADR-0054 CTX4）。
- 現物（main 200b6d5・verified）: `crates/scribe2/src/pipe/declaration.rs` の `kind_gap` は `VerifyKind` の列に `EntranceFlip` が無く `Cargo` が在れば `KindGap::NoEntranceRed`。宣言の key の集合は `schema` / `allowed-commands` / `common-verify` に任意の `detection-verify` / `requirements` / `remote` / `ci-cmd` と path の種別 3 本（`crates/scribe2/src/pipe/declaration/path_kinds.rs`・ADR-0047）で、未知の key は宣言の読みの誤りとして断る。契約表の検査の判定行は `crates/scribe2/src/pipe/table/check.rs` の 1 行（`docs= rows= untracked= findings=`）。
- 形（ADR-0054 の決定・番号は done と 1:1）:
  1. 宣言の任意 key `entrance-flip` を 1 本足す。値は閉じた 1 語 `unmeasured` だけ。key を持ち、`cargo` の行を持ち、入口の flip の行を持たない宣言は `kind_gap` が空＝受付と契約表の検査を通る。
  2. key の無い宣言は現行のとおり（`cargo` の行が在れば `NoEntranceRed`）。本 repo の宣言は key を持たない。
  3. 値が `unmeasured` 以外・空・list・key の重複は、宣言の読みの誤り（既存の `Unfit` と同じ極性・typed な理由 1 つ）として断る。
  4. key と入口の flip の行を同時に持つ宣言は矛盾（`KindGap` に 1 値足す・理由の名は variant の名）として断る。
  5. 契約表の検査の判定行は、key を持つ宣言の周だけ末尾に `entrance=unmeasured` の欄を持つ。key の無い周の判定行は 1 字も変わらない（既存の外形の pin が動かない）。
- 触らない: `ENTRANCE_FLIP_WORDS` の字面・`VerifyKind` の 3 値・約束 4 の判定そのもの・rules 行・gate の検証（名乗りは受付の入口の要求だけを外す・FR8 は不変）・schema の値（1 のまま）。
- 却下（ADR-0054 OPT2〜OPT5）: 消費側の自前 command で満たす（器が測れない）／flip の検査を器に内蔵する（規模 L・消費側が求める形は「契約の verify 行を base で撃つ」で別設計＝後続の memo）／rules 行で消費側を免除する（器の行に repo の事実を積む）／何もしない（世代のずれが続く）。
- 歯（in-file は `crates/scribe2/src/pipe/declaration.rs` の既存の族 `declaration_` に接頭辞 `declaration_entrance_`・e2e は `crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` の既存の族 `contract_check_` に接頭辞 `contract_check_entrance_`・tmp の repo に宣言を書いて撃つ既存の型）: (a) key 有り + `cargo` 2 行 + flip 行無し → `kind_gap` が `None`・宣言が通る (b) 同じ宣言から key を外すと `NoEntranceRed`（形 2） (c) 値が `unmeasured` 以外・空・list の 3 形は typed に断られ、理由が key の名を持つ（形 3） (d) key + flip の行の同居は矛盾の 1 値で断られる（形 4） (e) e2e: key を持つ宣言の repo の `contracts check` の判定行が `entrance=unmeasured` で終わり、key を外した同じ repo の判定行は既存の 4 欄のまま（形 5・両方の判定行を同じ歯で並べて pin する）。

## 55. flip-check が「mod 行 + pin の数値 1 か所」だけ動いた宣言 file を本体の木へ同梱し、同梱されなかった新規 module を green-on-base と呼ばない（契約表の行 ax・`s2-07l.562`）

- 何が起きているか（orchestrator の実測 2026-09-23・便 `s2-07l.317` の 033854Z の Gated FAIL を worktree で flip-check を撃ち直して再現・verified）: e2e に新設 module を足す便は、宣言 file `crates/scribe2-boundary/tests/e2e/main.rs` に **2 つの差**を持つ——`mod` 宣言 1 行と、同じ file の歯 `e2e_fixture_clock_dated_reset_lines_are_pinned` が pin する tracked な e2e の `.rs` の本数（母集団 29）の数値 1 か所である。flip-check はこの宣言 file を本体と同梱せず、本体を単独で base に写すので、module が compile されず新しい歯が 1 本も走らないまま base の歯が全部緑になり、判定は `green-on-base`（TDD 不履行の語）になる。母集団はこの日の新設 module の便 1 本で 2 便連続（025617Z は pin を上げずに workspace の歯が赤・033854Z は pin を上げて `green-on-base`）。同じ周に出る `stale-marker` の行は判定に効いていない。現状の逃がしは歯を既存の e2e file に置く運用で（`.317` はこの形で通した）、新設 module を置く場所が e2e のどこにも無い。
- 現物（main b82be74・`crates/xtask/src/flipcheck.rs` を読んで確認）:
  - `plan_of` は flip した file を 3 つに割る。宣言 file（`declaration_only`＝動いた行が全部 `mod` 行）と歯の外の file（`outside_teeth`＝動いた行が 1 本も歯の中に無い・§37）は同梱の側、残りは単独で撃つ本体。pin の数値の行は歯の中に在るので宣言 file の形にも歯の外の形にも当たらず、宣言 file は**本体**に落ちる。
  - `judge_each` は本体を 1 本ずつ撃つ。宣言 file を撃つ turn は新しい `mod` 行の本体が木に無く `E0583` の compile error＝RED で通り、新設 module の turn は base の宣言 file のままで module が compile 対象に入らず rc 0＝`judge_one` が `green-on-base file=<本体>` を返す。この語は「新しい歯が base で緑だった」と「新しい歯がそもそも compile されなかった」を区別しない。
  - **同梱するだけでは足りない**: base の木は `crates/xtask/src/flipcheck/git.rs` の `index_base` が index を base の tracked 集合で作り、overlay は working tree にだけ書く。pin の歯は `git ls-files` で母集団を読むので、どの turn でも base の本数（29）を見る。HEAD の数値（30）のまま同梱すると pin の歯が**毎 turn 赤**になり、本体の歯が base で緑でも turn が RED で通る——`bundle_decls` の doc が記録している `s2-07l.41` の fail-open（兄弟の `E0583` が緑の本体を隠した）と同じ型である。
  - 同じ型の穴が宣言だけの便にも在る（推論・未実測）: `crates/scribe2-boundary/tests/e2e/pipe.rs` の歯 `pipe_hermetic_sites_stay_one` は tracked な file 数（index）を同じ file の列 0 の `mod` 行の数 + 1（working tree）と比べる。`pipe/` 配下に module を足す便の宣言 file は `mod` 1 行だけの差で同梱されるが、同梱した turn では `mod` 行が 1 本増えて tracked は増えないので、この歯が毎 turn 赤になり本体の緑を隠しうる。
- 形（番号は done と歯に 1:1）:
  1. **宣言と pin の file を認める**: test 区間の動いた行を側ごとに取り（追加行と削除行・`changed_lines` と同じ多重集合の差を片側ずつ・空白だけの行は数えない）、次の 4 つが全部成り立つ file を「宣言と pin の file」と呼ぶ。(i) path が `crates/<c>/tests/` 配下 (ii) `mod` 行（`mod_name` が名を返す行）が 1 本以上動いた (iii) `mod` 行でない動いた行は削除 1 本と追加 1 本の対だけで、trim した 2 行が「連続する ASCII の数字の並び」の置き換え**ちょうど 1 か所**を除いて 1 字も違わない (iv) 削除行は base 側の、追加行は HEAD 側の歯の中に在る（`teeth_lines` の読み）。追加行の字面が HEAD の test 区間に 2 度以上現れる file は形に当たらない。どれか 1 つでも外れる file は従来の路（宣言 file・歯の外・本体）のまま。
  2. **同梱する**: `plan_of` は宣言 file の弁別の次・歯の外の弁別の前にこの形を見て、宣言 file と同じ側（`decls` の列）へ置く。置き方は宣言 file と同じ `bundle_decls`（turn ごとに本体が木に在る `mod` 行だけへ絞る）で、pin の行は HEAD の字面のまま書く（書き換えない）。stderr に `flip-check: decl-with-pin <rel> tooth=<歯の名>` を 1 行出し、判定行は `decl=N` にこの file を数えたうえで `pin=N`（この形で同梱した本数）を `decl=` の直後に後置する（0 の内訳は出さない）。
  3. **同梱した宣言 file の歯の赤を turn の RED に数えない**: 宣言 file を 1 本以上同梱した turn は、rc が test の失敗（nextest の rc 100）のとき `failed_tests` で落ちた歯を名指し、名の最後の `::` の後が同梱した宣言 file（形 1 の file と `mod` 行だけの宣言 file の両方）の HEAD 側の歯の名に一致する歯を除く。残りが 1 本以上なら RED、0 本なら `green-on-base file=<本体>`、名指しが 0 本なら `infra-error` の `bundled-unnamed rc=<rc>`（RED とも緑とも数えない）。rc 101（compile error）・rc 0・rc 4・signal は従来どおり `judge_one` の語。除くのは判定を厳しくする側なので、名の照合は最後の区切りの一致で足り、同名の歯を余分に除いても緩まない。この弁別は「落ちた歯の名の列と除く名の列から RED / 緑 / 名指せないの 3 値を返す純関数」1 つに置き、`judge_each` はその値を判定行へ写すだけにする。
  4. **同梱されなかった新規 module を `green-on-base` と呼ばない**: 本体を撃つ turn の前に、本体が base に無い file で `crates/<c>/tests/` 配下かつ target の根（`tests/<f>.rs` と `tests/<d>/main.rs`）でないとき、その turn の木で宣言の在処になりうる file（本体と同じ dir の `main.rs` と `mod.rs`、dir と同名の `<dir>.rs`・`present_mods_only` の 3 形の逆。**本体が `tests/<d>/mod.rs` で dir が `crates/<c>/tests` そのものの周は、その根の `tests/<f>.rs` 全部も在処に数える**＝各 target の根が `mod <d>;` を持ちうる〔便 160025Z の審査 contract-fit の根・在処を 3 形に閉じると `tests/<f>.rs` が宣言した新規 `tests/<d>/mod.rs` を厳しい側へ誤判定する〕）に本体の module 名を返す `mod` 行が 1 本も無ければ、nextest を撃たずに木を戻して `not-flippable files=<rel>` で落とす（stderr に `flip-check: undeclared-in-turn <rel>` と、既存の `no_flip_verdict` と同じ逃がし方の 1 行）。語は既存の `not-flippable`（「新規 module は base に mod 宣言ごと無く compile されない」）を使い、FAIL の語を増やさない。これで既存の歯 2 本の期待が `green-on-base` から `not-flippable` に替わる: `flip_check_still_judges_declaration_file_that_also_changes_tests`（宣言 file が自前の歯も動かした便）と `flip_check_treats_pub_crate_mod_line_as_declaration` の後半（空白の無い可視性の宣言）。どちらも「同梱されなかった」便で、本体の file を名指す assert と `decl=` の不在の assert は不変。
  5. **本体が 1 本も無い便は従来どおり**: 形 1 の file しか flip していない便は `plan_of` の空 bodies の分岐のまま単独で撃つ（`E0583` の本当の RED・同梱で消さない）。
- 触らない: `declaration_only` と `outside_teeth` の述語・`present_mods_only` の 3 形と base の宣言の持ち越し・単独 overlay の骨（本体 1 本ずつ・「どれか 1 本が赤い」へ緩めない）・`removed_only` の部分列・`retroactive` / `moved` の札と `stale-marker` の行・base 段の撃ち直しと `base-not-green` の弁別子・`index_base` の index の作り方・`crates/<c>/src/` 配下の名で test の file（`#[path]` で宣言される新規 file は形 4 の外で、従来どおり `green-on-base` で落ちる）・判定行の先頭と FAIL の語の集合・極性一覧の行・`crates/scribe2-boundary` の歯（pin の値も置き場も動かさない）。
- 却下:
  - (b) pin の歯を宣言を持たない別 file へ純移動し、宣言 file の差を `mod` 1 行に保つ——移動そのものが新設 module なので同じ穴に当たり、先に (a) か手で 1 回通す必要がある。しかも形 3 の穴（同梱した turn で宣言 file の歯が赤くなる型）は残る。
  - (c) 新設 module を禁じて既存 file に足す運用——散文の規則で器の穴を覆う（憲法 N2）。`.317` の逃がしとしては使ったが恒久策ではない。
  - pin の行を base の字面へ書き戻して同梱する——`git ls-files` で数える pin には効くが、working tree を数える pin では本体の file が木に在るぶん逆に赤くなり、緑の本体を隠す。形 3 の名の除外は pin の数え方に依らない。
  - 新しい FAIL の語（`undeclared-module` 等）を立てる——意味は既存の `not-flippable` と同じで、逃がし方の案内も同じ。語を増やすと操作役の読む語彙だけが増える。
  - 数値の差を 2 か所以上まで認める——`e2e_fixture_clock_dated_reset_lines_are_pinned` は本数と当たった行の数の組を pin しており、新設 module が日付の行も足すと 2 か所以上が動く。その便は形 1 に当たらず形 4 で `not-flippable` に落ちる（fail-closed）。広げると歯の中の値の書き換えを同梱で見逃す扉が開く。
  - 名の照合を binary id と module path の完全一致にする——除くのは厳しくする側なので、照合を狭くしても守るものが無く、宣言 file の path から module path を導く手間だけが増える。
- 歯（接頭辞 `flipcheck_declaration_pin_`・置き場は既存の `crates/xtask/src/flipcheck_declaration_tests.rs`・既存の fixture〔`base_commit_with_e2e` / `red_body` / `green_body` / `head_commit` / `judge` / `assert_verdict`〕の型・新設 test file は作らない）: fixture の宣言 file は `mod seed;` に加えて `git ls-files` で `tests/e2e` 配下の `.rs` を数えて数値と比べる歯 1 本を持ち、本物の pin と同じ母集団の読み（index）を再現する。
  - (a) 形 1〜3: `mod newmod;` の追加と pin の数値の +1 と、base で赤い本体の新設を持つ便が `RED-on-base ok` を出し、判定行に `decl=1` と `pin=1` が載る（base は宣言 file が本体に落ちて本体の turn が `green-on-base` → RED）。
  - (b) 形 3: 同じ便で本体だけを base で緑にすると `green-on-base file=<本体>` で落ちる（pin の歯の赤が本体の緑を隠さない）。
  - (c) 形 1 の狭さ: 数値の差が 2 か所の便と、pin の行を歯の外（helper の中）に置いた便は形に当たらず、判定行に `pin=` が載らない。
  - (d) 形 4: 宣言 file が自前の歯の本文も動かした便の新設 module の turn は `not-flippable files=<本体>` で落ち、判定行が `green-on-base` を含まない。
  - (e) 形 3 の純関数: 落ちた歯が除く名だけ → 緑、除く名の外に 1 本 → RED、名指し 0 本 → 名指せない、の 3 通り（歯からは `super::super::` 始まりの path で引き、`crates/xtask/src/flipcheck_tests.rs` の `use` は触らない）。
  - (f) 形 4 の在処: target の根 `tests/<f>.rs`（`main.rs` でない名）の `mod <d>;` で宣言された新規 `tests/<d>/mod.rs` の turn は `not-flippable` に落ちず撃たれ、同じ便から `tests/<f>.rs` の `mod` 行を消すと `not-flippable files=<本体>` で落ちる（在処を 3 形に閉じる変異を両側から捕まえる）。
  - 既存の `flip_check_` の宣言の歯と `flipcheck_declaration_nested_` の歯は、形 4 で期待を替える 2 本を除いて 1 字も変えずに緑。
- 大きさ: `crates/xtask/src/flipcheck.rs` は幅 120 で正規化した行数が 1254（上限 R-C4-2 = 1500・余地 246）。size M の既定の見積（300）は余地を超えるので、行 ax は `growth` でこの file の見積を 200 に置く（形 1〜4 の述語と純関数と doc）。歯の file は 478 行で余地が足りる。

## 56. 名乗った消費側の契約の nextest の検証行を受付で base の木でも撃ち、base で緑の本数を受付の 1 行と記録に残す — deny の名乗りは受付で断る（契約表の行 ay・`s2-07l.557`・[ADR-0059](../../design-intent/decisions/ADR-0059-consumer-verify-lines-are-fired-on-base-at-intake.html)）

- 何が起きているか（消費側 1 号の席の報告 2026-09-22 と照会の返答 2026-09-23・要旨・逐語は台帳 `s2-07l.557` の notes）: 台帳の規則は「検証は base で RED になる歯」だが、器の gate は便の木で検証行が緑になることしか見ず、base でも緑のままの歯を持つ便が gate を通った。§54 の名乗り `unmeasured` は入口の RED を測らないまま便を進める（ADR-0054 CSQ-N1）。消費側は xtask を持たないので、求める形は「契約の検証行そのものを base の木で撃つ」である。裁定 = AI（設計の選択・ADR-0059）。
- 現物（本行の base・main eafbfbc・verified）:
  - **受付の順**: `crates/scribe2/src/pipe/cli/intake.rs` の `intake_run` は入口の lock（ADR-0019 §2.1）を取ってから上限の読み・材料の読み・行の生成と表の検査・`judge`・`create` を撃ち、`judge` の先頭の `freeze` は HEAD の commit の宣言を読み直す。断る周は run dir も event も作らない。`judge` の呼び手は受付・preflight（`crates/scribe2/src/pipe/cli/preflight.rs`）・列の候補（`crates/scribe2/src/pipe/dispatch/candidates.rs`・置き場を渡さない）の 3 つで、判定の材料の構造体の構築点もこの 3 つ（review の同名の別型は数えない）。preflight の頭の doc は「run dir・写し・event は一切書かず」と書く。
  - **gate の撃ち方**: 契約の検証行は 1 行ごとに器の健康の遮断器（gate-cost.md §32）を通ってから `crates/scribe2/src/pipe/gate/verify.rs` の `run_line_captured` で撃たれる。行は sh -c で撃たれ、子が signal で死ねば rc 128+N・無い command は 127・起動できない周は -1 で、箱の oom_kill の数は別に返る。契約の検証行は admission（ADR-0021）の札を取らず host の memory から予備を引いた箱で撃たれ、時間の上限は無い。`run_line_captured` は `crates/scribe2/src/pipe/gate.rs` の再輸出に無く、gate の外から呼べない。
  - **nextest の行の読み手**: `crates/scribe2/src/pipe/closure/derive.rs` の `nextest_read` の 1 本で、`teeth_words` もこれを通る。`-p` / `--package` / `--test` は語の完全一致で読み、`=` で結んだ 1 語（`--test=t` 等）は読み飛ばす。`-p` の無い行の crate は器自身の crate 名に倒す（本 repo の閉包のための既定値）。本行はこの読み手を変えない（閉包の読みは不変）。
  - **名乗り**: `crates/scribe2/src/pipe/declaration.rs` の値の型 `EntranceFlip` は 1 語 `unmeasured` だけで、同居の矛盾の理由の 1 行はその語の字面を持つ。この file は幅 120 で正規化した行数が 1497（R-C4-2 = 1500・余地 3）。
- 形（ADR-0059 の決定・番号は done と歯に 1:1）:
  1. **名乗りの語を 3 語に**: `entrance-flip` の値を閉じた 3 語 `unmeasured` / `detect` / `deny` にする。3 語とも `NoEntranceRed` で断らず、入口の flip の行との同居は既存の `UnmeasuredWithEntranceFlip` の 1 値で断る（variant の名は変えない・理由の 1 行から語の字面を外す）。他の語・空・list は従来どおり宣言の読みの誤り。値の型とその読み手は本行の write-set の `+` の 2 本目の file（declaration の子 module・ADR-0047 の path の種別と同じ置き方）へ移して 3 語にし、親は型を再輸出して縮む（余地 3 のため・親の in-file の歯は 1 字も動かさない）。契約表の検査の判定行の `entrance=` は語をそのまま写す（`crates/scribe2/src/pipe/table/check.rs` の中身は変えない）。
  2. **読みを入口の lock の前へ**: 受付は名乗りに依らず材料の読みの前に対象 repo の HEAD の sha を読み（出力は変えない）、上限の読み・材料の読み・行の生成と表の検査・`freeze` を lock の前に 1 周に 1 回撃つ。lock の中の `judge` は `freeze` の結果（有効値と名乗りの語）を判定の材料の構造体の新しい欄 1 つで借りて宣言を読み直さない（欄が空の周＝列の候補は従来どおり自分で撃つ）。行は従来どおり作業木から 2 回読み直す。preflight も同じ順で撃つ。
  3. **nextest の行だけを base の木で撃つ**: `detect` か `deny` を名乗った宣言の周だけ、`freeze` を通った契約の検証行（共通の検証は撃たない）を分ける。(i) `nextest_read` が読まない行（先頭が cargo nextest run でない・filter 語が無い・timeout や env の前置き）と、1 語 `--no-tests=fail` を持たない nextest の行は撃たずに「測れない」。nextest の行かの選び方は既存の `nextest_read` の 1 本に従い、読み手を広げも増やしもしない（C2）。(ii) 「不在」の事前判定は語の全体を読み切れる行だけに限る。読み切れる行は 4 条件を全部満たす行である: 行の全体が ASCII の英数字と `_-.:=/` と半角 space（U+0020）だけ／`-p` と `--test` が半角 space で区切った旗と値で丁度 1 つずつ／`-p` と `--test` の値は `-` で始まらない／他の旗は `--no-tests=fail` と `--` の後ろの libtest の引数だけ。`=` で結んだ `-p` と `--test`（`--test=t` 等）は旗と値に割れないので読み切れない。全角空白（U+3000）・NBSP（U+00A0）・改頁（U+000C）・CR（U+000D）・tab を含む行も読み切れない（`nextest_read` は Unicode の空白で語を切るが sh はこれらで語を切らない＝旗を 1 語に埋めた行が「不在」に倒れる穴を塞ぐ）。値が `-` で始まる行（`--test --` 等）は撃つと cargo が値の欠けで rc 2 を返し「測れない」に写る。読み切れる行のうち、`-p` の名が sha の木の根の Cargo.toml の `[workspace]` の members の glob でない項の dir の `[package]` の name と逐語で一致し、その dir に `--test` の target（tests の直下の名の .rs・名の dir の main.rs・Cargo.toml の test の表の name）が無いと決まる行だけを撃たずに「不在」とする。target の有無は用意した sha の木を file system で読み（材料の tracked も git の object も借りない・tests の dir の symlink は辿る）、Cargo.toml は TOML として読んで `[[test]]` の表と inline の表の配列（`test = [{ name = "tt", path = … }]`）を同じ target の宣言と扱う（依存は足さない・器の読み手で読めない形は読めない Cargo.toml として撃つ側）。読み切れる行は空白で割った語の列で閉じるので、`-p` と `--test` の値と Cargo.toml の読みは `+` の 1 本目の file が持つ。この事前判定が守るのは「base で緑（rc 0）の行を不在にしない」ことだけである。(iii) 他の行（読み切れない行・`-p` の無い行・root の package・`[workspace]` の無い単一 crate・glob の members・読めない Cargo.toml）は全部、既定の crate を当てずに撃つ。限界は 2 つ。(1) 読み切れる条件は行の字面だけを見て、受付は base の木の cargo の差し替え（`.cargo/config.toml` の `[alias]` の nextest・`rust-toolchain.toml` や拡張子の無い `rust-toolchain` の toolchain の path 等）を読まないので、それで cargo か cargo nextest が差し替えられる周は測れない側に倒れない可能性が残る。alias も toolchain の path も偽の cargo で rc 0 を実測した（ADR-0059 CSQ-N6）。(2) 読み切れる行でも、`--` の後ろの libtest の引数や `--no-tests=fail` の重ねで撃てば引数の誤り（rc 96 / rc 2）になる行は、撃たずに「不在」と決まりうる。その行は rc 0 にはならず、便の木の gate も同じ誤りで落とす。撃つ木は sha を置き場の直下（`pipe/` の外・入口の lock file と同じ階層）の一時 dir に detach した worktree で、撃ち終えたら worktree の登録ごと片付ける。撃ち方は gate と同じく 1 行ごとに遮断器（gate の規則の読み手から材料を取る）を通し、`run_line_captured`（gate.rs の再輸出に 1 行足す・撃つ実装を 2 本にしない）と host の memory の箱で撃つ（admission の札は取らない・時間の上限は持たない）。
  4. **rc の 4 値の写し**: 撃った行は rc 0 =「base で緑」・4 =「不在」・100 =「落ちた」・他（101・signal・-1 を含む）=「測れない」。oom_kill が 1 以上の行と、遮断器が閉じた行とそれ以後の行は「測れない」。木を用意できない周と置き場の解けない preflight の周は撃たずに、実走の後に読み直した HEAD の sha が形 2 の sha と違う周は撃った結果を捨てて、全行を「測れない」とする。分け方と写しは本行の write-set の `+` の 1 本目の file の純関数に置く。
  5. **受付の 1 行と記録**: 受付の 1 行と preflight の行（1 行 1 事実）は `entrance=green-on-base:<本数>/<行数>`（行数は契約の検証行の全本数）を持ち、測れない行があれば同じ行に `unmeasurable:<本数>` を添える。通った受付は run dir に記録 file を 1 本書き（名は `+` の 1 本目の file の const）、測った sha と行ごとの 4 値・rc・秒（撃たなかった行は `-`）を持つ。`detect` は受付の結果を変えない。
  6. **deny の断り**: `deny` の周に base で緑の行か測れない行が 1 本以上あれば、`judge` の判定関数 1 本（余地の判定の後・置き場の要る判定の前）が `Refuse` の新しい variant 1 つ（宣言順の末尾・名 `entrance-not-red`・本数と行ごとの 4 値を持つ・rc 1）で断り、便・run dir・event を作らない。理由は受付の断りの行と preflight の `refuse=` に出る。結果を持たない `judge`（列の候補）は立てない。`Refuse` を組むのは intake.rs と refuse.rs だけで、`+` の 2 本は `Refuse` も `EntranceFlip` の variant も名指さない（閉じた型の閉包を他の行へ広げない）。既存の `KindGap` の `NoEntranceRed`（宣言の行の分類）とは別の型・別の段である。
  7. **不変**: `unmeasured` の宣言と key の無い宣言は木を用意せず、何も撃たず、受付の結果と 1 行と記録を 1 字も変えない（読みを lock の前へ移したことによる断りの先後の入れ替わりは除く）。本 repo の宣言は key を持たない。列の候補の判定は base を撃たない（新しい欄を空で渡す）。
- 触らない: `crates/scribe2/src/pipe/closure/derive.rs` と `crates/scribe2/src/pipe/closure.rs`（nextest の行の読み手と閉包の読み）・event の種類と段の種類・rules 行・gate の検証（便の木の実走と判定）・`crates/xtask/src/flipcheck.rs`・`crates/scribe2/src/pipe/spawn.rs`（便の base の決め方）・`crates/scribe2/src/pipe/confine.rs`・`crates/scribe2/src/pipe/health.rs`・本 repo の宣言・宣言の schema の値・ADR-0054 の `unmeasured` の扱い。
- 却下（ADR-0059 OPT2〜OPT5）: gate の段で base を撃つ（base の sha が同じなら答えは受付と同じで、実装役の費用を払ってから断る）／xtask の flip を器に内蔵して配る（規模と前提の設計が要り、消費側が求める形でない）／lens に読ませる（実走で決まる計算できる判断で、再現しない）／何もしない（base でも緑の歯が通り続ける）。契約の単位（全行が base で緑のときだけ記す）でなく行の単位を採るのは、新しい歯の行 1 本が base で緑の行を隠すため。glob の members を展開して解く形は、読み手に Cargo の glob と exclude の規則を足すので採らない。
- 実装の置き方（`s2-07l.591` の便）: lock の前の読み（freeze の結果と base の木の実走）は行 ay の write-set の `+` の 1 本目の file の構造体 1 つにまとめ、判定の材料の構造体の新しい欄はその借り 1 つ（列の候補は空）。freeze は有効値と名乗りの語の組を返す（宣言の読み手は declaration の子 module の 1 本）。記録 file の中身は 1 行目が sha= で、以後は検証行ごとに line=・value=（4 値の語）・rc=・secs=（撃たなかった行はどちらも -）・cmd= の 1 行。断りの理由の 1 行は名 entrance-not-red で始まり、本数と行ごとの 4 値の語を , で並べる。一時の木の名は置き場の直下の base-run- に process id と時刻を続けた dir。受付の直後の審査の段は要件面の path を HEAD の宣言から読み直すので、実走の間に宣言を壊す commit を積んだ周は受付が run dir を作った後に審査の段が rc 2 で止まりうる（受付の判定の外・形 2 の歯は run dir と受付の 1 行で測る）。Cargo.toml の読み手は表の見出し・key と値の行・1 行の文字列（複数行の文字列は読めない）・改行を跨ぐ配列・1 行の inline の表だけを受け、他の形は読めない Cargo.toml として撃つ側に倒す。
- **`pipe/cli/intake.rs` の余地（実測・main c96ab41）**: R-C4-2（file 1 本 1500 行）の余地は 72 行なので、本行が intake.rs に足すのは base の木で撃つ口の呼び出しと受付の 1 行と断りの分岐だけ（見込み 60 行以内＝growth の値）。読み・撃つ本体・rc の 4 値への写しは `+` の 1 本目の file に置く。
- 歯（接頭辞はどれも本行の base の `fn` 名に 0 件）: lib は `+` の 1 本目の file の中に `pipe_base_run_`。e2e は既存の `crates/scribe2-boundary/tests/e2e/pipe/intake.rs` に `pipe_intake_base_run_`（preflight の歯も同じ接頭辞）・既存の `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs` に `pipe_dispatch_base_run_`・既存の `crates/scribe2-boundary/tests/e2e/pipe/contracts.rs` に `contract_check_entrance_word_`。e2e は偽の cargo（filter 語ごとに決めた rc を返し、cwd と引数を記録 file に 1 行ずつ書く shim）を道具箱の PATH の前に積んで既存の `run_pipe_with_path` で撃つ。新しい e2e file は作らない（置き場の pin の歯を動かさない）。
  - (a) 形 1: `detect` と `deny` の宣言の repo で contracts check が rc 0 で、判定行が `entrance=detect` / `entrance=deny` の欄を持つ。同じ宣言に入口の flip の行を足すと `UnmeasuredWithEntranceFlip` で断られ、理由の 1 行が `unmeasured` の字面を持たない。既存の `contract_check_entrance_` の歯は 1 字も変えずに緑。
  - (b) 形 3 (i)・形 4 の純関数: rc 0 / 4 / 100 / 101 / 137 / -1 と oom_kill 1 の 7 形の写し、遮断器が閉じた行とそれ以後の行、nextest の行でない 5 形（cargo test・clippy・前置きの行・filter 語の無い行・`--no-tests=fail` の無い行）が撃つ列に入らない。
  - (c) 形 3 (ii)・(iii) の仕分けの純関数（fixture は 1 行 1 本・sha の木の fixture は members の項 a と b の 2 crate で、b の dir に target tt が無い）: 読み切れる対照の行 `cargo nextest run -p b --test tt --no-tests=fail old_tooth`（実測で rc 101 の真の不在）だけが「不在」になる。撃つ側に倒れるのは 16 行。7 周目の 6 形（`-p` が 2 つ・`--workspace` と `-p`・`--manifest-path`・`--test` の値に glob の字・引用符つきの `--test` の値・`--test` が 2 つ）、8 周目の 4 形（`#` のコメントの後ろの `-p b --test tt`・`\` の空白で連結した語・引用符の中の `-p b --test tt`・backtick の中の `-p b --test tt`）、`=` で結んだ `-p=b --test=tt` の行、9 周目の 5 形（全角空白 U+3000・NBSP U+00A0・改頁 U+000C・CR U+000D のどれか 1 種で `x`・`-p`・`b`・`--test`・`tt` を区切って 1 語に埋めた 4 行と、`--test` の値が `--` の 1 行）である。7 周目の 6 形と `=` の形は対照の行に形を 1 つ足した字面で組む（例: `cargo nextest run -p a -p b --test tt --no-tests=fail old_tooth`）。8 周目の 4 形は旗を `-p b --test tt` の 1 か所にだけ持ち、それを `#` の後ろ・`\` の後ろ・引用符の中・backtick の中に置く（sh で撃つと旗として効かず rc 0 になる形）。9 周目の 4 字の行は `--no-tests=fail` の後ろに、その字 1 種で 5 語を結んだ 1 語と filter 語を置く（sh で撃つと rc 0）。§ と fixture の字面は字を escape で書き、生の不可視文字を doc に埋め込まない。加えて target が「在る」と読まれて撃つ側に倒れる 5 形（名の .rs・名の dir の main.rs・path 付きの `[[test]]` の name・Cargo.toml の inline の表の配列 `test = [{ name = "tt", path = … }]` の name・b の tests の dir が a の tests への symlink でその下に名の .rs が在る木）と、`-p` だけ・`--test` だけの行、root の package・`[workspace]` の無い単一 crate・glob の members も撃つ側。
  - (d) 形 3・5・6: `deny` の repo で偽の cargo が 1 行目に 0・2 行目に 4 を返すと受付が rc 1 で `entrance-not-red` を名指し、run dir も event も 0。偽の cargo の cwd は対象 repo でなく、受付の後に一時の木も worktree の登録も残らない。2 行とも 4 なら受付を通り、1 行が `entrance=green-on-base:0/2` を持つ。preflight は同じ 1 行目 0 の repo で `entrance=` の行と `refuse=entrance-not-red:` の行を並べて rc 1。禁じる語列に当たる nextest の行を持つ `deny` の契約は宣言の断りで落ち、偽の cargo の記録が空。
  - (e) 形 5・7: `detect` の repo で同じ偽の cargo なら受付を通り、1 行が `entrance=green-on-base:1/2` を持ち、run dir の記録の sha が HEAD で行ごとの値が 2 行在る。同じ repo の key を外した周と `unmeasured` に替えた周は 1 行に `entrance=` が無く、偽の cargo の記録が空。
  - (f) 形 2・4: `detect` の repo で、偽の cargo が撃たれた間に対象 repo へ宣言を壊す commit を積むと、受付は宣言の断りで落ちずに run dir を作り、1 行が `entrance=green-on-base:0/2 unmeasurable:2` を持つ（lock の中で宣言を読み直す変異は宣言の断りに、HEAD を読み直さない変異は本数 1 に倒れる）。
  - (g) 形 7: `deny` の repo で `pipe dispatch ls` がその契約を待たせず（理由 `-`）、偽の cargo の記録が空。
  - 既存の `refuse_names_are_pinned_in_declaration_order` は母集団 23 値と末尾の名の期待だけを替えて緑。
- 大きさ: 幅 120 で正規化した行数は intake.rs 1405（余地 95）・refuse.rs 827（余地 673）・gate.rs 821（余地 679）で、size L の既定の見積（800）を超えるので行 ay は growth で 90 / 40 / 5 を置く。declaration.rs は縮む面（`-`）で余地を求めない。`Refuse` の閉包（touches）が名指す 3 file（`crates/scribe2/src/pipe/closure/names.rs`・`crates/scribe2/src/pipe/table.rs`・`crates/scribe2/src/pipe/table/check.rs`）は write-set に載せるが中身は変えず、余地の足りない 2 本は growth 5 を置く。

## 57. 着地が anchor を揃えなかった周を印に残す — anchor の git dir に印を置き、Landed の detail と land-window が 1 語で出し、pipe の口が古い中身だけを main の先端へ戻して印を外す（契約表の行 az・memo `s2-07l.695`・裁定 user 2026-09-27T13:32Z）

- 何が起きているか（2026-09-27・経緯は非公開の隣の project の設計席の申告・code と記録は verified）: 非公開の隣の project で、設計席が anchor の checkout（main）で設計の文書を編集している間に便が 3 本着地した。land は main を CAS で進めたが、anchor は汚れていたので index と作業の木を揃えなかった。設計席の `git add` と `git commit` が index の古い中身を載せ、着地 3 本（27 file）を巻き戻した。巻き戻った main を base に次の便が起き、Questioned で止まった。揃えなかった事実は判定行の token と stderr の warning にしか出ず、`RunDone stage=Landed` の detail にも run dir にも残らない。driver の出力を読まない席は `git status` を見るまで知れない。
- 現物（main 8f6072d・verified）:
  - land は `crates/scribe2/src/pipe/land.rs` の `attempt`（:444）と `crates/scribe2/src/pipe/land/finish.rs` の `land_train`（:451）の 2 か所で `sync_anchor`（land.rs:539）を撃つ。揃えるのは `anchor_plan`（:477）が「HEAD が `refs/heads/main` を指し tracked な変更が無い」と見立てた周だけである。揃えない理由は閉じた 5 値 `AnchorSkip`（:101・not-main / dirty / collision / unreadable / sync-failed）で、結果 `AnchorSync`（:488）は判定行の token（:497）と stderr の warning（:507・not-main 以外）にだけ写る。
  - `finish`（finish.rs:303）が書く `Landed` の detail は `sha:<new> main:<実測>` に `already-landed` を後置しうる形（:319）で、anchor の token を持たない。赤 / 測れない周の `Failed` の detail は逐語の `main-red` / `main-unmeasured`（`crates/scribe2/src/pipe/land/verify.rs` の :214 / :242）で、既存の歯が逐語で pin する。synced の周の `Landed` の detail も既存の歯 5 本が逐語で pin する。
  - `pipe land-window` の判定は `crates/scribe2/src/pipe/queue.rs` の `window_now`（:306）の 1 本で、`Window`（:262・欄 2 つ・構築点は `window_now` の 1 か所・欄は private）は列・追随・未 push だけを見る。汚れた anchor でも `land-window=clear` を返す。
  - 汚れた anchor の中身（再現・git 2.43・tmp の repo・2026-09-27）: CAS の後の `git status` は着地の変更を staged の逆向き（`M a` / `A d` / `D n`）で出す。着地の path の集合 L（old → HEAD の name-status・rename を割る）のどの path も、index が old と同じ（index と old の name-only の差分に出ない）。`git commit` はこの index を載せる。L の path の index と作業の木だけを HEAD の中身へ戻す `git restore --source=<HEAD> --staged --worktree` は、L の外の局所の変更を残したまま L を揃え、消えた path は index と作業の木から消し、足された path は作る。ただし足された path に untracked の file が在ると黙って上書きする（実測）。`git read-tree -m -u <old> HEAD` は、L の path に作業の木の変更が 1 つでも在ると "not uptodate" の rc 128 で全体を断る（実測）。
  - 置き場の形の前例: ADR-0004 D-4 は、置き場以外の場所と policy の受け渡しを「worktree の私有の git dir の下の file」の形で許す（"a file under the worktree's private git directory"）。write-set guard の policy はその形で、`<git-dir>/<NAME>/write-set.txt` に置く（`crates/scribe2/src/hook/guard.rs` の `policy_path`・:44）。main の worktree では git dir と common dir が同じ `.git` で、linked worktree は自分の git dir を持つ。
  - 使い方の面: `crates/scribe2/src/help.rs` の pipe の面の subcommands の表（:251-269・17 行・land-window は :266）は、`PipeCommand` の口ごとに 1 行を持つ。pipe の面は今 49 行（見出し 7・空行 6・本文 36）で、`cli_help_text_is_ascii_and_fits_the_width`（`crates/scribe2-boundary/tests/e2e/main.rs`）の上限 60 行と幅 100 字の内にある。
- 形（番号は done と 1:1）:
  1. **印を書く**: `sync_anchor` の直後（main の実測の前）に、結果が dirty / collision / unreadable / sync-failed の周は、anchor の git dir（`--absolute-git-dir`）の `<NAME>/` の下に印の file を 1 本置く。中身は 1 行で、着地の前の main の sha（from＝anchor の index と作業の木が最後に揃っていた main）と改行 1 つだけを持つ。一時 file から rename で置く。印が既に在る周は書き換えない（index に残る古い中身は最初に揃えなかった main のまま＝from を保つ）。from と着地後の main が同じ周（既着地の old → old）は書かない。理由の語は印に持たず、形 2 の detail が持つ。synced の周は印を消し、not-main の周は触らない。印の読み書きと判定は行 az の write-set の `+` の file（land の子 module）の 1 本に置き、land.rs には `sync_anchor` の 2 か所の呼び手の後ろに呼び出しの数行だけを足す（land.rs は 1151 行）。書けない・消せない周は land の rc を変えず、stderr に 1 行を足す（黙らない）。印の書き手は境界 crate の歯からも呼べる可視性で置く（行 h の歯の fixture が同じ書き手で印を置く）。
  2. **記録に残す**: 印を書いた周（dirty / collision / unreadable / sync-failed）だけ、`Landed` の detail の末尾に判定行と同じ token（空白 1 つと `anchor=skipped:<理由>`）を足す。synced と not-main の周の detail は 1 字も変わらない。`Failed` の detail・判定行・stderr の warning の字面は変えない。
  3. **古さの判定は印と今の中身で決める**（読み手は印を書き換えない）: git dir を解く 1 本で印を探し、印が無ければ新しい（repo でなく git dir を解けない周も印が無いので新しい）。印が在れば、L（from → HEAD の着地の path）のうち、index が from と同じ path か作業の木が from と同じ path が 1 つでも在れば古い。無ければ新しい（手で揃えた周）。git の読みは 3 本（from → HEAD の name-status・index と from の差分・作業の木と from の差分・どれも name-only で rename を割る）。印が読めない周（1 行の 16 進 40 字か 64 字でない・欠け・余り）と、from や HEAD が読めない周は「読めない」で、新しいに読み替えない。読めないは古いと同じ側に倒す（fail-closed）。窓は閉じ、行 h の門は断る。字面だけが anchor=unreadable で分かれる。判定の値は閉じた 3 値（新しい / 古い〔from と古い path の本数〕/ 読めない）で、形 4 と形 5 と行 h の門が同じ 1 本を撃つ。判定の 1 本は crate の中から呼べる可視性で置く（land.rs が `pub(crate)` で再輸出する。行 h の hook が land の外から呼ぶ）。
  4. **land-window が 1 語で出す**: `Window` に anchor の判定の欄を 1 つ足し、`window_now` が `--repo` の checkout について形 3 を撃つ。古い周と読めない周は窓を閉じ（rc 1）、busy の行の unpushed の欄の後ろ（remote の欄の前）に「anchor=stale」か「anchor=unreadable」を足す。新しい周の行は clear も busy も 1 字も変わらない。
  5. **印を外す口**: `PipeCommand` に variant を 1 つ足す（字面 anchor-sync・usage に 1 行・`crates/scribe2/src/help.rs` の pipe の面の subcommands の表に 1 行・flag は `pipe report` と同じ 3 つ〔`--state-dir` / `--repo` / `--rules`〕）。「pipe anchor-sync --repo R」は形 3 の材料を読み、次の 6 つのどれかを 1 行で返す。(a) 印が無い: 「anchor-sync=none」rc 0。(b) 古い path が無い、または印が読めなくても tracked の変更が 1 つも無い（古い中身を持ちえない）: 印を消して「anchor-sync=already」rc 0。(c) L のうち、index と作業の木の両方が from と同じで、足された path なら作業の木に file が無い path だけを、`git --literal-pathspecs restore --source=<HEAD> --staged --worktree --` で HEAD の中身へ戻す。戻した後の形 3 が新しいなら印を消して「anchor-sync=synced paths=<本数>」rc 0。(d) 片方だけが from と同じ path と、足された path に file が在る path は戻さない。path を名指して「anchor-sync=refused mixed=<path,…>」rc 1（戻せる path は戻し、印は残す）。(e) 印が読めず tracked の変更が在る周と、git を読めない周は「anchor-sync=refused unreadable:<理由>」rc 1。(f) restore が断った周は「anchor-sync=failed restore:<git の 1 行>」rc 2（印は残す）。戻すのは index と作業の木が from の中身のままの path だけで、利用者の編集を持つ path と L の外の path は 1 byte も触らない。
  6. **不変**: `anchor_plan` の見立て（dirty は揃えない）・`sync_anchor` の順と read-tree の形・`AnchorSkip` の 5 値・判定行の token と warning の字面・land の rc・`Failed` の detail・land-window の 3 条件（列・追随・未 push）と `--wait-s` の再評価。
- ADR を書かない判じ（ADR の条件 3・1 行）: 印は ADR-0004 D-4 の前例（git dir の下の器の名の dir の file）の中に置き、中身は sha 1 語の 1 行で、読めない印は古いと同じ側に倒れる（fail-closed）ので、版をまたいで字が変わっても誤って開く側には倒れない。新しい on-disk の契約を決めないので、新しい ADR は書かない。
- 触らない: `crates/scribe2/src/fleet/wait.rs`（`Completion::LandWindow` は `is_open` を読むだけで、窓が閉じる条件が 1 つ増えるのは形 4 の 1 か所）・`crates/scribe2/src/pipe/land/verify.rs`・event の種類と schema・rules 行（新しい値を持たない）・`crates/scribe2/src/hook/`（門は行 h）。
- 歯（接頭辞はどれも main 8f6072d の `fn` 名に 0 件・実測）:
  - lib（行 az の write-set の `+` の file の歯の区間・接頭辞 `pipe_anchor_stale_`）: 形 3 と形 5 の仕分けの純関数（入力は L の path と状態・index が from と違う path の集合・作業の木が from と違う path の集合・作業の木に在る path）。(a) index も作業の木も from と同じ path は古く戻せる (b) index だけが from と同じ path は古く戻せない（mixed）(c) 作業の木だけが from と同じ path も同じ (d) 足された path で index に無く作業の木に file が在る path は古く戻せない (e) index も作業の木も from と違う path（手で揃えた・利用者の編集）は古くない (f) L の外の path は数えない。印の 1 行の読み（16 進の 40 字か 64 字と改行 1 つだけが読める。欠け・余り・16 進でない字・2 行目は読めない）と書き（既に在る印は書き換えない・from と着地後の main が同じ周は書かない）。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/land.rs`・既存の anchor の歯と同じ fixture）: 接頭辞 `pipe_land_anchor_mark_` — (a) 便と別の file に局所の変更を置いた anchor で land すると、`Landed` の detail が「anchor=skipped:dirty」で終わり、`pipe land-window` が rc 1 で「anchor=stale」を持つ（base は detail に token が無く、窓は clear ＝機能不在の RED）(b) 同じ anchor で L を手で `git read-tree -m -u <old> <new>` に揃えると、印が在るまま land-window は「anchor=」を持たない (c) 次の便が clean な anchor に synced で着地すると印が消え、その `Landed` の detail は token を持たない。接頭辞 `pipe_anchor_sync_` — (d) (a) の anchor で「pipe anchor-sync --repo R」が rc 0 で「anchor-sync=synced paths=<L の本数>」を返し、index と HEAD の差分が空になり、局所の変更は残り、land-window が clear（base は未知の subcommand の rc 1 ＝ RED）(e) L の path の作業の木を書き換えた anchor では rc 1 でその path を mixed に名指し、その path の中身は変わらない (f) 足された path に untracked の file を置いた anchor では rc 1 で、その file の中身は変わらない (g) 印の無い repo では rc 0 の「anchor-sync=none」、中身を壊した印でも tracked の変更が無い anchor では rc 0 の「anchor-sync=already」で印が消え、tracked の変更が在れば rc 1 の unreadable (h) `help pipe` の SUBCOMMANDS に anchor-sync の 1 行が在る（base は行が無い＝RED）。
  - 既存の pin: `pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none` の本数 17 → 18 と字面の列・`pipe_external_form` の snapshot に usage の 1 行。使い方の面の歯（lib の `help_table_` は verify の行・e2e の `cli_help_` は `crates/scribe2-boundary/tests/e2e/main.rs` に在って write-set の外なので verify の行に置かず、done の定義の全数の nextest が撃つ）は字を変えずに緑（pipe の面は 49 → 50 行で上限 60 の内。足す行は ASCII で幅 100 字の内。pipe の FORM の `<…|…>` の語は変えないので、FORM と生きた使い方の突き合わせも動かない）。既存の anchor の歯（`pipe_land_anchor_`）と land-window の歯（`pipe_land_window_`）と synced の周の detail を pin する歯 5 本は 1 字も変えずに緑。
- 限界: 印は land が揃えなかった周にしか置かれない（人が terminal で main を動かす周・他の道具が ref だけを動かす周は測らない）。git stash に入った古い中身は測らない（stash を戻した後は、印が在る間だけ形 3 が拾う）。判定は L の path に限り、利用者が古い中身の上に書いた編集（index も作業の木も from と違う path）は古いと言わない（編集の出所は git の中身から分からない）。CAS と印の書きの間（ミリ秒）に撃たれた commit は拾わない（その間は行 h の窓の門が、着地中の便で閉じている）。
- 却下: (1) 印を置き場（state dir）に anchor の path を鍵にして置く（index は checkout ごとの git dir に在る。鍵の正規化〔symlink・末尾の区切り〕と linked worktree の除外を別に書くことになる。ADR-0004 D-4 の git dir の file の形を採る）(2) 古さを印の有無だけで決める（手で揃えた周も古いと言い続け、口を撃つまで門が閉じたまま）／index の tree と HEAD の tree の一致で決める（利用者が staged にした編集で一致せず、古いと言い続ける）(3) 印を外す口を `git read-tree -m -u <from> HEAD` の 1 形にする（L に利用者の編集が 1 つでも在ると全体が断られ、古い path も戻せない＝実測 rc 128）(4) land が dirty でも L と重ならなければ揃える（印の頻度は下がるが、.120 の見立て〔dirty は触らない〕を変え、席が編集中の木を land が書き換える窓を開く・別の行で測る）(5) 散文の務め（「commit の前に `git status` と land-window を見る」）を席の指示に足す（N2・今日の事故がその形で起きた）(6) event の種類を足し、印を event log の replay から導く（跨版の schema の変更で ADR 条件 3 に当たる。印は checkout の状態で、便の状態ではない）。

## 58. gate の段 ① が契約の約束を便の木で測る — write-set の `+` の file が在り `~` の file が無く、約束の行の新設の名が解けることを出所で測る（契約表の行 ba・[dispatcher.md](./dispatcher.md) §27 の安全の段・裁定 user 2026-09-27T14:02Z）

やさしく言うと: 契約が「この file を新しく作る」と約束しても、器は作ったかを 1 度も確かめずに着地させていた。gate の最初の段で、約束した file と名が便の木に在るかを測り、無ければ落とす。依存の約束を当てにして待ち行を先に審査する（dispatcher.md §27）ので、約束は出所で守らせる。着地の時の main の実測も同じ段を同じ関数で撃つので、同じ約束がもう 1 度測られる。

- 何が起きているか（実測 2026-09-27・verified）: 受付は `+` の項目が base に無いこと（`WriteSetItem::New`・`MustBeAbsent`）しか測らず、gate の段 ①（`check_write_set`）は diff の path が write-set の内に在るかだけを測る。`WriteSetItem::New` を読む file は `crates/scribe2/src/pipe/declaration/write_set.rs` と `crates/scribe2/src/pipe/cli/intake.rs` の 2 本だけで、gate と land は `+` の file が作られたかを測らない。設計 doc の契約表の write-set の `+` の項目は 232（重複なし 136 path）で、main に無いのは 2 path: 1 つは作られた後に別の便が正しく消し、もう 1 つ（consumer-sync.md 行 f の新設 file）は git の全履歴に 1 度も現れない＝宣言した新設 file を作らずに着地した実例（232 中 1）。`~` の項目は 52 path で、main に残るものは 0。約束の行の `symbols` は 11 で、うち新設の名（`+`）は 3。
- 現物（main 8f6072d・verified）:
  - 段 ① は `crates/scribe2/src/pipe/gate/verify.rs` の `check_write_set`（:395・`git diff --name-only -z <base>..HEAD` の path を write-set と照らす・外れは rc 1・読めない周は rc -1＝`is_unreadable`）。段の列は `run_checks`（:196）の 1 本で、gate と land の主実測がどちらもこれを撃つ。land の主実測は `crates/scribe2/src/pipe/land/verify.rs` の `verify_main_from`（:57）で、列の着地（`verify_train_main`）も同じ本体を通る。gate が判定した木と着地の木が同じ周は 1 本も撃たない（同じ file の :58 の skip）。
  - 約束の行の名の読み手は、受付の `check_symbols`（`crates/scribe2/src/pipe/cli/intake.rs`:508）が使う `symbols_in_base`（`crates/scribe2/src/pipe/closure/names.rs`:52・pub）である。本文の材料は `read_all`（`table` の再輸出・`crates/scribe2/src/pipe/table/check.rs`:710・pub(crate)・`.rs` の本文）、表の読み手は `read_table` と `promises_of`（`crates/scribe2/src/pipe/table/parse.rs`:163）で、どれも gate から呼べる。便の写し（`Contract`）は design の pointer を持ち、`symbols` は持たない。
  - e2e の置き場: 段 ① の既存の歯は gate の e2e の親 `crates/scribe2-boundary/tests/e2e/pipe/gate.rs`（:68・:137・:150）に在る。子 module は族ごとの 3 本（`confine` は封じ込めと受付札・`detection` は検出線と着地後の検出・`pure_move` は純移動の証明と diff の畳み）で、段 ① の約束の族に合う子は無い。親の `mod` の 1 行だけが動く宣言 file は flip-check が本体と同梱する（`s2-07l.685` の 3 本と同じ形）。e2e の全体の歯（`e2e_fixture_clock_dated_reset_lines_are_pinned`）は tracked な file と `mod` 宣言の一致を測り、本数を pin しない。着地の主実測を撃たせる fixture は `make_tree_differ`（`crates/scribe2-boundary/tests/e2e/pipe/land.rs`:807・pub(super)）が在る。
- 形（番号は done と 1:1）:
  1. **`+` と `~` を便の HEAD で測る**: 段 ① は diff の照合の後に、契約の write-set の `+` の項目（接頭辞を剥がした path）が便の HEAD の木に全部在ること、`~` の項目が全部無いことを測る（`git ls-tree` 1 回の読み・言語に依らない）。外れた項目は段 ① の rc 1 と stderr の見出しつきの列で名指す（diff の外れと同じ段・同じ極性＝FAIL）。木を読めない周は rc -1（今の読めない周と同じ・INCONCLUSIVE）。外れを返すのは形 2 と同じ pure な 1 関数である（入力は木の path の列・write-set・約束の名・本文）。
  2. **約束の行の新設の名を便の木で測る（Rust に依る）**: 契約の design が設計 pointer でその行が約束の行を持つ周だけ、便の base（段に渡る `base`）の設計 doc から約束の行の `symbols` の `+` の名を引き、便の HEAD の木の `.rs` の本文で `symbols_in_base` と同じ読み手で解く。解けない名を段 ① の rc 1 で名指す。本文の読み手が読まない言語の repo では解ける名が 0 本になりうるので、読み手の母集団（今は `.rs`）の file を 1 本も持たない木では名を測らず、段 ① の stderr にその旨の 1 行を残す（rc は変えない・測れないを通ったと言わない）。
  3. **判定の語彙は足さない**: 新しい段・理由の型・verdict を作らず、段 ① の rc と stderr で運ぶ。land の主実測（`verify_main_from`）も同じ `run_checks` の段 ① で同じ 1 本を撃つので、着地の時点でも同じ約束が測られる。
- 触らない: diff の照合の規則（`listed` と接頭辞の剥がし）・段の順（`CHECKS`）・受付の `MustBeAbsent` と `check_symbols`・lens の材料・再 gate と追随の口・列の便ごとの記録（`crates/scribe2/src/pipe/train.rs` の `checks_on` は段 ② ④ だけを残す）。
- 歯（接頭辞 `pipe_gate_promised_`・`grep -rn "fn pipe_gate_promised_" crates/` は 0 件・2026-09-27）:
- 既存の歯への波及（実測・便 s2-07l.697 の 1 回目の gate FAIL の根）: 畳みの歯の helper（gate の e2e の親 file の elide_run）は rename しない周にも + の先を write-set に宣言していたので、段 ① の新しい測りで rename しない周の歯が FAIL に倒れる。helper を rename の周だけ + の先を宣言する形に直し（歯の本文と期待は変えない）、flip-check の retroactive の札を 1 行添える。親 file に足すのは子 module の mod の 1 行とこの直しだけ。
  - lib（`crates/scribe2/src/pipe/gate/verify.rs` の歯の区間）: 形 1 と形 2 の外れを返す pure な 1 本。(a) `+` の file が木に在れば外れ無し、無ければ名指す (b) `~` の file が木に在れば名指す、無ければ外れ無し (c) 接頭辞なしの項目は測らない (d) 約束の `+` の名は、`.rs` の本文に宣言が在れば解け、無ければ名指す (e) `.rs` を 1 本も持たない木では名を測らずに外れ 0 と注記の 1 行を返し、同じ木の `+` / `~` の path の照合は続ける。
  - e2e（行 ba の write-set の `+` の file・gate の e2e の親の子 module で、族は段 ① の約束・親は `mod` の 1 行だけを足す・偽 runner）: (1) 契約が `+` の file を宣言し、偽 runner がそれを作らずに commit した便の gate が FAIL で、record の段 ① が rc 1 と file の path を持つ。作った便は PASS のまま。(2) 約束の行を持つ toy の設計 doc（`symbols` に `+` の fn の名・受付の約束の行の歯と同じ形）で、偽 runner が `+` の file は作るがその名を宣言しない便の gate が FAIL で、段 ① がその名を名指す。宣言した便は PASS。(3) PASS した便の写し（`contract_path` の file）の write-set に、便の木に無い `+` の項目を 1 つ足し、`make_tree_differ` で land に主実測を撃たせると、land は main-red（rc 1）で終わり、`verify-main.jsonl` の段 ① の record が rc 1 でその path を名指す（record は stderr を持たないので、path は同じ stem の診断 file `verify-main.stderr.log` の段 ① の見出しの下で測る・gate の側も `verify.stderr.log` で同じ）。
  - base で RED: lib は filter の該当 0 本（rc 4）、e2e は (1)(2) で base の段 ① が約束を測らず gate が PASS、(3) で land が Landed に着く（どれも機能不在）。
- 限界: 約束が守られても中身が空の file は通る（在るかだけを測る）。`.rs` の無い木の注記の 1 行は段 ① の stderr に載るが、診断 file は赤い段の stderr だけを残すので、段 ① が緑の周の注記は record にも診断 file にも残らない。約束の行を持たない行の新設の名（散文の backtick）は測らない（契約表の検査の §39 の母集団は宣言の名であって約束ではない）。gate が判定した木と着地の木が同じ周は、land は段を撃たず gate の判定を使う（木が同じなので同じ結果）。列の着地では write-set は列の便の和で測る（`+` の在りかは和で測っても同じ）。
- 却下: 受付で依存の `+` の在りかを測る（着地前の依存は測れない・約束の守りは出所の段が筋）／新しい段 ⑤ を足す（`CHECKS` と record の kind と land の段の列が動く・段 ① の照合は同じ diff の読みの隣）／land の後にだけ測る（着地した後では約束を破った便を戻せない）／e2e を既存の子 `pure_move` に置く（純移動の証明の族で主題が違う）・親の gate.rs に足す（段 ① の既存の歯と同じ file だが、親は族ごとに子へ割っている途中で、新しい族は子に置く）。

## 59. pipe/declaration.rs の任意 key の群を子 module へ割る（契約表の行 bb・純移動・FR83 / FR85 / FR86 の新しい key の受け皿）

やさしく言うと: 宣言 file（`.vessel.toml`）の任意の key を読む部分を、1 file の上限（1500 行）に迫った `crates/scribe2/src/pipe/declaration.rs` から子の file へ、中身を変えずに移す。後の行が足す 4 つの key（ruling-check・ruling-fixtures・floor-check・question-route）の置き場を先に空ける。

- 何が起きているか（main 24f6ef1e・2026-09-29・verified）: `crates/scribe2/src/pipe/declaration.rs` は 1476 行（幅 120 で正規化 1486・上限 R-C4-2 = 1500）で**余地 14**。SRS の FR83（ruling-check と ruling-fixtures）・FR85（floor-check）・FR86（question-route）は宣言の任意 key を 4 つ足し、どれも key の名・値の読み手・key の列・HEAD の宣言から値を外へ渡す口をこの file に足す。受付は write-set の file ごとに余地を測る（`cap-headroom`）ので、割らないとそれらの行は受付を通らない。
- 移す群（実測・17 item・HEAD 側 136 行）:
  - key の列: `DECLARED_KEYS`（40–54 行）と `OPTIONAL_KEYS`（96–108 行）。
  - key の名と既定: `REMOTE_KEY`・`CI_CMD_KEY`・`DEFAULT_CI_CMD`・`CI_SHA_HOLE`・`REQUIREMENTS_KEY`・`DEFAULT_REQUIREMENTS`（56–77 行）。
  - 値の読み手: `requirements_of`・`remote_of`・`ci_cmd_of`・`repo_relative`（609–658 行）。
  - 外へ渡す口: `TableFacts`・`table_facts`・`table_facts_named`・`TerminalFacts`・`terminal_facts`（660–707 行）。
  - 4 つの塊の後ろの空行 1 本ずつも親から消える。
- 群の閉じ方（grep で数えた・main 24f6ef1e）:
  - 親の本体が群を裸で呼ぶのは 2 か所だけ: `Declared` の `parse`（`DECLARED_KEYS`・`requirements_of`・`remote_of`・`ci_cmd_of`）と `fields`（`OPTIONAL_KEYS`）。親の私有の `use` 1 行で解けるので、親の本体は 1 字も変わらない。`declared_at_head` の doc の link（`terminal_facts`）は再輸出で解ける。
  - crate の中で `declaration` の path を通して群を引くのは 6 file: `crates/scribe2/src/fleet/wait.rs`（`CI_SHA_HOLE`）・`crates/scribe2/src/pipe/dispatch/prelens.rs` と `crates/scribe2/src/pipe/cli/step.rs`（`table_facts`）・`crates/scribe2/src/pipe/cli/intake.rs`（`table_facts` と `TableFacts`・歯の中の構築を含む）・`crates/scribe2/src/pipe/table/check.rs`（`table_facts_named`）・`crates/scribe2/src/pipe/land/finish.rs`（`terminal_facts`）。親の `pub use` 2 行で path が変わらないので、6 file は 1 字も変わらない。`DEFAULT_CI_CMD` と `DEFAULT_REQUIREMENTS` は外の site が 0 だが、公開の path を保つため同じ `pub use` に入れる。
  - 群が親から引く名は 9 つ: `Raw`・`DeclError`（私有の `new` を含む）・`Sourced`（私有の `read` と `measure`・欄 `declared`）・`Ceiling`・`EntranceFlip`・`declared_at_head`・`DETECTION_KEY`・入口の flip の key の別名（親の私有の `use` の束縛）・module `path_kinds`。子の `use super::` の 2 行で引ける（子孫は祖先の私有の item と束縛を見る）。
  - 兄弟の子 module（`crates/scribe2/src/pipe/declaration/entrance_flip.rs`・`crates/scribe2/src/pipe/declaration/path_kinds.rs`・`crates/scribe2/src/pipe/declaration/write_set.rs`）は群の名を 1 つも引かない。
  - 閉包: 他の行の `touches` が持つ `declaration` の型は `EntranceFlip`（本 doc の行 ay）と `WriteSetItem`（contract-source.md の 2 行）だけ。群は `EntranceFlip` を戻り型で名指すだけで、閉包の 5 形（構築・match の arm・件数 pin・const slice・variant の構築）のどれにも当たらない。
- 歯の置き場（実測）: 親の歯は in-file の `mod tests {`（904 行から末尾・23 本・573 行）の 1 つ。純移動の証明（`crates/scribe2/src/pipe/move_proof.rs`）は in-file の `mod tests {` を 1 item に畳むので、歯の一部を子へ出すと `mod tests` の本文が変わって `items-differ` で落ちる＝歯は 1 本も動かさない。歯の `use super::{…}` が名指す群の名（`CI_SHA_HOLE`・`DECLARED_KEYS`・`DEFAULT_CI_CMD`）は親の `pub use` と私有の `use` が解くので、歯の本文と `use` は 1 byte も変わらない。
- 形（番号は done と 1:1）:
  1. 上の 17 item を、行 bb の write-set の `+` の file（`crates/scribe2/src/pipe/declaration.rs` の子 module）へ名・本文・順序を変えずに移す。子の頭は module doc と `use` 3 行（`use super::path_kinds;`・上の 8 名を引く `use super::{…}`・`use std::path::Path;`）だけ。
  2. 親に増えるのは 4 行だけ: `mod` 宣言 1 行（既存の `mod entrance_flip;` の隣）・`pub use` 2 行（外へ渡す口の 5 名と、既定と穴の 3 名）・私有の `use` 1 行（`parse` と `fields` が裸で呼ぶ 5 名）。どれも 120 桁に収め、属性と `use` を同じ行に書かない。親の本体と外の 6 file は 1 字も変わらない。
  3. 可視性を上げるのは子の側だけで、語は `pub(super)` の 1 種・5 つ（`DECLARED_KEYS`・`OPTIONAL_KEYS`・`requirements_of`・`remote_of`・`ci_cmd_of`）。子の中だけで使う `REMOTE_KEY`・`CI_CMD_KEY`・`REQUIREMENTS_KEY`・`repo_relative` は私有のまま、`pub` の 8 つは `pub` のまま。親の側の可視性は 1 語も変えない。
  4. 歯は動かさない: in-file の歯 23 本の名・本文・`use` は不変で、増えるのは札 `// flip-check: moved <行 bb の bead>` の 1 行（親の `mod tests` の最後の行＝閉じの `}` の直前・頭に置くと後ろの doc 行が全部ずれて審査の要約のコメント行の差が 74 行に膨らむ）だけ。子は歯の区間を持たないので札を置かない（歯の区間の外の札は flip-check に数えられない・§45 約束 5）。
  5. 兄弟の子 module 3 本は 1 字も変えない。
  6. 割った後の行数は、親が 1328 行（正規化 約 1337・余地 約 163）・`+` の file が 162 行（正規化 約 163）。core の本体は約 13 行増える（子の module doc と `use`・親の `mod` と `use`）。
- 受け皿の使い方（後の行が決める・本行では足さない）: 新しい任意 key 1 本ごとに、key の名の const・読み手・`DECLARED_KEYS` と `OPTIONAL_KEYS` の 1 行ずつ・HEAD の宣言から値を渡す口（`terminal_facts` と同じ形）は子へ、`Declared` の欄 1 つと `parse` の読みの 1 行は親へ足す（親の増分は key 1 本あたり約 5 行）。
- 触らない: `Declared`・`Sourced`・`Effective` と値の層（`fields`・`value_of`・`int_of`・`text_of`・`list_of`・`Raw`）・`DETECTION_KEY` と `EFFECTIVE_KEYS`・`head_declaration` と `declared_at_head`・verify 行の判定（`unfit` の群・`VerifyKind`・`KindGap`）・`scaffold`・兄弟の子 module 3 本・in-file の歯の本文・外の 6 file・e2e。
- 却下: (a) key の名と読み手だけを移す（約 75 行）— key の列 2 つと外へ渡す口が親に残り、新しい key の行が親の 3 か所を触り続ける（受け皿にならない）。(b) 値の層（`fields`・`value_of`・`Raw` ほか）を移す — `Declared` と `Effective` の 2 つの読みが共有し、兄弟の path_kinds と entrance_flip も `use super::` で引くので兄弟の `use` を書き換えることになる。新しい key の受け皿でもない。(c) in-file の歯を `#[path]` の file へ出す — 余地は約 580 空くが読み手の受け皿は生まれず、次の行の歯が親の hub に戻る。(d) `DETECTION_KEY` も移す — 便の写し（`EFFECTIVE_KEYS`・`render`・`Effective` の `parse`）と上限の突き合わせで親が 5 か所使う core の key で、HEAD から外へ渡す任意 key ではない。(e) `Declared` の任意の欄を子の構造体へ束ねる（path の種別の `DeclaredPaths` の形）— 型の形と歯の欄の読み（`requirements`・`remote`・`ci_cmd`）が変わる＝純移動でない。要るなら後の行が決める。
- 歯: 新設は 0 本（純移動・新しい接頭辞は無い）。検証行は群を測る既存の lib の歯 6 本を**名の全体**で 1 行 1 本に名指す: `declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema`（key の列 11 本の pin）・`declaration_names_every_missing_key`（必須と任意の区別）・`declaration_requirements_is_an_optional_repo_relative_path`・`pipe_terminal_land_remote_is_an_optional_single_word`・`pipe_terminal_land_ci_cmd_must_carry_the_sha_hole`・`pipe_declaration_default_ci_cmd_carries_the_event_field`。6 本とも `crates/` の中で `crates/scribe2/src/pipe/declaration.rs` の 1 file だけに在る（grep・2026-09-29）＝歯の置き場の門が見る file は write-set の `-` の親だけ。外へ渡す口の e2e（契約表の検査と着地の終端）は done の全体の nextest が撃つ。
- base で RED の理由: 無い。歯を足さない純移動で、検証行 6 本は base でも HEAD でも緑（不変の証明）。入口の RED は札 `moved` が免除する（§7）。
- 実測（作業用の写しに本行の形を当てた・2026-09-29）: 純移動の証明が「名 + 本文の多重集合が一致 items=68 moved=17 visibility=5」・入口の flip-check が `RED-on-base ok tests_changed=0 moved=1`（rc 0）・`cargo clippy -p scribe2 --all-targets -- -D warnings` が rc 0・検証行の 6 本と現物の契約表の歯 2 本（`contract_closure_ext_real_table_has_zero_findings`・`contract_names_declared_real_table_has_zero_findings`）が緑・`cargo xtask check` が ok。

## 60. main-provenance が器の便でない先端の commit に発端の trailer を要る — 件名の (#N) に本文の発端の行を足し、(#N) だけの commit を名指して落とす（契約表の行 bc・[FR92](../../design-intent/spec/srs.html#FR92) / AC62・[ADR-0088](../../design-intent/decisions/ADR-0088-case-positions-are-computed-once-by-the-vessel-and-read-from-one-file.html) (7)）

やさしく言うと: main に入る commit のうち、器の便（自動の着地）でないもの＝人が PR を merge したものは、どの台帳の件のための変更かを本文の 1 行で名乗らないと CI が赤くなる。名乗り方の決まり（文法）と merge の手順の正本は [vessel-hook.md](./vessel-hook.md) §21 で、ここは CI の側の読み手を作る。

- 何が起きているか（main 7c4ab0a1・verified）:
  - `crates/xtask/src/provenance.rs` の `provenance_of` は件名の末尾の ` (#N)` を先に見て、無ければ 2 行目から後ろの `run: <run id>`（`is_run_id` の閉じた形）を探す。`verdict` の判定行は `ok via=pr number=<N>`・`ok via=land run=<run id>`・`FAIL reason=no-provenance` の 3 つで、発端の trailer は読まない。歯は in-file の 4 本。
  - CI の job main-provenance（`.github/workflows/ci.yml`）は push(main) のときだけ、既定の `--rev HEAD` で撃つ。checkout は既定の深さで、測るのは**その push の先端の 1 commit だけ**である。
  - main の直近 400 commit（2026-09-29）: 件名の末尾が (#N) の squash 252・本文に `run:` を持つ器の便の着地 147・どちらも無い手の merge commit 1。
  - squash の本文と件名: repo の既定は、本文が PR の commit の message の列、件名が commit か PR の題に ` (#N)` を足したもの。repo は merge commit と rebase merge も許す（どちらも先端の件名が (#N) で終わらない）。gh 2.45.0 の `gh pr merge` は `-b` / `--body`（merge の commit の本文）・`-F` / `--body-file`（file か `-` で標準入力）・`-t` / `--subject` を持つ。
  - xtask の依存は 0（`crates/xtask/Cargo.toml`）で core を引けず、core も xtask に依存しない。bead id の閉じた形は `crates/xtask/src/flipcheck.rs` の `is_bead_id`（run id の頭にも使う）。trailer の key は core では `crates/scribe2/src/pipe/land/finish.rs` の `trailer_key` が NAME から導く（先頭を大文字にして `-<語幹>: ` を足す）。xtask は NAME を `crates/xtask/src/workspace.rs` の `Layout` で core の name.rs から読む。
- 形（番号は done と 1:1）:
  1. **入口の判定**: 本文（2 行目から後ろ）を形 2 で `run` / `source` / `none` の 3 語に分け、件名の (#N) と組む。
     - `run` → `main-provenance: ok via=land run=<run id> sha=<sha>`（今のまま・件名に依らない）。
     - `source` で件名が (#N) で終わる → `main-provenance: ok via=pr number=<N> source=<id>[,<id>…] sha=<sha>`（rc 0）。
     - `none` で件名が (#N) で終わる → `main-provenance: FAIL reason=no-source number=<N> sha=<sha>`（rc 1）。(#N) だけの commit をこの行が名指す。
     - 件名が (#N) で終わらず `run` でもない → `main-provenance: FAIL reason=no-provenance sha=<sha>`（rc 1）。発端の行を持っていても直接の push は落とす（(#N) が PR を通った唯一の印）。
     - git を撃てない・rev が解けない・workspace の NAME を読めない周は、判定行を出さず stderr に理由を出して rc 2（今と同じ極性・測れないを通過にも赤にも化けさせない）。
  2. **本文の分け方**（正本は [vessel-hook.md](./vessel-hook.md) §21 形 2。ここは xtask の読み手が実装する形の写しで、2 つが 1 字でも違えば形 3 の見本の歯が落ちる）:
     - key は NAME から導く: 先頭を大文字にした NAME に `-Source:` を足す（本 repo では `Scribe2-Source:`）。
     - 本文の行は、各行から末尾の CR と、末尾の半角空白と tab を落とした字。本文の行のうち、**行頭から key で始まる行**を発端の行と読む。字下げした行と大小の違う行は発端の行でない（`run:` の読みと同じ）。
     - 発端の行が形に合うのは、その字が「key・半角空白 1 つ・bead id（`is_bead_id` の形）を半角空白 1 つで区切って 1 本以上」だけのとき。id の無い行・空白 2 つ（空の id）・`,` か読点の区切り・key の直後の空白の欠け・key の後ろか id の間の tab・形に合わない id は形の外。
     - 分け方の順: 形の外の発端の行を 1 本でも持つ本文は `none`（他の行に依らない）。そうでなく `is_run_id` の形の `run:` の行を持つ本文は `run`。そうでなく発端の行を 1 本以上持つ本文は `source`（id は全部の発端の行の id を出てきた順に並べる）。どれでもなければ `none`。
     - 台帳の接頭辞と台帳での実在は見ない（CI は台帳に届かない。打ち違いと台帳に無い発端は局面の出力の misfit が数える・FR92）。
  3. **2 つの読み手を 1 つの見本で揃える**: 行 bc の write-set の `+` の file（text の見本）に、塊ごとに期待の語と本文を置く。形: 行頭 `#` は注、行 `===` が塊の区切り、塊の 1 行目が期待の語（`source` / `run` / `none`）、2 行目から区切りの前までが本文（件名は含めない）。xtask の歯はこの file を読んで各塊を形 2 で分け、語が期待と一致することを確かめる。[vessel-hook.md](./vessel-hook.md) の行 mg の歯（merge の門の読み手・core）も同じ file を読む（`source` と `run` を通過、`none` を断りと読む）。core と xtask は互いに依存しないので読み手の実装は 2 つ、見本は 1 つである。CR を含む行は file の改行の扱いに依らないよう見本に置かず、両方の歯が本文を組んで測る。塊は少なくとも次の 24（key は本 repo の字）:

     | # | 期待 | 本文 |
     |---|---|---|
     | 1 | source | `Scribe2-Source: s2-07l.739` |
     | 2 | source | `Scribe2-Source: s2-07l.739 s2-07l.722`（id 2 本・AC62） |
     | 3 | source | 要旨の段落・空行・`Co-Authored-By:` の行・`Scribe2-Source: s2-a.1`（本文のどこに在ってもよい） |
     | 4 | source | `Scribe2-Source: s2-a.1` の後ろに半角空白（行末の空白は除く） |
     | 5 | source | `Scribe2-Source: s2-a.1` と `Scribe2-Source: s2-b.2` の 2 行 |
     | 6 | run | `run: s2-07l.351-20260922T061245Z` と `Scribe2-Contract: docs/design/pipeline.md#b` |
     | 7 | none | 空の本文 |
     | 8 | none | `Scribe2-Source:`（id なし） |
     | 9 | none | `Scribe2-Source: TODO` |
     | 10 | none | `Scribe2-Source: s2-a.1  s2-b.2`（空白 2 つ） |
     | 11 | none | `Scribe2-Source: s2-a.1,s2-b.2` |
     | 12 | none | `Scribe2-Source:s2-a.1`（key の直後の空白なし） |
     | 13 | none | 行頭に半角空白 2 つを置いた `Scribe2-Source: s2-a.1` |
     | 14 | none | `scribe2-source: s2-a.1`（小文字） |
     | 15 | none | `Scribe2-Source: s2-a.1` と `Scribe2-Source: s2-07l..3` の 2 行（1 本でも形の外） |
     | 16 | none | `run: s2-07l.351-20260922T061245Z` と `Scribe2-Source: TODO`（形の外の発端の行が run より先） |
     | 17 | none | `Scribe2-Source: S2-A.1`（大文字の id） |
     | 18 | source | `Scribe2-Source: s2-a.1` の後ろに tab（行末の tab は除く） |
     | 19 | none | `Scribe2-Source:` の直後に tab 1 つと `s2-a.1`（tab は区切りでない） |
     | 20 | none | `Scribe2-Source: s2-a.1、`（末尾に読点） |
     | 21 | none | `Scribe2-Source: 12345`（`-` の無い数字だけの id） |
     | 22 | none | 文の途中に `Scribe2-Source: s2-a.1` の字を持つ行だけ（行頭でないので発端の行でない） |
     | 23 | none | `run: TODO`（run id の形でない run の行は数えない） |
     | 24 | run | `run: s2-07l.351-20260922T061245Z` と `Scribe2-Source: s2-a.1`（両方が形に合えば run を先に採る） |
  4. **NAME の読み**: `run` は cwd の workspace を `Layout` で読んで NAME を得て key を組む。CI は repo の root で撃つので今の job のまま読める。
  5. **CI の job と口は変えない**: `.github/workflows/ci.yml`・`--rev` の読み・USAGE の字・`is_run_id` と `is_bead_id` の形は 1 字も変えない（job の頭の注だけは、本行が本 § と vessel-hook.md §21 を指す形に直す＝write-set の `.github/workflows/ci.yml`。docs PR で直すと flip-check が docs の面の外の変更として no-test-diff で落とす）。先端の 1 commit だけを測る形のまま。`crates/xtask/src/provenance.rs` の頭の doc を入口の 3 形と発端の行の読みに書き直す。
- 切り替えの線（新しい状態を持たない）: CI は push(main) ごとに**その先端の木に在る** xtask を build して先端の 1 commit だけを測る。ゆえに線は「行 bc の着地を木に持つ最初の push(main) の先端」で、それより前に main に入った commit は二度と先端として測られない（古い run の再実行は古い commit の木の xtask を撃つ）。state file・日付・sha の定数は要らない。行 bc の着地の commit は器の便の land（`run:`）なので自分では落ちない。局面の出力の側の線（event `LifecycleCutover`・AC62 の「線の前の trailer の無い commit は misfit に数えない」）は案件の局面の行の持ち分で、本行は触らない。
- 着地の後（手順の変わり目・自分を締め出さないために）: 行 bc の着地の**直後から**（PATH の binary の入れ替えを待たない。CI は先端の木の xtask を撃つ）、この repo の器の便でない merge は全部、本文 file の最後の段落に `Scribe2-Source: <bead id>` の 1 行を置いて `gh pr merge <N> --squash --body-file <絶対 path>` で撃つ。手順の正本は [vessel-hook.md](./vessel-hook.md) §21 の「席の merge の手順」（`--subject` を渡さない・`--merge` と `--rebase` を使わない・渡した本文は既定の本文を置き換える・merge の後に先端の本文と CI を確かめる・忘れた周は履歴を書き換えず次の push で直す・画面の merge の本文の欄にも足す）。[vessel-hook.md](./vessel-hook.md) の行 mg（merge の門）は行 bc の着地の後に起こし（行 bc の見本を読むため・台帳の blocks で持つ）、その着地と PATH の binary の入れ替えの後は、本文の無い `gh pr merge` を実行の前に断る（手順は同じ）。
- 触らない: `.github/workflows/ci.yml`（job・撃つ時機・checkout の深さ）・`crates/xtask/src/main.rs` の分岐と USAGE・`run:` の判定と `is_run_id`・`is_bead_id`・器の land の squash の本文（`trailer_key` と `run:` の行）・merge の門（[vessel-hook.md](./vessel-hook.md) の行 mg）・局面の出力の misfit（台帳に無い発端・event log に無い便・線の後の trailer の無い commit）・flip-check。
- 限界:
  - 先端しか測らないので、1 回の push に載った途中の commit は測らない（今と同じ。器の land の連なりの途中は便の commit）。
  - 形だけを見る。台帳に無い id・打ち違いは CI を通り、局面の出力の misfit でだけ見える（FR92・ADR-0088 の代償 N6）。
  - 発端の行をまねた直接の push は、件名を ` (#N)` で終わらせれば通る（今の (#N) と同じ強さ。事故の push を拾う門で、偽りを拾う門ではない）。
- 却下:
  - 発端の行だけで通す（(#N) を要らない）。直接の push が発端の行を写すと通り、§7 約束 3 が塞いだ「PR の flip-check を通らない push」が開く。
  - push の範囲（前の先端から今の先端まで）を測る。workflow に前の先端の sha を渡す変更と fetch の深さが要り、force の周と初回の push の扱いを決める手が要る。先端だけなら状態も線の定数も要らない。
  - 線の sha か日付を xtask の定数か state file に持つ。先端だけを測る形では要らない。
  - xtask に `Scribe2-Source` を字で焼く。名を 2 か所に持つ（C2.2）。
  - xtask を core に依存させて core の読み手を呼ぶ。xtask は依存 0 で core を測る側に居る（core の build が壊れた周に門も壊れる）。
  - 見本を 2 つの歯へ字で写す。片方だけ直すと静かにずれる。
  - merge commit と rebase merge も通す。repo の約束は「1 bead = 1 PR・squash」で、先端の件名で PR を見分けられない。
- 歯（`crates/xtask/src/provenance.rs` の in-file・接頭辞 `main_provenance_source_`・`grep -rn main_provenance_source_ crates/` は 0 件・2026-09-29。歯は NAME の小文字の字を引用符で書かない＝name-literal の門）:
  - (a) 見本の歯: 見本 file の塊を全部読み、各塊の本文を形 2 で分けた語が期待と一致する。key は本 repo の workspace を `Layout` で読んだ NAME から組む。塊が 24 以上・3 つの語が全部出ることも測る（母集団の件数を同時に出す）。加えて CR LF の行の本文（`Scribe2-Source: s2-a.1` の行末に CR）を歯の中で組んで `source`。
  - (b) 件名の末尾が (#542) で、本文が要旨と `Co-authored-by:` の行だけの message が `main-provenance: FAIL reason=no-source number=542 sha=abc123`・rc 1（今の「(#N) だけで通る」歯を書き換えたもの）。
  - (c) 件名が (#810) で終わり本文に `Scribe2-Source: s2-07l.739` を持つ message が `main-provenance: ok via=pr number=810 source=s2-07l.739 sha=…`・rc 0、`Scribe2-Source: s2-07l.739 s2-07l.722` を持つ message が `… source=s2-07l.739,s2-07l.722 …`・rc 0（AC62 の id 1 本と 2 本。器の便の trailer の通過は既存の歯が測る＝CI の通過 3/3）。
  - (d) 件名に (#N) が無く本文に形に合う発端の行を持つ message が `FAIL reason=no-provenance`・rc 1（回帰の歯・base でも緑）。
  - (e) key の導き: 名 `fixturename` で組んだ key では `Fixturename-Source: s2-a.1` が `source`、`Scribe2-Source: s2-a.1` が `none`。
  - 呼び出しの引数だけが動く既存の歯（verify に載せる）: 本文を分ける関数と判定行の関数（今の `provenance_of` と `verdict`）は key を引数に取る純な関数になり、NAME を読むのは `run` の 1 か所だけである（NAME を読めない周の rc 2 は `run` の 1 本の経路のまま・判定の関数は IO を持たない）。ゆえに `main_provenance_accepts_land_trailer_run_id` と `main_provenance_refuses_head_without_either_entrance` は、2 つの関数の呼び出しに歯の中で workspace の NAME から組んだ key（(a) と同じ組み方）を足すだけで、message の列・assert の期待値（`ok via=land run=… sha=def456`・11 形はどれも `no-provenance`）・歯の名は 1 字も変えない。`main_provenance_rev_argument_forms` は本文を変えない（base でも緑）。旧い引数の形の関数を残して新しい関数へ委ねる形は採らない（死んだ code になるか、関数の中で NAME を読んで rc 2 の経路が 2 本になる）。
  - 判定の順と変異（条件 1 つに歯 1 本）: 発端の行を要らなくする変異は (b)、(#N) を要らなくする変異は (d)、形を緩める変異（空の id を許す・`split_whitespace`・字下げを許す・1 本でも形に合えば通す）は (a) の none の塊、run を後に回す変異は (a) の塊 24、形の外の判定を run の後に回す変異は (a) の塊 16、key を字で焼く変異は (e)、id の区切りを変える変異は (c) が落とす。
- base で RED の理由: 歯の区間は新しい関数と引数を呼ぶので、base の本体へ写すと compile されず RED（flip-check の規則: 写した後の compile error は RED）。挙動でも (b) は base が (#N) だけで `ok via=pr` を返し、(c) は base の行が `source=` を持たない（機能不在）。(a) は base に見本 file も分ける関数も無い（機能不在）。(d) は回帰の歯で base でも緑。

## 61. runner と先撃ちの lens は Sonnet、契約の審査と gate の lens は Opus — lens の model を rules 行 `lens.model` に分け、先撃ちの lens は `--stage prelens` で rules 行 `pipe.precheck_lens_model` を読む（契約表の行 bd・裁定 user 2026-09-29T07:44Z）

やさしく言うと: いまは実装役（runner）と審査役（lens）が同じ 1 行の model を読んでいる。この行を分けて、実装役と先回りの審査は Sonnet、契約の審査と gate の審査は Opus で走らせる。effort はどれも high のまま変えない。先回りの審査の結果を契約の審査に使い回すのは、両方が同じ model のときだけにする。

- 何が起きているか（main 66d73b43・verified）:
  - model の行は `rules/manifest.toml` の `runner.model` の 1 本（値 opus）。runner（`crates/scribe2/src/headless/runner.rs`）と lens（`crates/scribe2/src/headless/lens.rs`）が同じ読み口 `runner_model`（`crates/scribe2/src/headless/mod.rs`）で読み、`--model` に渡す。lens は契約の審査（Reviewed の段・`crates/scribe2/src/pipe/review.rs`）・gate の審査（`crates/scribe2/src/pipe/gate/lens.rs`）・先撃ちの lens（`crates/scribe2/src/pipe/dispatch/prelens.rs` の `fire`）の 3 か所で同じ lens の cmd から起こされる。3 つとも lens の cmd に `--rules` が無ければ埋め込みの manifest を読む。
  - effort の行 `runner.effort`（値 high）も runner と lens が同じ読み口で読む。
  - 便用の口座選定（`crates/scribe2/src/pipe/ratelimit.rs` の `runner_model_of` → `crates/scribe2/src/fleet/select.rs` の `counts`）は `runner.model` の値を与え、5 時間窓と 7 日窓は常に数え、モデル別の 7 日窓はその model の行だけを数える。
  - 使い回し（[dispatcher.md](./dispatcher.md) §27 形 ac 1）: Reviewed の段は材料の鍵・判定・lens の cmd の字（置き場の file `lens`）が同じなら、先撃ちの判定を写して lens を撃たない。model は比べない。
  - 別名の解決（claude 2.1.284・無効な API key で課金なしに init の record を読んだ・2026-09-29）: `--model sonnet` は claude-sonnet-5-5、`--model opus` は claude-opus-5-5。別名は CLI の版に従う。
- 持ち主の裁定（user 2026-09-29T07:44Z）: 先撃ちの lens と runner は Sonnet 5.5・effort high。契約の審査の lens と gate の lens は Opus 5.5・effort high のまま。Sonnet は Fable と違って Opus と共通の 7 日窓を消費するので、モデル別の上限は考えなくてよい。
- 形（番号は done と 1:1）:
  1. **rules 行**（埋め込みの manifest・値は裁定 id つき・C1 / C5）:
     - `runner.model` の値を sonnet に替え、裁定 id を `user 2026-09-29T07:44Z`・裁定日 2026-09-29 に替える。読むのは runner と便用の口座選定だけになる。
     - 新しい行 `lens.model`（kind `LensModel`・Str・値 opus・同じ裁定）を、`--stage` を持たない lens（契約の審査と gate の審査）が読む。
     - 新しい行 `pipe.precheck_lens_model`（kind `PipePrecheckLensModel`・Str・値 sonnet・同じ裁定）を、`--stage prelens` の lens が読む。
     - 2 つの kind は宣言順で `RunnerEffort` の直後に `LensModel`・`PipePrecheckLensModel` の順で置き、`RoleModel` はその直後になる。manifest の行も `runner.effort` の直後に同じ順で置く。行数は 2 増え、kind の数も 2 増える。
     - `runner.effort` は値も裁定も変えない（runner・lens・先撃ちの lens の 3 つが同じ行を読むまま）。
  2. **読み口は 1 本**: headless の model の読み口を「行の id を引数に取る 1 関数」にし、`runner.model`・`lens.model`・`pipe.precheck_lens_model` の 3 行を同じ関数で読む。断りの 4 理由（行が無い・不発効・文字列でない・閉じた表 `Model` に無い）と極性（claude を呼ばず rc 2）は今の `runner.model` と同じ。effort の読み口は変えない。
  3. **lens の `--stage`**: lens が受ける flag に `--stage` を足す。値は `prelens` だけを取り、`--stage` が無い lens は `lens.model` を、`--stage prelens` の lens は `pipe.precheck_lens_model` を読む。`prelens` 以外の値と値の欠けは、未知の引数と同じ断りで claude を呼ばない。usage の 1 行と、help の頁（`crates/scribe2/src/help.rs` の lens の form と flags の列・生きた usage と突き合わせる歯 `cli_help_pages_match_the_live_form_and_every_subcommand` が読む）に `[--stage prelens]` を足す（便 s2-07l.736.19-20260929T080839Z の問い about:write-set・2026-09-29T08:23Z）。cap → model → effort の順と、先に落ちた理由 1 つだけを出す形は変えない。
  4. **先撃ちが `--stage prelens` を足す**: 先撃ちの `fire` は、穴を埋めた lens の行の末尾に ` --stage prelens` を足して起こす（器が lens の行の末尾に flag を足すのは、便の起こし口が `--account-dir` を足すのと同じ形）。置き場の file `lens` に写す字は今のまま穴を埋める前の lens の cmd で、足した flag を含まない。
  5. **使い回しは model の行が同じ時だけ**: 形 ac 1 の条件に「`pipe run` の審査が読む manifest の `lens.model` と `pipe.precheck_lens_model` が両方読めて、同じ `Model` に解ける」を足す。どちらかが読めない周と違う周は、今の「違えば lens を撃つ」と同じく lens を撃つ（使い回しは節約で、判定の型は変えない＝読めない側は撃つ側へ倒す）。比べる値の読みは `crates/scribe2/src/pipe/cli/step.rs` の `review_run` が持つ manifest で行い、結果を審査の入口へ渡す（`Review` の構築点は `review_run` の 1 か所・2026-09-29 に grep で 1 件）。今の値（sonnet と opus）では使い回しは起きず、契約の審査は毎回 Opus の lens を撃つ。
- 触らない: effort の行と読み口・`fleet/select.rs` の `counts`（Sonnet のモデル別の行が無ければ 5 時間窓と 7 日窓だけを数えるので、式は今のままで足りる）・`crates/scribe2/src/pipe/ratelimit.rs`（`runner.model` を読むまま）・席の model の行（`seat.model.*`）・gate の lens の起こし方（`crates/scribe2/src/pipe/gate/lens.rs`）・lens の cmd の雛形の穴・`fleet/usage.rs` の計測（model を渡さない）。
- 却下: lens の cmd の雛形に穴 `{model}` を足す（運用の雛形を全部書き換える要があり、雛形が穴を持たない周の扱いが増える）／lens が `--model` を argv で受ける（rules 行の固定を cmd の字が迂回できる・C1）／先撃ちの lens も `lens.model` を読ませ runner だけを分ける（裁定は先撃ちを Sonnet と名指す）／使い回しを先撃ちの `--stage` の字の違いで一律に止める（2 行を同じ値に戻した周の節約が消える）／effort の行も 3 つに分ける（裁定の値はどれも high・要る時に足す・C17）。
- 限界: 使い回しの比較は `pipe run` が読む manifest の値で、先撃ちの lens が実際に読んだ manifest は測らない（lens の cmd が `--rules` を持たない運用では両方が同じ binary の埋め込みを読む）。便用の口座選定は runner の model（sonnet）の窓を数え、同じ口座で走る審査の lens（opus）のモデル別の窓は数えない（裁定: Sonnet と Opus は共通の 7 日窓を消費し、モデル別の上限を持つのは Fable）。別名 sonnet / opus は CLI の版に従って黙って次の版へ移る。
- 歯（接頭辞 `model_split_`・`grep -rn model_split` は crates と docs で 0 件・2026-09-29）:
  - e2e（`crates/scribe2-boundary/tests/e2e/headless/lens.rs`）: 偽 claude で (a) `--stage` の無い lens は rules の `lens.model` の値を `--model` に渡し（`runner.model` と別の値を置いた fixture で弁別）、(b) `--stage prelens` の lens は `pipe.precheck_lens_model` の値を渡し、(c) `--stage gate` と値の無い `--stage` は断られ偽 claude の呼び出しが 0、(d) `lens.model` の無い manifest の `--stage` の無い lens と、`pipe.precheck_lens_model` の無い manifest の `--stage prelens` の lens が rc 2 で呼び出し 0、(e) `--rules` の無い lens が埋め込みの値で `--stage` 無しは opus・`--stage prelens` は sonnet を渡す。
  - e2e（`crates/scribe2-boundary/tests/e2e/headless/runner.rs`）: (f) `--rules` の無い runner が埋め込みの値 sonnet を渡す。
  - e2e（`crates/scribe2-boundary/tests/e2e/rules/embedded.rs`）: (g) 埋め込みの manifest の 3 行の値・kind・裁定 id の頭 `user 2026-09-29T07:44Z`・裁定日と、kind の宣言順 `RunnerModel` → `RunnerEffort` → `LensModel` → `PipePrecheckLensModel` → `RoleModel`。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`）: (h) 先撃ちの lens の argv の末尾 2 語が `--stage prelens` で、置き場の `lens` の字は穴を埋める前の cmd のまま (i) 形 ac 1 の使い回しの場面で、2 行が違う manifest と片方の行が無い manifest では Reviewed の段が lens を撃ち（回数が増え detail に ` prelens:reused` が無い）、2 行が同じ manifest では今どおり使い回す。
  - 直す既存の歯（同じ便）: `rules_manifest_carries_runner_model` の値と裁定 id と裁定日・`RunnerEffort` の直後を測る宣言順の歯・埋め込みの行数の歯（+2）・`rules_external_form` の snapshot（rows と kinds が 2 ずつ増える）・lens の usage を写す `headless_external_form` の snapshot・`crates/scribe2-boundary/tests/e2e/headless.rs` の fixture の helper（lens の fixture に `lens.model` の行を足し、埋め込みの値の const を runner は sonnet・lens は opus に分ける）・`crates/scribe2-boundary/tests/e2e/pipe/review.rs` の先撃ちの manifest の helper（使い回しの既存の歯が同じ値の 2 行を持つ）。
- base で RED の理由: (a)〜(e) と (h) は base の lens が `--stage` を未知の引数として断るか `lens.model` を読まないので落ち、(f) と (g) は base の埋め込みの値が opus で新しい kind が無い（compile されない）ので落ち、(i) は base の使い回しが model を比べないので落ちる（機能不在）。直す既存の歯のうち base でも緑になる file は、同じ file に base で赤い新しい歯を持つ。持たない file は test 区間の行頭に `// flip-check: retroactive <この契約の bead id>` を置く（runner が flip-check で実測する）。
- 着地の後: PATH の binary を `swap-binary.sh` で入れ替える（lens の cmd と runner の cmd は入れ替えた binary の埋め込みの manifest を読む）。消費側の席へ、runner と先撃ちの lens が Sonnet になったことを 1 行で知らせる。

## 62. 着地は、差分が足す解けない引用・裁定 id の無い判断の欄・ruling-check の外しを、main に載せず Gated に留め、理由の event を便ごとに 1 件だけ記帳して次の周に撃ち直す（契約表の行 be・[FR83](../../design-intent/spec/srs.html#FR83) / [FR10](../../design-intent/spec/srs.html#FR10) / AC53・dispatcher.md §36 の数えを使う）

### 何が起きているか（verified・main 3908279b）
- `pipe/land.rs` の land の順: 記録した base → verdict が PASS か → `--pr-cmd` の形なら PR を開いて返る → 番待ち（await_turn） → 列で着いたか → 候補の木（train） → 試行の周回。
  - Gated の detail の頭は verdict:・rebase:・rebase-conflict:・rebase-stale-rows:・stale: の 5 つ。どれも留めではない。
- 留めの形（main に載せず、Failed にせず、次の周に撃ち直す）を持つ判定は、land に 1 つも無い。FR10 の FR50・FR84 の留めも、まだ無い。
- 列（`pipe/queue.rs`）は、札が Dead の便だけを番の計算から外す。後続を積む列（train_in）は、Gated・worktree 在り・PASS の便を拾う。
- 起こし直し（`pipe/dispatch.rs` の passed_gate）は、Gated・PASS・札が無いか死んだ便を毎周 `pipe resume --drive` で起こす。verdict は review.json から読み、event の detail は読まない。
- 判断の欄の母集団（閉じた字面の案で数えた）:
  - rules の ruling 欄（toml の `ruling =` の行）: 82 行。
  - ADR の approval の裁定の欄（`class="role">裁定` の sign の行）: 69 行・64 file。
  - 設計の節の裁定の欄: 見出しが `裁定` + 空白か括弧のもの 34/509、行頭の `裁定` の欄 6 行・5 file。

### 約束
1. land は verdict が PASS と確かめた直後、`--pr-cmd` の分岐と番待ちの前に、留めの判定を 1 回撃つ。PR の形の便も同じに留める（PR を開かない）。
2. 掛けるのは、**便の base の宣言**で ruling-check が true の便だけ。宣言の読みは `dispatcher.md §36` の rev つきの読みで、base と先端の 2 回。
3. 差分は、worktree の `git diff --unified=0 <base> HEAD` の足した行（file ごと）。当たる形は次の 4 つ。
   - (a) `dispatcher.md §36` の規則で数える解けない問い id の形・batch: / policy: と、線より後の時刻の形。線の前か後かは同じ file を線の木で見る。fixtures は外す。
   - (b) 判断の欄を足すか変える行が、台帳の接頭辞の問い id の形を 1 つも持たない。bead の id だけの行も、接頭辞違いの問い id の形だけの行も当たる。判断の欄は閉じた 3 つの字面で見分ける:
     - toml の行で、剥がした字が `ruling` と `=` で始まる。
     - `design-intent/decisions/` の html の行で、`class="role">裁定` を持つ。
     - `docs/design/` の md の契約表の外の行で、見出しの行が `裁定` + 空白か括弧を持つか、リストの印と太字を剥がした字が `裁定` + `:`・`：`・`=`・括弧・` id` で始まる。
   - (c) base の宣言が ruling-check = true で、先端の宣言が false か無いのに、足した行に台帳の接頭辞の問い id の形が 1 つも無い。
4. 当たった便は main も PR も変えない。
   - `RunStage stage=Gated detail=held:FR83:<名指し>` を 1 件記帳する。名指しは並べ替えて重複を除いた列: `<id>`・`time:<時刻>@<path>`・`field:<rules|adr|design>@<path>`・`ruling-check-off`。
   - 直前の held と同じ名指しの周は記帳し直さない。名指しが変わった周は、新しい 1 件を記帳する。
   - Failed を書かず、verdict が PASS でない周と同じ rc の断りで返る。撃ち直しの上限（FR11・`retries`）に数えない（追随の数えを通らない）。
5. 当たらない周で、便の最後の held の後に released が無ければ、`RunStage stage=Gated detail=released:FR83` を 1 件記帳してから番待ちへ進む。
6. **列**: held が最後で、その後に released が無い便を、列の読み（queue_of）が列から外す。
   - 番の計算からも、後続を積む列からも外れる。自分も外れるが、判定は番待ちの前なので自分が held のまま番を待つ形は無い。
   - 候補の木の先頭は、積む前に後続を同じ判定に掛け、当たる便を積まない（記帳はその便の自走に任せる）。
7. **撃ち直し**: held の便は Gated・PASS のまま。既存の passed_gate の枝が次の周に起こし、同じ判定を撃つ。新しい枝は足さない。
   - 既存の枝は verdict の file だけを読み、held の detail を読まないので、留めの後も起こし直しの候補に残る。新しい code を持たない約束なので done には載せない（挙動に差が出ず歯では弁別できない・便の diff の設計適合は gate の審査で見る）。
8. 判定の台帳の読みは、land の `--bd` の client で 1 回。読めない周は当たりと同じく留める（名指しは `unmeasured:<語>`）。通すに読み替えない。
   - 差分も同じ: `git -c core.quotePath=false diff` で撃ち、`+++ ` の header が引用符で囲まれた形（`+++ "b/…"`・`"`・`\`・制御文字を含む path は quotePath=false でも引用される）は、引用符を剥がして C の escape（`\"`・`\\`・`\t`・`\n`・8 進の 3 桁）を戻してから `b/` を外して読む。header の path を読めない file に足した行が在る周は `unmeasured:diff-path` で留める（その file の足した行を読み飛ばして通さない）。
   - 便 s2-07l.738.37.6-20260930T162629Z の gate の審査が、`b/` を外してから引用符を剥がす順で引用された header の file を全部読み飛ばす通過（fail-open）を名指した。
9. **列の読み**: 列の読みは detail の頭 `held:` と `released:` を FR の語に依らず読む（行 bf の `held:FR84:` と `released:FR84` も同じ読みで外れて戻る）。
10. **閉包を広げない**: 子 module（留めの判定）は `Land`・`Stage`・`EventKind`・`Issue` を名指さず、repo・base・head・台帳の client の素の値を受けて名指しの列を返す。記帳（RunStage）は land.rs が書く。
    - 4 つの型はほかの行の touches に在り、子が名指すとその行の閉包に子の file が入って、現物の契約表の閉包の検査（gate の共通の verify の歯と同じ `contracts check`）が赤になる。runner が自分の verify で気づけるよう、便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (9) の歯にする（dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた・前の直しの「挙動に差が出ない」は誤り）。

### 依存
- `dispatcher.md 行 ak`（数えの関数と rev つきの宣言の読み）。台帳の依存（bead の blocks）で結ぶ。

### 歯
- `pipe_land_ruling_hold_`（e2e・`tests/e2e/pipe/land.rs`・偽の bd を PATH に置く）。
  - Gated 8/8: 差分の 3 形・判断の欄 3 種の bead の id だけ・接頭辞違いだけの判断の欄・ruling-check の外し。
  - event の 1 件: 同じ理由で land を 3 周撃っても `held:` は 1 件、`retries` = 1 を超えても Failed が無い。差分に別の解けない id を足して名指しを変えた周は `held:` が 2 件になる（便ごとに 1 件しか書かない実装を落とす・約束 4）。
  - 通過: 線の前の引用・key の無い repo・解ける 4 形。通過の便は `released:` を記帳しない（held の無い便に released を書く実装を落とす・約束 5）。
  - 解除: 台帳に裁定の行を足した次の周に着地し、`released:FR83` が 1 件。
  - PR の形: 留めで pr の command を撃たない。
  - 読めない台帳: 偽の bd が失敗する周は `held:FR83:unmeasured:<語>` で留まり、main は動かない。
  - 候補の木（約束 6）: 3 本の列の 2 本目だけが解けない問い id を足す周に、先頭の land は 1 本目と 3 本目を積んで着地させ（stdout の `train=2`・3 本目が 1 本目の上）、2 本目は Gated のまま main に載らず、2 本目の event も増えない（先頭の掛けを外す実装と、掛けの判定を固定の値にする実装を落とす）。
  - 母集団: 留め 8 本 + 通過 6 本。loop の中で確かめ終えた回数を数えて assert する（固定長の配列の `len` は型が決めるので常に真で、`cases.len()` の assert は何も測らない）。本数は assert の文にも出す。
  - base で RED: 機能不在（解けない id を足す便が main に載る）。候補の木の歯は、base の先頭が後続を掛けずに積むので 2 本目が main に載って落ちる。
- `hold_diff_`（lib・新しい子 module）。判断の欄の 3 字面 × 3 引用（bead の id だけ・接頭辞違いだけ・問い id の形）、外しの有無、名指しの並べ替えを確かめる。
  - 引用された header（約束 8）: `+++ "b/docs/a\"b.md"` と `+++ "b/docs/\346\227\245.md"` の file に足した解けない問い id が名指しに入り、名指しの path は escape を戻した字。閉じの引用符が無い header の file に足した行が在る周は `unmeasured:diff-path`（読み飛ばして通す実装を落とす）。
  - base で RED: 機能不在。
- `pipe_order_held_`（lib・queue.rs）。held の便が番と後続の列から外れ、released の後に戻ることを確かめる。
  - 約束 9（FR の語に依らない読み）: fixture を FR83 だけにしない。`held:FR84:x` と `held:FR99:y` の便もそれぞれ外れ、`released:FR84` と `released:FR99` の後に戻る（`held:FR83:` を字で見る実装を落とす）。
  - base で RED: 機能不在（held を外す読みが無い）。
- 候補の木の先頭の掛けは lib の歯で測らない。判定を関数で渡す純関数だけを測る歯は、`train.rs` の掛けの配線を外しても緑のまま（2026-09-30 の gate の審査で、配線を外す変異が全部の verify を通った）。配線は上の e2e の候補の木の歯が測る。
- 既存の歯を名指す: `pipe_order_`（lib 20 本・queue.rs 18 と gate/lens.rs 2）・`pipe_train_`（既存の 4 本・queue.rs）・`pipe_land_turn_`・`pipe_land_pr_cmd_`。

### 触らない
- 数えの規則と解き方（`dispatcher.md §36`）。受付の断り（`dispatcher.md §37`）。
- passed_gate と verdict の読み。FR84 の未反映（`dispatcher.md 行 am` と行 bf）は同じ留めの形（`held:FR84:`）を使う前提で、この § は FR83 だけを掛ける。
- FR50 の留め（別の行）。

### 限界
- 差分は base からの便の commit だけを見る。追随で解いた衝突の字は、次の周の判定で見る。
- 判断の欄の字面は閉じた決めごと。「裁定（要件）」のように裁定 id を持たない見出しの語も欄に数える（直し方は問い id を添えるか語を変える）。
- held の便の起こし直しは、周ごとに台帳の読みを 1 回撃つ。

### 却下
- 留めを Failed + 手の撃ち直しにする案: FR10 に反する。
- 列が event の detail を読まず、札で外す案: held の便の札は Absent なので、Dead と見分けられない。
- 判定を番待ちの後に置く案: 先頭の held が後続を塞ぐ。

## 63. 未反映の裁定に関わる便を着地の周に FR10 の留めに掛ける（契約表の行 bf・[FR10](../../design-intent/spec/srs.html#FR10) / FR84・AC54）

- 依存: 行 be（留めの口）と `dispatcher.md 行 am`（置き場の unreflected の file と、その読みの関数）。`dispatcher.md 行 am` は台帳の依存（bead の blocks）で結ぶ。
- 着地の判定は、`dispatcher.md §38` の置き場の unreflected の file の「関わる契約の表」に便の bead が在る周に、行 be の留めの口で便を Gated に留める。
  - 表の読みは `dispatcher.md 行 am` が `pub(crate)` で開く読みの関数を呼ぶ。その子の file は書かない。
    - 呼び先は挙動に差が出ないので done には載せない（便の diff の設計適合は gate の審査で見る・§62 の約束 7 と同じ扱い）。子の file を書かないことは、write-set の外の file として runner の guard が測るので done にしない（器の門）。
  - 名指しは表がその便の bead に結ぶ裁定 id で、detail は `held:FR84:<id>`。同じ名指しが続く周は記帳し直さない（行 be の約束 4・名指しが変わった周は新しい 1 件）。
  - 撃ち直しの上限に数えず、Failed にしない。
- 表から消えた後の周に、`released:FR84` を 1 件記帳して着地する。列の読みは行 be の読み（detail の頭 `held:` と `released:` を FR の語に依らず読む）のまま。
- 置き場の file が無い周は留めない（列がまだ測っていない）。在るのに読めない周だけ `held:FR84:unmeasured` で留める。file が無い周と読めない周の区別は、`dispatcher.md 行 am` の読みの関数が分けて返す（`dispatcher.md §38`）。
- released の語は、便の最後の held の FR の語に揃える。行 be の約束 5 は「当たらない周で最後の held の後に released が無ければ `released:FR83`」と書き、最後の held が `held:FR84:` の便にも `released:FR83` を書く。本行は land.rs の記帳を直し、`released:FR83` は最後の held が `held:FR83:` の便だけに書き、`released:FR84` は最後の held が `held:FR84:` の便で表から消えた周だけに書く。
  - 直さないと、ruling-check = true の repo で FR84 の留めの次の周に FR83 の判定が当たらず `released:FR83` が書かれ、便は列に戻る。表に残る同じ id の周は、直前の event が held でないので `held:FR84:` を新しく記帳し、held と released が周ごとに往復する。表から消えた周も `released:FR84` が出ない。
- 候補の木の先頭の判定（行 be の約束 6）にも FR84 を足す。先頭は後続を積む前に、FR83 の判定に加えて、置き場の表にその便の bead が在るかでも掛け、在る便を積まない（記帳はその便の自走に任せる・行 be と同じ）。判定の置き場は行 be が `crates/scribe2/src/pipe/train.rs` に足す後続の掛けの所で、train.rs は write-set に在る。
  - 足さないと、表に bead が在る後続の便は先頭の候補の木に積まれ、自分の着地の周の判定を経ずに先頭と一緒に main に載る（事前審査の束 151ecf21d5ac5fca の 2 周目の指摘）。
- 歯: 接頭辞 `pipe_land_unreflected_`（tests/e2e/pipe/land.rs）。
  - (a) 表に bead が在る便は Gated に留まり、main は動かない。留めの event の detail は `held:FR84:<表がその便の bead に結ぶ裁定 id>` と逐語で一致する（表に別の bead と別の id の組も置き、取り違える実装を落とす）。
  - (b) 上限の回数を越えて回しても Failed にならない。
  - (c) 表から消えた後の周に `released:FR84` を 1 件記帳して着地する。
  - (d) 同じ原因で 3 周撃っても `held:FR84:` は 1 件。
  - (e) 読めない file の周は `held:FR84:unmeasured` で留まり、file の無い置き場の便は着地する。
  - (g) 候補の木の先頭の周に、表に bead が在る後続の便（FR83 には当たらない）と表に無い後続の便を置くと、表に在る便は積まれず main に載らず、表に無い便は積まれる（先頭の掛けに FR84 を入れない実装を落とす）。
  - (f) ruling-check = true の宣言で FR83 の判定が当たらない便を、表に bead を置いたまま 3 周撃つと、event は `held:FR84:<id>` の 1 件だけで `released:FR83` は 0 件。表から消した周に `released:FR84` が 1 件で、`released:FR83` は 0 件のまま（最後の held の語に依らず `released:FR83` を書く実装を落とす）。
  - base で RED: 機能不在（表に在る便が main に載る）。(g) は、base の先頭の掛けが FR83 だけを見て表に在る後続を積むので落ちる。

## 64. 審査役の claude を読みの道具だけで起こす — 道具は Read・Grep・Glob、許可の問いを誰にも出さない mode、口座の自動 memory を読まない、契約の審査は審査の時点の HEAD の木の上で・先撃ちは予想の木の上で読み、予想の判定を Reviewed に写さず、prompt の「tool が渡されていない」を消す（契約表の行 bg・[ADR-0102](../../design-intent/decisions/ADR-0102-lens-reads-with-read-only-tools-and-runner-gets-the-common-verify.html) §2.1・epic `s2-07l.736.33` の打ち手 0）

やさしく言うと: 審査役（lens）は今、「道具は無い」と言われて材料の字だけで判定している。ところが実際には、編集を自動で承認する設定のまま、main の作業場所を起点に起きている。つまり審査役は code を読めるのに読まず、そのうえ書き換えもできてしまう。これを逆にする。審査役には読むだけの道具を渡し、それ以外は持たせない。契約の審査は、審査する時点の repo の写しの上で読ませる。先回りの審査（先撃ち）は、材料を組んだ予想の写しの上で読ませ、その判定を本番の審査に写さない。prompt にも正直にそう書く。

- 何が起きているか（main 633edd9a・verified）:
  - lens の起動形は `crates/scribe2/src/headless/lens.rs` の `dispatch` → `ask` → `crates/scribe2/src/headless/mod.rs` の `build`（runner と lens の唯一の構築点・[ADR-0011](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html) §2.1）。permission mode は lens の必須の flag `--permission-mode` の値をそのまま渡す。運用の lens の cmd（便の置き場の `lens.toml` に写る 1 行・§26）は `--permission-mode acceptEdits` を焼いている。この値は器の既定でなく、起動側の cmd の雛形（host の file・repo の外）から来ていて、隣の project の運転手も同じ形の cmd を渡している（orchestrator の実測 2026-09-30T12:23Z）。lens には `--allowedTools` も道具を絞る flag も渡らない（ADR-0011 §2.1 は「lens の起動形は本 ADR では変えない」と残した）。
  - 契約の審査（Reviewed の段）の lens は `crates/scribe2/src/pipe/review.rs` の `decide` が起こし、cmd の穴 `{worktree}` と wrapper の cwd に `entry.repo`（anchor の main の作業木）を置く。gate の lens（`crates/scribe2/src/pipe/gate/lens.rs`）は便の worktree を置く。
  - 先撃ちの lens（`crates/scribe2/src/pipe/dispatch/prelens.rs` の `fire`）も anchor の repo を cwd と `{worktree}` に置く。材料は予想の base の木（HEAD に祖先の層を当てた一時の worktree）で組むが、その木は lens を起こす前に外す（`build` の末尾の `drop_tree`）。Reviewed の段の使い回し（同じ file の `reusable`）の鍵は、材料の dir の digest と lens の cmd の字だけで、lens が読んだ木を含まない。読みの道具を渡すと、先撃ちの lens は材料と違う木の code を読み、祖先の着地の前の code で出した判定が着地の後の Reviewed にそのまま写りうる（Reviewed の FAIL は終端で、FR68 は同じ中身の契約を列から外す）。
  - 3 つの呼び手は lens の stderr を捨てる（`review.rs` と `gate/lens.rs` は null・先撃ちの包みは /dev/null）。
  - 契約の審査の雛形 `crates/scribe2/src/headless/lens-contract.txt` の 31 行目と gate の雛形 `crates/scribe2/src/headless/lens.txt` の 35 行目は「あなたには tool が渡されていない。shell も cargo も git も撃てない」と書く。2026-09-30 の生の log では lens 132 回のうち 126 回が 1 turn で終わった。前提を無視して code を読んだ 6 回のうち 3 回は、行番号つきで本物の欠陥を掘り出した（epic の分析の報告）。
  - 実測（claude 2.1.285・2026-09-30・空の設定 dir と無効な API key で init の record だけを読む形と、haiku の実 call 3 回・計 0.05 USD 未満）:
    - `--tools "Read,Grep,Glob"` を渡すと init の tools は Glob・Grep・Read の 3 つだけになる。Write と Bash は無い（model が「使える道具に無い」と答え、file は作られなかった）。
    - `--permission-mode dontAsk` を足し `--allowedTools` を渡さない形では、cwd の中の Read は通った。cwd の外の file の Read と Grep は「don't ask mode なので断った」で断られた（permission_denials 2 件）。`--allowedTools "Read,Grep,Glob"` を足すと cwd の外も読めてしまう。
    - 環境変数 CLAUDE_CODE_DISABLE_AUTO_MEMORY を 1 にすると、init の record から memory_paths が消えた（値 0 と未設定では口座の設定 dir の下の memory の path が載る）。`--setting-sources ""` だけでは自動 memory は切れない。
    - `--max-turns` は help に無いが受け付けられる（未知の flag は rc 1 で断られる）。
- 形:
  1. **lens の道具と mode は器が決める**: lens は渡された permission mode に依らず、permission mode dontAsk を毎回明示し、`build` に道具の列 Read,Grep,Glob を渡す。`build` はその列を `--tools` の値として渡す（道具の列を持つのは lens だけ・runner と `fleet usage` の refresh は渡さず argv は不変）。`--allowedTools` は lens に渡さない（渡すと cwd の外も読める・実測）。
     - `--permission-mode` は任意の flag に下げ、受けても値を使わない。消費側ごとの cmd の雛形は host の file で、断ると全部の消費側の審査が雛形を手で直すまで止まる。
     - 渡された値が dontAsk でない周（acceptEdits と plan を含む）は、`--contract` の file と同じ dir に 1 語の記録の file `lens.ignored` を置く。字は `ignored:` に渡された値を続けた 1 行。dontAsk の周と flag の無い周は置かず、前の周の file が在れば消す。どの雛形が古いかは、便の記録の dir に残る 1 語で分かる。
     - stderr には書かない。3 つの呼び手が lens の stderr を捨てるので、書いても誰も読めない。
     - usage の 1 行と help の頁（`crates/scribe2/src/help.rs` の lens の form）は `[--permission-mode M]` の任意の形にする。help の FORM と生きた usage の一致は、既存の歯 `cli_help_pages_match_the_live_form_and_every_subcommand` が gate の共通 verify の中で測る。help の flags の説明は英語（help の頁は ASCII・幅 100 字）で「受けるが使わない・審査役は読みだけで起きる」の旨にし、examples から `--permission-mode M` を外す。説明と examples の字は歯で測れない（help の頁の本文の snapshot は無い）ので、done には載せない（便の diff の設計適合は gate の審査で見る・行 al・be と同じ扱い）。
  2. **器が起こす claude は口座の自動 memory を読まない**: `build` が子の env に CLAUDE_CODE_DISABLE_AUTO_MEMORY=1 を毎回設定し、親の値を継承させない（agent view の env と同じ置き方・`AGENT_VIEW_ENV` の隣・器は子へ設定するだけで読まない）。runner・lens・`fleet usage` の refresh の全部に効く（構築点は 1 つ）。
  3. **契約の審査の cwd は審査の木**:
     - `review` は材料を組む前に、対象 repo の HEAD の sha を 1 回読む（受付の base の木と同じく、材料の前に読む）。
     - 先撃ちの判定を使い回す周（形 5 の鍵）は木を作らない。鍵はこの sha で決まる。
     - 使い回さない周は、run dir の直下の `<sha>.tree` に、その sha に detach した worktree（審査の木）を作る。材料の dir（`review/`）の中には置かない。材料の鍵の digest は dir の全 file を読むので、中に置くと鍵が壊れる。
     - 作り方と片付けは `crates/scribe2/src/pipe/dispatch/floor.rs` の `Worktree` の `make` と Drop を借りる（その 2 つを crate::pipe の中に見せる・一時の木の 7 つ目を書かない）。作る前に、同じ path に前の周の木（`.git` の file を持つ登録済みの worktree・lens の途中で driver が死んだ便の残り）が在れば `worktree remove --force` と `prune` で外す。判定を読んだ後に、木を登録ごと外す。
     - lens の cmd の穴 `{worktree}` と wrapper の cwd をその木にする。
     - worktree でない物が path を塞ぐ周と、git が作るのを断る周は、lens を撃たず INCONCLUSIVE にする。evidence は審査の木を作れない理由の 1 行で、anchor の作業木へは倒さない。
     - `review.json` に key `tree`（木の sha）を足す。事後に、lens が読んだ木を辿れる（NFR4）。
  4. **雛形は道具を正直に書く**: `lens-contract.txt` の「審査の前提」の本文を次の 4 つにする。
     - (i) あなたには読みの道具（Read・Grep・Glob）だけが在る。cwd は審査の木（この契約を当てる前の repo の写し。未着地の祖先を持つ先撃ちでは、祖先の変更を当てた予想の写し）で、その外の file は読めない。shell・cargo・git・書きの道具は無い（試さなくてよい）。
     - (ii) 材料に無い code の事実（関数の呼び手・struct を組み立てる全ての場所・match の全ての場所・名や字を含む既存の歯・可視性の連鎖・file の置き場）は、判定の前に木の現物で確かめる。
     - (iii) 穴を 1 つ見つけても止めず、材料と木を見終えてから判定し、`at` に見つけた場所を全部並べる。
     - (iv) 設計の節や要件が「（…を読めない）」「（…に無い）」の形で欠けている周は、木で補えるものを補い、補えない欠けを evidence に書いて INCONCLUSIVE を選ぶ。
     - 「読むだけで決める」の行は消す。
     - `lens.txt` の「審査の前提」も同じ形にする。cwd は便の worktree（diff を当てた後の木）で、diff の外の事実（呼び手・既存の歯・憲法の生成 file）は木の現物で確かめる。消すのは「tool が渡されていない」の行と「読むだけで決める」の行。「審査の材料は…だけである」の行は「審査の対象は契約と diff で、木は事実を確かめるために読む」にする。
     - `lens.txt` で字を変えないもの（既存の歯が逐語で測る）: 見出し `## 審査の前提（検証は済んでいる）`・見出し `## 審査の材料（契約と diff だけ）`・「契約に名指しされていない検査（整形・rustfmt・lint の既定 等）を根拠に INCONCLUSIVE / FAIL を出さない。」と「判定に届かない周は、evidence に「契約のどの行を撃てなかったか」を書く。」の 2 文・句「「verify を自分で撃てなかった」」を持つ行。前提の節 → 材料の節 → 契約の順も変えない。
     - 契約に名指しされていない検査を根拠にしない行と、`findings` と `population`（diff の母集団）の定めは変えない。
  5. **先撃ちの lens も木の上で読み、予想の判定は Reviewed に写さない**（PR #915 の審査の H1）:
     - `build` は材料を組んだ予想の木を外さずに残す。置き場の file `tree.sha` に、木の sha と種類の 1 行を書く。種類は、祖先の層が無い予想なら `actual`、層を当てた予想なら `forecast`。
     - `fire` は `{worktree}` と包みの cwd をその木にする。撃つ周に木が無い行（前の周の頭で外した行）は、撃つ前に同じ周の予想で組み直す。
     - 周の頭の `prune` は、撃ち中の置き場（`in_flight`）の木を外さない。ほかの木（lens が終わった行・撃たなかった行・母集団を出た行・落ちた周の残り）は今どおり外す。lens が終わった木は、次の周の頭で外れる。
     - Reviewed の段の使い回し（`reusable`）の鍵に木を足す。置き場の `tree.sha` が `<審査の木の sha> actual` と一致する周だけ使い回す。`forecast` の判定・sha の違う判定・`tree.sha` の無い置き場（この便の前に撃った判定）は写さず、Reviewed で lens を撃つ。
     - 置き場の判定（`out`）と `tree.sha` は、判定を撃った木を指し続ける。組み直し（`rebuild`）が材料の digest の同じ判定を残す周は、`tree.sha` も書き直さない。木を組み直して `tree.sha` を今の HEAD で書く周は、残した判定を捨てる（判定と `tree.sha` が別の木を指す置き場を作らない）。
       - 便 s2-07l.736.33.1-20260930T142727Z の gate の審査が名指した穴: `build` が組み直しのたびに `tree.sha` を今の HEAD で書き直し、`rebuild` は digest が撃った時と同じなら判定を残すので、HEAD=X の木で出した判定が main の Y への移動の後に `<Y> actual` と貼り替わり、使い回しの鍵を通って Reviewed に写る。歯が `tree.sha` を手で書くと、この経路は測れない（歯 (l) は書き手の経路だけで置き場を作る）。
     - 依存を待つ行の予想は、いつも祖先の層を持つ。そのため先撃ちが退役する（審査の門の設計・ADR-0103）までの間、使い回しは実際には起きない。予想の判定を写さない形は、その設計の「予想の記録は使い回さない」と同じ。
     - 事前審査の結果への写し（`carry`）と、材料の組み手は変えない。
- 触らない: runner の起動形（permission mode は runner の flag のまま・`--allowedTools` の allowlist・`--plugin-dir`）・gate の lens の cwd（便の worktree のまま）・審査の材料の組み手（`materials` は anchor の作業木を読むまま）・事前審査の結果への先撃ちの写し・lens の cap と model と effort の行・判定の読み（`read_outcome`・done の対応の表）・lens の cmd の穴の数（`{contract}` と `{worktree}` の 2 つ）。
- 却下（[ADR-0102](../../design-intent/decisions/ADR-0102-lens-reads-with-read-only-tools-and-runner-gets-the-common-verify.html) §4）:
  - **`--allowedTools` で Read と読みだけの Bash（git grep・git show）を許す**（棚卸しの案）: 許可の規則が cwd の外まで読みを開く（実測）。Bash は語の allowlist の外を承認の問いへ倒し、無人の lens では止まる。
  - **`--restricted` を渡す**: lens は書きの道具を持たないので、restricted が settings と git の書きを人の承認へ倒しても lens は止まらない。読みを cwd に閉じる保証を文書で持つ利点もある。それでも採らないのは、ADR-0011 §2.1 が「build は `--restricted` を渡さない（MUST NOT）」と決めていて（理由は runner の Bash と書きの承認）、lens のためにそれを部分 supersede するほどの差が無いからである（dontAsk と道具の列でも、実測で cwd の外の読みは断られた）。claude の版で効きが崩れたら、そのときに比べ直す。
  - **今の 1 turn・材料だけのまま、材料の組み手を広げる**: 材料の cap（`gate.token_cap`）の中に逆引きの全部は収まらない（外の材料は cap で半分以上落ちる・分析の実測）。読んだ 6 回の半分が欠陥を掘り出した事実に反する。
  - **lens の cmd の `--permission-mode` を plan に書き換えるだけ**: 起動形の値が運用の cmd の手書きに残り、器の外で緩められる（`--cap` を外した理由と同じ）。雛形は消費側ごとの host の外の file で、散文で直させる形は効かない。
  - **`--permission-mode` を未知の引数として断る**（`--cap` を外した周の形）: 雛形を直すまで全部の消費側の審査が INCONCLUSIVE で止まる。値を使わずに受け、古い雛形を 1 語の記録で名指すほうが、止めずに同じ安全を得る。
  - **`--bare` で自動 memory と hook を切る**: OAuth を読まないので subscription の口座で起きない。
  - **先撃ちの使い回しを止めるだけ**（審査の H1 の案 (b)）: 使い回しは止まるが、先撃ちの lens が anchor の作業木を読み、材料の木と読む木が食い違う形は残る。
- 限界:
  - turn の上限は渡さない。上限の値は rules 行の裁定が要り（C5）、SRS の追加 round で裁定を取った（user 2026-09-30T22:13Z 項 lens-turns・値 30・行 `lens.max_turns` を足すのは §67 の行 bi・読んだ lens の turn は実測で 2〜10）。
    - 行 bg は上限の行より先に着地する。その間は、読む lens の消費に天井が無い。便の token の検出線（R-C6-1）は判定行を出すだけで止めない。間の期間は SRS の round と上限の行の着地までで、その間の lens の消費は pipe show の判定行で見る。
  - 道具の名と env の名は claude の版に従う。器の歯は fake の argv と env しか測らないので、実 binary の効き（init の tools と memory_paths）は着地の後に orchestrator が init の record で測り、台帳の notes に残す（ADR-0011 の測り方と同じ）。
  - 自動 memory のほかに、口座で変わる入力が残るかは測っていない（例: 口座の設定 dir の CLAUDE.md が `--setting-sources ""` の下で読まれるか）。init の record はその読みを載せないので、課金の要る 1 回の実測で着地の後に測る。ADR-0102 の「口座に依らず同じ材料」は、自動 memory の分についての主張である。
  - 審査の木は HEAD の commit で、材料は anchor の作業木から読む。anchor に commit していない編集が在る周は、木と材料が違いうる。
  - 1 語の記録は契約の file の dir に置くので、契約の審査では材料の dir（`review/`）に入る。材料の鍵の digest は lens を撃つ前に取るので、鍵には入らない。撃ち直しの審査で前の周の file が残っていれば鍵が外れ、使い回さない側に倒れる。
- 歯（接頭辞 `lens_read_`・crates と docs で 0 件・2026-09-30）:
  - e2e（`crates/scribe2-boundary/tests/e2e/headless/lens.rs`）: 偽 claude が argv と env を file に写す形で、
    - (a) lens の argv に `--tools` の直後の値 Read,Grep,Glob の対がちょうど 1 つ・`--permission-mode` の直後が dontAsk・`--allowedTools` と acceptEdits が 0 件。
    - (b) `--permission-mode acceptEdits` と `--permission-mode plan` を渡した lens の argv の permission mode が dontAsk で、`--contract` の file の dir の `lens.ignored` の字がそれぞれ `ignored:acceptEdits` と `ignored:plan`。`--permission-mode dontAsk` を渡した周と flag の無い周は、偽 claude が呼ばれ（印の file が在る）lens が rc 0 で判定を返し、`lens.ignored` が無い（前の周の file を置いた dir でも消える）。
    - (c) 契約の審査の雛形と gate の雛形で組んだ prompt に「読みの道具（Read・Grep・Glob）」が在り、「tool が渡されていない」が 0 件。
  - e2e（`crates/scribe2-boundary/tests/e2e/headless.rs`）:
    - (d) runner と lens の子の env の自動 memory の値が 1 で、親に別の値を置いても継承されない（agent view の歯と同じ形・fixture の親の値は入力の字と別の字面）。
    - (e) runner の argv に `--tools` が 0 件で、`--allowedTools` と渡した permission mode は今のまま。
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`）:
    - (f) 契約の審査の lens の `{worktree}` が repo と違う path で、run dir の直下の `<sha>.tree`。lens の中で読んだ `git rev-parse HEAD` が審査の前の repo の HEAD と同じで、wrapper の cwd も同じ path、`review.json` の `tree` がその sha。審査の後にその dir は無く、`git worktree list` にも無い。材料の dir の名の列は今のまま。
    - (g) 木の path に file を置いた周は、Reviewed が INCONCLUSIVE で evidence が審査の木を名指し、偽の lens の印が無い。
    - (g2) 同じ path に前の周の登録済みの worktree を残した周は、それを外して審査が進み、偽の lens の印が在る。
    - (g3) 木の path に file を置いたうえで、使い回せる先撃ち（置き場の `tree.sha` を `<HEAD> actual` にした置き場）を置いた周は、INCONCLUSIVE でなく使い回しの判定になる（detail が ` prelens:reused` で終わる）。
    - (i) 先撃ちの偽 lens が写した cwd と `{worktree}` が置き場の `tree` で、そこに祖先の層の file が在る（宣言だけの祖先の `+` の file が空で在る）。
    - (j) 撃ち中の偽 lens（印の file が出るまで待つ）の間に撃った次の周の後も、置き場の `tree` が在る。lens を終わらせた後の周の後は、置き場にも `git worktree list` にも無い。
    - (l) 祖先の層の無い行の先撃ちの 1 周で置き場に判定を撃たせた後に、repo の main へ code の commit を 1 つ足し、次の周（組み直し）を経た置き場のまま契約の審査を撃つと、偽 lens が撃たれる（回数 2）。置き場の `tree.sha` は `build`・`rebuild`・`fire` の経路だけで作り、手で書かない。
    - (k) Gated の祖先を着地させた後の審査は、置き場の `tree.sha` が `forecast` の周に偽 lens を撃つ（回数 2・語が無い）。`<着地後の HEAD> actual` に書き換えた周は撃たずに写し（回数 1）、`<別の sha> actual` に書き換えた周は撃つ（回数 2）。
  - in-file（`crates/scribe2/src/headless/mod.rs` の `mod tests`）: (h) 道具の列を持つ call の argv に `--tools` と値の対がちょうど 1 つ、持たない call には `--tools` が 0 件。
  - 直す既存の歯（同じ便・本文を直す歯には retroactive の札を付ける。札の欠けは gate の flip-check が赤で落とすので、done には載せない・器の門が測る）:
    - lens が渡された permission mode をそのまま渡すと測る歯（`crates/scribe2-boundary/tests/e2e/headless/lens.rs` の acceptEdits と plan の対を測る 2 か所）は、dontAsk の定数を測る形に替える。fixture の helper（`crates/scribe2-boundary/tests/e2e/headless.rs` の `run_lens` と `lens_args`）は permission mode を渡したまま使える（値を使わない）ので、呼び手は直さなくてよい。
    - in-file の `invocation_wrap_headless_build_names_the_program_and_flags`（`crates/scribe2/src/headless/mod.rs`）は子の env の列を等号で測るので、列に自動 memory の env を足す。
    - `pipe_review_pass_spawns` の `{worktree}` の期待を審査の木に替える。
    - 先撃ちの 2 本 `pipe_prelens_declared_ancestor_places_empty_files_in_a_temporary_tree` と `pipe_prelens_leftover_tree_and_registration_are_removed_at_the_round_head` は、木が残らないことを、lens が終わった後の次の周の後に測る。
    - 使い回しの 2 本 `pipe_review_reuse_same_material_and_lens_copies_the_verdict_without_firing` と `pipe_review_done_items_reused_prelens_verdict_goes_through_the_same_fall` は、審査の前に置き場の `tree.sha` を `<着地後の HEAD> actual` に書き換える（層の無い予想の置き場を模す）。
    - 外形の snapshot 4 本（lens の usage を写す `headless_external_form`・`lens_prompt_external_form`・`lens_contract_prompt_external_form`・`lens_promise_prompt_external_form`）を更新する。
    - help の頁と生きた usage を突き合わせる歯 `cli_help_pages_match_the_live_form_and_every_subcommand` は緑のまま。
- base で RED の理由:
  - (a)〜(c) と (h) は、base の lens が permission mode を素通しし、記録の file を置かず、道具の列を渡さず、雛形が「tool が渡されていない」を持つので落ちる。(b) の flag の無い周は、base が `--permission-mode` を必須にして rc 1 で断るので落ちる。
  - (d) は、base の `build` が自動 memory の env を設定しないので落ちる。
  - (f)・(g)・(g2) は、base の `{worktree}` が repo なので落ちる（機能不在）。
  - (i)・(j) は、base の先撃ちが repo を cwd にし、木を撃つ前に外すので落ちる。(k) は、base の使い回しが `tree.sha` を読まず `forecast` の周も写すので落ちる。(l) は、base の使い回しが `tree.sha` を読まず、材料の digest の同じ判定を写すので落ちる。
  - (e) と (g3) は回帰の歯で、同じ file に base で赤い歯（(d) と (f)）を持つ。
- 着地の後: PATH の binary を `swap-binary.sh` で入れ替える。
  - 入れ替えの後の審査は、雛形が `--permission-mode acceptEdits` を持ったままでも読みだけで起き、便の記録の dir に 1 語の記録が残る。admin の道具が焼く雛形からは値を外してよい（外さなくても審査は止まらない）。
  - 消費側の席へ、審査役が読みだけで起きることと、1 語の記録の file `lens.ignored` を 1 行で知らせる。
  - 入れ替えの後に 1 回、実 binary の init の record（無効な API key の形）で tools と permissionMode と memory_paths の不在を測り、台帳の notes に残す。口座の設定 dir の CLAUDE.md の読み（限界の 3 項目め）も 1 回測る。

## 65. runner の stdin に共通 verify の行を渡す — 契約の後ろに「## 共通 verify」節を足し、便の写しの common-verify を 4 つの穴を gate と同じ埋め方で埋めた字で並べ、雛形は commit の後に全部撃って緑にしてから終えると読む（契約表の行 bh・[ADR-0102](../../design-intent/decisions/ADR-0102-lens-reads-with-read-only-tools-and-runner-gets-the-common-verify.html) §2.2・epic `s2-07l.736.33` の打ち手 2 の 1 段目）

やさしく言うと: 実装役（runner）は「契約の検証の行が緑になるまで直せ」とだけ言われていて、gate が必ず撃つ共通の検査（入口の RED・全部の test・lint・器の規則の検査・依存の監査）を知らない。そのせいで、直せば済む赤を gate で初めて見つけて便ごと捨てている。gate が撃つ行をそのまま渡し、終える前に自分で撃たせる。

- 何が起きているか（main 633edd9a・verified）:
  - runner の雛形 `crates/scribe2/src/headless/runner.txt` の 6 行目は「契約の verify 行が緑になるまで直す」だけを言う。7 行目は背景実行を残して終えることを禁じる。stdin の本文は `crates/scribe2/src/pipe/spawn.rs` の `prompt` が組み、順は 契約 → 回答 → 途中再開 → 追随で、共通 verify は載らない。
  - gate は便の写しの `vessel.toml` の common-verify（本 repo では flip-check・workspace の nextest・clippy・xtask check・deny の 5 行）を契約の verify の前に撃つ（`crates/scribe2/src/pipe/gate/record.rs` の `record_verify`）。
  - 共通 verify に置ける穴は 4 つ（`{base}`・`{jobs}`・`{threads}`・`{teeth}`・`crates/scribe2/src/pipe/declaration.rs` の `BASE_HOLES`）。gate は 4 つとも埋めて撃つ（`crates/scribe2/src/pipe/gate/verify.rs` の `fill_holes`・`{teeth}` は同じ file の `teeth_of`・受付を通らない周の `{jobs}` と `{threads}` は 1）。
  - 分析（2026-09-28〜09-30・gate の FAIL 30 件）: 15 件が共通 verify の赤で、中身は xtask check の規則 7・現物の契約表の歯 5・既存の歯の pin 3（重複あり）。runner 28 本のうち xtask check を撃ったのは 5 本、workspace の nextest は 8 本。
  - 費用（gate の common の record 570 件の実測）: 行ごとの中央値は flip-check 93 秒・nextest 35 秒・clippy 5 秒・xtask check 4 秒・deny 0 秒（p90 は 295・133・14・7・0 秒）。
- 形:
  1. **節を足す**: `prompt` は契約の本文の直後（回答の節の前）に「## 共通 verify」節を足す。
     - 本文は便の写しの common-verify の各行で、4 つの穴を gate と同じ埋め方で埋めた字を 1 行 1 項目で並べる。`{base}` は便の base の sha、`{jobs}` と `{threads}` は 1（runner が撃つ行は受付を通らない・gate の受付を通らない周の値と同じ）、`{teeth}` は契約の verify 行の filter 語（gate と同じ導出）。
     - 埋め方は `gate/verify.rs` の `fill_holes` と `teeth_of` を借りる（crate::pipe の中に見せる・2 本目を書かない）。
       - `gate.rs` の `mod verify;` は私有なので、2 つの fn を `pub(crate)` にし、`gate.rs` の既存の再輸出の列（`pub(crate) use verify::{…}`）に 2 つの名を足す。spawn.rs は `crate::pipe::gate` から呼ぶ。`gate.rs` は write-set に在る（便 s2-07l.736.33.2-20260930T151824Z の契約の審査が、私有の mod に path が届かず `gate.rs` が write-set に無いと名指した）。
     - 写しの読み手は gate と同じ `Effective::load` の 1 本。この読み手の分け方は挙動に差が出ないので、done には載せない（便の diff の設計適合は gate の審査で見る・行 al・be と同じ扱い）。
     - 読めない周は、節の本文を「（共通 verify の写しを読めない: 理由）」の 1 行にし、runner は止めない。行が 0 本の写しは「なし」。
  2. **雛形の 1 項目**: `runner.txt` の「守ること」に、次の旨の項目を足す。
     - 契約の末尾の「## 共通 verify」節の行は、gate が契約の verify 行の前に撃つ行である。
     - commit を作った後に、節の行を 1 本ずつ全部撃ち、全部緑にしてから turn を閉じる。節の行には commit を読む行があるので、commit の前に撃っても測れない。
     - 長い行は、Bash の timeout を上限 600000 ms まで明示して撃つ。
     - 緑にできない行が残るときは、何が赤かを最後に 1 行で述べる。
- 触らない: gate の撃つ行と判定・契約の verify の行・runner の allowlist（共通 verify の行の頭の語は allowlist の中）・回答と途中再開と追随の節の字と順・runner の起動形。
- 却下: runner.txt に common-verify の行を直に書く（写しは repo ごとの宣言で、雛形は器の全 repo に共通）／runner に `--vessel` の写しを読ませて自分で組ませる（穴の値を runner が知らない）／節を置かず終わりの門（§66）だけにする（門は SRS の追加 round を待つ。節は今の FR の中で効き、門が入った後も runner が 1 turn の中で直す材料になる）。
- 限界:
  - 撃ったかは器が測らない（撃たずに終えた runner は今と同じく gate で落ちる）。撃つ時間（中央値の和で約 137 秒）が runner の turn に足される。器が撃って赤を戻すのは §66。
  - **Bash の timeout**: runner が撃つ行は Claude Code の Bash の道具の上で走る。既定の timeout は 120 秒で、指定できる上限は 600 秒。flip-check の p90（295 秒）と nextest の p90（133 秒）は既定を越え、上限の 600 秒を越える行も在りうる。runner.txt は背景実行を禁じるので、切られた行は赤と読まれるか、撃ち直しが重なる。雛形で上限までの指定を言う（形 2）。
    - 論点（推奨つき）: 上限を env（Claude Code の Bash の timeout の上限の変数）で延ばすか。推奨は延ばさない。600 秒を越える行は §66 の終わりの門が器の側で撃って測る（門は Bash の道具を通らない）。延ばすと runner の turn が 1 行で 10 分を越えて止まって見え、env の名も claude の版に依る。
  - **受付と箱の外**: gate の行は受付・遮断器・行ごとの箱を通る（並列度を受け取らない行が host の core を全部取りにいく形を塞ぐ・`gate/verify.rs` の `admitted`）。runner が撃つ行はどれも通らず、runner 自身と同じ 1 × `gate.job_memory_mb`（3072 MB）の箱の中で走る。同時に走る便は `pipe.max_live`（16）まで。
    - 箱の中の OOM は runner の終端の Failed になる（`spawn.rs` の `box_killed`）。gate の OOM は INCONCLUSIVE（測り直せる）なので、扱いが違う。host の CPU の過剰な割り当ても起きうる（推測・測っていない）。
    - `{jobs}` と `{threads}` を 1 で埋めるのは、この外れを並列度の側で抑えるため。受付を通る撃ち方は §66 の門が持つ。
- 歯（接頭辞 `runner_common_section_`・crates と docs で 0 件・2026-09-30）: e2e（`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`）で runner の stdin を写す偽 runner を使う。
  - (a) 写しの common-verify に `{base}` を含む行・`{jobs}` と `{teeth}` を含む行・穴の無い行を置き、回答済みの質問を持つ便の 2 turn 目の stdin で、節が契約の本文の後・回答の節の前に在る（`find` の順・既存の順の歯 `crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs` と `crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` と同じ測り方）。各行の穴が、記録した base と同じ sha・1・契約の verify 行の filter 語で埋まり、`{` が残らない。
  - (b) 写しを読めなくした便の節の本文が読めない理由の 1 行で、段は Implemented まで進む。
  - (c) common-verify が 0 行の写しの便の節の本文が「なし」の 1 行。
  - 外形の snapshot `headless_runner_prompt_external_form` を更新する。
- base で RED の理由: base の `prompt` は節を足さないので (a)〜(c) が落ちる（機能不在）。snapshot は雛形の項目が増えるので base で落ちる。

## 66. runner の終わりに器が共通 verify と契約の検証行を撃つ — 赤なら同じ worktree で runner を起こし直して赤を渡し、上限の周まで直させてから Implemented にする（終わりの門・[ADR-0102](../../design-intent/decisions/ADR-0102-lens-reads-with-read-only-tools-and-runner-gets-the-common-verify.html) §2.3・epic `s2-07l.736.33` の打ち手 2 の 2 段目・契約表の行 bj・bk）

やさしく言うと: §65 は検査の行を runner に渡すだけで、撃ったかどうかは分からない。そこで runner が終わった直後に、器自身が gate と同じ行を撃つ。赤ければ、同じ作業場所で runner をもう一度起こし、どの行がどう赤かを渡して直させる。決めた回数を使い切るか、器が測れなかったときは、今と同じく gate へ進む。

- 何が起きているか（main 633edd9a・verified）:
  - runner の終わりの段は `crates/scribe2/src/pipe/spawn.rs` の `settle` が決める: rc 0 かつ base から先の commit が 1 本以上なら Implemented、それ以外は Failed（[FR6](../../design-intent/spec/srs.html#FR6)）。`settle` の前に、runner の pid の `SeatStopped` を記帳済み。
  - `spawn` の呼び手は `crates/scribe2/src/pipe/follow.rs` の `spawn_turn` の 1 本だけ（起動口は spawn の 1 本・C6）。`spawn_turn` は毎回 `Precheck::measure` で Budget を作り、回答・追随・途中再開の材料を置き場から読んで `Launch` を組む。
  - `spawn.rs` の `prepare_worktree` は、`Launch` の回答・追随・途中再開のどれかが在る周だけ、同じ worktree と記録済みの base を使う。ほかの周は HEAD から worktree を切る。
  - 手の `pipe resume`（`crates/scribe2/src/pipe/cli/resume.rs`）の Spawned の枝は、最後の `SeatSpawned` の pid の生死だけで「隣にもう 1 つ起こさない」を判じる。
  - gate の verify の撃ち手は `crates/scribe2/src/pipe/gate/verify.rs` の `run_checks_admitted` の 1 本（gate と着地の main の実測が共有）で、行ごとの受付・箱・遮断器を通り、record は `verify.jsonl`、赤い行の診断は stderr の診断 file に残る。
  - Gated の FAIL は終端で、器が理由を分けて自動で戻すことは §49 が却下している（「契約の赤を無限に撃ち直す形が作れる」）。
- 測り直し（main a526f825・2026-10-01・行 bj・bk の起票の前・verified）:
  - `spawn.rs` の `Launch` を組むのは `spawn_turn` の 1 か所（`Launch {` の grep の 6 か所のうち、残る 5 か所は別の型）。
  - `follow.rs` の `Runner`（`--runner` と対で運ぶ材料）を組むのは 5 か所（`cli/run.rs`・`cli/step.rs`・`ratelimit.rs` と、`follow.rs` の in-file の歯の 2 か所）。`Turn` を組むのは 4 か所、`Land` は 4 か所（ほかに struct 更新の形が 1 か所）。
  - gate の verify の撃ちと記録は `gate/record.rs` の `record_verify` の 1 本で、`Gate` の材料（便 id・置き場・契約・線・lock の待ち方）から便の写しの共通 verify を読み、受付の材料と `Checks` を組んで `run_checks_admitted` を撃ち、`records_of` と `diagnose` で `verify.jsonl` と `verify.stderr.log` に書き、赤と測れなかった行を数える。着地の主実測も `records_of` と `diagnose` を同じ形で使い、別の file 名（`verify-main.jsonl`）に書く。
  - verify の行の scope の unit 名は pid と process の中の通し番号を持つ（同じ引数でも別の名）。門の行と gate の行の名は衝突しない。
  - e2e の spawn の経路の多く（helper `implemented` と `resume`）は `--rules` を渡さず、埋め込みの manifest を読む。着地の後は、それらの便も門を撃つ。
- 形（番号は行 bj・bk の done と 1:1）:
  1. **撃つ時**: runner の turn が rc 0 かつ commit 1 本以上で終わった周（`settle` の Implemented の枝）だけ撃つ。停止・上限・質問・API に届かない周・Failed の周は今の枝が勝ち、門を撃たない。門の線が読めない周（形 8）と rules 行の値が 0 の周も撃たずに Implemented にする（要約の語はそれぞれ unmeasured と off）。
  2. **撃つ行と撃ち手**: gate の verify と同じ列（gate の write-set を照らす段 → 共通 verify → 契約の検証行・gate の `gate_checks` の 3 段）を、同じ撃ち手 `run_checks_admitted` で、同じ受付・箱・遮断器を通して便の worktree に撃つ。
     - `record_verify` の本体を 1 本に割る。本体が受けるのは、置き場・便 id・契約・gate の線・lock の待ち方と、record と診断の file の名の対。本体がするのは、便の写しの共通 verify の読み・受付の材料と `Checks` の組み・撃ち・`records_of` と `diagnose` での記録・赤と測れなかった行の数え。gate と門はその 1 本を呼ぶ（撃ち手も数え方も 2 本にしない）。gate が書く file の名と record の字は変えない。
     - 門の record は run dir の `end-gate.jsonl` に書く。1 行の形は `verify.jsonl` と同じで、周ごとに `n` は 1 から振る。赤い行の診断は `end-gate.stderr.log` に書く。
     - 周の終わりに、要約の 1 行 `{"end_gate":<周>,"result":"<語>"}` を `end-gate.jsonl` に足す。語は閉じた 5 つ（green・red・exhausted・unmeasured・off）で、unmeasured と off は key `reason` に理由を足す。撃たない周（形 1）は要約の 1 行だけを書く。
  3. **全行 rc 0**: Implemented にする（要約の語 green）。
  4. **赤が在り、測れなかった行が無く、門の赤の数が rules 行 `runner.end_gate_rounds` の値に届いていない**: 段は Spawned のまま、門の赤の `RunStage` を 1 件記帳する（要約の語 red）。
     - stage は `Spawned`、detail は `end-gate:red:<周>:<赤い行の数>`。周は記帳済みの門の赤の数 + 1。
     - 起こし直しは形 7 の輪が撃つ。
  5. **門の赤の数が値に届いた周**（要約の語 exhausted）と、**測れなかった行が在る周**（要約の語 unmeasured）は、Implemented にして gate へ進む。gate は今どおり全行を撃つ（門の結果を持ち越さない）。
     - 測れなかった行は、gate の `machine_order` の 3 つと同じ（gate の write-set を照らす段が読めない・箱の中で死んだ・遮断器が閉じて撃たなかった）。赤い行が在っても、測れなかった行が在る周は起こし直さない。
     - **Implemented の detail はどの周も今の字のまま（無い）**。門の結果は run dir の要約の行が持つ（撃ったか・何だったかは記録に残る・C10）。段の記帳の列を読む既存の歯と読み手の字を動かさない。
  6. **周の数え方**: 便の event を畳み、stage が `Spawned` でない最新の段の記帳より後ろの、detail が `end-gate:red:` で始まる `RunStage` の件数を数える（process の記憶に持たない・FR3）。rules 行の値は起こし直しの回数の上限で、値 2 なら runner の turn は最大 3 回。
  7. **輪の持ち主**: 門は `spawn.rs` の `settle` の Implemented の枝に置く（spawn の内側・起動口は 1 本のまま）。起こし直しの輪は `follow.rs` の `spawn_turn` が持つ。
     - `spawn` が rc 0 で返り、便の最新の `RunStage` が門の赤なら（event log から読む・spawn の戻りの字は読まない）、`Precheck::measure` で Budget を測り直し、`Launch` に門の赤の節を足して `spawn` を呼び直す。口座は同じ turn の値のまま。
     - 輪は回数を数えない（止めるのは形 4 と形 5 の数え）。追随の後始末（`follow.rs` の `settle`）は、輪を抜けた後に 1 回だけ撃つ。
  8. **門の線と Launch**:
     - 門の線は、gate の線と周の上限の値の対で、読めない周は理由の 1 行を持つ（閉じた 2 値の型を `spawn.rs` に新しく置く）。manifest から組む 1 本は、gate と同じ `Limits::of`（8 行）と、rules 行 `runner.end_gate_rounds` の `int_row` を読む。
     - 線は `Runner` に参照の field で載せる。`Runner` を組む 5 か所が、組む周の manifest から同じ 1 本で組む。`Turn` と `Land` には field を足さない（組む場所が多く、門が要るのは `--runner` の在る周だけ）。
     - `Launch` に 2 つの field を足す: 門の線（`spawn_turn` が `Runner` から写す）と、門の赤の節（前の周の赤い行の列・任意）。
     - stdin は、§65 の共通 verify の節の後・回答の節の前に「## 門の赤」節を足す。1 行目は周の番号と「器が撃った次の行が赤い。直して commit してから終える」の 1 文。続けて赤い行ごとに、撃った行の字・rc・診断の抜粋（撃った段の stderr の写しの末尾）を並べる。
     - 抜粋は行ごとに 40 行・合計で 16000 字まで。落とした行は数の 1 行を残す。これは窓の大きさで判定の閾値ではないので、rules 行にしない（`STDERR_TAIL_LINES` と同じ読み）。
     - `prepare_worktree` は、回答・追随・途中再開・門の赤のどれかが在る周に、同じ worktree と記録済みの base を使う。
     - 起こし直しの `Spawned` の detail は `end-gate:<周>` で、器が選んだ口座の在る周は `,account:<label>` を足す。`base:` で始めない（途中再開と同じく、base の読み手が飛ばす形）。
  9. **門の間の印と停止**（行 bk）: 門を撃つ間、runner の pid は死んでいて、段は Spawned のまま。
     - 門を撃ち始める process は、run dir に印 `end-gate.pid`（`<pid> <起動時刻>`・driver の札と同じ原子的な置き方）を置く。自分の pid の印が既に在る周（前の周の門）はそのまま使う。
     - 印を外すのは輪を抜ける時で、先頭の語が自分の pid の印だけを外す（driver の札の `Drop` と同じ形）。周の間（門の赤の記帳から次の `SeatSpawned` まで）も印は残る。
     - `pipe resume` の Spawned の枝は、API に届かない周の枝より前に印を `lock_owner` で読む。持ち主が死んでいない印（生きている・読めない）の周は断る（判定行 `run=<id> end-gate=alive pid=<pid>`・rc 1・event 0 件）。印の無い周と、持ち主の死んだ印の周は今のまま。driver の札は自動の経路だけを守るので、手の resume にはこの印が要る。
     - 門の後と起こし直しの前に停止の印（`is_stopping`）を読む。停止中なら段を書かず、起こし直さない（`stopped_underneath` と同じ・終端は `RunStopped` の経路が書く）。
  10. **rules 行**: `runner.end_gate_rounds`（kind `RunnerEndGateRounds`・Int・値 2・発効・ruling `user 2026-09-30T22:13Z 項 end-gate`・ruled_at `2026-09-30`）を足す。
     - 値 0 は門を撃たない。撃っても赤を渡す周が無く、gate が全行を撃つので、撃つ意味が無い。
     - kind の宣言順は `FollowRetries` の直後に置く（既存の順の歯の窓に入らない）。
- 行の割り方: 行 bj は形 1〜8 と形 10（門・記録・輪・stdin の節・rules 行）を持つ。行 bk は形 9（印・resume の断り・停止）を持ち、行 bj の後に着地する。
  - 行 bj の着地から行 bk の着地までの間、器は門の間の手の resume と stop を止めない。その間、orchestrator は門を撃っている便に手の resume を撃たない。
  - 行 bi（§67）と行 bj は、どちらも埋め込み manifest の行数の pin と外形の snapshot `rules_external_form` を 1 つ増やす。行 bj は行 bi の後に着地する。
- 門で向きが変わる既存の歯（census・main a526f825・`crates/scribe2-boundary/tests/e2e` の中・行 bj が直す）:
  - 撃たれた回数を数える stub（`verify-count.sh`）を共通 verify か契約の verify に置く歯: `pipe/gate.rs` の遮断器の歯の fixture（「gate の前は印が無い」を測る）と、印を数える歯。
  - 1 回目だけ緑の stub（`verify-once.sh`）を置く歯: `pipe/land.rs`（契約の verify と共通 verify）・`pipe/land/retire.rs`・`pipe/gate/detection.rs`。門が 1 回目の緑を使うので、gate が赤になる。
  - 撃ち始めの印を置いて眠る stub（`verify-slow.sh`）の歯: `pipe/stop.rs`。印が spawn の門で先に立つ。
  - 箱の中で死ぬ形の stub（`verify-oom.sh`）と、偽 `systemd-run` の記録を読む歯: `pipe/gate/confine.rs`。門の行の scope の記録が、gate の行の記録より前に積まれる。
  - 赤い行（`verify-red.sh`・`verify-noisy.sh`・赤い共通 verify・write-set の外を書く runner）を持つ便の歯: `pipe/gate.rs`・`pipe/land.rs`。runner が 2 回多く起き、段の記帳の列と commit の数と diff が変わる。同じ字を書く runner は 2 回目の commit が空で Failed に倒れる。
  - toy の crate を `cargo nextest` で撃つ行（`pipe/gate.rs`・`pipe/spawn.rs`・`pipe/land.rs`）は、門の分だけ時間が延びる（向きは変わらない）。
  - `--rules` の fixture（`write_rules_capped` の系）はこの行を持たないので、その便の門は撃たれず（要約の語 unmeasured）、段の記帳の列は変わらない。
  - 直し方: 歯の関心が門でない歯は、門を撃たない manifest（行の値 0）で起こすか、門の後に印や数えを始める形にする。直すのは歯の本文で、retroactive の札を付ける（helper だけを直す test file を作らない・flip-check）。census の外で落ちる歯が write-set の外の file に在れば、行を止めて orchestrator へ返す。
- 歯（接頭辞 `end_gate_`・行 bj・crates と docs で 0 件・2026-10-01）: e2e（`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`）で、stdin と回数を file に積む偽 runner と、`src/lib.rs` の中身で赤と緑が決まる stub の契約の verify 行を使う（stub は赤い周に cmd の字に無い語を stderr に出して rc 1）。stub の script は歯の file の中で書く。
  - (a) 1 周目に赤い中身を commit し、stdin に「## 門の赤」節の在る周に直す runner の便を 1 回 spawn する。runner が 2 回起き、2 回目の stdin に節と赤い行の字と `rc=1` と stub の語が在り、1 回目の stdin には節が無い。段の記帳の列が Spawned（`base:`）・Spawned（`end-gate:red:1:1`）・Spawned（`end-gate:1`）・Implemented（detail なし）の順。2 回目の runner の木の HEAD の祖先に 1 回目の commit が在る。`end-gate.jsonl` の要約の語が red・green の順。
  - (b) 直さない runner の便は、runner が 3 回起き、門の赤の記帳が 2 件で Implemented（要約の語 exhausted）。その便の gate（偽 lens は PASS）は FAIL で、`verify.jsonl` の record の数が、同じ契約を行の値 0 の manifest で通した便と同じ。値 0 の便は runner が 1 回で、`end-gate.jsonl` は要約の 1 行（off）だけ。
  - (c) 赤い行と一緒に、箱の中で死ぬ行（偽 `systemd-run` の PATH と `verify-oom.sh`）を持つ便と、遮断器の閉じる manifest（走行可能と待ちの倍率 0・待ちの上限 1 秒・gate の遮断器の歯と同じ形）で起こす赤い行を持つ便は、runner が 1 回で、門の赤の記帳が無く Implemented（要約の語 unmeasured）。
  - (d) 全行が緑の便は、runner が 1 回で Implemented の detail が無く、要約の語が green で、門の record の数が gate の段の数（write-set の段 1・共通 verify の行・契約の verify の行）と同じ。
  - (e) 埋め込み manifest の `runner.end_gate_rounds` の行（e2e `crates/scribe2-boundary/tests/e2e/rules/embedded.rs`）が、値 2・kind・発効・裁定 id と裁定日・形 Int を持ち、kind は `FollowRetries` の直後。行の無い manifest で起こす赤い行を持つ便（spawn.rs）は、runner が 1 回で、要約の語が unmeasured、理由が行の id を名指す。
  - 直す既存の歯: 埋め込み manifest の行数の pin と外形の snapshot `rules_external_form` の rows と kinds を 1 つ増やす。上の census の歯。
- 歯（接頭辞 `endgate_mark_`・行 bk・crates と docs で 0 件・2026-10-01）: e2e（`crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`）で、撃ち始めに git の共通 dir へ印を置き、解放の file が在るまで待つ（上限 60 秒）stub の契約の verify 行を使う。spawn を子 process で起こし、印を待つ。
  - (a) 門の間に撃った `pipe resume --run <id> --runner <偽 runner>` は rc 1 で、判定行に `end-gate=alive` を持ち、`SeatSpawned` が増えず、偽 runner の回数が 1。解放の後の spawn は Implemented で、run dir に `end-gate.pid` が無い。
  - (b) 門の間に撃った `pipe stop --run <id>` の後、解放した spawn は段を書かず（最後の event は `RunStopped`）、runner の回数が 1。
- base で RED の理由:
  - 行 bj: base は門を撃たない。(a) と (b) は runner が 1 回で落ちる。(c) と (d) は `end-gate.jsonl` が無くて落ちる。(e) は kind が無く compile されない（機能不在）。行数の pin と snapshot は、base の manifest が 1 行少ないので落ちる。
  - 行 bk: 行 bj の後の base は印を置かない。(a) の resume は死んだ runner の pid を読んで runner を起こし直し（回数 2）、(b) は門の後に Implemented を書くので落ちる（機能不在）。
- SRS: 門の赤で runner を起こし直す周は SRS 0.33 の FR98 と FR6（門を経て Implemented）が定め、周の上限の値は rules 行の裁定（user 2026-09-30T22:13Z 項 end-gate・値 2）が持つ。
- 却下（[ADR-0102](../../design-intent/decisions/ADR-0102-lens-reads-with-read-only-tools-and-runner-gets-the-common-verify.html) §4）:
  - **gate の verify の赤を自動で runner へ戻す**: §49 の却下と同じ（判定の段の後に戻すと、着地の列と終端の数えが動く）。門は判定の前で、runner の turn の延長として置く。
  - **claude の session を `--resume` で続ける**: session の記録は口座の設定 dir に在り、起こし直しの口座が変わると続けられない。器は起動形を「stdin の契約と節」の 1 つに保つ（途中再開と同じ）。
  - **gate が門の緑を持ち越して撃ち直さない**: 節約は common の中央値で約 137 秒で、門と gate を木の sha で結ぶ跨 process の記録が増え、gate が独立に測る性質が弱まる。着地の後に gate の時間が問題になったら測り直して決める。
  - **§65 の節だけで止める**: runner が撃ったかは器が測らない（撃った runner は 28 本中 5 本）。
  - **門で lens も撃つ**: 論理の誤りの審査は gate の lens の役目で、門は機械の検査だけを持つ。
  - **輪を spawn の中に置く**: spawn は Budget を受ける側で、測り直しの手を持たない。`spawn_turn` は既に回答・追随・途中再開の材料を毎周読み直すので、同じ所に輪を置く。
- 限界: 緑の便も門と gate で共通 verify を 2 回撃つ（中央値の和で約 137 秒・p90 で約 450 秒が足される）。runner の論理の誤り（gate の lens の FAIL の型）は塞がない。

## 67. lens の turn の上限 — rules 行 `lens.max_turns`（30）を lens の argv に毎回渡し、上限で終わった周の出力は判定を持たない周と読む（契約表の行 bi・SRS FR5・FR9・[ADR-0102](../../design-intent/decisions/ADR-0102-lens-reads-with-read-only-tools-and-runner-gets-the-common-verify.html) §2.1・§64 の限界の後の行）

やさしく言うと: 審査役は §64 で code を読めるようになったが、何回読むかに天井が無い。rules 行で上限 30 を決め、毎回 claude に渡す。上限に達して終わった周は、途中で出した判定があっても採らず、「判定できなかった」と読む。gate はその周を 1 回だけ撃ち直し、2 回目も上限なら「判定できなかった」で止まる。

- 何が起きているか（main a526f825・verified）:
  - `headless/mod.rs` の `Call` は `max_turns` を持ち、`Some` の周だけ argv に `--max-turns <n>` が載る。今 `Some` を渡すのは `fleet usage` の refresh（1）だけで、lens の `call_of` は `None` を渡す。`Call` の doc は「runner と lens は None」と書く。
  - lens は rules 行を `rows_of` で cap → model → effort の順に読む。解けない行の周は claude を呼ばず rc 2（`lens: <理由>` の 1 行）。`rows_of` は `dispatch` と memo の審査（`--stage memo`）の 2 か所から呼ばれ、どちらも `call_of` で claude を組む。
  - lens の判定の読み `verdict_line` は、最後の JSON 行が claude の封筒（`type` = `result`）なら、`result` の text の最後の JSON 行を判定に読む。封筒の `subtype` は見ない。
  - `subtype` の閉じた読み（`error_max_turns` を含む）は `headless/runner.rs` の `result_subtype` と `ResultKind` が持つ（runner の観測行が使う）。
  - `result` を持たない封筒は、判定の JSON 行が無い周として `lens output has no json line` の INCONCLUSIVE になる。gate は findings の無い判定の行を形の読めない周として同じ gate の中で 1 回撃ち直す（`gate/lens.rs` の `judge_pairs`）。契約の審査は kind の無い INCONCLUSIVE を `Unparsed` と読み、列が撃ち直す。
  - 上限で終わった実 claude の封筒が `result` を持つかは測っていない。持つ周に途中の判定の行が在ると、今の読みはそれを判定に採る。
- 形:
  1. **rules 行**: `lens.max_turns`（kind `LensMaxTurns`・Int・値 30・発効・ruling `user 2026-09-30T22:13Z 項 lens-turns`・ruled_at `2026-09-30`）を足す。kind の宣言順は `RunTokenCeiling` の直後に置く（`GateTokenCap` の直後の窓と `HookBudgetMs` の後ろの窓の間で、既存の順の歯の窓に入らない）。
  2. **読み**: lens は `rows_of` で effort の後にこの行を読む（読む順は cap → model → effort → turn の上限）。
     - 行が無い・不発効・整数でない・値 0 の周は、claude を呼ばず rc 2 で `lens: lens.max_turns …` の 1 行を出す（cap と同じ極性・0 は上限にならないので断る）。
     - `--stage` に依らず、契約の審査・gate・先撃ち・memo の審査の lens の全部が同じ行を読む（lens の turn の上限は 1 つ）。
  3. **渡し方**: `call_of` は `max_turns` にその値を渡す（argv に `--max-turns <値>` の対がちょうど 1 つ）。`Call` の `max_turns` の doc を「lens は rules 行の値・runner は None」に直す。
  4. **上限で終わった周の読み**: `verdict_line` は、封筒の `subtype` が `error_max_turns` の周は `result` の text を読まない。判定を INCONCLUSIVE にし、evidence に `lens が turn の上限（lens.max_turns）で終わった` と書く。
     - 消費の 3 対は他の周と同じく足す（`with_usage`・上限で終わった周も消費は在る）。
     - `subtype` の読みは runner と同じ `result_subtype` の 1 本を使う（閉じた読みを 2 つにしない）。
     - 判定の行は findings と population を持たないので、gate は形の読めない周として同じ gate の中で 1 回だけ撃ち直し、2 回目も上限なら INCONCLUSIVE に止まる（FR9・gate の側は変えない）。契約の審査は kind の無い INCONCLUSIVE を今どおり `Unparsed` と読む（FR49・審査の側は変えない）。
- 触らない: runner の起動形（turn の上限を渡さない）・gate の撃ち直しの判定・契約の審査の読み・lens の usage と help（flag を足さない）・lens の cmd の雛形。
- 却下:
  - **flag `--max-turns` を lens の起動行に焼く**: 値の出所が rules 行と起動側の雛形の 2 つに割れる（`--cap` を外した理由と同じ・C1）。
  - **上限で終わった周の `result` の途中の判定を採る**: 読み切る前の判定で、PASS なら偽の PASS になりうる（AC3）。
  - **上限で終わった周を撃ち直さずに止める分岐を gate に足す**: SRS FR5 が「parse できない周と読む」と決めていて、今の撃ち直しの 1 回で足りる。2 回目も上限なら止まる。
- 限界:
  - 上限の値 30 は、読んだ lens の実測（2〜10 turn）からの余裕である。重い便で足りるかは、着地の後に判定行の `turns` で見る。
  - 上限で終わった実 claude の封筒の形は、着地の後に 1 回測って台帳の notes に残す。器の歯は、`result` を持つ偽の封筒と持たない偽の封筒の両方を測る。
- 歯（接頭辞 `lens_turns_`・crates と docs で 0 件・2026-10-01）: 偽 claude が argv を file に写す形で、
  - (a) e2e（`crates/scribe2-boundary/tests/e2e/headless.rs`）: `lens.max_turns` を 7 と 30 にした manifest の lens の argv に、`--max-turns` の直後がその値の対がちょうど 1 つ在る。`--stage prelens` と `--stage memo` の lens も同じ対を持ち、`--rules` の無い lens は埋め込みの 30。
  - e2e（`crates/scribe2-boundary/tests/e2e/headless/lens.rs`）:
    - (b) 行の無い manifest・不発効の manifest・値 0 の manifest の lens は、claude を呼ばず（印の file が無い）rc 2 で、stderr が `lens: lens.max_turns` で始まる理由の 1 行。
    - (c) 偽 claude が `subtype` = `error_max_turns` で消費の 6 値を持つ封筒を返す lens の stdout の判定が INCONCLUSIVE で、evidence が `lens.max_turns` を含み、key `turns` を持つ。封筒の `result` が findings と population を持つ PASS の判定の行の周と、`result` を持たない周の両方で同じ。
  - (d) e2e（`crates/scribe2-boundary/tests/e2e/pipe/gate.rs`）: gate の lens の行を器の lens（本 binary の `lens` と、(c) の封筒を返し呼ばれた回数を file に積む偽 claude）にした便は、gate が INCONCLUSIVE で、偽 claude の回数が 2、stderr の撃ち直しの行が 1 本で理由に `lens.max_turns` を含む。
  - (e) e2e（`crates/scribe2-boundary/tests/e2e/rules/embedded.rs`）: 埋め込み manifest の `lens.max_turns` の行が、値 30・kind・発効・裁定 id と裁定日・形 Int を持ち、kind は `RunTokenCeiling` の直後。
  - 直す既存の歯:
    - 埋め込み manifest の行数の pin（`e2e/rules/embedded.rs`）と、外形の snapshot `rules_external_form` の rows と kinds を 1 つ増やす。
    - lens の成功の周を撃つ歯の fixture の manifest に turn の上限の行を足す（足さないと rc 2 で落ちる）。`e2e/headless.rs` の `rules_with_cap` と、`e2e/headless/lens.rs` の model・effort・段・memo の歯が組む行の列（main a526f825 で 5 か所）。失敗の周を測る fixture（model や effort の行が無い manifest）は、先の行で落ちるので直さない。
- base で RED の理由: (a) は base が `--max-turns` を渡さないので落ちる。(b) は base がこの行を読まず claude を呼ぶので落ちる。(c) は base が `result` の PASS を判定に読み、`result` の無い封筒では evidence が `lens output has no json line` なので落ちる。(d) は base の gate が PASS で偽 claude の回数が 1 なので落ちる。(e) は kind が無く compile されない。行数の pin と snapshot は、base の manifest が 1 行少ないので落ちる（機能不在）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "pipe/cli.rs を cli/args.rs / state.rs / show.rs / resume.rs に、e2e/pipe/lifecycle.rs を ratelimit.rs / stop.rs に割る（純移動）"
req = ["FR30"]
section = "5"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "docs/design/pipeline.md", "docs/design/dispatcher.md", "docs/design/working-memory.md", "docs/design/contract-source.md", "docs/design/account-autonomy.md", "docs/design/consumer-sync.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_ratelimit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_stop_"]
size = "S"
done = "pipe/cli.rs が入口と shim だけになり、lifecycle.rs が ratelimit.rs / stop.rs に割れて、歯の本数と外形 snapshot が不変"

[[contract]]
id = "b"
title = "受付の e2e の hub（4159 行・歯 134 本）を接頭辞ごとに 3 file へ割る（純移動）"
req = ["FR30"]
section = "42"
write-set = ["-crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "+crates/scribe2-boundary/tests/e2e/pipe/review.rs", "+crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "+crates/scribe2-boundary/tests/e2e/pipe/refuse.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_fail_stops_before_spawn pipe_review_fail_is_terminal_for_spawn_resume_stop_and_overlap pipe_review_inconclusive_without_lens_or_unreadable_output_is_terminal pipe_review_pass_spawns pipe_review_kind_fail_keeps_kind_and_at_in_review_json_and_two_word_detail pipe_review_kind_pass_carries_neither_kind_nor_at pipe_review_kind_missing_or_unknown_or_unreadable_falls_to_unparsed_without_moving_the_verdict pipe_review_reads_design_section_and_requirements_from_base pipe_review_reads_requirements_text_from_yaml pipe_review_reads_requirements_text_from_md pipe_review_reads_requirements_reason_for_bare_yaml_id pipe_review_reads_requirements_reason_for_empty_md_heading pipe_review_has_no_skip_flag pipe_review_stage_and_guard_are_pinned_in_declaration_order pipe_review_resume_from_intake_reviews_before_spawning", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_closure_ext_ contract_derive_ contract_check_ contract_declared_ contract_table_landed_ contract_schema_ contract_names_ pipe_contract_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_refuse_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_state_ pipe_show_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_ pipe_preflight_"]
size = "S"
done = "(1) 審査の段の歯 15 本が + の 1 本目に在り (2) 契約表と閉包の歯 46 本が + の 2 本目に在り (3) 断りの語彙の歯 8 本が + の 3 本目に在り (4) 単発の pipe_state_ / pipe_show_ の 2 本が親の tests/e2e/pipe.rs に在り (5) 元の file には受付の口の歯 63 本だけが残って行数が 4159 から約 2000 へ縮み (6) 親の mod 宣言が 3 本増えて歯の名・本文・順序と #[test] の総数 134 は不変で子は use super::* で親の helper を引き (7) 札 flip-check: moved s2-07l.351 が元の file と + の 3 file と親 tests/e2e/pipe.rs（移した 2 本の直前）に在って純移動の機械証明の残差が mod 宣言と札と移した 2 本と (9) の helper だけ (8) 親の pin 歯 pipe_hermetic_sites_stay_one は触られず緑（tracked 12 = 宣言 11 + 1） (9) 子が使う helper は逐語で親へ移して pub(super) か元の file に残して pub(super) のどちらかで、本文の差は 0 で可視性の差だけ"
depends = ["a", "aq"]

[[contract]]
id = "c"
title = "入口の flip check の免除経路を閉じる — docs-only の面・札の形と上限・push(main) の出所・宣言の VerifyKind"
req = ["FR7", "FR17", "FR50"]
section = "7"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck/git.rs", "crates/xtask/src/main.rs", "crates/xtask/src/limits.rs", "+crates/xtask/src/provenance.rs", "crates/xtask/src/flipcheck_tests.rs", "crates/xtask/src/flipcheck_declaration_tests.rs", "crates/xtask/src/flipcheck_overlay_tests.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", ".github/workflows/ci.yml", "docs/design/pipeline.md", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_check_docs_only_ flip_check_marks_ main_provenance_", "cargo nextest run -p xtask --no-tests=fail limits_match_rules_manifest", "cargo nextest run -p scribe2 --no-tests=fail declaration_kind_", "cargo nextest run -p scribe2 --no-tests=fail rules_flip_ rules_external_form rules_kind_parity_every_kind_has_sample"]
size = "M"
done = "(1) docs-only の面の外の file を含む便が .rs の差分無しでも no-test-diff で落ち、面の中だけの便は従来どおり通り、no-rust-diff の skip が判定の語彙から消える〔flip_check_docs_only_〕 (2) 形に合わない札が bad-marker・rules 行 flip.marks_per_pr を超える本数が too-many-marks で落ち、形も本数も満たす札は従来どおり免除が効く〔flip_check_marks_〕 (3) PR の squash の件名末尾 (#N) か pipe land の trailer run: <run id> を持つ HEAD を xtask main-provenance が通し、どちらも持たない HEAD を落とす〔main_provenance_〕 (4) 先頭語 cargo の行を持ちながら入口の flip を撃つ行を持たない宣言を intake が NoEntranceRed で断り、先頭語 cargo の行を持たない宣言（sh / git だけの common-verify）は断らず、宣言 file の schema は不変〔declaration_kind_〕 (5) 足した rules 行 2 本が kind・enabled・裁定 id・ALL の列・parse の 5 面で引け〔rules_flip_〕、外形 snapshot の rules: ok rows= と kinds= を持つ 2 行がどちらも 2 増える〔rules_external_form〕・kind の網羅が保たれる〔rules_kind_parity_every_kind_has_sample〕 (6) 閾値の読み手が足した行を含めて欠け無く読める〔limits_match_rules_manifest〕"

[[contract]]
id = "d"
title = "runner / lens の prompt 全文を外形 snapshot 2 本で pin する"
req = ["FR5"]
section = "6"
write-set = ["crates/scribe2/src/headless/runner.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_runner_prompt_external_form"]
size = "S"
done = "runner の prompt 全文の snapshot が足され（lens 側は既存の named snapshot lens_prompt_external_form）、prompt 本文の変更が .snap 差分として PR に現れる"

[[contract]]
id = "e"
title = "純移動の機械証明が base から持ち越した札（moved 以外）を新規の札と読まない — 両側で同じ字面の札を対にして残差から外し、対の無い札だけを ForeignMarker にする"
req = ["FR9", "NFR1"]
section = "7"
write-set = ["crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_move_proof_carried_"]
size = "S"
done = "持ち越しの札を持つ純移動が要約で lens に渡り、新規・id 違い・消えた札は従来どおり diff で渡る"

[[contract]]
id = "i"
title = "pipe retire の終端の列挙に Reviewed の非 PASS（FAIL / INCONCLUSIVE）を足す — 審査の段で終端した便の残った worktree を可逆 move で畳み、PASS と読めない判定は断る"
req = ["FR49", "FR14"]
section = "12"
write-set = ["crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_retire_reviewed_"]
size = "S"
done = "(1) retire が受ける段の列に Reviewed が在り (2) Reviewed で verdict が FAIL の便と INCONCLUSIVE の便がそれぞれ pipe retire で retired/ へ畳め〔pipe_retire_reviewed_fail_ / pipe_retire_reviewed_inconclusive_〕 (3) PASS は断られ〔pipe_retire_reviewed_pass_refused_〕 (4) 判定を読めない便も断られ〔pipe_retire_reviewed_unreadable_refused_〕 (5) 断りの 2 本が ReviewCheck::as_str の語で字面 run <id> の段は Reviewed である（verdict=PASS）と run <id> の段は Reviewed である（verdict=読めない）をそれぞれ逐語で測り、段違いの一般則（verdict の括弧を持たない）と区別が付き (6) 畳んだ 2 本が畳んだ後の段を Reviewed のまま・RunStage detail=retired を対で測り (7) 新しい歯 4 本が全部 crates/scribe2-boundary/tests/e2e/pipe/land.rs に在って接頭辞 pipe_retire_reviewed_ が base で 0 本・実装の後に 4 本だけを選び、既存の pipe_retire_ の 6 本と pipe_follow_retire_ の 2 本は名も本数も不変"

[[contract]]
id = "j"
title = "xtask の flipcheck.rs から git / tar で base を取り出す群を flipcheck/git.rs へ割る — 純移動・呼び手は pub use で不変・札 moved"
req = ["FR7"]
section = "13"
write-set = ["-crates/xtask/src/flipcheck.rs", "+crates/xtask/src/flipcheck/git.rs", "crates/xtask/src/flipcheck_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_check_base_", "cargo nextest run -p xtask --no-tests=fail flip_check_rename_", "cargo nextest run -p xtask --no-tests=fail flip_manifest"]
size = "S"
done = "git 群 10 関数が子 module に在り、親は mod 宣言と pub use だけが増えて呼び手と歯の import は不変、既存の flip_ の歯が全部緑で純移動の機械証明が残差 0"

[[contract]]
id = "f"
title = "未知の flag と --help を全 subcommand が typed に断る — 閉包の検査を 1 module の parse に集めて面の入口で 1 回撃ち、land が unknown flag で何も動かさない"
req = ["NFR4", "FR12"]
section = "14"
write-set = ["+crates/scribe2/src/cli_args.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/account/cli.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail cli_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_args_unknown_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_args_help_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail fleet_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail vessel_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail account_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail fleet_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail vessel_external_form"]
size = "M"
done = "(1) 共通の reader が parse(args, allowed) -> Result<Parsed, ArgsError> の 1 本で在り (2) ArgsError が Help / Unknown / Missing / Duplicate の閉じた enum で宣言順の as_str を持ち (3) --help と -h は allowed に無くても Help で返り呼び手が usage を出して rc 0 で終わり (4) Unknown / Missing / Duplicate は usage 1 行で rc 2 になり (5) 面の入口（pipe の dispatch・headless の分岐・fleet / seat / vessel / account の cli）が parse を 1 回撃って各 subcommand が宣言順の const の allowed を持ち、reader 6 本の定義と本体の呼び手は不変で account/cli.rs の flags だけが parse の呼出に置き換わり (6) 偽 remote の toy repo で pipe land に未知の flag を渡すと main の ref・event log・worktree が 1 つも動かず rc 2 になり〔歯 pipe_land_args_unknown_〕 (7) 同じ toy repo で pipe land に --help を渡しても main の ref・event log・worktree が 1 つも動かず usage を出して rc 0 で終わり〔歯 pipe_land_args_help_・2026-09-15 の回帰そのもの〕 (8) fleet / seat / headless / vessel / account の 5 口が 1 口ずつ名指しの歯で未知の flag を rc 2 に断り〔fleet_args_ / seat_args_ / headless_args_ / vessel_args_ / account_args_〕 (9) usage の外形 snapshot 5 本が 1 字も動かない"

[[contract]]
id = "g"
title = "pipe の --repo と --state-dir の cwd fallback を落とす — 写し面を消した run に --repo 無しで spawn しても cwd の repo に落ちない"
req = ["FR4", "FR39"]
section = "15"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_repo_required_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "S"
done = "(1) --repo の無い spawn が cwd を読まず flag 不在の断り 1 行で rc 1 になり〔pipe_repo_required_spawn_〕 (2) 写し面の無い周の run の repo 解決も同じ断りで止まり〔pipe_repo_required_run_〕 (3) --state-dir も --repo も無い（--repo だけ在る周は救われる）置き場の解決も同じ断りで止まり〔pipe_repo_required_state_dir_〕 (4) 上の 3 本がそれぞれ「worktree が 1 つも増えず event が 1 件も増えない」を対で測り (5) usage に --repo の要件の 1 句が載って pipe_external_form の snapshot がその 1 句だけ動く"

[[contract]]
id = "h"
title = "runner / lens の effort を rules 行 runner.effort から毎回渡す — build が --model と同じ場所で --effort を渡し、行が無い・表に無い値は rc 2"
req = ["FR5", "FR9"]
section = "16"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_runner_passes_effort_ headless_lens_passes_effort_ headless_effort_row_refuses_"]
size = "S"
done = "偽 claude で runner と lens を撃つと argv に effort の値が rules 行のとおり載り、行の無い manifest と表に無い値は rc 2 で claude の呼出 0"

[[contract]]
id = "k"
title = "lens の verdict に findings の閉じた category と件数・母集団を必須 key にする — 欠落と母集団 0 は INCONCLUSIVE"
req = ["FR9", "NFR1"]
section = "17"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/lens.rs", "+crates/scribe2/src/pipe/gate/findings.rs", "crates/scribe2/src/headless/lens.txt", "crates/scribe2/src/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_findings_", "cargo nextest run -p scribe2 --no-tests=fail lens_prompt_external_form"]
size = "M"
done = "lens の verdict が category ごとの件数と母集団を必ず持ち、欠落は INCONCLUSIVE"

[[contract]]
id = "l"
title = "land の stale base を同じ経路で人手なしで追随し直す（resume の Gated(PASS) 受けは既在・同じ周回を通る）"
req = ["FR30", "FR50"]
section = "18"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_stale_", "cargo nextest run -p scribe2 --lib --no-tests=fail follow_stale_"]
size = "S"
done = "toy repo で stale base の便が同じ land の中で人手なしで追随して Landed し、上限は合算の回数で typed な Failed"

[[contract]]
id = "m"
title = "着地列の窓の待ちを Completion に足し、pipeline 外の docs merge が pipe land-window を前置して撃つ"
req = ["FR30", "FR50"]
section = "19"
write-set = ["crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_window_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_wait_land_window_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_wait_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_stop_group_completion_match_is_exhaustive", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "S"
done = "(1) 唯一の wait に pid を見張らない窓の variant が 1 つ増え、既存の 8 つと deadline の経路が不変（wait.rs の既存の in-file の歯 15 本が全部緑で、変わるのは census の歯 pipe_stop_group_completion_match_is_exhaustive の腕 1 本と名 1 つだけ） (2) 窓は列の PASS 0 本・追随中（最新の RunStage が Implemented で detail が rebase: の便）0 本・local main が origin/main の祖先の 3 つ全部で開き、閉じる条件ごとに 1 本ずつ歯が在る〔fleet_wait_land_window_queue_ / _following_ / _unpushed_〕 (3) local を先に読み読めない周は origin の有無に依らず閉じ〔_unreadable_〕origin の無い周は remote=none を載せて 3 つ目を数えず〔_remote_none_〕 (4) origin の ref を fetch せず git の読みが既存の 2 口だけで (5) pipe land-window が開いた周は rc 0 の clear・閉じた周は rc 1 の busy を返し〔pipe_land_window_clear_ / pipe_land_window_busy_〕 (6) busy の行が unpushed=<sha|unreadable|-> の 3 値でどの条件で閉じたかを示し (7) usage の 1 行が増えて pipe_external_form の snapshot がその 1 行だけ動く"

[[contract]]
id = "n"
title = "runner の雛形に turn 終端の規律を足し、片付けで殺した子の数を record に残す"
req = ["FR5", "FR22"]
section = "20"
write-set = ["crates/scribe2/src/headless/runner.txt", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_runner_prompt_closes_turn_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_runner_prompt_external_form", "cargo nextest run -p scribe2 --lib --no-tests=fail confine_orphans_", "cargo nextest run -p scribe2 --lib --no-tests=fail runner_orphans_"]
size = "S"
done = "(1) 雛形に「検証は前面で完走させてから turn を閉じる（背景実行を残して終えない・残した task は片付けで止められ done に数えない）」の 1 行が在り（逐語の正本は設計 §20 の約束 1 で、done はその字面を写したもの） (2) その 1 行を逐語で名指す歯が別に在り〔headless_runner_prompt_closes_turn_〕 (3) 雛形の外形 snapshot が足した 1 行だけ動いて全文を pin し続け〔headless_runner_prompt_external_form〕 (4) 片付けが scope を止める直前に残った process の数を読み〔confine_orphans_counts_before_release_〕 (5) 終端の 1 行に orphans=<n|-> が載り、n=0 の周と読めない周の 2 本の歯が 0 と「測れなかった」を融合しないことを測り〔runner_orphans_zero_ / runner_orphans_unreadable_〕 (6) 数える関数（pipe/confine.rs）と行を組む関数（headless/runner.rs）が pure に切られて systemd の scope を起こさない fixture でそれぞれの in-file の歯が測る"

[[contract]]
id = "o"
title = "gate の段の通知行を rc に依らず record と run の stderr に残す"
req = ["FR8", "FR22"]
section = "21"
write-set = ["crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_notice_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_notice_"]
size = "S"
done = "(1) rc 0 で終わった段の err が捨てられず pipe run の stderr に段の順で出て〔pipe_spawn_notice_〕 (2) 同じ歯が対で stdout の判定行が base と 1 字も変わらないことを測り (3) gate の記録の step 行と同じ log に lens-input=<kind> reason=<語> が要約の周も diff の周も残り〔pipe_gate_notice_summary_ / pipe_gate_notice_diff_ の 2 本〕 (4) kind が diff / summary で語は要約の周が - になり（0 と測れなかったを融合しない） (5) 記録の歯が tests/e2e/pipe/gate.rs に・pipe run の stderr の歯が tests/e2e/pipe/spawn.rs に在る"

[[contract]]
id = "p"
title = "着地の番を取った事実を記帳し、撃ち直しの間も番を鍵の順と独立に列の先頭に残す"
req = ["FR50", "FR30"]
section = "22"
write-set = ["crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_turn_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_taken_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_record_values_are_the_closed_four pipe_order_key_is_the_first_gated_ts"]
size = "S"
done = "(1) 番を取った周に RunStage stage=Gated detail=turn:taken が 1 行だけ増え EventKind は増えず〔pipe_land_turn_taken_〕 (2) 縮退と測れなかった周は 1 行も書かず〔pipe_land_turn_not_taken_〕 (3) 偽の列で turn:taken を持つ便が在れば最新の 1 本（同時刻は run id の辞書順）だけが先頭になり無ければ鍵の順に戻り〔pipe_order_taken_latest_ / pipe_order_taken_falls_back_to_key_〕 (4) 終端・worktree 無し・verdict が PASS でない便の turn:taken は数えず INCONCLUSIVE で離れて戻った便が撃ち直し中の便を追い抜かず〔pipe_order_taken_leaver_〕 (5) Queued が最新の turn:taken の ts の field を持ち、turn_in が pure で Turn が閉じた 3 値のままなこと・鍵が最初の Gated の ts のままなことを既存の in-file の歯 2 本（pipe_order_record_values_are_the_closed_four / pipe_order_key_is_the_first_gated_ts）が緑のまま示す"

[[contract]]
id = "q"
title = "pipe stop 起因の終端を oom-kill に誤分類せず、kernel の証拠が無い kill は unknown に倒す"
req = ["FR22", "FR46"]
section = "23"
write-set = ["crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_terminal_reason_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_reason_vocabulary_"]
size = "S"
done = "stop した便が oom-kill に分類されず、証拠の無い kill は unknown"

[[contract]]
id = "r"
title = "pipe retire が受ける終端の段を Stage の終端全部（Failed は detail 不問）と Gated(FAIL) に広げる — 残る穴は discriminate の Failed の detail の弁別だけ"
req = ["FR34"]
section = "24"
write-set = ["crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_retire_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_follow_retire_"]
size = "S"
done = "(1) Failed の便が detail を問わず pipe retire で畳め、base で断られていた 4 つの detail（main-red / main-unmeasured / rebase-dirty / precheck:…）を 1 つずつ名指す歯がそれを測り〔pipe_retire_failed_any_detail_〕 (2) 受ける集合が Stage の終端全部（Landed / Stopped / Failed）∧ Gated(FAIL) になり (3) 畳んだ歯が対で clean の検査・retired/ への可逆 move・RunStage detail=retired の記帳・allowed の列が不変なことを測り (4) 非終端（Spawned / Implemented）と Gated(PASS) は断られる〔pipe_retire_failed_any_detail_still_refuses_live_〕 (5) 既存の pipe_retire_ 6 本 / pipe_follow_retire_ 2 本（母集団 = tests/e2e/pipe/land.rs の歯全数）は極性が反転する 1 本（pipe_retire_rebase_empty_refuses_other_failed_reasons）を除き不変"

[[contract]]
id = "s"
title = "純移動の要約（MoveSummary）のコメント行の差に base 側 - / head 側 + の逐語を載せ、件数は母集団と対で残す"
req = ["FR9"]
section = "25"
touches = ["crate::pipe::move_proof::CommentDiff"]
tests = ["crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail move_proof_comment_verbatim_"]
size = "S"
done = "コメント行だけが違う純移動の要約に、違う行の逐語が - / + 付きで件数の行の下に並ぶ"

[[contract]]
id = "t"
title = "pipe review が受けた lens の cmd を run dir の lens.toml に写し、gate / land / resume が --lens の無い周にそれを読む"
req = ["FR10", "FR9"]
section = "26"
write-set = ["+crates/scribe2/src/pipe/lens_record.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_lens_record_"]
size = "S"
done = "review で渡した lens だけで land の再 gate が lens を起動して着地し、写しが読めない周は「無い」と別の理由で INCONCLUSIVE"

[[contract]]
id = "u"
title = "pipe land の終端が refs/heads/main を実測し、stdout の main= と Landed の detail の main: に写す"
req = ["FR50"]
section = "27"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_main_measured_"]
size = "S"
done = "reference-transaction hook で main を動かす toy repo の land が、landed= と違う main= を stdout と detail に写し rc 0 で終端する"

[[contract]]
id = "v"
title = "e2e の pipe が binary を起こす口を新設の bin_cmd 1 本に集め、git repo でない temp dir を cwd にして起こす — cwd の fallback が本番の state dir へ届かない"
req = ["NFR6"]
section = "28"
write-set = ["crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/launch_failure.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_hermetic_"]
size = "S"
done = "(1) crates/scribe2-boundary/tests/e2e/pipe.rs に binary を起こす関数 bin_cmd が新設され、Command を返して cwd を git repo でない temp dir に固定する (2) 親の helper 3 本 run_pipe / run_pipe_with_path / land_once_with_git_shim のそれぞれで、--state-dir も --repo も無い pipe show が「repo の root を解決できない」の断りで rc 1 になる — 3 本を 1 本ずつ名指した歯 pipe_hermetic_run_pipe_ / pipe_hermetic_run_pipe_with_path_ / pipe_hermetic_land_once_with_git_shim_ が測り、1 本だけ直しても緑にならない (3) 歯 pipe_hermetic_sites_ が tracked の 9 file（crates/scribe2-boundary/tests/e2e/pipe.rs と crates/scribe2-boundary/tests/e2e/pipe/ 配下）を読み、Command::new(bin()) と Command::new(super::bin()) の出現の合計が 1（bin_cmd の中の 1 箇所）であることを、読んだ file 数 9 と base の 41 site を母集団として同時に出して測る (4) cwd が主題の 2 site（crates/scribe2-boundary/tests/e2e/pipe/launch_failure.rs の相対 --repo の歯と crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs の 1 箇所）は bin_cmd の返り値に自分の current_dir を後置して主題を保ち、(3) の合計 1 を崩さない (5) 差し替えで cwd の fallback に頼っていたことが露出した呼出しは、その site に --repo / --state-dir を明示して直す — 歯の名・本数・assert は 1 つも変えないので本行はこの句に検証行を置かず、緑の担保は done の 8 門の workspace 実走が持つ（設計 §28 の「触らない」と同じ）"

[[contract]]
id = "w"
title = "pipe land が rebase-empty の周に main の log を run: trailer で探し、在れば squash と CAS を撃たず主実測の後に Landed（already-landed）で終端する"
req = ["FR50"]
section = "29"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_already_landed_"]
size = "S"
done = "自分の trailer を持つ squash が main に在る便の land が main を動かさず主実測を撃って Landed で終端し stdout と detail に already-landed を持ち、trailer が無い周と別の便の trailer の周は従来どおり rebase-empty"

[[contract]]
id = "x"
title = "検出線は main の差分が DETECTION_SCOPE に触れた周だけ撃つ — 追随の再 gate は Gate の印で、主実測は diff-tree で判定し、飛ばした周は reason 付きで record する"
req = ["FR46"]
section = "30"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_detection_scope_"]
size = "M"
done = "docs だけで main が動いた便の追随の再 gate と主実測が検出線を撃たず reason=outside-scope の record を残し、crates が動いた周と読めない周は従来どおり撃つ"

[[contract]]
id = "y"
title = "flip-check の present_mods_only に <stem>/<name>.rs の子 module の探索を足す — tests/e2e/<x>.rs の子に置いた新規 module の宣言が base 段で落ちない"
req = ["FR7"]
section = "31"
write-set = ["crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck_declaration_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flipcheck_declaration_nested_"]
size = "S"
done = "宣言 file の子 dir に新規 module を足す diff で flip が RED-on-base ok を出し、同じ dir の 2 形と #[path] の扱いは不変"

[[contract]]
id = "z"
title = "flip-check の base-not-green に経路の弁別子（signal / unnamed rc=<rc> / retry-failed rc=<rc>）を後置する — 閉じた enum 1 つ・極性一覧と判定行の先頭は不変"
req = ["FR7"]
section = "32"
write-set = ["crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flipcheck_base_reason_"]
size = "S"
done = "base が compile error の周の判定行が base-not-green:unnamed rc=101 を含み、理由の純関数が 3 経路を宣言順の variant に写し、極性一覧の行と判定行の先頭は不変"

[[contract]]
id = "aa"
title = "追随の再 gate を main の差分が検出線の面の外だけの周は省く — 前周の Gated PASS を新 base へ引き継ぎ skipped=regate reason=outside-scope を記録し、主実測は従来どおり全行"
req = ["FR14", "FR34"]
section = "33"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_follow_docs_only_"]
size = "S"
done = "docs だけで main が進んだ周は land が lens を呼ばず再 gate せずに Gated PASS を引き継いで着地し、crates/ が進んだ周と diff を読めない周は従来どおり再 gate する"

[[contract]]
id = "ab"
title = "追随で入った契約表の行が便の消した path を名指す周は rebase-stale-rows で記帳して runner を起こし直す — 写しの write-set にその設計 doc を追記し、回数は衝突と同じ上限、他の findings は従来どおり再 gate"
req = ["FR34", "FR47"]
section = "34"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_follow_stale_rows_"]
size = "M"
done = "(1) 便が消した path を名指す行が追随で入った周は land が Implemented detail=rebase-stale-rows: を記帳して runner を 1 回起こし直し、写しの write-set にその設計 doc が末尾 1 項目として追記され（既存の項目は順序も字面も不変）、再 gate は撃たれない〔pipe_follow_stale_rows_restarts_〕 (2) 行が便と無関係の path を名指す周と契約表の検査を撃てない周は起こし直さず従来どおり再 gate へ進む〔pipe_follow_stale_rows_unrelated_〕 (3) 衝突と同じ 1 つの上限 pipe.follow_retries に達した周は Failed detail=rebase-stale-rows で終端する〔pipe_follow_stale_rows_exhausted_〕 (4) --runner の無い land は記帳して rc 1 で止まり resume で続けられる〔pipe_follow_stale_rows_no_runner_〕"

[[contract]]
id = "ac"
title = "verify の record に failed=<歯の名> と落ちた歯ごとの stderr の区間を残す — nextest の FAIL 行と stderr の小見出しだけを読む pure な関数 1 本を gate と主実測が共有する"
req = ["FR50", "NFR4"]
section = "35"
write-set = ["crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_verify_failed_"]
size = "S"
done = "nextest 形の stderr から failed= が最初の落ちた歯を指し、落ちた歯ごとの区間が上限行数で record に残り、FAIL 行の無い stderr は従来どおり末尾だけ、gate と主実測が同じ関数を通り、主実測は verify-main.jsonl と同じ dir に同じ stem の verify-main.stderr.log を残し（main-red の便の e2e が file の有無を測る）、MainCheck の 3 値と末尾行数の定数は不変"

[[contract]]
id = "ad"
title = "着地の列が driver の死んだ便を先頭に数えない — Queued に札の生死の 3 値を足し、死んでいる便だけを ahead から外して skipped-dead で名指し、段は動かさない"
req = ["FR11", "FR14"]
section = "36"
write-set = ["crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_order_dead_"]
size = "S"
done = "先頭の便の札が死んだ pid の fixture で後続の pipe land が first で進み order= の行に skipped-dead=1 と verdicts.jsonl に skipped_dead が載り、札が生きている先頭と札の無い先頭は従来どおり待ち、死んだ便の段と worktree は不変"

[[contract]]
id = "ae"
title = "flip-check が歯の外の行だけ動いた test file を単独で撃たず本体を撃つ木へ同梱する — 判定行に fixture=N を後置し、歯の中の行が動いた file と、同梱しか flip の無い便は従来どおり落ちる"
req = ["FR7"]
section = "37"
write-set = ["crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck_overlay_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flipcheck_fixture_"]
size = "S"
done = "歯の外の行だけ動いた file を持つ diff が RED-on-base ok を出して判定行に fixture=1 が載り、歯の中の行が動いた file は green-on-base のまま単独で落ち、歯の外の file しか flip しない便も green-on-base で落ち、宣言 file の同梱と removed-only と札の扱いは不変"

[[contract]]
id = "af"
title = "追随の形が無い便を merge-base からの rebase --onto で追随する — 祖先検査を閉じた 3 値（follow.rs・follow_main と section が同じ 1 関数）にし、祖先でないが merge-base の在る base は便の commit だけを main の上へ運んで既存の記帳・衝突・再 gate の経路に合流し、起こし直しの追随節と runner の雛形も --onto の 2 sha の形にし、merge-base の無い周だけ stale base で断る"
req = ["FR11", "FR30"]
section = "38"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_onto_"]
size = "M"
done = "base が main の祖先でなく merge-base の在る便の land が rebase --onto で追随して Landed になり、衝突は既存の rebase-conflict の経路へ、merge-base の無い main は従来どおり stale base の rc 1 で event 0 増"

[[contract]]
id = "ag"
title = "pipe stop --run が段を問わず便を終端にする — store の条件付き append で Stopped の後の RunStage / RunDone / SeatSpawned を lock の中で断り、stop は札の運転手の process group を席と同じ 1 関数で止めてから RunStopped を書く"
req = ["FR13", "FR11"]
section = "39"
write-set = ["crates/scribe2/src/fleet/store.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_stop_driver_"]
size = "M"
done = "RunStopped の後の段の記帳が lock の中で断られて event が増えず、pipe stop --run が札の運転手を止めて記録を残し、自分自身と札の無い便は従来どおり、止め切れない周は RunStopped を書かず rc 1"

[[contract]]
id = "ah"
title = "着地の列の先頭 N 本を候補の木 1 つに積み検査を 1 回撃って列の順に着地し、赤なら列を解く（merge train・rules 行 land.train_max〔4・裁定 id user 2026-09-17T03:28Z〕）"
req = ["FR34", "FR10", "FR12", "FR50"]
section = "40"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2/src/pipe/train.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_train_", "cargo nextest run -p scribe2 --no-tests=fail rules_land_train_ rules_external_form rules_kind_parity_every_kind_has_sample"]
size = "M"
done = "(1) 上限 3 で先頭を land すると 3 本が列の順に着地し（親の連鎖・main の先端・Landed 3 件・verdicts.jsonl 3 行・後続 2 本が order=train）追随は 0 回で、先頭の verify.jsonl に共通 verify が train=3 で 1 組・後続の verify.jsonl は契約 verify と検出線だけ・着地済みの便の land は rc 0 で main 不変・番待ちの 2 本目は起きた後に rc 0 already-landed で終端し event が増えない〔pipe_train_〕 (2) 3 本目の契約 verify が赤なら列を解いて先頭だけが着地し、後続 2 本は Gated PASS のまま列に残り stdout に dissolved が出る (3) 2 本目が先頭と衝突する周は 2 本目を候補から外して 1 本目と 3 本目が着地し、2 本目の worktree は clean のまま event が増えない (4) 上限 1 と行の不在は先頭だけが着地し後続は従来どおり追随 1 回 (5) 列を選ぶ pure 関数の in-file の歯が鍵の順・PASS でない便・worktree の無い便・終端の便を数えないこと・上限で切ることを測る (6) rules 行 land.train_max が kind・enabled・裁定 id・ALL の列・parse で引け〔rules_land_train_〕、外形 snapshot の rules: ok rows= と kinds= を持つ 2 行がどちらも 1 増え〔rules_external_form〕、kind の網羅が保たれる〔rules_kind_parity_every_kind_has_sample〕 (7) 先端の木の主実測が赤の周は列の便すべてが Failed の main-red で Landed 0 件・main は N 本ぶん進んだまま"

[[contract]]
id = "ai"
title = "検出線は便の diff の patch-id を record に残し、追随の撃ち直しと候補の木の段で patch-id 不変なら前周の record を carried 付きで写して撃たない"
req = ["FR34", "FR46"]
section = "40"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/train.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_detection_carry_"]
size = "S"
done = "(1) gate が検出線を撃つ周の record に patch_id が在り git patch-id --stable の値と一致する (2) main が crates/ で動いた追随で便の diff が不変なら検出線を撃たず、前周の record を carried=<n> を付けて写す（Detection の閉じた値に持ち越しが 1 つ増え、写した値は record の field で実測と区別が付く） (3) 便の diff の中身が変わる周・前周の record が無い周・record が patch_id を持たない周・読めない周は撃つ（fail-closed）"
depends = ["ah"]

[[contract]]
id = "aj"
title = "pipe/land.rs の主実測の群（verify_main / main_red / main_unmeasured ほか 9 item と const 3 つ）を land/verify.rs へ割る — 純移動・MainCheck は親に残す（極性一覧の pin）・land の本体は不変・札 moved"
req = ["FR11"]
section = "41"
write-set = ["-crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_land_subject_", "cargo nextest run -p scribe2 --no-tests=fail polarity_external_form"]
size = "S"
done = "主実測の群 9 item と const 3 つが子 module に在り、MainCheck は親に残って極性一覧の snapshot が不変、親は mod 宣言と use だけが増え（子側の pub(super) 4 語〔measure_main は finish が呼ぶ〕で land と finish の本体は不変）、file-lines で land.rs の余地が base より 180 行以上増え、in-file と e2e の歯が全部緑、純移動の機械証明の残差が use と path と可視性の語だけ"

[[contract]]
id = "ak"
title = "pipe/land.rs の「squash と finish」の群（18 item・326 行）を子 module へ割る — 純移動・Terminal は親に残す（極性一覧の pin）・landed_sha と terminal は pub use で再輸出・札 moved"
req = ["FR50"]
section = "43"
write-set = ["-crates/scribe2/src/pipe/land.rs", "+crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_land_subject_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_terminal_land_landed_sha_reads_only_its_own_run_done pipe_terminal_land_squash_message_carries_the_contract_and_requirements_trailers pipe_terminal_land_outcomes_are_the_closed_seven", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_external_form"]
size = "S"
done = "(1) squash と finish の群 18 item が + の file に名・本文・順序のまま在り (2) Terminal / TERMINAL_TOKENS / TERMINAL_POLARITY / impl Terminal / RUN_TRAILER / VERDICTS_FILE / verdicts_path が親に残って polarity_external_form の snapshot が 1 字も動かず (3) 親に増えた item が mod 宣言 1 つと use 文 3 つだけ（open_pr / squash / finish の素の use・landed_sha / terminal の pub(in crate::pipe) use・squash_message / subject_of / trailer_key / CONTRACT_TRAILER / REQUIREMENTS_TRAILER / SHA_PREFIX / SUBJECT_CHARS の #[cfg(test)] 付き use）で land と attempt の本体が 1 字も変わらず、cfg(test) の無い build で unused_imports が 0 件（cargo clippy --workspace --all-targets -- -D warnings が緑） (4) 可視性を上げるのは子側だけ（親が呼ぶ item と歯が読む item は pub(super)・land:: で引かれる 2 つは pub(in crate::pipe)＝再輸出は可視性を広げられないため）で親側の可視性は 1 語も変わらず (5) in-file の歯 8 本が 1 本も動かず全部緑で、歯の区間の use super::{…} が 1 字も変わらず (6) 札 flip-check: moved が親の歯の区間の先頭と + の file の先頭に対で在り (7) file-lines で land.rs の余地が base の 84 から 300 以上へ増え (8) crates/scribe2-boundary/tests/e2e/polarity.rs の diff が 0 行（この file が write-set に在るのは受付が検証行の歯の file を求めるためで、便は 1 byte も変えない）"

[[contract]]
id = "al"
title = "pipe/move_proof.rs の「diff を読んで item の列にする」群（24 item・357 行）を子 module へ割る — 純移動・公開の 5 名は親に残す・呼び手の字面は不変・札 moved"
req = ["FR9"]
section = "44"
write-set = ["-crates/scribe2/src/pipe/move_proof.rs", "+crates/scribe2/src/pipe/move_proof/read.rs", "crates/scribe2-boundary/tests/e2e/polarity.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail move_proof_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_external_form"]
size = "S"
done = "(1) 前段の 24 item が + の file に名・本文・順序のまま在り (2) 後段の 6 群と外から引かれる 5 名（judge / keep / LensInput / POLARITY / RULINGS_FILE）が親に残って polarity_external_form の snapshot が 1 字も動かず (3) 親は mod 宣言 1 行と use だけが増えて pub use が 0 本で（前段の名を move_proof:: で引く呼び手が 0 件） (4) 可視性を上げるのは子側だけ（後段が呼ぶ item を pub(super)）で親側の可視性は 1 語も変わらず (5) in-file の歯 19 本が 1 本も動かず全部緑で (6) 札 flip-check: moved が親の歯の区間の先頭と + の file の先頭に対で在り (7) file-lines で move_proof.rs の余地が base の 0 から 300 以上へ増える"

[[contract]]
id = "am"
title = "xtask の flipcheck.rs から nextest を撃って出力を読む群（7 item・約 130 行・struct FailedTest と impl は親に残す）を子 module へ割る — 純移動・親に増えるのは mod 1 行と use 2 文（本体用 5 名と、#[cfg(test)] を別の行に持つ 1 名）の計 4 行だけ・歯の 5 file と git.rs は 1 行も触らず flipcheck_tests.rs は札 2 行だけ・札 moved"
req = ["FR7"]
section = "45"
write-set = ["-crates/xtask/src/flipcheck.rs", "+crates/xtask/src/flipcheck/nextest.rs", "crates/xtask/src/flipcheck_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_check_parses_failed_tests_", "cargo nextest run -p xtask --no-tests=fail flip_check_child_nextest_disables_color no_fail_fast_is_in_flipcheck_nextest_args"]
size = "S"
done = "(1) nextest を撃って出力を読む群 7 item（trimmed / relay / nextest / nextest_with / nextest_args / strip_csi / failed_tests）が + の file に名・本文・順序のまま在り、struct FailedTest と impl FailedTest は親に残って 1 字も変わらず、子は use super::FailedTest; の 1 行でそれを引き (2) 親に増えた行が mod 宣言 1 行・failed_tests / nextest / nextest_with / relay / trimmed の 5 名を列挙した素の use 1 行・nextest_args 1 名の use（#[cfg(test)] だけの行と use nextest::nextest_args; の行の 2 行・src の本体の全 item の後・既存の #[cfg(test)] #[path] mod tests の 3 行の直上）の計 4 行だけで（pub は付けない）、judge_each / base_is_green / retry_named / run_on_base を含む親の本体が 1 字も変わらず、cfg(test) の無い build で unused_imports が 0 件（cargo clippy --workspace --all-targets -- -D warnings が緑） (3) 歯の 6 file のうち 5 file と crates/xtask/src/flipcheck/git.rs が 1 行も変わらず（5 file は use super::* ・crates/xtask/src/flipcheck_tests.rs の use super::{…} と git.rs の use super::{trimmed, …} は親の 2 つの use が解く）、write-set に在る crates/xtask/src/flipcheck_tests.rs の diff が先頭の札 2 行（// 始まりの説明 1 行と // flip-check: moved の 1 行）の追加だけで歯の本文と use super::{…} は 1 byte も変わらない (4) 子側で pub(super) にする item が名指しの 6 つ（relay / nextest / nextest_with / nextest_args / failed_tests / trimmed・全部 fn の頭の行）で、strip_csi と struct FailedTest の field と filterset の可視性は 1 語も変わらず、pub(crate) と pub use は 1 つも増えず、親側の可視性も変わらず (5) 札 flip-check: moved が crates/xtask/src/flipcheck_tests.rs の先頭（既存の use super::{…} の直前・.372 の札の隣）と + の file の module doc の直後に対で在り、親 flipcheck.rs には無く、gate の flip-check の判定行に moved=1 が載り (6) file-lines で flipcheck.rs の余地が base の 216 から 300 以上へ増える"
[[contract]]
id = "an"
title = "着地の CI 照合が event=schedule の run を母集団から外す — 既定の 1 行に event を足し、ci_now は schedule の run を除いてから落ちた run 優先・全 completed で success・残り 0 本は測れないと判定する（約束の行の形）"
req = ["FR50", "FR12"]
section = "46"
size = "S"

[[promise]]
of = "an"
n = 1
text = "ci_now は run の event が schedule のものを母集団から外し、残りで従来の判定（落ちた run を先に見て Failure・全部 completed で Success・それ以外は None）を行い、外した後に run が 0 本なら None を返す。event の欄が無い run は外さない"
files = ["crates/scribe2/src/fleet/wait.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
teeth = ["pipe_terminal_land_ci_ignores_scheduled_runs", "pipe_terminal_land_ci_only_scheduled_runs_is_unmeasurable"]
place = "crates/scribe2-boundary/tests/e2e/pipe/land.rs"
fixture = "偽 CI（既存の fake_terminal_json）が [{completed, success, event=push}, {in_progress, null, event=schedule}] を返す宣言で Gated PASS の便を land する（上限は fixture の manifest の pipe.ci_wait_s）。負の枝は [{completed, success, event=schedule}] だけを返す偽 CI"
expect = "正の枝は終端の detail が terminal:ci:success → close まで進み bead が閉じる（base は schedule の run を待って ci:unmeasurable）。負の枝は terminal:ci:unmeasurable で bead が閉じない（base は success に倒れる）"

[[promise]]
of = "an"
n = 3
text = "母集団を絞る関数は event の欄が無い run を外さない（宣言 ci-cmd が event を返さない consumer は従来どおりの判定）＝外すのは event の値が schedule の run だけで、欄の不在は schedule と読まない"
files = ["crates/scribe2/src/fleet/wait.rs"]
teeth = ["fleet_wait_ci_runs_without_event_are_kept_and_scheduled_are_dropped"]
place = "crates/scribe2/src/fleet/wait.rs"
fixture = "in-file の歯: 偽の JSON の木 3 本（event の欄が無い completed/success・event=push の completed/success・event=schedule の in_progress）を、ci_now が母集団を絞るのに使う関数（base に無い）へ渡す"
expect = "残るのは event 無しと event=push の 2 本で schedule の 1 本だけが外れる（関数が base に無い＝道具不在の RED）"

[[promise]]
of = "an"
n = 2
text = "既定の CI の 1 行（DEFAULT_CI_CMD）が JSON の欄に event を含む（宣言 ci-cmd が無い consumer でも schedule の run を弁別できる）"
files = ["crates/scribe2/src/pipe/declaration.rs"]
teeth = ["pipe_declaration_default_ci_cmd_carries_the_event_field"]
place = "crates/scribe2/src/pipe/declaration.rs"
fixture = "in-file の歯（既存の DEFAULT_CI_CMD が {sha} の穴を持つ歯の隣）"
expect = "DEFAULT_CI_CMD の --json の欄の列に event が在り、{sha} の穴は 1 つのまま"

[[contract]]
id = "ao"
title = "着地列の窓が止めた便・終えた便を追随中に数えない — 追随中の条件に「replay した段が終端でない（Landed / Failed / Stopped でない）」を重ね、窓の (a)(c)・行の字面・rc は不変（§19 約束 2 (b) を閉じる）"
req = ["FR30", "FR50"]
section = "47"
write-set = ["crates/scribe2/src/pipe/queue.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_window_following_"]
size = "S"
done = "(1) Implemented rebase: の後に RunStopped で止めた便と RunDone Landed で終えた便が追随中に数えられず、窓が (a)(c) だけで開く (2) Implemented rebase: のまま終端の記帳が無い便は従来どおり追随中に数える (3) Implemented の detail が rebase: で始まらない便は従来どおり数えない (4) 窓の判定は replay を 1 回だけ読む（(a) の材料の段を (b) が重ねる） (5) e2e の pipe_land_window_ の歯 3 本と queue.rs の既存の in-file の歯が 1 字も変わらず緑"

[[contract]]
id = "ap"
title = "終端の台帳の close を repo を cwd にして撃つ — ledger の close が repo を受けて current_dir で撃ち、運転手の cwd（消えた dir でも）に依らず台帳を閉じる。断りの 2 値と記帳の字面は不変"
req = ["FR50"]
section = "48"
write-set = ["crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_land_close_cwd_"]
size = "S"
done = "(1) ledger の close が repo を受けて current_dir(repo) で bd を撃ち、偽 bd が書いた cwd が repo に等しい (2) 偽 bd が rc 1 で断る周は Refused { rc: Some(1), tail: 末尾の 1 行 } のまま (3) 消える dir を cwd にした子 process からの pipe land が terminal:close:ok まで進む（base は close:failed） (4) 終端の 3 段の順・CI の照合・CloseError の 2 値・CLOSE_REASON・台帳を読む口が 1 字も変わらず、既存の pipe_terminal_land_ の歯が緑"

[[contract]]
id = "aq"
title = "e2e の置き場の pin 歯を mod 宣言との整合で測る — pipe_hermetic_sites_stay_one の file 数の定数 9 を親の列 0 の mod 宣言の数 + 1 に替え、site の合計 1 と 41 の母集団は不変（分割のたびに定数を触らない・歯だけ・retroactive 札）"
req = ["NFR6"]
section = "28"
write-set = ["crates/scribe2-boundary/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_hermetic_sites_stay_one"]
size = "S"
done = "(1) 歯 pipe_hermetic_sites_stay_one が tracked の file 数と親の列 0 の mod 宣言の数 + 1 の等式で置き場を pin し、site の合計 1 と base の 41 site の母集団の出し方は不変 (2) base（9 file・宣言 8）で緑・宣言を 1 本消した A/B と pipe/ 配下に file を 1 本足した A/B が赤（変異 proof は notes） (3) 札 flip-check: retroactive s2-07l.547 が歯の fn の中の行頭に在って base に無い"

[[contract]]
id = "ar"
title = "契約の赤でない Gated FAIL を同じ worktree で再 gate する口を足す — 段が Gated かつ判定が FAIL かつ運転手が居ない便に、裁定の逐語を持つ RunStage を 1 件書いて Implemented へ戻し、最新の Gated より後ろの 2 度目は断る（段の種別も event の種別も増やさない）"
req = ["FR8", "FR34"]
section = "49"
touches = ["crate::pipe::cli::PipeCommand"]
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2/src/pipe/regate.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_regate_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_regate_"]
size = "M"
done = "(1) pipe の subcommand の表と flag の表に regate の 1 行が在り、--run と --reason を受ける (2) 判定は state.rs の live（pub(in crate::pipe)・不変）を呼んで読み、+ の file は Stage の変種を Gated と Implemented の 2 つ名指すだけで Stage の網羅 match を持たず（第 2 の段の読み手を作らない・C2）、段が Gated かつ判定が FAIL かつ運転手の札が無いか死んでいて --reason が非空の 5 形のうち 1 形だけが通り、live が None の周を含む残り 4 形は rc 1 で何も書かない (3) 通った周が書く event はちょうど 1 件で、種別が RunStage・段が Implemented・detail が regate: の後ろに入力の逐語をそのまま持ち、worktree と判定の file は 1 byte も変わらない (4) 同じ便への 2 度目が断られ、間に Gated の RunStage を 1 件挟むと次の 1 回が通る (5) 判定 FAIL の Gated の便に口を撃つと rc 0 で regate: run= from=Gated to=Implemented の 1 行が出て、その後の段が Implemented になり worktree の path が変わらず、段の種別 11 個と event の種別 19 個はどちらも増えない"
[[contract]]
id = "as"
title = "固定日付を持つ fixture の母集団を歯 1 本で pin する — e2e の tracked な .rs の本数と、reset / 期限の欄に座る日付の字面の本数を年で 2 つに割って 3 つ組で留める（fixture と器の src は 1 字も動かさない・歯だけ・retroactive 札）"
req = ["FR33", "FR36"]
section = "50"
write-set = ["crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail e2e_fixture_clock_"]
size = "S"
done = "(1) 足した歯が e2e の tracked な .rs を git ls-files で引き、reset / 期限の key を持つ行のうち日付の形の字面を持つ行を年で割って (file 数, 年 2099 以上, 年 2099 未満) = (29, 7, 5) を 1 つの等値で測り、失敗の message に母集団の全数と当たった行の file 名が出る (2) 歯の file 自身が母集団に数えられない（key の字面を concat! で割って組む＝歯を足す前後で file 数以外の 2 欄が動かない） (3) 変異の A/B が 3 本とも赤（e2e に 2026 年の reset 行 1 行・2099 年の reset 行 1 行・.rs 1 本をそれぞれ足す・proof は便の notes） (4) 札 flip-check: retroactive s2-07l.469 が歯の fn の中の行頭に在って base に無い (5) crates/scribe2/src の file と e2e の既存の歯が 1 字も変わらず緑"
[[contract]]
id = "at"
title = "stop の全部止めを live な便の本数で絞る — Live な席が指す別々の便が 2 本以上の周は理由の逐語を要り、逐語は止めた便の終端の記帳に載り、rc 0 の 1 行が止めた便と止め切れなかった便を母集団つきで返す"
req = ["FR13", "NFR4"]
section = "51"
write-set = ["crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "=crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_stop_scope_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_stop_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "M"
done = "(1) Live な席が指す別々の便が 2 本以上の周は --all だけでは rc 1 で断られ、席も events も 1 つも動かず、断りの 1 行が便の本数を母集団として名指す (2) 便が 1 本以下の周は --all だけで従来どおり通り、対象なしは rc 0 のままである（歯 (e) が 0 本と 1 本の 2 形で pin する） (3) stop の flag の表と usage の 1 行が --reason を値つきで持ち（usage の外形 snapshot を更新し既存の歯 pipe_external_form が緑・snapshot の .rs は = の印で write-set に在り審査の要約がそれを剥がして読む＝行 au（contract-source §44）の着地後の世代の器で審査する）、--run と --reason を同時に渡す周は rc 1 で断られる (4) --all と --reason で通った周の便の終端の記帳が reason: の後ろに入力の逐語をそのまま持ち、--run で止めた便の記帳は detail を持たない (5) rc 0 の 1 行が席数と止めた席数に加えて、終端を記帳した便の id を記帳順で並べる欄と止め切れなかった席を持つ便の id を並べる欄を持ち、空の欄は出ない (6) rc の 3 値・停止の順・pid を持たない Live 席を母集団に数える形・運転手の札の扱いが変わらず、既存の pipe_stop_ の歯が 1 字も変わらず緑（verify の e2e 行が接頭辞 pipe_stop_ で既存と新設を丸ごと撃つ）"

[[contract]]
id = "au"
title = "gate の段の便が base を追随できる口を足す — 終端でなく運転手が居ず木が clean で base が main の祖先の便に、木だけを main の先端へ載せ替えて段を実装へ戻す記帳を 1 件書き、衝突した周は木を戻して断る（段の種別も event の種別も増やさない）"
req = ["FR14", "FR34"]
section = "52"
touches = ["crate::pipe::cli::PipeCommand"]
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2/src/pipe/follow_step.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_follow_step_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_follow_step_"]
size = "M"
done = "(1) pipe の subcommand の表と flag の表に追随の口の 1 行が在り、便 1 本を名指す flag を受ける (2) 便が在る・段が終端でない・運転手の札が無いか死んでいる・木が clean・base が main の祖先 の 5 つを全部満たす 1 形だけが通り、1 つずつ外した 5 形と段を測れない周は rc 1 で何も書かない (3) 通った周が書く event はちょうど 1 件で段が実装へ戻り、detail が着地の追随と同じ接頭のあとに 2 つの sha を持ち、main の sha は 1 字も変わらない (4) 載せ替えが衝突した周は rc 1 で木の先端が撃つ前と同じ sha に戻り、events が 1 件も増えず、着地側の衝突の起こし直しは通らない (5) 木の載せ替えの 1 段が 1 本だけになり（着地の 746 行がその 1 本を呼ぶ）、着地の追随の既存の歯が 1 字も変わらず緑 (6) Gated の便に口を撃つと rc 0 で便 id と 2 つの sha を持つ 1 行が出て、その後の段が実装になり木の base が main の先端になり、段の種別と event の種別はどちらも増えない"

[[contract]]
id = "av"
title = "入口の flip-check が rename を対にして読む — 変更 file の列挙を name-status で取り、R の行は base 側を旧 path で読んで test 区間の同一を test-diff 無しに数え、旧 path の札を持ち越す（A / M / D の対と札の門の値は変えない）"
req = ["FR7"]
section = "53"
write-set = ["crates/xtask/src/flipcheck/git.rs", "crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck_tests.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_check_rename_"]
size = "S"
done = "(1) rename だけの便（同一本文の対）に入口の flip-check を撃つと too-many-marks にも green-on-base にも not-flippable にも落ちず、flip 0 で rc 0 (2) 対の base の本文が旧 path から読まれ、旧 path の test 区間の札はこの便の札に数えない (3) test 区間に差が在る対は flip に数え、overlay の書き先が新 path で、moved の札の免除と tests-removed-only が従来どおり当たる (4) docs-only の面の判定が旧 path と新 path の両方を数え、面の中から外へ・外から中へ rename しただけの便はどちらも docs-only にならない（歯 (f)） (5) A / M / D だけの便の対と判定行が 1 字も変わらず、既存の flip_check_ の歯が全部緑 (6) 対の rename は base 側と HEAD 側の path の違いだけで決まり、同じ path で本文が同一の M の行（mode だけの変更）だけを持つ便は no-test-diff で落ちる"

[[contract]]
id = "aw"
title = "宣言の任意 key entrance-flip（値は unmeasured の 1 語だけ）で入口の flip を測らないと名乗った宣言を、受付と契約表の検査が NoEntranceRed で断らず、名乗りと入口の flip の行の同居は矛盾として断り、判定行に entrance=unmeasured の欄を出す（key の不在は現行・本 repo の宣言は key を持たない）"
req = ["FR7", "FR8"]
section = "54"
write-set = ["crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_entrance_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_check_entrance_"]
size = "S"
done = "(1) key entrance-flip = unmeasured を持ち cargo の行を持ち入口の flip の行を持たない宣言が kind_gap 無しで受付と契約表の検査を通る (2) key の無い同じ宣言は NoEntranceRed のまま断られ、本 repo の宣言は key を持たない (3) 値が unmeasured 以外・空・list・key の重複は宣言の読みの誤りとして typed に断られ、理由が key の名を持つ (4) key と入口の flip の行を同時に持つ宣言は KindGap の 1 値で矛盾として断られる (5) 契約表の検査の判定行が key を持つ周だけ末尾に entrance=unmeasured の欄を持ち、key の無い周の判定行は 1 字も変わらず、既存の declaration_ と contract_check_ の歯が全部緑"

[[contract]]
id = "ax"
title = "flip-check が mod 行と pin の数値 1 か所だけ動いた宣言 file を本体の木へ同梱し、同梱した宣言 file の歯の赤を turn の RED に数えず、同梱されなかった新規 module を green-on-base でなく not-flippable で落とす"
req = ["FR7"]
section = "55"
write-set = ["crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck_declaration_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flipcheck_declaration_pin_"]
size = "M"
growth = ["crates/xtask/src/flipcheck.rs:200"]
done = "(1) tests 配下の file で、mod 行が 1 本以上動き、他の動いた行が歯の中の削除 1 本と追加 1 本の対で数字の並び 1 か所だけが違う file を宣言と pin の file と認め、どれか外れる file は従来の路のまま (2) その file を宣言 file と同じ側で本体の木へ同梱し（mod 行は turn ごとに絞り pin の行は HEAD のまま）、判定行が decl= に数えたうえで pin=N を後置する (3) 宣言 file を同梱した turn は落ちた歯のうち同梱した宣言 file の歯の名を除いて判定し、残り 0 本は green-on-base、名指し 0 本は infra-error の bundled-unnamed で、弁別は 3 値の純関数 1 つ (4) tests 配下の target の根でない新規 module の turn の木に宣言が無ければ nextest を撃たずに not-flippable files=<本体> で落ち（在処は本体の dir の main.rs / mod.rs と dir 同名の .rs に加え、本体が tests/<d>/mod.rs の周は target の根 tests/<f>.rs 全部＝根の別名の file が宣言した新規 module は撃たれる）、既存の歯 2 本（flip_check_still_judges_declaration_file_that_also_changes_tests と flip_check_treats_pub_crate_mod_line_as_declaration の後半）の期待がこの語に替わる (5) 形 1 の file しか flip しない便は従来どおり単独で撃たれ、宣言 file の他の既存の歯と flipcheck_declaration_nested_ の歯は 1 字も変えずに緑"

[[contract]]
id = "ay"
title = "名乗った消費側の契約の nextest の検証行を受付で base の木でも撃つ — 名乗りの語を unmeasured / detect / deny の 3 語にし、読みを入口の lock の前へ移し、rc を 4 値に写して受付の 1 行と preflight に base で緑の本数を出して run dir に記録を残し、deny は base で緑か測れない行が 1 本でもあれば受付で断る（key の無い宣言と本 repo の宣言は不変）"
req = ["FR1", "FR7"]
section = "56"
touches = ["crate::pipe::refuse::Refuse", "crate::pipe::declaration::EntranceFlip"]
write-set = ["crates/scribe2/src/pipe/cli/intake.rs", "+crates/scribe2/src/pipe/cli/intake/refusal.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/cli/base_run.rs", "-crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/pipe/declaration/entrance_flip.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_base_run_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_base_run_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_base_run_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_check_entrance_word_"]
size = "L"
growth = ["crates/scribe2/src/pipe/cli/intake.rs:60", "crates/scribe2/src/pipe/refuse.rs:40", "crates/scribe2/src/pipe/gate.rs:5", "crates/scribe2/src/pipe/table.rs:5", "crates/scribe2/src/pipe/table/check.rs:5"]
done = "(1) 宣言の entrance-flip が unmeasured・detect・deny の 3 語を受け、3 語とも NoEntranceRed で断られず、入口の flip の行との同居は UnmeasuredWithEntranceFlip（名は不変・理由の 1 行は語の字面を持たない）で断られ、契約表の検査の判定行の entrance= が語をそのまま写し、値の型と読み手が declaration の子 module に在って親の行数が縮む (2) 受付と preflight が HEAD の sha を材料の読みの前に読み、上限の読み・材料の読み・行の生成と表の検査・freeze を入口の lock の前に 1 周に 1 回撃ち、lock の中の judge は freeze の結果を借りて宣言を読み直さない（実走の間に宣言を壊す commit を積んでも受付は宣言の断りで落ちない） (3) detect か deny の周だけ契約の nextest の検証行（既存の nextest_read の 1 本で選び、--no-tests=fail を持つ行）を sha の木の一時の worktree で 1 行ずつ gate と同じ遮断器・撃つ口・箱で撃ち、nextest の行でない行と --no-tests=fail の無い行は撃たず、読み切れる行（行の全体が ASCII の英数字と _-.:=/ と半角 space だけ・-p と --test が半角 space で区切った旗と値で丁度 1 つずつ・その値は - で始まらない・他の旗は --no-tests=fail と -- の後ろの libtest の引数だけ）で members の逐語の crate の dir に target が無い行だけを撃たずに不在とし（仕分けの純関数の歯で、読み切れる対照の 1 行が不在で、7 周目の 6 形・8 周目の 4 形・= の形の 1 行・全角空白 / NBSP / 改頁 / CR で旗を 1 語に埋めた 4 行・値が - で始まる 1 行の計 16 行は撃つ側・target は用意した木を symlink を辿って読み Cargo.toml を TOML として読んで、名の .rs・名の dir の main.rs・[[test]] の name・inline の表の配列の name・symlink の tests dir の 5 形を在ると読む・この判定は base で緑の行を不在にしない）、他の行は既定の crate を当てずに撃ち、nextest_read と閉包の読みは変わらず、撃ち終えた木と worktree の登録が残らない (4) 撃った行が rc 0 / 4 / 100 / 他 で base で緑・不在・落ちた・測れないに写り、oom_kill の行と遮断器の閉じた行以後は測れず、木を用意できない周・置き場の無い preflight の周・実走の後の HEAD の sha が違う周は全行が測れない (5) 受付の 1 行と preflight の行が entrance=green-on-base:<本数>/<行数> を持ち、測れない行があれば unmeasurable:<本数> を添え、通った受付の run dir の記録 file が sha と行ごとの 4 値・rc・秒を持ち、detect は受付の結果を変えない (6) deny の周は base で緑か測れない行が 1 本以上あれば judge の判定関数 1 本が Refuse の宣言順の末尾の variant entrance-not-red（rc 1）で断って便・run dir・event を作らず、preflight の refuse= に同じ理由が出て rc 1 で、列の候補の判定は立てず（pipe dispatch ls が待たせない）、refuse_names_are_pinned_in_declaration_order が 23 値の期待で緑 (7) unmeasured と key の無い宣言は木を用意せず何も撃たず、受付の結果と 1 行と記録が変わらず（読みを lock の前へ移したことによる断りの先後の入れ替わりは除く）、本 repo の宣言は key を持たず、既存の contract_check_entrance_・declaration_entrance_・pipe_intake_ の歯が緑"
depends = ["aw"]

[[contract]]
id = "az"
title = "着地が anchor を揃えなかった周（dirty / collision / unreadable / sync-failed）を anchor の git dir の印に残し、その周の Landed の detail に判定行と同じ token を足し、pipe land-window が古い anchor を 1 語で出して窓を閉じ、pipe の新しい口 anchor-sync が index と作業の木に残る着地前の中身だけを main の先端の中身へ戻して印を外す（利用者の編集と着地の外の path は触らず、戻せない path は名指して断る）"
req = ["FR11", "FR12", "FR30"]
section = "57"
touches = ["crate::pipe::cli::PipeCommand", "crate::pipe::queue::Window"]
write-set = ["+crates/scribe2/src/pipe/land/anchor.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_anchor_stale_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_anchor_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_anchor_sync_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_window_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none", "cargo nextest run -p scribe2 --lib --no-tests=fail help_table_"]
size = "M"
growth = ["crates/scribe2/src/pipe/land.rs:30", "crates/scribe2/src/pipe/land/finish.rs:15", "crates/scribe2/src/pipe/queue.rs:30", "crates/scribe2/src/pipe/cli.rs:30", "crates/scribe2/src/pipe/cli/args.rs:5"]
done = "(1) sync_anchor の 2 か所の呼び手の直後（main の実測の前）に、dirty / collision / unreadable / sync-failed の周は anchor の git dir の NAME の dir の下に着地の前の main の sha（from）1 行だけの印が rename で置かれ（ADR-0004 D-4 の git dir の下の file の形・新しい ADR は書かない）、既に在る印は書き換えられず（from を保つ）、from と着地後の main が同じ周は書かれず、synced の周は印が消え、not-main の周は触られず、書けない・消せない周は land の rc を変えず stderr に 1 行が足され、書き手は境界 crate の歯から呼べる (2) 印を書いた周だけ Landed の detail の末尾に空白 1 つと anchor=skipped:<理由> が足され、synced と not-main の周の detail と Failed の detail と判定行と warning の字面は 1 字も変わらない (3) 古さの判定が閉じた 3 値（新しい / 古い〔from と本数〕/ 読めない）の 1 本で、crate の中から呼べ（land.rs が pub(crate) で再輸出・行 h の hook が呼ぶ）、印が無ければ git dir を解く 1 本のほかは撃たずに新しく、印が在れば from から HEAD の着地の path のうち index か作業の木が from と同じ path が在れば古く、無ければ（手で揃えた周）新しく、印（16 進 40 字か 64 字の 1 行でない）か from か HEAD を読めない周は読めないで、読めないは古いと同じく窓を閉じる側（fail-closed）に倒れる (4) Window が anchor の判定の欄を持ち、window_now が --repo の checkout に判定を撃ち、古い・読めない周は rc 1 の busy の行の unpushed の欄の後ろに anchor=stale / anchor=unreadable を持ち、新しい周の行は clear も busy も 1 字も変わらない (5) PipeCommand の variant anchor-sync（usage に 1 行・本数 18・help pipe の SUBCOMMANDS に 1 行）の pipe anchor-sync --repo R が、印が無ければ anchor-sync=none rc 0、古い path が無いか印が読めなくても tracked の変更が無ければ印を消して anchor-sync=already rc 0、index と作業の木の両方が from と同じで足された path なら作業の木に file が無い path だけを restore --source=<HEAD> --staged --worktree で戻して新しくなれば印を消して anchor-sync=synced paths=<本数> rc 0、片方だけが from と同じ path と足された path に file が在る path は戻さず anchor-sync=refused mixed=<path,…> rc 1 で印を残し、印が読めず tracked の変更が在る周と git を読めない周は anchor-sync=refused unreadable:<理由> rc 1、restore が断った周は anchor-sync=failed restore:<1 行> rc 2 で印を残し、利用者の編集を持つ path と着地の外の path の中身は変わらない (6) anchor_plan の見立て・sync_anchor の順と read-tree の形・AnchorSkip の 5 値・land の rc・land-window の 3 条件と --wait-s が変わらず、既存の pipe_land_anchor_ と pipe_land_window_ の歯と synced の周の detail を pin する歯と help_table_ の歯（と done の定義の全数の nextest が撃つ cli_help_ の歯）が 1 字も変えずに緑"

[[contract]]
id = "ba"
title = "gate の段 ①（write-set 照合）が契約の約束を便の木で測る — write-set の + の file が便の HEAD に全部在り ~ の file が全部無いこと、約束の行の symbols の + の名が便の木で解けることを測り、外れた周は段 ① の rc 1（FAIL）で外れた項目を名指す（段・理由の型・verdict は足さない）"
req = ["FR8", "FR9", "FR48"]
section = "58"
write-set = ["crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "+crates/scribe2-boundary/tests/e2e/pipe/gate/promised.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_gate_promised_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_promised_"]
size = "M"
growth = ["crates/scribe2/src/pipe/gate/verify.rs:150"]
done = "(1) 段 ① は diff の照合の後に、write-set の + の項目が便の HEAD の木に全部在り ~ の項目が全部無いことを git の木の読み 1 回で測り、外れた項目を rc 1 と stderr の見出しつきの列で名指し、木を読めない周は rc -1 で、外れを返すのは (2) と同じ pure な 1 関数 (2) 設計 pointer の行が約束の行を持つ周だけ、便の base の設計 doc の約束の行の symbols の + の名を便の HEAD の木の .rs の本文で symbols_in_base と同じ読み手で解き、解けない名を rc 1 で名指し、読み手の母集団の file（.rs）を 1 本も持たない木では名を測らず rc を変えずにその旨の 1 行を stderr に残す (3) 段・理由の型・verdict を足さず、land の主実測も同じ run_checks の段 ① で同じ 1 本を撃つ 歯: pipe_gate_promised_ の lib が + の在る / 無い・~ の在る / 無い・接頭辞なし・約束の + の名の解ける / 解けない・.rs の無い木の注記の形を測り、e2e（gate の e2e の子 module・親は mod の 1 行と、+ の先を宣言しながら作らない既存の fixture の直し〔畳みの歯の helper が rename しない周にも + の先を write-set に宣言していたので、rename の周だけ宣言する形にし、flip-check の retroactive の札を 1 行添える〕だけ）が + の file を作らずに commit した偽 runner の便の gate FAIL と段 ① の rc 1 と path・作った便の PASS、約束の行の + の名を宣言しない便の gate FAIL と名・宣言した便の PASS、写しの write-set に木に無い + の項目を足して主実測を撃たせた land の main-red と verify-main.jsonl の段 ① の rc 1 と path を測り、base では gate が PASS・land が Landed で RED"
[[contract]]
id = "bb"
title = "pipe/declaration.rs の任意 key の群（key の列 2 つ・key の名と既定 6 つ・値の読み手 4 つ・外へ渡す口 5 つの 17 item）を子 module へ割る — 純移動・外から引く path は親の再輸出で不変・歯は動かさず札 moved（FR83 / FR85 / FR86 の新しい key の受け皿）"
req = ["FR83", "FR85", "FR86"]
section = "59"
write-set = ["-crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/pipe/declaration/optional_keys.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_kind_passes_declarations_without_cargo_and_keeps_the_schema", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_names_every_missing_key", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_requirements_is_an_optional_repo_relative_path", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_terminal_land_remote_is_an_optional_single_word", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_terminal_land_ci_cmd_must_carry_the_sha_hole", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_declaration_default_ci_cmd_carries_the_event_field"]
size = "S"
done = "(1) 17 item（DECLARED_KEYS / OPTIONAL_KEYS / REMOTE_KEY / CI_CMD_KEY / DEFAULT_CI_CMD / CI_SHA_HOLE / REQUIREMENTS_KEY / DEFAULT_REQUIREMENTS / requirements_of / remote_of / ci_cmd_of / repo_relative / TableFacts / table_facts / table_facts_named / TerminalFacts / terminal_facts）が + の file に名・本文・順序のまま在り、純移動の証明が moved=17 の要約を返す (2) 親に増えるのは mod 宣言 1 行・pub use 2 行・私有の use 1 行だけで、親の本体と、群を declaration の path で引く外の 6 file は 1 字も変わらない (3) 可視性を上げるのは子の側の pub(super) 5 つ（DECLARED_KEYS / OPTIONAL_KEYS / requirements_of / remote_of / ci_cmd_of）だけで、親の側は 1 語も変わらない (4) in-file の歯 23 本の名・本文・use が不変で全部緑、札 flip-check: moved が親の mod tests の最後の行（閉じの } の直前）に 1 行在り、入口の flip-check が moved=1 で rc 0 (5) 兄弟の子 module 3 本（entrance_flip / path_kinds / write_set）が 1 字も変わらない (6) file-lines で declaration.rs の余地が base の 14 から 150 以上へ増える"

[[contract]]
id = "bc"
title = "main-provenance が器の便でない先端の commit に発端の trailer を要る — 本文を run / source / none に分ける 1 関数（key は NAME から導く・行頭の key の行が形に合うのは bead id を半角空白 1 つで区切って 1 本以上のときだけ・形の外が 1 本でも在れば none）で、件名の (#N) と発端の行の両方を持つ commit だけを via=pr で通し、(#N) だけの commit を no-source で名指して rc 1 にし、run の trailer の commit は今どおり通す — 2 つの読み手が同じ見本 file を読む（xtask・core の外・先端だけを測る形と CI の job は不変・FR92 / AC62）"
req = ["FR92"]
section = "60"
write-set = ["crates/xtask/src/provenance.rs", "+crates/xtask/src/source_trailer_cases.txt", ".github/workflows/ci.yml", "=crates/xtask/src/workspace.rs", "=crates/xtask/src/flipcheck.rs", "=crates/scribe2/src/pipe/land/finish.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail main_provenance_source_", "cargo nextest run -p xtask --no-tests=fail main_provenance_accepts_land_trailer_run_id", "cargo nextest run -p xtask --no-tests=fail main_provenance_refuses_head_without_either_entrance", "cargo nextest run -p xtask --no-tests=fail main_provenance_rev_argument_forms"]
size = "S"
growth = ["crates/xtask/src/provenance.rs:150"]
done = "(1) 本文（2 行目から後ろ）を run / source / none に分ける 1 関数が、形の外の発端の行を 1 本でも持つ本文を none、そうでなく is_run_id の形の run: の行を持つ本文を run、そうでなく発端の行を 1 本以上持つ本文を source（id は全部の発端の行の id を出てきた順）、どれでもなければ none に分け、判定行は run が ok via=land run=<id>、source で件名が (#N) で終わる commit が ok via=pr number=<N> source=<id,…>、none で (#N) の commit が FAIL reason=no-source number=<N>、(#N) で終わらず run でもない commit が FAIL reason=no-provenance で、どれも末尾 sha=<sha>・rc は 0 / 1 (2) 発端の行は行頭から key（先頭を大文字にした NAME と -Source:）で始まる行で、字下げと大小の違う行は発端の行でなく、形に合うのは末尾の CR と半角空白と tab を落とした字が key・半角空白 1 つ・is_bead_id の形の id を半角空白 1 つで区切って 1 本以上だけのとき (3) 行 bc の + の見本 file（行頭 # の注・行 === の区切り・塊の 1 行目の期待の語・2 行目からの本文）が設計 §60 形 3 の 24 塊以上を持ち、xtask の歯が全塊を読んで分けた語が期待と全部一致し、その file は core の歯からも workspace の path で読める (4) run は cwd の workspace を Layout で読んで NAME を得て key を組み、NAME を読めない周は git を撃てない周と同じく判定行なしの rc 2 (5) .github/workflows/ci.yml は job の頭の注だけが本 § と vessel-hook.md §21 を指す形に変わり（job・撃つ時機・checkout の深さは不変）、main.rs の分岐と USAGE・is_run_id・is_bead_id は変わらず、provenance.rs の頭の doc が入口の 3 形と発端の行の読みを書く 歯: main_provenance_source_ の (a) 見本の全塊の語が期待と一致し塊は 24 以上で 3 語が全部出る（key は本 repo の workspace の NAME から組む）・CR LF の行の本文を歯の中で組んだ本文も source(b) 件名が (#542) で終わり発端の行の無い message が FAIL reason=no-source number=542 sha=abc123・rc 1（今の (#N) だけで通る歯を書き換えたもの）(c) (#810) の件名と Scribe2-Source: s2-07l.739 の本文が ok via=pr number=810 source=s2-07l.739・rc 0、Scribe2-Source: s2-07l.739 s2-07l.722 の本文が source=s2-07l.739,s2-07l.722・rc 0 (d) (#N) の無い件名と形に合う発端の行の本文が FAIL reason=no-provenance・rc 1（回帰の歯・base でも緑）(e) 名 fixturename で組んだ key で Fixturename-Source: s2-a.1 が source・Scribe2-Source: s2-a.1 が none、既存の歯 main_provenance_accepts_land_trailer_run_id・main_provenance_refuses_head_without_either_entrance は 2 つの判定の関数の呼び出しに歯の中で NAME から組んだ key を足すだけで message の列と期待値と名を変えずに緑、main_provenance_rev_argument_forms は本文を変えずに緑、判定の関数は key を引数に取る純な関数で NAME を読むのは run の 1 か所だけ、歯は NAME の小文字の字を引用符で書かない（name-literal の門）・base は歯の区間が新しい関数を呼んで compile されず、挙動でも (b) が ok を返し (c) が source= を持たないので RED"
[[contract]]
id = "bd"
title = "runner と先撃ちの lens は Sonnet・契約の審査と gate の lens は Opus — rules 行 runner.model を sonnet に替え、lens.model（opus）と pipe.precheck_lens_model（sonnet）を足し、lens は --stage prelens の有無で読む行を選び、先撃ちは lens の行の末尾に --stage prelens を足し、形 ac 1 の使い回しは 2 行が同じ model の時だけ（effort の行は共通のまま・裁定 user 2026-09-29T07:44Z）"
req = ["FR5", "FR9", "FR49"]
section = "61"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/pipe/dispatch/prelens.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/headless/runner.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail model_split_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_manifest_carries_runner_model", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_manifest_carries_role_defaults", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_reuse_same_material_and_lens_copies_the_verdict_without_firing"]
size = "M"
done = "(1) 埋め込みの manifest の runner.model が sonnet・裁定 id の頭 user 2026-09-29T07:44Z・裁定日 2026-09-29 で、lens.model（LensModel・Str・opus）と pipe.precheck_lens_model（PipePrecheckLensModel・Str・sonnet）が同じ裁定で runner.effort の直後に在り、kind の宣言順が RunnerModel → RunnerEffort → LensModel → PipePrecheckLensModel → RoleModel、行数と kind の数が 2 ずつ増え、runner.effort は値も裁定も不変 (2) headless の model の読み口は行の id を引数に取る 1 関数で 3 行を読み、行が無い・不発効・文字列でない・閉じた表に無い周は claude を呼ばず rc 2 (3) lens は --stage の無い周に lens.model、--stage prelens の周に pipe.precheck_lens_model の値を --model に渡し、prelens 以外の値と値の欠けは未知の引数と同じ断りで claude を呼ばず、usage の 1 行と help.rs の lens の form と flags の列が [--stage prelens] を写し、--rules の無い lens は --stage 無しで opus・--stage prelens で sonnet、--rules の無い runner は sonnet を渡す (4) 先撃ちの lens の argv の末尾 2 語が --stage prelens で、置き場の file lens の字は穴を埋める前の cmd のまま (5) 形 ac 1 の使い回しは pipe run の審査が読む manifest の lens.model と pipe.precheck_lens_model が両方読めて同じ Model に解ける時だけ起き、違う周と片方が読めない周は Reviewed の段が lens を撃って detail に prelens:reused を持たず、同じ周は今どおり使い回す (6) 直す既存の歯と fixture の helper が新しい値と行で緑で、base でも緑になる file は同じ file に base で赤い新しい歯を持つか test 区間の行頭に retroactive の札を持つ"

[[contract]]
id = "be"
title = "着地は、便の base の宣言が ruling-check = true の便の差分が足す解けない裁定 id と線の後の時刻の形・問い id の形を持たない判断の欄 3 種・問い id を足さない ruling-check の外しを、main も PR も変えず Gated に留め、RunStage detail=held:FR83:<名指し> を同じ理由につき 1 件だけ記帳し、Failed にせず撃ち直しの上限に数えず、列から外し、解けた周に released:FR83 を 1 件記帳して戻す（FR83・FR10 / AC53）"
req = ["FR83", "FR10"]
section = "62"
write-set = ["+crates/scribe2/src/pipe/land/ruling_hold.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/train.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "=crates/scribe2/src/pipe/declaration/optional_keys.rs", "=crates/scribe2/src/pipe/dispatch.rs", "=crates/scribe2/src/pipe/gate/lens.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_ruling_hold_", "cargo nextest run -p scribe2 --lib --no-tests=fail hold_diff_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_held_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_train_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_turn_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_pr_cmd_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/land.rs:12", "crates/scribe2/src/pipe/queue.rs:22", "crates/scribe2/src/pipe/train.rs:6"]
done = "(1) base の宣言が ruling-check = true の便で、差分が足す 3 形（解けない問い id の形・解けない batch: か policy: の形・線の後の時刻の形）・問い id の形の無い判断の欄 3 種・接頭辞違いだけの判断の欄・問い id を足さない ruling-check の外しの 8 形が、main を動かさず PR も開かず Gated に留まる (2) 理由の event は RunStage Gated の held:FR83:<並べ替えた名指し> で、同じ理由の周を何度撃っても 1 件のまま、名指しが変わった周は新しい 1 件。retries を超える周を回しても Failed が無い (3) 線の前の引用・key の無い repo・解ける 4 形は着地し、released を記帳しない (4) 留めの後に台帳が解けた周は released:FR83 を 1 件記帳して着地する (5) 列の読みが held の便を番と後続の列から外し、released の後に戻す。候補の木の先頭は当たる後続を積まない (6) 台帳を読めない周と、差分の header の path を読めない file に足した行が在る周は unmeasured の名指しで留め、通さない。差分は core.quotePath=false で撃ち、引用符で囲まれた +++ の header の file の足した行も escape を戻した path で判じる (7) 列の読みは detail の頭 held: と released: を FR の語に依らず読む (8) 候補の木の先頭が当たる後続を積まないことを、e2e の pipe_land_ruling_hold_ の歯 1 本（3 本の列の 2 本目だけが留めに当たり、先頭の land が 1 本目と 3 本目を着地させ、2 本目は Gated のまま main に載らず event が増えない）で測る (9) 子 module は Land・Stage・EventKind・Issue を名指さず、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "bf"
title = "未反映の裁定に関わる便を着地の周に FR10 の留めに掛ける — 置き場の関わる契約の表に bead が在る便を Gated に留め、原因 FR84 と id の event を便ごとに 1 件、上限に数えず Failed にしない（§63）"
req = ["FR10", "FR84"]
section = "63"
depends = ["be"]
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/train.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "=crates/scribe2/src/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_unreflected_"]
size = "S"
growth = ["crates/scribe2/src/pipe/land.rs:25"]
done = "(1) 表に bead が在る便は Gated に留まり main は動かない (2) detail は held:FR84:<表がその便の bead に結ぶ裁定 id> で、同じ名指しの event は 1 件（3 周撃っても 1 件） (3) 上限の回数を越えて回しても Failed にならない (4) 表から消えた後の周に released:FR84 を 1 件記帳して着地する (5) 置き場の file が無い周は留めず、在るのに読めない周だけ held:FR84:unmeasured で留める (6) ruling-check = true の repo で FR83 の判定が当たらない周も、最後の held が held:FR84: の便には released:FR83 を記帳せず、表から消えた周の released:FR84 だけを書く (7) 候補の木の先頭は、表に bead が在る後続の便を積まず、表に無い後続は積む"

[[contract]]
id = "bg"
title = "審査役の claude を読みの道具だけで起こす — lens は渡された permission mode に依らず dontAsk と --tools Read,Grep,Glob を毎回渡して値の違う周は 1 語の記録を残し、器が起こす claude は口座の自動 memory を読まず、契約の審査は審査の時点の HEAD の木を・先撃ちは予想の木を cwd に読み、予想の判定を Reviewed に写さず、雛形 2 本の「tool が渡されていない」を消す（§64）"
req = ["FR5", "FR9", "FR49"]
section = "64"
write-set = ["crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/headless/lens.txt", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/dispatch/prelens.rs", "crates/scribe2/src/pipe/dispatch/floor.rs", "crates/scribe2/src/help.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/headless/runner.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_promise_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_read_", "cargo nextest run -p scribe2 --lib --no-tests=fail lens_read_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_prompt_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_contract_prompt_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_lens_promise_prompt_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_pass_spawns"]
size = "L"
growth = ["crates/scribe2/src/pipe/review.rs:50", "crates/scribe2/src/headless/mod.rs:15", "crates/scribe2/src/headless/lens.rs:12", "crates/scribe2/src/headless/runner.rs:2", "crates/scribe2/src/fleet/usage.rs:2", "crates/scribe2/src/help.rs:2", "crates/scribe2/src/pipe/dispatch/prelens.rs:25", "crates/scribe2/src/pipe/dispatch/floor.rs:4"]
done = "(1) lens は渡された permission mode に依らず permission mode dontAsk を毎回明示し、argv に --tools と値 Read,Grep,Glob の対をちょうど 1 つ持ち --allowedTools を持たず、--permission-mode の無い周も claude を呼んで rc 0 で判定を返す〔lens_read_ の (a)(b)〕 (2) dontAsk でない値（acceptEdits・plan）が渡された周は --contract の file の dir の lens.ignored の字が ignored: に値を続けた 1 行で、dontAsk の周と flag の無い周はその file が無い（前の周の file を置いた dir でも消える）〔(b)〕 (3) lens の usage の 1 行の --permission-mode M が任意の flag の形〔外形の snapshot headless_external_form〕 (4) runner と lens の子の env の自動 memory の値が 1 で親の値を継承せず、runner の argv は --tools を持たず permission mode と --allowedTools は今のまま〔(d)(e)(h)〕 (5) 契約の審査の lens の {worktree} と wrapper の cwd は、審査の前に読んだ repo の HEAD の sha に detach した run dir の直下の <sha>.tree で、review.json の tree がその sha、判定の後にその dir も worktree の登録も無く、材料の dir の名の列は今のまま〔(f)〕 (6) 木の path を worktree でない file が塞ぐ周は lens を撃たず INCONCLUSIVE で evidence が審査の木を名指し、同じ path に前の周の登録済みの worktree が残る周はそれを外して審査が進む〔(g)(g2)〕 (7) 使い回せる先撃ちの判定を持つ周は木を作らず、木の path が塞がれていても使い回しの判定になる〔(g3)〕 (8) 契約の審査の雛形と gate の雛形で組んだ prompt に「読みの道具（Read・Grep・Glob）」が在り「tool が渡されていない」が 0 件〔(c)・外形の snapshot lens_prompt_external_form・lens_contract_prompt_external_form・lens_promise_prompt_external_form〕 (9) 先撃ちの lens の {worktree} と cwd は置き場の tree で祖先の層の file を持ち、撃ち中の間の周は tree を外さず、lens が終わった後の周に tree も登録も無い〔(i)(j)〕 (10) Reviewed は置き場の tree.sha が <審査の木の sha> actual の先撃ちの判定だけを写し、forecast と別の sha の判定では lens を撃つ〔(k)〕 (11) 先撃ちが判定を撃った後に main が動いて組み直した置き場は、判定と tree.sha が撃った木を指し続け、今の HEAD の契約の審査は lens を撃つ〔(l)〕"

[[contract]]
id = "bh"
title = "runner の stdin の契約の直後に「## 共通 verify」節を足し、便の写しの common-verify を 4 つの穴（{base} は base の sha・{jobs} と {threads} は 1・{teeth} は契約の verify 行の filter 語）を gate と同じ埋め方で埋めた字で並べ、runner.txt は commit の後にその行を全部撃って緑にしてから終えると読む（§65）"
req = ["FR4", "FR8"]
section = "65"
write-set = ["crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_common_section_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_runner_prompt_external_form"]
size = "M"
growth = ["crates/scribe2/src/pipe/spawn.rs:30", "crates/scribe2/src/pipe/gate/verify.rs:4"]
done = "(1) 回答済みの質問を持つ便の runner の stdin は、契約の本文の後・回答の節の前に「## 共通 verify」節を持ち、写しの common-verify の各行を、{base} は記録した base の sha・{jobs} と {threads} は 1・{teeth} は契約の verify 行の filter 語で埋めた字で 1 行 1 項目に並べ、穴の字 { が残らない〔runner_common_section_ の (a)〕 (2) 写しを読めない便の節の本文は読めない理由の 1 行で、段は Implemented まで進む〔(b)〕 (3) common-verify が 0 行の写しの便の節の本文は なし の 1 行〔(c)〕 (4) runner.txt の守ることに、節の行は gate が契約の verify 行の前に撃つ行で、commit の後に全部撃って緑にしてから turn を閉じ、長い行は Bash の timeout を上限 600000 ms まで明示し、緑にできない行は最後に 1 行で述べる、の項目が在る〔外形の snapshot headless_runner_prompt_external_form〕"

[[contract]]
id = "bi"
title = "lens の turn の上限 — rules 行 lens.max_turns（30）を足し、lens は段に依らず argv に --max-turns でその値を毎回渡し（行が無い・不発効・整数でない・0 の周は claude を呼ばず rc 2）、封筒の subtype が error_max_turns の周は result を読まずに turn の上限を名指す INCONCLUSIVE にして gate の撃ち直しの 1 回に乗せる（§67）"
req = ["FR5", "FR9"]
section = "67"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/headless/lens.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_turns_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds"]
size = "M"
growth = ["crates/scribe2/src/headless/lens.rs:25", "crates/scribe2/src/rules/mod.rs:6", "crates/scribe2/src/headless/mod.rs:2"]
done = "(1) lens の argv は rules 行 lens.max_turns の値を --max-turns の直後に置いた対をちょうど 1 つ持ち（値 7 と 30 の manifest で弁別）、--stage prelens と --stage memo の lens も同じ対を持ち、--rules の無い lens は埋め込みの 30〔lens_turns_ の (a)〕 (2) 行が無い・不発効・値 0 の manifest の lens は claude を呼ばず rc 2 で、stderr が lens: lens.max_turns で始まる理由の 1 行〔(b)〕 (3) 封筒の subtype が error_max_turns の周は、result に findings と population を持つ PASS の判定の行が在る周も result の無い周も、判定が INCONCLUSIVE で evidence が lens.max_turns を含み key turns を持つ〔(c)〕 (4) gate の lens を器の lens と上限で終わる偽 claude にした便は INCONCLUSIVE で、偽 claude の回数が 2、撃ち直しの行が 1 本で理由に lens.max_turns を含む〔(d)〕 (5) 埋め込み manifest の lens.max_turns の行は値 30・kind LensMaxTurns・発効・裁定 id user 2026-09-30T22:13Z 項 lens-turns・裁定日 2026-09-30・形 Int で、kind は RunTokenCeiling の直後〔(e)〕 (6) 埋め込み manifest の行数の pin と外形の snapshot rules_external_form の rows と kinds が base より 1 つ多い〔rules_embedded_manifest_is_valid_and_covers_all_kinds・rules_external_form〕"

[[contract]]
id = "bj"
title = "終わりの門 — runner の turn が rc 0 かつ commit 1 本以上で終わった周に、器が gate と同じ撃ち手と受付で gate の write-set を照らす段・共通 verify・契約の検証行を便の worktree に撃って end-gate.jsonl に残し、赤で測れなかった行が無く rules 行 runner.end_gate_rounds（2）の回数が残る周は段を Spawned のまま門の赤を記帳して同じ worktree と base で runner を起こし直し stdin に「## 門の赤」節を渡し、使い切った周と測れなかった行の在る周は Implemented にして gate へ進める（§66 形 1〜8・10）"
req = ["FR98", "FR6", "FR8"]
section = "66"
touches = ["crate::rules::RuleKind", "crate::pipe::follow::Runner", "crate::pipe::spawn::Launch"]
depends = ["bi"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/land.rs", "crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate/confine.rs", "crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail end_gate_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds"]
size = "L"
growth = ["crates/scribe2/src/pipe/spawn.rs:170", "crates/scribe2/src/pipe/follow.rs:35", "crates/scribe2/src/pipe/gate/record.rs:25", "crates/scribe2/src/rules/mod.rs:6", "crates/scribe2/src/pipe/gate.rs:3", "crates/scribe2/src/pipe/cli/run.rs:3", "crates/scribe2/src/pipe/cli/step.rs:3", "crates/scribe2/src/pipe/ratelimit.rs:3"]
done = "(1) 1 周目に赤い中身を commit し「## 門の赤」節の在る周に直す runner の便は、spawn の 1 回で runner が 2 回起き、2 回目の stdin に節と赤い行の字と rc=1 と stub の語が在り（1 回目には節が無い）、段の記帳の列が Spawned（base:）・Spawned（end-gate:red:1:1）・Spawned（end-gate:1）・Implemented（detail なし）の順で、2 回目の木の HEAD の祖先に 1 回目の commit が在り、end-gate.jsonl の要約の語が red・green の順〔end_gate_ の (a)〕 (2) 直さない runner の便は runner が 3 回・門の赤の記帳 2 件で Implemented（要約 exhausted）、その便の gate は FAIL で verify.jsonl の record の数が行の値 0 の manifest で通した便と同じで、値 0 の便は runner が 1 回で end-gate.jsonl が要約の 1 行（off）だけ〔(b)〕 (3) 赤い行と一緒に箱の中で死ぬ行を持つ便と、遮断器の閉じる manifest で起こす赤い行を持つ便は、runner が 1 回・門の赤の記帳 0 件で Implemented（要約 unmeasured）〔(c)〕 (4) 全行が緑の便は runner が 1 回で Implemented の detail が無く、要約 green で、門の record の数が gate の段の数と同じ〔(d)〕 (5) 埋め込み manifest の runner.end_gate_rounds の行は値 2・kind RunnerEndGateRounds・発効・裁定 id user 2026-09-30T22:13Z 項 end-gate・裁定日 2026-09-30・形 Int で kind は FollowRetries の直後、行の無い manifest で起こす赤い行を持つ便は runner が 1 回で要約 unmeasured の理由が行の id を名指す〔(e)〕 (6) 埋め込み manifest の行数の pin と外形の snapshot rules_external_form の rows と kinds が base より 1 つ多い〔rules_embedded_manifest_is_valid_and_covers_all_kinds・rules_external_form〕"

[[contract]]
id = "bk"
title = "終わりの門の間の印 — 門を撃ち始める process が run dir に end-gate.pid（pid と起動時刻）を置いて輪を抜ける時に外し、pipe resume の Spawned の枝は持ち主の死んでいない印の周を end-gate=alive で断り、門の間に止められた便は門の後に段を書かず起こし直さない（§66 形 9）"
req = ["FR98"]
section = "66"
depends = ["bj"]
write-set = ["crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail endgate_mark_"]
size = "S"
growth = ["crates/scribe2/src/pipe/spawn.rs:30", "crates/scribe2/src/pipe/follow.rs:10", "crates/scribe2/src/pipe/cli/resume.rs:25"]
done = "(1) 門の間に撃った pipe resume --run <id> --runner <cmd> は rc 1 で判定行に end-gate=alive を持ち、SeatSpawned を足さず runner を起こさず、門を抜けた spawn は Implemented で run dir に end-gate.pid が無い〔endgate_mark_ の (a)〕 (2) 門の間に撃った pipe stop --run <id> の後、門を抜けた spawn は段を書かず最後の event が RunStopped で、runner は 1 回だけ起きる〔(b)〕"
<!-- contracts:end -->
