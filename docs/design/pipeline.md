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
5. **plugin の root `<state_dir>/pipe/<run>/plugin/` を組む**（§6 の runner がこの root の配下を 1 dir = 1 plugin として claude の `--plugin-dir` に渡す）: (i) **器の plugin**（binary に埋め込んだ `.claude-plugin/plugin.json` と `hooks/hooks.json`＝`gen-manifest` の生成物と同じ bytes）を `plugin/<NAME>/` に**必ず**書く＝plugin を持たない consumer repo でも hook 側の in-loop guard 3 本（write-set guard・permission の deny・cap guard）が便に載る（憲法 C16 / C16.2・`s2-07l.149` 裁定 (A)・2026-09-13）。(ii) worktree に `.claude-plugin/plugin.json` と `hooks/hooks.json` が**両方**在り、その plugin.json の `name` の値（既存の JSON 読み手で取る）が `NAME` と**違う**ときだけ、worktree の 2 dir を `plugin/consumer/` へ写す（file だけ・**symlink は追わない**＝dir 自体が link の面も写さない・写すのは **worktree の**中身＝便の base の内容であって anchor の現在値ではない）。`name` が `NAME` と同じ周は器自身の repo＝世代がずれていても器の 1 本だけを載せ、同じ hook を 2 度走らせない。片方だけ在る周・`name` が読めない周は consumer の plugin とは見ない（写さない）。再走のため root を先に空にする。
6. `RunStage stage=Spawned` → runner を `sh -c <cmd>` で **cwd = worktree** で起動し `SeatSpawned seat=<run> pid=<pid>`。runner の **stdin には契約の写し（`contract.toml`・再読）を流し**、回答済みの質問からの再 spawn ではその末尾に「## 回答」節（`QuestionRaised.detail` と `QuestionAnswered.detail` の対）を付ける。**stdout は捕らえる**（`gate.rs::ask_lens` と同じ piped + `wait_with_output`・質問 record の読み面）。cmd 中の placeholder `{run}` `{worktree}` `{contract}` `{write_set}` `{base}` `{plugin_dir}`（= 手順 5 の plugin root）`{vessel}`（= §5.1 の Effective の写し `vessel.toml`・[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)）を置換する。**scribe2 固有の env は 1 つも足さない**（親の env はそのまま継承・ADR-0004 §2.4）。
7. **runner の終了待ちは `wait_with_output`**（rc と stdout を運ぶ・待機の実装は増えない）。rc が `RC_QUESTION`（76）の周だけ stdout の最終 JSON 行を質問 record（`question` 必須非空 1 行・`about` 任意）として読み、commit 0 なら `QuestionRaised` + `RunStage(Questioned)` を記帳して `run=<id> stage=Questioned question=<id>` を出し **rc 3** で止まる。record が無い / 読めない周は `Failed detail=question-record-missing:<理由>`、record と commit が同時の周は `Failed detail=runner-rc:76,commits:<n>`。rc が 76 でない周は最終行を読まない。捕らえた stdout は**全文を `<state_dir>/pipe/<run>/runner.stdout.log` へ見出し行（`## <ts> rc=<rc>`）付きで append する**（包みの観測行 `runner: rc=… records=… observed=…` を端末から消さない・機械は読まない診断 file・空の周は書かない）。pid の生存待ち（`stop`）は `wait(Completion::SeatGone)`（[fleet-event-log.md §4](./fleet-event-log.md)）で、`Completion::RunnerExited` は**別 process が spawn した runner を待つ resume 経路のために残す**。`SeatStopped`。**rc 0 ∧ `git rev-list --count <base>..HEAD` ≥ 1** → `Implemented`、それ以外 → `Failed detail=runner-rc:<rc>,commits:<n>`（commit 0 は完了ではない）。stdout `run=<id> stage=<s>`。

### 5.3 gate（(b)・FR8 / FR9 / NFR1）
- 資源の受付（実効 jobs）・子 process の封じ込め・`detection-verify` の段（①②③④）は [gate-cost.md](./gate-cost.md) §3〜§5（ADR-0021 §2.6 の部分 supersede）が本節を上書きする（**契約 land 後**・本節の①②③と `{base}` 唯一の穴は land 前の現物と一致）。
`pipe gate --run <id> [--lens <cmd>]`: 前提 = **`Implemented` ∨ (`Gated` ∧ verdict が INCONCLUSIVE)** ∧ worktree clean（`git status --porcelain` 空）∧ commits ≥ 1。**違反の扱いは 2 通りに分ける**（いずれも rc 1 で lens は起動しない）。
- **worktree の事実**（clean でない / commits 0）の違反 → `RunStage stage=Failed detail=precheck:<理由>`。実装が済んだと名乗る便の中身が前提を満たしていない＝その便はここで終わる。
- **段違い**（`Implemented` でも `Gated(INCONCLUSIVE)` でもない）→ §4 の一般則どおり **何もせず rc 1**（event を 1 件も書かない）。gate を早く叩いただけの便を `Failed` で終端させると、`resume` が引けなくなる（`Failed` からは再開しない）。
- **測り直し**（`Gated` ∧ INCONCLUSIVE・FR14）: INCONCLUSIVE は道具が足りず判定に届かなかった印（`--lens` 無し / diff が cap 超 / lens の不備）ゆえ、道具を揃えて**同じ便を撃ち直せる**。`verdict.json` は最後の判定で上書きし、`RunStage stage=Gated detail=verdict:<V>` は**追記**する（append-only＝1 度目の INCONCLUSIVE が残る）。**PASS / FAIL は終端**（判定に届いた周＝撃ち直す口を開けない。verify が赤い便は測り直しても赤い＝「壊れたまま進まず」GOAL 2）。**測り直しの周も worktree の事実の違反は `Failed` で終端する**（道具を揃える前に worktree を clean へ戻す）——precheck の極性を段ごとに分けると「段の検査は入口 / worktree の事実は gate」の分離が濁るためで、終端しても worktree と branch は残る（N1）＝作り直せるのは run 1 本の側である。verdict を読む関数は `pipe/land.rs` の `verdict_of` の 1 本で、land の前提・gate の入口・resume の行き先が同じ値を見る（**判定が読めない周**——file 不在 / JSON が壊れ / 3 値の外——は INCONCLUSIVE と同じ扱いにせず断る）。残るのは **3 値の履歴だけ**で、1 度目の evidence（なぜ測れなかったか）は `verdict.json` の上書きで消える。`verify.jsonl` は同じ file へ 2 周目を**追記**する（`n` は周ごとに 1 から＝file 内で一意ではない。INCONCLUSIVE の周は lens に届く前で止まっているので古い行が偽の RED を作ることはない。段①が読めない周は `verify_red > 0` の INCONCLUSIVE になりうる＝測れなかったは赤より先・`s2-07l.65`）。**同一便へ `pipe gate` を並行して撃たない**（`verdict.json` の write は event の lock の外にあり、file の最終内容と最終 event が別の周を指しうる）。
- **機械検証**（順序は [ADR-0009 §2.4](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-4-common-verify)）: ① **write-set の照合**を Rust の 1 関数で行う（`git diff --name-only <base>..HEAD` の各 path が契約 write-set のいずれか〔file 一致 or dir prefix〕に含まれる・外れが 1 件でも赤・外れた path を `verify.stderr.log` に列挙。**diff の path を読めない周は赤ではなく「測れなかった」**＝record は残し（rc は u64 の記録形 255・`verify.stderr.log` の見出しは -1）、②③ は従来どおり撃って record し（費用は ②③ 分・record を欠かさないため）、判定は lens を呼ばずに INCONCLUSIVE〔既存 3 値の内側〕へ倒す。land の `main-unmeasured` と同じく「赤ではない」側だが、land が `Failed` で終端するのに対し gate は `Gated` に留まり測り直せる（FR14）。段①の -1 は `verify_red` に数えない・`s2-07l.65`）→ ② run の写し `vessel.toml` の `common-verify` 各行（[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)・manifest の行ではない・`{base}` を便の base に置換・repo 共通の検証＝Rust repo なら flip check・test・lint・依存監査）→ ③ 写しの `detection-verify` 各行（任意 key・検出線＝変異検出・穴は②と同じ・rc 1〔R-C12-1 が deny に昇格した周だけ現れる〕は②と同じく赤・**rc 2〔測れなかった・道具の不在・baseline 落ち〕は赤に数えず判定を INCONCLUSIVE へ倒す**〔他の行の赤が 0 の周だけ・赤が在る周は FAIL が先＝[gate-cost.md](./gate-cost.md) §28・`Gated` に留まり測り直せる・FAIL にして runner をもう 1 周払わせない・`s2-07l.331`〕・木が gate と同じ main 実測では撃ち直さない・[gate-cost.md](./gate-cost.md) §5・ADR-0021 §2.4）→ ④ contract の `verify` 各行（便固有の行だけ）。**① は Rust の照合で `sh -c` を撃たない**（record の `cmd` は段の名 `write-set`）・②③④ は worktree で `sh -c` 実行し、①〜④を**通し番号 `n`** で並べて行ごとの rc を `<state_dir>/pipe/<run>/verify.jsonl`（`{"schema":1,"n":<i>,"rc":<rc>,"cmd":"…"}`）に逐条記録。**rc≠0 の行だけ、その行の stderr の末尾 20 行を `<state_dir>/pipe/<run>/verify.stderr.log` へ見出し行（`## n=<i> rc=<rc> cmd=<cmd>`）付きで append する**——rc だけでは「何がどう赤いか」が便の外から読めず、落ちるたびに人が同じ行を手で撃ち直して理由を取り直すことになる。`verify.jsonl` の record の形は変えない（跨版の契約ゆえ不変）＝`verify.stderr.log` は**機械が読まない診断 file** で、緑の行は残さない（読む理由の無い出力で埋めると赤い行の見出しが埋もれる）。
- **lens**: 本数 = rules 行 `gate.lens_count`（MVP は 1）、cap = `gate.token_cap`。**本数は照合する**: `gate.lens_count` が 1 でない周（0 = lens を呼ばずに通す / 2 以上 = 1 本で足りたことにする）は「lens の verdict」を得ていないので **INCONCLUSIVE**（多 lens は (b) の射程外なので、実装しない代わりに fail-closed に断る）。`--lens <cmd>` に `git diff <base>..HEAD` を stdin で渡し、stdout の JSON 1 行 `{"verdict":"PASS|FAIL|INCONCLUSIVE","evidence":"…"}` を採る。**cmd の `{contract}` / `{worktree}` は run の path へ置換する**（`--runner` 側と共有するのは placeholder の語彙であって置換関数ではない・置く穴は **2 つ**〔`{contract}` / `{worktree}`〕・出所 `s2-07l.60`）。穴が 2 つ目を持つのは、lens に憲法（生成 file `docs/constitution.md`）を載せる経路が**起動 cwd 1 本**で、tracked file に絶対 path は書けない（PUBLIC repo）ため worktree を gate が埋めるほかないからである。置換は **1 走査**で行う（重ねて replace すると先に埋めた path の中の `{worktree}` まで展開されうる）——lens に問うのは「diff が**契約の**求めるものを満たすか」なので、diff だけを渡すと実 lens は「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる（実測 2026-09-10・`s2-07l.24` の実 5 便）。**渡すのは path であって本文ではない**（cmd は `sh -c` の 1 行ゆえ、本文を埋めると契約の中の引用符 1 つで cmd の構造が変わる）。**穴を持たない古い `--lens` は fail-closed に落ちる**——`{contract}` / `{worktree}` を書いていない cmd へ `<NAME> lens` を渡すと lens 自身が rc 1 で断り（`--worktree` は `--contract` と同じ必須 flag で、無ければ claude を起こさない＝憲法の載らない判定を出さない）、gate は判定順の 3 番目で INCONCLUSIVE にする（極性は正しいが、gate は lens の stderr を捨てる〔`Stdio::null()`〕ので evidence に残るのは「lens が rc 1 で終わった」だけ＝**理由は lens を手で 1 回叩いて読む**）。
- **予算の照合**（NFR1「diff byte と cap の照合」）: diff の byte 数を `gate.token_cap` と**直接比べる**（byte ≥ token の保守的な読み・換算係数を持たない）。diff byte > cap → **INCONCLUSIVE**（lens を起動しない）。
- **純移動の機械証明**（`s2-07l.266`・user 裁定 2026-09-14「それでよい。後で戻すのを忘れないで」＝分割便が cap に当たる問題の恒久解・cap を一時的に上げた `s2-07l.265` の対・戻しは `s2-07l.267`）: 予算の照合の**前**に、diff が**純移動**かを純関数（`pipe/move_proof.rs`・I/O は gate 側）で判定する。**item** = base と HEAD の「diff に現れる `.rs` file」で **列 0 から始まる宣言単位**（`fn` / `struct` / `enum` / `impl` / `trait` / `const` / `static` / `type` / `mod <name> {`・直前に連なる属性行と doc コメント〔`///` / `#[…]`〕を含む・終端は列 0 の `}` か次の item の開始＝入れ子〔`impl {}` の中の fn・inline `mod tests {}` の中の歯〕は外側の item 1 本の本文に含める）。本文の正規化は **行頭の indent の除去**と**コメント行の除外**（`s2-07l.294`・2026-09-14 の .286 = gate.rs の 3 module 分割が 2 周とも純移動と読まれなかった型: module を跨ぐ移動では doc コメントの intra-doc link の path〔`[`super::x`]` → `[`crate::…::x`]`〕の書き換えが常に要る＝コメントは挙動を持たないので、item の区間のうち `//` / `///` / `//!` で始まる行〔行頭の indent の後〕は hash に入れない。**札の字面**〔`// flip-check:` で始まる行〕だけは除外せず従来どおり残差の検査に掛ける〔`retroactive` を item の中に隠す形を作らない・`ForeignMarker`〕。除外したコメント行の差は item ごとに数えて要約に載せる〔「コメント行の差 N」・lens が読める〕）。行内の空白と文字列 literal は変えない（pane の字面を持つ歯の literal を潰さない）。**可視性の prefix**（`pub` / `pub(crate)` / `pub(super)`）は item の名の前から剥がして hash に入れず、剥がした前後を item ごとに記録する（子 module へ出した helper は必ず可視性が広がる＝`s2-07l.257` の「本文字面不変・可視性と改名のみ」と同じ扱い）。(名, 本文の hash) の**多重集合**が HEAD と base で一致（追加 0・削除 0・本文差 0）し、**移動した item が 1 つ以上**在り、**残差分**（両側の diff 行のうちどの item の区間にも入らない行）が `mod` / `use` / `pub use` / `#[path]` / `#[cfg(test)]` / `// flip-check: moved <id>` の宣言と札、**item に付かない裸のコメント行**（`//` / `//!` / `///`・module doc と区切り線）、空行だけなら**純移動**（`s2-07l.261` の diff = `pub(super)` 化 9 行 + `//!` 31 行 + 区切り 26 行がこの形に当たる＝本機構の出所の便を通す基準）。純移動の周は lens の入力を diff でなく**要約**（型 `MoveSummary`: file ごとの item の移動元 → 先と本数・行数・可視性が変わった item の一覧〔名 + 前 → 後〕・宣言と札の残差分〔逐語・小さい〕・「名 + 本文の多重集合が一致」の判定行 1 本）にし、雛形 `lens.txt` の `{diff}` の穴に**そのまま**入れる（雛形は変えない＝`lens_prompt_external_form` の snapshot は動かない・要約の先頭行が「これは diff ではなく純移動の要約である」と名乗る）。予算の照合は lens に渡す本文の byte で行い、`verdict.json` の `diff_bytes` は従来どおり diff の byte のまま（意味を変えない・要約の byte は判定行に出す）。lens への入力は閉じた型（`LensInput::Diff` / `LensInput::Summary`・C3.3 の判定入力）で運び、要約の本文は run dir に `lens-input.txt` として残す（事後に読める・NFR4）。純移動でない周は従来の diff。結果は gate の stdout の判定行に `lens-input=<diff|summary> bytes=<N>` として出す（gate の外形 snapshot が動く周は同じ便で更新）。**極性**（C11.2 / C16.2）: 純移動の誤判定は lens から diff を奪う側に倒れる（FailOpen・PostHoc）ので `Guard` に variant 1 つ（`MoveProof`）を足し、極性一覧に載せる（in-loop の本数は変わらない・行数 +1）。切り出しの立場は閉包（[contract-source.md](./contract-source.md) §3）と同じ**下界**（構文木を持たない・A3 の依存を足さない）: macro で生成する item・1 行に複数の item は純移動と判定しない（保守側に倒れ lens が diff を読む従来形になる）。item の中のコメント行だけの差は hash に入らない（上の正規化・`s2-07l.294`・要約に件数が載る）。歯: `s2-07l.261` と同型の fixture（1 file → 複数 file の移動・`pub(super)` 化・module doc・区切り線つき）で lens 入力が要約になり verdict が読める／本文を 1 行変えた fixture は純移動でなく diff が渡る／宣言と札とコメント以外の行が残る fixture も diff が渡る／移動 item 0 の fixture（宣言だけ）は純移動でない／要約の外形は snapshot／判定の純関数は in-file（可視性の剥がしと入れ子の切り出しを直接撃つ）。**持ち越しの札**（`s2-07l.362`・契約表の行 e）: base に元から在る `moved` 以外の札（`retroactive` 等）が item ごと head へ移る周は、両側で同じ字面（id まで）の札を**対にして**残差から外し、対の無い札（head だけの新規・id 違い・base だけの消えた札）だけを `ForeignMarker` にする＝移動の中に新しい札を隠せない意図は保ったまま、持ち越しを新規と読まない。要約は持ち越した札の本数を 1 行で名乗る。
- **判定順**（wildcard 無しの match）: 箱の中で殺された行（**検出線以外の行**の `oom_kill`・どの行でも包みごとの signal 死＝[gate-cost.md](./gate-cost.md) §4.2）が在る → **INCONCLUSIVE**（赤より先）／ 検出線の行（`kind=detection`）が rc 2（測れなかった）→ **INCONCLUSIVE**（赤より先・理由に行番号と「検出線が測れなかった」・`s2-07l.331`）／ verify に rc≠0 が 1 本でも（検出線の rc 2 を除く）→ **FAIL** ／ **この 2 つの順は [gate-cost.md](./gate-cost.md) §28 が入れ替える**（行 t の着地の後の形: 赤が 1 行でも在る周は検出線が rc 2 でも FAIL・検出線の rc 2 が INCONCLUSIVE に倒すのは赤が 0 の周だけ）／ diff byte > cap → **INCONCLUSIVE** ／ lens が要るのに `--lens` 無し・lens rc≠0・stdout が parse 不能・verdict が 3 値外 → **INCONCLUSIVE** ／ それ以外は lens の verdict。
- **記録の追加**（[gate-cost.md](./gate-cost.md) §5.1・**契約 land 後**）: `verify.jsonl` の `kind=detection` の行だけ `line=<stdout の末尾の非空 1 行・逐語>` を持つ（検出線の値・parse しない・無い周は欠く）。
- 結果は `<state_dir>/pipe/<run>/verdict.json`（`{"schema":1,"run":…,"verdict":…,"evidence":…,"verify_red":<n>,"diff_bytes":<n>,"ts":…}`）と `RunStage stage=Gated detail=verdict:<V>`。stdout `run=<id> verdict=<V>`・rc は PASS=0 / FAIL=1 / INCONCLUSIVE=3。

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
- runner / lens の子 process の封じ込め（cgroup scope）は [gate-cost.md](./gate-cost.md) §4（ADR-0021 §2.2）。

- **器が起こす claude は settings を 1 つも読まない**（[ADR-0011 §2.1](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-1-launch-form)）: claude の Command を組む `build` が `--setting-sources ""`（空＝user / project / local のどれも読まない）と `--strict-mcp-config` を**毎回**渡す（runner・lens 共通の唯一の構築点に置き、呼出側には置かない＝足し忘れた側が既定の全 source へ落ちる形を作らない）。`--settings`（追加読込・settings を消さない）と `--restricted`（Bash を外し人の承認へ倒す）は渡さない。checkout の `.claude/settings.json` の allow 規則は承認要求を出さず PermissionRequest hook を素通りし、`hooks` は checkout が process を起動する口になるので、**入力の段で断つ**（[ADR-0011 §1 / §2.3](../../design-intent/decisions/ADR-0011-vessel-launches-claude-without-settings.html#s2-3-no-fallback)・intake の字面検査で代替しない）。
- **model も毎回明示する**（`s2-07l.297`・user 裁定 2026-09-14T21:59Z）: 同じ `build` が `--model <値>` を runner・lens 共通に渡す（`--permission-mode` と同じ理由＝版の既定に従うと便が消費するモデル別窓が黙って変わり、便用の口座選定（[account-autonomy.md](./account-autonomy.md) §3）が数える窓とずれる）。値は rules 行 `runner.model`（`--rules` / 埋め込み・lens の cap と同じ読み口・runner にも `--rules` の seam を足す。写しの allowlist と common-verify は従来どおり写しから＝[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers) は動かない）。行が無い / 不発効 / 文字列でない周は claude を呼ばず rc 2（cap と同じ極性）。実測（2026-09-14・4 便の transcript）: `--model` 無しの runner / lens は `claude-opus-5` で走っていた＝行の初期値はその実測値で、挙動は変えずに選定の窓だけを合わせる。
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

- **判定クラス**（語彙の SSOT は `cargo xtask flip-check` の判定行そのもの＝`judge` が stdout へ出す 1 行。本節は意味だけを持ち、README は本節への pointer だけを持つ・ADR-0013 §2.1・`s2-07l.92`。判定行の書式は ADR-0013 §2.4 のとおり pin されていないので、ここが実装と食い違ったら実装が正）。判定行は 3 形: `RED-on-base ok tests_changed=N`（rc 0・免除・同梱・base 段の撃ち直しが在るときだけ `removed-only=N` / `retroactive=N` / `moved=N` / `decl=N` / `fixture=N` / `base-retried=N` を後置。flip が 0 本でも免除だけの便は `tests_changed=0` のこの形で通る）／`skip reason=no-rust-diff`（rc 0・変更に `.rs` が 1 本も無い＝docs-only の便。runner を撃たずに通す唯一の経路）／`FAIL reason=<理由>`（rc 1）。FAIL の理由は 4 語: `green-on-base`（overlay した歯が base で緑＝TDD の不履行。1 本ずつ撃つ周〔flip が 2 本以上、または宣言 file を同梱した周〕は `file=<rel>` で緑だった file を名指す）／`no-test-diff`（`.rs` は変わったが test 区間の差が 1 本も無く、免除の札も無い）／`not-flippable`（下）／`infra-error <理由>`（道具の失敗＝git / tar / cargo の spawn 失敗・base 自身の test が緑でない `base-not-green`・base で該当 test が 0 本の `no-tests-on-base`・runner が signal で死んだ `runner-killed-by-signal`。**測れなかった**であって赤ではない）。**base 段の撃ち直し**（`s2-07l.270`・負荷下の flaky の検出線）: base の素の runner が落ち、落ちた歯を runner の出力の `FAIL` 行（binary id と歯の名）から名指せる周は、**その歯だけ**を同じ base copy で **1 回だけ**撃ち直し、通れば base 緑と読んで判定行に `base-retried=N`（N = 撃ち直した歯の本数）を後置する。2 回目も落ちる・落ちた歯を 1 本も名指せない（compile error・signal・出力の形が読めない）・撃ち直しの rc が 0 でない周は従来どおり `base-not-green`（撃ち直しは緩める側なので狭く取る＝名指せない失敗を撃ち直しで緑に化けさせない）。stderr に `base-retry <binary>::<name>` を 1 行ずつ残す。**base の実体化**（`s2-07l.280`）= `git archive` の展開 + index（`git init` / `git add -A`）+ HEAD（共有 object store を alternates で読み、base の commit を `update-ref HEAD` で置く）＝base の tracked 集合を `git ls-files` で、宣言を `HEAD:<file>` で読める git repo（`contracts check` 等 tracked 集合と HEAD を読む歯が base で測れる・commit は作らない＝HEAD は base の sha そのもの・overlay は working tree にだけ書き index にも HEAD にも載せない）。引数の不正（`--base` の不在・空）だけは判定行を出さず rc 2（直上の mutants-diff の rc 2「測れなかった」とは別の意味）。stderr の行は判定ではなく、判定行を読む人のための診断である。
- **測れなかった便は「測れなかった」と言う**（`s2-07l.14`・reason 語彙を 3 つ足した）。いずれも fail-closed のままで、`skip` で rc 0 にする経路は持たない。
  - `not-flippable`: base に無い `.rs`（新規 module）の in-file 歯は、base 側に `mod` 宣言ごと存在せず compile されないので**構造的に測れない**。flip が 1 本も無く、そういう file が 1 本以上在る周は runner を撃たず `FAIL reason=not-flippable files=<rel,…>` rc 1（stderr に逃がし方 1 行）。`green-on-base`（TDD の不履行）と同じ札を貼らない。
  - `tests-removed-only`: overlay できる file で **HEAD の test 区間の行列が base の行列の部分列**（順序を保った行の削除だけで得られる）なら flip に数えず、stderr へ `not-flipped reason=tests-removed-only <rel>`。純粋な module 分割（歯の移動）が恒久 FAIL しないための門である。**`crates/*/src/**/*_tests.rs`（と tests という名の file）は名前で test file と見なし全体を写す**——`#[path]` で src 配下へ外出しした test module は `#[cfg(test)] mod` の形を持たず、名前で見なければ区間判定には src 区間だけの file に見え、そこへ足した歯が 1 本も測られない。**`#[test]` fn 名では数えない**——名前の集合で見ると本文の改変が免除される（`⊆` は「名前が同じで本文だけ変えた歯」を、真部分集合でも「1 本消して別の 1 本の本文を変えた file」を通す）。部分列なら 1 行でも足された / 書き換えられた時点で成立しない。
  - **宣言 file の同梱**（`s2-07l.41`）: flip した file のうち、test 区間の差分行が**すべて** `mod x;` 形（`pub` / `pub(crate)` 可）の file は「宣言 file」と呼び、**単独では撃たず**本体 file を撃つ木へ同梱する（判定行に `decl=N`・stderr に `decl-with-body <rel>`）。新規 module は宣言と本体が別 file に割れ、単独 overlay ではどちらの判定も意味を持たない（宣言だけ = 本体不在の `E0583` の偽 RED／本体だけ = base に宣言が無く compile 対象外の偽 GREEN・実測 2026-09-10 `s2-07l.38.2` が初発）。弁別は**差分行の字面だけ**で行い parser は足さない＝`mod` 以外の行が 1 行でも動いていれば宣言 file ではない（同梱は判定を緩める側なので狭く取る）。**宣言 file しか flip していない便は従来どおり単独で撃つ**（存在しない module を指す `E0583` は本当の RED である）。同梱は**本体 1 本を撃つ turn ごと**に、**その便が足した** `mod <name>;` 行のうちその時点の tree に本体が無いもの（同じ dir の `<name>.rs` か `<name>/mod.rs` で見る）を落として置く。`mod` 行**以外は 1 行も触らず**、**base に既に在った宣言行も落とさない**——base が緑である以上その本体は必ず在り、落とすと `#[path = "…"]` の属性行だけが孤児になって`expected item after attributes` の compile error＝**別の捏造 RED**を作る（lens-44 H1）。`#[path]` 付き module を**救う**わけではない（その便が足した `#[path]` 宣言は従来どおり測れない・M4）——便の宣言行を全部置くと、その turn ではまだ置かれていない兄弟 module の `E0583` が RED に化け、**本体がどちらも base で緑でも隠れる**（新規 module 2 本以上の便の fail-open・実測 2026-09-10・`s2-07l.44`）。絞ったうえで本体の歯が base で緑なら `green-on-base` のまま落ちる＝同梱は RED を捏造しない。
  - **歯の外の file の同梱**（`s2-07l.450`・§37）: flip した file のうち、その便で動いた行が 1 本も歯の中に無い file（fixture だけの差）は宣言 file と同じ側＝単独では撃たず本体を撃つ木へ同梱する（判定行に `fixture=N`・stderr に `not-flipped reason=outside-teeth <rel>`）。歯の中の行が動いた file と、同梱しか flip の無い便は従来どおり落ちる。
  - `moved`（`s2-07l.86`）: **歯を 1 本も足さず挙動も変えない純粋な移動**の便は、test 区間へ `// flip-check: moved <bead-id>` を 1 行置くと RED を要求されず、判定行に `moved=N` が載る。**`tests-removed-only` との弁別は「自動か明示か」**——あちらは test 区間の差が**削除だけ**（部分列）のとき機械が自動で通す門で、こちらは差が削除にならない便（歯が `check()` 越しの統合形で書かれていて、実装だけを module へ出した周）を、**書いた人が札 1 行で明示して**通す逃がしである。`retroactive` を転用しない——あの数は「後から足した歯が N 本」と読まれるので、移動の便に貼ると判定行から何を免除したのか読めなくなる（実測 2026-09-11・`s2-07l.84`）。効く条件は `retroactive` と**同じ 4 つ**（test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない）で、判定は同じ実装（`marker_beads`）を通る。
  - `retroactive`: 既に land した挙動へ**後から歯を足す**便は、歯をどこへ置いても base で緑になる（測る対象が base に在る）。test 区間内の行 `// flip-check: retroactive <bead-id>` を置いた file は RED を要求せず、判定行に `retroactive=N` が載る。**src 区間の marker は効かない**（実装の隣に 1 行足すだけで検査を外せる形にしない）。**効くのはその便で足した札だけ**である＝札の bead id が HEAD の test 区間に在り、かつ base の test 区間に無いときに限る（base に無い file は test 区間が丸ごと新しいので HEAD に在れば足りる）。札は file に残るので、在るだけで数えると一度貼った札がその file の test 区間を触る以後のすべての便を免除し、札の bead id と便が対応しなくなる。**同一性は bead id で見る**（字下げや id 前後の空白が 1 個違うだけで持ち越した札が新しい札に化けると、古い id のまま免除が効き続ける）。持ち越した札しか無い file で **test 区間が動いた便**には免除を与えず、stderr に `flip-check: stale-marker <rel>` を 1 行出す（免除を求めていない便＝src だけ触った便には出さない。札は file に残るので、出すとその file の src を触るたびに「削除しろ」と言われ、本当に効かない札を見落とす）。**限界（もう 1 つ）**: 札の bead id が実在の便を指すかは照合しない（bd を見ない）ので、**新規 file へ古い id の札を置く**形は通る——判定行の `retroactive=N` が review の入口である。N ≥ 1 は review の対象で、notes に変異 proof を要する。marker の無い後から足す歯は従来どおり落ちる。**限界**: 行が実際にコメントか文字列の中身かは **parser 無しでは弁別できない**ので、複数行文字列の中に行頭から marker が現れる file は免除される（`crates/*/tests/*.rs` は全体が test 区間なので特に当たりやすい）。塞ぐには parser が要り、それは本器の取らない道である——代わりに `retroactive=N` が判定行に必ず出るので、**事故は見える形で残る**（review が拾う）。
  - **持ち越し**（`s2-07l.362`・契約表の行 e）: 純移動で base から item ごと移る `moved` 以外の札（`retroactive` 等）は、純移動の機械証明（§5.3）が両側で同じ字面の札を対にして残差から外す＝新規の札と読まない。対の無い札だけが `ForeignMarker`。
- **免除経路の閉じ方**（`s2-07l.170`・監査 2026-09-12 塊 14）: (a) docs-only の分類は path の面（rules 行 `flip.docs_only_faces`）で決め、面の外の file を含む便は `.rs` の差分が無くても `no-test-diff` で落ちる（`no-rust-diff` の skip は消す）。(b) 札（`retroactive` / `moved`）の bead id は閉じた形で受け、形に合わない札は `bad-marker`、便が持つ札の本数が rules 行 `flip.marks_per_pr` を超えれば `too-many-marks` で落ちる。(c) push(main) の CI は HEAD が PR の squash（件名末尾の `(#N)`）か `pipe land` の trailer（`run: <run id>`）を持つことを `xtask main-provenance` で測る。(d) 宣言の `common-verify` の各行は先頭語列で閉じた `VerifyKind` に分類され、先頭語 `cargo` の行を 1 本でも持ちながら入口の flip を撃つ行を持たない宣言は intake が `NoEntranceRed` で断る。先頭語 `cargo` の行を持たない宣言（Rust でない toy repo・`sh` / `git` だけの `common-verify`）は分類だけで断らない＝上の「Rust 固有の検査を内蔵しない」のまま。宣言 file の schema は変えない。

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
- **質問の口 1 便（AC10・`s2-07l.116`・2026-09-12 実測）**: toy crate に「verify 行が矛盾する契約」（同じ純関数に `quarter(8) == 2` と `quarter(8) == 20` を要求）を `pipe run`（実 runner・席の口座・実 lens）で流す。実装役は矛盾を見抜いて最終行に質問 record を 1 つ書き commit 0 で止まり、包みが rc 76 で名乗って `QuestionRaised`（逐語）→ `Questioned` になる。回答なしの `resume` は rc 3 で event 0 件。契約の所有者が `pipe answer --words` で回答（`QuestionAnswered`・machine・逐語）し **run の写し `contract.toml` の done を回答どおりに直してから** `resume` すると、prompt の末尾に「回答」節が載った再 spawn が回答どおりに実装し（`Implemented`）、verify 4/4 と実 lens の PASS を経て `Landed` に至る。fleet の event は全部 machine（`pipe report` = `human_events=0`）・席が手で書いた commit 0。**写しの done を直さずに `resume` した便は実 lens が「契約と違う引数で verify だけ通した」と FAIL にする**（終端・回答節は契約を上書きしない＝回答が契約を変える周は写しも直す）。runner seam に `< {contract}` を付けると pipeline が stdin に流す「契約 + 回答」節が shell の redirect で潰れる（seam は §6 の形＝redirect 無し）。**包みが実 record を拾えない周が先に在った**（`s2-07l.123`・claude 2.1.268 の `result` record は入れ子の `"type"` が top-level より前）＝AC10 は `.123` の land 後に成立。

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
- 現物（planner が grep で実測・main f678bd0）: 受ける段の列挙は `crates/scribe2/src/pipe/cli/step.rs` の `retire_run`（`pub(super) fn`）が持つ `allowed` = `[Stage::Landed, Stage::Failed, Stage::Gated, Stage::Stopped]`＝`Reviewed` は列に無い。段の中の弁別は `crates/scribe2/src/pipe/cli/state.rs` の `discriminate`（同 file の私有 `fn`・`resolve` から呼ばれる）が持ち、`(&Extra::Retire, Stage::Gated)` と `(&Extra::Retire, Stage::Failed)` の 2 arm だけが在って `Reviewed` は末尾の catch-all で `Ok(())` に落ちる。判定の読み手は `crates/scribe2/src/pipe/review.rs` の `ReviewCheck::judge`（`pub fn`・返り値は `Passed` / `Stopped(Verdict)` / `Unreadable` の閉じた 3 つ）で、同 file の `(&Extra::Spawn, Stage::Reviewed)` の arm が既に呼んでいる。畳む本体は `crates/scribe2/src/pipe/retire.rs` の `retire`（`pub fn`・在るか / clean かだけを見て段を動かさず `retired/` へ可逆 move する）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `retire_run` の `allowed` に `Stage::Reviewed` を 1 つ足す（既存の 4 つと順序は不変）。
  2. `discriminate` に `(&Extra::Retire, Stage::Reviewed)` の arm を足し、`ReviewCheck::judge` が `Stopped(_)`（FAIL / INCONCLUSIVE）を返す周は `Ok(())`＝畳める。
  3. 同じ arm で `Passed` は断る（live・起こす側）。
  4. 同じ arm で `Unreadable` も断る（読めない判定を終端に読み替えない・fail-closed）。
  5. 断りの字面は `Spawn` × `Reviewed` の既存の arm と同じ形 `run <id> の段は Reviewed である（verdict=<V>）`＝段違いの一般則の字面（`run <id> の段は Reviewed である`・verdict の括弧を持たない）と区別が付く。歯はこの逐語で新 arm を pin する。
  6. 畳んだ後の段は `Reviewed` のまま（`Failed` / `Gated` と同じ）＝`RunStage detail=retired` の記帳と `retired/` への可逆 move は不変（N1.2）。
- 触らない: `retire.rs` の `retire` 本体（在るか・clean か）・`Extra::Retire` × `Gated` の arm・`Extra::Retire` × `Failed` の arm（そこは行 r が別便で触る）・worktree の無い Reviewed 終端の便（畳む物が無い＝既存の断りのまま）。
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
  7 本目の `crates/scribe2/src/account/cli.rs` の `flags`（私有 `fn`・`allowed: &[&str]` を取る）だけが allowed の集合で閉じるが、断りを全部 `None` に畳むので Help / Unknown / Missing / Duplicate を弁別しない（[account-lifecycle.md](./account-lifecycle.md) §4）。usage の字面は面ごとに 1 本ずつ在り（`pipe` は `crates/scribe2/src/pipe/cli.rs` の `usage`・`pub fn`）、外形は insta の名付き snapshot 5 本が pin している: `pipe_external_form`（`crates/scribe2/tests/e2e/pipe.rs`）・`fleet_external_form`（`crates/scribe2/tests/e2e/fleet.rs`）・`seat_usage_external_form`（`crates/scribe2/tests/e2e/seat.rs`）・`headless_external_form`（`crates/scribe2/tests/e2e/headless.rs`）・`vessel_external_form`（`crates/scribe2/tests/e2e/hook.rs`）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 共通の reader 1 本を新 module（行 f の write-set の `+` の file・`crates/scribe2/src/lib.rs` に `mod` 宣言）に置く: `parse(args, allowed) -> Result<Parsed, ArgsError>`。`Parsed` は名指しの flag の値（`value` / `need` の 2 面）と positional の列を持つ。
  2. `ArgsError` は閉じた enum（`Help` / `Unknown` / `Missing` / `Duplicate`・宣言順の `as_str`）。
  3. `--help` / `-h` は `allowed` に無くても `Help` で返し、呼び手は usage を出して rc 0 で終わる（state も ref も 1 本も動かさない）。
  4. `Unknown` は usage 1 行で rc 2。`Missing` / `Duplicate` も同じ rc 2 の口から出す。
  5. 上の 6 本と `account/cli.rs` の `flags` を `parse` の呼出に置き換え、各 subcommand の `allowed` を宣言順の const 配列で持つ（`flags` の `allowed` の集合と `None` に畳む挙動は、typed な 4 値へ置き換わる以外は変えない）。
  6. `pipe land` に**未知の flag** を渡した周は、偽 remote の toy repo で main の ref・event log・worktree が 1 つも動かず rc 2 で断る。
  7. `pipe land` に **`--help`** を渡した周も同じ toy repo で main の ref・event log・worktree が 1 つも動かず、usage を出して rc 0 で終わる（**2026-09-15 の実測の回帰そのもの**＝この枝は (6) と別の歯で 1 本ずつ測る。単体の reader の歯だけでは緑にならない）。
  8. `fleet` / `seat` / `headless` / `vessel` / `account` の 1 口ずつが未知の flag を rc 2 で断る（**5 口を 1 口ずつ名指して測る**＝1 口だけ直して緑にならない）。
  9. 既存の flag の意味と usage の文は不変＝上の外形 snapshot 5 本が 1 字も動かない（挙動の差は「未知の flag と `--help` を断る」だけ）。
- 触らない: subcommand の本体・rules・docs。依存を足さない。`.config/nextest.toml` は、seat の口の新しい歯が tmux の直列化の一覧（`test-groups.tmux` の filter）に載る周だけ 1 行が増える面として write-set に持つ（載らない周は触らない）。

## 15. pipe の `--repo` / `--state-dir` の cwd fallback を落とす（契約表の行 g・`s2-07l.310`）

- 出所: cargo-mutants の一時コピーは worktree の `.git`（file・本物の gitdir を指す）を持つので、コピーの中で cwd から解いた repo に便を起こすと worktree と branch が本物の repo に登録される（prunable 47 件・fixture 名の branch 44 本・実測 2026-09-15・prune は user 承認 event 02:5xZ）。base を記録して worktree を切る runner の起動（FR4）が、呼び手の指さない repo に効いてしまう＝intake の排他（FR39）が数える write-set の相手も別 repo に割れる。
- 現物（planner が grep で実測・main f678bd0・`.349` の分割後）: cwd の枝を持つのは `crates/scribe2/src/pipe/cli/args.rs` の `repo_of`（`pub(super) fn`・`--repo` が無いと cwd の repo root へ落ちる）で、呼び手は同 file の `state_dir_of`（`pub(in crate::pipe) fn`・`--state-dir` が無い周に `repo_of` 経由で置き場を解く）と `crates/scribe2/src/pipe/cli/intake.rs` の `run_repo`（`pub(super) fn`・写し面が読めない周の最終枝）の 2 つ。usage の字面は `crates/scribe2/src/pipe/cli.rs` の `usage`（`pub fn`）で、外形は `pipe_external_form`（`crates/scribe2/tests/e2e/pipe.rs`）の名付き snapshot が pin する。
- 順序: 本便は**行 v（`s2-07l.381`）の後に出す**。行 v が e2e の helper の cwd を git repo でない temp dir に固定するので、repo を要る呼出しは行 v の時点で既に `--repo` を渡しており、本便は歯の helper と既存の呼出し site を 1 つも触らない（触るのは下の新しい歯だけ）。
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
- 現物（planner が grep で実測・main f678bd0）: 着地の列は `crates/scribe2/src/pipe/queue.rs` の `turn_in`（`pub fn`）/ `await_turn`（`pub(super) fn`）が読む。唯一の wait は `crates/scribe2/src/fleet/wait.rs` の `Completion`（`pub enum`・`RunnerExited` / `SeatGone` / `SlotFree` / `GroupGone` / `LandTurn` / `AccountFree` / `CiResult` の 7 つで、`SlotFree` / `LandTurn` / `AccountFree` / `CiResult` が pid を見張らない側）。git の読みは `crates/scribe2/src/pipe/mod.rs` の `git_ok`（`pub fn`）と `git_line`（`pub fn`）の 2 口だけ。subcommand の verb は `crates/scribe2/src/pipe/cli.rs` の `match verb` 1 か所に並び、usage の外形は `pipe_external_form`（`crates/scribe2/tests/e2e/pipe.rs`）の snapshot が pin する。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `Completion` に pid を見張らない variant を 1 つ足す（窓の待ち）。既存の 7 つの判定と deadline の経路、唯一の wait の loop は不変。
  2. 窓の判定条件は 3 つ全部: (a) 着地の列に PASS の便が 0 本 (b) `Landed` の記帳から追随中の便が無い (c) local の `refs/heads/main` が `refs/remotes/origin/main` の祖先である（**未 push の squash が無い**＝主実測中の便が local main に積んだ squash と push 待ちの `Landed` の両方を数える・`s2-07l.449` の 1 面目: 窓が `Landed` 前の squash を数えず docs の merge で origin と分岐した・実測 2026-09-17）。
  3. (c) の読みの順は固定: **先に** local の `refs/heads/main` を読み、読めない周は origin の有無に依らず窓を**閉じる**（fail-closed）。**次に** `refs/remotes/origin/main` を読み、無い周は (c) を数えず（(a)(b) だけで判定し）行に `remote=none` を載せる。両方読めた周だけ祖先を判定する。
  4. origin の ref は読むだけで fetch しない（撃つ側が fetch する・器は網を撃たない）。git の読みは既存の `git_ok` / `git_line` の 2 口だけを使う。
  5. 新しい subcommand の口 `pipe land-window` を足す。窓が開いていれば rc 0 で `clear` の 1 行、待ちが切れれば rc 1 で列の便を名指した `busy` の 1 行を返す。pipeline の外で merge を撃つ側はこの口を前置して撃てる＝散文の窓判断が器の rc に変わる。
  6. `busy` の行は列の便の名指しの隣に `unpushed=<local main の sha|unreadable|->` を持つ（(c) で閉じた周は sha・読めない周は `unreadable`・(a)(b) だけで閉じた周は `-`）＝どの条件で閉じたかが 1 行で読める（C10）。
  7. verb が 1 つ増えるので usage に 1 行増え、`pipe_external_form` の snapshot がその 1 行だけ動く。
- 触らない: `gh pr merge` 自体（器は merge を撃たない）・列の順序と鍵・`LandTurn` の判定・`turn_in` の本体。
- 却下: docs-only PR も器が `gh` を撃って merge する（外部 binary を撃つ面が増える）／運用のまま据え置く（規則が散文のまま・N2）。

## 20. runner の雛形に終端の規律を足し片付けで殺した子の数を記録する（契約表の行 n・`s2-07l.275`）

- 出所: `.270` run 1 の観測: runner が「flip-check を背景で回している・完了通知を待つ」と言って turn を閉じ（rc 0・自己申告は done）、背景の task が scope の片付けで止められた。runner の口（FR5）の自己申告が背景 task の完了を含まず、人手 0 の計測（FR22）に載る終端の 1 行が「何を殺したか」を持たない。
- 現物（planner が grep で実測・main f678bd0）: 雛形は `crates/scribe2/src/headless/runner.txt`（34 行・`crates/scribe2/src/headless/runner.rs` が `include_str!` で読む）で、「背景実行で turn を閉じない」に当たる行は 1 本も無い（`背景` / `前面` / `turn を閉じ` の 3 語とも 0 件）。片付けは `crates/scribe2/src/pipe/confine.rs` の `release_scope`（`pub fn`・引数は `&Confinement`・返り値は `Option<Released>`）が行い、`crates/scribe2/src/headless/mod.rs` がそれを呼ぶ。終端の 1 行は `crates/scribe2/src/headless/runner.rs`（`<who>: scope=<Released の語> claude_peak_bytes=<n|->`）が組んで stderr へ出す＝片付けで殺した子の数はどこにも残らない。雛形の外形は `headless_runner_prompt_external_form`（`crates/scribe2/tests/e2e/headless.rs`）の snapshot が pin する。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 雛形に「検証は前面で完走させてから turn を閉じる（背景実行を残して終えない・残した task は片付けで止められ done に数えない）」の 1 行を足す。
  2. その 1 行だけを名指す歯を別に持つ（`.snap` は入口の flip の test 区間に入らないので、雛形の RED は逐語を名指すこの歯で測る）。
  3. 雛形の外形は `headless_runner_prompt_external_form` の snapshot が引き続き全文を pin する（足した 1 行だけが snapshot に現れる）。
  4. `release_scope` が scope を止める**直前**に scope に残った process の数を読む。
  5. 終端の 1 行に `orphans=<n|->` を足す（0 も書く・読めなければ `-`＝「0 本」と「測れなかった」を融合しない・C10）。
  6. 数える関数（`crates/scribe2/src/pipe/confine.rs`）と行を組む関数（`crates/scribe2/src/headless/runner.rs`）は pure に切り、それぞれの file の in-file の歯が fixture で測る（systemd の scope を歯で起こさない）。
- 触らない: 片付けの極性（止める）・runner の権限・`Released` の語彙・`claude_peak_bytes` の読み。
- 却下: 背景 task を待ってから片付ける（turn の終端の規律が曖昧になる）／雛形だけ直す（殺した事実が記録に残らない）。

## 21. gate の段の通知行を rc に依らず record と stderr に残す（契約表の行 o・`s2-07l.293`）

- 出所: `.286` の gate が 2 周とも lens への入力が diff だったのに理由語が残らなかった（run.stderr 0 bytes・rc 0）。gate の機械検証（FR8）が「測れなかった / 要約にならなかった」理由を捨てるので、人手 0 の計測（FR22）で後から原因を引けない（黙って落とさない・NFR4 の面）。同じ形は precheck の注意行・追随の rebase の行にも当たる。
- 現物（planner が grep で実測・main f678bd0）: 理由の 1 行は `crates/scribe2/src/pipe/move_proof.rs` の `LensInput::notice`（`pub fn`・返り値は `Option<String>`）が出し、`crates/scribe2/src/pipe/gate.rs` がそれを `Outcome.err` に載せる。`crates/scribe2/src/pipe/cli/run.rs:126` の `chain`（`pub(super) fn chain(lines: &mut Vec<String>, outcome: Outcome) -> Option<Outcome>`）は rc が `RC_OK` の周に `lines.extend(outcome.out)` して `None` を返す＝**`err` をそこで捨てる**。run dir にも書かれない（`lens-input.txt` は要約の周だけ残る）。gate の記録の行は `crates/scribe2/src/pipe/gate/record.rs` の `step_record`（`pub fn`）が組む。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. `chain` が rc 0 の段の `err` を保持し、`pipe run` の stderr に段の順で出す。
  2. stdout の判定行は 1 字も変わらない（`out` の繋ぎ方と rc の極性は不変）＝(1) の歯が「stderr に段の順で出る」と「stdout が base と同じ」を**対で**測る。
  3. gate の記録の step 行と同じ log に `lens-input=<kind> reason=<語>` を追記する（**要約の周も diff の周も**・run dir に残る）。
  4. `kind` は判定行が既に出す `LensInput` の kind（`diff` / `summary`）、`語` は `notice` の reason の値で、要約の周は `-`（0 と「測れなかった」を融合しない・C10）＝`move_proof.rs` は触らない。
  5. 歯の置き場: 記録の歯は `crates/scribe2/tests/e2e/pipe/gate.rs`、`pipe run` の stderr の歯は `pipe run` の e2e が在る `crates/scribe2/tests/e2e/pipe/spawn.rs`（§5.9 の分割で出来た `crates/scribe2/tests/e2e/pipe/ratelimit.rs` と `crates/scribe2/tests/e2e/pipe/stop.rs` はどちらも `pipe run` の歯を持たない）。
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
- 現物（planner が grep で実測・main f678bd0・`.349` の分割後）: 畳む本体は `crates/scribe2/src/pipe/retire.rs` の `retire`（`pub fn`）で「在るか・clean か」だけを検査し、段を動かさず `retired/` へ move する（段の弁別は持たない）。受ける段の弁別は呼び手が持つ＝`crates/scribe2/src/pipe/cli/step.rs` の `retire_run`（`pub(super) fn`）の `allowed`（`Landed` / `Failed` / `Gated` / `Stopped`）と `crates/scribe2/src/pipe/cli/state.rs` の `discriminate`（同 file の私有 `fn`）の 2 arm＝`Extra::Retire` × `Gated` は verdict が `Fail` の周だけ・× `Failed` は `last_stage_detail` が `REBASE_EMPTY` か `follow::EXHAUSTED` の周だけで、他の段は末尾の catch-all で受ける。＝`Landed` / `Stopped` / `Gated(FAIL)` / `Failed(rebase-empty / rebase-conflict)` は**既に畳める**（`crates/scribe2/tests/e2e/pipe/land.rs` の `pipe_retire_*` 6 本 / `pipe_follow_retire_*` 2 本の歯が pin・母集団は同 file の歯全部）。base で断るのは **`Failed` の他の detail**（`main-red` / `main-unmeasured` / `rebase-dirty` / `precheck:…`）だけで、`pipe_retire_rebase_empty_refuses_other_failed_reasons` がその拒否を pin している。
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
- 既存の歯の書き直し（run 1 = Questioned 2026-09-16・runner の逐語「本便の新外形と正面から矛盾して必ず RED になる」・admin が現物で確認）: `crates/scribe2/tests/e2e/pipe/gate.rs` の `pipe_gate_move_proof_comment_only_diff_inside_items_sends_summary` は旧外形を字面で pin する（件数の行の直後が `## 残差分（逐語）`・コメントの字面 `helper two` は要約に載らない）。本便はその歯を新外形（件数の行の直後に `-` / `+` の逐語が並び、コメントの字面が要約に載る）へ書き直す。歯の名・`lens-input=summary`・rc・stderr 空の pin は不変＝write-set はこの e2e file を含む。
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
- site の census（planner が実測・main f678bd0・母集団 = `crates/scribe2/tests/e2e/pipe.rs` と `crates/scribe2/tests/e2e/pipe/` 配下の全 file）: binary を起こす site は **合計 41 箇所・9 file** で、字面は 2 形ある——`Command::new(bin())` が 38 箇所（`pipe.rs` 4 / `pipe/intake.rs` 13 / `pipe/land.rs` 6 / `pipe/spawn.rs` 5 / `pipe/stop.rs` 6 / `pipe/gate.rs` 2 / `pipe/ratelimit.rs` 1 / `pipe/launch_failure.rs` 1）と `Command::new(super::bin())` が 3 箇所（`pipe/dispatch.rs`）。**どの site も cwd を継ぐ**（nextest の子 process の cwd = crate dir）。例外は既に自分で `current_dir` を置いている 2 箇所だけで、`pipe/launch_failure.rs` の 1 箇所（相対 `--repo` が cwd から解けることがその歯の主題）と `pipe/dispatch.rs` の 1 箇所である。`crates/scribe2/tests/e2e/pipe/spawn.rs` の残り 1 つの `bin()` は runner の shell 文字列に埋める path で Command を作らない（site に数えない）。親の helper のうち `pipe` の verb で binary を起こすのは 3 本＝`run_pipe` / `run_pipe_with_path` / `land_once_with_git_shim`（`intake_raw` と `land_once` は `run_pipe` 経由なので直の site ではない）。
- 形: binary を起こす口を **1 関数**（`crates/scribe2/tests/e2e/pipe.rs` に新設・`Command` を返し `current_dir(<git repo でない temp dir>)` を付ける・**関数の名は行 v の契約が持つ**＝base に無い名を本節は名指さない）に集め、上の 41 site を全部それに通す。cwd が主題の 2 site は、返った `Command` に自分の `current_dir` を**後置**して主題を保つ（後の指定が勝つ＝歯の意味は変えない）。cwd が repo でなければ、どの変異の下でも cwd の fallback は「repo の root を解決できない」で**断る**（fail-closed）＝本番へは届かない。器の側（`state_dir_of` / `repo_of` の fallback・`vessel::state_dir`）は触らない（読みの口 `show` / `report` を anchor の cwd で撃つ常道を残す）。本便の write-set は全部 test 区間なので flip-check の overlay は HEAD と一致し base で赤くならない。歯は retroactive 札で通し、`current_dir` を外した A/B と差し替えを 1 site 戻した A/B の rc を変異 proof として notes に残す。
- 歯（2 本立て・番号は done と 1:1）:
  1. 親の helper 3 本（`run_pipe` / `run_pipe_with_path` / `land_once_with_git_shim`）の**それぞれ**で、`--state-dir` も `--repo` も無い `pipe show` が「repo の root を解決できない」の断りで rc 1 になる（3 本を 1 本ずつ名指して測る＝1 本だけ直して緑にならない）。
  2. 置き場の pin: tracked の 9 file を読み、`Command::new(bin())` と `Command::new(super::bin())` の出現の**合計が 1**（新設の関数の中の 1 箇所）であることを、読んだ file 数と base の 41 site を母集団として同時に出して測る。
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
- 形（衝突の機械解消 [pipeline-conflict.md](./pipeline-conflict.md) §3 と同じ経路・新しい経路を持たない）: (1) `follow_main`（`land.rs`）は rebase が通った直後・§33 の省略の判定と再 gate の**前**に、契約表の検査（`contracts check` と同じ 1 関数 `check_repo`・`table.rs`）を便の木に撃つ。findings が 0 なら従来どおり。(2) findings が在り、そのすべてが write-set の項目の未解決で、名指された path が**便自身の diff で消えた・改名した path**（`git diff --name-status <base>..HEAD` の D / R の旧 path・写しの write-set の `-` の項目とは別の実測）に含まれる周は、`RunStage stage=Implemented detail=rebase-stale-rows:<base>..<main>` を記帳し（終端にしない・§3 の手順 2 と同型）、runner を起こし直す（同じ worktree・同じ契約・stdin の「追随」節に行の一覧〔`<doc>#<id>` と未解決の項目〕を足す・`spawn.rs` の節の出所は `follow.rs` の `section` の 1 本のまま）。回数は衝突の回数と**同じ 1 つの上限**（rules 行 `pipe.follow_retries`・`is_conflict` の読み手を `rebase-stale-rows:` の接頭辞も数える 1 本にする・resume の弁別も同じ 1 本）で、上限に達した周は `Failed detail=rebase-stale-rows`。(3) 起こし直しの turn が行を直せるよう、写しの write-set（run dir の `contract.toml`・`contract_path`）に findings の行を持つ設計 doc（`docs/design/<doc>.md`）を器が**追記する**（追記だけ・既存の項目は動かさない・追記した項目は同じ event の stderr の行に写す＝gate の照合と runner の guard が同じ写しを読むので食い違わない・`.133` の「契約の改訂を器の口で持つ」の最小形）。(4) それ以外の findings（便が消していない path・行の形の誤り）は便の責任ではない＝従来どおり再 gate へ進み、赤なら gate の判定で止まる（本行は「便が消した path を名指す行」だけを拾う・fail-closed の向きは変えない）。契約表の検査を撃てない周（repo を読めない）は従来どおり再 gate（読めないを「行なし」に読み替えない・NFR4）。
- 触らない: 追随の要否判定・rebase と衝突の経路・§33 の省略の判定（本検査はその前に撃つ）・検出線の rc 2 の扱い（穴 (2) は原因を木から取り除くことで到達しなくなる・resume の形は不変）・純移動の便が契約時に他の行を直す義務（contract-source.md §15 の型・本行は契約の後に入った行だけを拾う）。
- 歯（`pipe_follow_stale_rows_` 接頭辞・`tests/e2e/pipe/land.rs`・既存の追随の fixture〔`gated_pass` + 別便の commit + 偽 runner〕の型）: (a) 便が file を消した後、main が「消えた path を write-set に持つ行」を足す docs の commit で進んだ周は、land が `Implemented detail=rebase-stale-rows:` を記帳して偽 runner を 1 回起こし、写しの write-set にその設計 doc が追記され、再 gate は撃たれない（偽 lens の写し 0）／(b) 行が便と無関係の path を名指す周は起こし直さず従来どおり再 gate へ進む／(c) 上限 `pipe.follow_retries` に達した周は `Failed detail=rebase-stale-rows` で終端する／(d) `--runner` の無い land は `rebase-stale-rows:` を記帳して rc 1 で止まり resume で続けられる（§3 の手順 4 と同型）。
- 却下: 器が行を書き換えて着地する（land が write-set の外の doc を触る＝gate の照合と runner の guard の外の変更・C16）／stale な行を INCONCLUSIVE の理由の 1 つとして記帳するだけ（記帳は在っても直す手が無い・穴 (1) そのもの）／検出線の rc 2 の周に resume が全 verify を撃ち直す（原因が木に在る間は何回撃っても同じ・費用だけ増える）。

## 35. main 実測の赤に落ちた歯の名と panic の抜粋を残す — record に `failed=`・落ちた歯ごとの stderr の区間（契約表の行 ac・`s2-07l.401`）

- 何が起きているか（admin 実測 2026-09-16 07:18Z `.164` run 051333Z・14:2xZ の 3 便比較・verified）: land の主実測（§5.4・`verify_main`）で全件 nextest が rc 100 → `Failed detail=main-red`（push なし・main 無傷＝止め方は正しい）。record（`verify-main.jsonl`・gate の `verify.jsonl` と同じ `records_of` の形）は行ごとの `rc` と stderr の末尾 `STDERR_TAIL_LINES` 行（20）を持つので、落ちた歯の名（nextest の Summary の後の `FAIL [` 行）は残るが、**panic の本文（assert の文）は落ちた歯の実行位置が末尾に入る周だけ残る**（`.389` は在る・`.323` は 441/1490 と 642/1490 の位置で無い）。原因の切り分け（rebase の相互作用か flaky か）を admin が手で撃ち直して探した。
- 形（gate の verify 行と主実測の**同じ 1 本**・`pipe/gate/record.rs`）: (1) record の head に `failed=<歯の名>` を 1 つ足す（nextest の stderr の `FAIL [` 行の最初の 1 本・無い周は書かない・pure な抽出関数 1 本・in-file の歯）。(2) stderr の写しは末尾 N 行に加えて、**落ちた歯ごとの区間**（nextest は落ちた歯ごとに即時の `FAIL [ … ] <歯の名>` の進捗行の後へ小見出し `stdout ───` / `stderr ───` を出す〔0.9.143〕。区間 = その `stderr ───` の小見出しから次の進捗行（`PASS [` / `FAIL [`）か Summary の直前まで・歯の名は直前の `FAIL [` の行から取る・歯 1 本あたり `STDERR_TAIL_LINES` 行を上限・落ちた歯が複数なら順に・字面は cargo-nextest 0.9.143 の出力を gate の実 log で実測したもの）を残す＝末尾の N 行に panic が入らない位置の歯でも本文が残る。区間の切り出しは nextest の字面の閉じた 2 形（`FAIL [` の進捗行 / `stderr ───` の小見出し）だけを読む pure な関数で、他の verify 行（clippy 等）は従来どおり末尾 N 行だけ。(3) gate の `verify.jsonl` と主実測の `verify-main.jsonl` は同じ関数を通る（片側だけに足さない・C2）。写しの診断 file も対で持つ: gate は既存の `verify.stderr.log`、主実測は同じ dir に同じ形で `verify-main.stderr.log`（`verify-main.jsonl` と同じ stem・機械は読まない・人が読む）。
- 触らない: `MainCheck` の 3 値と `main-red` の極性（auto revert しない）・record の `n` / `rc` / `cmd` の形・`STDERR_TAIL_LINES` の値・stdout の扱い。
- 歯（`pipe_verify_failed_` 接頭辞・`pipe/gate/record.rs` の in-file の pure な歯 + `tests/e2e/pipe/land.rs` の既存の main-red の fixture の型）: nextest 形の stderr（Summary の後に `FAIL [` 2 本・各歯の `stderr ───` の区間・落ちた歯が末尾から遠い位置）から `failed=` が最初の 1 本を指し、区間が歯ごとに上限行数で残る／`FAIL [` の無い stderr は `failed=` を持たず末尾 N 行だけ／main-red の便の `verify-main.jsonl` に `failed=` と区間が載る（e2e）。
- 却下: 末尾の行数を増やす（歯の数に比例して膨らみ、位置の問題は残る）／nextest の JSON 出力を読む（出力形式の依存が 1 つ増え、共通 verify の行の字面を器が縛る・ADR-0010 の宣言の外）／runner の stdout に写す（主実測は runner が居ない）。

## 36. 着地の列が driver の死んだ便を先頭に数えない — 札の所有者が死んだ便を `skipped-dead` で外し、段は動かさない（契約表の行 ad・`s2-07l.388`）

- 何が起きているか（admin の実測 2026-09-16 04:0xZ・verified）: 着地の列（`pipe/queue.rs` の `turn_in`・`gated_at` 順・[gate-cost.md](./gate-cost.md) §6）の先頭の便が host の再起動で driver（`pipe run` の process）ごと死に Implemented のまま止まると（Gated PASS の判定と worktree は在る）、後続の便は `await_turn` で先頭が退くのを rules 行 `pipe.land_wait_s` の上限（90 分）まで待ち `unmeasured` に倒れる＝再起動・oom・stop の失敗のたびに「上限 × 後続の本数」を失う。現物（main a620600）: `turn_in` は自分より鍵の小さい PASS の便を「終端でない ∧ Gated を 1 度通った ∧ worktree が在る」で数え、その便を進める者が生きているかを見ない。driver の生死は [dispatcher.md](./dispatcher.md) §5（行 d・`s2-07l.352`）の**札**（`<state_dir>/pipe/<run>/driver`・pid + 起動時刻・lock の所有者と同じ probe・`Owner::Dead`）が typed に持つ＝本 § はその読みを列に足すだけで、生死の判定を 2 本にしない（C2・C3.3）。
- 形: (1) `Queued` に driver の生死（closed 3 値: 生きている / 死んでいる / 札が無い・読めない）を足し、`queue_of` が行 d の札の読み手（同じ関数・pub(crate)）で埋める。(2) `turn_in` は**死んでいる**便だけを `ahead` の候補から外す。札が無い・読めない便は従来どおり数える（測れないを「死んだ」に読み替えない・fail-closed・行 d の「札の無い便は触らない」と同じ極性）。自分の便の札は見ない（自分は生きている）。(3) 外した便を黙らせない（C10）: `Turn` の 3 値は不変で、`await_turn` の記録（面 5 `verdicts.jsonl` の `order` の隣）に任意 field `skipped_dead=<run,…>`（外した便 id の列・鍵の順・外した周だけ）を足し、`pipe land` の stdout の `order=` の行に `skipped-dead=<n>` を後置する（0 の周は書かない）。(4) 外すだけで段は動かさない（死んだ便は Implemented / Gated のまま・resume 可・N1）。起こし直しは行 d（dispatch の turn 関数の `pipe resume`）の領分＝本 § は列の側だけ。
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
- 形（判定は typed・段の関数に読み手を増やさない・C2）: (1) **記帳の門**: `fleet/store.rs` に「lock の中で述語を評価して偽なら書かない」条件付き append の口を 1 つ足し（述語は閉じた enum の値＝`NotStopped { run }`・自由な closure は受けない・lock の外で読んだ値との race を塞ぐ）、`pipe/mod.rs` の `emit` は kind が `RunStage` / `RunDone` / `SeatSpawned` の周だけこの口を通す＝その run の最後の run event が `RunStopped` なら書かずに `StoreError` の新しい variant `Stopped` で断る（呼び手は既存の Err の経路＝rc 2 と stderr 1 行で止まる・運転手はそこで終わる・`chain` が後段を撃たない）。読めない周は書く側に倒さない（既存の `Malformed` / `Io` の断りのまま・fail-closed）。`RunStopped` 自身と `SeatStopped` は門を通さない（停止の記帳を停止が塞がない）。(2) **運転手の停止**: `pipe stop --run` は席を止めた後、札（dispatcher.md 行 d・`<state_dir>/pipe/<run>/driver`・pid + 起動時刻・`Owner` の probe）が生きた運転手を指す周は、その process group を席と同じ 1 関数（`terminate`・猶予は `pipe.stop_grace_ms`・TERM → wait → KILL）で止め、`SeatStopped` と同じ形の記録（`seat=driver` の 1 行・detail に `stopped-by-stop`）を残す。札の pid が**自分自身**（`pipe run` の中から stop を撃つ形）の周は止めない。札が無い・読めない・死んでいる周は止めない（測れないを「止めた」に読み替えない）。止め切れなかった周は従来どおり `RunStopped` を書かず rc 1。(3) 段の順序は不変: 席 → 運転手 → `RunStopped`（`RunStopped` は最後＝書けた時点で live から外れる・§5.6 の極性）。
- 触らない: `pipe stop --all`（席の掃除・運転手は止めない）・`pipe.stop_grace_ms` の値・`Stopped` の便の `retire`（pipeline-conflict.md §5）・`queue.rs` の `may_queue`・oom の分類（行 q・`s2-07l.340`）・`resume`（`Stopped` は終端＝再開の口は無いまま）・札の書き・消し（行 d）。
- 歯（`pipe_stop_driver_` 接頭辞・e2e は `tests/e2e/pipe/stop.rs`・偽の運転手 = `sleep` の process group の pid を札に書いた fixture）: (a) in-file（`pipe/mod.rs` の tests）`RunStopped` の後に `RunStage stage=Gated` を `emit` すると `StoreError` の `Stopped` で断られ event log の byte 数が不変（base は書く → RED）・`RunStopped` / `SeatStopped` は書ける／(a′) e2e の race: verify を `sleep` にした gate を子 process で走らせ走行中に `pipe stop --run` を撃つと、gate は `Gated` の記帳で断られ rc 2・event log の最後は `RunStopped` のまま（札の無い gate は止められず門だけが効く形）／(b) `pipe stop --run` が札の pid の group を止め `seat=driver` の記録を残す（base は生きたまま・記録 0 → RED）／(c) 札の pid が stop を撃つ process 自身の周は止めずに `RunStopped` を書く（fixture は `sh -c` で `$$` と起動時刻を札に書いてから同じ shell で `exec` して stop を撃つ＝札の pid == stop の pid）／(d) 札が無い・死んでいる周は席の停止と `RunStopped` だけ（従来と同じ event 列）／(e) TERM を無視する偽の運転手（`trap '' TERM` の sh）は猶予の後の KILL で止まり rc 0（席の既存の歯と同型）／(f) in-file（`fleet/store.rs` の tests）: 条件付き append は lock の中で述語を評価し、偽の周は file が 1 byte も変わらない。
- 却下: 各段の関数の先頭で `Stopped` を読む（読み手が段の数だけ増え、新しい段を足すたびに漏れる・C2）／stop が state dir に印の file を置いて段が読む（記帳と別の状態・C3）／運転手を殺すだけで記帳の門を持たない（殺す前に書かれた event と race する・.289 の型）／記帳の門だけで運転手を殺さない（gate の verify が走り切るまで CPU と worktree を握る）。

## 40. 着地の列の先頭 N 本を候補の木 1 つに積んで検査を 1 回撃ち、patch-id 不変の便は検出線を持ち越す（契約表の行 ah / ai・`s2-07l.428`・[ADR-0039](../../design-intent/decisions/ADR-0039-landing-train-gates-one-candidate-tree.html)）

- 何が起きているか（planner の実測 2026-09-17・fleet の event log 09-16 00:00Z 以降 84 便）: 便の延べ 83 時間のうち gate 36 時間（43%）・追随の再 gate 29 回（中央値 16 分・最大 151 分）・着地した 17 bead の壁時計は中央値 238 分。列の先頭が着地するたびに後続が追随して全部撃ち直す＝列の長さ N に対して再 gate が N 回・gate の時間が N の 2 乗に伸びる。29 回の再 gate はすべて `rebase:<old>..<new>` の記帳を持つ衝突無しの rebase で、便の diff は変わっていないのに検出線（mutants-diff）も撃ち直している。§30 / §33 は main の差分が検出線の面の外の周だけを省く。
- 現物: `pipe/land.rs` の `land` は便 1 本ごとに「番待ち → 追随（rebase → gate の撃ち直し）→ squash → 主実測 → finish」を通す 1 本道で、列（`pipe/queue.rs` の `turn_in`）は順番だけを決める。検出線の record（`pipe/gate/record.rs`）は便の diff の指紋を持たないので、再 gate は「同じ diff か」を測れない。
- 形 (a)（行 ah・候補の木）: 列の先頭（`turn_in` が `First` を返した便）が着地する周、**列の自分の後ろに並ぶ便**（`turn_in` と同じ条件＝終端でない ∧ Gated 済 ∧ worktree 実在 ∧ 最新 verdict PASS・鍵の順）を rules 行 `land.train_max`（列の長さの上限・kind `LandTrainMax`・Int・値 4・裁定 id = user 2026-09-17T03:28Z・C5）− 1 本まで取り、自分を先頭に並べた列を **1 つの候補の木**に積む。選ぶ関数は `pipe/queue.rs` の pure な 1 本（`turn_in` と同じ列の読みの上）で、行が無い・読めない周と上限 1 の周は自分だけ（現行と同じ）。候補の木は main の先端から切った tmp worktree（主実測の `verify` の隣・便の worktree と記録の base は触らない）に、便ごとに `git cherry-pick <base>..<HEAD>` で順に積む。積めなかった便（衝突）は `cherry-pick --abort` で候補から外し（その便の event は書かない・後続は詰める）、外れた便は自分の land で従来どおり追随して衝突を pipeline-conflict.md §3 の起こし直しへ進める。積んだ便ごとにその段の tree を覚え、便の diff に対する検出線をその場で撃つ（base = 直前の段・record はその便の `verify.jsonl`）。全部積んだ木に対して共通 verify を **1 回**（record は先頭の便の `verify.jsonl`・field `train=<N>`）と各便の契約 verify をその便の分（record はその便の `verify.jsonl`）撃つ＝lens は撃たない（各便の verdict PASS が入口の条件で、候補の木で測るのは木の緑）。**緑**なら列の順に、覚えた段の tree で `commit-tree`（親 = 直前の着地 commit）して main を CAS で N 本ぶん進め（squash の材料を「worktree の tree」から「段の tree」に広げる 1 引数）、次に主実測 `verify_main` を従来どおり先端の木で 1 回撃つ（§33・C12.6 の緑はここが担う・木は候補と同じなので検出線は `same-tree` で省かれる）。主実測が**緑**なら便ごとに列の順で `verdicts.jsonl` の 1 行（`order` の閉じた値に `train` を 1 つ足す）と `Landed` event（detail は従来の形＝`sha:` に自分の段の commit・`main:` に実測した先端）と worktree の退避を書く。主実測が**赤**なら列の便すべてに従来の `Failed` の `main-red` を書く（main は進んだまま・巻き戻さない＝既存の極性・N 本のどれが赤かは帰属しない）。FR50 の面: push と CI の照合は先端 commit 1 回で N 本ぶん（同じ push・同じ CI run）、台帳の close は便ごとの `Landed` の `sha:` で 1 本ずつ（push・照合・close の機械化は本節の外＝行 ah の write-set に無い）。**赤**（共通 / 契約 / 検出線のいずれか）なら候補の木を畳んで**列を解き**、先頭 1 本の既存の経路（追随 → 撃ち直し）にそのまま入る（どの便が赤かは帰属しない・後続の便は列に残る）。候補の木を切れない・積めない・読めない周も同じく解く（fail-closed）。列の後ろの便が自分の land に来た周は、**番待ち（`await_turn`）から戻った直後の 1 点**で段を読み、`Landed` なら **何もせず rc 0**（§29 の冪等の終端を段で先に読む・worktree の実在を要さない・待たない周も同じ点を通るので、番待ちの間に列で着地した便も追随へ進まず終端する＝`await_turn` 自身は触らない）。上限 1 と行の不在は現行の経路そのもの。stdout の 1 行に `train=<積んだ本数>`（解いた周は `train=<N> dissolved`）。train の本体は行 ah の write-set の `+` の file（新設 module）に置き、`pipe/land.rs` は `Landed` の早期終端・`First` の周の委譲・`squash` の 1 引数だけが動く。
- 形 (b)（行 ai・検出線の持ち越し）: gate は検出線を撃つ周に便の diff の `git patch-id --stable`（`<base>..HEAD`）を検出線の record に `patch_id=` で残す。追随の撃ち直し（`follow_main`）と候補の木の各段は、検出線を撃つ前に同じ 1 関数で「面に触れたか（§30）→ 前周の record に `patch_id` が在り今の diff の patch-id と同じか」を順に読み、同じ周は前周の検出線の record を **`carried=<前周の n>` を付けて写し**撃たない（`Detection` の閉じた値に持ち越しを 1 つ足す・持ち越した値は record の field で実測と区別する・C10）。record が無い・`patch_id` が無い・違う・読めない周は撃つ（fail-closed）。§30 の `outside-scope` の省略は不変で先に効く。
- 触らない: 列の順序と鍵（`turn_in`・ADR-0021 §2.5）・`await_turn` と `pipe.land_wait_s`・stale base の判定と CAS・rebase と衝突の経路（便の worktree は候補の木の外）・主実測の行・lens の判定・verdict の 3 値・`Landed` の detail の形・`--pr-cmd` 形（列を見ない）・`DETECTION_SCOPE`。
- 却下（ADR-0039 §03）: 現行のまま（2 乗の撃ち直し）／追随の再 gate を撃たない（着地前に着地後の木を検査しない・C12.6）／楽観着地して赤なら revert（main が赤の時間を認める）／先頭 k 本ごとの木を並列に検査する（費用が N 倍・改訂 ADR で足す候補）／候補の木を便の worktree を順に rebase して作る（後続の便の記録の base が main の祖先でなくなり、解いた周に便が stale で固まる）／`Landed` の detail に train の印を足す（便ごとの記録の形は不変・train の事実は先頭の便の record と stdout に置く）。
- 歯（`pipe_train_` 接頭辞・`tests/e2e/pipe/land.rs`・既存の `three_gated_runs` + 偽 lens + rules の tmp manifest の型／`rules_land_train_` 接頭辞・`tests/e2e/rules.rs`／行 ai は `pipe_detection_carry_` 接頭辞・`tests/e2e/pipe/gate.rs` と `land.rs`）: (a) 上限 3 で先頭を land すると 3 本が列の順に着地し（親の連鎖・main の先端・`Landed` 3 件・`verdicts.jsonl` 3 行・`order=train` が後続 2 本）、後続の追随は 0 回・先頭の `verify.jsonl` に共通 verify が `train=3` で 1 組・後続の `verify.jsonl` に契約 verify と検出線だけ／着地済みの便の land は rc 0 で main 不変／番待ちで待っている 2 本目の land（子 process・`pipe.land_wait_s` の窓）と並行に先頭が列で着地すると、2 本目は起きた後に rc 0 `already-landed` で終端し event が増えない。(b) 3 本目の契約 verify が赤なら列を解いて先頭だけが着地し、後続 2 本は Gated PASS のまま列に残り、stdout に `dissolved`。(c) 2 本目が先頭と衝突する周は 2 本目を外して 1・3 本目が着地し、2 本目の worktree は clean のまま event が増えない。(d) 上限 1 と行の不在は先頭だけが着地し後続は従来どおり追随 1 回。(e) 列を選ぶ pure 関数の in-file の歯（鍵の順・PASS でない / worktree 無し / 終端の便を数えない・上限で切る）。(f) rules 行の pin（kind・enabled・裁定 id・`ALL` に在る・parse で引ける・外形 snap の `rows=` / `kinds=` が 1 増える）。(g) 先端の木の主実測が赤の周は列の便すべてが `Failed` の `main-red` で `Landed` 0 件・main は N 本ぶん進んだまま（巻き戻さない・既存の極性）。行 ai: (g) gate の検出線の record に `patch_id` が在り `git patch-id --stable` と一致／(h) main が `crates/` で動いた追随で便の diff が不変なら検出線を撃たず `carried=<n>` の record が前周の写し／(i) 便の diff の中身が変わる周（衝突無しでも context が動く fixture）と前周の record が無い周は撃つ。

## 41. pipe/land.rs の主実測の群を land/verify.rs へ割る（契約表の行 aj・`s2-07l.457`・純移動）

- 何が起きているか（planner の実測 2026-09-18・main 36d9c39・`pipe preflight` で verified）: `pipe/land.rs`（1238 行・src 1160 + in-file の歯 78）は R-C4-2 の余地が 261 行しか無く、size M の便 3 本（行 ah・`s2-07l.428`／行 af・`s2-07l.449`／行 ab・`s2-07l.400`）を受付が `cap-headroom` で断る（3 本とも rc 1 を実測）。責務は 6 群（入口と番待ち／追随〔`follow_main` の群〕／squash と finish／主実測〔`verify_main` の群〕／anchor の同期／worktree の検査）で、主実測の群は §5.4 の tmp worktree の中だけを触り、追随・squash の群に依存しない閉じた集合（呼び手は `land`〔`verify_main` / `main_red` / `main_unmeasured`〕と `finish`〔`measure_main`〕の 2 か所・grep で確認・run 213227Z の Gated FAIL で 4 語目を実測）。
- 決定的な制約（実測）: `MainCheck` は極性一覧（`crates/scribe2/src/polarity.rs`・`tests/e2e/polarity.rs`・snapshot `polarity_external_form`）が境界の型名 `pipe::land::MainCheck` を pin する＝**`MainCheck` は親に残す**（`pub use` では型名が変わらない・contract-source.md §15 の `TableError` と同型）。`AnchorPlan` / `WorktreeCheck` も同じ pin を持つが移す群に無い。
- 形（contract-source.md §14 / §15 と同型）: 子 module（行 aj の write-set の `+` の file）へ主実測の群（`check_path` / `verify_main` / `materials` / `main_detection` / `record_main` / `measure_main` / `main_red` / `with_anchor` / `main_unmeasured` と const `CHECK_DIR` / `VERIFY_MAIN_FILE` / `MAIN_UNKNOWN`・約 230 行）をそのまま移す。親は `mod` 宣言と `use`（子の 3 関数を `land` が・1 関数を `finish` が呼ぶ）で `land` と `finish` の本体を不変に保つ。in-file の歯 5 本（`pipe_land_subject_` / `pipe_detection_scope_` / `mutant_in_pipe_land_next_number_`）は移す群の歯ではないので親に残す＝子に歯は無い（純移動の証明は札と既存の e2e の歯）。子は親の私有 item（`MainCheck` / `AnchorSync` / `Land` の欄 / `verdicts_path` / `with_lines` 等）を `super::` でそのまま呼べる（Rust の可視性＝子孫は祖先の私有を見る）ので親側の可視性は変えない。上げるのは**子側**の可視性＝親の `land` が呼ぶ `verify_main` / `main_red` / `main_unmeasured` と親の `finish` が呼ぶ `measure_main` の `pub(super)` の 4 つだけ。可視性の 1 語と mod 宣言・`use` の path・doc コメント行は移動の一部（純移動の残差として許す）。札 `// flip-check: moved s2-07l.457` は親の歯の区間と子の先頭に対で置く。verify は親に残る in-file の歯（`pipe_land_subject_`）と極性一覧の snapshot の歯（`polarity_external_form`・`tests/e2e/polarity.rs`）を撃つ＝後者の file は行 aj の write-set に持つが本便では触らない（受付の歯の置き場の門のため）。
- 見積: 親 約 1000 行（余地 約 500）・子 約 240 行。
- 歯: 既存の `pipe_land_` / `pipe_order_` / `pipe_follow_` / `pipe_retire_` の e2e と極性一覧の snapshot（`polarity_external_form`）が全部緑で期待を変えない。
- 却下: anchor の群を移す（88 行で M の余地に届かない）／追随の群を移す（行 af / 行 ab が同じ群を触る＝それらの write-set が 2 file に割れて交差が増える）／行 ah / af / ab を S に落とす（見積が S の 100 を超える＝size の字面だけ変える嘘）。

## 42. 受付の e2e の hub を接頭辞ごとに割る（契約表の行 b・`s2-07l.351`・純移動）

- 出所: `crates/scribe2/tests/e2e/pipe/intake.rs` は open な便の write-set に何本も現れる hub で、intake の排他（FR39）の交差の母集団を大きくし便を直列にする。接頭辞で割って母集団を構造から減らす（`.327` / `.349` / `.264` と同じ型・要件は FR30 の配送構造の面）。受付が verify 行の scope（`-p` / `--test` / `--lib`）を読むようになった `s2-07l.451` が着地した後なので、歯の置き場の門は本便の形で通る。
- 現物（planner が実測・main f678bd0）: `crates/scribe2/tests/e2e/pipe/intake.rs` は 2943 行・`#[test]` の歯 97 本で、接頭辞の内訳は `pipe_intake_` 31・`contract_` で始まる 39（`contract_closure_ext_` 17 / `contract_derive_` 8 / `contract_check_` 6 / `contract_declared_` 3 / `contract_table_landed_` 2 / `contract_schema_` 2 / `contract_names_` 1）・`pipe_review_` 12・`pipe_refuse_` 8・`pipe_preflight_` 4・`pipe_contract_` 1・`pipe_state_` 1・`pipe_show_` 1（母集団は同 file の `#[test]` 全数 97）。共有 helper は親の `crates/scribe2/tests/e2e/pipe.rs` に 40 本在り、子は `use super::*;` で引く（`.264` の分割と同じ形）。親の `mod` 宣言は 8 本。
- filter の当たりの実測（`--test e2e` の scope・fn 名の substring・母集団は `crates/scribe2/tests/e2e` の `#[test]` 919 本）: 裸の `contract_` は 9 file の 68 本に当たる（`headless.rs` 7 / `hook.rs` 1 / `pipe/dispatch.rs` 5 / `pipe/gate.rs` 2 / `pipe/spawn.rs` 1 / `polarity.rs` 1 / `prop.rs` 1 / `rules.rs` 3 / `pipe/intake.rs` 47）＝**verify 行には使えない**（受付が teeth-outside-write-set で断る）。細かくした接頭辞のうち他 file に漏れるのは 2 つだけで、`contract_closure_ext_` が `crates/scribe2/tests/e2e/prop.rs` の 1 本に当たり（だから行 b の write-set はこの file を持つ＝**本便では 1 字も触らない**）、`contract_table_` は `polarity.rs` 1 本と `rules.rs` 3 本に当たるので `contract_table_landed_` まで伸ばす（当たりは `pipe/intake.rs` の 2 本だけ）。`pipe_intake_` は親の `pipe.rs` の 1 本にも当たる（親は write-set に在る）。
- 形（純移動・番号は done と 1:1。行 b の write-set の `+` の 3 file は**宣言順に**〔審査の面・契約の面・断りの面〕を受ける）:
  1. 審査の段の歯 `pipe_review_` 12 本を `+` の 1 本目（審査の面）へそのまま移す。
  2. 契約表と閉包の歯 40 本（`contract_` で始まる 39 + `pipe_contract_` 1）を `+` の 2 本目（契約の面）へそのまま移す。
  3. 断りの語彙の歯 `pipe_refuse_` 8 本を `+` の 3 本目（断りの面）へそのまま移す。
  4. 単発の `pipe_state_` 1 本と `pipe_show_` 1 本は親（`crates/scribe2/tests/e2e/pipe.rs`）へ移す。
  5. 元の file には受付の口の歯 35 本（`pipe_intake_` 31 + `pipe_preflight_` 4）だけが残り、2943 行は約 800 行へ縮む。
  6. 親に `mod` 宣言を 3 本足す（宣言は既存の 8 本と合わせて名の昇順）。歯の本文・名・順序・`#[test]` の総数 97 は変えず、子は親の helper を `use super::*;` で引く。
  7. 札 `// flip-check: moved s2-07l.351` を元の file の歯の区間の先頭と `+` の 3 file の先頭に対で置く（純移動の機械証明は §5.3）。
- 触らない: 歯の名・本文・本数・親の 40 本の helper・`prop.rs` / `polarity.rs` / `rules.rs`（filter が当たるだけで中身は触らない）・受付の口の src。
- 却下: `contract_` 1 本の filter で verify を書く（9 file に当たり受付が断る）／歯の名を変えて接頭辞を揃える（純移動でなくなり機械証明が残差を出す）／割らずに据え置く（交差の母集団が減らない）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "pipe/cli.rs を cli/args.rs / state.rs / show.rs / resume.rs に、e2e/pipe/lifecycle.rs を ratelimit.rs / stop.rs に割る（純移動）"
req = ["FR30"]
section = "5"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "docs/design/pipeline.md", "docs/design/dispatcher.md", "docs/design/working-memory.md", "docs/design/contract-source.md", "docs/design/account-autonomy.md", "docs/design/consumer-sync.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_"]
size = "S"
done = "pipe/cli.rs が入口と shim だけになり、lifecycle.rs が ratelimit.rs / stop.rs に割れて、歯の本数と外形 snapshot が不変"

[[contract]]
id = "b"
title = "受付の e2e の hub（2943 行・歯 97 本）を接頭辞ごとに 3 file へ割る（純移動）"
req = ["FR30"]
section = "42"
write-set = ["-crates/scribe2/tests/e2e/pipe/intake.rs", "+crates/scribe2/tests/e2e/pipe/review.rs", "+crates/scribe2/tests/e2e/pipe/contracts.rs", "+crates/scribe2/tests/e2e/pipe/refuse.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/prop.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_review_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_closure_ext_ contract_derive_ contract_check_ contract_declared_ contract_table_landed_ contract_schema_ contract_names_ pipe_contract_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_refuse_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_state_ pipe_show_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_ pipe_preflight_"]
size = "S"
done = "(1) 審査の段の歯 12 本が + の 1 本目に在り (2) 契約表と閉包の歯 40 本が + の 2 本目に在り (3) 断りの語彙の歯 8 本が + の 3 本目に在り (4) 単発の pipe_state_ / pipe_show_ の 2 本が親の tests/e2e/pipe.rs に在り (5) 元の file には受付の口の歯 35 本だけが残って行数が 2943 から約 800 へ縮み (6) 親の mod 宣言が 3 本増えて歯の名・本文・順序と #[test] の総数 97 は不変で子は use super::* で親の helper を引き (7) 札 flip-check: moved s2-07l.351 が元の file と + の 3 file に対で在って純移動の機械証明の残差が mod 宣言と札だけ"
depends = ["a"]

[[contract]]
id = "c"
title = "入口の flip check の免除経路を閉じる — docs-only の面・札の形と上限・push(main) の出所・宣言の VerifyKind"
req = ["FR7", "FR17", "FR50"]
section = "7"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck/git.rs", "crates/xtask/src/main.rs", "crates/xtask/src/limits.rs", "+crates/xtask/src/provenance.rs", "crates/xtask/src/flipcheck_tests.rs", "crates/xtask/src/flipcheck_declaration_tests.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap", ".github/workflows/ci.yml", "docs/design/pipeline.md", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_docs_only_ flip_marks_ provenance_", "cargo nextest run -p scribe2 --no-tests=fail declaration_kind_", "cargo nextest run -p scribe2 --no-tests=fail rules_flip_"]
size = "M"
done = "rules 行だけの便が no-test-diff で落ち、札は形と上限で止まり、push(main) の CI が出所を測り、cargo の行を持ちながら入口の flip を撃たない宣言が intake で断られる"

[[contract]]
id = "d"
title = "runner / lens の prompt 全文を外形 snapshot 2 本で pin する"
req = ["FR5"]
section = "6"
write-set = ["crates/scribe2/src/headless/runner.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_runner_prompt_external_form"]
size = "S"
done = "runner の prompt 全文の snapshot が足され（lens 側は既存の named snapshot lens_prompt_external_form）、prompt 本文の変更が .snap 差分として PR に現れる"

[[contract]]
id = "e"
title = "純移動の機械証明が base から持ち越した札（moved 以外）を新規の札と読まない — 両側で同じ字面の札を対にして残差から外し、対の無い札だけを ForeignMarker にする"
req = ["FR9", "NFR1"]
section = "7"
write-set = ["crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_move_proof_carried_"]
size = "S"
done = "持ち越しの札を持つ純移動が要約で lens に渡り、新規・id 違い・消えた札は従来どおり diff で渡る"

[[contract]]
id = "i"
title = "pipe retire の終端の列挙に Reviewed の非 PASS（FAIL / INCONCLUSIVE）を足す — 審査の段で終端した便の残った worktree を可逆 move で畳み、PASS と読めない判定は断る"
req = ["FR49", "FR14"]
section = "12"
write-set = ["crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_retire_"]
size = "S"
done = "(1) retire が受ける段の列に Reviewed が在り (2) Reviewed で verdict が FAIL の便と INCONCLUSIVE の便がそれぞれ pipe retire で retired/ へ畳め〔pipe_retire_reviewed_fail_ / pipe_retire_reviewed_inconclusive_〕 (3) PASS は断られ〔pipe_retire_reviewed_pass_refused_〕 (4) 判定を読めない便も断られ〔pipe_retire_reviewed_unreadable_refused_〕 (5) 断りの 2 本が字面 run <id> の段は Reviewed である（verdict=<V>）を逐語で測って段違いの一般則（verdict の括弧を持たない）と区別が付き (6) 畳んだ 2 本が畳んだ後の段を Reviewed のまま・RunStage detail=retired を対で測る"

[[contract]]
id = "j"
title = "xtask の flipcheck.rs から git / tar で base を取り出す群を flipcheck/git.rs へ割る — 純移動・呼び手は pub use で不変・札 moved"
req = ["FR7"]
section = "13"
write-set = ["-crates/xtask/src/flipcheck.rs", "+crates/xtask/src/flipcheck/git.rs", "crates/xtask/src/flipcheck_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_"]
size = "S"
done = "git 群 10 関数が子 module に在り、親は mod 宣言と pub use だけが増えて呼び手と歯の import は不変、既存の flip_ の歯が全部緑で純移動の機械証明が残差 0"

[[contract]]
id = "f"
title = "未知の flag と --help を全 subcommand が typed に断る — 引数の reader を 1 module に集め、land が unknown flag で何も動かさない"
req = ["NFR4", "FR12"]
section = "14"
write-set = ["+crates/scribe2/src/cli_args.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/account/cli.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/seat/account.rs", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail cli_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_args_unknown_ pipe_land_args_help_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail fleet_args_ seat_args_ headless_args_ vessel_args_ account_args_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form fleet_external_form seat_usage_external_form headless_external_form vessel_external_form"]
size = "M"
done = "(1) 共通の reader が parse(args, allowed) -> Result<Parsed, ArgsError> の 1 本で在り (2) ArgsError が Help / Unknown / Missing / Duplicate の閉じた enum で宣言順の as_str を持ち (3) --help と -h は allowed に無くても Help で返り呼び手が usage を出して rc 0 で終わり (4) Unknown / Missing / Duplicate は usage 1 行で rc 2 になり (5) 既存の reader 7 本が全部この呼出に置き換わって各 subcommand が宣言順の const の allowed を持ち (6) 偽 remote の toy repo で pipe land に未知の flag を渡すと main の ref・event log・worktree が 1 つも動かず rc 2 になり〔歯 pipe_land_args_unknown_〕 (7) 同じ toy repo で pipe land に --help を渡しても main の ref・event log・worktree が 1 つも動かず usage を出して rc 0 で終わり〔歯 pipe_land_args_help_・2026-09-15 の回帰そのもの〕 (8) fleet / seat / headless / vessel / account の 5 口が 1 口ずつ名指しの歯で未知の flag を rc 2 に断り〔fleet_args_ / seat_args_ / headless_args_ / vessel_args_ / account_args_〕 (9) usage の外形 snapshot 5 本が 1 字も動かない"

[[contract]]
id = "g"
title = "pipe の --repo と --state-dir の cwd fallback を落とす — 写し面を消した run に --repo 無しで spawn しても cwd の repo に落ちない"
req = ["FR4", "FR39"]
section = "15"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_repo_required_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "S"
done = "(1) --repo の無い spawn が cwd を読まず flag 不在の断り 1 行で rc 1 になり〔pipe_repo_required_spawn_〕 (2) 写し面の無い周の run の repo 解決も同じ断りで止まり〔pipe_repo_required_run_〕 (3) --state-dir も --repo も無い（--repo だけ在る周は救われる）置き場の解決も同じ断りで止まり〔pipe_repo_required_state_dir_〕 (4) 上の 3 本がそれぞれ「worktree が 1 つも増えず event が 1 件も増えない」を対で測り (5) usage に --repo の要件の 1 句が載って pipe_external_form の snapshot がその 1 句だけ動く"

[[contract]]
id = "h"
title = "runner / lens の effort を rules 行 runner.effort から毎回渡す — build が --model と同じ場所で --effort を渡し、行が無い・表に無い値は rc 2"
req = ["FR5", "FR9"]
section = "16"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_runner_passes_effort_ headless_lens_passes_effort_ headless_effort_row_refuses_"]
size = "S"
done = "偽 claude で runner と lens を撃つと argv に effort の値が rules 行のとおり載り、行の無い manifest と表に無い値は rc 2 で claude の呼出 0"

[[contract]]
id = "k"
title = "lens の verdict に findings の閉じた category と件数・母集団を必須 key にする — 欠落と母集団 0 は INCONCLUSIVE"
req = ["FR9", "NFR1"]
section = "17"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/lens.rs", "+crates/scribe2/src/pipe/gate/findings.rs", "crates/scribe2/src/headless/lens.txt", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_findings_", "cargo nextest run -p scribe2 --no-tests=fail lens_prompt_external_form"]
size = "M"
done = "lens の verdict が category ごとの件数と母集団を必ず持ち、欠落は INCONCLUSIVE"

[[contract]]
id = "l"
title = "land の stale base を同じ経路で人手なしで追随し直す（resume の Gated(PASS) 受けは既在・同じ周回を通る）"
req = ["FR30", "FR50"]
section = "18"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_stale_", "cargo nextest run -p scribe2 --lib --no-tests=fail follow_stale_"]
size = "S"
done = "toy repo で stale base の便が同じ land の中で人手なしで追随して Landed し、上限は合算の回数で typed な Failed"

[[contract]]
id = "m"
title = "着地列の窓の待ちを Completion に足し、pipeline 外の docs merge が pipe land-window を前置して撃つ"
req = ["FR30", "FR50"]
section = "19"
write-set = ["crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_window_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_wait_land_window_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_wait_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_external_form"]
size = "S"
done = "(1) 唯一の wait に pid を見張らない窓の variant が 1 つ増え、既存の 7 つと deadline の経路が不変（wait.rs の既存の in-file の歯 8 本が全部緑のまま） (2) 窓は列の PASS 0 本・追随中 0 本・local main が origin/main の祖先の 3 つ全部で開き、閉じる条件ごとに 1 本ずつ歯が在る〔fleet_wait_land_window_queue_ / _following_ / _unpushed_〕 (3) local を先に読み読めない周は origin の有無に依らず閉じ〔_unreadable_〕origin の無い周は remote=none を載せて 3 つ目を数えず〔_remote_none_〕 (4) origin の ref を fetch せず git の読みが既存の 2 口だけで (5) pipe land-window が開いた周は rc 0 の clear・閉じた周は rc 1 の busy を返し〔pipe_land_window_clear_ / pipe_land_window_busy_〕 (6) busy の行が unpushed=<sha|unreadable|-> の 3 値でどの条件で閉じたかを示し (7) usage の 1 行が増えて pipe_external_form の snapshot がその 1 行だけ動く"

[[contract]]
id = "n"
title = "runner の雛形に turn 終端の規律を足し、片付けで殺した子の数を record に残す"
req = ["FR5", "FR22"]
section = "20"
write-set = ["crates/scribe2/src/headless/runner.txt", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_runner_prompt_closes_turn_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_runner_prompt_external_form", "cargo nextest run -p scribe2 --lib --no-tests=fail confine_orphans_", "cargo nextest run -p scribe2 --lib --no-tests=fail runner_orphans_"]
size = "S"
done = "(1) 雛形に「検証は前面で完走させてから turn を閉じる（背景実行を残して終えない）」の 1 行が在り (2) その 1 行を逐語で名指す歯が別に在り〔headless_runner_prompt_closes_turn_〕 (3) 雛形の外形 snapshot が足した 1 行だけ動いて全文を pin し続け〔headless_runner_prompt_external_form〕 (4) 片付けが scope を止める直前に残った process の数を読み〔confine_orphans_counts_before_release_〕 (5) 終端の 1 行に orphans=<n|-> が載り、n=0 の周と読めない周の 2 本の歯が 0 と「測れなかった」を融合しないことを測り〔runner_orphans_zero_ / runner_orphans_unreadable_〕 (6) 数える関数（pipe/confine.rs）と行を組む関数（headless/runner.rs）が pure に切られて systemd の scope を起こさない fixture でそれぞれの in-file の歯が測る"

[[contract]]
id = "o"
title = "gate の段の通知行を rc に依らず record と run の stderr に残す"
req = ["FR8", "FR22"]
section = "21"
write-set = ["crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_notice_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_notice_"]
size = "S"
done = "(1) rc 0 で終わった段の err が捨てられず pipe run の stderr に段の順で出て〔pipe_spawn_notice_〕 (2) 同じ歯が対で stdout の判定行が base と 1 字も変わらないことを測り (3) gate の記録の step 行と同じ log に lens-input=<kind> reason=<語> が要約の周も diff の周も残り〔pipe_gate_notice_summary_ / pipe_gate_notice_diff_ の 2 本〕 (4) kind が diff / summary で語は要約の周が - になり（0 と測れなかったを融合しない） (5) 記録の歯が tests/e2e/pipe/gate.rs に・pipe run の stderr の歯が tests/e2e/pipe/spawn.rs に在る"

[[contract]]
id = "p"
title = "着地の番を取った事実を記帳し、撃ち直しの間も番を鍵の順と独立に列の先頭に残す"
req = ["FR50", "FR30"]
section = "22"
write-set = ["crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_land_turn_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_taken_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_order_record_values_are_the_closed_four pipe_order_key_is_the_first_gated_ts"]
size = "S"
done = "(1) 番を取った周に RunStage stage=Gated detail=turn:taken が 1 行だけ増え EventKind は増えず〔pipe_land_turn_taken_〕 (2) 縮退と測れなかった周は 1 行も書かず〔pipe_land_turn_not_taken_〕 (3) 偽の列で turn:taken を持つ便が在れば最新の 1 本（同時刻は run id の辞書順）だけが先頭になり無ければ鍵の順に戻り〔pipe_order_taken_latest_ / pipe_order_taken_falls_back_to_key_〕 (4) 終端・worktree 無し・verdict が PASS でない便の turn:taken は数えず INCONCLUSIVE で離れて戻った便が撃ち直し中の便を追い抜かず〔pipe_order_taken_leaver_〕 (5) Queued が最新の turn:taken の ts の field を持ち、turn_in が pure で Turn が閉じた 3 値のままなこと・鍵が最初の Gated の ts のままなことを既存の in-file の歯 2 本（pipe_order_record_values_are_the_closed_four / pipe_order_key_is_the_first_gated_ts）が緑のまま示す"

[[contract]]
id = "q"
title = "pipe stop 起因の終端を oom-kill に誤分類せず、kernel の証拠が無い kill は unknown に倒す"
req = ["FR22", "FR46"]
section = "23"
write-set = ["crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_terminal_reason_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_reason_vocabulary_"]
size = "S"
done = "stop した便が oom-kill に分類されず、証拠の無い kill は unknown"

[[contract]]
id = "r"
title = "pipe retire が受ける終端の段を Stage の終端全部（Failed は detail 不問）と Gated(FAIL) に広げる — 残る穴は discriminate の Failed の detail の弁別だけ"
req = ["FR34"]
section = "24"
write-set = ["crates/scribe2/src/pipe/cli/state.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_retire_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_follow_retire_"]
size = "S"
done = "(1) Failed の便が detail を問わず pipe retire で畳め、base で断られていた 4 つの detail（main-red / main-unmeasured / rebase-dirty / precheck:…）を 1 つずつ名指す歯がそれを測り〔pipe_retire_failed_any_detail_〕 (2) 受ける集合が Stage の終端全部（Landed / Stopped / Failed）∧ Gated(FAIL) になり (3) 畳んだ歯が対で clean の検査・retired/ への可逆 move・RunStage detail=retired の記帳・allowed の列が不変なことを測り (4) 非終端（Spawned / Implemented）と Gated(PASS) は断られる〔pipe_retire_failed_any_detail_still_refuses_live_〕 (5) 既存の pipe_retire_ 6 本 / pipe_follow_retire_ 2 本（母集団 = tests/e2e/pipe/land.rs の歯全数）は極性が反転する 1 本（pipe_retire_rebase_empty_refuses_other_failed_reasons）を除き不変"

[[contract]]
id = "s"
title = "純移動の要約（MoveSummary）のコメント行の差に base 側 - / head 側 + の逐語を載せ、件数は母集団と対で残す"
req = ["FR9"]
section = "25"
touches = ["crate::pipe::move_proof::CommentDiff"]
tests = ["crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail move_proof_comment_verbatim_"]
size = "S"
done = "コメント行だけが違う純移動の要約に、違う行の逐語が - / + 付きで件数の行の下に並ぶ"

[[contract]]
id = "t"
title = "pipe review が受けた lens の cmd を run dir の lens.toml に写し、gate / land / resume が --lens の無い周にそれを読む"
req = ["FR10", "FR9"]
section = "26"
write-set = ["+crates/scribe2/src/pipe/lens_record.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_lens_record_"]
size = "S"
done = "review で渡した lens だけで land の再 gate が lens を起動して着地し、写しが読めない周は「無い」と別の理由で INCONCLUSIVE"

[[contract]]
id = "u"
title = "pipe land の終端が refs/heads/main を実測し、stdout の main= と Landed の detail の main: に写す"
req = ["FR50"]
section = "27"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_main_measured_"]
size = "S"
done = "reference-transaction hook で main を動かす toy repo の land が、landed= と違う main= を stdout と detail に写し rc 0 で終端する"

[[contract]]
id = "v"
title = "e2e の pipe の helper が binary を git repo でない temp dir を cwd にして起こす — cwd の fallback が本番の state dir へ届かない"
req = ["NFR6"]
section = "28"
write-set = ["crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/launch_failure.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/stop.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_hermetic_"]
size = "S"
done = "(1) tests/e2e/pipe.rs に binary を起こす関数 bin_cmd が新設され Command を返して cwd を git repo でない temp dir に固定し (2) 親の helper 3 本 run_pipe / run_pipe_with_path / land_once_with_git_shim のそれぞれで --state-dir も --repo も無い pipe show が「repo の root を解決できない」で rc 1 に断られ（3 本を 1 本ずつ名指して測る） (3) tracked の 9 file を読む歯が Command::new(bin()) と Command::new(super::bin()) の出現の合計を 1（bin_cmd の中）と測り読んだ file 数と base の 41 site を母集団として同時に出し (4) cwd が主題の 2 site（pipe/launch_failure.rs の相対 --repo の歯と pipe/dispatch.rs の 1 箇所）は bin_cmd の返り値に自分の current_dir を後置して主題を保つ"

[[contract]]
id = "w"
title = "pipe land が rebase-empty の周に main の log を run: trailer で探し、在れば squash と CAS を撃たず主実測の後に Landed（already-landed）で終端する"
req = ["FR50"]
section = "29"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_already_landed_"]
size = "S"
done = "自分の trailer を持つ squash が main に在る便の land が main を動かさず主実測を撃って Landed で終端し stdout と detail に already-landed を持ち、trailer が無い周と別の便の trailer の周は従来どおり rebase-empty"

[[contract]]
id = "x"
title = "検出線は main の差分が DETECTION_SCOPE に触れた周だけ撃つ — 追随の再 gate は Gate の印で、主実測は diff-tree で判定し、飛ばした周は reason 付きで record する"
req = ["FR46"]
section = "30"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
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
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_follow_docs_only_"]
size = "S"
done = "docs だけで main が進んだ周は land が lens を呼ばず再 gate せずに Gated PASS を引き継いで着地し、crates/ が進んだ周と diff を読めない周は従来どおり再 gate する"

[[contract]]
id = "ab"
title = "追随で入った契約表の行が便の消した path を名指す周は rebase-stale-rows で記帳して runner を起こし直す — 写しの write-set にその設計 doc を追記し、回数は衝突と同じ上限、他の findings は従来どおり再 gate"
req = ["FR34", "FR47"]
section = "34"
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_follow_stale_rows_"]
size = "M"
done = "便が消した path を名指す行が追随で入った周は land が rebase-stale-rows を記帳して runner を起こし直し、写しの write-set にその設計 doc が追記され再 gate は撃たれず、無関係の findings は従来どおり再 gate へ進み、上限に達した周は Failed で終端し、--runner の無い land は記帳して rc 1 で止まる"

[[contract]]
id = "ac"
title = "verify の record に failed=<歯の名> と落ちた歯ごとの stderr の区間を残す — nextest の FAIL 行と stderr の小見出しだけを読む pure な関数 1 本を gate と主実測が共有する"
req = ["FR50", "NFR4"]
section = "35"
write-set = ["crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_verify_failed_"]
size = "S"
done = "nextest 形の stderr から failed= が最初の落ちた歯を指し、落ちた歯ごとの区間が上限行数で record に残り、FAIL 行の無い stderr は従来どおり末尾だけ、gate と主実測が同じ関数を通り、MainCheck の 3 値と末尾行数の定数は不変"

[[contract]]
id = "ad"
title = "着地の列が driver の死んだ便を先頭に数えない — Queued に札の生死の 3 値を足し、死んでいる便だけを ahead から外して skipped-dead で名指し、段は動かさない"
req = ["FR11", "FR14"]
section = "36"
touches = ["crate::pipe::queue::Queued"]
tests = ["crates/scribe2/tests/e2e/pipe/land.rs"]
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
write-set = ["crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_onto_"]
size = "M"
done = "base が main の祖先でなく merge-base の在る便の land が rebase --onto で追随して Landed になり、衝突は既存の rebase-conflict の経路へ、merge-base の無い main は従来どおり stale base の rc 1 で event 0 増"

[[contract]]
id = "ag"
title = "pipe stop --run が段を問わず便を終端にする — store の条件付き append で Stopped の後の RunStage / RunDone / SeatSpawned を lock の中で断り、stop は札の運転手の process group を席と同じ 1 関数で止めてから RunStopped を書く"
req = ["FR13", "FR11"]
section = "39"
write-set = ["crates/scribe2/src/fleet/store.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_stop_driver_"]
size = "M"
done = "RunStopped の後の段の記帳が lock の中で断られて event が増えず、pipe stop --run が札の運転手を止めて記録を残し、自分自身と札の無い便は従来どおり、止め切れない周は RunStopped を書かず rc 1"

[[contract]]
id = "ah"
title = "着地の列の先頭 N 本を候補の木 1 つに積み検査を 1 回撃って列の順に着地し、赤なら列を解く（merge train・rules 行 land.train_max〔4・裁定 id user 2026-09-17T03:28Z〕）"
req = ["FR34", "FR10", "FR12", "FR50"]
section = "40"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2/src/pipe/train.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_train_", "cargo nextest run -p scribe2 --no-tests=fail rules_land_train_"]
size = "M"
done = "偽の列 3 本が候補の木 1 つの検査 1 回で列の順に着地して追随 0 回、赤の周は列を解いて先頭だけが従来の経路で着地し、上限 1 と行の不在は現行と同じ"

[[contract]]
id = "ai"
title = "検出線は便の diff の patch-id を record に残し、追随の撃ち直しと候補の木の段で patch-id 不変なら前周の record を carried 付きで写して撃たない"
req = ["FR34", "FR46"]
section = "40"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/land.rs", "+crates/scribe2/src/pipe/train.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_detection_carry_"]
size = "S"
done = "gate の検出線の record が patch_id を持ち、diff 不変の追随と候補の木の段は検出線を撃たず carried=<n> の写しを残し、違う周と record の無い周は撃つ"
depends = ["ah"]

[[contract]]
id = "aj"
title = "pipe/land.rs の主実測の群（verify_main / main_red / main_unmeasured ほか 9 item と const 3 つ）を land/verify.rs へ割る — 純移動・MainCheck は親に残す（極性一覧の pin）・land の本体は不変・札 moved"
req = ["FR11"]
section = "41"
write-set = ["-crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/tests/e2e/polarity.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_land_subject_", "cargo nextest run -p scribe2 --no-tests=fail polarity_external_form"]
size = "S"
done = "主実測の群 9 item と const 3 つが子 module に在り、MainCheck は親に残って極性一覧の snapshot が不変、親は mod 宣言と use だけが増え（子側の pub(super) 4 語〔measure_main は finish が呼ぶ〕で land と finish の本体は不変）、file-lines で land.rs の余地が base より 180 行以上増え、in-file と e2e の歯が全部緑、純移動の機械証明の残差が use と path と可視性の語だけ"
<!-- contracts:end -->
