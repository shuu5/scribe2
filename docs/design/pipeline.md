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
- **機械検証**（順序は [ADR-0009 §2.4](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-4-common-verify)）: ① **write-set の照合**を Rust の 1 関数で行う（`git diff --name-only <base>..HEAD` の各 path が契約 write-set のいずれか〔file 一致 or dir prefix〕に含まれる・外れが 1 件でも赤・外れた path を `verify.stderr.log` に列挙。**diff の path を読めない周は赤ではなく「測れなかった」**＝record は残し（rc は u64 の記録形 255・`verify.stderr.log` の見出しは -1）、②③ は従来どおり撃って record し（費用は ②③ 分・record を欠かさないため）、判定は lens を呼ばずに INCONCLUSIVE〔既存 3 値の内側〕へ倒す。land の `main-unmeasured` と同じく「赤ではない」側だが、land が `Failed` で終端するのに対し gate は `Gated` に留まり測り直せる（FR14）。段①の -1 は `verify_red` に数えない・`s2-07l.65`）→ ② run の写し `vessel.toml` の `common-verify` 各行（[ADR-0010 §2.4](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-4-consumers)・manifest の行ではない・`{base}` を便の base に置換・repo 共通の検証＝Rust repo なら flip check・test・lint・依存監査）→ ③ 写しの `detection-verify` 各行（任意 key・検出線＝変異検出・穴は②と同じ・rc 1〔R-C12-1 が deny に昇格した周だけ現れる〕は②と同じく赤・**rc 2〔測れなかった・道具の不在・baseline 落ち〕は赤に数えず判定を INCONCLUSIVE へ倒す**〔`Gated` に留まり測り直せる・FAIL にして runner をもう 1 周払わせない・`s2-07l.331`〕・木が gate と同じ main 実測では撃ち直さない・[gate-cost.md](./gate-cost.md) §5・ADR-0021 §2.4）→ ④ contract の `verify` 各行（便固有の行だけ）。**① は Rust の照合で `sh -c` を撃たない**（record の `cmd` は段の名 `write-set`）・②③④ は worktree で `sh -c` 実行し、①〜④を**通し番号 `n`** で並べて行ごとの rc を `<state_dir>/pipe/<run>/verify.jsonl`（`{"schema":1,"n":<i>,"rc":<rc>,"cmd":"…"}`）に逐条記録。**rc≠0 の行だけ、その行の stderr の末尾 20 行を `<state_dir>/pipe/<run>/verify.stderr.log` へ見出し行（`## n=<i> rc=<rc> cmd=<cmd>`）付きで append する**——rc だけでは「何がどう赤いか」が便の外から読めず、落ちるたびに人が同じ行を手で撃ち直して理由を取り直すことになる。`verify.jsonl` の record の形は変えない（跨版の契約ゆえ不変）＝`verify.stderr.log` は**機械が読まない診断 file** で、緑の行は残さない（読む理由の無い出力で埋めると赤い行の見出しが埋もれる）。
- **lens**: 本数 = rules 行 `gate.lens_count`（MVP は 1）、cap = `gate.token_cap`。**本数は照合する**: `gate.lens_count` が 1 でない周（0 = lens を呼ばずに通す / 2 以上 = 1 本で足りたことにする）は「lens の verdict」を得ていないので **INCONCLUSIVE**（多 lens は (b) の射程外なので、実装しない代わりに fail-closed に断る）。`--lens <cmd>` に `git diff <base>..HEAD` を stdin で渡し、stdout の JSON 1 行 `{"verdict":"PASS|FAIL|INCONCLUSIVE","evidence":"…"}` を採る。**cmd の `{contract}` / `{worktree}` は run の path へ置換する**（`--runner` 側と共有するのは placeholder の語彙であって置換関数ではない・置く穴は **2 つ**〔`{contract}` / `{worktree}`〕・出所 `s2-07l.60`）。穴が 2 つ目を持つのは、lens に憲法（生成区間を持つ `CLAUDE.md`）を載せる経路が**起動 cwd 1 本**で、tracked file に絶対 path は書けない（PUBLIC repo）ため worktree を gate が埋めるほかないからである。置換は **1 走査**で行う（重ねて replace すると先に埋めた path の中の `{worktree}` まで展開されうる）——lens に問うのは「diff が**契約の**求めるものを満たすか」なので、diff だけを渡すと実 lens は「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる（実測 2026-09-10・`s2-07l.24` の実 5 便）。**渡すのは path であって本文ではない**（cmd は `sh -c` の 1 行ゆえ、本文を埋めると契約の中の引用符 1 つで cmd の構造が変わる）。**穴を持たない古い `--lens` は fail-closed に落ちる**——`{contract}` / `{worktree}` を書いていない cmd へ `<NAME> lens` を渡すと lens 自身が rc 1 で断り（`--worktree` は `--contract` と同じ必須 flag で、無ければ claude を起こさない＝憲法の載らない判定を出さない）、gate は判定順の 3 番目で INCONCLUSIVE にする（極性は正しいが、gate は lens の stderr を捨てる〔`Stdio::null()`〕ので evidence に残るのは「lens が rc 1 で終わった」だけ＝**理由は lens を手で 1 回叩いて読む**）。
- **予算の照合**（NFR1「diff byte と cap の照合」）: diff の byte 数を `gate.token_cap` と**直接比べる**（byte ≥ token の保守的な読み・換算係数を持たない）。diff byte > cap → **INCONCLUSIVE**（lens を起動しない）。
- **純移動の機械証明**（`s2-07l.266`・user 裁定 2026-09-14「それでよい。後で戻すのを忘れないで」＝分割便が cap に当たる問題の恒久解・cap を一時的に上げた `s2-07l.265` の対・戻しは `s2-07l.267`）: 予算の照合の**前**に、diff が**純移動**かを純関数（`pipe/move_proof.rs`・I/O は gate 側）で判定する。**item** = base と HEAD の「diff に現れる `.rs` file」で **列 0 から始まる宣言単位**（`fn` / `struct` / `enum` / `impl` / `trait` / `const` / `static` / `type` / `mod <name> {`・直前に連なる属性行と doc コメント〔`///` / `#[…]`〕を含む・終端は列 0 の `}` か次の item の開始＝入れ子〔`impl {}` の中の fn・inline `mod tests {}` の中の歯〕は外側の item 1 本の本文に含める）。本文の正規化は **行頭の indent の除去**と**コメント行の除外**（`s2-07l.294`・2026-09-14 の .286 = gate.rs の 3 module 分割が 2 周とも純移動と読まれなかった型: module を跨ぐ移動では doc コメントの intra-doc link の path〔`[`super::x`]` → `[`crate::…::x`]`〕の書き換えが常に要る＝コメントは挙動を持たないので、item の区間のうち `//` / `///` / `//!` で始まる行〔行頭の indent の後〕は hash に入れない。**札の字面**〔`// flip-check:` で始まる行〕だけは除外せず従来どおり残差の検査に掛ける〔`retroactive` を item の中に隠す形を作らない・`ForeignMarker`〕。除外したコメント行の差は item ごとに数えて要約に載せる〔「コメント行の差 N」・lens が読める〕）。行内の空白と文字列 literal は変えない（pane の字面を持つ歯の literal を潰さない）。**可視性の prefix**（`pub` / `pub(crate)` / `pub(super)`）は item の名の前から剥がして hash に入れず、剥がした前後を item ごとに記録する（子 module へ出した helper は必ず可視性が広がる＝`s2-07l.257` の「本文字面不変・可視性と改名のみ」と同じ扱い）。(名, 本文の hash) の**多重集合**が HEAD と base で一致（追加 0・削除 0・本文差 0）し、**移動した item が 1 つ以上**在り、**残差分**（両側の diff 行のうちどの item の区間にも入らない行）が `mod` / `use` / `pub use` / `#[path]` / `#[cfg(test)]` / `// flip-check: moved <id>` の宣言と札、**item に付かない裸のコメント行**（`//` / `//!` / `///`・module doc と区切り線）、空行だけなら**純移動**（`s2-07l.261` の diff = `pub(super)` 化 9 行 + `//!` 31 行 + 区切り 26 行がこの形に当たる＝本機構の出所の便を通す基準）。純移動の周は lens の入力を diff でなく**要約**（型 `MoveSummary`: file ごとの item の移動元 → 先と本数・行数・可視性が変わった item の一覧〔名 + 前 → 後〕・宣言と札の残差分〔逐語・小さい〕・「名 + 本文の多重集合が一致」の判定行 1 本）にし、雛形 `lens.txt` の `{diff}` の穴に**そのまま**入れる（雛形は変えない＝`lens_prompt_external_form` の snapshot は動かない・要約の先頭行が「これは diff ではなく純移動の要約である」と名乗る）。予算の照合は lens に渡す本文の byte で行い、`verdict.json` の `diff_bytes` は従来どおり diff の byte のまま（意味を変えない・要約の byte は判定行に出す）。lens への入力は閉じた型（`LensInput::Diff` / `LensInput::Summary`・C3.3 の判定入力）で運び、要約の本文は run dir に `lens-input.txt` として残す（事後に読める・NFR4）。純移動でない周は従来の diff。結果は gate の stdout の判定行に `lens-input=<diff|summary> bytes=<N>` として出す（gate の外形 snapshot が動く周は同じ便で更新）。**極性**（C11.2 / C16.2）: 純移動の誤判定は lens から diff を奪う側に倒れる（FailOpen・PostHoc）ので `Guard` に variant 1 つ（`MoveProof`）を足し、極性一覧に載せる（in-loop の本数は変わらない・行数 +1）。切り出しの立場は閉包（[contract-source.md](./contract-source.md) §3）と同じ**下界**（構文木を持たない・A3 の依存を足さない）: macro で生成する item・1 行に複数の item は純移動と判定しない（保守側に倒れ lens が diff を読む従来形になる）。item の中のコメント行だけの差は hash に入らない（上の正規化・`s2-07l.294`・要約に件数が載る）。歯: `s2-07l.261` と同型の fixture（1 file → 複数 file の移動・`pub(super)` 化・module doc・区切り線つき）で lens 入力が要約になり verdict が読める／本文を 1 行変えた fixture は純移動でなく diff が渡る／宣言と札とコメント以外の行が残る fixture も diff が渡る／移動 item 0 の fixture（宣言だけ）は純移動でない／要約の外形は snapshot／判定の純関数は in-file（可視性の剥がしと入れ子の切り出しを直接撃つ）。**持ち越しの札**（`s2-07l.362`・契約表の行 e）: base に元から在る `moved` 以外の札（`retroactive` 等）が item ごと head へ移る周は、両側で同じ字面（id まで）の札を**対にして**残差から外し、対の無い札（head だけの新規・id 違い・base だけの消えた札）だけを `ForeignMarker` にする＝移動の中に新しい札を隠せない意図は保ったまま、持ち越しを新規と読まない。要約は持ち越した札の本数を 1 行で名乗る。
- **判定順**（wildcard 無しの match）: 箱の中で殺された行（**検出線以外の行**の `oom_kill`・どの行でも包みごとの signal 死＝[gate-cost.md](./gate-cost.md) §4.2）が在る → **INCONCLUSIVE**（赤より先）／ 検出線の行（`kind=detection`）が rc 2（測れなかった）→ **INCONCLUSIVE**（赤より先・理由に行番号と「検出線が測れなかった」・`s2-07l.331`）／ verify に rc≠0 が 1 本でも（検出線の rc 2 を除く）→ **FAIL** ／ diff byte > cap → **INCONCLUSIVE** ／ lens が要るのに `--lens` 無し・lens rc≠0・stdout が parse 不能・verdict が 3 値外 → **INCONCLUSIVE** ／ それ以外は lens の verdict。
- **記録の追加**（[gate-cost.md](./gate-cost.md) §5.1・**契約 land 後**）: `verify.jsonl` の `kind=detection` の行だけ `line=<stdout の末尾の非空 1 行・逐語>` を持つ（検出線の値・parse しない・無い周は欠く）。
- 結果は `<state_dir>/pipe/<run>/verdict.json`（`{"schema":1,"run":…,"verdict":…,"evidence":…,"verify_red":<n>,"diff_bytes":<n>,"ts":…}`）と `RunStage stage=Gated detail=verdict:<V>`。stdout `run=<id> verdict=<V>`・rc は PASS=0 / FAIL=1 / INCONCLUSIVE=3。

### 5.4 land（(b)・FR10 / FR11 / FR12・N1）
- main 実測で木の hash が gate の木と一致する周は検出線を撃ち直さず、record は `verify-main.jsonl` に書く（[gate-cost.md](./gate-cost.md) §5・ADR-0021 §2.4・**契約 land 後**）。着地の順序の原則は同 §6。
`pipe land --run <id> [--lens <cmd>]`: 前提 = Gated ∧ verdict.json が PASS（それ以外 = rc 1・**何もしない**）。`git rev-parse refs/heads/main` が記録した `base` と違う周は **追随する**（`s2-07l.119`・FR30・並行に流した便の 2 本目が先着の後に置き去りになる形）: (i) `base` が main の祖先でなければ rc 1 `stale base`（main が巻き戻った / 分岐した＝追随の形が無い・何も書かない）(ii) worktree が clean でなければ rc 1（何も書かない・汚れた木では rebase を走らせない）(iii) worktree の branch を `git rebase <main>` する——効くのは **worktree の branch だけ**で main は 1 byte も動かさず、force 系は使わない（N1）。衝突は `git rebase --abort` で木を戻す。**ADR-0019 §2.2 の形**では `RunStage stage=Implemented detail=rebase-conflict:<base>..<main>` を記帳して runner を起こし直し、回数上限で `Failed detail=rebase-conflict`（[pipeline-conflict.md](./pipeline-conflict.md) §3・契約 (b) の land まで現物は `RunStage stage=Failed detail=rebase-conflict` + rc 1 の終端）(iii′) rebase が通って便の commit が 0 本になった周（同一変更の便が先に land した）は gate を撃ち直さず `RunStage stage=Failed detail=rebase-empty` + rc 1（便の変更は既に main に在る＝close してよい合図・lens を起動しない・main 不変・`s2-07l.125`）。commit 数を読めない周は 0 に読み替えず (iv) へ進む（fail-closed の向きを変えない） (iv) `RunStage stage=Implemented detail=rebase:<old>..<new>` を追記する（段が `Gated` から `Implemented` へ戻る 1 件＝撃ち直す便の記帳。`base` の読み手〔`base_of_run` の 1 本〕はこの行の新しい側を読む）→ stdout に `run=<id> rebase=<old>..<new>` (v) §5.3 の gate を**同じ関数で**撃ち直す（機械検証 + lens・diff が変わりうる）。PASS でなければ gate の判定行と rc で止まる（FAIL は `Gated` のまま land しない・INCONCLUSIVE は測り直せる側・lens は `--lens` で渡す）(vi) 撃ち直しの間に main がさらに動いた周は rc 1 `stale base`（次の land が同じ経路で追随する＝1 回の land が rebase するのは 1 度だけで、event 列が追随の回数をそのまま語る）。追随した周も以下の手順は同じ（CAS の old は新しい base）。
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
subcommand と helper は責務ごとに 1 file に置く——入口（usage / dispatch / contracts）は `cli.rs`、引数と rules 行の helper は `cli/args.rs`、便の状態の helper は `cli/state.rs`、`show` は `cli/show.rs`、`resume` は `cli/resume.rs`。`cli.rs` は `mod` 宣言と再輸出の shim だけを持ち、`intake.rs` / `run.rs` / `step.rs` の `use super::…` は shim で解く（既存の subcommand の file を触らずに割る）。usage の字面と歯の本数は移動の前後で変えない。

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
- `--claude <path>` は test の seam（fake の実行 file が引数と stdin を file に写す）。prompt の文面は tracked な template file（`crates/<NAME>/src/headless/*.txt`）で持ち、絶対 path・口座名を含めない。
- **prompt 本文の外形**（`s2-07l.176`）: runner と lens の prompt は fixture の契約 / write-set / diff で組んだ**全文**を外形 snapshot で pin する（`headless_runner_prompt_external_form` / `headless_lens_prompt_external_form`・lens-contract と同じ型）。本文の 1 字の変更は `.snap` の差分として PR に現れ、review の入口になる（C12.5）。

## 7. FR7（入口の flip check）の置き場

本 repo 自身の flip check は `cargo xtask flip-check` と CI の job が担う。**CI の flip-check job は `s2-07l.17` で land 済み**（CI は nextest / clippy / xtask-check / flip-check / deny / insta の 6 job・flip-check は PR のときだけ撃つ）。pipeline は **vessel 宣言 `common-verify`** の 1 行として flip check を撃つ（Rust repo の行は `cargo xtask flip-check --base {base}`・契約にも manifest にも書かない・[ADR-0010 §2.1](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-1-declaration-file)・[ADR-0009 §2.4](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-4-common-verify)）。pipeline が Rust 固有の検査を内蔵する形は採らない（toy repo は Rust でないことがある）。**非空虚性（変異検出線・C12 R-C12-1）も同じ置き場**: `cargo xtask mutants-diff --base {base}` が `cargo mutants --in-diff` を便の diff に当て `total / caught / missed / unviable / timeout / scope` の 1 行を出す。`scope` は `-p` へ**実際に渡した** package 名で、現状は **core package 固定**＝xtask 側の diff は母集団に入らない（`s2-07l.82`・行は出所から切り離されて流通するので限界は報告でなく行に載せる。diff が触った package を並べて測る形は費用を測ってから別便）。rc は **3 値**である: (i) `outcomes.json` が在る → manifest の `R-C12-1` 行の極性（`enabled=false` = 記録のみ・`enabled=true` = missed>0 で rc≠0）／(ii) 無い ∧ cargo-mutants が rc 0 → `total=0` を含む 1 行で **rc 0**（**測る対象が無い**＝core を触らない便を恒久 FAIL にしない・母集団を額面に出すので「0 件の緑」と読み違えない）／(iii) 無い ∧ cargo-mutants が非 0、または**道具の不在** → **rc 2**（測れなかったを 0 に化けさせない）。**道具の rc は捨てない**——baseline（変異を当てない木）の test が落ちた周も cargo-mutants は `outcomes.json` を書く（`total_mutants=0`）ので、rc を見ないと「suite が壊れているときほど門が緑」になる。非 0 の理由が件数から説明できる周（生存・時間切れが在る）だけを測定として受ける。**前回の出力 dir は撃つ前に掃除する**（変異 0 の周は cargo-mutants が dir へ触らないので、掃除しないと前便の `total=18 missed=6` が今便の測定を名乗る・実測 2026-09-11）。**置き場は本 repo の `.vessel.toml` の `common-verify` の末尾**（`s2-07l.58` で land・先頭語 `cargo` は上限と宣言の allowlist の内）。**歯は cargo-mutants 本体を起動しない**——fixture（`outcomes.json` の 5 種と、道具の rc の 2 値）で 1 行の形と rc の 3 値だけを測る（CI に 10 分の実行を持ち込まない）。契約に変異 script・変異 anchor を書かない（[ADR-0009 §2.3](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html#s2-3-mutation-proof)）。

- **判定クラス**（語彙の SSOT は `cargo xtask flip-check` の判定行そのもの＝`judge` が stdout へ出す 1 行。本節は意味だけを持ち、README は本節への pointer だけを持つ・ADR-0013 §2.1・`s2-07l.92`。判定行の書式は ADR-0013 §2.4 のとおり pin されていないので、ここが実装と食い違ったら実装が正）。判定行は 3 形: `RED-on-base ok tests_changed=N`（rc 0・免除・同梱・base 段の撃ち直しが在るときだけ `removed-only=N` / `retroactive=N` / `moved=N` / `decl=N` / `base-retried=N` を後置。flip が 0 本でも免除だけの便は `tests_changed=0` のこの形で通る）／`skip reason=no-rust-diff`（rc 0・変更に `.rs` が 1 本も無い＝docs-only の便。runner を撃たずに通す唯一の経路）／`FAIL reason=<理由>`（rc 1）。FAIL の理由は 4 語: `green-on-base`（overlay した歯が base で緑＝TDD の不履行。1 本ずつ撃つ周〔flip が 2 本以上、または宣言 file を同梱した周〕は `file=<rel>` で緑だった file を名指す）／`no-test-diff`（`.rs` は変わったが test 区間の差が 1 本も無く、免除の札も無い）／`not-flippable`（下）／`infra-error <理由>`（道具の失敗＝git / tar / cargo の spawn 失敗・base 自身の test が緑でない `base-not-green`・base で該当 test が 0 本の `no-tests-on-base`・runner が signal で死んだ `runner-killed-by-signal`。**測れなかった**であって赤ではない）。**base 段の撃ち直し**（`s2-07l.270`・負荷下の flaky の検出線）: base の素の runner が落ち、落ちた歯を runner の出力の `FAIL` 行（binary id と歯の名）から名指せる周は、**その歯だけ**を同じ base copy で **1 回だけ**撃ち直し、通れば base 緑と読んで判定行に `base-retried=N`（N = 撃ち直した歯の本数）を後置する。2 回目も落ちる・落ちた歯を 1 本も名指せない（compile error・signal・出力の形が読めない）・撃ち直しの rc が 0 でない周は従来どおり `base-not-green`（撃ち直しは緩める側なので狭く取る＝名指せない失敗を撃ち直しで緑に化けさせない）。stderr に `base-retry <binary>::<name>` を 1 行ずつ残す。**base の実体化**（`s2-07l.280`）= `git archive` の展開 + index（`git init` / `git add -A`）+ HEAD（共有 object store を alternates で読み、base の commit を `update-ref HEAD` で置く）＝base の tracked 集合を `git ls-files` で、宣言を `HEAD:<file>` で読める git repo（`contracts check` 等 tracked 集合と HEAD を読む歯が base で測れる・commit は作らない＝HEAD は base の sha そのもの・overlay は working tree にだけ書き index にも HEAD にも載せない）。引数の不正（`--base` の不在・空）だけは判定行を出さず rc 2（直上の mutants-diff の rc 2「測れなかった」とは別の意味）。stderr の行は判定ではなく、判定行を読む人のための診断である。
- **測れなかった便は「測れなかった」と言う**（`s2-07l.14`・reason 語彙を 3 つ足した）。いずれも fail-closed のままで、`skip` で rc 0 にする経路は持たない。
  - `not-flippable`: base に無い `.rs`（新規 module）の in-file 歯は、base 側に `mod` 宣言ごと存在せず compile されないので**構造的に測れない**。flip が 1 本も無く、そういう file が 1 本以上在る周は runner を撃たず `FAIL reason=not-flippable files=<rel,…>` rc 1（stderr に逃がし方 1 行）。`green-on-base`（TDD の不履行）と同じ札を貼らない。
  - `tests-removed-only`: overlay できる file で **HEAD の test 区間の行列が base の行列の部分列**（順序を保った行の削除だけで得られる）なら flip に数えず、stderr へ `not-flipped reason=tests-removed-only <rel>`。純粋な module 分割（歯の移動）が恒久 FAIL しないための門である。**`crates/*/src/**/*_tests.rs`（と tests という名の file）は名前で test file と見なし全体を写す**——`#[path]` で src 配下へ外出しした test module は `#[cfg(test)] mod` の形を持たず、名前で見なければ区間判定には src 区間だけの file に見え、そこへ足した歯が 1 本も測られない。**`#[test]` fn 名では数えない**——名前の集合で見ると本文の改変が免除される（`⊆` は「名前が同じで本文だけ変えた歯」を、真部分集合でも「1 本消して別の 1 本の本文を変えた file」を通す）。部分列なら 1 行でも足された / 書き換えられた時点で成立しない。
  - **宣言 file の同梱**（`s2-07l.41`）: flip した file のうち、test 区間の差分行が**すべて** `mod x;` 形（`pub` / `pub(crate)` 可）の file は「宣言 file」と呼び、**単独では撃たず**本体 file を撃つ木へ同梱する（判定行に `decl=N`・stderr に `decl-with-body <rel>`）。新規 module は宣言と本体が別 file に割れ、単独 overlay ではどちらの判定も意味を持たない（宣言だけ = 本体不在の `E0583` の偽 RED／本体だけ = base に宣言が無く compile 対象外の偽 GREEN・実測 2026-09-10 `s2-07l.38.2` が初発）。弁別は**差分行の字面だけ**で行い parser は足さない＝`mod` 以外の行が 1 行でも動いていれば宣言 file ではない（同梱は判定を緩める側なので狭く取る）。**宣言 file しか flip していない便は従来どおり単独で撃つ**（存在しない module を指す `E0583` は本当の RED である）。同梱は**本体 1 本を撃つ turn ごと**に、**その便が足した** `mod <name>;` 行のうちその時点の tree に本体が無いもの（同じ dir の `<name>.rs` か `<name>/mod.rs` で見る）を落として置く。`mod` 行**以外は 1 行も触らず**、**base に既に在った宣言行も落とさない**——base が緑である以上その本体は必ず在り、落とすと `#[path = "…"]` の属性行だけが孤児になって`expected item after attributes` の compile error＝**別の捏造 RED**を作る（lens-44 H1）。`#[path]` 付き module を**救う**わけではない（その便が足した `#[path]` 宣言は従来どおり測れない・M4）——便の宣言行を全部置くと、その turn ではまだ置かれていない兄弟 module の `E0583` が RED に化け、**本体がどちらも base で緑でも隠れる**（新規 module 2 本以上の便の fail-open・実測 2026-09-10・`s2-07l.44`）。絞ったうえで本体の歯が base で緑なら `green-on-base` のまま落ちる＝同梱は RED を捏造しない。
  - `moved`（`s2-07l.86`）: **歯を 1 本も足さず挙動も変えない純粋な移動**の便は、test 区間へ `// flip-check: moved <bead-id>` を 1 行置くと RED を要求されず、判定行に `moved=N` が載る。**`tests-removed-only` との弁別は「自動か明示か」**——あちらは test 区間の差が**削除だけ**（部分列）のとき機械が自動で通す門で、こちらは差が削除にならない便（歯が `check()` 越しの統合形で書かれていて、実装だけを module へ出した周）を、**書いた人が札 1 行で明示して**通す逃がしである。`retroactive` を転用しない——あの数は「後から足した歯が N 本」と読まれるので、移動の便に貼ると判定行から何を免除したのか読めなくなる（実測 2026-09-11・`s2-07l.84`）。効く条件は `retroactive` と**同じ 4 つ**（test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない）で、判定は同じ実装（`marker_beads`）を通る。
  - `retroactive`: 既に land した挙動へ**後から歯を足す**便は、歯をどこへ置いても base で緑になる（測る対象が base に在る）。test 区間内の行 `// flip-check: retroactive <bead-id>` を置いた file は RED を要求せず、判定行に `retroactive=N` が載る。**src 区間の marker は効かない**（実装の隣に 1 行足すだけで検査を外せる形にしない）。**効くのはその便で足した札だけ**である＝札の bead id が HEAD の test 区間に在り、かつ base の test 区間に無いときに限る（base に無い file は test 区間が丸ごと新しいので HEAD に在れば足りる）。札は file に残るので、在るだけで数えると一度貼った札がその file の test 区間を触る以後のすべての便を免除し、札の bead id と便が対応しなくなる。**同一性は bead id で見る**（字下げや id 前後の空白が 1 個違うだけで持ち越した札が新しい札に化けると、古い id のまま免除が効き続ける）。持ち越した札しか無い file で **test 区間が動いた便**には免除を与えず、stderr に `flip-check: stale-marker <rel>` を 1 行出す（免除を求めていない便＝src だけ触った便には出さない。札は file に残るので、出すとその file の src を触るたびに「削除しろ」と言われ、本当に効かない札を見落とす）。**限界（もう 1 つ）**: 札の bead id が実在の便を指すかは照合しない（bd を見ない）ので、**新規 file へ古い id の札を置く**形は通る——判定行の `retroactive=N` が review の入口である。N ≥ 1 は review の対象で、notes に変異 proof を要する。marker の無い後から足す歯は従来どおり落ちる。**限界**: 行が実際にコメントか文字列の中身かは **parser 無しでは弁別できない**ので、複数行文字列の中に行頭から marker が現れる file は免除される（`crates/*/tests/*.rs` は全体が test 区間なので特に当たりやすい）。塞ぐには parser が要り、それは本器の取らない道である——代わりに `retroactive=N` が判定行に必ず出るので、**事故は見える形で残る**（review が拾う）。
  - **持ち越し**（`s2-07l.362`・契約表の行 e）: 純移動で base から item ごと移る `moved` 以外の札（`retroactive` 等）は、純移動の機械証明（§5.3）が両側で同じ字面の札を対にして残差から外す＝新規の札と読まない。対の無い札だけが `ForeignMarker`。
- **免除経路の閉じ方**（`s2-07l.170`・監査 2026-09-12 塊 14）: (a) docs-only の分類は path の面（rules 行 `flip.docs_only_faces`）で決め、面の外の file を含む便は `.rs` の差分が無くても `no-test-diff` で落ちる（`no-rust-diff` の skip は消す）。(b) 札（`retroactive` / `moved`）の bead id は閉じた形で受け、形に合わない札は `bad-marker`、便が持つ札の本数が rules 行 `flip.marks_per_pr` を超えれば `too-many-marks` で落ちる。(c) push(main) の CI は HEAD が PR の squash（件名末尾の `(#N)`）か `pipe land` の trailer（`run: <run id>`）を持つことを `xtask main-provenance` で測る。(d) 宣言の `common-verify` の各行は先頭語列で閉じた `VerifyKind` に分類され、入口の flip を撃つ行を持たない宣言は intake が `NoEntranceRed` で断る（宣言 file の schema は変えない）。

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

- **歯の file の置き場**（`s2-07l.351`）: `tests/e2e/pipe/` の file は接頭辞（責務）ごとに 1 file——`intake.rs` = `pipe_intake_`、`review.rs` = `pipe_review_`、`contracts.rs` = 契約表の検査の歯（`contracts_check` を使うもの）、`refuse.rs` = 残りの `pipe_refuse_`、`ratelimit.rs` / `stop.rs` = `pipe_ratelimit_` / `pipe_stop_`（`s2-07l.349`）。2 file 以上が使う helper は `pipe.rs` の `pub(super)` に置いて複製せず、外形 snapshot の歯は `pipe.rs` に残す。

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

- 審査の段（[contract-source.md](./contract-source.md) §4）が足した終端 Reviewed の FAIL / INCONCLUSIVE は live を持たない（判定の読み手は `pipe/review.rs` の 1 本）が、retire の入口（`pipe/cli.rs` の段の列挙と弁別）は Gated の FAIL と Failed の一部しか畳めない＝審査の段で終端した便が前の周の worktree を残すと畳めず、run N+1 が別 worktree で立つ（.209 run 1 の実測 2026-09-15）。
- 形: retire が許す段の列挙に Reviewed を足し、弁別は判定の読み手の 3 値で分ける（FAIL / INCONCLUSIVE = 畳む・PASS = 断る〔live・起こす側〕・読めない = 断る〔読めない判定を終端に読み替えない・fail-closed〕）。畳んだ後の段は Reviewed のまま（Failed / Gated と同じ・可逆 move の 1 本は不変・N1.2）。worktree の無い Reviewed 終端の便は畳む物が無い＝既存の断りのまま。
- 却下: 審査の段の中で自動で畳む（終端の後始末は go を挟む retire の 1 口に揃える）／live が false の段を全部畳める側にする（Failed の理由ごとの弁別が消える）。

## 13. xtask の flipcheck.rs の分割（契約表の行 j・純移動）

- 何が起きているか: `crates/xtask/src/flipcheck.rs`（約 1320 行・上限 1500）は R-C4-2 の余地が 177 行しか無く、size M の便（.170 の行 c）を受付が断る（admin の実測 2026-09-16: src の満杯面 6 つのうちの 1 つ）。責務は 10 群あり、git / tar で base を取り出す群（parse_base / git_stdout / changed_rs / show / load_pairs / repo_root / extract_archive / materialize_base / index_base / work_dir・231 行）は他群から独立している（呼び手は run と judge の側だけ）。
- 形（.363 の `pipe/closure.rs` → `pipe/closure/derive.rs` と同型）: flipcheck.rs は残し、同名の新規 dir に子 module flipcheck/git.rs を置いてその群をそのまま移す（名・本文・順序を変えない）。親は mod 宣言と名指しの `pub use` で呼び手（`main.rs` の run・歯の `use super::{…}` 11 個）を無傷に保つ。歯（`flipcheck_tests.rs` と子 5 file）は動かさず、`super::` で読む private item のうち移す 4 つ（parse_base / failed_tests / nextest_args / FailedTest のうち git 群に当たるもの）は pub 化 + 再輸出で解く。親に残る私有 item を子が呼ぶ周は可視性を `pub(super)` に上げる＝可視性の 1 語と mod 宣言・`pub use`・`use` の path は移動の一部（純移動の残差として許す・.363 と同じ）。移動で生じた可視性の制約を説明する doc コメント行（例: 親の private 型を引数に持つ関数を pub に上げられない理由）も移動の一部＝要約の「コメント行の差」に数えてよい（.372 run 1 の Gated INCONCLUSIVE・admin の逐語実測 2026-09-16）。札 `// flip-check: moved <bead>` は親の歯の区間（flipcheck_tests.rs の先頭）と子の歯の区間に対で置く（純移動の機械証明は §5.3）。
- 触らない: FilePair / is_test_file / split_regions（fan-out が大きい）・cargo 実行の群（後続の便で runner.rs へ）・歯の中身。
- 見積: 親 1323 → 約 1100 行・子 約 235 行。

## 14. 引数の reader を 1 本に（契約表の行 f・`s2-07l.306`）

- 何が起きているか: flag の reader が 6 本（`pipe/cli.rs` の flag / need・`fleet/cli.rs`・`seat/cli.rs`・`headless/mod.rs`・`hook/vessel.rs` の flag_value）あり、どれも argv 全体が既知の集合に閉じているかを見ない（名指しの flag を拾うだけ）＝未知の flag と `--help` を黙って無視する fail-open。`pipe land --run <id> --help` が help を出さず land を完走した（uns planner の実測 2026-09-15）。`account/cli.rs` の flags だけが allowed の集合で断る形（[account-lifecycle.md](./account-lifecycle.md) §4）。
- 形: 共通の reader 1 本（新 module cli_args・`lib.rs` に宣言）= `parse(args, allowed) -> Result<Parsed, ArgsError>`。Parsed は名指しの flag の値（value / need）と positional の列。ArgsError は閉じた enum（Help / Unknown / Missing / Duplicate・as_str）。`--help` / `-h` は allowed に無くても Help で返し呼び手は usage を出して rc 0。Unknown は rc 2（usage 1 行）で state / ref を 1 本も動かさない。6 本の reader を parse の呼出に置き換え、各 subcommand の allowed を const 配列（宣言順）で持つ。挙動の差は「未知の flag と --help を断る」だけ（既存の flag の意味・usage の文は不変＝外形 snapshot を動かさない）。
- 触らない: subcommand の本体・rules・docs。依存を足さない。

## 15. pipe の `--repo` / `--state-dir` の cwd fallback を落とす（契約表の行 g・`s2-07l.310`）

- 何が起きているか: `pipe/cli.rs` の repo_of は `--repo` が無いと cwd の repo root へ落ちる（呼び手 = state_dir_of と `pipe/cli/intake.rs` の run_repo の最終枝）。cargo-mutants の一時コピーは worktree の `.git`（file・本物の gitdir を指す）を持つので、コピーの中で cwd から解いた repo に便を起こすと worktree と branch が本物の repo に登録される（prunable 47 件・fixture 名の branch 44 本・admin の実測 2026-09-15・prune は user 承認 event 02:5xZ）。
- 形: repo_of から cwd の枝を落とし、`--repo` が無ければ flag 不在の断り（`--run が要る` と同じ作り・Refuse は契約単位の拒否ゆえ variant を足さない）。run_repo の最終枝（写し面が無い周）と state_dir_of（`--state-dir` も `--repo` も無い周）も同じ断り。usage に `--repo` の要件を 1 句（外形 snapshot が動く）。歯の helper（`tests/e2e/pipe.rs` と `pipe/*.rs`）で `--repo` / `--state-dir` を渡していない呼出には tmp の fixture repo を渡す。
- 触らない: `hook/vessel.rs` の repo_root（cwd から解くのは hook の領分）・契約 file の schema・写し面の読み。
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

## 18. land の stale base を人手なしで追随し直し、resume が Gated(PASS) を受ける（契約表の行 l・`s2-07l.335`）

- 出所: admin 報告（`.329` run 2）: 追随の撃ち直し中に main が動くと `pipe land` が stale base の rc 1 で抜け、`pipe run` はそこで終了する＝段は `Gated`（PASS）のまま次の land を撃つ主体が無い。user 直命: dispatcher の仕組みを最優先にし、admin が手で撃ち直す穴を器で塞ぐ。
- 現物: `pipe/land.rs` の `land` 関数が stale base を refused で返す。`pipe/cli.rs` の `resume` は `Stage::Gated` の周を `verdict_of` の値で分けている。`pipe/follow.rs` は起こし直しの上限を rules 行 `pipe.follow_retries` で持ち、回数は replay から導く（`EXHAUSTED`）。
- 形: `pipe run` の着地の段で land が stale base を返した周は `RunStage`（`stage=Gated detail=stale:<base>..<main>`）を記帳し、同じ追随の経路（rebase → gate の撃ち直し → 順番待ち → land）へ戻る。回数は既存の `pipe.follow_retries` の判定に「起こし直し 1 回」として数え、上限に当たれば typed な `Failed` で終端する（既存の終端の型を使い新しい理由の variant は増やさない）。
- `pipe resume` が `Stage::Gated` かつ `Verdict::Pass` の便を受け、同じ追随の経路へ入れるようにする（`verdict_of` が読む値・land の CAS・stale の判定は不変）。verdict が Pass でない周は従来どおり断る。
- 触らない: `land` の CAS と stale の判定・`follow_retries` の値・`pipe/queue.rs`。
- 却下: stale を state dir に記録するだけで撃ち直しは人に任せる（撃つ主体が席のまま残る）／新しい rules 行を作る（既存の `pipe.follow_retries` で足りる）。

## 19. pipeline 外の merge のための着地列の待ち口（契約表の行 m・`s2-07l.212`）

- 出所: planner の実測: docs PR の squash merge が `Gated`（PASS）の便の追随を 1 周誘発する。いまは「Gated PASS の便が 0 の窓か Landed の直後に merge」を席の運用（散文）で守っている＝規則ではない。
- 現物: 着地の列は `pipe/queue.rs` の `turn_in` / `await_turn` が読む。唯一の wait は `fleet/wait.rs` の `Completion`（`LandTurn` / `SlotFree` / `AccountFree` は pid を見張らない variant）。
- 形: `Completion` に pid を見張らない variant を 1 つ足す（窓の待ち）。判定条件は、着地の列に PASS の便が 0 本かつ `Landed` の記帳から追随中の便が無いこと。既存の deadline・唯一の wait の経路をそのまま通す。
- 新しい subcommand の口（`pipe land-window`）を足す。窓が開いていれば rc 0 で `clear` を、待ちが切れれば rc 1 で列の便を名指した `busy` を返す。席の docs merge はこの口を前置して撃ち、散文の窓判断を消す。
- 触らない: `gh pr merge` 自体（器は merge を撃たない）・列の順序・`LandTurn`。
- 却下: docs-only PR も器が `gh` を撃って merge する（外部 binary を撃つ面が増える）／運用のまま据え置く（規則が散文のまま）。

## 20. runner の雛形に終端の規律を足し片付けで殺した子の数を記録する（契約表の行 n・`s2-07l.275`）

- 出所: admin の観測（`.270` run 1）: runner が「flip-check を背景で回している・完了通知を待つ」と言って turn を閉じ（rc 0）、背景の task が scope の片付けで止められた。
- 現物: `crates/scribe2/src/headless/runner.txt` に「背景実行で turn を閉じない」の規律は無い。`headless/mod.rs` の片付けは `pipe/confine.rs` の `release_scope` が行い、片付けで殺した子の数は record に残らない。
- 形: 雛形 `runner.txt` に「検証は前面で完走させてから turn を閉じる（背景実行を残して終えない・残した task は片付けで止められ done に数えない）」の 1 行を足し、雛形の外形を snapshot で pin する。
- `release_scope` が scope を止める直前に scope に残った process の数を読み、record に `orphans=<n|->` として残す（0 も書く・読めなければ `-`）。
- 触らない: 片付けの極性（止める）・runner の権限。
- 却下: 背景 task を待ってから片付ける（turn の終端の規律が曖昧になる）／雛形だけ直す（殺した事実が記録に残らない）。

## 21. gate の段の通知行を rc に依らず record と stderr に残す（契約表の行 o・`s2-07l.293`）

- 出所: `.286` の gate が 2 周とも lens への入力が diff だったのに理由語が残らなかった（run.stderr 0 bytes）。同じ形は precheck の注意行・追随の rebase の行にも当たる。
- 現物: gate は理由を `notice()`（`pipe/move_proof.rs`）で出すが、`pipe/cli/run.rs` の `chain` 関数は rc 0 の段の err を捨てて out だけを繋ぐ。run dir にも書かれない（`lens-input.txt` は要約の周だけ残る）。
- 形: `chain` が rc 0 の段の err を保持し、`pipe run` の stderr に段の順で出す（stdout の判定行は不変）。
- gate の record（`pipe/gate/record.rs`）の step 行と同じ log に `lens-input=<kind> reason=<語>` を追記する（要約の周も diff の周も・run dir に残る）。
- 触らない: 判定の極性・`notice()` の語彙・lens の入力の選び方。
- 却下: stderr にだけ出す（run dir に残らず事後に読めない）／record にだけ書く（席が run の場で読めない）。

## 22. 撃ち直しの間も着地の番を先頭に保つ（契約表の行 p・`s2-07l.305`）

- 出所: admin の現物確認: `.294` が `.279` を追い抜き、`.279` が撃ち直し 1 周分を余計に払った。
- 現物: `pipe/queue.rs` の `turn_in` は「最初の `Gated` の ts が自分より小さい PASS の便」だけを前に数える。`pipe/land.rs` は `await_turn` を追随の前に 1 回だけ撃ち、撃ち直しの後は番を読み直さない。鍵の早い便が Inconclusive で一度列を離れて戻ると先頭が 2 つになる。
- 形: `await_turn` が「自分の番」と判定した周に `RunStage`（`stage=Gated detail=turn:taken`）を 1 行追記する（既存の段の event・detail で弁別・新しい kind は足さない）。
- `turn_in` は「鍵が自分より小さい PASS の便」に加えて「`turn:taken` を記帳済みで終端でない便」も前に数える（自分自身は除く・`Queued` に導出の field を 1 つ足す）。番を取った便が `Landed` / `Stopped` / `Failed` で終端すれば外れる（既存の条件）。
- 触らない: 鍵（最初の `Gated` の ts）の定義・stale base の判定・`await_turn` の待ち（唯一の wait）。
- 却下: 撃ち直しの後に番を読み直す（払う側が入れ替わるだけで 1 周の損失は消えない）／受容する（dispatcher で便が増えると追い抜きの頻度が上がる）。

## 23. stop 起因の終端を oom-kill に誤分類しない（契約表の行 q・`s2-07l.340`）

- 出所: admin 実測: `.336` run 2 を `pipe stop` した同じ秒に `RunStage`（`stage=Failed detail=oom-kill`）が記録され、その後 `RunStopped` で最終的に `Stopped` になった（直後の available memory は圧迫なし）。
- 現物: `pipe/spawn.rs` の `OOM_DETAIL`（"oom-kill"）・`pipe/confine.rs` の `Reason::OomKill`（終端行の `oom_kill` ≥ 1 で判定・`pipe/gate/lens.rs`）・`pipe/stop.rs` は席を止め切ってから `RunStopped` を書く。stop の終端検出が「runner が消えた」を oom-kill に倒す経路が疑われる（kernel の証拠は権限で未確認）。
- 形: `pipe stop` は signal を送る前に、その run の「停止中」の印を書く。runner の消滅を見た経路（`pipe/spawn.rs` の終端検出）は、その run が停止中なら `Failed detail=oom-kill` を書かず `RunStopped` の経路に任せる。
- oom-kill は `Reason::OomKill` の既存の判定条件（終端行の `oom_kill` ≥ 1）が在る周だけに限る。終端行が無い / 読めない周は `Reason` に variant を 1 つ足し（unknown・`as_str`）、`Failed detail=unknown` に倒す（0 と「測れない」を融合しない）。
- 触らない: stop の極性（止め切れなければ `RunStopped` を書かない）・oom の閾値。
- 却下: dmesg / journalctl を読む（権限と host 依存）／stop 後の `Failed` を後から書き換える（append-only の log を汚す）。

## 24. pipe retire が受ける終端の段を広げる（契約表の行 r・`s2-07l.132`）

- 出所: `s2-07l.127` phase 1 / 2 の実測: gate FAIL で終端した run の worktree が live のまま残り、`pipe retire` は限られた段しか受けないので操作役が畳めない。dispatcher で便が増えると FAIL 終端の worktree が積む。
- 現物: `pipe/retire.rs` の `retire` 関数は「在るか・clean か」だけを検査し段を動かさず `retired/` へ move する。受ける段の弁別は呼び手（`pipe/cli.rs` の `discriminate`）が持ち、`Extra::Retire` は現状 `Stage::Gated`（verdict が `Verdict::Fail`）と `Stage::Failed`（detail が `REBASE_EMPTY` か `follow::EXHAUSTED`）だけを受ける。
- 形: `discriminate` が `Extra::Retire` に対して受ける便の条件を「終端の段（`Stage` の終端＝`Landed` / `Stopped` / `Failed`〔detail を問わない〕）∧ `Gated` で最新 verdict が `Fail`」に広げる。clean の検査と `retired/` への move は不変。段は動かさない（`RunStage detail=retired` の記帳も不変）。
- 触らない: 非終端（`Spawned` / `Implemented` / `Gated`(PASS)）の便は断る（退行の pin）・`stop` の極性。
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

- 何が起きているか: admin の実測 2026-09-16 01:4xZ（本番の state dir の `pipe/` に e2e の fixture 便 `s2-2e5-…`〔`contract_body()` の goal・owner・`write-set = ["src/lib.rs"]`・`repo` file は tmp の toy repo・review.json は `evidence:"fake"`〕が 1 件〔全 320 件中〕・`fleet/events.jsonl` に RunCreated / RunStage Reviewed の 2 件〔2320 件中〕）。現物（verified・main d6e4e6f）: `pipe/cli.rs` の `state_dir_of` は `--state-dir` が無いと `repo_of` へ落ち、`repo_of` は `--repo` が無いと **cwd** から repo root を解いて `hook/vessel.rs` の `state_dir`（`git -C <root> config --get <NAME>.stateDir`）を読む。e2e の helper（`tests/e2e/pipe.rs` の `run_pipe` / `intake_raw` ほか）は全部の呼出しで `--state-dir` を渡している（母集団 = `run_pipe(&[` 136 箇所・grep）が、binary を **cwd を継いだまま**（nextest の子 process の cwd = crate dir・便の worktree の中）起こす。便の worktree は anchor の `.git/config` を共有し、この host の anchor は `<NAME>.stateDir` に本番を持つ。経路（inferred・時刻 01:39Z は `.349` / `.379` が Implemented → gate に入った直後）: gate の変異検査は**変異 binary で歯を回す**ので、`flag` / `state_dir_of` / `repo_of` を壊す変異の下では `--state-dir` / `--repo` が読めず cwd の fallback が本番へ届く。CI は config を持たないので露出せず、この host でだけ非 hermetic。
- 形: e2e の helper が binary を起こす口を **1 関数**（`tests/e2e/pipe.rs` に新設・`Command::new(bin())` に `current_dir(<git repo でない temp dir>)` を付けて返す・関数の名は行 v の契約が持つ＝base に無い名を本節は名指さない）に集め、`run_pipe` / `run_pipe_with_path`（`pipe.rs`）/ `run_pipe_in_pane`（`pipe/spawn.rs`）と `pipe` を直接起こす helper（`intake_raw` ほか）は全部それを通す。cwd が repo でなければ、どの変異の下でも cwd の fallback は「repo の root を解決できない」で**断る**（fail-closed）＝本番へは届かない。器の側（`state_dir_of` / `repo_of` の fallback・`vessel::state_dir`）は触らない（読みの口 `show` / `report` を anchor の cwd で撃つ admin の常道を残す）。
- 触らない: `state_dir_of` / `repo_of` / `vessel::state_dir` の解決順・`vessel init` の呼出し（`--state-dir` と root を明示済み）・`fleet` / `seat` / `hook` の e2e の helper（本便の射程外・同じ型は別便で数える）・汚れた 1 件の処分（消さず `retired/` へ移す = N1.2・admin の運用）。
- 却下案: 書く口（intake / run）に `--repo` を必須にして cwd の fallback を消す（変異の下では必須の検査も壊れる＝歯の側で cwd を固定しないと閉じない・admin の launcher の引数も変わる）／CI に `<NAME>.stateDir` の config を足して再現する（露出の面を増やすだけ）／本番の置き場を手で掃除する（不可逆・N1）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "pipe/cli.rs を cli/args.rs / state.rs / show.rs / resume.rs に、e2e/pipe/lifecycle.rs を ratelimit.rs / stop.rs に割る（純移動）"
req = ["FR30"]
section = "5"
write-set = ["crates/scribe2/src/pipe/cli.rs", "+crates/scribe2/src/pipe/cli/args.rs", "+crates/scribe2/src/pipe/cli/state.rs", "+crates/scribe2/src/pipe/cli/show.rs", "+crates/scribe2/src/pipe/cli/resume.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/lifecycle.rs", "+crates/scribe2/tests/e2e/pipe/ratelimit.rs", "+crates/scribe2/tests/e2e/pipe/stop.rs", "docs/design/pipeline.md", "docs/design/dispatcher.md", "docs/design/working-memory.md", "docs/design/contract-source.md", "docs/design/account-autonomy.md", "docs/design/consumer-sync.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_"]
size = "S"
done = "pipe/cli.rs が入口と shim だけになり、lifecycle.rs が ratelimit.rs / stop.rs に割れて、歯の本数と外形 snapshot が不変"

[[contract]]
id = "b"
title = "tests/e2e/pipe/intake.rs を接頭辞ごとに review.rs / contracts.rs / refuse.rs へ割る（純移動）"
req = ["FR30"]
section = "8"
write-set = ["crates/scribe2/tests/e2e/pipe/intake.rs", "+crates/scribe2/tests/e2e/pipe/review.rs", "+crates/scribe2/tests/e2e/pipe/contracts.rs", "+crates/scribe2/tests/e2e/pipe/refuse.rs", "crates/scribe2/tests/e2e/pipe.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_"]
size = "S"
done = "intake.rs が pipe_intake_ だけになり、review / contracts / refuse の 3 file に歯が移って本数が不変"
depends = ["a"]

[[contract]]
id = "c"
title = "入口の flip check の免除経路を閉じる — docs-only の面・札の形と上限・push(main) の出所・宣言の VerifyKind"
req = ["FR7", "FR17", "FR50"]
section = "7"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/xtask/src/flipcheck.rs", "crates/xtask/src/main.rs", "crates/xtask/src/limits.rs", "+crates/xtask/src/provenance.rs", "crates/xtask/src/flipcheck_tests.rs", "crates/xtask/src/flipcheck_declaration_tests.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap", ".github/workflows/ci.yml", "docs/design/pipeline.md", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail flip_docs_only_ flip_marks_ provenance_", "cargo nextest run -p scribe2 --no-tests=fail declaration_kind_"]
size = "M"
done = "rules 行だけの便が no-test-diff で落ち、札は形と上限で止まり、push(main) の CI が出所を測り、入口の flip を撃たない宣言が intake で断られる"

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
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/tests/e2e/pipe/land.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_retire_"]
size = "S"
done = "Reviewed の FAIL / INCONCLUSIVE で終端した便が pipe retire で畳め（段は Reviewed のまま・可逆 move）、PASS は断られ、読めない判定は終端に読み替えない"

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
write-set = ["+crates/scribe2/src/cli_args.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/account/cli.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/seat/account.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail cli_args_", "cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_land_refuses_unknown_flag"]
size = "M"
done = "偽 remote の toy repo で pipe land に未知の flag か --help を渡すと main の ref・event log・worktree が 1 つも動かず、fleet / seat / headless / vessel の 1 口ずつが未知の flag を rc 2 で断り、usage の外形 snapshot は不変"

[[contract]]
id = "g"
title = "pipe の --repo と --state-dir の cwd fallback を落とす — 写し面を消した run に --repo 無しで spawn しても cwd の repo に落ちない"
req = ["FR4", "FR39"]
section = "15"
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_repo_required_"]
size = "S"
done = "--repo 無しの spawn と --state-dir 無しの置き場解決が断りの 1 行で止まり worktree を作らず、通常の nextest と変異の周で本物の repo の worktree と branch が増えない"

[[contract]]
id = "h"
title = "runner / lens の effort を rules 行 runner.effort から毎回渡す — build が --model と同じ場所で --effort を渡し、行が無い・表に無い値は rc 2"
req = ["FR5", "FR9"]
section = "16"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_runner_passes_effort_ headless_lens_passes_effort_ headless_runner_refuses_"]
size = "S"
done = "偽 claude で runner と lens を撃つと argv に effort の値が rules 行のとおり載り、行の無い manifest と表に無い値は rc 2 で claude の呼出 0"

[[contract]]
id = "k"
title = "lens の verdict に findings の閉じた category と件数・母集団を必須 key にする — 欠落と母集団 0 は INCONCLUSIVE"
req = ["FR9", "NFR1"]
section = "17"
write-set = ["crates/scribe2/src/pipe/gate.rs", "+crates/scribe2/src/pipe/gate/findings.rs", "crates/scribe2/src/headless/lens.txt", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_findings_"]
size = "M"
done = "lens の verdict が category ごとの件数と母集団を必ず持ち、欠落は INCONCLUSIVE"

[[contract]]
id = "l"
title = "land の stale base を同じ経路で人手なしで追随し直し、resume が Gated(PASS) の便を受ける"
req = ["FR30", "FR50"]
section = "18"
write-set = ["crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/lifecycle.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_stale_"]
size = "S"
done = "toy repo で stale base の便が人手なしで追随して Landed し、上限は typed な Failed"

[[contract]]
id = "m"
title = "着地列の窓の待ちを Completion に足し、pipeline 外の docs merge が pipe land-window を前置して撃つ"
req = ["FR30", "FR50"]
section = "19"
write-set = ["crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_window_"]
size = "S"
done = "偽の列で窓の開閉が rc と 1 行で判り、席の docs merge が散文の窓判断を持たない"

[[contract]]
id = "n"
title = "runner の雛形に turn 終端の規律を足し、片付けで殺した子の数を record に残す"
req = ["FR5", "FR22"]
section = "20"
write-set = ["crates/scribe2/src/headless/runner.txt", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_release_orphans_ headless_runner_prompt_"]
size = "S"
done = "runner の雛形が終端の規律を持ち、片付けで殺した子の数が record に残る"

[[contract]]
id = "o"
title = "gate の段の通知行を rc に依らず record と run の stderr に残す"
req = ["FR8", "FR22"]
section = "21"
write-set = ["crates/scribe2/src/pipe/cli/run.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/lifecycle.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_notice_"]
size = "S"
done = "成功した便でも要約にならなかった理由が record と stderr に残る"

[[contract]]
id = "p"
title = "着地の番を取った事実を記帳し、撃ち直しの間も番を鍵の順と独立に列の先頭に残す"
req = ["FR50", "FR30"]
section = "22"
write-set = ["crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_land_turn_"]
size = "S"
done = "偽の列で番を取った便が鍵の順と独立に先頭に残る"

[[contract]]
id = "q"
title = "pipe stop 起因の終端を oom-kill に誤分類せず、kernel の証拠が無い kill は unknown に倒す"
req = ["FR22", "FR46"]
section = "23"
write-set = ["crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/lifecycle.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_spawn_terminal_reason_"]
size = "S"
done = "stop した便が oom-kill に分類されず、証拠の無い kill は unknown"

[[contract]]
id = "r"
title = "pipe retire が受ける終端の段を Stage の終端全部（detail 不問）と Gated(FAIL) に広げる"
req = ["FR34"]
section = "24"
write-set = ["crates/scribe2/src/pipe/retire.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_retire_failed_"]
size = "S"
done = "FAIL 終端の便の worktree を器の口で可逆に畳める"

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
write-set = ["crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_hermetic_"]
size = "S"
done = "--state-dir も --repo も無い pipe の呼出しが helper 経由では「repo の root を解決できない」で rc 1 に断られ、既存の e2e は全部緑のまま"
<!-- contracts:end -->
