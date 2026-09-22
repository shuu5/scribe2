# 設計: gate の費用構造 — 変異の並列度は project 横断（host 単位）の受付の実測で決め、器の子 process は cgroup で封じ、木が同じ main 実測は検出線を撃ち直さず、着地は gate 済みの便を先に通す

- 要件: [FR8](../../design-intent/spec/srs.html#FR8) [FR9](../../design-intent/spec/srs.html#FR9) gate の機械検証と lens / [FR10](../../design-intent/spec/srs.html#FR10) [FR11](../../design-intent/spec/srs.html#FR11) land の前提と squash / [FR34](../../design-intent/spec/srs.html#FR34) 追随 / [NFR1](../../design-intent/spec/srs.html#NFR1) lens 予算 / [NFR4](../../design-intent/spec/srs.html#NFR4) 読めない store は rc 2。host の資源を枯渇させない要件は SRS に無く、v0.7 の材料（planner state dir・NFR 候補）として user の手番に出す。
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) / [C5](../../design-intent/spec/constitution.html#c5) 値は rules 行・裁定 id 付き / [C3.4](../../design-intent/spec/constitution.html#c3) 待ちは完了 enum の 1 実装 / [C2.2](../../design-intent/spec/constitution.html#c2) env を読まない・置き場は NAME から導く / [C6](../../design-intent/spec/constitution.html#c6) 起動口は 1 つ・Budget は Precheck から / [C10](../../design-intent/spec/constitution.html#c10) 宣言値・測定値・実効値を型で分ける / [C11.2](../../design-intent/spec/constitution.html#c11) 境界は極性を持つ / [C12.4](../../design-intent/spec/constitution.html#c12) 変異の生存は検出線 / [C12.6](../../design-intent/spec/constitution.html#c12) main は常に緑 / [N1](../../design-intent/spec/constitution.html#n1) 不可逆に消さない。
- 決定: [ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html)（本 doc の決定の正本・ADR-0009 §2.3 / §2.4 と ADR-0010 §2.1 / §2.3 / §2.4 / §2.5 を §2.6 で部分 supersede）/ [ADR-0050](../../design-intent/decisions/ADR-0050-cores-and-scope-creation-are-hard-resources.html)（硬い資源の面・ADR-0021 §2.1 / §2.2 / §2.3 / §2.7 を部分 supersede・§2 / §31 / §32 の正本）/ [ADR-0009](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html) §2.3 / §2.4（変異検査と共通 verify の順序）/ [ADR-0010](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html)（宣言 file）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)（極性一覧）/ [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)（追随）。
- 土台: [pipeline.md](./pipeline.md) §5.3（gate）/ §5.4（land・main 実測）/ §6（runner と lens の起動形）・[pipeline-conflict.md](./pipeline-conflict.md)（追随と衝突）・[rules-manifest.md](./rules-manifest.md)（rules 行と検出線）・[polarity.md](./polarity.md)（極性一覧）。
- この設計から出る契約: §9（4 便）。値の裁定（rules 行）は user 2026-09-12（台帳 s2-07l.153 notes・逐語）。

## 1. 何を解くか

並列 3 便の実測（台帳 s2-07l.143・2026-09-12）で、後着の便 1 本が 7 時間 13 分かかった。内訳は runner 18 分・gate 82 分 × 4 周（初回 + 追随 3 回）・land の main 実測 78 分で、便の規模に帰せるのは gate 1 周だけである。残りは 3 つの構造の費用に出ている。

1. **gate の 1 変異あたりの費用が高い**: 変異検査（cargo-mutants）は変異 1 本ごとに増分 build + test suite 1 周を払う。suite の壁時計は 30 秒の実待ちを持つ歯が床を作り（台帳 s2-07l.151・本 doc の外）、並列度は 1（xtask が jobs を渡していない）。
2. **main 実測が gate と同じ木に同じ 8 本を撃ち直す**: squash 後の木は gate PASS の木と同一（lossless の実測が既に在る）なのに、変異検査を含む全行を撃つ。
3. **並列の便が着地のたびに stale を起こす**: gate PASS の便が他便の Landed で stale になり、追随 + gate 全部やり直しを払う（pipeline-conflict.md §5・台帳 s2-07l.147）。

並列度を上げるだけなら xtask に `--jobs` を足せば済むが、それは host の memory を溢れさせて席や他 project の便を殺す形になる。user の裁定（2026-09-12・台帳 s2-07l.153 逐語）は「防御を作った上で並列を上げる・複数 project / 複数 pipeline の同時走行に対応・余らせず使い切る・kill の事故は起こさない」。本 doc はその形を決める。

やさしく言うと: 変異検査を 4 本同時に走らせて速くする。ただし「いま host に空いている memory」を測ってから走らせ、他の便や他の project の分と合計で溢れないように受付で枠を取る。それでも溢れたら、殺されるのはその検査の process だけで、席は生き残る。着地後の実測は、木が同じなら変異検査をもう一度撃たない。着地は審査済みの便を先に通す。

## 2. 費用の原則（設計の向き）

- **memory だけが硬い資源**。CPU は溢れても遅くなるだけなので上限を持たず、重み（席 > 便）だけを付ける。memory は溢れると kernel が process を殺すので、合計を受付で守り、個々を封じ込めで守る。
- **硬い資源は 3 つ**（[ADR-0050](../../design-intent/decisions/ADR-0050-cores-and-scope-creation-are-hard-resources.html) が上の 1 行を置き換えた・2026-09-20 の事故 `s2-07l.504`）: memory に加えて CPU の core と、器が host に頼む scope の作成も、枯渇すれば host を止める。core は受付の枠（§31）と器の健康の遮断器（§32）が守り、scope の作成の rate は歯の道具箱（§30）が下げる。封じ込めの箱に CPU の上限を付けない形と重みだけを付ける形は不変である。
- **宣言 → 測定 → 実効**（C10）: 並列度の上限（rules 行）は宣言値、host の空き memory と core 数は測定値、実際に渡す jobs は両者から導く実効値。宣言値をそのまま渡さない。
- **止めない、縮退する**: 資源が足りない周は便を断らず、並列度を 1 まで下げて進む（1 は常に許される＝従来と同じ費用）。封じ込めが使えない host でも便は流れる（並列度 1）。
- **同じ木を 2 度測らない**: 木の hash が一致する周の検出線（C12.4）は撃ち直さない。deny する行（nextest / clippy / check / deny）は撃ち直す（gate は run の worktree〔untracked を含む〕で撃ち、main 実測は tracked だけの木で撃つ＝環境が違う）。

## 3. 並列度と受付（ADR-0021 §2.1 / §2.3）

### 3.1 rules 行（値は裁定 id 付き・C1 / C5・本 doc は値を写さない）

| id | kind | 意味 |
|---|---|---|
| `gate.mutants_jobs` | `GateMutantsJobs`（Int） | 変異検査の並列度の**上限**（宣言値）。実効値は §3.3。 |
| `gate.job_memory_mb` | `GateJobMemoryMb`（Int） | job 1 つが要る memory の宣言値。受付の分母と封じ込めの上限に使う。測定値（§4.3 の peak）が溜まったら裁定で置き換える。 |
| `host.reserve_memory_mb` | `HostReserveMemoryMb`（Int） | 席と host のために常に残す memory。受付はこれを差し引いた空きしか配らない。 |
| `gate.slot_wait_s` | `GateSlotWaitS`（Int） | 受付で枠が空くのを待つ上限。超えたら並列度 1 で進む（縮退・止めない）。 |
| `gate.cpu_weight` | `GateCpuWeight`（Int） | 便の scope に付ける CPU の重み（席は既定の重み）。 |
| `gate.tmux_test_threads` | `GateTmuxTestThreads`（Int） | tmux を立てる歯（e2e の isolated seat）の同時本数。値の写しは nextest の test-group `tmux` の `max-threads`（`.config/nextest.toml`・新規）で、`cargo xtask check` が写しの一致と配線（tmux を立てる歯＝本文が席の fixture の道具を名指すか、それを名指す e2e の木の関数を呼ぶ `#[test]`・閉包は器が関数名の固定点で決め、filter はその歯を module 付きの名で全部列挙する固定形＝file 単位や接頭辞では決めない〔.360 run 2 の QUESTION・helper 越しの歯 21 本を接頭辞が拾えない〕）を測る（clippy.toml ↔ R-C4-4.* と同型・C10.3）。並列 gate 下の負荷で tmux の歯が落ちる flake（`s2-07l.360`・契約表の行 b）の解＝並列度そのものは下げない。 |
| `pipe.max_live` | `PipeMaxLive`（Int） | host で同時に走る便（live な便）の本数の**最大値**（[ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)・値は user 裁定 id 付き）。受付が便を作る前に live な便を数え、値以上の周は typed に断る（§24）。変異検査の並列度（`gate.mutants_jobs`）や memory の枠（§3.2）とは別の軸で、走行中の便には効かない。本表の行は在り、manifest の行と `RuleKind` の variant は行 o（`s2-07l.398`）が足した（受付の断りは `max-live`・§24）。 |
| `host.runnable_per_core` | `HostRunnablePerCore`（Int） | 器の健康の遮断器（§32）が「混んでいる」と判じる走行可能な process 数の**倍率**。閾値 = この値 × 実測の core 数で、超えた周は verify の行を撃たずに空くまで待つ。値は裁定 id 付き。 |
| `host.blocked_per_core` | `HostBlockedPerCore`（Int） | 同じ遮断器（§32）が読む**待ち**（D 状態）の process 数の倍率。閾値の組み立ては走行可能と同じで、どちらか一方が超えれば「混んでいる」。値は裁定 id 付き。 |

manifest に行が載るまでは ADR-0021 の予定行（C14.2 の相互参照は行が在って成立・ADR-0018 §4 と同じ）。

### 3.2 受付（project 横断・host 単位の枠）

- **置き場**: `<state_dir の親>/<NAME>-host/slots/`。state dir は project ごとに違う（`<NAME>-v2-state` / `<NAME>-v2-state-<project>`）が、同じ host の state dir は 1 つの親（host の state root）に置く運用なので、その親から導けば project をまたいで 1 つになる。env（`XDG_RUNTIME_DIR` / `HOME` / `TMPDIR`）は読まない（C2.2・land.rs の tmp dir と同じ理由）。親が違う state dir を使う project は別の受付になる＝その運用は本 doc の外（§8）。
- **受付札**（語彙の `lease` は fleet の割当で別の実体・使わない）: 枠 1 組 = file 1 つ `slots/<pid>-<run>.slot`（内容 = `schema` / `pid` / `run` / `jobs` / `ts`・state dir と同じ TOML subset）。生きている札 = `/proc/<pid>` が在り、その process の起動時刻（`/proc/<pid>/stat` の starttime）が札の `ts` 以前のもの（pid の再利用を弁別）。死んだ札と**読めない札**（schema / 数値の壊れ）は受付が次に走ったとき回収し、record に `slot=reclaimed:<n>` を残す（黙って落とさない・止めない・NFR4）。削除でよい: 札は器が管理する「物」ではなく受付の一時的な印（N1 の対象外・ADR-0021 §5 (D)）。
- **容量の測定**（受付のたびに測る・C10）:
  - `avail = MemAvailable(/proc/meminfo) − host.reserve_memory_mb`
  - `by_avail = floor(avail / gate.job_memory_mb)`（いま実際に空いている分。他 project や host の他 process が使った分は自然に減る）
  - `by_token = floor((MemTotal − host.reserve_memory_mb) / gate.job_memory_mb) − Σ 生きている札の jobs`（受け付けたがまだ常駐していない分を数える＝2 つの gate が同時に測って両方が満額を取る競合を塞ぐ）
  - `free = min(by_avail, by_token)`
- **取得**: slot dir 直下の lock file 1 つ（実装は fleet の `acquire` を公開して使う＝file は別・実装は 1 本。event log の lock file は state dir ごとで project をまたげない・retry / stale は rules 行 `fleet.lock_retry_ms` / `fleet.lock_stale_ms`・flock ではない・第 2 の lock 実装を持たない〔C6.3〕）の内側で測り、`jobs = min(gate.mutants_jobs, free)` の札を書く。`free == 0` なら lock を離して待つ。**待ちは完了 enum の variant 1 つ**（`Completion::SlotFree { slots_dir, want }`・現物の variant は pid を運ぶが本 variant は置き場と要る枠を運ぶ）を足して唯一の wait 実装を通す（C3.4・第 2 の poll loop を書かない・周期 = wait 実装の定数のまま〔`fleet.lock_retry_ms` は lock 取得の上限で周期ではない〕・上限 = `gate.slot_wait_s`）。`gate.slot_wait_s` を超えたら `jobs = 1` で進み、record に `slot=degraded` を記す。**0 で走らせない・断らない**。lock の失敗は受付側の閉じた型に包み直す（fleet の `StoreError` が持つ fail-closed の極性を受付が fail-open に読まない・ADR-0014 §2.1・境界型は 1 極性）。variant が運ぶ物（置き場・要る枠）と wait の引数（`free` の再計算に要る rules 3 値と meminfo の読み手）の分担は契約 (b) で決める。
- **解放**: verify 行の終了で札を消す（Drop でも消す）。器が死んだ周は次の受付が pid で回収する。
- **極性**: 受付は行為を止めうる判定を持たない（縮退する）ので ADR-0014 §2.1 の guard ではなく、極性一覧に載せない。測れない周（`/proc/meminfo` が読めない・lock が取れない）は `jobs = 1` で進み `slot=unmeasured` を記す（縮退＝従来の費用）。slot dir は真実を持たない印の置き場で NFR4 の「store」ではない（読めない札は回収して記録・rc は変えない）。
- **tmux を立てる歯の同時本数**（`s2-07l.360`・契約表の行 b）: 受付は memory の枠だけを数え、tmux を立てる歯（e2e の isolated seat）の同時本数は数えない。その本数は nextest の test-group `tmux` で rules 行 `gate.tmux_test_threads`（§3.1）の値に絞り、値の写し（`.config/nextest.toml` の `max-threads`）と配線（group の filter が列挙する module 付きの名 ↔ e2e の木を関数名の固定点で閉じた `#[test]` の集合・両向き）は `cargo xtask check` の fact `nextest-tmux-group=<ok|drift> tests=<m> files=<n>` が manifest と突合する（`crates/xtask/src/check_facts.rs`・clippy.toml ↔ R-C4-4.* と同型・C10.3）。

#### 3.2.1 errata（現物との差・s2-07l.158・規範は上の §3.2 のまま）

- **module は `pipe/admission.rs`**（§7 の旧名は slots.rs・その file は無い）。code の識別子は admission / Ticket 系で、hook の注入計測の slot（FR21）と intake の「受付」との字面衝突を避ける。file 名の `.slot` と record の `slot=` は ADR-0021 §2.3 の字面のまま。置き場は seat/mod.rs `host_slots_dir`（`StateDir::slots_dir` はその委譲）。
- **札の中身は 1 行 JSON**（`schema` / `pid` / `run` / `jobs` / `ts`・§3.2 は「state dir と同じ TOML subset」と書いた）。ADR-0004 §2.3 D-3 の TOML subset の列挙を広げないためである。`ts` は UNIX epoch の ms で、生きている判定の起動時刻は `/proc/stat` の `btime` + `/proc/<pid>/stat` の starttime ÷ `USER_HZ`（ABI の 100）で組む（`btime` の秒の切り捨ては持ち主を死んだと読まない側へ寄る）。札は `.partial` に書いて rename する＝読み手は半端な札を見ない。
- **`Completion::SlotFree { slots_dir, want, job_mb, reserve_mb, cap }`**（§3.2 の分担の宿題の決着）。variant はデータだけを運び、meminfo と札の読み手は wait の内側（`admission::has_room`）が持つ。待ちの間の観測は lock を取らず札も消さない（回収と記録は lock の内側の受付だけ）。`pid()` は pid を見張らない本 variant で 0 を返す（`/proc/0` は無い）。
- **`slot=` の値**: `granted` / `degraded` / `unmeasured`、回収が在った周は `reclaimed:<n>`（枠を配れた周）か `<degraded|unmeasured>,reclaimed:<n>`（縮退と重なった周）。測れなかった理由は閉じた enum で `slot_why=<slots-dir|lock|meminfo|cores>` に残す（`cores` は行 w・§31.1）。meminfo が読めない周は札を回収しない（回収の数を残す前に縮退するため）。縮退（`degraded`）の周も 1 枠の札を置く。
- **包めない周（`Unconfined`）は 1 枠だけを取りにいく**（札は置く）。箱の無い行に並列度を上げると、溢れたときに殺されるのが席の側になる。
- **受付を通るのは gate の共通 verify の `{jobs}` 行だけ**。land の main 実測（`run_checks`・land.rs）は受付を持たず `jobs = 1` のまま撃つ（gate.rs `UNADMITTED_JOBS`・§3.3 の errata の `EFFECTIVE_JOBS` の改名）。main 実測の検出線は (c) で撃たなくなる。
- **受付の 4 行（`gate.mutants_jobs` / `gate.job_memory_mb` / `host.reserve_memory_mb` / `gate.slot_wait_s`）は `--rules` の manifest から読む**（pipe/cli.rs `limits_of`）。封じ込めの 3 線（§4.4・埋め込みだけ）と読み面が違うのは、待ちの上限を振る歯の fixture が gate へ届く口がここだけだからである。

### 3.3 実効 jobs の渡し方

- 宣言 file の共通 verify と検出線の穴を `{base}` と **`{jobs}`** の 2 つにする（declaration.rs `Holes::Base` → 穴の列挙を「宣言の行に置ける穴」の閉じた集合にする・ADR-0010 §2.1 の部分 supersede）。scribe2 自身の宣言は `cargo xtask mutants-diff --base {base} --jobs {jobs}`。
- gate は受付で得た jobs を `{jobs}` に置換して撃つ。`{jobs}` を持たない行は受付を通らない（枠を取らない＝mutants を持たない consumer は費用を払わない）。
- xtask `mutants-diff` は `--jobs N` を cargo-mutants の `--jobs` にそのまま渡す（値は持たない）。
- env で渡さない（C2.2 の精神・折り返しの裏口を作らない）。
- errata（s2-07l.157 の現物）: 置ける穴は declaration.rs の**閉じた集合**（`BASE_HOLES` = `{base}` `{jobs}`・行 w で `{threads}` が 3 つ目に加わった・§31.1）1 本が持ち、`unfit` の判定と gate の置換が同じ列を読む（片側だけに足すと、intake を通った行が穴のまま撃たれる）。受付が入るまでの実効 jobs は gate.rs の `EFFECTIVE_JOBS = 1`（§9 (a)）で、xtask 側の既定も 1（`--jobs` 無し・読めない字面・0 は 1 へ落とす＝道具に「速い既定」を持たせない）。

## 4. 封じ込め（ADR-0021 §2.2）

### 4.1 何を封じるか

器が起こす子 process の起動点は 4 つ（gate.rs `run_line_captured`〔verify 行・gate と main 実測の共通の 1 本〕・spawn.rs `launch_runner`〔runner〕・headless/mod.rs `build`〔claude = runner と lens〕・land.rs〔`--pr-cmd`〕）。verify 行と runner / lens の 3 つを scope に入れる（`--pr-cmd` は host の gh を呼ぶだけで軽い・射程外）。

### 4.2 形

- `systemd-run --user --scope --quiet --unit=<NAME>-<run>-<段>-<n> -p MemoryMax=<上限> -p CPUWeight=<gate.cpu_weight> -p OOMPolicy=continue -- sh -c <line>`。`MemoryHigh` は付けない（係数を持たない・rules 行を増やさない）。`OOMPolicy=continue` は包みを systemd の OOM 停止から外すため（既定の `stop` では kernel が箱の中の 1 process を殺した直後に unit ごと止められ、包みが終端行を出す前に SIGTERM で死ぬ・本 host 実測 2026-09-12: 既定で 3 回中 1 回が終端行なし、continue で 3/3 が終端行あり・rc 137）。
- **上限は 2 種**: `{jobs}` を持つ行 = `実効 jobs × gate.job_memory_mb`。それ以外の verify 行（`{jobs}` を持たない行〔workspace の nextest / clippy 等〕）= `MemTotal − host.reserve_memory_mb`（host の予約分だけを守る箱。行ごとの値を持たない＝rules 行を増やさない）。runner / lens / claude の包み = `1 × gate.job_memory_mb`（§12・契約表の行 c・裁定 id user 2026-09-15T18:2xZ。呼び手 4 か所が `Limit` の variant `PerJob(1)` を選ぶ＝ADR-0021 §2.2 の `MemTotal − host.reserve_memory_mb` の割り当てをこの面だけ置き換えた）。
- 止められたのは scope の内側の process だけで、席・他の便・他の project は影響を受けない。scope の `memory.events` の `oom_kill` が 1 以上の周は、その行を rc に依らず「測れなかった」に倒す（gate は INCONCLUSIVE・main 実測は `main-unmeasured`・赤に化けさせない）。**検出線の行（`detection-verify`・§5）は例外**で、`oom_kill` ≥ 1 でも測れなかったに倒さない——変異ごとの test process を箱の中で起こす道具は、無限 loop になる変異 1 つが kernel に殺されてもその死を吸収して完走する（測れた周・判定は rc と outcomes が持つ・record の `reason=oom-kill` / `peak_mb` は現物のまま残す・`s2-07l.228`〔.217 run 2 の実測 2026-09-13〕）。包みごと signal で死んだ周（終端行なし）は検出線でも従来どおり測れなかった。
- **runner の scope が殺された周**: verify 行の「測れなかった」とは極性を分ける（便の内容が測れないのではなく、便自身が host の予約分を超えた）。便は閉じた理由 1 つ（`Failed detail=oom-kill`・pipeline.md §5.2 の `runner-rc` と同じ終端の段・C2 の variant 1 つ）で終端し、`Failed` からは resume しない＝intake からの起こし直し。根拠は包みが出す `memory.events` の `oom_kill`（包みごと死んだ周は signal 死）。peak の記録先（runner.stdout.log の終端行）は契約 (a) で確定する。
- **lens の scope が殺された周**: FR9 の既存極性のまま **INCONCLUSIVE**（stdout が parse できない周と同じ・pipeline.md §5.3 の判定順を変えない・道具を揃えて同じ便を撃ち直せる〔FR14〕）。runner と違い便の成果は残っているので終端しない。
- 実測（2026-09-12・本 host）: cgroup v2・user scope に memory / cpu / pids の controller が委譲されていて上限が効く。transient scope は最後の process の終了で cgroup dir ごと消える（0.3 s 後に不在を実測）。
- **無い host**（`systemd-run` が無い・scope を作れない）: 封じ込めなしで**並列度 1** で走り、record に `confined=false reason=<閉じた enum の名>` を記す（理由は自由文にしない・C3.3）。止めない（systemd の無い host で便が 1 本も流れない形を作らない）。

### 4.3 測定の環（宣言値を測定値で置き換えるため）

読みは scope の**内側**で行う: 包みの `sh -c` が行の終了後に自分の `/proc/self/cgroup` の path（scope の内側ではそれが scope 自身＝prefix の導出は要らない・端末直起動で `user@<uid>.service` の segment が無い文脈でも成立する）から `memory.peak` と `memory.events` を読み、stdout の終端に固定形の 1 行で出す（固定形は `{` で始めない＝gate.rs `last_json_object` が lens の verdict / runner の質問 record を末尾から探す経路と衝突させない）。器はその行を pure な parser（in-file の歯・fixture 文字列）で剥がし、record に `peak_mb=<n> jobs=<k>` を残す。外から終了後に読む形は成立しない（transient scope は最後の process の終了で消える・2026-09-12 本 host 実測）。包みは `OOMPolicy=continue` で OOM 停止から外す（§4.2）。それでも包みが死んだ周は signal 死（record の 255・gate.rs `recorded_rc`）を代理にする。その周の record にも `reason=<閉じた enum>`（oom-kill / signal）を載せて外からの kill と弁別する（契約 (a)）。`memory.peak` の無い kernel と終端行の無い周は field を欠く＝0 と書かない。`gate.job_memory_mb` の宣言値は、peak の測定が溜まった後に裁定で置き換える（C10: 宣言値を実効に上げるのは測定を通してだけ）。台帳 s2-07l.152（検出行の記録）と同じ行に載せる。runner / lens が起こす claude の scope（argv の包み・epilogue を持てない）は、器が**走行中に** `memory.peak` を sample し（`systemctl show` で解いた cgroup dir を周期で読む・最後に読めた値が peak・読めない周は `-`）、runner / lens の stderr の `scope=` 行に `claude_peak_bytes=` で残す（§13・`s2-07l.273`）。

### 4.4 errata（現物との差・s2-07l.157・規範は §4.1〜§4.3 のまま）

実装で決め直した点だけを記す。どれも「止めない、縮退する」（§2）を強める側の差である。

- **unit 名は `<NAME>-<場所>-<段>-<n>-<pid>`**（§4.2 は `<NAME>-<run>-<段>-<n>`）。同じ id を別 process が同時に測る周（歯の並列走行）で transient scope の名が衝突し、2 本目が起動できず**偽の RED** になるため、pid を足して一意にした。`<場所>` は gate では便の worktree の dir 名（= run id）、`<n>` は `verify.jsonl` の record 番号である。
- **scope を作れるかを probe 1 回で先に確かめる**（process ごとに 1 度だけ憶える）。`systemd-run` が在っても user の session manager が無い host では scope を作れず、包んだ行は**撃たれないまま rc≠0** になる——これは縮退ではなく偽の RED である。理由は閉じた enum で `no-systemd-run`（起動できない）/ `no-scope`（作れない）を弁別する。
- **`MemTotal − host.reserve_memory_mb` が残らない host は包まない**（`reason=no-room`）。`MemoryMax=0M` の箱を作ると中の process が即座に全部殺される。
- **封じ込めの 3 線は埋め込み manifest から読む**（`--rules` の override は通らない・読めない周は `reason=no-rules` で包まない）。起動点 3 つのうち 2 つ（runner の包み・claude）は `--rules` を受ける口を持たないので、片方だけ override が効く形にすると同じ host の箱が別々の値で走る。
- **終端行の固定形は `confine-usage peak_bytes=<n|-> oom_kill=<n|->`** で、包みは `/proc/self/cgroup` の末尾が**自分の unit の `.scope`** である周にだけ出す（包みの外で読んだ別 cgroup の数を自分の peak と名乗らない）。器は包めた周の stdout だけを読む。
- **`peak_mb` は field を欠かさず `-` を書く**（§4.3 は「field を欠く」と書いた）。読み手が「field が無い」と「0」を取り違えないためで、極性は同じ（0 と書かない）である。
- **gate が起こす lens の 1 行（gate.rs `ask_lens`）も同じ包みを通す**。§4.1 の列挙は claude の構築点を「lens」と呼ぶが、gate が `sh -c` で起こす lens そのものを箱に入れないと §4.2 の「lens の scope が殺された周」を器が観測できない。判定順は不変（殺された周は INCONCLUSIVE・便は終端しない）。
- **verify 行の signal 死は、包めた周だけ**「測れなかった」へ倒す（包めていない行が外から kill された周は従来どおり赤のまま）。根拠が箱の中に在るかどうかで極性を分ける。
- **unit 名の末尾に process 内の通し番号 `<seq>` を足す**（`<NAME>-<場所>-<段>-<n>-<pid>-<seq>`・起動ごとに 0 から・s2-07l.234）。pid だけでは**同じ process が同じ `<n>` を 2 度撃つ周**（land の追随 → 再 gate → main 実測・場所はどちらも run id）を分けられず、1 周目の scope が孤児の process で active のまま残ると 2 周目が `was already loaded` で起動できず偽の RED になる（.208 run 3 の実測 2026-09-13）。`<n>` の意味と別 process の一意性（pid）は変えない。
- **行の終端で scope を片付ける**（s2-07l.234）: §4.3 の「最後の process の終了で消える」は、行が fixture の tmux server や shell を孤児で残す周に成立しない。包めた起動（verify 行・gate の lens・runner・claude の子）は子が終わった直後に `systemctl --user kill --signal=SIGKILL <unit>.scope` を 1 回撃つ（SIGTERM の猶予を待たない）。結果は閉じた enum（`gone`〔既に無い＝正常・記録しない〕/ `killed` / `failed` / `no-tool`）で、`gone` 以外の周だけ record に `scope=` を足す（verify 行 = `verify.jsonl` の行・gate の lens = `verdict.json`・runner / claude の子 = stderr の 1 行）。判定の極性は持たない（片付けの失敗で行を赤にしない・§4.5）。包めなかった周は撃たない。

### 4.5 極性

封じ込めは行為を止めうる判定を持たない（縮退するだけ）ので、ADR-0014 §2.1 の guard の定義（行為を止めうる判定を返す境界）に当たらず、**極性一覧に載せない**（受付と同じ）。可視化は record の `confined=` / `reason=`。「止めない」を選ぶ理由: 止めると systemd の無い host で便が流れず、縮退の並列度 1 は従来の費用と同じで安全側。

## 5. main 実測は木が同じなら検出線を撃ち直さない（ADR-0021 §2.4）

- 宣言 file に **`detection-verify`**（検出線の行の列・`{base}` `{jobs}` の穴を置ける・**任意の key**＝無ければ③は空・toy repo の宣言は不変・ADR-0010 §2.1 の部分 supersede）を足す。scribe2 自身は `cargo xtask mutants-diff --base {base} --jobs {jobs}` をここへ移し、`common-verify` から外す（ADR-0009 §2.3 の置き場の 1 文を ADR-0021 §2.6 (v) で読み替える）。③の行は②と同じく写し（intake が凍結した宣言）から読む。検出線の rules 行（R-C12-1）が deny に昇格した周は、同じ便でその行を `common-verify` へ戻す（deny する行は撃ち直す側・ADR-0021 §2.4）。検出線 = 落ちても deny しない行（C12.4）。rc は 3 値で極性が違う: rc 0 = 測定（生存は行に載るだけ）／ rc 1 = R-C12-1 が deny に昇格した周だけ現れ、gate は赤に数える／ rc 2 = 「測れなかった」（道具の不在・baseline 落ち）で、gate は赤に数えず INCONCLUSIVE へ倒す（他の行に赤が在る周は §28 が FAIL を先に読む・`Gated` に留まり測り直せる・[pipeline.md](./pipeline.md) §5.3 の判定順・`s2-07l.331`。以前は rc≠0 を一律に赤に数え、測れなかった周が FAIL → run N+1 で runner 1 周を払っていた〔.329 run 1・2026-09-15〕）＝測れなかったを通ったにも赤にも化けさせない。
- gate は ① write-set 照合 → ② common-verify → ③ detection-verify → ④ 契約 verify の順で撃つ（ADR-0009 §2.4 の順序に③を挿す・ADR-0021 §2.6）。verify.jsonl の record は schema 1 のまま**任意 field を足す**（`kind` / `jobs` / `peak_mb` / `confined` / `reason` / `slot` / `skipped` / `tree`・古い読み手は無視・ADR-0017 §2.1 の event と同じ足し方）。
- gate は verdict.json に **`tree`**（gate を撃った HEAD の `^{tree}` の sha）を残す（schema は 1 のまま field を足す・読み手は未知の field を無視する）。
- land の main 実測は record を **`verify-main.jsonl`**（gate と同じ record 形・別 file・gate の周の `n` と重ねない・現物の main 実測は record を書いていない）に書く。`git rev-parse <new>^{tree}` が verdict の `tree` と一致する周は **detection-verify を撃たず** `skipped=detection tree=<sha>` を記し、①②④ は従来どおり撃つ。一致しない周（在りえないが在れば）は全部撃つ。`tree` が無い verdict（旧 gate）も全部撃つ。
- 節約の実測（.136 run 2）: main 実測 78 分のうち変異検査が約 75 分。

### 5.1 検出線の値と便の規模を record に残す（s2-07l.152 / .206 / .189・C12.4 / C10）

- **判定行の記録**（.152 / .206・planner 裁定 2026-09-14「全行」）: gate は verify 各行の record（`verify.jsonl`・schema 1 のまま任意 field・§5）に、**kind と rc を問わず全部の行に** `line=<stdout の末尾の非空 1 行・逐語>` を足す（検出線 = xtask `mutants-diff` が出す `mutants-diff: total=… caught=… missed=… unviable=… timeout=… scope=…` の 1 行・形の正本は xtask の `Counts::line`／flip-check の判定行 `flip-check: RED-on-base ok … base-retried=N`〔[pipeline.md](./pipeline.md) §8〕・rc 0 で通った周の「撃ち直しで通った」は stderr にも残らないのでこの field だけが証拠）。**1 行だけ**を残すので stdout の量（無界）は record に入らない。core はこの行を **parse しない**（判定は rc のまま・`run_line_captured` の「stdout を判定に使わない」規律は不変・記録に写すだけ）。stdout が空・読めない・行の無い周は field を**欠く**（0 と書かない・「未取得」と「0 件」を混ぜない・C10）。land の main 実測（`verify-main.jsonl`・同じ関数）も同じ形で、③ を省いた周は `skipped=detection` のまま `line` を持たない。`pipe show --run` は detection 行の `line` を逐語で 1 行出す（外形 snapshot・他の行の `line` は record を読む）。kind で行を選ばない（C2: 1 関数・行の種類で分岐しない）。
- **便の規模**（.189・research ponytail §4 (2)）: land の面 5（`verdicts.jsonl`・schema 1 のまま任意 field・`order` の後ろ）に `size=<契約の size の字面〔S / M / L〕>`・`files=<touched file 数〔write-set 照合と同じ `git diff --name-only <base>..<new>` の件数〕>`・`lines=<+行数>/<−行数〔`git diff --numstat <base>..<new>` の合計・test 区間を除かない＝xtask flip-check の区間判定を core に写さない〕>`・`pub_symbols=<`git diff <base>..<new>` の追加行のうち `pub ` で始まる行の数〔字面走査の下界・ADR-0023 §2.3 と同じ扱い〕>` を足す。**閾値は持たない**（knob 0・分布が溜まった後に rules 行 1 本を裁定で足すのは別の裁定・C12.4 と同型）。計算は pure 関数（diff の text → 4 値・in-file の歯）で、git を撃つのは land の既存の経路。読めない周は field を欠く（0 と書かない）。stdout の land 行には出さない（面 5 の行だけ）。
- **却下**: (i) xtask が counts の json を書いて core が読む（core に第 2 の reader・path の literal を 2 crate で持つ）(ii) core で `mutants-diff:` の行を parse して 5 数を record に持つ（同上・値の読み手が無い間は逐語で足りる）(iii) `lines` から test 区間を除く（flip-check の区間判定の複製）。

## 6. 着地は gate 済みの便を先に通す（ADR-0021 §2.5・機構は .147）

- 原則: `Gated` ∧ verdict PASS の run が在る間、他の run の land は待つ（先に gate を通った便を先に着地させ、stale の連鎖を止める）。
- **順番の鍵は event log の replay で導く**（C3・別の状態 file を持たない）: 同じ state dir の run のうち「終端でない（Landed / Failed / Stopped でなく・最新の段が `Gated` なら verdict が FAIL でない）∧ `Gated` の event を一度でも持ち最新の verdict が PASS ∧ worktree dir が実在（retire 済みは外れる）」を **着地待ちの列** とし、列の順序は各 run の**最初の** `Gated` event の ts（同時刻は run id の辞書順）。**追随して段が `Implemented` に戻り撃ち直している run も列に残る**（鍵は最初の Gated の ts のまま・撃ち直しが FAIL なら終端で外れる）＝撃ち直し中に後続が番を得ない（run 1 ee36511 の gate FAIL・lens の指摘 2026-09-13T04:05Z: 最新の段が Gated の run だけを列に入れると撃ち直し中に列から外れ、3 本以上の形で後着の 1 本が (vi) を踏み追随 2 回になる）。自分より前に列に在る run が 1 本でも在れば、自分の land は待つ。順序は全順序なので待ちが循環しない。
- **待ちは完了 enum の variant 1 つ**（`Completion::LandTurn { state_dir, run }`・宣言順の末尾・C3.4）を足して唯一の wait 実装（`fleet::wait`）を通す。第 2 の poll loop を書かない。判定（列の導出と「自分の番か」）は pure 関数 1 本（入力 = replay した run ごとの (最新段, verdict, Gated ts, worktree 実在) の列と自分の id・出力 = `Turn::{First, After(run), Unmeasurable}` の閉じた 3 値）。列の材料（event log と `verdict.json`）が変わらない周は replay を省いて前回の判定を使う（`s2-07l.300`・形は [fleet-event-log.md](./fleet-event-log.md) §4「着地の列の待ちの費用」）。
- **上限は rules 行 `pipe.land_wait_s`**（Int・裁定 id 付き・C5）。上限を超えた周は**待たずに進む**（縮退・受付の枠 §3 と同じ極性: 詰まって止まるより stale 1 回の費用を払う側に倒す・断らない・止めない）。列を導けない周（store が読めない）は `Unmeasurable` として同じく進む（読めないを「列なし」に読み替えない・記録に残す）。**待ちが解けた周は番を再評価する**（run 2 bba45bd の追随 gate FAIL・lens 2026-09-13T04:35Z: `is_met` = `!After` は Unmeasurable でも真になるので、先頭の便が撃ち直しで verdict.json を書き直す瞬間に読めないと待ちが解け、再確認が `== Unmeasurable` だけだと After に戻っていても進む）: pure 関数 `after_wake(Turn) -> Next::{KeepWaiting, Proceed(Order)}` を 1 本置き、`After` → 待ち直す（残りの deadline で同じ完了 enum を唯一の wait 実装へ再投入・第 2 の poll loop は書かない）/ `First` → `Waited` / `Unmeasurable` → `Unmeasured`。**verdict.json は atomic に書く**（gate.rs の `settle` の書きを 1 関数にし、同じ dir の tmp へ書いて rename・読み手が途中の file を見ない＝Unmeasurable の瞬間を出す側で塞ぐ）。
- **記録**: land の record（verdicts.jsonl の行・schema 1 のまま任意 field・ADR-0021 §2.6 (iv)）に `order=` を 1 つ足す: `first`（待ち無し）/ `waited:<秒>`（列の前が空くのを待った）/ `degraded`（上限で進んだ）/ `unmeasured`（列を導けなかった）。stdout の land 行にも同じ `order=` を出す。
- **順番が来た便**: 既存の §5.4 の経路のまま追随（rebase）→ gate 撃ち直し（省かない・C12.6）→ CAS で着地。撃ち直しの間もその便は列の先頭に残るので他の便は待ち、(vi) の `stale base` は起きない（3 本以上の形でも各便の追随は 1 回）。
- **効かない形**: `--pr-cmd`（自 repo への PR の口・main を動かさない）は列を見ない（stale base を見ない形と同じ理由）。`pipe land` を席が手で撃つ周も同じ列を通る（経路は 1 本）。
- **限界（残す側・doc に書く）**: 列に在る run の `pipe run` process が死んで Gated(PASS) のまま放置されると、後続は上限まで待ってから縮退する＝操作役がその run を retire するまで 1 回ごとに `land_wait_s` を払う（.132 / .156 と同じ「置き去りの run」の箱・本便では解かない）。

### 6.1 errata（現物との差・s2-07l.147・規範は上の §6 のまま）

- **module は `pipe/queue.rs`**（判定の pure 関数 `turn_in`・最初の `Gated` の ts を選ぶ pure 関数 `first_gated_at`・列の導出 `queue_of`・待ち `await_turn`・起票時は `pipe/land.rs` に在り .349 系の分割で移った＝.357 の審査の申し送り 2026-09-15）。`Completion::LandTurn` の観測は `queue::turn_now` の 1 本を通る（wait の内側が読み手を持つ・`SlotFree` と同じ分担）。上限の読み口は pipe/cli.rs `land_run`（`--rules` の manifest から読む・env を読まない）。
- **撃ち直し中の「最新の verdict」は前の周の PASS**: gate は判定の確定時にだけ `verdict.json` を上書きする（gate.rs `settle`）ので、追随して `Implemented` へ戻った run は撃ち直しの間 PASS のまま列に残り、撃ち直しが FAIL / INCONCLUSIVE を確定した時点で外れる（FAIL の run は段が `Gated` のまま＝§6 の「終端で外れる」は verdict で外れる形）。
- **読めない判定も `Unmeasurable`**: 列に入りうる run（終端でない ∧ `Gated` を一度でも持つ ∧ worktree が実在）で `verdict.json` を読めない run が 1 本でも在る周は列を導けない（PASS かを測れない run を列から外すと、読めないを「列なし」に読み替えることになる）。worktree の実在を判じる repo（便の写し面 `repo`）を読めない run が在る周も同じ。store が読めない周は land の前提検査（replay）が先に rc 2 で断り、main 実測の材料（`base_of_run`）も同じ store を読むので、e2e の歯は events.jsonl を壊す形でなく「前の便の判定を壊した fixture」で `order=unmeasured` を測る。
- **`after_wake` は待った秒も受ける**（`after_wake(&Turn, waited_s) -> Next`・`First` の周の `Waited(<秒>)` を組むため・pure のまま）。`await_turn` は解けるたびに `after_wake` を通し、`KeepWaiting` なら残りの上限（`land_wait_s` − 経過）で同じ `Completion::LandTurn` を `fleet::wait` へ再投入する。残りが 0 の周の `KeepWaiting` は `degraded`（上限で進む）。
- **atomic な書きは `pipe/gate/lens.rs` の `write_verdict`**（`verdict.json.partial` へ書いて rename・落ちた周は書きかけを消して本 file を作らない）。gate.rs の `settle` の書きはこの 1 本だけを通る。
- **面 5 の `order` は key 列の末尾**（既存の 7 key の並びは動かさない）。`order=` は land が成立した周（main 実測が緑）の record と stdout にだけ載る。
- **衝突の起こし直し中の run も列に残る**: 追随の rebase が衝突して実装役を起こし直した run（`Implemented detail=rebase-conflict:`・pipeline-conflict.md §3）も、前の周の PASS が残り終端でない間は列の定義を満たす。起こし直しの間、後続は上限まで待ってから縮退しうる（上の「置き去りの run」と同じ箱・本便では解かない）。
- **歯の fixture も atomic に書く**（`s2-07l.357`・契約表の行 a）: 列の歯が別 thread から前の便の判定を書き換える fixture は `write_verdict` と同じ形（`.partial` へ書いて rename）で書く。素の write（truncate → write）は 20 ms の poll に書きかけを読ませ、`Unmeasurable` → `order=unmeasured` の flake になる（main 201b2ef の CI nextest が 1 度赤・測り直しで緑・local 44 回は全部緑＝遅い runner でだけ開く窓）。assert は緩めない（`Unmeasured` を `Waited` と同じにすると歯が空虚になる）。
- **やさしく言うと**: 審査を通った順に 1 本ずつ main へ載せる。前が詰まっていたら決まった時間だけ待ち、それでも空かなければ待たずに進む（その場合は main が動いて 1 回余分に審査し直すかもしれない）。

## 7. 歯（契約ごと・in-file の unit と `tests/e2e/` の e2e の分担）

e2e は binary を spawn し外部 command は PATH 先頭の stub で差し替える（pipe.rs / seat.rs の慣行）。`/proc/meminfo` と cgroup の file を binary の外から差し替える口は持たない（C2.2・裏口を作らない）ので、**測る計算は pure 関数にして in-file の歯が fixture 文字列で測り、e2e は stub の引数・record の field・札 file の実在だけを測る**。

- 受付（in-file・pipe/admission.rs〔§3.2.1 errata〕）: `capacity(meminfo_text, rules, live_jobs)` の式（by_avail / by_token の min・reserve の差引・0 の床）・読めない meminfo → `unmeasured`・壊れた札の parse → 回収の数・`slot_detail` の合成（Granted + 回収 n → `reclaimed:n`・縮退 + 回収 n → `<slot>,reclaimed:n`・回収 0 → slot だけ）。
- 受付（e2e・`pipe_slots_`・rules fixture は `slot_wait_s = 1`〔待ちが解ける歯だけ長い値〕）: tmp の state root に state dir 2 つ〔project 2 つ〕で同じ slots dir を見る・自 pid の札で `by_token` を 0 にすると待ち、`slot_wait_s = 1` で `jobs=1 slot=degraded`・存在しない pid の札が回収され `slot` が `reclaimed:1` を含む（枠の可否は host 依存〔errata s2-07l.158: 起票時は `slot=reclaimed:1` と pin していた・MemAvailable が `reserve + job` 未満の host では `degraded,reclaimed:1`〕）・`{jobs}` の無い行は札を作らない・終了で札が消える・置換後の `cmd` に実効 jobs が載る・待ちの途中で塞ぐ札を消すと上限を待たずに枠を配る（容量の 2 線を fixture で最小にして host の memory に依らせない・待ちの間に置いた死んだ札が残る＝待ちの観測は受付を回さない）。
- 封じ込め（e2e・`pipe_confine_`・偽 `systemd-run` = 引数を file に写して `sh -c` を exec する stub）: `-p MemoryMax=` が `{jobs}` 行で `jobs × job_memory_mb`、それ以外の行で `MemTotal − reserve`・`CPUWeight=` が rules 行・stub 不在で素の `sh -c` と `confined=false reason=`・stub の引数に `-p OOMPolicy=continue` が在る・runner / lens の起動も wrap を通る。**歯で測れるのは引数まで**: scope の外が死なないこと・`memory.peak` / `memory.events` が読めることは実 host の 1 回を契約の done に入れる（planner の再実測・host 依存）。peak / oom_kill の読みは in-file（cgroup file の fixture 文字列）。
- main 実測（e2e・`pipe_detection_`・verify 行は「呼出回数 file に 1 行足す」stub）: verdict.json の `tree` が gate の HEAD の木と一致・一致する周は③の stub が呼ばれず `verify-main.jsonl` に `skipped=detection` が載り**②④は呼ばれる**（回数 file を行ごとに分ける）・`tree` を壊した fixture では③も呼ばれる・`tree` 無しの verdict でも呼ばれる・検出線の rc≠0 は gate FAIL のまま。検出線の oom（`pipe_detection_oom_`・偽 `systemd-run` の PATH で gate を撃つ）: (a) 検出線の行が `oom_kill=1` の終端行を出し rc 0 の周は verdict が lens の verdict（INCONCLUSIVE でない）で record に `reason=oom-kill` が残る／(b) 共通 verify の行の `oom_kill=1` は従来どおり INCONCLUSIVE／(c) 検出線の行が包みごと signal で死んだ周（終端行なし・rc 255）は従来どおり INCONCLUSIVE。
- e2e の state dir は tmp root の**直下に置かない**（親が tmp root になり、gate / land を撃つ既存の全 test が `<tmp>/<NAME>-host/slots/` を共有して flaky になる）: 既存 helper `tmp()` の呼び手で state dir を `<tmp>/state` に 1 段下げる（契約 (b) の write-set に tests/e2e/pipe.rs の既存呼び手を含める）。
- 宣言（in-file・declaration.rs）: `detection-verify` の穴（`{base}` `{jobs}` 以外は Unfit）・先頭語 / 制御文字 / repo 外 path の検査は共通 verify と同じ関数（`check_lines`）を detection-verify の行にも掛ける（ADR-0010 §2.3 (2)・ADR-0021 §2.6・迂回行を宣言に置けないことを歯で pin）・key の無い宣言は従来どおり通る・空配列は不備のまま（ADR-0010 §2.1）。
- rules: 5 行の kind 件数と外形 snapshot・欠落は RuleError（既存の型）。
- 完了 enum: `Completion::SlotFree` の網羅 match（compile）と wait の唯一性（実装が 1 本・呼び手は複数でよい・grep でなく型で）。
- 記録（§5.1・e2e・`pipe_record_`）: detection-verify の stub が `mutants-diff: total=3 caught=2 missed=1 unviable=0 timeout=0 scope=x` を stdout に出す fixture で `verify.jsonl` の detection 行にその逐語が `line` として載り、common の行には載らない・stdout の無い detection 行は `line` を欠く・`verify-main.jsonl` の skip の周も欠く・`pipe show` が逐語を 1 行出す（外形 snapshot）。便の規模（in-file・pure）: numstat / name-only / diff の text の fixture から 4 値・空の diff は `files=0 lines=0/0 pub_symbols=0`（読めた 0）・読めない周は None。e2e（`pipe_land_`）: land の面 5 の行に `size= files= lines= pub_symbols=` が `order` の後ろに載る。

## 8. 射程外

- 複数 host にまたがる受付（受付は host 単位・event log も host ごと）。
- state root を分けた project 同士の受付の統合（運用で親を揃える）。
- CPU の上限（重みだけ）・swap の制御・cargo の `-j`（cargo-mutants の `--jobs` の内側は cargo の既定）。
- 便の同時本数の上限（admission control・口座の自律制御 s2-07l.142 と併せて別途）。
- 30 秒の実待ちの歯の改修（s2-07l.151・本 doc より先に流す）。
- 検出線の deny 化そのもの（C12.4 の裁定。本 doc は昇格時の置き場だけ）。
- 検出行の値を **parse して型で持つ**こと（§5.1 は逐語の 1 行を残すまで。5 数を core の型で持つ形は、値を読む側〔report・deny への昇格 C12.4〕が要る周に別途・xtask の `Counts` と第 2 の parser を core に作らない ADR-0010 §2.1 の趣旨）。

## 9. 契約（bead）

1. **(a) 実効 jobs + 封じ込め**（M）: rules 5 行 + `RuleKind` 5 variant + 外形 snapshot / kind 件数の歯・`{jobs}` の穴・xtask `--jobs`・`Confinement`（guard ではない・極性一覧に載せない）・record の `jobs= confined= reason= peak_mb=`・oom_kill → unmeasured。runner / lens の起動点（spawn.rs / headless/mod.rs）も同便。**受付が入るまでは上限をそのまま実効に使わない**: (a) 単独では `jobs = 1` のまま（並列を上げるのは (b) の後）。
2. **(b) 受付**（M）: slot dir・受付札・容量の測定（pure 関数）・`Completion::SlotFree` と唯一の wait・縮退・回収・歯。land 後に (a) の実効 jobs が上限まで上がる。
3. **(c) main 実測の検出線省略**（S）: `detection-verify`（任意 key）・verdict `tree`・`verify-main.jsonl`・land の skip・歯。
4. **(d) land の順序**（.147・.146 の後）。
5. **(e) 検出線の 1 行を record に**（S・.152 + .206 を 1 便・§5.1）: gate.rs の `step_record` に detection 行だけ `line`・`Fired` が stdout の末尾行を運ぶ・`pipe show` の外形・歯。
6. **(f) 便の規模を面 5 に**（S・.189・§5.1）: land.rs の `export_verdict` に 4 field・pure な計算関数・歯。(e) と gate.rs / land.rs で交差しない読みだが e2e/pipe.rs で交差＝直列。

(a) と (b) は pipe/ と rules/ で交差するので直列。(c) は (a) と宣言 file・gate.rs で交差するので直列。s2-07l.151 は test/ と seat/ で交差しないので先に流せる。

## 10. 却下案

- **xtask に `--jobs 4` を焼くだけ**: 合計を見ないので複数 gate / 複数 project で溢れる。user 裁定で不採用。
- **cargo-mutants の test suite を絞る（`-p` を狭める・unit だけ）**: 変異の kill 判定が弱くなる（検出線の質を落とす）。.151 の 30 秒待ちの解消で同じ以上の効果が出るので採らない。
- **main 実測を全部省く**: gate は run の worktree（untracked を含む）で撃ち、main 実測は tracked だけの木で撃つので環境が違う。deny する行は撃ち直す価値が残る。検出線だけを省く。
- **受付を fleet DB（C3）に置く**: DB は state dir ごと＝project ごとで、project 横断にならない。受付札は C3 の lease（fleet の割当）ではなく host 内・pid 寿命の予約の印で、失われても過剰配分側に倒れ封じ込めが最悪を閉じる＝真実の置き場ではない（ADR-0021 §5 (D)）。
- **cgroup の path を `XDG_RUNTIME_DIR` から組む**: env を読む（C2.2）。`/proc/self/cgroup` から導く。
- **memory が足りない周は便を断る（fail-closed）**: 断ると systemd の無い host や memory の少ない host で便が 1 本も流れない。縮退（並列度 1）は従来の費用と同じで安全側。

## 11. 後続

- `gate.job_memory_mb` の宣言値を peak の測定で置き換える裁定（C10）。
- state root の運用（同じ host の state dir は 1 つの親）を doctor（C3.2・移行後の epic）の検査項目に足す。
- SRS v0.7 に NFR（host の資源を枯渇させない）を足す材料は planner state dir に置いた（user の /folio-architect 手番）。

## 12. runner / lens / claude の箱の上限を 1 × gate.job_memory_mb に揃える（契約表の行 c・`s2-07l.230`・cargo mutants の deny は ADR-0025 / `s2-07l.168` で既着）

- **出所・現物**: .222 run 1 で Failed detail=oom-kill（runner が自分の箱の中で cargo mutants を回し host の memory を圧迫）が起きた。user の裁定（2026-09-15・裁定 id user 2026-09-15T18:2xZ・C5 / A2）は「runner の箱の上限を gate の job と同じ rules 行の値にし、runner の allowlist から cargo mutants を外す」。後半（cargo mutants の deny）は ADR-0025 / `s2-07l.168` で main に在る: rules 行 `runner.denied_commands`（値に "cargo mutants"・`RuleKind` の variant `RunnerDeniedCommands`）・Bash の command guard `crates/scribe2/src/hook/command.rs`（pre-tool-use・FailClosed）・intake の同型判定。歯は `crates/scribe2/tests/e2e/hook.rs`（hook_command_guard_denies_a_denied_sequence_from_bash / hook_command_guard_matches_sequence_regardless_of_flag_order）と `crates/scribe2/tests/e2e/rules.rs`（rules_embedded_manifest_declares_the_denied_commands_row）。`crates/scribe2/src/hook/permission.rs` は Bash の承認要求を一律 deny する別の門で、語列の判定は持たない。前半（箱の上限）の現物: `limit_of`（`crates/scribe2/src/pipe/confine.rs`）は gate の verify 行（`crates/scribe2/src/pipe/gate/verify.rs`）だけが呼び、`{jobs}` を持つ行を `Limit` の variant `PerJob`（実効 jobs × gate.job_memory_mb）・持たない行を `HostReserve`（MemTotal − host.reserve_memory_mb）に振る。runner / lens / claude の包みは `limit_of` を通らず、呼び手 4 か所が `HostReserve` を字面で選んでいる: `crates/scribe2/src/pipe/spawn.rs`（runner の process）/ `crates/scribe2/src/pipe/gate.rs`（gate の lens）/ `crates/scribe2/src/pipe/review.rs`（審査の lens）/ `crates/scribe2/src/headless/mod.rs`（runner と lens が起こす claude の子＝cargo が実際に走る箱・§13 のとおり runner の包みとは別 scope）。この割り当ては ADR-0021 §2.2（runner・lens = MemTotal − host.reserve_memory_mb）のもので、本節は裁定 id でその面だけを置き換える（`Limit` の variant と rules 行は増やさない）。
- **形（何を作るか）**: (1) 上の呼び手 4 か所の limit を `HostReserve` から `PerJob(1)` に替える＝上限 = 1 × gate.job_memory_mb（値は manifest が持つ）。`Limit` の variant は増やさない（§4.2「2 種」のまま）。`limit_of` と gate の verify 行の箱は変えない。`confine.rs` は `HostReserve` の doc の 1 行（「runner・lens」の語を外す）だけ。(2) 禁じる語列の rules 行・`RuleKind` の variant・hook の deny は ADR-0025 / `s2-07l.168` で既着＝本便は行を増やさず、既存の歯が緑のままであることを回帰の柵にする。(3) runner の雛形（`crates/scribe2/src/headless/runner.txt`）の「実行してよい command」節に「検出線（cargo mutants）は gate が撃つ・runner は撃たない（禁じる語列で止まる）」の 1 行を足す。雛形の外形は insta の snapshot（`crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap`・歯 `headless_runner_prompt_external_form`）が pin しているので、その 1 行ぶんだけ snapshot が動く（write-set に持つ・[pipeline.md](./pipeline.md) 行 af〔`s2-07l.449`〕が同じ雛形の 1 行で同じ snapshot を動かした先例・2026-09-22 の便 1 本目が `Questioned about:write-set` で止まった再現）。(4) 設計の写し: §4.2 の割り当ての句（本 doc）と pipeline.md §6 の封じ込めの pointer に同じ 1 句。
- **歯**（呼び手 4 か所に 1 本ずつ・どれか 1 か所を `HostReserve` のまま残すと赤になる）: `crates/scribe2/tests/e2e/pipe/spawn.rs`（runner の unit の MemoryMax・接頭辞 `pipe_confine_runner_limit_`）/ `crates/scribe2/tests/e2e/pipe/gate.rs`（gate の lens の unit の MemoryMax ∧ 同じ gate の `{jobs}` 無しの verify 行は host の箱のまま・両方向を 1 本で・接頭辞 `pipe_confine_lens_box_`）/ `crates/scribe2/tests/e2e/pipe/intake.rs`（審査の lens の unit `-review-1` の MemoryMax・審査の歯は既存の pipe_review_ 接頭辞と同じ file・接頭辞 `pipe_confine_review_box_`）/ `crates/scribe2/tests/e2e/headless.rs`（claude の unit の MemoryMax・雛形の 1 行・接頭辞 `headless_runner_box_`）。偽 systemd-run は gate.rs の歯の stub が private なので、spawn.rs / intake.rs / headless.rs の歯は同型の stub を歯の中で書く（§13 と同じ・module の可視性を触らない）。
- **既着の回帰の柵（(2) の「緑のまま」を測る側）**: 上の (2) は「歯を書かない」ではなく「**既着の歯を行の verify が完全名で撃つ**」である。撃つ 3 本は `hook_command_guard_denies_a_denied_sequence_from_bash` / `hook_command_guard_matches_sequence_regardless_of_flag_order`（`crates/scribe2/tests/e2e/hook.rs`）と `rules_embedded_manifest_declares_the_denied_commands_row`（`crates/scribe2/tests/e2e/rules.rs`）で、この 2 file は「柵として撃つ file」として write-set に在る（歯の置き場の門が verify の歯の file を write-set の中に要求する・[contract-source.md](./contract-source.md) §20）。本便はこの 2 file を**書き換えない**（撃つだけ）。
- **触らない**: gate.job_memory_mb / host.reserve_memory_mb の値そのもの、gate の箱の上限、rules 行・`RuleKind`・`crates/scribe2/src/hook/permission.rs`。
- **却下案**: 上限だけ下げて allowlist をそのままにする案は、ADR-0025 が語列で止めた時点で前提が消えた。`limit_of` に stage 名の分岐を足す案は、呼び手が行を渡さず `Limit` を字面で選んでいるので unit 名を嗅ぐ分岐になり C2（1 関数 1 列挙）に反するため不採用。`Limit` に 3 つ目の variant を足す案は `PerJob(1)` と同じ式になるため不採用（§4.2「2 種」を守る）。allowlist を cargo ごと外す案は build / nextest まで撃てなくなるため不採用。
- **risk（裁定の値そのものは触らない）**: claude の箱が 1 × gate.job_memory_mb になると、runner の cargo nextest / clippy の workspace build がその箱で走る（gate は同じ行を host の箱で回している・§4.2）。溢れた周は §4.2 の runner の極性どおり Failed detail=oom-kill で終端し record に残る＝値の見直しは実測後の別裁定（A2）。

## 13. runner / lens が起こす claude の子の peak memory を record に残す（契約表の行 d・`s2-07l.273`）

- **出所・現物**: planner の実測（2026-09-14）で、runner の record の confine-usage peak_bytes は包みそのものの数 MB しか持たず、claude の子（`crates/scribe2/src/headless/mod.rs` の build が起こす別 scope）は 5000 MB を超えても記録されていなかった。gate-cost.md §4.3「測定の環」の runner 面が空振りし、gate.job_memory_mb の宣言値を測定で置き換える材料が溜まらない。現物: `crates/scribe2/src/pipe/confine.rs` の終端行は包みの末尾で memory.peak を読み、`crates/scribe2/src/headless/mod.rs` の build は claude の scope の unit 名を返して終端で `release_scope` を呼ぶ。
- **形（何を作るか・run 1 審査 INCONCLUSIVE 2026-09-16「§13 に write-set / seam の材料が無い」の解として書き直し）**:
  1. **読む時機 = 走行中の sample**（release の直前ではない）: transient scope は最後の process が終わると消える（`confine.rs` の `Released::Gone` の doc「unit が既に無い（最後の process の終了で消えた・正常）」・verified）ので、`child.wait()` の後に読む形は大半の周で `Gone` になり測れない。claude の scope の cgroup dir は起動直後に 1 回 `systemctl --user show <unit>.scope -p ControlGroup --value`（子 process・`SYSTEMCTL` 定数・PATH 解決）で解き、`<cgroup root>/<ControlGroup>/memory.peak` を走行中に周期で読む（high-water mark なので最後に読めた値が peak・値は単調）。読めない周（file 無し・parse 不能・`show` の失敗）は欠落の記号 `-` を保つ（0 と融合しない・C10）。歯は scope の消滅を模す＝偽 claude が終端の前に `<cgroup root>/<ControlGroup>` を消し、走行中に読んだ値だけが行に載ることを測る（終端で 1 回読む実装は `-` になる）。
  2. **cgroup root は typed な値**: `confine.rs` の定数（`/sys/fs/cgroup`）を既定にし、`runner` / `lens` の口の flag `--cgroup-root DIR`（optional・`KNOWN_FLAGS` に 1 つ・env は読まない・C2.2）で差し替える。歯はこの flag に偽 dir を渡す（`/sys/fs/cgroup` 固定 path への注入 seam は持たない）。定数は新設する（現物は `script` の epilogue に字面が埋め込まれているだけ・埋め込みは触らない）。
  3. **書く面 = 既存の stderr 1 行**（`headless/runner.rs` の `runner: scope=<…>`・`headless/lens.rs` の `lens: scope=<…>`・record と呼んでいるのはこの行）。同じ行に `claude_peak_bytes=<n|->` を 1 語足す。stdout（判定の面）と rc は変えない。`spawn.rs` の `pipe: runner scope=` は包みの側で本便の外。この行は Confined の周に必ず出す（`Gone` の周は `scope=gone`・現物の `release_scope` は `Gone` を `None` に落とすので正常系ではこの行が出ない）。runner / lens は `release` を直に撃って `Released` の字面を写す＝`release_scope` の filter と型は他の呼び手のまま。
  4. **sample の置き場**: runner は stream の行を読む loop（`headless/runner.rs`）の各周で 1 回読む（file の read 1 回・追加の待ちは無い）。lens は `wait_with_output` を `try_wait` の poll（1 秒・`std::thread::sleep`・async 無し C13.3）に替え、各周で読む。閉じた型 `Peak { Bytes(u64), Unreadable }` と読み手 1 関数（pure な parse + I/O）は `confine.rs` に置く。poll の間も stdout は別 thread（`std::thread`・`read_to_end`）で読み切り、終端で join する（子の stdout は pipe なので、誰も読まないと 64 KiB で子が書き待ちになり poll が永久に回る）。
  5. **systemd 無しの host**: `Confinement::Unconfined` の周は scope が無い＝`show` を撃たず、行に `claude_peak_bytes` の語を**出さない**（現物と同じく `scope=` も出ない・stub は不要＝PATH から `systemd-run` を外した fixture がそのまま「無い host」）。
- **歯の seam（write-set に数える）**: `crates/scribe2/tests/e2e/headless.rs` に偽 `systemd-run`（内側の command をそのまま exec）と偽 `systemctl`（`show … -p ControlGroup --value` に fixture の path を返し、`kill` に rc 0）を PATH の先頭に置く shim を歯の中で書く（`e2e/pipe/gate.rs` の shim は流用しない＝module の可視性を触らない）。`--cgroup-root` に tmp dir を渡し、`<tmp>/<ControlGroup>/memory.peak` を歯が書く。usage の外形 snapshot `e2e__headless__headless_external_form.snap` は flag の追加で動く。
- **触らない**: 包みの形・上限・`sh -c` 側の終端行（`confine-usage` の epilogue）・`release_scope` の型と `Released` の variant・`Call` の field・`spawn.rs` / `gate/verify.rs` / `review.rs` / `gate/lens.rs` / `fleet/usage.rs` の `release_scope` の呼び手。
- **却下案**: claude も同じ包みで起こして終端行を出させる案は、直接起動の裁定に触れ permission の境界を動かすため不採用。記録しないままにする案は、宣言値が測定を通って実効に上がる環（C10）が閉じないため不採用。release の直前に 1 回読む案（run 1 までの形）は scope が既に消えている周が正常系なので不採用。`release_scope` の戻り値に peak を載せる案は呼び手 6 か所の型が動き write-set が倍になるため不採用（peak は別の 1 関数）。

## 14. 純移動と証明された行を検出線の母集団から外す（契約表の行 e・`s2-07l.292`）

- **出所**: admin の観測（2026-09-14）で、純移動 40 項目の便の検出線が 2 周とも大きな母集団を回し、生存 1 本は移した行の既存の弱さで新しい情報を出さないまま時間と memory を払っていた。
- **現物**（planner が grep で実測・main 0b7e0a1）: gate の検出線は diff の追加行を母集団にして撃つ経路を持つ。証明は `crates/scribe2/src/pipe/move_proof.rs` の `judge`（`pub fn`）が移動した item の名と本文の一致を純関数で確かめて `LensInput` を組み、`keep`（`pub fn`）がその要約を run dir に残す。**`LensInput::Summary` が運ぶ `MoveSummary` は `text`（描画済みの要約）1 field だけで、行範囲を持たない。** 行範囲そのものは在る: 証明の内部で item は `lines`（1 始まり・両端含む head 側の区間）を持ち、`matched_of` が移動の行数を数える所と `residual_lines` が残差を弁別する所の 2 か所で既に読まれている。つまり本行が足すのは**区間の新しい計算ではなく、証明が既に持つ区間を呼び手へ渡す 1 本の道**である。`Check::Detection` は `crates/scribe2/src/pipe/gate/verify.rs`、検出線の record は `crates/scribe2/src/pipe/gate/record.rs` に在る。**検出線の口**（main 0878dad・`.292` run 2 の問い）: gate は `cargo xtask mutants-diff --base … --jobs … --threads … --teeth …`（`verify.rs` の `DETECTION`・的の在る便は record が run dir の file を `--targets <file>` の対で足す・§16 (2)）の 1 行で撃ち、`crates/xtask/src/mutantsdiff.rs` は `write_diff` が `git diff <base>...HEAD` を**自分で**組んで `target/mutants-diff/in.diff` に落とす＝外から母集団の diff を受ける旗は無い。gate が組んだ母集団を渡すには xtask 側に受け口 1 つが要るので、本行の write-set は `crates/xtask/src/mutantsdiff.rs` を含む（受け口の形は約束 2）。
- **前提の充足（実測 main b063ba0）**: `crates/scribe2/src/pipe/move_proof.rs` は当初、幅 120 で正規化した行数が上限 R-C4-2 ちょうどで余地 0 だったため、本行は §44（[pipeline.md](./pipeline.md) の行 al・純移動）が割った後に始まると条件付けた。**行 al は着地済み**（`crates/scribe2/src/pipe/move_proof/read.rs` が main に在り、親の正規化行数は 1154＝余地 346 で size S の見積 100 を満たす）＝本行の前提は満たされ、depends は要らない。割った後も `judge` / `keep` / `LensInput` / `MoveSummary` と突き合わせの後段は親に残るので、本行の write-set は親 1 本で足り、受け皿の file は要らない（行 al が移したのは diff を読んで item の列にする前段だけ・親は移した item の区間を今と同じ字面で読む）。
- **約束（この行が作るもの・番号は done と 1:1）**:
  1. `MoveSummary` は一致と証明された item の head 側の行範囲（file と区間の対の列）を `text` と並んで運ぶ（証明の判定・`text` の字面・`keep` の書き口は不変）。
  2. gate は検出線を撃つ前に、`LensInput::Summary` の行範囲に入る hunk を落とした母集団用の diff を run dir 直下の file に組み、検出線の 1 行に `--diff <file>` の対で渡す（`--targets` と同じ渡し方・的の無い純移動でない便は 1 字も足さない）。xtask の `mutants-diff` は `--diff <file>` が在る周はその file を `in.diff` に写して `write_diff` を撃たず、無い周は従来どおり `git diff <base>...HEAD` を組む（受け口はこの 1 旗だけ・除外の判定は core に留まる）。
  3. 残る追加行が 0 本になる純移動だけの便は `Check::Detection` を赤にも測定未了にもせず、純移動として名指す記号を、撃った検出線の record の field に `pure-move=<落とした hunk の追加行数>` で残す（`record.rs` が検出線の段だけに `patch_id=` を足すのと同じ置き方・撃たない周と `LensInput::Diff` の便には書かない）。**`crates/scribe2/src/pipe/gate.rs` の `Detection`（Run / Skip / Carry）は増やさない**: その網羅 match は `crates/scribe2/src/pipe/train.rs`（210 行）と `crates/scribe2/src/pipe/gate/record.rs`（81 行）の 2 か所で、`land.rs` / `cli/step.rs` は variant の名指しだけ＝閉じた値を増やすと write-set の外の `train.rs` が動く。
  4. `LensInput::Summary` にならない便（`LensInput::Diff`）は従来どおり全ての追加行を母集団にする。
  5. 純移動と証明された item の外の追加行（`mod` 宣言の追加・可視性の変更・残差）は母集団に残る。
- **歯**（接頭辞 `pipe_gate_detection_pure_move_`・置き場は `crates/scribe2/tests/e2e/pipe/gate.rs`・base の当たりは 0 本。done (1) の「要約の字面と証明の判定は不変」は `crates/scribe2/src/pipe/move_proof.rs` の既存の in-file の歯 3 本〔`move_proof_judge_pins_each_reason`＝判定の理由の pin・`move_proof_comment_diff_inside_items_is_counted_and_markers_inside_items_are_checked` と `move_proof_comment_verbatim_is_absent_when_comments_match`＝要約の本文の字面〕を verify の別の行で名の全体で撃って測る＝要約の本文を描き換えれば赤・2026-09-20 の審査 FAIL vacuous-assert の再現）: (a) 純移動だけの便は検出線の母集団が 0 行になり `Check::Detection` が赤にも測定未了にもならず record に純移動の記号が残る／(b) 移動と実変更が混ざる便は実変更の追加行だけが母集団に入る（移した item の区間の行は入らない）／(c) 移動でない追加行だけを持つ便（`mod` 宣言の追加・可視性の変更）は母集団に残り従来どおり撃つ／(d) `LensInput::Diff` の便は全ての追加行が母集団に入る（従来の極性）／(e) xtask の受け口は `crates/xtask/src/mutantsdiff.rs` の in-file の歯（接頭辞 `mutants_in_diff_`・`measure_args` の歯と同じ置き場・base の当たりは 0 本）: `--diff <file>` の在る周は `git diff` を撃たずその file が母集団になり、無い周は従来どおり・file が読めない周は測定未了の口で断る。
- **触らない**: xtask 側の変異の生成・測定・数え手（`--diff` の受け口と `write_diff` を撃つか否かの分岐以外）、lens の入力の判定、`judge` / `keep` の証明そのものと `MoveSummary` の `text` の字面、`crates/scribe2/src/pipe/move_proof.rs` の in-file の歯の名と assert。
- **却下案**: 純移動便の検出線を全部 skip する案は、移動でない追加行（mod 宣言の追加や可視性の変更）まで母集団から落としてしまうため不採用。除外の判定を xtask 側に置く案は、証明が core の `crates/scribe2/src/pipe/move_proof.rs` に既にあり、同じ判定を 2 か所に持つことになるため不採用（xtask に足すのは判定でなく diff の受け口 1 つ）。母集団の行数だけ記録して 0 行なら撃たない案（`.292` run 2 の問いの第 2 案）は、移動と実変更が混ざる便（歯 (b)）で移した行が母集団に残り出所の痛みが解けないため不採用。行範囲を `keep` の写し（run dir の file）から読み直す案は、gate が同じ周に持っている値を file 経由で往復させるだけで、写しの形を跨版契約にしてしまうため不採用。

### 14.1 errata（現物との差・`s2-07l.292`・規範は上の §14 のまま）

- **落とす単位は hunk でなく `+` 行**: 純移動の新 file は module doc・`use`・移した item を 1 つの hunk に持つので、hunk ごと落とすと約束 5 の行（残差）まで母集団から消える。`crates/scribe2/src/pipe/move_proof.rs` の `population` は動いた item の区間に入る `+` 行だけを落とし、残る `+` 行を hunk の中で連なる本数ごとの挿入の hunk（`@@ -<o>,0 +<n>,<k> @@`・HEAD 側の行番号は元の diff のまま）に切り直す。`-` 行と context は運ばない（母集団は追加行だけ）。
- **「一致と証明された item」は file を跨いで動いた item だけ**: 同じ file に留まった item（可視性・字下げ・コメントだけの差）の区間は `MoveSummary` に載せない＝その行は母集団に残る（約束 5 の「可視性の変更」）。
- **record の field**: 検出線の record は `pure-move`（落とした `+` 行の本数・数値）を持ち、遮断器で撃たなかった行と要約にならない便の行は欠く。xtask の `--diff` は file の byte をそのまま `in.diff` に写す（空の file は 0 行の母集団）。

## 15. gate の周ごとの検出線の出力を run dir へ写し、show はその写しから読む（契約表の行 f・`s2-07l.298`）

- **出所・現物**: admin の提案（2026-09-14・.286 run 1 の実測）で、検出線の出力が便の worktree の out にだけ在り、追随周の撃ち直しが out を作り直すと前の周の生存の一覧が消えることが分かった。gate の record（`crates/scribe2/src/pipe/gate/record.rs` が書く verify.jsonl）は行ごとの rc を残すが生存の一覧は残していない。現物（verified・main f678bd0）: pipe show の判定行の読み手は **`crates/scribe2/src/pipe/cli/show.rs` の private な `detection_lines(`**（引数は record の path 1 つ・戻り値は行の列・`s2-07l.349` の純移動で `cli.rs` から移った・呼び手は `crates/scribe2/src/pipe/cli/show.rs` の `run(` 1 か所だけ）で、verify.jsonl の detection record の line を逐語で写す。
- **形（何を作るか・番号は done と歯の対）**:
  1. **写す**: gate が検出線を撃った直後に、その周の判定行（record の `line=` と同じ字面）と出力（`outcomes.json` と missed.txt）を run dir 配下の**周ごとの置き場**へ写す（上書きせず周ごとに別の置き場・周の番号は Gated の verdict 件数）。
  2. **不在を弁別する**: 出力が無い周は不在を表す marker を同じ置き場に置き、判定行も無い周は不在と分かる 1 行（**0 件と弁別**・判定行の形と衝突しない字面）を残す。空の写しで「0 件だった」に倒さない（C10・NFR4）。
  3. **読み口を向け替える**: `detection_lines` の読み口を **verify.jsonl から (1) の写しの判定行へ**向け、verify.jsonl を読む経路を残さない（2 実装にしない・C2）。**pipe show は現物でも worktree の out を読んでいない**（読み手は `crates/scribe2/src/pipe/cli/show.rs` の `detection_lines(` 1 本で、そこへ渡る path は run dir の verify.jsonl だけ・verified）＝本項が動かすのは「verify.jsonl から写しへ」の 1 手だけで、「out を読まなくなる」ことは約束の中身ではない（それを測る歯は HEAD でも base でも緑＝空虚になる）。
  4. **数え直さない**: total / caught / missed の数え手は `crates/xtask/src/mutantsdiff.rs` の 1 つのまま（C2）＝pipe show の判定行の**字面は 1 字も変わらない**。
- **歯**（`pipe_gate_detection_copy_` 接頭辞・置き場は `crates/scribe2/tests/e2e/pipe/gate.rs`・偽の検出線は同じ `crates/scribe2/tests/e2e/pipe/gate.rs` の既存の gate の歯が使う stub と同じ作り）: (a) 2 周撃った gate で周ごとの置き場が 2 つ在り、1 周目の生存の一覧が 2 周目の後も読める（上書きされない）／(b) 出力を書かない偽の検出線の周は marker が在り、判定行の写しは在る／(c) 判定行も出力も無い周は不在の 1 行が在り、**0 件の周（出力が在って missed が 0）とは別の字面**である（(b)(c) が (2) の否定の枝）／**(d) (3) の pin は「写しが出所である」ことを 2 例で測る**（base で必ず RED になる形）: (d1) gate の後に歯が**写しの判定行だけ**を別の字面へ書き換え（verify.jsonl の record は元のまま）、`pipe show` がその**書き換えた字面**を出す＝base は verify.jsonl から読むので元の字面が出て落ちる／(d2) gate の後に歯が**写しを消す**と `pipe show` が (2) の不在の 1 行を出す（verify.jsonl に detection record が在るまま）＝base は record から判定行を出すので落ちる。(4) の「字面が変わらない」は**既存の歯**が受け、行の verify が完全名 `pipe_record_show_external_form`（置き場は `crates/scribe2/tests/e2e/pipe/gate.rs`・外形 snapshot `e2e__pipe__gate__pipe_record_show_external_form.snap`）で撃つ。
- **触らない**: 検出線の実行そのもの・判定・verify.jsonl の record・`crates/xtask/src/mutantsdiff.rs` の数え手。
- **却下案**: admin が Gated の時点で手で写す運用は散文の手順になり、追随の再 gate が同じ秒に起きると間に合わないため不採用。worktree の out を周ごとに別名で残す案は、worktree が retire で畳まれるため置き場として不適で不採用。

### 15.1 errata（現物との差・`s2-07l.298`・規範は上の §15 のまま）

- **置き場は run dir 直下の `detection/<周>/`**（§15 は「周ごとの置き場」とだけ書いた）。中身は判定行の写し 1 file・段の秒 1 file・写せた出力（`outcomes.json` / missed.txt の**在る物だけ**）・出力が 1 つも無い周の marker 1 file の 4 種で、名は本便の code が 1 か所（gate の記録の module）に持つ。
- **段の秒も同じ置き場へ写す**（§15 (1) は判定行と出力だけを挙げた）。`pipe show` の行は判定行の逐語 + `secs=<秒>` で、(3) の「verify.jsonl を読む経路を残さない」を満たすには秒の出所も写しでなければならない（record を読む第 2 の経路を残すと、判定行の出所が 2 つに戻る）。秒を持たない周は file を置かない＝`secs=0` と書かない（C10）。**値は record と同じ 1 つ**で、数え直さない（§15 (4)）。
- **周の番号は `Gated` の件数の次**（§15 (1) の「Gated の verdict 件数」の現物）。`Gated` は周の終端で 1 件追記されるので、写しを書く時点の件数は済んだ周の数である。**既に在る写しの最大の番号も併せて見る**のは、event log を読めない周に 1 周目の写しを潰さないためで、上書きしないことが写しの目的そのものだからである。
- **撃たなかった周は写さない**（設計 §30 の [`Detection::Skip`] の周）。撃っていない周の置き場を作ると、撃って 0 件だった周と読み分けられない（(2) の極性と同じ理由）。
- **写しの出所は便の worktree の cargo-mutants の出力 dir**（`target/mutants-diff/out/mutants.out/`）で、中身は**読まない**（数え直さない・§15 (4)）。無い出力は写さず marker に倒し、在るのに読めない出力は記録の書けない周と同じ極性（gate は rc 2）で止める——「無い」と「読めない」を融合しない（C10）。
- **歯の fixture は `target/` を ignore する**: 検出線の出力が untracked のまま残ると 2 周目の precheck が「clean でない」で止まり、2 周分の写しを測れない（実 repo でも `target/` は ignore される）。

## 16. 契約が名指した生存行に変異を当てて outcomes の 4 kind + 不在の 5 値で記す（契約表の行 g・`s2-07l.341`）

- **出所・現物**: .338（歯だけの便）の gate で検出線が 2 周とも母集団 0 になった（admin 実測 2026-09-15）。diff が mod tests の中だけで、検出線が変異を生やす本体の行を持たなかったため。歯だけを足す便が base の生存行を撃ち落としたかどうかを、器がこれまで測っていなかった。現物: 契約 file（`crates/scribe2/src/pipe/contract.rs` の `Contract`・write_set field を含む）は変異の的を宣言する field を持たず、`crates/xtask/src/mutantsdiff.rs` の検出線を撃つ口も diff の追加行を母集団にする経路しか持たない。
- **形（何を作るか）**: (1) 契約 field を 1 つ新設し、契約が生存行（ファイル・行・変異の名）を的として名指せるようにする（**実装側の面は 4 つ・verified 2026-09-20 main 9a218c5・4 つ目は 2026-09-21 の便 1 本目の Questioned で実測**: 欄の正本は `crates/scribe2/src/pipe/table.rs` の `FIELDS`〔記録時点で必須 7・任意 9 の 16 欄で、件数を pin する in-file の歯が同じ `crates/scribe2/src/pipe/table.rs` に在る〕・行を 1 欄ずつ読むのは `crates/scribe2/src/pipe/table/parse.rs` の欄の読み手・**行を契約 file の写しへ運ぶのは `crates/scribe2/src/pipe/contract.rs` の `pub fn render(`**〔行と設計 pointer と write-set を取って TOML の本文を返す・読み戻しは同じ `crates/scribe2/src/pipe/contract.rs` の optional の欄〕。`render(` の**唯一の呼び手**は `crates/scribe2/src/pipe/cli/intake.rs` で、行をそのまま渡すだけなので**受付の file は触らない**〔欄の増減は `render(` の中で閉じる・同 file の `ContractRow` の構築は `..row.clone()` の functional update で欄の増減に閉じている〕＝行の write-set に `crates/scribe2/src/pipe/cli/intake.rs` は入らない。**4 つ目の面**: `ContractRow` を全欄列挙の struct literal（`..Default` 無し）で組む site は 2 つで、どちらも in-file の歯の fixture `row()`——`crates/scribe2/src/pipe/contract.rs` と `crates/scribe2/src/pipe/table/check.rs`——であり、欄を足すと後者も 1 行増える（fixture の行 1 本だけ・`check.rs` の判定の本体と歯の assert は 1 字も触らない）。ゆえに `crates/scribe2/src/pipe/table/check.rs` は行の write-set に入る。欄の追加は `FIELDS` の tracked な生成物 `contracts/schema.toml` の描き直しを伴う〔xtask check が render と tracked の差分 0 を測る〕。受付を通る側を測る歯は `crates/scribe2/tests/e2e/pipe/intake.rs`）。(2) 的を直接絞って撃つ口を `crates/xtask/src/mutantsdiff.rs` に足し、的ごとの分類を閉じた enum 1 つで記す判定行を出す。**口は既存の `mutants-diff` の旗 1 つであって新しい subcommand ではない**（verified 2026-09-20: `crates/xtask/src/main.rs` の subcommand の分岐は `mutants-diff` の 1 行で `crates/xtask/src/mutantsdiff.rs` の `pub fn run(` へ丸ごと渡し、旗は `run(` が自分で読む＝**dispatch 側の `crates/xtask/src/main.rs` は触らない**ので行の write-set に入らない・C2「1 口」）。値は cargo-mutants の outcomes の 4 kind（caught / missed / unviable〔コンパイル不能〕/ timeout＝現物の `Counts` が読む 4 つの数と同じ語）+ 的が outcomes に当たらない absent（file・行・変異の名が現物とずれた）の 5 値で、母集団 = 的の本数・5 値の和 = total。noop（変異前後で挙動差なし）は outcomes の上では missed と同じで測れないため分類に持たない（挙動差の A/B は本節の射程外）。(3) gate はこの field が在る便では的を絞った口で検出線を撃ち、無い便は従来どおり diff の追加行を母集団にする。verdict の判定は変えない（検出線は deny ではない）。
- **歯**（`pipe_gate_targets_` / `mutants_targets_` / `pipe_intake_targets_` の 3 接頭辞・(1)(2)(3) の対）: (a) **受付**（`crates/scribe2/tests/e2e/pipe/intake.rs`・接頭辞 `pipe_intake_targets_`）= 的の欄を持つ行が受付を通って生成された契約 file に的が写り、**欄を持たない行は従来どおり通る**（(1) の否定の枝）／欄の値が形に合わない行は typed に断られる。(b) **的を撃つ口**（`crates/xtask/src/mutantsdiff.rs` の in-file の歯・接頭辞 `mutants_targets_`）= 的 3 本の fixture の outcomes から 5 値（caught / missed / unviable / timeout / absent）が出て**和 = 的の本数**になり、現物とずれた的だけが absent に落ち、outcomes を読めない周は 5 値に化けず測れていない側へ倒れる（C10）。(c) **gate**（`crates/scribe2/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_targets_`）= 欄を持つ便は的を絞った口で撃ち母集団 = 的の本数・欄を持たない便は diff の追加行を母集団にする従来の経路のまま・どちらの周も **verdict の判定は変わらない**（検出線は deny ではない）。(d) **欄の追加の生成物**は既存の歯が受け、行の verify が完全名 `contract_schema_matches_the_tracked_file_and_the_field_slice`（置き場は `crates/scribe2/tests/e2e/pipe/intake.rs`・`contracts/schema.toml` の本文と `FIELDS` から描いた本文の byte の一致を測る・verified 2026-09-20）で撃つ＝描き直しを忘れた周はここで赤になる。
- **触らない**: diff の追加行を母集団にする従来の経路、verdict の 3 値、挙動差の A/B（手順のまま・別便）。
- **依存**: `crates/scribe2/src/pipe/gate.rs` / `crates/scribe2/src/pipe/gate/record.rs` で契約表の行 e・f と交差するため、それらの後に流す。
- **却下案**: 歯が名指す関数の本体全体を母集団に加える案は、宣言した的を測定するという型に合わないため不採用。noop を分類に入れる案は、outcomes だけでは missed と区別する規則が無く（挙動差は変異前後の実 binary の A/B でしか測れない）偽の outcomes に札を貼るだけの空虚な歯になるため不採用。admin の手作業を続ける案は散文の手順になり、便が増えると追いつかないため不採用。

### 16.1 errata（現物との差・s2-07l.341・規範は上の §16 のまま）

- **欄の名と値の形**: 欄は targets（任意の list・FIELDS の末尾）で、値 1 つが的 1 本の字面 file:行:変異の名 である。file は空白を持たない .rs・行は 1 以上の十進・名は空でない。行の後ろに桁を 1 つ挟んだ形（cargo-mutants の一覧の行 file:行:桁: 名）もそのまま受け、桁は照合に使わない。形の判定は core の契約 file の読みに 1 本だけ置き、行の欄の読み手（受付と CI の contracts check が通る同じ読み）がそれで測って、外れた値を contract-table の target-form（rc 1）で名指す。
- **契約 file の側**: 生成の写しは的の在る行だけが targets の key を持つ。読み込み済みの契約の struct には field を足さない（struct literal の site が write-set の外に在る）——gate は run dir の契約の写しから的の列を読み直す（読めない写しは gate の記録の失敗＝的を空と読んで従来の経路へ黙って倒さない）。
- **gate から口への渡し方**: 的の在る便は、gate が的の列を run dir 直下の file（1 行 1 本）に書き、便の写しの検出線の各行の末尾に --targets と その file の path を足して撃つ。的の無い便は写しの行を 1 字も変えない。行の穴は残るので受付と箱の選び方は変わらない。
- **口の側（mutants-diff の旗 --targets）**: diff を絞る旗の対を落とし、的の file ごとの file 旗と的ごとの名の regex（字面を逃がした式）で cargo-mutants を絞る（歯だけの便の的は diff に無い base の行である）。分類は outcomes.json の 5 つの数（従来と同じ読み・読めない周は rc 2）と、同じ出力 dir の 4 つの一覧（caught / missed / unviable / timeout の txt）で、的は file・行・名の 3 つが一致した一覧の kind に落ち、どれにも当たらなければ absent。数が 1 以上の kind の一覧が無い周は測れていない（rc 2）。outcomes.json が無く道具が rc 0 の周は変異 0 本＝的は全部 absent の測定である。
- **判定行**: mutants-diff: total=<的の本数> caught= missed= unviable= timeout= absent= scope= teeth= population=targets。rc の極性は従来と同じ R-C12-1 の 1 本（absent は赤にしない）。

## 17. 歯の fixture の一時 dir を終端で必ず片付ける（契約表の行 h・`s2-07l.343`）

- **出所・現物**: admin の実測（2026-09-15）で、e2e の fixture の一時 dir が多数残っていた。掃除そのもの（消す操作）は user の承認（A1）を要するため本便の外だが、残る原因は歯の側にある。現物（verified・main f678bd0）: 一時 dir の作り手は **`crates/scribe2/tests/e2e/main.rs` の `pub fn make_tmp_dir() -> Option<PathBuf>`**（`std` だけの helper・`tempfile` は A3 ゆえ足さない）で、素の path を返すだけで guard を返さない。直に呼ぶ歯の file は main.rs を含めて **8 file**（母集団 = `crates/scribe2/tests/e2e` の `.rs` 21 file・2026-09-20 実測）で、各 file の呼出は**その file の局所 helper の中の 1 か所ずつ**（main.rs だけ 2 か所）＝戻り値の型を変えるとその helper の戻り値の型が伝播する。削除は歯の中で成功した経路だけが呼ぶため **panic した歯は dir を残す**。main.rs の 2 つ目の呼出は path を正規化して別の値にする＝包みをそこで落とすと dir が残るので、正規化の後も包みが生きる形にする。
- **本節から外れた 2 面（前の形の (2)(3)・重複を残さないため明記する）**: (2) だった「`release_scope` の終端で failed を戻す」は §25（契約表の行 p・`s2-07l.421`）が `--collect` と併せて持つ＝本行は `crates/scribe2/src/pipe/confine.rs` を**触らない**。(3) だった「e2e の疑似 seat を Drop で畳む」は既に着地済み（`crates/scribe2/tests/e2e/seat.rs` の `Drop` の実装が独立 socket 上の tmux server を畳み、既存の歯 `seat_isolated_session_is_torn_down_when_guard_drops` が測る）＝本行は作らない。
- **形（何を作るか・番号は done と歯の対）**:
  1. **包む型**: 一時 dir を包む型を歯の側（main.rs）に新設し、`Drop` で dir を再帰削除する（panic でも unwind の途中で消える・`std` だけ・依存を足さない）。作り手はこの型を返す形へ変える。
  2. **呼び手の変更は最小**: 包む型は path として読める形（`Deref` で `Path` を貸す）にし、path を繋ぐだけの呼び手は 1 字も変えない。path を struct の欄へ持つ呼び手だけが欄の型を変える。
  3. **残す口は引数で**: 落ちた歯の dir を調べたい周のために、包む型から **path を取り出して guard を降ろす** 1 つの口を置く（env を読まない・C2.2）。降ろした周は消えない。
  4. **素の path を返し続ける helper は guard を歯の thread へ預ける**（実装時の追記・`s2-07l.343`）: `tests/e2e/pipe.rs` の `tmp()` は write-set の外の `pipe/` 配下の歯が `tmp().join(..)` の一時値の形でも呼ぶ＝包みを返すと文の終わりで dir が消える。ゆえに戻り値は `PathBuf` のまま、包みを thread local の列へ預けて path だけを返す口（`held`）を置く。libtest は歯 1 本を 1 thread で走らせるので、列は歯の終わり（panic で落ちた thread を含む）で drop される。正規化は包みを保ったまま path を差し替える口（`canonical`）で行う。struct の欄に置く呼び手のうち tmux の guard と同居するもの（`hook.rs` の `PluginPlace`）は guard を先頭の欄に移す（欄は宣言順に drop＝席を畳んでから socket の dir を消す）。
- **歯**（`e2e_fixture_` 接頭辞・置き場は `crates/scribe2/tests/e2e/main.rs`）: (a) 包む型を drop した後に dir が**無い**（中に file を置いた周も再帰で消える）／(b) **panic した歯**でも dir が消える（std の unwind を捕まえる口の中で作って落とし、外で不在を測る＝(1) の否定の枝・現物の形では残る・(4) の預けた包みも panic した thread の join の後に不在）／(c) guard を降ろした周は drop の後も dir が**在る**（(3) の枝・降ろす口が無ければ空虚になる pin）／(d) 作り手が 2 回続けて別の path を返す（既存の一意性が壊れていない）。
- **触らない**: 封じ込めの形（scope の unit 名・上限）と `crates/scribe2/src/pipe/confine.rs`（§25 の領分）・疑似 seat の畳み方（着地済み）・各歯の assert の中身。既存に残っている dir・scope・process の掃除自体（A1 の後に別途行う）。
- **却下案**: 依存 crate を足して一時 dir を管理する案は、依存の追加が承認事項（A3）になり標準ライブラリの `Drop` で足りるため不採用。歯の終端で個別に手で消す既存のやり方を続ける案は、失敗した歯が残す問題を解かないため不採用。作り手の戻り値を変えず別の guard 型を「使いたい歯だけ」が使う案は、既に残している 18 か所が変わらず問題が残るため不採用。

## 18. fleet/store.rs の起動時刻算術に境界と property の歯を足す（契約表の行 i・`s2-07l.247`）

- **出所・現物**: .222 の close 時に admin が明記した積み残し。.182（変異生存の対処）で扱った起動時刻の算術（clock tick を ms に変換する割り算と剰余）が `crates/scribe2/src/fleet/store.rs` の `started_ms_in` へ移った結果、.222 の write-set の外に出て未着手のままになっていた。前提の .245 は既に着地済み。
- **形（何を作るか）**: `started_ms_in` の内側で使う純粋な変換（tick と Hz から ms を出す計算）に、境界値（0・1・Hz−1・Hz・Hz+1）を pin する歯と、任意の tick で計算が一致することを確かめる property の歯を足す。/proc を読まない純粋な内側の関数がまだ無ければ 1 つ切り出し、`started_ms_in` はそれを呼ぶ形にする（挙動は変えない純粋な移動）。
- **触らない**: `started_ms_in` の外形・戻り値の意味。

## 19. gate の歯・検出線の baseline・flip-check の base 段を fail-fast にしない（契約表の行 j・`s2-07l.383`）

- 何が起きているか: admin の実測 2026-09-16（`.354` run 2 / `.247` run 1）。gate の共通 verify `cargo nextest run --workspace --no-tests=fail`（`.vessel.toml` の `common-verify`）と検出線 `cargo xtask mutants-diff`（`detection-verify`）の baseline は fail-fast で、負荷で歯 1 本が落ちると残りが未実行のまま便が Gated INCONCLUSIVE で 1 周を払う（`.354` run 2: 1448 本中 915 passed / 1 failed / **992 未実行**・738 秒）。落ちた歯が 1 本しか名指されないので、flaky か本物かの弁別も 1 本ずつしか進まない。
- 現物（verified・main 43706fe）: (i) `.vessel.toml:6` の `common-verify` の nextest 行に `--no-fail-fast` は無い。同じ行は `CLAUDE.md` の done 区間（`<!-- done:begin -->`〜・`ci.yml` から生成・`claude-md-done` の検出線が drift を見る）と `.github/workflows/ci.yml:13` にも在る＝3 面が同文。(ii) `crates/xtask/src/mutantsdiff.rs` の `measure_args` は `cargo mutants --in-diff … -p … --no-shuffle --copy-vcs true -o … --jobs N` で、cargo-mutants の baseline は `cargo test` を既定で撃つ（`--` の後ろの引数を持たない＝失敗した test binary の後ろは走らない）。(iii) `crates/xtask/src/flipcheck.rs` の `nextest_args` は `nextest run --workspace --no-tests=fail --color never` + extra（base 段の `failed_tests` は落ちた歯を列で読み `retry_named` が名指せた歯だけ 1 回撃ち直す＝名指せる本数が増えるほど撃ち直しが効く）。
- 形: 3 か所とも **`--no-fail-fast`** を足す。判定は不変（落ちた歯が 1 本でも赤・rc の意味は変えない）。(i) `.vessel.toml` / `ci.yml` / `CLAUDE.md` の done 区間の nextest 行を `cargo nextest run --workspace --no-tests=fail --no-fail-fast` に（3 面同文・`claude-md-done` の検出線が一致を見る）。(ii) `measure_args` の末尾に `--` `--no-fail-fast` を足す（cargo-mutants が `cargo test` へ渡す引数・baseline と変異の両方に効く・`Scope` の束縛は不変）。(iii) `nextest_args` の固定引数に `--no-fail-fast` を 1 つ足す（`--color never` の隣接は不変）。
- 歯の置き場（run 1 = 審査 FAIL 2026-09-16「3 面同文の歯が write-set の外の読み手を要る」の解）: (a) `nextest_args` の pin は `crates/xtask/src/flipcheck_tests.rs`（既存の `--color never` の pin と同型）。(b) `measure_args` の pin は `crates/xtask/src/mutantsdiff.rs` の in-file の歯。(c) 3 面同文の pin は `crates/xtask/src/check_prose_tests.rs` に置き、既存の歯と同じく `CARGO_MANIFEST_DIR` から repo root を解いて `.vessel.toml` / `ci.yml` / `CLAUDE.md` の 3 file を**字面で読む**（`common-verify` の nextest 行・`run:` の nextest 行・done 区間の nextest 行が同文で `--no-fail-fast` を持つ）。`xtask check` の `claude-md-done` の読み手（`claude_md.rs`）は触らない＝(c) は同じ読み手を使わず、3 file を直接読む独立の pin。
- 触らない: 検証行の順序・`R-C12-1` の極性・`retry_named` の回数（1 回）と範囲（名指せた歯だけ）・`gate.job_memory_mb` 等の値・`claude_md.rs`。値の線は増えない（flag 1 つ）。
- 却下案: gate の nextest だけ直す（検出線の baseline と flip-check の base 段で同じ 1 本が同じ損失を出す・`.247` は両方で落ちた）／`--no-fail-fast` を rules 行にする（極性でも閾値でもない・argv の形）／CI は fail-fast のまま残す（CLAUDE.md の done 区間が ci.yml から生成される＝3 面が割れる）。

## 20. e2e の tmux fixture の prompt 待ち（壁時計 5 秒）を負荷下でも足りる値へ（契約表の行 k・`s2-07l.392`）

- 何が起きているか: admin の実測 2026-09-16 04:5xZ（#246〔docs-only〕の CI で `tests/e2e/hook.rs` の歯 `hook_brief_planner_carries_the_dialogue_surface_lines` が `guard.ready()`〔assert の字面「独立 socket に session を立てられる」〕で落ち、main で単独なら PASS・本日 2 例目の「並列が高いときに fixture の tmux 席が立たない」型・1 例目は検出線の rc 2 = `.390`）。現物（verified・main aee95a3）: `tests/e2e/seat.rs` の `start_seat_sized` が独立 socket に `new-session` を立てた後、`capture` の末尾が prompt の字になるまで **`PROMPT_WAIT` = 5 秒**を 100 ms 刻みで待ち、届かなければ `ready = false` の guard を返す（helper は panic せず呼び側の `#[test]` が落とす）。`ready()` の呼び手は e2e の 7 file・100 箇所（`hook.rs` 8 / `seat.rs` 2 / `seat/account.rs` 13 / 作り直しの歯の file〔削除済み〕 35 / `seat/launch.rs` 14 / `seat/register.rs` 5 / 管理 tick の module〔削除済み〕 23・grep）＝同じ 1 定数が全部の tmux fixture の起動待ちを決める。負荷下（CI の並列・便の gate と build の同時走行）では `sh -i` が prompt を描くまで 5 秒を超える周が在る。
- 形: `PROMPT_WAIT` を **60 秒**にする（`.385` = account-autonomy.md §12 と同じ型）。緑の周は prompt が描かれた時点で抜けるので費用は変わらず、赤の周だけ待ちが延びる。`new-session` 自体が失敗した周（`out.status.success()` が偽）は待たずに `ready = false` を返す形も不変＝「立てられない」と「描くのが遅い」の弁別は現物のまま。assert の字面・`ready()` の型（bool）・`Drop` の畳み方・socket と `-f /dev/null` の分離は不変。本体不変の歯だけの便＝base で RED を作れないので、`seat.rs` の test 区間へ `// flip-check: retroactive <bead-id>` の札を 1 行足して flip-check を通す（id 必須・id 無しの札は数えない・base から持ち越した `.196` の札は効かない＝pipeline.md §5.3 の対の規則・`.342` / `.385` の型・xtask 側に登録は無い）。
- 触らない: 器の src・`ready()` の呼び手 100 箇所・並列度（並列の上限は write-set の重複と直列依存だけ・user 直命 2026-09-16）・`PROMPT` の字と `PS1`。
- 却下案: 起動を 1 回だけ再試行する（memo の案・`new-session` が通った後の遅さには効かず、通らなかった周の再試行は server の二重起動と socket の取り合いを生む＝待ちを伸ばす方が 1 定数で閉じる）／並列度を下げる（user の許しが要る側・歯を直せば要らない）／歯を `#[ignore]` にする（対話面の行の pin を失う）／CI の再実行で凌ぐ（1 周の損失が続く・#246 で実測）。

## 21. 検出線が rc 2（測れなかった）で終えた周は同じ gate の中でその行だけ 1 回撃ち直す（契約表の行 l・`s2-07l.390`）

- 何が起きているか: admin の実測 2026-09-16 04:2xZ（`.247` / `.377`・母集団 = 本日 gate に到達した便）。検出線 `cargo xtask mutants-diff`（宣言の `detection-verify`・`{jobs}` の穴）が負荷下（load average 10 前後・並列 10 便）で rc 2（測れなかった・赤ではない）で終わると gate は INCONCLUSIVE に倒し、便は 1 周（追随 → 再 gate = workspace 全件 + 変異）を丸ごと払う。`.247` は再 gate 1 回で PASS＝時間切れの型で、並列を上げるほど頻度が上がる。現物（verified・main aee95a3）: `pipe/gate/verify.rs` の `run_checks_admitted` が `CHECKS` の宣言順に全行を `fire` 1 本で撃ち、`pipe/gate/record.rs` の `detection_unmeasured`（`Check::Detection ∧ rc 2`）を `Counted.detection_unmeasured` に数え、`pipe/gate.rs` の判定が INCONCLUSIVE に倒す。flip-check の base 段には名指せた歯だけの撃ち直し（xtask の `retry_named`・`base-retried=N`）が在るが、検出線には撃ち直しの口が無い。
- 形: `run_checks_admitted` の **`Check::Detection` の行だけ**、`fire` の結果が `detection_unmeasured` なら**同じ行を同じ受付の形（穴の値・箱・枠）でもう 1 回** `fire` し、2 回目の `Step` を採る（1 行につき撃ち直しは 1 回・3 回目は撃たない）。撃ち直した `Step` は `retried = true` を持ち（`Step` に field 1 つ・`unwrapped` / `check_write_set` / 1 回目の `fire` は false）、`record.rs` の `step_record` が `verify.jsonl` の record に **`retried=1`** を任意 field で足す（schema 1 のまま・§5 の足し方・C10 = 撃ち直した事実を 0 に潰さない）。1 回目の stderr の末尾は従来どおり stderr の log に残す＝`record.rs` の `append_stderr` は `steps` の `Step` と 1 対 1 なので、1 回目を `steps` から落とすと 1 回目の末尾は書かれない。よって 2 回目の `Step` が **1 回目の rc と stderr の末尾を運ぶ欄**（`retried_from`・`Option<(rc, tail)>`・`retried` の bool はこれの有無に畳む）を持ち、`record_verify` はその欄を見つけた周に `append_stderr` を 1 回多く呼んで 1 回目の見出し（`## n=<i> rc=2 retry=1 cmd=…`）+ 末尾を先に書き、2 回目は従来の見出しで続ける（`verify.jsonl` の record は 2 回目の 1 本だけ・診断 log だけが 2 段になる）。1 回目の `Step` は `Counted` に数えない＝「測れなかった → 測れた」の向きだけが変わる。2 回目も rc 2 なら従来どおり `detection_unmeasured` → INCONCLUSIVE（`retried=1 rc=2` の record が残る）。rc 1 の検出線（deny 昇格後の赤）と検出線以外の rc 2 は撃ち直さない（`detection_unmeasured` の 1 点だけ・C11.2 の極性は不変）。land の main 実測（`run_checks`・§5）は同じ 1 本を通るので同じ挙動になる（木が同じ周は検出線を撃たないので実際には gate だけ）。
- 歯（`pipe_detection_retry_` 接頭辞・`tests/e2e/pipe/gate.rs`・fixture の script は歯の中で書く: 奇数回目は `exit 2`・偶数回目は `exit 0` の検出線／常に `exit 2`／`exit 1`）: (a) rc 2 → rc 0 の検出線を持つ gate は PASS で終わり、撃たれた回数が +2・`verify.jsonl` の `kind=detection` の record は 1 本で `rc=0` `retried=1`／(b) rc 2 が 2 回の周は INCONCLUSIVE・回数 +2（3 回目は無い）・record は `rc=2` `retried=1` の 1 本／(c) rc 1 の検出線と共通 verify の rc 2 は撃ち直されない（回数 +1・`retried` 無し）／(d) (a) の周の `verify.stderr.log` に 1 回目の見出し（`rc=2 retry=1`）と 2 回目の見出し（`rc=0`）が**この順で両方**在る（1 回目の末尾を捨てない pin）。
- 触らない: `fire` の中身（受付・箱・`run_line_captured`）・`CHECKS` の順序・`detection_unmeasured` の判定・`Counted` の形・gate の verdict の 3 値と rc・common-verify / 契約 verify の行（撃ち直さない）・xtask の `retry_named`・`mutants-diff` の rc の意味（道具の側が持つ）。
- 却下案: 検出線の rc 2 を PASS 扱いにする（測れないを緑にする・C10 違反）／並列度を下げる（user の許しが要る側・user 直命 2026-09-16）／gate 全体を撃ち直す（1 周と同じ費用）／撃ち直しの回数を rules 行にする（値の線が増える・1 回で足りる＝2 回目も rc 2 なら負荷でなく道具の側）／1 回目の `Step` も `Counted` に残す（`position` が 1 回目の rc 2 を拾って INCONCLUSIVE に倒す＝撃ち直しが効かない）。

## 22. 変異検査の中の test 走行に thread の上限を渡し、入れ子の並列で core を溢れさせない（契約表の行 m・`s2-07l.393`）

- 何が起きているか: user の観測 2026-09-16 05:5xZ「メモリは大丈夫な割に CPU に負荷がかかって温度が上がっている」。planner / admin の実測（同時刻・verified）: load average 57 / 16 core・CPU 81℃・memory 9 / 62 GB・`cargo-mutants` 2 本（`.322` / `.349` の gate・各 `--jobs 4`）・e2e の test binary 42 本同時・rustc 0。読み（deduced）: 検出線は `cargo-mutants --jobs 4` で変異 4 本を同時に build + test し、各 job の `cargo test` は既定で全 core に広がる（libtest の test-threads = core 数）＝gate 1 本で最大 4 × 16 並列、2 本同時で 128 並列相当。受付の枠（§3・`admission.rs` の `capacity`）は memory だけを数えるので、62 GB の host ではこの入れ子を止めない。同じ周に `.247` が負荷由来の flaky 2 本（tmux fixture の prompt 待ち・usage refresh の 65.9 秒）で Gated FAIL した＝温度と CPU が便の周を焼いている。
- 形: xtask `mutants-diff` の `measure_args` が **既存の末尾 `-- --no-fail-fast`（§19・行 j）の後ろに `-- --test-threads <t>` を足す**＝引数の末尾は `-- --no-fail-fast -- --test-threads <t>` の 5 語で、cargo test へ渡る引数は `--no-fail-fast -- --test-threads <t>`（1 つ目の `--` で cargo-mutants から `cargo test` へ、2 つ目の `--` で cargo test から test binary へ渡る・現物 = `cargo-mutants mutants --help` の usage `[-- <CARGO_TEST_ARGS>...]`「Pass remaining arguments to cargo test」＝1 つ目より後ろは 2 つ目の `--` を含めて逐語で渡る）。`--` は 2 つになるので、§19 の歯の「`--` は 1 つだけ」の assert は「1 つ目の直後が `--no-fail-fast`・2 つ目の直後が `--test-threads`」に改める（変更された歯＝flip の RED の一部）。`t = max(1, floor(cores / jobs))`・`cores` = `std::thread::available_parallelism()`（読めない周は **1**＝速い側へ倒さない・`JOBS_FLOOR` と同じ向き）・`jobs` は §3.3 のとおり器から来た値。これで gate 1 本の test 並列は jobs × t ≤ cores に閉じる（例: 16 core・jobs 4 → t 4・16 並列）。値は rules 行にしない（cores と jobs から決まる導出値・線が増えない・C10 の derived）。`--jobs` / `-p` / `--in-diff` / `-o` の対と `--no-shuffle` / `--copy-vcs` は不変。
- 触らない: 受付の枠（`capacity`・memory の項）・`gate.mutants_jobs` の値・baseline の撃ち方（cargo-mutants の側）・共通 verify の `cargo nextest run --workspace`（gate 1 本につき 1 回で入れ子ではない）・flip-check。受付の枠に CPU の項を足すか（rules 行 = user 裁定）は本便の実測の後に別便で判定する。
- 却下案: `NEXTEST_TEST_THREADS` / `RUST_TEST_THREADS` を env で渡す（§3.3「env で渡さない」・C2.2 の seam）／`--jobs` を下げる（変異 1 本ずつの費用は上がり総時間が伸びる・入れ子は残る）／`.cargo/mutants.toml` の `additional_cargo_test_args` に固定値を置く（host の core 数で決まる値を repo に焼く・N3 の向き）／変異検査を全便で直列化する（枠の話であって入れ子の話ではない・別便）。

## 23. 負荷下で落ちる歯 — 列の歯は壁時計を記録の pin に替え、refresh の停止経路の歯は起動未達を待たずに段を名指し zombie を残存に数えない（契約表の行 n・`s2-07l.417`）

- 何が起きているか（gate の log 5 便の実測 2026-09-16 11:3xZ〜13:5xZ・verified）: 走行 9〜11・load 5〜12 の下で全件 nextest の歯が落ち、`.389` / `.323` / `.360` を Gated FAIL（retire・実装喪失）に、`.188` を INCONCLUSIVE にした。(1) `tests/e2e/pipe/land.rs` の `pipe_order_regate_fail_leaves_the_queue`（195 s・panic 文 = 「上限まで待たない」）は `assert!(started.elapsed() < Duration::from_secs(30), ..)`＝「列に居ない便を待たない」を壁時計で測っており、負荷下では land 自体（rebase + 再 gate + 主実測）が 30 s を超えて偽に落ちる。待ちの唯一の記録は stdout の `order=` と面 5（`verdicts.jsonl`）の `order`（`pipe/queue.rs` の閉じた 4 値 `first` / `waited:<s>` / `degraded` / `unmeasured`）で、同じ歯の `order_token(&out) == "first"` が「待たなかった」を既に pin している（待ちの別 record は無い）。`pipe_order_pr_cmd_does_not_look_at_the_queue` も同じ壁時計を持つ。(2) `tests/e2e/fleet.rs` の `fleet_usage_refresh_timeout_stops_the_child_and_its_grandchild` は 5 便で落ち、所要は 69〜73 s。panic 文が残る便（`.360`）は `assert_gone` の「child: 子が起動に達しない（pid file が 60s で書かれない・停止経路の失敗ではない）」＝負荷下で偽 claude（包める host では `systemd-run` の scope 越し）が fixture の上限 `REFRESH_TIMEOUT_S`（4 s）までに `{spy}/child` を書けず、器が group を止めて `refresh=timeout` で返った**後**に `spy_line` が `PID_FILE_WAIT`（60 s）を空しく poll して落ちる。壁時計 bound（`assert_refresh_timeout`）ではない（bound は起動の段を測れない）。他 4 便は所要が同じ型（60 s の poll + 停止経路）＝同じ段（deduced・panic 文は log の末尾 20 行に入らない）。(3) 同じ family の `fleet_usage_refresh_timeout_unconfined_kills_the_child_that_ignores_term`（`.323`・8 s）は panic 文が残らず、8 s = 上限 4 + 猶予 2 × 2 の停止経路の全長＝`assert_gone` の `/proc/<pid>` 判定が KILL 後の zombie（回収されるまで `/proc` に残る・`fleet/wait.rs` の group 判定も zombie を数える）を「残った」と読む型が候補（inferred・本便の歯が段を名指せば次の周に確定する）。
- 形: (1) 列の歯 2 本の壁時計 assert を外す。「待たなかった」の pin は歯ごとに違う: 列を通る歯（`pipe_order_regate_fail_leaves_the_queue`）は既存の `order_token(&out) == "first"` に加えて面 5 の `exported_order(&state, &id) == "first"`（既存 helper・`waited:<s>` なら待った）で pin する。面 5 へ書かない pr_cmd の形の歯（`pipe_order_pr_cmd_does_not_look_at_the_queue`・歯の doc の逐語）は列を見ないので `order=` を出さないことの既存 pin（stdout に `order=` が無い）だけを残し、**面 5 の pin は置かない**（record を書かない面に空文字の pin を置いても RED を作れない＝空虚な歯）。(2) refresh の family（4 本・共通 helper）: (a) `spy_line` の起動待ちは**器が返る前**にだけ意味が在る＝run が返った後に pid file が無い周は `PID_FILE_WAIT` を待たず即落とし、文に段 `launch`・経過 2 値（起動から返るまで・返ってから判定まで＝helper が測る値・待たなかったことの証跡）・`REFRESH_TIMEOUT_S`・`/proc/loadavg` の 1 分値（provenance・C10）を載せる。(b) `assert_gone` は `/proc/<pid>/stat` の state `Z`（回収待ち）を「残っていない」に数え、`/proc` の消滅は猶予（`pipe.stop_grace_ms`・embedded manifest から読む・`refresh_stop_bound` と同じ読み方）まで poll する。落ちた周の文は段 `stop`・pid・state・経過。(c) fixture の上限 `REFRESH_TIMEOUT_S` を 4 → 15 に（`.249` の 1 → 4 と同じ型・fixture の manifest の値であって rules 行の裁定ではない）。`assert_refresh_timeout` の bound の式（`REFRESH_TIMEOUT_S + STOP_MARGIN_S + 2 × pipe.stop_grace_ms`）は不変で値だけ従属して伸びる。(d) `assert_refresh_timeout` は bound を超えた周の文に段 `return`・経過・上限・猶予を載せる。段の時刻から器の停止経路の欠陥が見えた周は本便で直さず notes に evidence を残し memo に切る。
- 触らない: 器の src・rules 行・並列度（user 直命）・fixture の socket と偽 claude の本文・`PID_FILE_WAIT` の値（返る前の待ちとしては不変）。
- 歯（`gate_flaky_bound_` 接頭辞・`tests/e2e/fleet.rs`）: (a) `gate_flaky_bound_launch_miss_is_reported_without_the_pid_wait` = pid を書かずに上限を超えて眠る偽 claude で `refresh=timeout` の後、起動の helper が段 `launch` の未達を返す（poll しない）。(b) `gate_flaky_bound_zombie_counts_as_gone` = 歯が起こして回収しない子（std の Command で `exit 0` の sh を spawn し wait しない）を `/proc/<pid>/stat` の `Z` で「残っていない」に数える（歯の側の pure な判定）。両方とも helper の変更と同じ file に在り base では該当 0 本（verify 行が rc 4 = RED）＝本体不変の歯だけの便として `// flip-check: retroactive s2-07l.417` の札（pipeline.md §5.3 の型・`.342` と同じ）。既存の歯 6 本（列 2 本・family 4 本）は改名しない。
- 却下: 壁時計の bound だけ伸ばす（起動の段を測れず、落ちた周の文が経過時間のまま）／`PID_FILE_WAIT` を伸ばす（返った後に待つ意味が無い）／歯を `#[ignore]`（列の順序と停止経路の pin を失う）／並列度を下げる（user 直命）／偽 claude が pid を書くまで器の上限を止める（器の src に歯の都合を入れる）。

## 24. host で同時に走る便の本数の最大値 — rules 行 `pipe.max_live` を受付が live な便の本数で撃つ（契約表の行 o・`s2-07l.398`）

- 何が起きているか（user 直命 2026-09-16 05:5xZ / 裁定 06:39Z・11:14Z・逐語は台帳 `s2-07l` notes・決定は [ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)）: 並列度を上げた周の実測（別 host・16 core）は load 18〜26・CPU 81 ℃で memory は 10 / 62 GB＝受付（§3.2）は memory の枠だけで本数を絞るので CPU と温度の逼迫が受付に映らない。user は、走っている便は止めず次に走らせる分から絞る → 同時本数の最大値を 1 つ器の規則として持つ・値は 16、と裁定した（裁定 id = user 2026-09-16T11:14Z・逐語は台帳・CON2）。暫定の上限は admin の launcher の変数と live を数え直す script（器の外・C2.2 / N2・ADR-0034 §1 が事故として挙げた型）に在り、器には無い。ADR-0034 の決定文と SRS FR68 の「数値上限を持たない」句は ADR-0035 が部分 supersede する（SRS の同句は user の /folio-architect の周）。
- 形: (1) rules 行 `pipe.max_live`（kind `PipeMaxLive`・Int・本）を manifest に足す。**本 doc の §3.1 の表（`docs/design/gate-cost.md`・行 o の write-set に在る）には行が既に載っている**（`.398` run 2 の INCONCLUSIVE の根＝表は rules-manifest.md §4 でなく本 doc の §3.1 で、足すのは manifest の行と `RuleKind` の variant・外形の pin）。**値は user 裁定で既に出ている**: **値 = 16・裁定 id = `user 2026-09-16T11:14Z`・裁定日 = 2026-09-16**（C5・上の「何が起きているか」の裁定・逐語は台帳・CON2）＝行の `ruling` と `ruled_at` にこの 2 つを入れる。規範の値を持つのは manifest だけで、ここの 16 は裁定の出所の記録である（C1）。連鎖は行 j〔[account-autonomy.md](./account-autonomy.md) §13〕と同型（ `RuleKind` の variant・Int の列・manifest の行・[rules-manifest.md](./rules-manifest.md) §4 の表・歯の kind 件数）。(2) 受付（`pipe/cli/intake.rs`・交差の判定 `exclude_overlap(` と同じ段・`--design` / 従来形の両方が通る同じ関数）が、交差と同じ live の判定（`crates/scribe2/src/pipe/cli/state.rs` の `live(`＝可視性 `pub(in crate::pipe)`・引数は置き場と便の id と段・戻り値は 3 値〔live / 終端 / 読めない〕の option・段の網羅 match）で state dir の live な便を数え、本数 ≥ 値の周は `Refuse` に足す variant 1 つ（live の本数と上限を運ぶ・slug `max-live`・stderr の 1 行 `pipe: max-live live=<n> cap=<c>`）で断る（run を作らず event を書かない・rc は既存の拒否と同じ 1）。live を読めない便が 1 つでも在れば交差と同じく `WriteSetUnreadable` 側（rc 2・fail-closed・NFR4）。数える順は交差の前。**短絡しない**: contract-source.md §21（`s2-07l.394`・受付の `judge` は各判定関数を全部撃って断りを列に積む＝preflight の一覧性）に合わせ、上限で断る周も交差の判定はそのまま撃ち、交差の組は列に並ぶ（上限の断りが先頭・交差は後続の行・上限で断る周に交差を並べても害は無い＝性能の話に留まる）。(3) 数えるのは便を作る前だけ＝走行中の便には効かず、`pipe resume` と追随の起こし直しは新しい便を作らないので数えない。(4) dispatcher の列の理由（[dispatcher.md](./dispatcher.md) §3 の閉じた型）に「上限で待つ」variant 1 つを足すのは行 a の Landed 後の別の行（本行は受付だけ）。(5) 一時的な引き下げは rules 行の値の改訂（裁定 id 付きの PR）でだけ行い、env・launcher の変数・host.toml から読まない（C1 / C2.2）。
- 触らない: 受付の memory の枠（§3.2）と `gate.mutants_jobs`・交差の判定 `overlaps`・`pipe run` / `pipe intake` の外形（usage）・段の enum・`Refuse` の既存 variant と rc の語彙。
- 歯（2 群・行の verify がそれぞれを撃つ）:
  1. **受付**（`pipe_intake_max_live_` 接頭辞・`crates/scribe2/tests/e2e/pipe/intake.rs`・toy repo・tmp の manifest を `--rules` で渡す・helper は `crates/scribe2/tests/e2e/pipe.rs` の既存の写しの書き手）: `pipe.max_live = 1` で live 1 本の下の 2 本目の intake が slug `max-live` と `live=1 cap=1` の 1 行で断られ、run dir も event も増えない／その live の便を `stop --run` で終端に倒すと同じ契約が通る／Gated で verdict FAIL の便は live に数えず上限 1 でも通る／写しを読めない live の便が在る周は `write-set-unreadable`（rc 2）で断る／上限で断る周も交差の組が列に並ぶ（短絡しない・(2) の末尾の枝）。
  2. **rules 行**（`rules_embedded_manifest_declares_max_live_` 接頭辞・`crates/scribe2/tests/e2e/rules.rs`）= 埋め込みの manifest が `pipe.max_live` の行を値・裁定 id・裁定日つきで持ち、kind の包含で**行と variant を対で足させる**（片方だけの manifest は parse できず、片方だけの enum は親 test が落とす・行 j と同型）。**外形**は既存の歯が受け、行の verify が完全名 `rules_external_form`（同 file・外形 snapshot `e2e__rules__rules_external_form.snap` の `rows=` / `kinds=` が 1 つ増える）で撃つ。
- 却下: ADR-0035 §3（写しは持たない）。

## 25. 一時 scope を終端で unload する — `--collect` と kill の後の `reset-failed`（契約表の行 p・`s2-07l.421`）

- 何が起きているか（別 project の planner の relay 2026-09-16・planner 再実測・verified）: host の `systemctl --user --failed` に器の一時 scope が 239 件残っていた（probe 85 / review 60 / common 48 / contract 14 / lens 11 / runner 10 / lens-claude 4 / toy 1）。sample の state は `ActiveState=failed SubState=failed Result=success`＝process は正常終了しているのに unit が failed で残る（transient scope は `--collect` が無いと終了後に unit を残す周がある）。同じ host の他 project の観察（failed unit 0）を汚す。現物: `pipe/confine.rs` の `scope_args` に `--collect` が無く、`release`（`systemctl --user kill --signal=SIGKILL`）は残った unit を `reset-failed` しない。probe（`sh -c exit 0` の scope）は起動結果だけ読む。
- 形: (1) `scope_args` に `--collect` を 1 語足す（systemd の `-G`・全部の scope に効く・終了後に unit を unload・失敗した周も）。(2) `release` は kill の後に `systemctl --user reset-failed <unit>.scope` を 1 回撃つ（unit が無い周の字面は `Gone` と同じ扱い・reset の rc は record に写さない＝`Released` の閉じた 4 値は不変）。(3) probe の scope は (1) で消える（後始末の口を足さない）。
- 触らない: 箱の大きさ（`MemoryMax` / `CPUWeight` / `OOMPolicy`）・`Released` の variant と `as_str`・record の `confined=` / `reason=` の語彙・封じ込めの 3 線。
- 歯（`confine_collect_` 接頭辞・`pipe/confine.rs` の in-file の pure な歯 + `tests/e2e/pipe/gate.rs`）: `scope_args` の列に `--collect` が 1 回在り既存の引数の順序が不変／偽 `systemctl` の呼出の写しに `kill` の後 `reset-failed` が 1 回在る／unit が無い周（`reset-failed` が「not loaded」の字面で断る）も `Gone` として record が変わらない。
- 却下: 定期の `reset-failed` を管理 tick に置く（掃除の 2 本目・原因の側を直さない）／`--collect` だけ（release で kill した周は failed のまま残る）／人が `systemctl --user reset-failed` を撃つ運用（散文の手順・N2）。

## 26. 1 便の時間と token を器が測る — verify record の段ごとの秒と、runner / lens の claude の usage を event に記す（契約表の行 q・`s2-07l.466` と行 r・`s2-07l.462`）

- 何が起きているか（planner の実測 2026-09-17・main deec0a9・verified）: user 裁定 2026-09-17T22:4xZ（逐語は台帳 `s2-07l.462`）で最大の bottleneck は 1 便あたりの時間と token と定まったが、器はどちらも測っていない。(1) 時間: `fleet/events.jsonl` の段の時刻から着地 37 便（09-16〜17）の内訳は gate 44%（`Implemented` → `Gated` の中央値 24 分・1 便あたり平均 2.3 周）/ 追随の再 gate 25%（main が `crates/` 等で進んだ周の再 gate 41 回・docs だけで進んだ周は §33〔`s2-07l.416`・09-17 06:09Z Landed〕が前周の PASS を引き継ぐので Landed 後の 11 回中 9 回は 0 分）/ 実装 18% / land 10% と読めるが、gate の**段別**（flip-check / nextest / clippy / xtask check / deny / 検出線 / 契約 verify）の秒は record（`verify.jsonl` / `verify-main.jsonl`・`step_record` の field = schema / n / rc / cmd / jobs / confined / peak_mb / kind / reason / slot / line …）に無い。(2) token: runner の claude は stream-json で起き（`headless/mod.rs` の `Call` の `streaming`・`headless/runner.rs` は result record の `subtype` / `is_error` / `result` の text だけを読む）、lens は text で起きる（`headless/lens.rs`・「最後の JSON 行」を判定に使う）ので、result record が運ぶ `usage`（`input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens`）と `num_turns` / `duration_ms` はどこにも残らない。管理席の wrapper が tee した raw の stream（器の外・107 session・52 bead）を planner が集計すると出力 3.65M token・cache 読み 706M token・cache 作成 13.6M token・turn 数は 1 bead あたり 134〜447＝費用は turn 数 × context の cache 読みが支配的で、便の焼き直し（1 bead 平均 2.06 session）がそのまま倍になる。憲法 C6.3（消費の記録は append-only の store 1 つ）と C6.2（便ごとの上限 R-C6-1）は便の token に配線が無く（manifest に R-C6 の行なし）、SRS は席の context（FR25 / FR26）と hook の 1 行（FR21 の who / what / when / bytes / tokens / wall）と lens の目標値（NFR1 = 0.15M token）を持つが便の消費の要件行は無い（SRS v0.17 の Phase F の材料＝user の `/folio-architect` の手番）。
- 形 (1) **段の秒**（行 q・S）: `run_line_captured` が process の起動から終了までの壁時計を測り `Fired` に秒を運び、`fire` が `Step` へ写し、`step_record` が record に `secs=` を足す（schema 1 のまま任意 field・ADR-0017 §2.1 の足し方・§5）。撃たなかった段（skip record）と write-set 照合（process を持たない・`unwrapped`）は秒を持たない＝field を欠く（0 と書かない・C10）。`pipe show` の record の行は `secs=` をそのまま写す（外形 snapshot `pipe_record_show_external_form` が動く＝同じ便で更新）。land の `verify-main.jsonl` は同じ `step_record` を通るので同時に秒を持つ。
- 形 (2) **claude の usage**（行 r・M）: 出所は claude の result record の `usage` object（4 値）と top-level の `num_turns` / `duration_ms`（`total_cost_usd` は CLI の見積＝派生値ゆえ運ばない・C10）。(a) runner: `headless/runner.rs` の `Watched` が result record を見る周（`is_result_record`）に usage を読み（入れ子の object を読む口は `find_key` / `top_level_string` と同じ深さの規則で `usage` の直下だけ・flat parser を使わない）、`conclude` の要約行に `usage=in:<n>,out:<n>,cache_read:<n>,cache_create:<n> turns=<n> wall_ms=<n>` を足す（読めない周は field を欠く・rc は変えない）。(b) lens: `Call` の `streaming`（bool）を**閉じた 3 値**（text / json / stream-json・名は実装が決める）に替え、lens は json（1 object・`result` の text と `usage` を持つ）で起こし、判定は従来どおり `result` の text の最後の JSON 行から読む。**読みの分岐は 1 つ**: stdout の最後の JSON object が `type` = `result` の封筒（claude の json 出力）ならその `result` の text の最後の JSON 行を判定に、封筒でなければその object をそのまま判定に読む（従来の text の形）＝偽 lens の fixture（`tests/e2e/pipe/gate.rs` / `intake.rs` / `land.rs` / `ratelimit.rs` / `spawn.rs` / `tests/e2e/pipe.rs` の 6 file・verified・裸の判定 JSON を 1 行出す）は不変で write-set に入れない。`Call` の構築点は 7 か所（`headless/mod.rs` の 4・`headless/runner.rs` の 1・`headless/lens.rs` の 1・`fleet/usage.rs` の 1・verified）で、型の変更ゆえ全部を新しい値に写す（`fleet/usage.rs` の refresh は text の値・runner は stream-json の値＝外形は不変でも file は触る）。lens は判定の JSON object に `usage` の 4 値と `turns` / `wall_ms` を足して stdout に写し、`pipe/gate/lens.rs` の読み手は `findings` / `population` と同じ形で `usage` を読む（**無くても INCONCLUSIVE にしない**＝古い lens・偽 claude の周は field を欠くだけ）。(c) 記録: `fleet/mod.rs` の `EventKind` に variant 1 つ（消費の 1 件・名は実装が決める・`as_str` の 1 腕・`replay` は段を変えない腕 1 つ）を足し、`Event` に `allowance` / `registration` と同じ形の任意 field 1 つ（閉じた型 Cost: 出所〔runner / lens / review の閉じた 3 値〕・token 4 値・turn・wall_ms）を足す＝C6.3 の「append-only の store 1 つ」は `fleet/events.jsonl` で、run dir には別 file を作らない。`Event` の struct は `fleet/event.rs` に在り（`EventKind` は `fleet/mod.rs`・どちらも行 r の write-set 内）、`Event` の field を足すと **`Event` の literal 構築点の全部**に 1 行足す（main 2f846df の実測・`Event {` の grep・verified）: src は `fleet/event.rs` 2 / `fleet/usage.rs` 2 / `pipe/mod.rs` 4 / `pipe/queue.rs` 4 / `pipe/dispatch.rs` 5 / `account/mod.rs` 1 / `seat/role.rs` 1 / `fleet/cli.rs` 1 / `hook/vessel.rs` 1、tests は `tests/e2e/fleet.rs` 13 / `tests/e2e/pipe/ratelimit.rs` 2 / `tests/e2e/seat.rs` 1 / `tests/e2e/ledger_memo.rs` 1（母集団 = 13 file・38 か所。`seat/state.rs` の `Event` は同名の別 enum、`pipe/stop.rs` の 1 か所は `..` の struct update なので触らない）＝行 r の write-set はこの 13 file を全部持つ（当初の census〔main e9add0f・13 file〕の後に `pipe/dispatch.rs`〔`s2-07l.366`〕・`hook/vessel.rs`〔`s2-07l.336`〕・`tests/e2e/ledger_memo.rs` が増えた 3 file を足した）。審査の周の口は `pipe/review.rs` の `review`（`headless/mod.rs` の `fill` で lens を起こし、判定を書く周に消費の 1 件を足す）で write-set 内。`s2-07l.545` が `pipe/review/judgement.rs` へ純移動した 14 item は判定の読み手（verdict / judgement の読み・未対応の照合・節の切り出し）だけで、headless の呼び出しも usage の読みも持たない＝write-set に要らない。書く口は `pipe/mod.rs` に **消費専用の 1 関数**を足す（`emit` の隣・`Emit` は触らない＝`Emit` の literal 構築点は src に 29 か所在り〔verified〕、欄を足すと閉包が全 file に広がる・消費の event は `Event` を直接組んで同じ store の append に渡す）。書く側は 3 か所: `pipe/spawn.rs` が runner の要約行を読んで `Implemented` / `Questioned` の event の前に 1 件、`pipe/gate.rs` が lens の verdict を書く周に 1 件、`pipe/review.rs` の審査の周に 1 件（review は lens と同じ `headless/lens.rs` の口）。**6 値（usage の 4 値と `turns` / `wall_ms`）のどれか 1 つでも欠けるか数でない周は、その出所の Cost を組まず event を書かない**（欠けを 0 に倒すと「測って 0」と「読めなかった」が 1 つの値に潰れる・C10・run 030606Z の Gated FAIL の根）。runner の要約行と lens / review の判定 object も 6 値が揃った周だけ `usage=` / `usage` を運び、揃わない周は field を欠く＝読み手は None に倒す。(d) 読む側: `pipe show --run` は消費の event を 1 行ずつ写し、`pipe report` の行に `cost: with_usage=<便数> out=<token> cache_read=<token> gate_secs=<秒の和>` の 1 行を足す（母集団 = 便数を同じ行に）。読む側の字面の pin は 2 系統（verified）: `pipe report` の行を逐語で持つ歯は `tests/e2e/pipe/spawn.rs` と `tests/e2e/pipe/ratelimit.rs`、`pipe show` の record の描画は形 (1) の外形 snapshot `pipe_record_show_external_form`（`tests/e2e/pipe/gate.rs`）＝行 r の write-set は両方の file と snapshot を持つ（消費の行が無い run の描画は不変でも、動く周に同じ便で更新する）。
- 触らない: 判定の意味（verdict の 3 値・rc）・runner の rate limit の読み（`decide`）・record の既存 field・event の schema 番号・rules 行（R-C6-1 は測れた後に C5 の裁定 id で足す別の行）・`fleet usage` の起動（text のまま）。
- 歯（`gate_secs_` 接頭辞 = 行 q / `run_cost_` 接頭辞 = 行 r）: 行 q = (a) in-file（`pipe/gate/verify.rs`）: 撃った段の `Step` が秒を持ち write-set 照合の `Step` は持たない／(b) e2e（`tests/e2e/pipe/gate.rs`）: gate の `verify.jsonl` の撃った record 全部に `secs=` が在り skip record には無い（母集団 = record 数を同じ assert に）・`pipe show` の外形 snapshot。行 r = (c) in-file（`headless/runner.rs`）: result record の 1 行から usage 4 値と turns / wall_ms を読む・`usage` が無い record は `None`・入れ子の `usage.iterations[]` の中の数に釣られない／(d) e2e（`tests/e2e/headless.rs`・偽 claude が usage 付きの result record を出す）: runner の要約行に `usage=` が載る・lens の判定 object に `usage` が載る／(e) e2e（`tests/e2e/pipe/spawn.rs` / `tests/e2e/pipe/gate.rs`）: 便 1 本で消費の event が runner 1 件 + lens 1 件（review を通す周は +1）書かれ token の値が偽 claude の出した数と一致・`pipe show --run` と `pipe report` の行（母集団 = event 数）／(f) 偽 claude が usage を出さない周は event を書かず gate の verdict は変わらない（fail-open ではなく「測れなかった」を field の不在で運ぶ・C10）。(g) **部分欠けと数でない値**（in-file `headless/runner.rs` と e2e `tests/e2e/headless.rs` / `tests/e2e/pipe/gate.rs`・同じ `run_cost_` 接頭辞）: result record が usage 4 値を持ち `num_turns` だけ欠く周と、`duration_ms` が数でない周の 2 分岐で、要約行に `usage=` が載らず・判定 object に `usage` が無く・event は 0 件・verdict と rc は不変。同じ歯の中で「6 値揃い＝1 件」を対に並べる（欠けを 0 に倒す実装は両分岐の event 数が同じ 1 になって落ちる＝0 を作らないことを測る）。
- 却下: run dir に usage.json を置く（C6.3 の store が 2 つになる）／`total_cost_usd` を記す（CLI の見積＝派生値・口座の種別で意味が変わる）／lens を stream-json にする（最後の JSON 行が claude の record になり判定が埋もれる・`headless/lens.rs` の実測 2026-09-10）／runner の要約行を parse せず run dir の `runner.stdout.log` から後で拾う（log は要約だけで record を持たない・verified）／R-C6-1 を同じ便で足す（値の裁定が先・C5）。

## 27. 主実測は着地する木が gate の木と同じなら全段を撃たない — record 1 本 `kind=main skipped=main tree=<sha>` で main-green にする（契約表の行 s・`s2-07l.464`・[ADR-0043](../../design-intent/decisions/ADR-0043-same-tree-main-check-is-one-record.html)）

- 何が起きているか（planner の実測 2026-09-17・main 12e64cc・verified）: land の主実測（`verify_main`・§5）は着地する木が gate の verdict の `tree` と同じ周でも ① write-set 照合・② 共通 verify・④ 契約 verify を撃ち直し、③ 検出線だけを `skipped=detection reason=same-tree` で省く。母集団 = `verify-main.jsonl` を持つ着地 39 便のうち理由を持つ 36 便: `same-tree` 27 / `outside-scope` 9。same-tree の 27 便で squash の commit 時刻から主実測の record の終端までの壁時計は中央値 14.9 分（p25 5.6 / p75 15.9 / 最大 30.6）＝③を省いた後に残る ①②④ の時間。着地 45 便の Gated(PASS) → Landed は中央値 10.4 分（p25 5.1 / p75 15.9）・1 便 126 分の約 1 割。木の sha が同じ＝gate が測った木と byte 単位で同一（content-addressed）ゆえ、①②④ は同じ木の同じ測定の重複で情報を足さない。
- 形（行 s・S・ADR-0043 §2.1 / §2.2）: `verify_main` は tmp worktree を切る**前に** `main_detection` と同じ比較（verdict の `tree` と `<new>^{tree}`）を読み、**同じ周は tmp worktree も verify の段も撃たず** `verify-main.jsonl` に record 1 本（`kind=main skipped=main tree=<sha> reason=same-tree`・schema 1 のまま既存 field の組だけ）を書いて緑の `MainCheck` を返す（finish へ進む・Landed の detail と stdout は不変）。record は `pipe/gate/record.rs` の `Skipped` に構築の口を 1 つ足す（段の閉じた値に主実測の 1 つを足す・理由は既存の `same-tree` を再利用・木は必ず持つ＝どの木の測定を再現と見なしたかを残す・C10）。一致しない周（`outside-scope` と面の内）と verdict に `tree` が無い周は従来どおり全段（③ は §5 の規則のまま）。候補の木（[pipeline.md](./pipeline.md) §40）の主実測も同じ 1 関数を通る＝候補の先端と着地の先端の木が同じ周は record 1 本。ADR-0021 §03 (C) が「untracked に依って通った便を main 実測が捕まえる」として全省略を却下した前提は、gate の前提検査（`precheck`・`status --porcelain` が空でなければ撃たない・verdict の `tree` はその木）が既に untracked と未 commit を断るので今は無い。残るのは ignore された file に依る周だけで、その面は main の CI（FR50）が持つ。
- 触らない: gate の段と順序（§5）・③ の省き方（`outside-scope` の面・`DETECTION_SCOPE`）・CAS と anchor 同期・`finish` と `Landed` の detail・`MainCheck` の 3 値と極性（`Unmeasurable` の周は不変）・`verify.jsonl` の record・record の schema 番号・行 q の `secs`（撃たない周は持たない＝skip record の規則のまま）・`pipe show` の読み（`skipped=` の非空で従来どおり拾う）。
- 置き場（`s2-07l.457` Landed a3e29b9 の後・verified）: 主実測の群（`verify_main` / `main_detection` / `record_main` / `materials` / `check_path` / `VERIFY_MAIN_FILE`）は `crates/scribe2/src/pipe/land/verify.rs`（215 行・[pipeline.md](./pipeline.md) 行 aj の純移動）に在り、`MainCheck` と呼び手 `land` は `land.rs` に残る（本便は `land.rs` を触らない＝呼び手の形は不変）。
- 歯（`pipe_main_same_tree_` 接頭辞・`tests/e2e/pipe/gate.rs`・`pipe_detection_scope_` の歯と同じ fixture〔tmp git repo + 偽 verdict.json + 撃った段を `added` に残す verify 行〕／in-file は `main_skip_record_` 接頭辞・`pipe/gate/record.rs`）: (a) verdict の `tree` と着地の木が同じ周は `verify-main.jsonl` が 1 行（key 列 = `schema` / `n` / `kind` / `skipped` / `tree` / `reason`・`kind=main skipped=main tree=<sha> reason=same-tree`・`n` は 1）で verify の cmd は 1 本も走らず（`added` が空）・Landed の detail と main の先端は従来の形／(b) `tree` が違う周は record が従来の段数で並び主実測の skip record は無い／(c) verdict に `tree` が無い周は全段撃つ（record の形は (b) と同じ）／(d) in-file: 主実測の skip record の字面が固定で、木の無い構築は口が取らない。**変更する既存の歯 2 本**（同じ file・§5 の same-tree の期待を持つ）: `pipe_detection_land_skips_detection_when_tree_matches` と `pipe_detection_scope_same_tree_records_reason` は「②④ だけを撃つ・③ の位置に `skipped=detection`」の期待を (a) の形へ写す（test diff だけを base に当てると base は `kind=main` を書かないので RED）。`outside-scope` と読めない周の歯（同 file）は不変。base は `verify_main` が木の比較の前に worktree を切って全段撃つので (a) も RED。**write-set の外の歯の走査**（verified・main a3e29b9・grep `verify-once` / `verify-red` / `verify-probe` / `verify-kill` / `main-red` / `main-unmeasured` の歯の所有者）: `tests/e2e/pipe/land.rs` の 9 本（`pipe_land_reruns_verify_on_main_and_fails_loud` / `pipe_land_reruns_common_verify_from_vessel_copy_on_main` / `pipe_land_already_landed_red_main_is_not_landed` / `pipe_land_turns_unstartable_verify_step_into_unmeasured` / `pipe_land_keeps_signal_killed_verify_line_as_red` / `pipe_land_reports_unmeasured_main_apart_from_red` / `pipe_retire_rebase_empty_refuses_other_failed_reasons` / `pipe_land_anchor_syncs_even_when_main_verify_is_red` / `pipe_land_anchor_before_verify_records_clean_anchor_during_main_check`）は gate を実際に通した便（verdict に `tree` が在る）を**同じ木のまま** land して主実測の赤 / 測れない / verify の副作用を期待する＝本便の後は主実測が走らず壊れる。これらは**期待を変えず fixture だけ**を「木が違う周」にする: land の前に verdict.json の `tree` を base の木に差し替える（`pipe_detection_scope_main_skips_detection_when_tree_differs_outside_scope` と同じ型・helper 1 本を `land.rs` の歯の隣に置く）＝主実測の経路の極性（赤 / unmeasured / anchor の順）を測る意味は不変で、「同じ木で main が初めて赤くなる」形は ADR-0043 §03 が引き受けた分。gate.rs の `verify-red` の歯 4 本は gate 側で不変。`verify-main.jsonl` の実在だけを見る歯（`pipe_land_already_landed_finishes_without_moving_main` ほか）は record 1 本でも緑のまま。
- 却下（ADR-0043 §03）: ① だけ残す（数秒だが経路が 2 本になり、木の一致で守れている diff を 2 度測る）／主実測を廃止し gate 後は常に着地（`outside-scope` の 9/36 = 木が違う周に main が未検査の木になる・C12.6）／主実測を Landed 後に非同期で撃つ（赤の周に main が赤のまま・C12.6）／②④ のうち clippy / deny だけ省く（手書きの選別・C2）。

## 28. 赤い行が在る周は、検出線が測れなくても FAIL に着く（契約表の行 t・`s2-07l.495`）

- 何が起きているか（実測 2026-09-20・verified）: 全体の歯が 1 本赤い便の gate が、FAIL（終端）でなく INCONCLUSIVE になった。検出線（変異の記録）は測る前に元の木の歯を全部走らせるので、**歯が赤い木では必ず rc 2（測れなかった）で終わる**。判定は「検出線の rc 2 は赤より先」（[pipeline.md](./pipeline.md) §5.3 の判定順・本 doc §5 の検出線の rc の読み・`s2-07l.331`）の順なので、歯が赤い便は全部 INCONCLUSIVE に倒れる。INCONCLUSIVE は終端でない＝便は live のまま残り、driver は抜け、同じ木を測り直しても赤いままで、自分の bead と write-set の重なる契約を塞ぎ続ける。
- やさしく言うと: 「test が落ちている」と分かっているのに、変異検査が動かなかったことを理由に「判定できず」と言って便が居座る。落ちているなら、落ちたと言って終わらせる。
- 形: 判定の順を 1 か所だけ入れ替える。**赤（今の数え方のまま＝rc≠0 の行・除くのは「検出線 ∧ rc 2」の 1 点だけで、検出線の rc 1 も赤）が 1 行でも在る周は、検出線が測れなかった周でも FAIL**（evidence は赤い行の数・今の FAIL と同じ字面）。検出線の rc 2 が INCONCLUSIVE に倒すのは、赤が 0 の周だけである（道具の都合で測れなかった便を終端させない、という `s2-07l.331` の理由はこの周にだけ当たる）。[pipeline.md](./pipeline.md) §5.3 の判定順と本 doc §5 の「rc 2 は赤に数えず INCONCLUSIVE へ倒す」は、この順で読み替える（同じ docs の便で、判定順の文・機械検証の③の文・本 doc §5 の 3 か所に pointer を足した）。
- 変えない順: diff の path を読めない周と、行が scope の中で殺された周は今までどおり赤より先に INCONCLUSIVE（どちらも「赤」が内容の赤か測れなさかを区別できない）。検出線の撃ち直し（§21）も不変で、撃ち直した後の値で上の順を読む。
- FAIL に着いた便は終端の列外（[dispatcher.md](./dispatcher.md) §2）に入り、契約の字を直すか `release` の印（同 §12）で列に戻る＝居座らない。
- 歯（`tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_red_wins_over_detection_`）: (a) 共通 verify が赤 ∧ 検出線が rc 2 の便は `Gated` verdict=FAIL で、evidence が赤い行の数を持ち、lens は呼ばれない／(b) 契約 verify が赤 ∧ 検出線が rc 2 の便も同じく FAIL／(c) 赤が 0 ∧ 検出線が rc 2 の便は今までどおり INCONCLUSIVE（既存の歯が測る側・変えない）。「変えない順」の 2 つと「検出線の rc 1 は赤」は既存の歯がそのまま測る（本行は足さない）。
- 触らない: **赤の数え方**（検出線の rc 1 も赤・除くのは「検出線 ∧ rc 2」の 1 点）・検出線の record の形・撃ち直しの回数・`verdict.json` の field・land の前提。

## 29. 審査役の出力が読めなかった周は同じ gate の中で lens を 1 回だけ撃ち直す（契約表の行 u・`s2-07l.495`）

やさしく言うと: 検証の行が全部緑でも、審査役の答えの中の「何行読んだか」の欄が数でないだけで、gate 全体が「判定できず」になって便が居座る。答えの形が崩れた周だけ、もう一度だけ聞き直す。

- 何が起きているか（実測 2026-09-20・verified）: 共通 verify も契約 verify も全部緑の便が、審査役の出力の母集団の欄（行数）が数でない字面（例 `~330`）だったために gate 全体 INCONCLUSIVE になった。INCONCLUSIVE は終端でないので便は live のまま居座り、driver は抜け、同じ木を測り直す口は gate 1 周（追随 → 再 gate ＝ workspace 全件 + 変異 + 審査役 1 回）を丸ごと払う——§28 と同じ居座りの型である。
- 現物（本行の base・verified）: `crates/scribe2/src/pipe/gate/lens.rs` の `parse_lens` は、最後の JSON object を読めない周・`verdict` が 3 値でない周・`findings` か `population` の key が無い周・集計の読みが `Err` の周（category の欠け・重複・表に無い名・母集団の不備）を**全部 1 本の `unjudged` に倒す**＝`Inconclusive` で集計を持たない戻りになる。母集団の 1 つの数の読み（`crates/scribe2/src/pipe/gate/findings.rs`）は、欄が無い周と数でない周を別の理由 1 行に分けて `Err` にする（数でない周の理由はその値を写す）。`crates/scribe2/src/pipe/gate.rs` の `decide` は審査役を**1 回だけ**撃ってその戻りをそのまま判定にする＝撃ち直しの口が無い。
- 撃ち直しの先例は §21（検出線が rc 2 で終えた周を同じ gate の中でその行だけ 1 回）で、`decide` は stderr へ写す行（`notes`）を既に引数で持ち、審査役の箱の unit 名は**試行の番号の欄を既に持つ**（いまは常に 1 番）。
- 形: 審査役の**出力は在るが形が読めなかった**周だけ、**同じ行・同じ本文・同じ箱の形**でもう 1 回撃ち、2 回目の戻りを採る（3 回目は撃たない・2 回目の箱の unit は 2 番）。
- 読めなさを**2 値に割る**（この行の唯一の新しい型）: 集計の読みが返す理由は、いま 1 本の文字列である。これを「形が読めない」（JSON でない・`verdict` が 3 値でない・key が無い・category が欠け / 重複 / 表に無い・件数や母集団の数が数でない）と「読めたが規則で断った」（母集団が 0＝審査役は読んでいない）に割り、**撃ち直すのは前者だけ**にする。後者は審査役が形どおりに答えた上での主張なので、同じ問いを 2 度出しても向きが変わらない。印は `parse_lens` の `Err` の分岐だけが立て（`Judged` の欄 1 つ・`unjudged` の既定は伏せた側）、`lens_outcome` が返す「箱の中で殺された」「rc が非 0」「起動できない」も**撃ち直さない**——どれも撃ち直しで向きが変わらず、箱の中の死は 2 回目も同じ費用を払って同じ死に方をする。
- 1 回目の理由は `notes` に 1 行（撃ち直した事実と 1 回目の理由）で残す＝stderr へ写り**判定は変えない**（`decide` が既に持つ口・口座の計測の行と同じ扱い）。record の field も `verdict.json` の schema も足さない（C10 = 撃ち直した事実を 0 に潰さない・C17.1）。2 回目も読めなければ今までどおり INCONCLUSIVE で、理由は 2 回目のものになる（1 回目は notes に残る）。
- **消すもの**（C17.2）: この class の回復に要っていた「人が 1 周ぶんの再 gate を撃つ」手（§28 と同じ居座りの型）。審査役の 2 回目 1 本は、再 gate 1 周（workspace 全件 + 変異 + 審査役）より小さい＝費用は減る側である（§2）。
- 触らない: `Verdict` の 3 値と rc・母集団の「0 は見ていない」の読み（C10・型として 0 を作らない側）・集計の 8 category・判定順（§5 / §28 / [pipeline.md](./pipeline.md) §5.3）・検出線の撃ち直し（§21）・`verdict.json` の field・審査役の行の組み立て（口座の付け足しと穴の埋め）・**契約の審査の段の審査役**（[contract-source.md](./contract-source.md) §4 の別の口・材料も判定の記録も別）。
- 却下案: 数でない母集団を判定と切り離して警告行に落とす（母集団の読みは「0 は見ていない」を**型で**持つ＝母集団を読めない PASS は findings 0 件の非空虚性を裏書きできず C10 を緩める側になり、0 は INCONCLUSIVE のままで数でない字面だけ通すと同じ不備の扱いが 2 本に割れる）／数の字面を緩めて近似の印を読み飛ばす（手書きの字面規則が増え、次の形で破れる・C1 / N2）／撃ち直しの回数を rules 行にする（§21 と同じ理由で 1 回で足りる＝2 回目も読めなければ負荷でなく審査役の側）／gate 全体を撃ち直す（1 周と同じ費用・§21 の却下案と同じ）／出力が読めない周を FAIL に倒す（測れなかったを赤に読み替える・C10 違反）。
- 歯（`pipe_gate_lens_reread_` 接頭辞・置き場は gate の歯の file・fixture の script は歯の中で書き、撃たれた回数を数える）: (a) 1 回目が数でない母集団・2 回目が正しい出力の審査役の便は `PASS` で終わり、撃たれた回数が 2・stderr に撃ち直しの 1 行（1 回目の理由つき）が在る／(b) 2 回とも数でない周は INCONCLUSIVE で回数が 2（3 回目は無い）・理由は 2 回目のもの／(c) **母集団が 0 の出力は撃ち直さない**（回数 1・INCONCLUSIVE・理由の字面は今までどおり）＝「読めたが規則で断った」側の pin／(d) rc が非 0 で終わる審査役と起動できない審査役は撃ち直さない（回数 1・stderr に撃ち直しの行が無い）／(e) 審査役が `INCONCLUSIVE` を**自分で**答えた周（集計は正しい）は撃ち直さない（回数 1・母集団 = 撃ち直さない 4 形）／(f) 母集団の欄が無い周と母集団が 0 の周の判定と理由の字面は 1 字も変わらない（既存の歯が測る側・行の verify がその 2 本を完全名で撃つ）。

## 30. e2e の歯の道具箱に偽 systemd-run を標準で置く — 歯が toy repo で実 binary を撃つ PATH の組み立てを 1 本に寄せ、偽の本体を 1 本に統一する（契約表の行 v・`s2-07l.504` の直しの層 (1)・歯だけの便）

やさしく言うと: 歯（test）が「子 process を箱に入れる道具」を本物のまま呼んでいたので、歯をたくさん並べて走らせると host の管理係が詰まり、machine ごと止まった。歯の道具箱に偽物を標準で入れ、本物を呼ばない形にする。

- **出所**（`s2-07l.504`・実測 2026-09-20）: 変異検査を持つ gate が 6 本同時に走った 4 時間半、開発 host がほぼ凍った（load 平均が 4 桁・D 状態の process が 5 万本超・10 分刻みの採取が抜ける）。詰まった先は user の systemd で、journal の 6 時間の scope の event は 36.5 万件、うち 99% 超が歯の toy 便と probe だった（実便の verify 行に帰属できた scope は 0 件）。
- **構造**（本行の根拠）: scope は slice 直下の**平面**にしか作れず、入れ子を持たない。ゆえに実 systemd-run を撃つ歯が起こす toy の process は、その歯を走らせている gate の箱（`MemoryMax`・§4）から**構造的に外れる**——いまの箱は歯の process を 1 本も数えていない。偽にすると toy の process は包まれず歯の process の子のまま走る＝gate の箱の中に留まる。scope の rate が歯から消えるだけでなく、箱の精度は**上がる**側である。
- **現物**（本行の base・verified）:
  - 器が包む口は src の 6 か所（`wrap_command(` が 1・`wrap_line(` が 5）で、いずれも PATH から systemd-run を解く（絶対 path を焼かない・C2.2）。包めた周は行の終端で `release_scope(` が systemctl を、argv の包みは走行中に `control_group_of(` が systemctl の show を撃つ＝**偽 systemd-run だけを置くと実 systemctl の呼出が残る**。
  - 歯が実 binary を撃つ口は 3 つである。(i) `run_pipe(`（`crates/scribe2/tests/e2e/pipe.rs`・呼出 183 か所・10 file）(ii) `run_bin(`（`crates/scribe2/tests/e2e/headless.rs`・呼出 11 か所）(iii) `Command::new(bin())` の直起動 64 か所のうち、包む subcommand を PATH を差し替えずに撃つ 6 か所（`crates/scribe2/tests/e2e/pipe/land.rs` 2・`crates/scribe2/tests/e2e/pipe/spawn.rs` 1・`crates/scribe2/tests/e2e/pipe/ratelimit.rs` 1・`crates/scribe2/tests/e2e/pipe/launch_failure.rs` 1・`crates/scribe2/tests/e2e/headless.rs` 1）。
  - (i) の 183 か所のうち 173 か所は argv に `--state-dir` を持つ。持たない 10 か所は usage の断りか「置き場が紐づいていない」の断りで、段に届かず run dir も event も作らない＝scope を 1 本も作れない。(ii) の 11 か所は argv に置き場を持たない（呼び手は fixture の dir を変数で持つ）。
  - PATH を差し替える口は `run_pipe_with_path(` の 17 か所だけである（`crates/scribe2/tests/e2e/pipe/gate.rs` 10・`crates/scribe2/tests/e2e/pipe/stop.rs` 4・`crates/scribe2/tests/e2e/pipe/spawn.rs` 2・`crates/scribe2/tests/e2e/pipe/launch_failure.rs` 1）。
  - 偽 systemd-run の script は **4 本**に重複している: `systemd_stub(`（記録は 1 起動 1 file・同名の 2 本目を実 systemd と同じ字面で断る）・`confined_path(`（1 file へ追記）・`terminal_confined_path(`（同形・別の file 名）・`peak_shims(`（記録を持たず偽 systemctl と対で置く）。`lean_path(` は逆向きで、systemd-run の**無い** host を作る。
  - 実 systemd-run を要る歯は **0 本**である。§7 が「歯で測れるのは引数まで」と置き、scope の外が死なないことと `memory.peak` が読めることは実 host の 1 回を契約の done に入れている（歯にしていない）。ゆえに「実物を使う」opt-in の口は作らない。
  - 歯の本数（`#[test]` の実測）: gate の歯 122・intake の歯 117・land の歯 111・spawn の歯 67・dispatch の歯 66・pipe の歯 23・ratelimit の歯 22・stop の歯 17・launch_failure の歯 5。
- **既存の assert が動かない根拠**: 包めた / 包めないは host で既に割れている——CI の runner は user の session manager を持たないので `confined=false`、開発 host は実 systemd-run が在るので `confined=true` で、main はどちらでも緑である。本行はその 2 状態を「偽で包めた」1 つに固定するだけなので、`confined=` や `reason=` の値に依る assert は base に在り得ない（在れば main が片方の host で赤い）。開発 host の側は値が動かず、本行で挙動が動くのは CI の側だけである。
- **約束**（番号は done と歯に 1:1 で対応する）:
  1. **道具箱は 1 本**: 歯が toy repo で実 binary を撃つときの PATH の組み立ては `crates/scribe2/tests/e2e/main.rs` の 1 関数（`make_tmp_dir(` の隣・全 module から見える可視性）に寄り、偽 systemd-run と偽 systemctl を置いた dir を先頭に積んだ PATH の値を返す。host の PATH は後ろに残る（git / sh / cargo の解決は不変）。
  2. **3 つの口が全部そこを通る**: (i) は撃つ argv から `--state-dir` の値を読んでその下に道具箱を置く（値を持たない 10 か所は段に届かないので host の PATH のまま撃つ）。(ii) は呼び手が既に持つ fixture の dir を引数で受けて置く。(iii) の 6 か所は (i) か (ii) を通る形に替える。**既存の歯の総数は不変で、既存の `#[test]` の中の assert は 1 字も動かない**（増えるのは下の「歯」の道具箱の歯〔接頭辞 `e2e_toolbox_`・(a)〜(d)〕だけで、既存側で動くのは口の実装と (ii) の呼出の引数 1 つだけ）。
  3. **偽の本体は 1 本**: 4 本に重複した script は 1 つの生成関数から出る。本体は `systemd_stub(` の形（`--unit=` を読み、同名の 2 本目を実 systemd と同じ字面で断り、argv を 1 起動 1 file で記録 dir へ写し、`--` の後ろを exec する）。`confined_path(` / `terminal_confined_path(` / `peak_shims(` はその生成関数を呼び、記録の読み手（`runner_was_confined(` と `spawn_confined(` の前提）は 1 file の追記から記録 dir の走査へ揃う——`scope_record(` と同じ形で、母集団の件数を出してから 1 件を取る。
  4. **偽 systemctl も既定**: 道具箱の systemctl は kill に「もう無い」の字面（→ `Released::Gone`）・show に空（→ peak は読まない）を返す。偽が作らなかった unit に実 host が返す答えと同じなので、record の field は増えも減りもしない（`Released::Gone` は行に `scope=` を書かない・§4.4）。
  5. **逃がしは残る**: `lean_path(` の「systemd-run の無い host」と `run_pipe_with_path(` の明示の口は不変で、`Reason::NoTool` の縮退（FR46）を測る歯は base のまま緑である。実物を使う opt-in は作らない（要る歯が 0 本ゆえ・作れば「本物を撃ってよい口」が 1 つ残る）。
- **歯**（接頭辞 `e2e_toolbox_`・置き場は行 v の write-set の pipe の歯の file と headless の歯の file）:
  (a) 約束 1 と 2(i): `run_pipe(` で toy repo の gate を 1 本撃つと、道具箱の記録 dir に共通 verify の scope の記録が在り、その引数に `--scope` と `MemoryMax=` が在る（母集団 = 記録 dir の全件を同じ assert に出す）。
  (b) 約束 2(iii): 直起動の 6 か所と同じ形（子として背景で起こす便）で撃った周も同じ記録が残る。
  (c) 約束 2(ii): `run_bin(` で lens を 1 回撃つと claude の scope の記録が残る。
  (d) 約束 5 の否定の枝: `lean_path(` の PATH で同じ gate を撃つ周は記録が 1 件も増えず、record は `confined=false reason=no-systemd-run` のままである。
  既存の歯が測る側（本行は足さない・行の verify が完全名か接頭辞で撃つ）: 同じ名の 2 本目を断る性質（約束 3）・`Released::Gone` の周が行に `scope=` を書かないこと（約束 4）・包めない host の縮退（約束 5）・claude の peak の読み・停止起因の終端の理由 5 本・`--state-dir` を持たない口が置き場を作らないこと。
- **flip-check**（歯だけの便）: src を 1 行も触らないので「test 区間を base に当てて RED」の入口は成立しない。`// flip-check: retroactive s2-07l.504` の札を、**その便で test 区間が動いた file の行頭**に置く（効く 4 条件 = test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない）。札は HEAD から読まれるので、**commit してから** flip-check を撃つ（未 commit の作業木では効かない）。判定行の `retroactive=N` は planner review の対象で、notes に変異の proof を残す。接頭辞 `e2e_toolbox_` は base に 0 本なので、行の 1 本目の verify は base で「該当 0 本」＝RED、HEAD で緑になる。
- **write-set の 0 行の file**: `crates/scribe2/tests/e2e/pipe/intake.rs` は 1 行も変えない（diff 0 行）。在る理由は、行の verify が「`--state-dir` を持たない口が置き場を作らない」歯を完全名で撃ち、その歯の置き場がこの file だからである。残る 10 file のうち歯の 9 file は全部に diff が在り、11 file 目の `docs/design/gate-cost.md` は現物との差を errata の小節（§30.1・規範は本節のまま）として書く置き場で、差が無い周は diff 0 行でよい。(i) の 183 か所の呼出は 10 file に散るが**呼出の字面は変えない**（PATH の組み立ては共有 module の 1 関数の中）ので、呼出しか持たない file（`crates/scribe2/tests/e2e/pipe/dispatch.rs` 等）は write-set に要らない。
- **触らない**: 器の src（`wrap_command(` / `wrap_line(` / `probe(` / `release_scope(` / `control_group_of(` と `Reason` の 8 値・`Released` の 4 値・`Confinement` の 2 値・record の field）・箱の大きさの式と rules 行（§4.2）・極性一覧（§4.5・封じ込めは guard ではない）・`run_pipe_with_path(` の口と `lean_path(`・`systemd_stub(` の記録の dir 名と `scope_record(` の読み（gate の歯の母集団が動かない）・`peak_shims(` の偽 systemctl の答えが歯ごとに変わる形（本行が揃えるのは systemd-run の側だけ）・既存の歯の総数・既存の `#[test]` の中の assert（増えるのは接頭辞 `e2e_toolbox_` の歯だけ）。
- **却下案**:
  - **道具箱を process ごとの静的な置き場に持つ**（口が argv も引数も読まずに済む）: 片付ける手が無く、歯 1 本ごとに dir が 1 つ残る——`/tmp` の fixture が 5.8 万 dir に育った `s2-07l.343` と同じ型を作る。fixture の dir は呼び手が既に持っているので、そこへ置く。
  - **183 か所の呼出を全部 PATH つきの口へ書き換える**: 呼出の字面が 183 か所動く割に、**後から書かれる呼出**を守らない（口の既定にすれば新しい呼出も自動で通る）。
  - **偽 systemd-run だけ置いて systemctl は実物のまま**: 包めた周は行ごとに kill が、argv の包みは show が実 D-Bus を往復する＝詰まる先を scope の作成から unit の照会へ移すだけである。
  - **実 systemd-run を使う opt-in の口を残す**: 要る歯が 0 本（§7・箱の実測は契約の done 側）なので、口だけが残って再び使われる。要る歯が出た周に、その歯と一緒に作る。
  - **歯の同時本数を絞って実物のまま使う**（nextest の test-group）: host 全体の同時 gate 本数は器が知らない（行 b は tmux の歯を絞る別の面）。1 本の gate の中で絞っても 6 本同時の周は同じ積になる。scope の rate を歯から**消す**ほうが強い。
  - **全部を包めない host にする**（PATH から systemd-run を外す＝`lean_path(` に揃える）: 包めた周の経路（箱の引数・包みの終端行・終端の片付け）が歯から丸ごと消え、CI でも開発 host でも包みの経路が測られなくなる。

### 30.1 errata（現物との差・`s2-07l.504.1`・規範は上の §30 のまま）

- **道具箱の置き場の leaf 名**: 偽 binary は `<置き場>/toolbox-bin`・argv の記録は `<置き場>/toolbox-scope-args` である。明示の口（`systemd_stub(`）の `systemd-bin` / `scope-args` と**名を分ける**——同じ名に重ねると、明示の口で撃つ歯の母集団（`scope_record(` の「ちょうど 1 件」）に既定の口の起動まで混ざり、既存の assert が動く。
- **(ii) の置き場は plugin の root ではない**: runner の口は root の配下の dir を 1 つずつ `--plugin-dir` へ渡す（設計 pipeline.md §6）ので、root に道具箱を置くと dir 2 本が plugin に化け、「配下の dir を名前順に渡す」歯と「配下 0 の root は rc 2」の歯が両方落ちる。ゆえに (ii) は **dir を引数で受ける**形にし、runner の呼出は呼び手が既に持つ worktree を、lens の呼出は契約の置き場を渡す。約束 2(ii) の「呼び手が既に持つ fixture の dir」はこの 2 つである。
- **(iii) の 6 か所が通る形**: 起動を返す口を 2 つ置いた——`pipe_cmd(`（口 (i)・撃つ argv の `--state-dir` の値から）と `bin_cmd_with_toolbox(`（口 (ii)・引数の dir から）。背景で起こす便・pane の env を足す周・cwd を後置する周は、この起動に `Stdio` や `env` を足して使う。
- **偽 `systemctl` の `kill` の字面**: 実 systemctl の `Failed to kill unit %s: Unit %s not loaded.` を argv の 4 語目（`<unit>.scope`）で埋める。読み手が見るのは `not loaded` の字面だけ（`Released::Gone`・§4.4）である。
- **歯の本数の実測**: `cargo nextest run -p scribe2 --test e2e` は 986 本 → **990 本**（増えたのは `e2e_toolbox_` の (a)〜(d) の 4 本だけ・既存の歯の総数と assert は不変）。

## 31. 受付に CPU の次元を足す — 枠を memory の 2 項と core の 1 項の min にし、job ごとの thread は器が決めて穴で行へ渡す（契約表の行 w・`s2-07l.504`）

やさしく言うと: いままで「空いている memory」だけを見て変異検査を何本走らせるか決めていた。core の数も見て、host 全体で同時に走る test の thread が core を超えるところで止める。混んでいて 1 本に落としたときは、その 1 本が core を全部使わないよう thread も 1 にする。

- **何が起きているか**（事故の実測 2026-09-20・台帳 `s2-07l.504`・verified）: host が 4.5 時間ほぼ凍った。load の 1 分値は 8 → 19,459 → 68,816、D 状態（待ち）の process は最大 52,412、便の event は 4 時間 1 件も出ていない。**memory は余裕**（使用 30%・OOM 0 件）で、溢れたのは core の側である。同時刻に変異検査を持つ gate が 6 本同時に走っていた。
- **受付が CPU を見ていない**（verified・main b2cf656）: `crates/scribe2/src/pipe/admission.rs` の `capacity`（pub・pure）は `by_avail` と `by_token` の min で、どちらも memory の式である。事故の host は `by_token` が 39 job ぶんを許したので、6 gate × 4 job = 24 枠が全部通った。
- **入れ子の上限は gate 1 本の中でしか閉じない**: 行 m（§22）が入れた thread の上限は `crates/xtask/src/mutantsdiff.rs` の `test_threads`（`pub use scope::test_threads` で公開・**max(1, cores / jobs)**）で、`jobs × t ≤ cores` を**その gate の中だけ**で閉じる。core 32 の host では gate 1 本（jobs 4・t 8）で 32 thread＝それだけで core が満杯になり、6 本同時はその 6 倍である。
- **縮退した gate ほど core を広く使う**: 待ちの上限（rules 行 `gate.slot_wait_s`）を超えた周は `admission.rs` の `degraded` が jobs 1 で進むが、xtask 側の導出は `cores / 1` ＝ **core 数ぶんの thread** になる。枠を配れないほど混んだ host で、いちばん太い行を撃つ形である。
- **層 1 との関係**: §30（行 v）は歯が実 systemd-run を撃つ rate を消す層で、scope の作成が詰まる面を閉じる。本節は**同じ事故の別の面**（core の勘定が受付に無い）で、歯の scope が 0 になっても 6 本の変異検査が core を 6 倍に使う形は残る＝2 つは重ならない。
- **前提（ADR が先）**: §2 は「memory だけが硬い資源・CPU は溢れても遅くなるだけなので上限を持たない」と書き、ADR-0021 §2.1 / §2.3 がその面の正本である。事故は「CPU は遅くなるだけ」が偽であることを示した（D 状態の山が session manager を飽和させ、host ごと 4.5 時間止まった）。本行と §32 はその面を置き換えるので、**実装の前に ADR を 1 本 land する**（CLAUDE.md の「ADR を書く条件」1 / 4・行 o が ADR-0035 を要したのと同型）。その ADR は [ADR-0050](../../design-intent/decisions/ADR-0050-cores-and-scope-creation-are-hard-resources.html)（硬い資源を memory と core と scope の作成の 3 つに読み替え、ADR-0021 §2.1 / §2.2 / §2.3 / §2.7 を部分 supersede する）で、行 w と行 x の実装はその land を前提とする。本節は形だけを決め、値と原則の改訂は ADR と裁定が持つ。
- **現物**（verified・main b2cf656）: `crates/scribe2/src/pipe/admission.rs` = `capacity(meminfo, sizes, live_jobs) -> Free`（pub・pure）/ `Sizes { job_mb, reserve_mb }`（pub）/ `Free::{Slots, Unmeasured}`（pub）/ `Rules { sizes, cap, wait_s, policy }`（pub）/ `Unreadable::{SlotsDir, Lock, Meminfo}`（pub・`as_str` が record の `slot_why=`）/ `Grant { jobs, detail, why, ticket }`（pub・`jobs` は 1 以上）/ `has_room(dir, want, sizes) -> bool`（pub）/ `admit`（pub）/ `degraded` / `unmeasured` / `take` / `Ask::{UpTo, Floor}`（private）。`crates/scribe2/src/pipe/gate/verify.rs` = `fire` / `fill_holes(line, base, jobs)` / `admitted`（どれも private）と `UNADMITTED_JOBS`。`crates/scribe2/src/pipe/declaration.rs` = `BASE_HOLE` / `JOBS_HOLE` / `BASE_HOLES`（pub・**閉じた 2 つ**・intake の `unfit` と gate の置換が同じ列を読む）。`crates/scribe2/src/fleet/wait.rs` = `Completion::SlotFree { slots_dir, want, job_mb, reserve_mb, cap }`（pub）で、待ちの観測が `has_room` を撃つ。`.vessel.toml` の `detection-verify` は 1 行で穴が 2 つ。`crates/xtask/src/mutantsdiff.rs` = `measure_args(diff, out, scope, jobs, cores)`（pub・pure）が末尾に `-- --no-fail-fast -- --test-threads <t>` を置き、`run` が `std::thread::available_parallelism()` を 1 回だけ読む。
- **約束**:
  1. **job 1 つの thread の値段を器が決める**: 受付が pure 関数 1 本で `price = max(1, floor(cores / gate.mutants_jobs))` を出す。`cores` は `std::thread::available_parallelism()` の**実測**で、env を読まない（C2.2・器の `env::` の許し列は `cargo xtask check` の `env-reads` が母集団ごと数える面であって、増やす便ではない）。新しい rules 行は足さない（C1 / C5）。
  2. **枠は 3 項の min**: `by_cpu = floor(cores / price) − Σ 生きている札の jobs`（引き算は 0 の床）を pure 関数 1 本で出し、配る枠を **min(by_avail, by_token, by_cpu)** にする。**`capacity` の引数と式は変えない**（memory の 2 項はそのまま・既存の歯が 1 字も変わらずに通る）。
  3. **札の形は変えない**: `by_cpu` の単位は job で、1 job の値段が `price` thread である。生きている札の jobs の和がそのまま CPU の勘定になるので、札 file の schema も回収の判定（pid と起動時刻）も不変である。
  4. **縮退と測れない周は thread も 1**: 受け付けた枠（`Grant`）が jobs と並べて thread を運び、測れた周は `price`、待ちの上限を超えた周と測れなかった周は **1** にする（`cores / 1` をやめる＝速い側へ倒さない）。
  5. **cores を読めない周は測れなかった側へ倒す**: 受付の閉じた理由（record の `slot_why=`）に variant 1 つを足し、jobs 1 / thread 1 で進む（断らない・止めない・FR46）。0 と書かない（C10）。
  6. **thread は穴で行へ渡す**: 宣言の置ける穴の閉じた集合に 3 つ目を足し、`.vessel.toml` の検出線をその穴を持つ 1 行にする。置換は `{jobs}` と同じ関数 1 本が行い、受付を通らない行（land の主実測）は jobs と同じく 1 を埋める。**穴の列と宣言の行は同じ便で land する**（片側だけに足すと、intake を通った行が穴のまま撃たれる・§3.3 errata と同じ理由）。
  7. **導出の正本は器の 1 か所**: xtask は受けた値を `-- -- --test-threads <値>` へそのまま渡すだけにし、cores から thread を導く計算を持たない（`--jobs` と同じ扱い・§3.3「値は持たない」）。渡されない周と数でない周の既定は **1**（`JOBS_FLOOR` と同じ向き）。引数の対と `--` が 2 つの形（§22）は不変。
  8. **待ちも 3 項で測る**: 枠が空くのを待つ完了 enum が CPU の材料も運び、待ちの中の観測が受付と同じ 3 項を測る。memory だけで待ちが解ける形を残さない（解けた直後に受付が 0 を出して待ち直す空回りになる）。
- **歯**:
  - in-file（`crates/scribe2/src/pipe/admission.rs`・接頭辞 `admission_cpu_`）: (a) 値段が **max(1, floor(cores / cap))** で、cap 0 と cores 不明の 2 形を弁別する／(b) `by_cpu` の式（floor・生きている札の差引・0 の床）／(c) 3 項の min が **CPU 側で決まる** fixture（memory は潤沢・core が細い）と **memory 側で決まる** fixture の両方で正しい（どちらか一方だけの歯は、min の項を落としても緑になる）／(d) cores を読めない周は閉じた理由で「測れなかった」に倒れ、その字面が固定である。
  - in-file（`crates/scribe2/src/pipe/declaration.rs`・接頭辞 `declaration_threads_hole_`）: 穴の閉じた集合が 3 つちょうどで、3 つ目を持つ検出線の行は intake を通り、集合の外の穴は従来どおり不適合に落ちる。
  - in-file（`crates/xtask/src/main.rs`・接頭辞 `mutants_diff_threads_flag_`・行 m と同じ置き場）: 引数の末尾が `-- --no-fail-fast -- --test-threads <受けた値>` で、渡さない周と数でない周は 1、`--jobs` / `--in-diff` / `-p` / `-o` / `--no-shuffle` / `--copy-vcs` の対と順序は不変。
  - e2e（`crates/scribe2/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_slots_threads_`・既存の受付の歯と同じ toy の形）: 3 つの穴を持つ行の置換後の `cmd` に実効 jobs と実効 thread が**両方**載り、**待ちの上限を超えた周の `cmd` は jobs も thread も 1** である（`slot=degraded` の record と対で 1 本）。
  - **不変の柵**（行の verify が完全名で撃つ・本便は書き換えない）: `admission_capacity_takes_the_min_of_the_two_formulas` / `admission_capacity_floors_at_zero` / `admission_capacity_is_unmeasured_on_unreadable_meminfo`（置き場は `crates/scribe2/src/pipe/admission.rs`）。memory の 2 項の式が 1 字も動かないことを測る側である。
- **触らない**: `capacity` の引数と式・`Sizes` の 2 線・札 file の schema と回収の判定・`gate.mutants_jobs` と `gate.job_memory_mb` と `host.reserve_memory_mb` の値・`gate.slot_wait_s` の値・`pipe.max_live`（行 o）・封じ込めの箱の大きさと `Limit` の 2 種・検出線の rc の意味と撃ち直し（§21）・歯の道具箱（§30・行 v の面）・受付が極性一覧に載らないこと（§3.2 の末尾・縮退するだけの境界のまま）。
- **却下**:
  - **`capacity` に cores の引数を足して 3 項を 1 本の式にする**: 既存の 3 本の歯が全部書き換えになり、memory の式が不変であることを測る側を同じ便で失う。
  - **thread を `--jobs` から道具が導き続ける（穴を足さない）**: 受け付けた枠が上限より小さい周に **floor(cores / jobs)** が job あたりの値段を押し上げ、2 本の gate が合わせて core の 2 倍の thread を作る（core 32・各 jobs 2 なら 2 × 16 が 2 本で 64）。縮退の周は 1 本で core 数ぶんになる（本節の出所そのもの）。
  - **thread を rules 行にする**: cores は host ごとに違うので、tracked の manifest に焼くと host ごとに裁定が要る（値の線が増える・N3 の向き）。
  - **cores を env から読む**: C2.2。`available_parallelism` は env でなく host の面を読む口である。
  - **CPU を上限（宣言値の rules 行）にする**: 本便が足すのは**枠の分母**（core 数という測定値）であって新しい宣言値ではない。上限の是非は ADR と裁定の側に残す。
  - **§30（行 v）で足りるとする**: 歯の scope が 0 になっても、変異検査そのものが core を 6 倍に使う形は残る（事故の load は歯の scope だけでは説明が付かない＝memory は余っていた）。
  - **混んだ周に便を断る**: §10 のとおり断らない（縮退する・FR46）。

### 31.1 errata（現物との差・`s2-07l.504.2`・規範は上の §31 のまま）

- **3 つ目の穴の字面は `{threads}`**（declaration.rs の閉じた集合の末尾・intake の `unfit` と gate の置換 `fill_holes` が同じ 3 つを読む）。scribe2 自身の検出線は `cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads}` の 1 行で、xtask の口は **`--threads <t>`**（`--jobs` と同じ読み方・渡されない / 数でない / 0 は 1）。道具の側の `cores / jobs` の導出（§22 の `test_threads`）は消えた。
- **CPU の材料は受付の型 `Cpu { cores, price }`**（測定値と導出値を対で持つ・C10）。値段は `Cpu::priced(cores, cap)` の 1 本（**max(1, floor(cores / cap))**・cap 0 は 1 で割る）で、cores を読めない周は材料を組めない（`cpu_of` が `None`・0 や 1 に潰さない）。cores は **1 受付で 1 回だけ**読む（`admit` の入口・待ちの観測も同じ値を運ぶ・meminfo と違い受付の間に動かない）。
- **3 項の min は `room(meminfo, sizes, cpu, live_jobs)`**（`capacity` の結果に `by_cpu(cpu, live_jobs)` を重ねる・`capacity` の引数と式は不変・既存の歯 3 本は 1 字も変えていない）。
- **測れなかった理由の 4 つ目は `Unreadable::Cores`**（record の `slot_why=cores`・§3.2.1 の 3 値に足す）。理由の判定順は meminfo → cores → 札の回収で、どちらも回収の前に返る（回収の数を残す前に縮退する側）。
- **`Grant` は `jobs` と `threads` を対で運ぶ**: 配れた周は `(min(cap, free), price)`、縮退（`degraded`）と測れない（`unmeasured`）は `(1, 1)`。受付を通らない行（land の主実測・`run_checks` の受付なしの形）は gate/verify.rs の `UNADMITTED_THREADS`（= `UNADMITTED_JOBS` = 1）を埋める。record に `threads=` の field は**足していない**（record の読み書きは本行の write-set の外・置換後の `cmd` が実効 thread を持つ）。
- **`Completion::SlotFree` は `cores` を 1 つ足して運ぶ**（`{ slots_dir, want, job_mb, reserve_mb, cap, cores }`）。観測は `admission::has_room_on(dir, want, sizes, Cpu::priced(cores, cap))` で受付と同じ `room` を読む。
- **列の起動前の余地の検査（pipe/dispatch.rs）は memory の 2 項のまま**である。その呼び手は本行の write-set の外で、CPU の材料（cap）も持たないため、`has_room(dir, want, sizes)` の口を memory だけの形で残し、待ちの観測（3 項）とは `has_room_on` で分けた（実装は `observe` の 1 本・CPU の材料の有無で 2 項 / 3 項が決まる）。列にも 3 項を読ませるかは後続。
- **e2e の歯**は `sh verify-slot.sh {jobs} {threads}` の toy の行で、撃たれた側が `$1` / `$2` を別 file に写す（record の字面だけの置換でないことを測る）。枠を配れた周の thread はその歯が同じ host で測った **max(1, floor(cores / gate.mutants_jobs))** と一致し、縮退の周は cmd が `… 1 1`。

## 32. 器の健康の遮断器 — 行を撃つ前に走行可能と待ちの process を読み、混んだ周は空くまで待ち、待てなかった周は終端させない（契約表の行 x・`s2-07l.504`）

やさしく言うと: host が息をしていないときに検証を撃ち続けない。走れる process の数と、待たされている process の数を先に見て、多すぎたら空くまで待つ。待っても空かなければ「判定できなかった」で止める。「落ちた」ではないので、あとで測り直せる。

- **何が起きているか**（同じ事故・台帳 `s2-07l.504`・verified）: 凍結の 4.5 時間、器は撃った行が返らないまま走り続け、便の event を 1 件も出していない。load の 1 分値 68,816・D 状態の process 最大 52,412 に対し、**器は host の健康を 1 度も読まない**。混んだ host でも同じ勢いで行を撃ち、負荷で落ちた歯をそのまま赤に数える（§23 が壁時計の歯で踏んだのと同じ面）。台帳の notes は直しの層 (3) にこの遮断器を挙げ、待ちの上限を超えた周は **FAIL でなく INCONCLUSIVE** にせよと記す（凍った host の下で便を終端させると、実装の成果ごと列から外れる）。
- **現物**（verified・main b2cf656）: `crates/scribe2/src/pipe/gate/verify.rs` = `fire`（private・gate も land の主実測も通る**唯一の**起こし口）/ `run_line_captured`（pub）/ `Step`（pub・record の欄を運ぶ）/ `Checks<'a>`（pub・`worktree` / `base` / `contract` / `common` / `detection` の 5 欄）。`crates/scribe2/src/pipe/gate/record.rs` = `record_verify`（pub(super)）が `Checks` を組み `Counted { red, unreadable, killed, detection_unmeasured }`（pub(super)）を返す。`crates/scribe2/src/pipe/land/verify.rs` = 主実測が 2 つ目の `Checks` を組む。`crates/scribe2/src/pipe/gate.rs` = `Limits`（pub・6 欄）/ `decide`（private・判定順は diff が読めない → 箱の中で死んだ → 赤 → 検出線の rc 2 → 予算 → lens の本数）/ `inconclusive`（private）。`crates/scribe2/src/pipe/cli/step.rs` = `limits_of`（private・6 行を `--rules` の manifest から読む）で、`Limits` の literal 構築点は 2 つ（`crates/scribe2/src/pipe/cli/step.rs` と `crates/scribe2/src/pipe/queue.rs` の in-file の歯）。`crates/scribe2/src/fleet/wait.rs` = `Completion`（pub・7 variant）で、pid を見張らない 4 つは `pid()` が 0 を返す。`crates/scribe2/src/polarity.rs` = `Guard`（pub・24 variant）と `ALL` と 3 つの網羅 match。host の面: 走行可能な process 数は `/proc/loadavg` の 4 番目の欄の**分子**、待ちの process 数は `/proc/stat` の `procs_blocked` の行で、器がこの 2 面を読む口は無い（`/proc` を読むのは受付と封じ込めの memory の 2 か所だけ）。
- **前提**: §31 と同じ ADR（§2 の「CPU は上限を持たない」の面）に乗る。閾値 2 つの**値は user 裁定で既に出ている**（約束 2）。
- **約束**:
  1. **判定は pure な閉じた 3 値**: 行 x の write-set の `+` の file（受付と同じ `pipe/` の直下）が、2 つの面の**字面**（走行可能を運ぶ 1 行と、待ちを運ぶ本文）と 2 つの閾値から「空いている / 混んでいる / 測れない」の 3 値を出す pure 関数 1 本を持つ。host の面を読む口は同じ置き場の 1 本で、判定は fixture 文字列で測る（§7 の分担・外から差し替える口は作らない）。
  2. **閾値は core あたりの倍率で rules 行 2 本**（`host.runnable_per_core` / `host.blocked_per_core`・kind `HostRunnablePerCore` / `HostBlockedPerCore`・どちらも Int・§3.1 の表と manifest に足す）: host ごとに core 数が違うので tracked の manifest に絶対値を焼かず、**閾値 = 倍率 × 実測の core 数**にする。**値は user 裁定で既に出ている**: 裁定は「走行可能 > 4 × core 数」「待ち > core 数」で、**走行可能の倍率 = 4・待ちの倍率 = 1・裁定 id = `user 2026-09-20T15:23Z`・裁定日 = 2026-09-20**（C5）＝行の `ruling` と `ruled_at` にこの 2 つを入れる。裁定は比の形で出ており（事故の host の実値は 128 と 32）、`RuleKind` が持つのは Int だけなので、**倍率の側を Int 2 本で持つ**のが裁定の字面に素直で host を跨いで同じ意味になる（絶対値を写すと core 数の違う host で別の意味になる・却下の 3 つ目）。規範の値を持つのは manifest だけで、ここの 4 と 1 は裁定の出所の記録である（C1）。連鎖は行 o（§24）と同型で、置き場は全部 write-set の中である: `RuleKind` の variant と Int の列 = `crates/scribe2/src/rules/mod.rs`・manifest の行 = `rules/manifest.toml`・表 = [rules-manifest.md](./rules-manifest.md) §4（`docs/design/rules-manifest.md`）・歯の kind 件数 = `crates/scribe2/tests/e2e/rules.rs`・外形 = その snapshot。§24 と §3.1 を読まなくても、この 5 file で連鎖が 1 周閉じる。
  3. **待つ口は 1 本**: 行を撃つ前に「空いている」を待つ。待ちは完了 enum の variant 1 つ（2 つの閾値を運ぶ・pid を見張らない側＝`pid()` は 0）で、唯一の待機実装を通る（第 2 の poll loop を書かない・C3.4）。variant が pid を見張らない側に在ることは `crates/scribe2/src/fleet/wait.rs` の in-file の歯で測る（`pid()` が 0・既存の見張らない 4 つと同じ側）。
  4. **待ちの上限は `gate.slot_wait_s` を使い回す**: 受付の待ちと本待ちはどちらも「gate が行を撃つ前に待つ上限」で、同じ 1 本の便の中で順に効く。3 本目の値の線を足さない（C1 / C5）。
  5. **上限を超えた周は撃たない**: その行は process を起こさず、`Step` が**閉じた印**を運び、record に任意 field 1 つで残す（schema 1 のまま・§5 の足し方＝任意 field は読み手が無視できる欄で、schema 番号を上げない）。record の schema と欄を書く置き場は `crates/scribe2/src/pipe/gate/record.rs`（`record_verify`）と `Step` を持つ `crates/scribe2/src/pipe/gate/verify.rs` で、どちらも write-set の中である。行ごとの集計は最初にそうなった行の record 番号を持つ（検出線の rc 2 と同じ形）。
  6. **判定順は赤より先**: 判定は「箱の中で死んだ」の直後にこの印を読み、**閉じた理由 1 つ**で INCONCLUSIVE に倒す。§28 が「赤は検出線の rc 2 より先」と決めたのは**赤が信用できる周**の話で、host を測れていない周はその前段である（混んだ host で落ちた歯を赤に数えると、負荷が便を終端させる）。INCONCLUSIVE は終端でないので、負荷が引いた後に同じ木を測り直せる。
  7. **測れない周は待たずに進む**（縮退・止めない・FR46）: どちらかの面を読めない周と数でない周は行を撃ち、record に「測れない」の字面を残す（0 に潰さない・C10）。これが本境界の fail-open の面である。**この枝は host の面を差し替えずに測る**: 3 値から行動（「混んでいる」だけが待つ・「空いている」と「測れない」は撃つ）と record の字面（「測れない」の周だけ載り、空でも 0 でもない）を出す関数も同じ file の pure 関数で、判定と同じく fixture 文字列で測る（却下の 5 つ目と両立・e2e の歯はこの枝に届かなくてよい）。
  8. **極性一覧に載せる**: 行を撃つという行為を止めうる判定を返す境界なので、受付・封じ込めと違い ADR-0014 §2.1 の guard に当たる。`Guard` に variant 1 つ・`ALL` に 1 つを足し、境界の側に `POLARITY`（**in-loop / fail-open**）を置く。宣言順は行為の流れに合わせて gate の機械検証の段の直前で、外形の集計行の 4 つの数が動く。
  9. **land の主実測も同じ 1 本を通る**: 行を撃つ実装が 1 本である以上、主実測も同じ遮断器を通る。閾値は `Limits` が運び、2 つの `Checks` の構築点が同じ欄を埋める。
- **歯**:
  - in-file（行 x の write-set の `+` の file・接頭辞 `health_judge_`）: (a) 走行可能を 4 番目の欄の**分子**から読み、分母（総 process 数）に釣られない（分母だけが閾値を超える fixture を対に置く）／(b) 待ちを `procs_blocked` の行から読み、似た見出しの行に釣られない（釣られた周と読めた周で**判定が割れる** fixture で測る: 見出しが同じ語で始まる行〔`procs_blocked_total`〕を閾値超えの値で**先に**置き、本物の行を閾値以下の値で後に置いて「空いている」を期待する＝接頭辞一致に変えた変異が「混んでいる」へ倒れて落ちる。両方が閾値超えの形は接頭辞一致でも通るので歯にならない）／(c) 閾値 = 倍率 × core 数で、**ちょうど**の値は「混んでいる」でない（等号を境界に置かない）／(d) 片方だけが超えた 2 形はどちらも「混んでいる」／(e) 空・数でない・行が無いの 3 形はどれも「測れない」で、「空いている」に潰れない／(f) 3 値から行動と record の字面を出す関数は「混んでいる」だけを待つ側に、「空いている」と「測れない」を撃つ側に置き、record の字面は「測れない」の周だけ載って空でも 0 でもない（「空いている」の周と字面だけが違う）。
  - in-file（`crates/scribe2/src/fleet/wait.rs`・接頭辞 `fleet_wait_health_variant_`）: 新しい variant は pid を見張らない側で `pid()` が 0 を返し、閾値 2 つを運ぶ（既存の見張らない 4 つと同じ側に並ぶ）。
  - in-file（`crates/scribe2/src/pipe/gate.rs`・接頭辞 `gate_busy_order_`）: 印が在る周は**赤が 1 行在っても** INCONCLUSIVE になり、印が無い周の順（赤 → 検出線の rc 2）は 1 字も変わらない（2 つの枝を 1 本の歯に対で並べる）。
  - e2e（`crates/scribe2/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_health_`・toy repo・`--rules` の fixture で倍率を振る）: (a) 倍率 0 の fixture（＝必ず「混んでいる」）と `gate.slot_wait_s = 1` の便は verify の行が 1 本も撃たれず（呼出回数の file が空）verdict が INCONCLUSIVE で、record に印が載る／(b) 倍率を十分大きく取った fixture の便は従来どおり全段撃って PASS で終わり、record に印が載らない／(c) (a) の便は `Gated` に留まって同じ便を撃ち直せる（FAIL で終端しない）。
  - rules 行（`crates/scribe2/tests/e2e/rules.rs`・接頭辞 `rules_embedded_manifest_declares_host_health_`）: 埋め込みの manifest が 2 行を**値（走行可能 4・待ち 1）と裁定 id `user 2026-09-20T15:23Z` と裁定日 2026-09-20** つきで持ち、kind の包含で**行と variant を対で足させる**（片方だけの manifest は parse できず、片方だけの enum は親の歯が落とす・行 o と同型）。**外形**は行の verify が完全名 `rules_external_form` で撃つ（`rows=` と `kinds=` が 2 つずつ増える）。
  - 極性（`crates/scribe2/tests/e2e/polarity.rs`・接頭辞 `polarity_gate_health_`）: 一覧に新しい guard が in-loop / fail-open で在り、宣言順が gate の機械検証の段の直前である。**外形**は完全名 `polarity_external_form` で撃つ。
- **触らない**: 赤の数え方（§28・検出線の rc 1 も赤）・検出線の rc の意味と撃ち直し（§21）・審査役の撃ち直し（§29）・受付の枠と札（§3.2・行 w の面）・歯の道具箱（§30・行 v の面）・封じ込めの箱と `Released` の 4 値・`Verdict` の 3 値と rc・`verdict.json` の schema と field・record の schema 番号・`pipe show` の描画（判定行と秒だけを写す＝外形 snapshot は動かない）・`pipe.max_live`（行 o）・`gate.slot_wait_s` の**値**。
- **却下**:
  - **上限超を FAIL にする**: 凍った host は便の内容を測れていない。終端させると実装の成果ごと列から外れ、人が起こし直すことになる（台帳 `s2-07l.504` notes の直しの層 (3)）。
  - **待ちの上限に 3 本目の rules 行を足す**: 値の線と裁定が 1 つずつ増える。`gate.slot_wait_s` と意味が同じ（gate が行を撃つ前に待つ上限）。
  - **閾値を絶対値の rules 行にする**（事故の host の実値 128 と 32 を写す）: 裁定そのものが core 数との比で出ており、絶対値を写すと core 数の違う host で同じ manifest が別の意味になる（tracked の値に 1 つの host の形を焼く）。倍率 2 本なら裁定の字面がそのまま Int 2 つに落ちる。
  - **load average の 1 分値（小数）を読む**: 平均は遅れて動くので、凍り始めと回復の両側で実際の混み具合とずれる。走行可能と待ちの**瞬間値**はどちらも整数で、pure な判定が字面から出せる。
  - **host の面を歯から差し替える口を作る**: C2.2（裏口を作らない）。§7 の分担どおり、判定を pure 関数にして fixture 文字列で測る。
  - **遮断器を受付（§3.2）の中に置く**: 受付を通るのは `{jobs}` を持つ行だけで、共通 verify の全件 nextest も clippy も通らない。凍結を作ったのは歯の走行そのものなので、行を撃つ 1 点に置く。
  - **極性一覧に載せない（受付と同じ扱いにする）**: 受付は縮退して必ず進むが、本境界は行を撃たずに判定を止めうる＝guard の定義に当たる（ADR-0014 §2.1）。

### 32.1 errata（現物との差・`s2-07l.504.3`・規範は上の §32 のまま）

- **record の字面**: 任意 field は `host` の 1 つで、値は 2 語——待ちの上限を超えて撃たなかった行（閉じた印）は `host=busy`、測れないまま撃った行は `host=unmeasured`。空いていた周と撃つ process を持たない段（write-set 照合）は field を欠く。撃たなかった行は秒も封じ込めの欄も持たず、rc は撃てなかった周の -1（赤にも診断 file にも数えない）。
- **閉じた後の行**: 1 行目で上限を超えた周は、以後の行を**待たずに**閉じる（上限を行の本数だけ重ねない）。判定の理由は最初に閉じた行の record 番号を名指す（`n=<i>`）。
- **land の主実測**: 閉じた行が 1 本でも在る周は赤の集計より先に `MainCheck::Unmeasurable` へ倒す（gate の約束 6 と同じ極性・撃っていない行の -1 を赤に数えない）。
- **歯の fixture**: e2e の rules fixture（`crates/scribe2/tests/e2e/pipe.rs` の `SlotFixture`）は倍率の既定を十分大きな値に置く＝並列の歯で混んだ host でも既存の歯の遮断器は閉じない。倍率を振るのは接頭辞 `pipe_gate_health_` の歯だけである。

## 33. 変異検査は baseline だけ全数走らせ、mutant の test は最初に落ちた binary で打ち切る（契約表の行 y）

やさしく言うと: 変異を 1 つ当てるたびに歯を全部回している。単体の歯が 3 秒で「落ちた」と言っているのに、そのあと 5 分かけて e2e の歯まで回してから「落ちた」と数えていた。落ちたら即やめる。全部回すのは、変異を当てない最初の 1 回（baseline）だけでよい。

- **何が起きているか**（verified 2026-09-21・run `s2-07l.511-20260921T035301Z` の `outcomes.json` と mutant の log）: mutant 1 つの費用は build 約 2 秒 + test **320〜410 秒**。test の内訳は lib の歯（`finished in 2.98s`・`395 passed; 7 failed` で既に落ちている）→ bin の歯（0.01 秒）→ e2e の歯（残り全部）。cargo test は落ちた binary が在っても `--no-fail-fast`（§19・行 j）で次の binary へ進むので、**撃墜が 3 秒で決まった後の 5 分半は判定に寄与しない**。baseline の test は 309 秒。同じ日の実測: 58 mutants × 約 6 分（1 job・1 thread）、105 mutants で 1,751 秒（3 job・8 thread）。撃墜した 10 本は全部 lib の歯で落ちている。
- **§19 の理由は baseline にしか当たらない**: 行 j が `--no-fail-fast` を足したのは「flaky な歯 1 本で残りが未実行のまま baseline 失敗に倒れ、落ちた歯の全数を名指せない」ため（C10）。mutant の test は「1 本でも落ちれば撃墜」の 2 値で、落ちた歯の全数を読む読み手が居ない（`outcomes.json` は phase の rc しか持たない）。cargo-mutants は baseline と mutant に同じ cargo test の引数しか渡せない（`--` の後ろは逐語で両方へ行く）ので、道具の中で分けるには baseline を道具の外で撃つしかない。
- **現物**（verified・main a32b146）: `crates/xtask/src/mutantsdiff.rs` の `measure_args` が `cargo mutants … --jobs <j> -- --no-fail-fast -- --test-threads <t>` を組み、`run` が `Command::new("cargo")` で 1 回撃ち、rc 2 の周は cargo-mutants が書いた `mutants.out/baseline.log` の末尾を理由行に添える（`baseline_log_tail` / `diagnosed`・§21）。in-file の歯 `no_fail_fast_is_passed_to_cargo_test_by_mutants`（行 j の verify `no_fail_fast_`）が `measure_args` の末尾を pin する。`crates/xtask/src/main.rs` の in-file の歯 `mutants_diff_threads_flag_tail_is_two_dashes_then_the_received_value`（行 m・§22）も末尾 5 語 `-- --no-fail-fast -- --test-threads <t>` を pin し、同じ file が `measure_args` を 5 引数で呼ぶ site を 6 か所持つ（引数を足すと compile が落ちる＝この file は write-set の中）。cargo-mutants v27 は `--baseline skip`（baseline を撃たない）を持つ。**mutant の test の timeout は baseline から導かれる**（verified・cargo-mutants v27.1.0 の source の timeout 導出）: `--timeout` が無い周は **max(20 秒, ceil(baseline の test phase の秒 × 5))**。baseline を skip した周は導けず、警告を出して**固定 300 秒**に倒れる。build の timeout は既定で無い（`--build-timeout-multiplier` を渡していない）＝今も無い。
- **約束**:
  1. **baseline は道具の外で 1 回、全数**: `mutants-diff` が cargo-mutants の前に、同じ木で **2 手**撃つ——(i) `cargo test -p <scope> --no-run`（build・秒は測らない）→ (ii) `cargo test -p <scope> --no-fail-fast -- --test-threads <t>`（test・**壁時計の秒を測る**＝cargo-mutants の baseline の test phase と同じ意味の値）。(ii) の引数は pure 関数 1 本（`baseline_args(scope, threads)`）が組み、in-file の歯が pin する。(ii) の stdout / stderr は `target/mutants-diff/baseline.log` へ写す（cargo-mutants が書いていた file の代わり・置き場は同じ作業 dir）。
  2. **baseline が赤なら測らない**: rc ≠ 0 の周は cargo-mutants を起こさず、従来と同じ rc 2（測れていない）で終え、理由行の後ろに自前の `baseline.log` の末尾を添える（§21 の撃ち直しと `diagnosed` の形は不変・読む file の出所が変わるだけ）。
  3. **mutant の test は fail-fast**: `measure_args` から `--no-fail-fast` を落とし、`--baseline skip` と `--timeout <T>` を足す（`--jobs <j>` の後ろ・1 つ目の `--` の前）。1 つ目の `--` の後ろは `-- --test-threads <t>` だけになる。cargo test の binary の順（lib → bin → integration の名前順）は道具の既定のままで、in-file の歯が先に判定する。
  4. **timeout は道具と同じ式で自前に導く**（固定 300 秒に倒さない）: 壁時計は**ミリ秒の整数**で測り、`T = max(20, ceil(5 × ms / 1000))` を**整数演算**で導く（float を使わない＝`61.8 × 5` の丸めで 309 / 310 が揺れる形を作らない）。式は pure 関数 1 本（ms → 秒・in-file の歯が床 20 と倍率 5 と切り上げを pin する）。導いた値は record の 1 行に**足さない**（外形不変・`baseline.log` の末尾に 1 行 `timeout=<T>` を書き、rc 2 の周の診断で読める）。build の timeout は今と同じく渡さない。
  5. **数と判定は不変**: `outcomes.json` の 5 数の読み・`R-C12-1` の極性・record の 1 行（`mutants-diff: total=… scope=…`）の字面・`--jobs` / `--test-threads` の受け方（§22 / §31）は 1 字も変えない。timeout の式が道具の既定と同じなので、生き残り（missed）と時間切れ（timeout）の境も動かない。
- **歯**（in-file・`crates/xtask/src/mutantsdiff.rs`・接頭辞 `mutants_diff_fail_fast_`）: (a) `measure_args` の 1 つ目の `--` の後ろに `--no-fail-fast` が**無く**、`--baseline` `skip` が**在る**（位置も pin する: `--jobs <j>` の後ろ・`--` の前）／(b) `baseline_args` が `test -p <scope> --no-fail-fast -- --test-threads <t>` を**この順**で組む（`--no-fail-fast` は 1 つ目の `--` の前）／(c) baseline の rc ≠ 0 を受けた判定（pure・`Err` の字面に自前の log の末尾が載る）と rc 0 を受けた判定（`Ok`・cargo-mutants へ進む）の 2 形を対で／(d) timeout の式（入力は ms）: 3000 で 20（床・5 × 3 = 15 が 20 に上がる）・4020 で 21（切り上げ・5 × 4.02 = 20.1。floor なら 20・round なら 20 で、ceil だけが 21）・62000 で 310（倍率 5・切り上げも床も効かない点）の 3 点と、`measure_args` の `--timeout` が `--baseline skip` の後ろ・`--` の前に `T` の字面で載ること。行 j の歯 `no_fail_fast_is_passed_to_cargo_test_by_mutants` は **baseline の引数**を pin する形に書き直す（名は残す・行 j の verify `no_fail_fast_` が空にならない）。行 m の歯 `mutants_diff_threads_flag_tail_is_two_dashes_then_the_received_value`（`crates/xtask/src/main.rs`）は末尾 **4 語** `-- -- --test-threads <t>` を pin する形に書き直し、同じ file の `measure_args` の呼び site は新しい引数（timeout の秒）を足して合わせる（名は残す・接頭辞 `mutants_diff_threads_flag_` の 3 本が新しい引数で緑）。
- **実装の形**（`s2-07l.518`）: `measure_args` の引数は 6 つにできない（`clippy.toml` の `too-many-arguments-threshold` = 5）ので、jobs / threads / timeout 秒を `Pace { jobs, threads, timeout_s }` に束ねて 4 引数で受ける（値は 3 つの独立な値のまま・歯が別々に pin する）。`main.rs` の test 区間は 5 引数の shim を 1 つ置いて 6 か所の呼び site の形を保つ。式は `mutant_timeout_s(ms)`・判定は `baseline_judged(rc 0 か, log)`・build の手は `baseline_build_args(scope)`。
- **触らない**: §19 の (i)（gate の共通 verify と CI と done 区間の nextest 行の `--no-fail-fast`）・§21（rc 2 の撃ち直し）・§22（thread）・§31（受付）・`R-C12-1`・`parse_outcomes` と `Counts`・record の外形・`--jobs` を持つ行の封じ込め（§4）・rules 行。値の線は増えない（flag の入れ替え）。
- **却下**:
  - **baseline も fail-fast にする**（`--no-fail-fast` を全部落とす）: §19 の理由がそのまま戻る（flaky 1 本で落ちた歯の全数を名指せない）。
  - **`--test-tool nextest` に替える**: nextest の fail-fast は歯 1 本の粒度で残りを cancel するが、歯ごとに process を起こすので e2e が残る mutant で費用が増え、baseline の失敗の形（§21 の `baseline.log`）と timeout の導出も変わる。1 flag の入れ替えで足りる面に道具の入れ替えを持ち込まない。
  - **mutant の e2e を契約の verify の歯だけに絞る**: 宣言 file の行に穴が 1 つ増え（`{teeth}`）、器の render と intake の閉じた集合が動く（binary の入れ替えを伴う）。効くのは lib の歯で落ちなかった mutant だけで、本節の後に別の行で起こす（§11）。
  - **gate の壁時計に rules 行の予算を足す**: 値の線と裁定が 1 つ増える。撃墜が秒で決まれば予算が要る周は残らない（要るなら実測してから）。
  - **`--timeout` を渡さず道具の固定 300 秒に任せる**: 導出の出所（baseline）を消したのに導出値を道具に期待する形で、thread 1 で約 5 分回る生き残り mutant が時間切れに倒れて数が動く（審査 FAIL 2026-09-21・run `s2-07l.518-20260921T060328Z`）。
  - **timeout を rules 行の固定値にする**: host の thread 数で baseline の秒が変わるので固定値は host の形を焼く。道具の式（baseline × 5・床 20）をそのまま自前で持てば値の線は増えない。
- **後続**（§11）: lib の歯で落ちない mutant は今も e2e を全部回す（thread 8 で約 40 秒・thread 1 で約 5 分）。契約の verify 行の歯だけを e2e で回す形（宣言 file の穴 `{teeth}`）は別の行。列の 2 番目が枠を待ち切れず 1 job・1 thread に縮退する面（`gate.slot_wait_s` = 900 が gate 1 本の長さより短い）は値の裁定で、本節の外。

## 34. 検出線は契約が名指した歯と単体の歯だけを mutant に当てる — 穴 `{teeth}` と nextest の filter（契約表の行 z / aa・[ADR-0052](../../design-intent/decisions/ADR-0052-detection-line-runs-only-contract-teeth-and-unit-teeth.html)・`s2-07l.519`）

やさしく言うと: 変異を 1 つ当てるたびに package の test を全部回していたのを、「この契約が名指した test」と「単体の test」だけにする。どの test を名指したかは器が契約から読んで、検証の行の 4 つ目の差し込み口で道具へ渡す。変異を当てない最初の 1 回（baseline）は今までどおり全部回す。

- **何が起きているか**（verified 2026-09-21・run `s2-07l.516-20260921T071110Z` の detection/2/outcomes.json）: §33 の形（.518）で走った初回。67 mutants（撃墜 52・生存 7・compile 不能 7）で mutant 1 つの test phase の中央値 **43.7 秒**（撃墜も生存も同じ）、合計 714 秒（旧形 755 秒）。この契約の歯は e2e の binary にしか無く（`ledger_memo_plan_`）、e2e の binary は package の e2e の歯を全部走らせるので、fail-fast は lib の歯で落ちる契約（.511 の型）にしか効かない。枠を待ち切れず 1 thread に縮退した周は同じ走行が直列になり 1 mutant 約 6 分（.511 / .517・5〜8 時間の見込みで席が止めた）。rules 行 `R-C12-1` は enabled = false（週次記録）で、検出線は gate を落とす線ではなく lens の材料と記録である。
- **現物**（verified・main 998afca）: 宣言の検出線が置ける穴は `crates/scribe2/src/pipe/declaration.rs` の `BASE_HOLES`（`{base}` `{jobs}` `{threads}` の閉じた 3 つ・intake の `unfit` と gate の置換が同じ列を見る）。置換は `crates/scribe2/src/pipe/gate/verify.rs` の `fill_holes(line, base, jobs, threads)`、撃つ材料は同 file の `Checks<'a>`（`worktree` / `base` / `contract` / `common` / `detection`）で、gate（`crates/scribe2/src/pipe/gate.rs`）と land の主実測（`crates/scribe2/src/pipe/land/verify.rs`）が組む。契約の verify 行から filter の語を取る関数は `crates/scribe2/src/pipe/closure/derive.rs` の `nextest_filter`（行 → crate と filter と scope の 3 つ組・`teeth_places` と preflight の `teeth=` 行が使う）。道具は `crates/xtask/src/mutantsdiff.rs` の `measure_args`（§33 の形: `--baseline skip --timeout <T> -- -- --test-threads <t>`）と `baseline_args`。`.vessel.toml` の検出線は `cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads}` の 1 行。cargo-mutants v27 は `--test-tool nextest` を持ち、1 つ目の `--` の後ろを nextest の引数として逐語で渡す。nextest は filter の式（`kind(lib)` / `kind(bin)` / 正規表現の `test` 形 の和）を持つ。
- **前提**: ADR-0052（穴の閉じた集合は ADR-0021 §2.1 / ADR-0050 の決定・R-C12-1 の記録の母集団に触れる）。値の裁定は要らない（rules 行は増えない）。
- **約束**:
  1. **道具は `--teeth <語列>` を受ける**（行 z）: 語列は `,` 区切り・語は `[A-Za-z0-9_]+` だけ（それ以外を含む周は測らず rc 2「測れていない」・fail-closed）・`-` は空。受けた周の mutant の test は nextest で走らせる: `cargo mutants … --baseline skip --timeout <T> --test-tool nextest -- -E <式> --test-threads <t>`（2 つ目の `--` は無い）。式は `kind(lib) | kind(bin)`、語が在れば `| test(/^(語1|語2|…)/)` を足す（e2e の歯は契約が名指した語で始まる分だけ）。**`--teeth` 無しの周は §33 の形を 1 字も変えない**（行 aa が land するまで gate は無しで撃つ・CI で道具だけ撃つ周の既定）。
  2. **baseline は不変**（§33 約束 1 / 4）: 道具の外で package の歯を全数・`--no-fail-fast`・timeout の導出も同じ。mutant の test が nextest でも baseline は cargo test のまま（母集団の違いは記録が運ぶ）。
  3. **記録の 1 行に `teeth=` を足す**: `mutants-diff: total=… scope=<pkg> teeth=<-|n>`（`--teeth` 無し = `-`・空 = `0`・語が在れば語の数）。読み手（gate の record・lens の材料）は検出の母集団が狭まった事実をここから読む（C10）。5 数の読みと `R-C12-1` の極性は不変。
  4. **穴は閉じた 4 つ**（行 aa）: `BASE_HOLES` の末尾に `{teeth}` を足し（intake の `unfit` と gate の置換が同じ列）、`.vessel.toml` の検出線を `cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads} --teeth {teeth}` にする。契約の verify 行には従来どおり穴を置けない。
  5. **語の導出は器の 1 か所**: gate と land の主実測が `Checks` の契約の verify 行から `nextest_filter` で filter を取り、`,` で結んで `{teeth}` に置く（語が 0 の周は `-`）。preflight の `teeth=` 行と同じ関数を通す（導出の正本を増やさない・C10・ADR-0050 の thread と同じ形）。語を環境変数で渡さない（C2.2）。
  6. **着地は道具の入れ替えを伴う**: 行 aa が land した後、PATH の器を入れ替えるまで古い intake は新しい宣言の行（4 つ目の穴）を断る（`admission:declaration`）。入れ替えは席の手番（起動コマンドは repo に入れない）。
- **歯**:
  - 行 z（in-file・`crates/xtask/src/mutantsdiff.rs`・接頭辞 `mutants_diff_teeth_`）: (a) `--teeth a,b` で引数に `--test-tool nextest` が在り、1 つ目の `--` の後ろが `-E` `kind(lib) | kind(bin) | test(/^(a|b)/)` `--test-threads <t>` **だけ**で、2 つ目の `--` が無い／(b) `--teeth -` で式が `kind(lib) | kind(bin)`／(c) `--teeth` 無しの引数が §33 の形と 1 語も違わない（既存の歯 `mutants_diff_fail_fast_` の pin と同じ列）／(d) 語に空白・`.`・`/` を含む周は pure 関数が `Err`（閉じた理由 1 つ）／(e) 記録の行の `teeth=` が `-` / `0` / `2` の 3 形で出る（既存の行の 5 数と `scope=` は不変）。行 m の歯 `mutants_diff_threads_flag_tail_is_two_dashes_then_the_received_value`（`crates/xtask/src/main.rs`）は `--teeth` の有無で末尾が 2 形（無し = §33 の 4 語・在り = `--test-threads <t>` の 2 語）になることを対で pin する（名は残す）。
  - 行 aa（in-file・`crates/scribe2/src/pipe/declaration.rs`・接頭辞 `declaration_teeth_hole_`）: (a) `BASE_HOLES` が 4 つで末尾が `{teeth}`／(b) 検出線の行の `{teeth}` は `unfit` を通り、契約の verify 行の `{teeth}` は断られる（既存の `{jobs}` と同じ対）。
  - 行 aa（in-file・`crates/scribe2/src/pipe/gate/verify.rs`・接頭辞 `gate_fill_teeth_`）: (a) verify 行 2 本（`--lib … foo_` / `--test e2e … bar_`）から `foo_,bar_` が置かれる／(b) filter を持たない行は飛ばされる／(c) 0 本で `-`／(d) 語の順は verify 行の宣言順。
  - 行 aa（e2e・`crates/scribe2/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_teeth_`・toy repo の宣言に `{teeth}` を持つ検出線を置く）: 撃たれた行の record に契約の verify 行の語が `,` で結ばれて載り、`{teeth}` の字面が残らない。
- **触らない**: baseline の形（§33）・`--jobs` / `--threads` の受け方（§22 / §31）・受付と封じ込め（§3 / §4）・木が同じ主実測の省略（§5）・`R-C12-1` の極性と値・`parse_outcomes` と `Counts` の 5 数・契約の verify の実走（FR8）・rules 行。
- **却下**（ADR-0052 §3 と同じ）: 検出線を gate の外へ出す（lens の材料が消える・裁定）／契約の歯だけにして lib と bin を回さない（共有 code の生存が実態より増える）／道具が契約を自分で読む（導出の正本が 2 つ・C2.2）／cargo test の位置引数の filter（全 binary に同じ filter が掛かる）／並列度の上限を上げる（core で頭打ち・縮退に効かない）。
- **後続**（§11）: 行 aa の着地後に .516 型の契約（e2e の歯）で検出線の秒を実測し、ADR-0052 CSQ-P1 の「数分」を確かめる。生存の週次記録の母集団が変わった日を記録の側に残す（R-C12-1 の裁定材料は実測 2 周分）。
- **実測**（行 aa 着地後の 2 周・4 job × 8 thread・枠は granted）: .517 型（e2e の歯・`--teeth` 4 語）は検出線 157 秒 / 66 mutants（mutant 1 つ 2.6〜3.7 秒）・verify 合計 232 秒。.511 型（lib の歯・`--teeth` 4 語）は検出線 161 秒 / 78 mutants（caught 58・missed 2・unviable 18）・verify 合計 228 秒。同じ 2 契約の旧形（§33 前・縮退 1 job × 1 thread）は 5〜8 時間の見込みで止めた。CSQ-P1 の「数分」は実測で確かめた。検出の母集団が変わった日 = 行 aa の着地日（記録の `teeth=` が `-` から語数に変わる周が境）。

## 35. gate.slot_wait_s を 5400 に — 受付の待ちの上限を着地の列の上限と同じ値にする（契約表の行 ab・`s2-07l.519`）

- **出所**: user 裁定 2026-09-21T09:41Z（A2 の閾値変更・逐語は台帳 `s2-07l.519` の notes）。席の推奨（900 → 5400）を user が「それでよい」と裁定した。旧値 900 の裁定は user 2026-09-12T12:08Z（`s2-07l.153`）。
- **観測**（§34 の実測と同じ 2 周）: 検出線が 4 分の桁になった後も、枠（`gate.mutants_jobs` = 4）が塞がった周は 900 秒で縮退（1 job × 1 thread）に倒れ、縮退した 1 本が枠 1 つを数時間握る（.511 / .517 の旧形は 5〜8 時間見込みで止めた）。待ちの上限が gate 1 本の長さ × 枠の本数より短いと、混む周は必ず縮退に倒れる。着地の列の上限（`pipe.land_wait_s` = 5400）は同じ「便が進む前に待つ上限」で、その値まで待てば枠は gate 1 本の長さの桁で空く。
- **形**（値の変更だけ・rules 行の形は不変）: `rules/manifest.toml` の行 `gate.slot_wait_s` の `value` を 900 → 5400、`ruling` を裁定の id、`ruled_at` を裁定日にする。`kind` / `enabled` / 他の 5 行は不変。歯の pin（`crates/scribe2/tests/e2e/rules.rs` の `rules_embedded_manifest_declares_the_gate_cost_rows`・6 行の表）の該当行を新しい値と裁定 id と裁定日にする。§3 の表と rules-manifest.md は値を持たないので不変。
- **触らない**: 縮退の枝（§31 `degraded`・jobs 1 × threads 1）・受付の待ちの形（§22 / §31・poll の間隔）・`pipe.land_wait_s`・外形 snapshot（`rules_external_form`・値を持たない）・`R-C12-1`。
- **歯**（既存の e2e の歯 1 本の値の変更・新しい歯は無い）: `rules_embedded_manifest_declares_the_gate_cost_rows` の表の `gate.slot_wait_s` の行が (5400, 新しい裁定 id, 新しい裁定日) になり、base の manifest（900）に対して RED・HEAD で GREEN。値の両側の歯は tests 全体で走査して他に無い（`900` の字面は manifest の `pipe.ci_wait_s` にも在るが別の行・不変）。
- **却下**: 縮退を無くす（枠が空かない周に進めなくなる・fail-open か永久待ちの二択）／値を gate 1 本の実測から計算する（裁定値は宣言・C10・A2 の閾値は user が決める）／`pipe.land_wait_s` と 1 本の行に統合する（意味が違う 2 本・rules 行の削除は別の裁定）。

## 36. 道具箱の偽 binary を、走っている process の本体を書き換えずに入れ替える — 同じ dir に書いて rename で差し替える（契約表の行 ac・`s2-07l.530`）

- **出所**（`s2-07l.530`・orchestrator の実測 2026-09-21）: 同じ歯（`pipe_terminal_dispatch_manual_turn_starts_the_runs_it_can`）で main の CI が 2 回赤い。どちらも道具箱の偽 systemd-run を書く行の expect で、io の error は ExecutableFileBusy。便の差分と無関係で rerun は緑＝着地の終端を止める fixture の race である。
- **何が起きているか**（本行の base・main 4f70b12・verified）:
  - 道具箱を組む 1 関数（`crates/scribe2/tests/e2e/main.rs` の `toolbox_path`）は、呼ばれるたびに偽 systemd-run（`write_systemd_run_stub`）と偽 systemctl（`write_systemctl_stub`）を**同じ置き場の同じ名へ書き直す**。書き方は std の fs の write で、既に在る file を**その場で切り詰める**＝走っている process が握っている本体（inode）に触る。
  - その口は 1 便 1 回ではない。`crates/scribe2/tests/e2e/pipe.rs` の `pipe_cmd` が --state-dir を持つ起動の**たびに**道具箱を組み直す＝同じ歯の中で binary を 2 度撃つ周は、1 度目が起こした子 process が偽 systemd-run を exec している最中に 2 度目が同じ本体へ書く。落ちた歯はまさにその形（`turn` を 4 度撃ち、1 度目が 2 便を背景で起こす）である。
  - 偽 systemd-run の生成は **1 関数に寄っている**（§30 約束 3）ので、明示の 4 つの口（gate / stop / spawn / headless・呼出 4 か所）も同じ書き方を共有する＝直すのはその 1 関数の中だけで足りる。
  - 実行権つきの偽 binary を書く口は歯の全体で **20 か所**在るが、**同じ path へ 2 度目を書きうるのは道具箱の 2 本だけ**である（残る 18 か所は呼出 1〜2 か所の helper が fixture の組み立てで 1 度だけ書く）。本行が替えるのは 2 本の書き方で、20 か所の字面ではない。
- **形**（番号は done と歯に 1:1 で対応する）:
  1. **入れ替えは rename で**: 実行権つきの偽 binary を置く手が、目的の名と**同じ dir の中**に一時の名で本文を書き、実行権を付け、std の fs の rename で目的の名へ入れ替える。走っている process が握っている本体には 1 byte も書かない。同じ dir に書くのは、別の file system を跨ぐ rename が落ちるからである。実行権は入れ替えの**前**に付ける（付ける前に見える窓を作らない）。
  2. **道具箱の 2 本が両方そこを通る**: 偽 systemd-run と偽 systemctl の生成がどちらもその手を通る。明示の 4 つの口は呼び先が同じ 1 関数なので、**呼出の字面は 1 か所も動かない**。
  3. **残骸を残さない**: 入れ替えの後、道具箱の bin dir に在る entry は置いた偽 binary の名だけである（一時の名は rename で消える）。一時の名は目的の名から導いて同じ dir に作る。
- **触らない**: 偽 systemd-run の script の本文（--unit= の読み・同じ名の 2 本目を断る性質・argv を 1 起動 1 file で写す形・-- の後ろの exec）・偽 systemctl の答え 2 つ・記録 dir の名と読み手・PATH の組み方と 3 つの口（§30 約束 1 / 2）・明示の 4 つの口とその記録 dir・包めない host を作る口・既存の歯の総数と既存の assert。器の src は 1 行も触らない。
- **却下**:
  - **道具箱を歯ごと（起動ごと）の別 dir に切る**（memo の案 1）: 記録 dir も口ごとに割れ、`scope_record` と `toolbox_record` の「ちょうど 1 件」の母集団が起動ごとに変わる＝既存の assert の意味が動く。落ちているのは書き方で置き場ではない。
  - **道具箱を 1 便 1 回に絞る**: 置き場の中身が消える周に 2 度目の起動が道具箱**無し**で走り、実 systemd-run へ静かに戻る。§30 の「後から書かれる呼出も自動で通る」を壊す。
  - **ExecutableFileBusy を握り潰して撃ち直す**: 落ちる窓は残したまま歯を鈍くするだけで、何回で足りるかを契約が答えられない。原因は書き方 1 つで消える。
  - **落ちる周そのものを歯で再現する**（exec 中に書き直して ExecutableFileBusy を測る）: 窓は execve の中の一瞬で base でも**通ってしまう周がある**＝base で RED と書けない空虚な歯になる。本行は「走っている本体を触らない」という**観測できる性質**（歯 (a)(b)）で測る。
  - **書く前に目的の名を消す**: 入れ替えの間、道具箱に偽 binary が**無い**窓ができ、その窓の起動が実 systemd-run を解く。rename は窓を作らない。
- **歯**（接頭辞 `e2e_shim_atomic_`・置き場は行 ac の write-set の pipe の歯の file）:
  (a) 形 1 の核: 同じ bin dir へ、記録 dir だけ替えて偽 systemd-run を 2 度置く。1 度目の本体に hard link を張っておくと、2 度目の後もその link の本文は**1 度目の記録 dir を名指したまま**で、2 度目の記録 dir の字面を**持たない**（在ると不在の両方を測る）。その場で切り詰める書き方では link の本文が 2 度目の字面に変わる。
  (b) 形 1 の別面: 同じ置き場に道具箱を 2 度組むと、偽 systemd-run と偽 systemctl の inode 番号が**2 本とも**変わる（母集団 = bin dir の entry 名の全件を同じ assert に出す）。
  (c) 形 3: 2 度組んだ後の bin dir の entry は偽 systemd-run と偽 systemctl の**ちょうど 2 件**（一時の名の残骸 0・母集団は entry 名の全件）。
  (d) 形 1 の権限の窓: 2 度目の後の偽 systemd-run を PATH を通さず直に 1 回撃つと rc 0 で終わり、記録が 1 件増える＝実行権は入れ替えの前に付いている。
  既存の歯が測る側（本行は足さない）: 道具箱を通した便が共通 verify の行を包むこと（行の 2 本目の verify が完全名で撃つ）。
- **flip-check**（歯だけの便）: 器の src を 1 行も触らないので、retroactive の札（[contract-source.md](./contract-source.md) §31・`s2-07l.530` を名指す）を、本便で test 区間が動いた file の**行頭**に置く（効く 4 条件 = test 区間内 / 行頭 / bead id 必須 / base から持ち越した札は効かない）。札は HEAD から読まれるので **commit してから** flip-check を撃つ。接頭辞 `e2e_shim_atomic_` は base に 0 本なので、行の 1 本目の verify は base で「該当 0 本」＝RED、HEAD で緑になる。

## 37. 歯の道具箱に台帳 client の見張りを既定で置く — 歯から実台帳へ届く経路を塞ぎ、撃ちにいった周を記録の件数で数える（契約表の行 ad・`s2-07l.484`）

- **出所**（`s2-07l.484`・memo 2026-09-19 + 本 § の再実測 2026-09-22）: 歯の helper が --rules を渡す口は少なく、器が台帳 client を既定名で解く経路が残る、という memo。
- **何が起きているか**（本行の base・main 4f70b12・verified）:
  - 器が台帳 client を子 process で起こす口は src の **5 か所**である（読み 4・書き 1）。列の 1 周（`dispatch.rs` の `turn`）・着地の終端の close（`land/finish.rs`）・doctor の台帳 lint と台帳の形（`ledger/` の 2 本）・hook の 1 本。client の名は --bd か既定値で、既定値は **PATH から解く 1 語**である。
  - **既定値に倒れた周の行き先は host で割れる**: 開発 host は実 client を PATH に持ち CI は持たない＝同じ木で子 process が起きるか否かが host に依る。§30 が偽 systemd-run で畳んだのと同じ割れが台帳の側に残っている。
  - **いま実台帳へ届いている歯は 0 本である**（本 § の再実測）。終端の close は「押す先を宣言した repo」の後ろに在り（`finish.rs` の早期 return）、宣言する fixture を使う歯 9 本は**全部 --bd を渡す**。列の 1 周で --bd を渡さない 3 か所は --repo を渡さず reason=args で返る。ゆえに本行は**時間の短縮を約束しない**——約束するのは届く経路を塞ぐことと、届きにいった回数を**測れるようにする**ことである（memo の費用の主張はその記録で初めて真偽が決まる）。
  - 台帳 client を明示する口（--bd に偽 client の絶対 path を渡す口）は **43 か所**で、PATH を通らないので本行と交わらない。
- **形**（番号は done と歯に 1:1 で対応する）:
  1. **道具箱に見張りを 1 本置く**: 道具箱を組む 1 関数（§30 約束 1・`crates/scribe2/tests/e2e/main.rs` の `toolbox_path`。偽 systemd-run と偽 systemctl を bin dir に書いて PATH の先頭に足す）が、その 2 本に並べて**台帳 client の既定名の偽物**（既定名は `crates/scribe2/src/seat/ledger.rs` の `DEFAULT_BD`）を置く。呼ばれた argv を 1 起動 1 file で記録 dir へ写してから、**台帳を解けない host と同じ形で断る**（rc は非 0・標準出力は空）。
  2. **答えが host に依らなくなる**: 道具箱を通す起動は、実 client を持つ host でも持たない host でも同じ 1 つの答えになる。--bd を渡す既存の 43 か所は絶対 path なので 1 つも通らず、既存の assert は動かない。
  3. **撃ちにいった周が数で残る**: 見張りの記録 dir の件数が「器が台帳 client を起こした回数」である。0 件は「起こしていない」、1 件以上はその argv が読める（母集団は件数と対で出す）。
- **触らない**: 器の src（台帳 client の解き方・--bd の受け方・rules 行 `seat.ledger_timeout_s` の読み・列の 1 周の分岐の順と unmeasured の理由・終端の 3 段と close の引数と cwd の固定〔[pipeline.md](./pipeline.md) §48〕）・道具箱の既存の 2 本とその記録 dir・PATH の組み方と 3 つの口・明示の 4 つの口・--bd を渡す 43 か所・既存の歯の総数と既存の assert。
- **却下**:
  - **memo の案（run / resume の helper に toy の --rules を渡す）**: 実測で壊れる。列の 1 周は `seat.ledger_timeout_s` の行が**無い**と台帳に届く前に reason=no-rule へ倒れる（`turn` の最初の分岐）ので、列の歯の写しはその行を**わざわざ足している**（`dispatch.rs` の `dispatch_rules`）。行を落とした写しを helper の全部に渡すと、列の歯が「台帳を読んだ」を 1 本も測らなくなる。鍵の行を抜く形は歯を空虚にする。
  - **--bd を全部の起動に渡す**: 起動の呼出は `run_pipe` だけで 210 か所在り、字面が全部動く割に**後から書かれる呼出**を守らない（§30 の同じ却下）。既定に置けば新しい呼出も自動で通る。
  - **器の側に「toy なら台帳を撃たない」分岐を作る**: 契約と宣言の外に既定を作る（C5 / C1）。器が見るのは --bd と rules 行だけ、という面を崩す。
  - **実 client を PATH から外す**: 台帳を読む経路が歯から丸ごと消え、読みの引数も断りの型も測られなくなる（§30 の「全部を包めない host にする」と同型）。
  - **見張りを rc 0 で「空の台帳」として答えさせる**: 列の 1 周が「0 件の台帳を読めた」に倒れ、unmeasured reason=ledger の枝が測られなくなる。断る側なら実 client を持たない host のいまの答えと同じである。
- **歯**（接頭辞 `e2e_ledger_tripwire_`・置き場は `crates/scribe2/tests/e2e/pipe.rs` と列の歯の file `crates/scribe2/tests/e2e/pipe/dispatch.rs`。行の 2 本目の verify が名指す既存の歯 `pipe_terminal_dispatch_manual_turn_starts_the_runs_it_can` も同じ `pipe/dispatch.rs` に在る）:
  (a) **非空虚の枝**（先に書く）: 列の 1 周を --repo と --runner と台帳の待ち上限の行を持つ写しつきで、--bd を**渡さず**撃つと、見張りの記録が**ちょうど 1 件**在り、その本文が読みの引数（--readonly と一覧の語）を持つ。これが無いと (b) は「経路が無いから 0 件」で空虚に通る。
  (b) **既定の枝**: helper 経由で 1 便を intake → spawn → gate → land まで通した後、見張りの記録が **0 件**である。同じ便で道具箱の systemd-run の記録が**1 件以上**在ることを対で測る（便が道具箱を通っていない周に 0 件が空虚に通らない）。
  (c) **明示の口と食い合わない**: --bd に fixture の偽 client の絶対 path を渡した周は、見張りの記録が 0 件のまま、偽 client 側の log に呼出が残る。
- **flip-check**（歯だけの便）: 器の src を 1 行も触らないので、retroactive の札（[contract-source.md](./contract-source.md) §31・`s2-07l.484` を名指す）を、本便で test 区間が動いた file の**行頭**に置く（効く 4 条件は §36 と同じ）。接頭辞 `e2e_ledger_tripwire_` は base に 0 本なので、行の 1 本目の verify は base で「該当 0 本」＝RED である。

### 37.1 errata（現物との差・`s2-07l.484`・規範は上の §37 のまま）

- **§36 歯 (c) の期待の列が 1 件増えた**: 見張りは形 1 のとおり偽 systemd-run と偽 systemctl に**並べて同じ bin dir** へ置く（§36 の入れ替えの手を通る）ので、bin dir の entry 名の全件を測る §36 歯 (c) の期待は「2 本の名」から「置いた偽 binary の名の全件（既定名の見張りを含む 3 件）」へ変わった。測る性質（一時の名の残骸 0・母集団は entry 名の全件）は変わらない。別の dir に置いて期待を守る案は PATH の組み方を変える（触らないに反する）ので採らない。
- **見張りの記録の名**: 1 起動 1 file の名は mktemp の一意な名で、systemd-run の記録（unit 名）とは別の記録 dir に置く＝既存の「ちょうど 1 件」の母集団に混ざらない。
- **既存の歯の総数**: e2e の歯は 1182 本から本行の 3 本を足して 1185 本（既存の歯は 1 本も消えず・名も変わらない）。

## 38. 作り手が死んだ席の scope を unit 名の pid で見つけて畳む — 止めた後に残る tmux の server を cgroup ごと終える（契約表の行 ae・`s2-07l.452`）

- 出所（`s2-07l.452`・planner 実測 2026-09-17 + 本 § の再実測 2026-09-22・verified・main f25084c）: 便が止まった後も席の tmux の server とその子の sh が生き残り、systemd の scope が active running のまま残る。母集団 = その日 active だった scope 5 本のうち走行中でない 4 本で、**4 本とも**再現した。うち 1 本は 5 時間以上残っていた。影響は (a) 走行中の便の本数を scope の active で数える読み手が偽の live を数える、(b) server 1 本ごとに fd と process が漏れる、の 2 つである。
- 現物（本行の base・main f25084c・verified）:
  - 片付けの実体は 1 本（`crates/scribe2/src/pipe/confine.rs` の `release`・kill の後に reset-failed を 1 回）で、呼び手は src の **6 か所**である: 終端の 1 行の口（`crates/scribe2/src/headless/runner.rs`）が直に撃つ 1 か所と、`release_scope` を撃つ 5 か所（`crates/scribe2/src/pipe/spawn.rs` / `crates/scribe2/src/pipe/review.rs` / `crates/scribe2/src/pipe/gate/lens.rs` / `crates/scribe2/src/pipe/gate/verify.rs` / `crates/scribe2/src/fleet/usage.rs`）。**6 か所とも、自分が起こした子を待ち終えた周にしか通らない**。
  - unit の名を知っているのは作った process ただ 1 つである。名は `unit_name` が場所・段・n・**自分の pid**・通し番号から組み、event log にも state にも**どこにも記録しない**（`unit=` の字面は src 全数 grep で 2 件、どちらも `systemd-run` へ渡す引数と in-file の歯の fixture）。作った process が死ぬと、その scope の名を知る者が居なくなる＝6 か所のどれからも畳めない。
  - 止める口（`crates/scribe2/src/pipe/stop.rs`・485 行）に `confine` の語は **0 件**である。止める相手は席の pid（process group 宛ての signal）と運転手の pid だけで、scope には触らない。tmux の server は自分の session へ離れるので group 宛ての signal に当たらず、cgroup には残ったままになる。
  - 終端した便は止める口が「既に終端である」で断る（段が `Stopped` の周は生きていない）ので、後から畳み直す口も無い。
  - **名は既に作り手の pid を持っている**: 形は 器の名・場所・段・n・作り手の pid・通し番号を `-` で連ねた 1 語で、場所と段の中の記号は `tame` が `-` に畳むが、**末尾から 2 番目**は必ず pid である。作り手が生きているかは既にある probe 1 本で測れる＝第 2 の probe を作らない（C6.3）: `crates/scribe2/src/fleet/store.rs` の `started_ms`（`pub`・pid 1 つを取り `Probe` を返す）を pid で呼び、`Started` が生きている・`Absent` が死んでいる・`Unreadable` は測れない（生きている側に倒し、畳まない）。lock の本文を取る `lock_owner` は本行では呼ばない。`store.rs` は読むだけで write-set に入れない。
- 形（番号は done と歯に 1:1 で対応する）:
  1. **名から作り手の pid を読む pure な 1 本**（`crates/scribe2/src/pipe/confine.rs`）: `-` で割った列の末尾から 2 番目を数として読む。割れ数が足りない名・数でない名は `None`（推測で埋めない）。
  2. **残骸の一覧を取る 1 口**: `systemctl` を `--user list-units` で 1 回撃ち、legend と pager を止めた素の形で active な scope に絞り、pattern は器の名から導いた 1 語（`-*.scope`）にする。行頭の unit 名だけを取る。道具が無い周と rc が非 0 の周は**空の一覧ではなく「測れなかった」**を返す（0 件と融合しない・C10）。
  3. **畳む相手を決める pure な 1 本**: 一覧の行から、名が形 1 で読める ∧ その pid が**生きていない**ものだけを残す。生きている作り手の scope には触らない（自分の pid の scope も、自分が生きているので残る）。
  4. **撃つのは止める口の終端**: 席への signal と運転手を止めた後・`RunStopped` を書く前に、形 3 が残した unit を既存の `release` で 1 本ずつ畳む（`--run` と `--all` の両方）。kill と reset-failed の並びは §25 のまま 1 unit につき 2 呼出で、verify 行ごとの片付けの並びは 1 字も動かない。
  5. **数が行に出る**: 止める口の行の末尾に `scopes=<畳んだ数>/<一覧の件数>` を足す（形 2 が測れなかった周は `scopes=-`）。rc と既存の token（`run=` / `seats=` / `stopped=` / `driver=stopped`）は不変で、止め切れなかった周に `RunStopped` を書かない極性も不変である。
- 触らない: `release` の中身と片付けの結果の 4 値・`release_scope` の filter と 6 か所の呼び手・`unit_name` の形と通し番号・verify 行ごとの片付けの呼出の並び（§25・e2e の `release_sequence` が pin する 2 行）・席と運転手を止める順（[pipeline.md](./pipeline.md) §39）・道具箱の PATH の組み方と bin dir に置く偽 binary の本数（3 件のまま）。**§36 と §37 の「触らない」が挙げる「偽 systemctl の答え 2 つ」は本節が 1 つ足して 3 つになる**（数を測る歯は base に無い——§36 の歯 (c) が数えるのは bin dir の entry 名で、偽 systemctl の script の分岐の本数ではない）。
- 却下:
  - **unit の名を event log に記帳して後から読む**: 便 1 本の gate は verify 行ごとに scope を作るので記帳が数十件に膨れ、段を持たない event を記帳の口が運ぶ形になる。名は既に pid を持っている（形 1）ので記帳は要らない。
  - **止める口が cgroup の中身を直に読んで殺す**: 片付けの経路が 2 本になる（C2）。畳むのは既存の 1 本のままにする。
  - **便の席の tmux の server を名指しで畳む**: 席の socket の名を知る必要があり、tmux 以外の孤児（sh・cargo）には効かない。scope を畳めば cgroup の全 process が終わる。
  - **時間で切る**（「N 分 active なら孤児」）: 閾値が恣意で、長い verify 行の scope を殺す。作り手の生死は測れる値である。
  - **終端した便も止める口が受け付けるようにする**: 終端の極性が緩み、`RunStopped` の二重記帳を招く。残骸は便の段に紐づかない（一覧は host 全体を見る）ので、掃除は段の外に置く。
  - **一覧が空の周と測れなかった周を同じ `0` にする**: 道具の無い host で「孤児 0 件」が緑に化ける（C10）。
- 歯（接頭辞 `pipe_scope_reap_`・`crates/` 全体の fn 名の substring として base に 0 件。in-file は `crates/scribe2/src/pipe/confine.rs` の `mod tests`・e2e は偽 `systemctl` の呼出を記録する fixture が既に在る `crates/scribe2/tests/e2e/pipe/gate.rs`）:
  (a) 形 1: 素直な名・場所に記号を含む名（`tame` が `-` に畳んだ形）の 2 形で末尾から 2 番目の pid が読め、割れ数が足りない名・pid が数でない名が `None`（母集団 = 4 形を assert に出す）。
  (b) 形 3: 一覧の行の列と「生きている pid の集合」を渡し、死んだ作り手の unit だけが残る。生きた作り手の行と形に合わない行は残らない（母集団 = 入れた行数を assert に出す）。
  (c) 形 2: 一覧を取る呼出の引数の列が `--user` `list-units` で始まり、末尾の pattern が器の名から導いた字面である（定数を直に読む）。rc 非 0 の答えが「測れなかった」になり、空の一覧と弁別される（2 形を対で）。
  (d) 形 4 / 形 5（e2e）: 偽 `systemctl` が active な scope を 2 本返し（1 本は生きた pid・1 本は死んだ pid を名に持つ）、止める口を撃つと kill が**死んだ側の 1 本にだけ** 1 回撃たれ、生きた側には 0 回で、行の末尾が `scopes=1/2` になる。base は一覧を 1 度も撃たない＝機能不在の RED。

### 38.1 errata（現物との差・`s2-07l.452`・規範は上の §38 のまま）

- **一覧の引数**: `--user list-units` の後に `--no-legend` `--no-pager` `--plain`（行頭の印を落とす）`--type=scope` `--state=active` を並べ、末尾が器の名から導いた pattern である。rc 0 の空の出力は 0 件、rc 非 0 と起動できない周は測れなかった値である。
- **畳んだ数**: 既存の片付けの結果が「殺した」か「既に無い」の unit を数える（「失敗」と「道具が無い」は数えない）。形 3 は器の名で始まり `.scope` で終わる行だけを相手にする。
- **`--all` の行**: `--run` と同じ位置（席の後・`RunStopped` の前）で畳み、行の末尾に同じ 1 語を足す。in-file の歯は畳む手を stub で受ける（実 `systemctl` を撃たない・止める席の stub と同じ形）。
- **既存の歯の期待**: 道具箱の偽 `systemctl` は一覧に rc 1 で答えるので、止める口の行の字面を全文で測る既存の歯 3 本の期待は末尾に `scopes=-` を持つ形になった（既存の token は 1 字も動かない）。host の PATH のまま止める口を撃っていた既存の歯 1 本（札の pid が自分自身の周）は道具箱の PATH で撃つ形にした（歯が host の実 scope を畳まない）。

## 39. 検出線が測っていない package を判定行に出す — 差分が触れた member のうち範囲の外の dir を名指し、母集団 0 と生存 0 を弁別する（契約表の行 af・`s2-07l.274`）

- 出所（`s2-07l.274`・admin の観測 2026-09-14 + 本 § の再実測 2026-09-22・verified・main f25084c）: 検出線は core の package 1 つしか測らないので、task runner の側だけを変える便は母集団 0 で終わる。判定行はそれを「生存 0」と同じ字面で書く。task runner は CI の門そのもの（入口確認・検査・検出線・rules の差分・path の掃除）なので、門の歯が空虚でも検出線が沈黙する＝憲法 C12.4 の範囲に門が入っていない。
- 現物（本行の base・main f25084c・verified）:
  - 測る範囲は 1 つの名前に束ねられている: `crates/xtask/src/mutantsdiff.rs` の走らせる手が workspace の配置から core の名を解き、引数を組む関数へ**1 語だけ**渡す。その関数は `-p` の後ろに受けた 1 語を push するだけで、第 2 の package を受ける口が無い。範囲を運ぶ型も名 1 つと歯の語数しか持たない。
  - `-p xtask` の字面は `crates/xtask/src` の全数 grep で **0 件**である。
  - 判定行は `mutants-diff:` の後ろに 7 つの token（`total=` `caught=` `missed=` `unviable=` `timeout=` `scope=` `teeth=`）を並べる 1 行で、範囲の外だけを変えた便は `total=0` で終わる。読み手（gate の record・審査役の材料・週次の記録）にとって「生存が 1 本も無い」と「1 本も測っていない」が**同じ字面**になる（C10 に反する面）。
  - 母集団を導く材料は既に在る: workspace の配置の型が root と core の dir と**全 member の dir** を持ち、走らせる手は cargo-mutants を撃つ前に差分を file へ置いている。
  - 前提が 1 つ動いた: ADR-0052 で歯の絞り（§34）が入り、mutant の test を nextest の式で絞る道具は既に在る。動いていないのは package の範囲だけである。
- 形（番号は done と歯に 1:1 で対応する）:
  1. **差分が触れた member を導く pure な 1 本**: 差分の本文と member の dir の列から、触れた member の dir を root 相対・宣言順・重複無しで返す。member のどれにも属さない path（設計 doc など）は数えない。
  2. **範囲の外を名指す**: 形 1 の結果から core の dir を除いた残りが「触れたのに測っていない面」である。
  3. **判定行の末尾に 1 語だけ足す**: `outside=` の後ろに形 2 の dir を `,` で結ぶ（0 件は `-`）。既存 7 token の名・順序・書式は 1 字も変えない（§34 約束 3 が `teeth=` を足したときと同じ足し方）。的を絞った周の行も同じ 1 語を末尾に足す。
  4. **判定は 1 つも動かない**: 5 数の読み・週次の記録の行の極性と値・rc・`-p` に渡す名・baseline の 2 手・歯の絞りの式は 1 字も変えない。`outside=` は**記録であって門ではない**。
- 触らない: 引数を組む関数の引数の列と、速さの 3 値と範囲を運ぶ型の形・的を絞った周の引数の組み直しと的の行の読み・baseline の 2 手と timeout の式（§33）・歯の絞りと nextest の式（§34）・workspace の配置を読む手・宣言 file の 4 つの穴と検出線の 1 行。
- 却下:
  - **task runner を第 2 の範囲として足す**（memo の候補 (a)）: task runner の歯は fixture の toy な crate を実 cargo で build するので、変異 1 本が分の桁になりうる。**その秒を 1 周も実測していない**（本節の base では撃てない）ので、枠と時間の予算の裁定より先に既定を変えない。本節の `outside=` が、どの便がどれだけ範囲の外だったかを記録に残し、その裁定の材料になる（後続・§11）。
  - **task runner は in-file の単体の歯だけを対象にする**（候補 (b)）: 同じ費用の実測が要る。絞りの道具は §34 で既に在るので、実測の後は値の面になる。
  - **「検出線の範囲 = core」と書いて対象外と明記する**（候補 (c)）: 穴を文書で固定するだけで、測れていない周を読み手が見分けられる形にはならない。本節は同じ事実を**記録の側**に置き、見分けを作る。
  - **範囲の外が在る周を `total=-` に倒す**: 既存 7 token の書式が動き、判定行の最終行を写す読み手が全部動く。足すのは末尾の 1 語に留める。
  - **範囲の外が在る周を測れなかった（rc 2）にする**: 門の極性が動く。週次の記録の行は今は門ではないので、ここで倒すと deny 化と同じ効きになる。
  - **member の package 名を解いて名指す**: 各 member の manifest を読む手が 1 つ増える。dir の root 相対の path は既に在る材料から導けて、どの面が測られていないかを同じだけ示す。
- 歯（接頭辞 `mutants_diff_outside_`・`crates/` 全体の fn 名の substring として base に 0 件。in-file は `crates/xtask/src/mutantsdiff.rs` の `mod tests`）:
  (a) 形 1: 差分が core だけ・task runner だけ・両方・member の外だけ、の 4 形で、触れた member の dir が宣言順・重複無しで出る（母集団 = 入れた member dir の件数を assert に出す）。同じ member の 2 file を触る差分が 1 件に畳まれることを同じ歯で測る。
  (b) 形 2: (a) の 4 形から core の dir を除いた結果が、順に 0 件・1 件・1 件・0 件になる。
  (c) 形 3: 判定行の末尾が `outside=-` と `outside=` + dir 1 件の 2 形で出て、先頭の 7 token が 2 形とも 1 字も変わらない（母集団 = token を割った列の長さを assert に出す）。`crates/xtask/src/main.rs` の既存の行の字面の歯が同じ列を見るので、そちらの期待も末尾の 1 語を持つ形に揃える（名は残す）。

### 39.1 errata（現物との差・`s2-07l.274`・規範は上の §39 のまま）

- **path の取り方**: 差分の本文の `--- a/` と `+++ b/` の行から path を取り、`/dev/null` の側は数えない（追加も削除も片側の path で拾う）。member の dir と一致するか dir + `/` で始まる path だけをその member に数える＝名の接頭辞だけが同じ dir は別の member である。
- **行を組む 2 本は範囲の外の列を引数で受ける**: 5 数の行と的を絞った周の行の両方が同じ 1 語を末尾に足す（的の周は `population=targets` の後ろ）。範囲を運ぶ型の形と引数を組む関数の引数の列は変えない。差分の file が読めない周は空の差分として `outside=-` を書く（記録であって門ではない・rc は動かない）。

## 40. 検出線の走らせる手から「的を絞った周」の群を子 module へ割る（契約表の行 ag・純移動・`s2-07l.198.2` の前提）

- 出所（orchestrator の実測 2026-09-22・verified・main 4aafb6f）: `crates/xtask/src/mutantsdiff.rs` は 1439 行・幅 120 で正規化して **1458**（上限 R-C4-2 = 1500）＝**余地 42** である。行 af（`s2-07l.274`・a4f6255 で Landed）が `outside=` の 1 語を足した直後の姿で、境界 crate の便（`s2-07l.198.2`・size M・見積 `pipe.size_m_lines` = 300）はこの file を write-set に持つ——変異した core の歯が境界 crate へ移るので、cargo-mutants の test の範囲を組む手を直す必要がある。**受付は `cap-headroom` で断る**（余地 42 < 300）。[pipeline.md](./pipeline.md) §42 / §43 と [contract-source.md](./contract-source.md) §37 と同じ型の純移動で余地を作る。
- 現物（行番号は main 4aafb6f・verified）: 責務は 9 群——(1) 5 数の型（`Counts` と impl・21〜71）、(2) 範囲の外を名指す群（§39 が足した 4 item・72〜114）、(3) 引数を組む inline の子 module（115〜216・既に `mod scope` として割ってある）、(4) **的を絞った周の群**（217〜474）、(5) 5 数の読みと rc（`verdict` / `parse_outcomes` / `measured` / `without_outcomes`・475〜585）、(6) baseline と timeout（586〜660）、(7) manifest の deny 行の読み（661〜712）、(8) 引数の読み（713〜784）、(9) 走らせる手（785〜1014）、そして歯の区間（1015〜1439）。
  - (4) の item は **14 個・258 行（正規化 260）**: `Target` と impl・`Outcome` と `OUTCOMES` と impl・`Aimed` と impl・`aimed_of` / `targets_of` / `diff_of` / `place_diff` / `targets_in` / `aimed_args` / `regex_literal`（宣言順・217〜474）。
  - (9) のうち `aimed_run`（886〜924・doc を含めて 884 行から・**39 行**）は (4) の後段（出力 dir を読んで的ごとに分類し判定行を出す手）で、責務は (4) と 1 つである。移すのは **15 item・297 行（正規化 299）**になる。
  - `mod scope` は既に inline の子で、親は `pub use` 1 文で `measure_args` と `Pace` と `Scope` を再輸出している（115 行）＝**同じ file に子 module を足す形は既に通っている**。
- 決定的な制約（実測）:
  - **歯は 1 本も動かせない**: `crates/xtask/src/main.rs` の doc（125〜129 行）が「`mutants-diff` の歯は base に在る file へ置く。新規 module の中に置くと file ごと base に無いので flip-check が構造的に測れない（not-flippable）」と書く。本節が移すのは **src の item だけ**で、歯の区間（1015 行以降・同 file の `#[test]` は **20 本**）は 1 本も動かさない。
  - **歯は `use super::{…}` で名を引く**（1017〜1021 行）。子へ移した名は親の `use` 文で親の scope に戻せば、歯の本文も `use` 文も 1 字も変わらない（[contract-source.md](./contract-source.md) §37 と同じ名前解決の形）。
  - **`#[cfg(test)]` の `use` 文の置き場は歯の区間の直前**（親の `#[cfg(test)]` + `mod tests` の 2 行の直前）に限る。xtask の門は file を「最初の行頭 `#[cfg(test)]` より前 = src 区間 / 以後 = test 区間」で切る（`crates/xtask/src/workspace.rs` の `split_test_src`）ので、file の頭へ置くと src の本体が丸ごと test 区間に落ちる（`s2-07l.257` の 2 本目の gate FAIL と同じ形）。
  - **親の本体が呼ぶ名は 5 つだけ**（実測・移す範囲の外の行）: `targets_of` と `diff_of`（797 行）・`place_diff`（816 行）・`aimed_args`（835 行）・`aimed_run`（851 行）。**歯だけが引く名は 6 つ**: `Target` / `Outcome` / `OUTCOMES` / `Aimed` / `aimed_of` / `targets_in`。`regex_literal` は群の中だけで使う（private のまま子へ）。
  - **doc の中の参照が 1 つ残る**: `diagnosed`（610 行・親に残る）の doc が `Aimed` を intra-doc link で名指す（609 行）。`Aimed` は歯だけが引く名なので `#[cfg(test)]` の `use` に入り、素の build では親の scope に居ない＝link を子の path 形に書き直す。純移動の機械証明はコメント行を hash から外して差だけを要約に載せる（`crates/scribe2/src/pipe/move_proof.rs` の comment-diff）ので、この 1 行の書き直しは残差にならない。
  - 子が親から引く名（`use super::{…}`）は実測で 10 個: `flag`（6 site）・`write_diff` / `unmeasured` / `judged` / `diagnosed` / `baseline_log_tail`（各 1）・`Counts`（3）・`Scope`（2）と、`emit` と `ExitCode`。子は親の private item を見る（Rust の可視性は module 単位）ので、可視性を上げるのは**子側**の 5 名だけである。
- 形（番号は done と 1:1）:
  1. 上の **15 item（297 行）**を、名・本文・順序を変えずに行 ag の write-set の `+` の file へそのまま移す。
  2. 親に増えるのは `mod` 宣言 1 行と `use` 2 文（本体用 5 名・`#[cfg(test)]` 付き 6 名）だけ。`run` と `judged` と `own_baseline` の本体は 1 字も変わらない。`#[cfg(test)]` の `use` は歯の区間の直前に置き、file の最初の行頭 `#[cfg(test)]` が src の本体の全 item より後に在る状態を保つ。
  3. 歯の区間（`#[test]` 20 本）は 1 本も動かさない。`use super::{…}` の名の列も 1 字も変えない。
  4. 札 `// flip-check: moved s2-07l.198.2` を親の歯の区間の先頭と `+` の file の先頭に対で置く（純移動の機械証明は [pipeline.md](./pipeline.md) §5.3）。
  5. `diagnosed` の doc の intra-doc link を子の path 形に直す（本文の行は 1 字も変えない・コメント行は hash の外）。
  6. 割った後の正規化行数は親が **約 1163**（余地 **約 337**）で、`s2-07l.198.2` の size M（300）を受付が通す。`+` の file は約 300 行。
- 触らない: (1)(2)(3)(5)(6)(7)(8)(9) の残りの群の本体・`measure_args` と `Pace` と `Scope` の再輸出・判定行の 8 token（`total=` から `outside=` まで）の名と順序と書式・rc の極性と `R-C12-1` の読み・baseline の 2 手と timeout の式（§33）・歯の絞りの式（§34）・`crates/xtask/src/main.rs`（この file は歯も本体も 1 字も変わらない＝write-set に入れない）。
- 却下:
  - **歯の区間を名前付きの test file へ外出しする**（`check.rs` が `s2-07l.257` でやった形）: 余地は 438 行増えて一番大きいが、親の歯は inline の `mod tests` に畳まれた **1 item** で、外出し先では列 0 の item が 20 本以上に割れる＝(名, hash) の多重集合が合わず純移動の機械証明が items-differ で落ちる（`crates/scribe2/src/pipe/move_proof.rs` の「inline の `mod tests` の歯は外側の 1 本に畳む」）。`check.rs` の便はこの証明が入る前である。
  - **(9) の走らせる手だけを割る**（230 行）: 余地が 42 + 230 = 272 で size M（300）に届かない。入口の 1 本が子へ移るので親の再輸出も要る。
  - **(4) だけを割り `aimed_run` を親に残す**（258 行）: 余地が約 297 で **300 に 3 行届かない**。責務も 2 つに割れる（的の分類と的の周の後段）。
  - **上限 R-C4-2 の値を上げる**: 値の線と裁定が動く（C5・A2）。file を割れば済む面に閾値を持ち込まない。
  - **`s2-07l.198.2` を size S に書き直す**: size は便の write-set 全体に掛かる見積で、境界 crate の便は core の bin と歯の移動を含む（S では足りない）。
- 歯（**新しい歯は 1 本も足さない**・純移動ゆえ既存の歯が母集団である）: 移した 15 item を測る歯は `mutants_targets_` **5 本**と `mutants_in_diff_` **2 本**（どちらも同 file の歯の区間に在り、`crates/` 全体で他 file に 0 件）。親に残る群を測る歯は `mutants_diff_fail_fast_` **4 本**・`mutants_diff_teeth_` **5 本**・`mutants_diff_outside_` **3 本**（同じく他 file に 0 件）。母集団は同 file の `#[test]` **20 本**で、便の前後で 20 のまま動かない。接頭辞 `no_fail_fast_` は `crates/xtask/src/check_prose_tests.rs` と `crates/xtask/src/flipcheck_tests.rs` にも当たるので検証行には使わない（受付が teeth-outside-write-set で断る）。

### 40.1 errata（現物との差・`s2-07l.549`・規範は上の §40 のまま）

- 札の id: 形 4 は `s2-07l.198.2` を名指すが、札は移した便の bead を名乗る（他の純移動の札と同じ・`s2-07l.198.2` はこの移動を前提にする後続の便で、移動はしない）＝`// flip-check: moved s2-07l.549` を親の歯の区間の先頭と行 ag の write-set の `+` の file の先頭に対で置いた。
- 子側で可視性を上げたのは 2 名（`place_diff` と `aimed_run`・親の本体が呼ぶ 5 名のうち元が private の 2 つ）で、残りの 3 名と歯だけが引く 6 名は元から `pub`。子が親から引く名は 12 個（上の 10 個から `crate::emit` の path 呼びと `ExitCode` の std の use を除き、`parse_outcomes` / `measured` / `without_outcomes` / `outside_token` を足した数）。
- 移した群の doc の intra-doc link のうち親の再輸出を指す 1 つ（`aimed_args` の doc の `measure_args`）も `super::` の path 形に直した（コメント行・hash の外）。

## 41. lens に渡す diff から「rename の対の path 置換だけの docs の hunk」を省く — 移動した file を名指す契約表の行の書き換えが cap を焼き尽くす（契約表の行 ah・`s2-07l.198.2` の便 153839Z の Gated INCONCLUSIVE）

- 出所（orchestrator の実測 2026-09-22 16:0xZ・verified）: `s2-07l.198.2`（境界 crate の新設・core-boundary.md 行 b）の便 153839Z は verify 9/9 rc 0 のまま Gated INCONCLUSIVE（evidence「diff 792551 byte が cap 150000 を超えた」・`# lens-input=diff reason=items-differ`）。diff の内訳は **docs/design の書き換えが 754352 byte・code 側が 38199 byte**。docs 側の変更行は pipeline.md 99・contract-source.md 60・gate-cost.md 56・seat-roles.md 37・dispatcher.md 28・account-lifecycle.md 15・account-autonomy.md 14・consumer-sync.md 11（各 -N/+N で対）で、その 1 行は契約表の 1 row（write-set と done を 1 行に持つ・1 KB 前後）である。書き換えの中身は rename 49 file（tests 45・src 4）の **旧 path → 新 path の置換だけ**（新 path の出現 699 か所が tests/e2e の移動由来）。lens が読む価値の無い機械置換が cap の 5 倍を占め、code 側だけなら cap の 4 分の 1 で収まる。
- 便 161614Z（2026-09-22 16:31Z・Gated FAIL・lens の finding を orchestrator が現物で再現・verified）: 畳みの判定が「置換後の `-` の列 == `+` の列」の 1 条件だけで、置換が 1 か所も効かない hunk（`-X` / context / `+X` の並べ替え）も畳まれて lens から隠れた。上の形 1 の「各行が置換で変わる」「1 塊」の 2 条件はこの便の解。検出線は total=42 caught=4 missed=35（歯 5 本の周）で、歯 (f)(g) はこの生存も削る。
- 現物（main 50832c9・verified）: `crates/scribe2/src/pipe/gate/lens.rs` の `lens_input` は diff の字面と両側の file の読みを `crate::pipe::move_proof` の `judge` へ渡し、`LensInput`（diff か純移動の要約）を返す。`crates/scribe2/src/pipe/gate.rs` の `measure` は生 diff と `LensInput` を `Measured` に持ち、`decide` は `LensInput::body`（diff の周は生 diff そのもの）の byte を rules 行 `gate.token_cap` と比べて超えれば INCONCLUSIVE、通れば同じ本文を lens の stdin へ渡す。`crates/scribe2/src/pipe/gate/record.rs` の `notice_line` は run dir の `verify.stderr.log` に `# lens-input=<kind> reason=<語>` を残し、gate の stdout の判定行は `bytes=` に lens へ渡した本文の byte を出す。`verdict.json` の `diff_bytes` は生 diff の byte（NFR1 の記録）。git の diff は rename を `rename from <旧 path>` / `rename to <新 path>` の header の対で運ぶ（100% の rename は `---` / `+++` を持たない・[pipeline.md](./pipeline.md) §53 の flip-check と同じ根）。
- 形（番号は done と 1:1）:
  1. **pure な 1 本**（置き場は `crates/scribe2/src/pipe/gate/lens.rs`・入力は diff の字面だけ・git を呼ばない）: diff の header から rename の対（旧 path, 新 path）を集め、`+++ b/` 側の path が `docs/design/` 配下の `.md` である file の各 hunk について、`-` 行の列に全ての対の置換（旧 path の字面 → 新 path・長い旧 path から順）を当てた結果が `+` 行の列と**順序も本数も同じ**で、かつ hunk の本文が context・`-` の連続・`+` の連続・context の **1 塊**（`-` と `+` の間に context 行が無い）で、かつ **`-` の各行が置換で 1 字以上変わっている**（置換が効かない行が 1 行でも在れば逐語のまま＝行の並べ替えや同文の消して足すを隠さない）なら、その hunk の本文（`-` / `+` / context の行）を **1 行の印**（`~ rename の置換だけの hunk（-N/+N 行）を省いた`）に置き換える。1 行でも合わなければその hunk は逐語のまま。`diff --git` / `---` / `+++` / `@@` の header は残す。戻りは（省いた後の本文, 省いた hunk 数, 省いた行数）で、行数は省いた hunk の `-` と `+` の行の和（印の -N/+N の和）。rename の対が 0 の diff は本文も件数もそのまま（0/0）。git の「No newline at end of file」の注記行は行に数えず 1 塊も切らない（歯 (g) は末尾の改行だけが違う同文の行を効かない行として 1 塊に置く＝git が同文の `-` / `+` を 1 塊に出す唯一の形）。
  2. `measure` は `LensInput` が diff の周だけこの 1 本を通し、**生 diff と lens 用の本文の両方**を `Measured` に持つ。`decide` の cap の照合と lens の stdin は lens 用の本文で、`verdict.json` の `diff_bytes`・`patch_id`・検出線の持ち越しは生 diff のまま（記録の意味を変えない・NFR1）。純移動の要約の周は 1 字も変わらない。
  3. 通知: 省いた hunk が 1 つ以上の周だけ `verify.stderr.log` の行を `# lens-input=diff reason=<語> elided=<hunk 数>/<行数>` にし、0 の周は従来の字面のまま（既存の歯の pin を動かさない）。gate の stdout の `bytes=` は lens に渡した本文の byte（省いた後）。呼び手の端末へ出す `pipe: lens-input=diff reason=<語>` の 1 行は触らない。
  4. 省く面は `docs/design/` の `.md` に閉じる。code file（`.rs` / `.toml` 等）の同じ置換（`use` / `#[path]` / Cargo の member）は lens が読む対象なので逐語のまま。
  5. 歯（e2e・`crates/scribe2/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_elide_`・既存の記録 lens と rules の写しの fixture を使う）: (a) rename 1 本 + その旧 path を write-set に持つ docs/design の md 1 行の置換 → lens の stdin に印が在り置換後の row が無く、通知に `elided=1/2`、`bytes=` が生 diff より小さく、`verdict.json` の `diff_bytes` は生 diff の byte のまま。(b) 同じ hunk に path 以外の 1 語の差も在る → 逐語のまま・通知に `elided=` が無い。(c) 同じ置換が `.rs` の hunk に在る → 逐語のまま。(d) rename の header が無い diff → 従来の字面と `bytes=` のまま（既存の `assert_sends_diff` 系の歯が母集団）。(e) rules の写しで cap を小さくし、生 diff は cap 超・省いた本文は cap 内 → INCONCLUSIVE でなく lens が呼ばれ verdict が lens の値（本節の出所の形）。(f) rename を含む diff で docs/design の md の hunk が行の並べ替えだけ（`-X` / context / `+X`・置換が効かない）→ 逐語のまま・通知に `elided=` が無い。(g) 置換が効く行と効かない行が同じ hunk に混在 → 逐語のまま。(h) `-` の各行は置換で変わるが `-` と `+` の間に context 行が在る（`-A(旧 path)` / context / `+A(新 path)`＝置換を伴う行の移動）→ 逐語のまま・通知に `elided=` が無い（1 塊の条件だけを単独で測る歯・便 163714Z の審査の finding）。変異の A/B（形 1 と形 4 の条件 1 つに歯 1 本・判定の順は path の絞り → 置換後の一致 → 各行の効き → 1 塊）: `docs/design/` の `.md` の絞りを外すと (c) が落ちる・置換後の一致の判定を常に真にすると (b) が落ちる・常に偽にすると (a)(e) が落ちる・各行の置換の効きの判定を外すと (g) が落ちる・1 塊の判定を外すと (h) が落ちる。(d) と (f) は回帰の歯で単独の変異を持たない（(d) は rename の対が 0 の周の従来の字面と `bytes=`・(f) は便 161614Z の finding の形で、効きと 1 塊の両方を外して初めて落ちる）。
- 触らない: `crate::pipe::move_proof`（`judge` / `NotPure` / 要約の形）・rules 行 `gate.token_cap` の値・機械検証の段と判定順（[pipeline.md](./pipeline.md) §5.3）・検出線（§28 / §40）・lens の起動の形（`{contract}` / `{worktree}` / stdin）・`verdict.json` の key 列。
- 却下:
  - **`gate.token_cap` を一時的に上げる**（`s2-07l.265` / `s2-07l.375` の型・1,000,000）: 値の線と裁定が動く（A2）うえ、gate を通った lens 入力の最大は 116 KB（verdict.json 244 件の実測）で 792 KB を lens が読んだ前例が無い。読めても 754 KB は機械置換で、審査の質を薄める。
  - **docs の path 書き換えを別 PR に割る**: 移動の前に新 path を書けば CI の名の検査が落ち（base に無い file）、後に書けば移動の PR が同じ検査で落ちる＝同じ PR にしか置けない。
  - **`docs/design/` を lens の入力から丸ごと外す**: 契約表の done や § の散文の変更は lens が読む対象（契約適合の材料）。省くのは置換で説明が付く hunk だけに閉じる。
  - **純移動の要約（[pipeline.md](./pipeline.md) §5.3）に docs の置換を載せる**: 要約は item の多重集合が一致した周にしか組めず、本便のように code 側に差が在る周（items-differ）には届かない。

## 42. 畳みの対を hunk の段ごとに取り、移動で空になった dir の対を rename の対から導く — §41 の畳みでも lens 入力が cap の 1.9 倍残った（契約表の行 ai・`s2-07l.198.2` の便 171521Z の Gated INCONCLUSIVE）

- 出所（orchestrator の実測 2026-09-22 17:2xZ・verified）: `s2-07l.198.2` の便 171521Z は行 ah の着地後の器で gate を撃ち、verify 9/9 rc 0・通知 `# lens-input=diff reason=items-differ elided=226/484` のまま Gated INCONCLUSIVE（evidence「diff 281110 byte が cap 150000 を超えた」・生 diff 806111 byte）。畳まれずに残った docs/design の hunk を §41 形 1 の判定を byte 単位で写した写しで数えると（写しの結果は 281562 byte で器の 281110 と一致）、**(A) 1 塊でない hunk が 19**（契約表の隣り合う行が両方書き換わり、間に無変更の行を挟んで `-` `+` context `-` `+` と並ぶ＝git は近い変更を 1 つの hunk に束ねる）、**(B) 置換後の `-` が `+` と一致しない hunk が 13**（散文や write-set が file でなく **dir** を名指す: `crates/scribe2/tests/`・`tests/e2e/`・`tests/e2e/pipe/`・`tests/e2e/seat/`・`tests/e2e/snapshots/`・`src/snapshots/`＝移動で空になった dir で、rename の対には file の path しか無い）、(C) 効きの条件で残った hunk が 1。写しで (A) を許すと 159538 byte・(A)(B) を許すと **86507 byte**（残る docs の hunk は seat-roles.md の 1 つ・13865 byte・code 側は約 19 KB）で cap に収まる。
- 現物（main 155a248・verified）: `crates/scribe2/src/pipe/gate/lens.rs` の畳み（行 ah）は diff の字面だけを読み、rename の対を `rename from` / `rename to` の header から集め、hunk ごとに「path の絞り → 置換後の一致 → 各行の効き → 1 塊」の順で判定する。`crates/scribe2/src/pipe/gate.rs` の `measure` が diff の周だけそれを通し、通知は `crates/scribe2/src/pipe/gate/record.rs` が書く。
- 形（番号は done と 1:1）:
  1. **段ごとの対**（(A) の解）: hunk の本文を「`-` の連続 k 行の直後に `+` の連続 k 行」の**段**に切り、段と段の間と前後に context が在る形（段が 1 つの周は §41 形 1 の 1 塊と同じ）を許す。段ごとに本数が同じで、`-` の各行が置換で 1 字以上変わり、置換後の列が `+` の列と順序も同じなら hunk 全体を 1 行の印に畳む（印の件数は全段の `-` / `+` の合計）。`-` の連続の直後が context（`+` が続かない）か、段の本数が違えば逐語のまま＝§41 の歯 (f)(h) の形は今までどおり逐語。
  2. **移動で空になった dir の対**（(B) の解）: `measure` が HEAD の tracked path の列（`git ls-tree -r --name-only HEAD`・読めない周は空の列＝dir の対を 1 つも足さない fail-closed）を pure な 1 本に渡す。1 本は rename の対 (a, b) ごとに末尾の共通の path component を 1 つずつ剥がした prefix の対 (pa, pb) を**長い pa から順に**見て、(i) HEAD の列に `pa/` で始まる path が 1 つも無く、(ii) `pa/` で始まる旧 path を持つ rename の対が全部 `pb/` + 同じ相対 path へ行く、の両方を満たす間だけ dir の対として足す（どちらかが破れたら、それより短い prefix は見ない）。dir の対は file の対と同じ列に入り、長い旧 path から順に置換する（本便の diff では 6 対: tests・tests/e2e・tests/e2e/pipe・tests/e2e/seat・tests/e2e/snapshots・src/snapshots）。
  3. 通知と判定行の字面は §41 のまま（`elided=<hunk 数>/<行数>`・`bytes=` は畳んだ本文・`diff_bytes` は生 diff）。
  4. 歯（e2e・`crates/scribe2/tests/e2e/pipe/gate.rs`・接頭辞 `pipe_gate_elide_` のまま・§41 の 8 本は 1 字も変えず緑のまま）: (i) 2 段の hunk（`-A` / `+A'` / context / `-B` / `+B'`・どちらの段も置換だけ）→ 畳む・通知に `elided=1/4`。(j) 段の本数が違う（`-A` / `-B` / `+A'`）→ 逐語。(k) rename が tests 配下の file 1 本から boundary 側の同じ相対 path への 1 本で、HEAD に `tests/` 配下の path が無く、docs の行が `tests/` の dir だけを名指す置換 → 畳む。(l) (k) と同じで HEAD に `tests/` 配下の path が 1 つ残る → 逐語（dir の対を足さない）。(m) (k) と同じで `tests/` 配下のもう 1 本の rename が別の dir へ行く → 逐語（一貫しない dir は対にしない）。変異の A/B（判定の順は 段の切り分け → 段ごとの本数 → 一致 → 効き、dir の対の導出は (i) 空 → (ii) 一貫・条件 1 つに歯 1 本）: 段の切り分けを 1 塊に戻すと (i) が落ちる・段ごとの本数の照合を外すと (j) が落ちる・dir の対の導出を外すと (k) が落ちる・(i) 空の条件を外すと (l) が落ちる・(ii) 一貫の条件を外すと (m) が落ちる。§41 の (a)〜(h) は本便の変異のどれでも落ちない回帰の歯。
- 触らない: §41 の path の絞り（`docs/design/` の `.md`）・効きの条件・rules 行 `gate.token_cap`・`crate::pipe::move_proof`・通知の字面・`verdict.json` の key 列・機械検証の段と判定順。
- 却下:
  - **hunk を context 0 で取り直して 1 塊に戻す**: lens に渡す diff の形が変わり、§41 の (f)(h)（context を挟む並べ替え・移動）が測れなくなる。段ごとの対は同じ diff のまま (A) だけを解く。
  - **dir の対を「rename の対の共通 prefix」だけで作る**（HEAD の列を見ない）: `crates/scribe2/` → `crates/scribe2-boundary/` のような広い対が生まれ、移動していない file の path の誤った書き換えまで「置換で説明が付く」と畳んで lens から隠す。空の条件 (i) が要る。
  - **`gate.token_cap` を一時的に上げる**: §41 却下 1 と同じ（A2・前例の最大 169 KB・機械置換で審査を薄める）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "着地の列の歯の fixture が verdict.json を素の write で書く flake — 器の write_verdict と同じ atomic な形（.partial へ書いて rename）にする"
req = ["FR50"]
section = "6"
tests = ["crates/scribe2/src/pipe/queue.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail mutant_in_pipe_land_await_turn_"]
size = "S"
done = "fixture の書き換えが atomic になり、await_turn の poll が書きかけを読む窓が無い（歯の中身と assert は不変・test だけの差分ゆえ札 retroactive）"

[[contract]]
id = "b"
title = "tmux を立てる歯を nextest の test-group で同時本数 = rules 行 gate.tmux_test_threads に絞る — 値の写し（.config/nextest.toml）と配線を xtask check が manifest と突合する"
req = ["FR8", "NFR3"]
section = "3"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/xtask/src/limits.rs", "crates/xtask/src/check.rs", "crates/xtask/src/check_facts.rs", "crates/xtask/src/check_tests.rs", "+.config/nextest.toml", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail nextest_tmux_group_"]
size = "M"
done = "tmux を立てる歯が test-group tmux で同時本数 1 に絞られ、値の写しと配線を cargo xtask check が測り、写しの値違い・file 無し・group の外の tmux 歯・固定形でない filter を名指して落とす"

[[contract]]
id = "c"
title = "runner / lens / claude の箱の上限を 1 × gate.job_memory_mb に揃える（裁定 id user 2026-09-15T18:2xZ・cargo mutants の deny は ADR-0025 / s2-07l.168 で既着）"
req = ["FR46", "NFR3"]
section = "12"
write-set = ["crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/rules.rs", "docs/design/gate-cost.md", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_confine_runner_limit_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_confine_lens_box_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_confine_review_box_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_runner_box_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail hook_command_guard_denies_a_denied_sequence_from_bash", "cargo nextest run -p scribe2 --test e2e --no-tests=fail hook_command_guard_matches_sequence_regardless_of_flag_order", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_embedded_manifest_declares_the_denied_commands_row"]
size = "S"
done = "runner / lens / 審査の lens / claude の包み 4 か所の MemoryMax が 1 × gate.job_memory_mb に揃い、同じ gate の {jobs} を持たない verify 行の箱は MemTotal − host.reserve_memory_mb のまま、runner の雛形に検出線を撃たない 1 行が在り、rules 行と RuleKind の variant は 1 本も増えず、cargo mutants の deny の既着の歯 3 本（hook の語列 2 本と rules 行の 1 本）が行の verify から完全名で撃たれて緑である"

[[contract]]
id = "d"
title = "runner / lens が起こす claude の子の peak memory を record に残す"
req = ["FR46", "NFR3"]
section = "13"
write-set = ["crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_claude_peak_", "cargo nextest run -p scribe2 --lib --no-tests=fail confine_peak_"]
size = "M"
done = "claude の scope の memory.peak が走行中に sample され、scope が終端で消えた周も runner / lens の stderr の scope= の行（Confined の周は gone でも出る）に claude_peak_bytes= で残り、読めない周は 0 でなく - になり、lens の poll は stdout を読み切り、systemd 無しの host では語が出ない"

[[contract]]
id = "e"
title = "純移動と証明された行を検出線の母集団から外す"
req = ["FR8", "NFR3"]
section = "14"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/move_proof.rs", "crates/xtask/src/mutantsdiff.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_detection_pure_move_", "cargo nextest run -p xtask --no-tests=fail mutants_in_diff_", "cargo nextest run -p scribe2 --lib --no-tests=fail move_proof_judge_pins_each_reason", "cargo nextest run -p scribe2 --lib --no-tests=fail move_proof_comment_diff_inside_items_is_counted_and_markers_inside_items_are_checked", "cargo nextest run -p scribe2 --lib --no-tests=fail move_proof_comment_verbatim_is_absent_when_comments_match"]
size = "S"
done = "(1) 一致と証明された item の head 側の行範囲が要約に載り、要約の字面と証明の判定は不変 (2) gate がその行範囲の hunk を落とした diff を run dir の file に組んで検出線の 1 行に --diff <file> で渡し、xtask の mutants-diff はその周は git diff を撃たずその file を母集団にする〔歯 mutants_in_diff_〕 (3) 純移動だけの便は母集団が 0 行になり Check::Detection が赤にも測定未了にもならず、検出線の record に純移動の記号が残る〔pipe_gate_detection_pure_move_〕 (4) 純移動でない便は従来どおり全ての追加行を母集団にする (5) 移動でない追加行（mod 宣言の追加・可視性の変更・残差）は母集団に残る"

[[contract]]
id = "f"
title = "gate の周ごとの検出線の出力を run dir へ写し、show はその写しから読む"
req = ["FR8", "FR22"]
section = "15"
write-set = ["crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__gate__pipe_record_show_external_form.snap", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_detection_copy_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_record_show_external_form"]
size = "S"
done = "検出線の判定行と出力が周ごとに run dir の別の置き場へ残って 2 周目が 1 周目を上書きせず、出力の無い周は marker が・判定行も無い周は不在の 1 行が在って 0 件の周と字面で弁別され、pipe show の判定行の出所が verify.jsonl からその写しへ移って、写しの判定行だけを書き換えた周は show がその字面を出し（verify.jsonl は元のまま）・写しを消した周は show が不在の 1 行を出し（record は在るまま）、数え手は 1 つのままで通常の周の判定行の字面と外形 snapshot は 1 字も変わらない"

[[contract]]
id = "g"
title = "契約が名指した生存行に変異を当てて outcomes の 4 kind（caught / missed / unviable / timeout）+ 不在（absent）の 5 値で記す"
req = ["FR8"]
section = "16"
write-set = ["crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "contracts/schema.toml", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/xtask/src/mutantsdiff.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "docs/design/gate-cost.md", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_targets_", "cargo nextest run -p xtask --no-tests=fail mutants_targets_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_targets_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail contract_schema_matches_the_tracked_file_and_the_field_slice"]
size = "M"
done = "契約表の欄が 1 つ増えて契約が生存行を的として名指せ（欄の無い行は従来どおり通り・形に合わない値は typed に断られ）、歯だけの便でその的が撃たれて caught / missed / unviable / timeout / absent の 5 値（和 = 的の本数）で記録され、現物とずれた的だけが absent に落ち、outcomes を読めない周は 5 値に化けず、欄を持たない便は diff の追加行を母集団にする従来の経路のままで verdict の判定はどちらの周も変わらず、contracts/schema.toml が描き直されて FIELDS と byte で一致する"

[[contract]]
id = "h"
title = "歯の fixture の一時 dir を Drop で必ず片付ける — 作り手が包みを返し、panic した歯も dir を残さない（scope の reset は行 p・疑似 seat は着地済み）"
req = ["NFR3"]
section = "17"
write-set = ["crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/ledger_form.rs", "crates/scribe2/tests/e2e/ledger_memo.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail e2e_fixture_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_isolated_session_is_torn_down_when_guard_drops"]
size = "S"
done = "一時 dir の作り手が Drop で再帰削除する包みを返し、panic した歯も dir を残さず（現物の形では残る）、path を取り出して guard を降ろした周だけ dir が残り、作り手の path の一意性と各歯の assert は不変で、疑似 seat の畳みの既着の歯は緑のまま"

[[contract]]
id = "i"
title = "fleet/store.rs の起動時刻算術（clock tick から ms への変換）に境界と property の歯を足す"
req = ["FR46"]
section = "18"
write-set = ["crates/scribe2/src/fleet/store.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_store_started_ms_"]
size = "S"
done = "started_ms_in の内側の変換に境界と property の歯が在り、挙動を変えない純粋な移動として flip-check は retroactive 札で通る"

[[contract]]
id = "j"
title = "gate の共通 verify・検出線の baseline（cargo mutants の cargo test）・flip-check の base 段を --no-fail-fast にし、落ちた歯の全数を 1 周で名指す"
req = ["FR46"]
section = "19"
write-set = [".vessel.toml", ".github/workflows/ci.yml", "CLAUDE.md", "crates/xtask/src/flipcheck.rs", "crates/xtask/src/flipcheck_tests.rs", "crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/check_prose_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail no_fail_fast_"]
size = "S"
done = "3 面同文の nextest 行と measure_args と nextest_args が --no-fail-fast を持ち、判定の極性と rc の意味は不変"

[[contract]]
id = "k"
title = "e2e/seat.rs の PROMPT_WAIT を 5 s → 60 s（負荷下で tmux fixture の prompt が描かれず ready() が偽で歯が落ちる・歯だけの便・retroactive）"
req = ["FR46"]
section = "20"
write-set = ["crates/scribe2/tests/e2e/seat.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_isolated_session_is_torn_down_when_guard_drops"]
size = "S"
done = "PROMPT_WAIT が 60 秒で、assert の字面・ready() の型・new-session 失敗時の即返しが不変"

[[contract]]
id = "l"
title = "検出線が rc 2（測れなかった）で終えた周は同じ gate の中でその行だけ 1 回撃ち直し、record に retried=1 を残す"
req = ["FR9", "AC3"]
section = "21"
write-set = ["crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_detection_retry_"]
size = "S"
done = "rc 2 → rc 0 の検出線を持つ toy の gate が PASS で終わり record に retried=1 を持ち、rc 2 が 2 回の周は従来どおり INCONCLUSIVE、rc 1 の検出線と共通 verify の rc 2 は撃ち直されない"

[[contract]]
id = "m"
title = "xtask mutants-diff が cargo-mutants の末尾に -- -- --test-threads <cores / jobs> を足し、変異検査の入れ子の並列を core 数に閉じる"
req = ["NFR6"]
section = "22"
write-set = ["crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/main.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail mutants_diff_test_threads_"]
size = "S"
done = "measure_args の末尾が -- -- --test-threads <t> で t = max(1, cores / jobs)、cores が読めない周は 1、既存の対（--in-diff / -p / -o / --jobs）と順序は不変"

[[contract]]
id = "n"
title = "負荷下で落ちる歯 — 列の歯 2 本の壁時計 assert を order の記録の pin に替え、refresh の停止経路の family は起動未達を器が返った後に待たず段 launch を名指し、zombie を残存に数えず、fixture の上限を 15 s にする"
req = ["NFR6"]
section = "23"
write-set = ["crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail gate_flaky_bound_"]
size = "S"
done = "列の歯 2 本が壁時計を持たず、列を通る歯は order_token の first と面 5 の exported_order の first で、面 5 へ書かない pr_cmd の歯は stdout に order= が無いことだけで「待たなかった」を pin し、refresh の helper は器が返った後の起動未達を PID_FILE_WAIT を待たず段 launch と経過・load で落とし、assert_gone は state Z を残存に数えず、REFRESH_TIMEOUT_S は 15、bound の式と器の src は不変"

[[contract]]
id = "o"
title = "host で同時に走る便の本数の最大値 — rules 行 pipe.max_live を足し、受付が live な便を交差と同じ判定で数えて値以上の周は typed に断る（走行中の便は止めない・dispatcher の列の理由は行 a の後）"
req = ["FR68", "FR39"]
section = "24"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/rules-manifest.md", "docs/design/gate-cost.md", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_intake_max_live_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_embedded_manifest_declares_max_live_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_external_form"]
size = "S"
done = "rules 行 pipe.max_live が裁定 id と裁定日つきで 1 本増えて RuleKind の variant と対になり外形の rows= と kinds= が 1 つ増え、live な便が値以上の周の intake は max-live の 1 行（live= と cap= を運ぶ）で断られて run dir も event も増えず、上限で断る周も交差の組は列に並び、live の便を止めれば同じ契約が通り、Gated FAIL の便は数えられず、写しを読めない周は write-set-unreadable（rc 2）で止まる"

[[contract]]
id = "p"
title = "一時 scope を終端で unload する — scope_args に --collect を足し、release は kill の後に reset-failed を 1 回撃つ（Released の 4 値と record の語彙は不変）"
req = ["NFR6"]
section = "25"
write-set = ["crates/scribe2/src/pipe/confine.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail confine_collect_"]
size = "S"
done = "scope_args の列に --collect が 1 回在り、release が kill の後に reset-failed を 1 回撃ち、unit の無い周は Gone のまま record が変わらず、箱の大きさと Released の 4 値は不変"

[[contract]]
id = "q"
title = "gate / land の verify record に段ごとの壁時計 secs を足す — run_line_captured が測り Fired / Step が運び step_record が書く・skip record と write-set 照合は持たない・pipe show の字面と外形 snapshot"
req = ["FR8"]
section = "26"
write-set = ["crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__gate__pipe_record_show_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail gate_secs_", "cargo nextest run -p scribe2 --no-tests=fail gate_secs_"]
size = "S"
done = "撃った段の record 全部に secs が在り skip record と write-set 照合の record には無く、verify-main.jsonl も同じ形で秒を持ち、pipe show が secs= を写し、既存の record の field と rc は不変"

[[contract]]
id = "r"
title = "runner / lens / review の claude の usage を消費の event に記す — result record の usage 4 値と turns / wall_ms を読み、EventKind の variant 1 つと Event の任意 field 1 つ（閉じた型 Cost・出所 3 値）で fleet/events.jsonl に書き、pipe show / pipe report が写す（C6.3 の store は 1 つ・R-C6-1 は別の行）"
req = ["NFR1", "FR21"]
section = "26"
write-set = ["crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/report.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/ledger_memo.rs", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__gate__pipe_record_show_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail run_cost_", "cargo nextest run -p scribe2 --no-tests=fail run_cost_"]
size = "M"
depends = ["q"]
done = "runner の要約行と lens の判定 object が usage 4 値と turns / wall_ms を運び、便 1 本で消費の event が出所ごとに 1 件ずつ fleet/events.jsonl に書かれて token の値が claude の record と一致し、usage の無い周と 6 値のどれかが欠けるか数でない周は event を書かず判定も rc も変わらず、pipe show と pipe report が消費の行を母集団つきで写し、run dir に別 file は増えない"
[[contract]]
id = "s"
title = "主実測は着地する木が gate の verdict の tree と同じ周は全段を撃たず record 1 本（kind=main skipped=main tree=<sha> reason=same-tree）で main-green にする — 違う周と tree の無い周は従来どおり全段（ADR-0043・ADR-0021 §2.4 の部分 supersede・.457 Landed 後）"
req = ["FR34", "FR12", "FR50"]
section = "27"
write-set = ["crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_main_same_tree_", "cargo nextest run -p scribe2 --lib --no-tests=fail main_skip_record_", "cargo nextest run -p scribe2 --no-tests=fail pipe_detection_land_skips_detection_when_tree_matches", "cargo nextest run -p scribe2 --no-tests=fail pipe_detection_scope_same_tree_records_reason"]
size = "S"
done = "着地する木が verdict の tree と同じ周は verify-main.jsonl が skip record 1 本で verify の cmd が 1 本も走らず Landed の形は不変、違う周と tree の無い周は従来の段数で撃ち、skip record は木を必ず持つ"

[[contract]]
id = "t"
title = "赤い行が在る周は検出線が測れなくても FAIL に着く — 判定の順を 1 か所入れ替える（検出線の rc 2 が INCONCLUSIVE に倒すのは赤が 0 の周だけ・赤の数え方は変えない）"
req = ["FR9", "AC3", "FR14"]
section = "28"
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_red_wins_over_detection_"]
size = "S"
done = "共通 verify が赤で検出線が rc 2 の便が Gated verdict=FAIL に着いて evidence が赤い行の数を持ち lens は呼ばれず、契約 verify が赤で検出線が rc 2 の便も同じく FAIL に着き、赤が 0 で検出線が rc 2 の便は今までどおり INCONCLUSIVE のままで、赤の数え方（検出線の rc 1 も赤）と diff の path を読めない周と行が scope の中で殺された周の順は変わらない"

[[contract]]
id = "u"
title = "審査役の出力の形が読めなかった周は同じ gate の中で lens を 1 回だけ撃ち直す — 集計の読みの理由を「形が読めない」と「読めたが規則で断った（母集団 0）」の 2 値に割り、前者だけ撃ち直す（箱の中の死・rc 非 0・起動の失敗は撃ち直さない・1 回目の理由は stderr の行・record の field も verdict.json の schema も足さない）"
req = ["FR9", "FR14", "NFR4"]
section = "29"
write-set = ["crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/gate/findings.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/gate.rs"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_lens_reread_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_findings_missing_population_is_inconclusive", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_findings_zero_population_is_inconclusive"]
size = "S"
done = "撃たれた回数を数える偽の審査役で、1 回目が数でない母集団・2 回目が正しい出力の便が PASS で終わって回数が 2 になり stderr に 1 回目の理由を持つ撃ち直しの 1 行が出て、2 回とも数でない周は INCONCLUSIVE で回数が 2（3 回目は無い）で理由が 2 回目のものになり、母集団が 0 の出力と rc が非 0 で終わる審査役と起動できない審査役と審査役が自分で INCONCLUSIVE を答えた周は撃ち直されず（回数 1・母集団 = 撃ち直さない 4 形）、母集団の欄が無い周と母集団が 0 の周の判定と理由の字面は 1 字も変わらず、Verdict の 3 値と rc と集計の 8 category と判定順と検出線の撃ち直しと verdict.json の field は変わらない"

[[contract]]
id = "v"
title = "e2e の歯の道具箱に偽 systemd-run を標準で置く — 歯が toy repo で実 binary を撃つ PATH の組み立てを統合 test の共有 module の 1 関数に寄せ、3 つの口を全部そこへ通し、4 本に重複した偽の script を 1 つの生成関数に統一する（src は 1 行も触らない歯だけの便・retroactive）"
req = ["NFR6", "FR46"]
section = "30"
write-set = ["crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/pipe/land.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/launch_failure.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/headless.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail e2e_toolbox_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_confine_release_regate_in_one_process_uses_distinct_unit_names", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_confine_release_gone_leaves_no_scope_field", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_confine_falls_back_to_the_plain_shell_without_the_tool", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_spawn_terminal_reason_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail headless_claude_peak_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_preflight_without_state_dir_marks_overlap_unmeasured"]
size = "M"
done = "歯が toy repo で実 binary を撃つときの PATH の組み立てが統合 test の共有 module の 1 関数に寄って偽 systemd-run と偽 systemctl を既定で先頭に積み、pipe の口 183 か所のうち --state-dir の値を持つ 173 か所は撃つ argv の置き場から（値を持たない 10 か所は段に届かないので host の PATH のまま撃つ）・headless の口 11 か所は呼び手の fixture の dir から・直起動の 6 か所はその 2 つのどちらかを通ってその PATH で撃たれ、4 本に重複していた偽 systemd-run の script が 1 つの生成関数から出て記録の読み手が記録 dir の走査に揃い、同じ名の 2 本目を断る性質と包めた周の片付けの記録は不変で、包めない host を作る口と PATH を明示する口も不変ゆえ縮退の歯が緑のまま、実物を使う opt-in の口は作られず、既存の歯の総数と既存の各歯の assert は 1 字も動かず（増えるのは接頭辞 e2e_toolbox_ の道具箱の歯 (a)〜(d) だけ）、intake の歯の file は verify の置き場として write-set に在るだけで diff 0 行である"

[[contract]]
id = "w"
title = "受付に CPU の次元 — 枠を by_avail / by_token / by_cpu の 3 項の min にし、job 1 つの thread の値段（cores / gate.mutants_jobs）を器が決めて 3 つ目の穴で行へ渡す（縮退と測れない周は jobs 1 かつ thread 1・xtask は値を持たない）"
req = ["NFR6", "FR46"]
section = "31"
write-set = ["crates/scribe2/src/pipe/admission.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/fleet/wait.rs", "crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/main.rs", ".vessel.toml", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail admission_cpu_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_threads_hole_", "cargo nextest run -p xtask --no-tests=fail mutants_diff_threads_flag_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_slots_threads_", "cargo nextest run -p scribe2 --lib --no-tests=fail admission_capacity_takes_the_min_of_the_two_formulas", "cargo nextest run -p scribe2 --lib --no-tests=fail admission_capacity_floors_at_zero", "cargo nextest run -p scribe2 --lib --no-tests=fail admission_capacity_is_unmeasured_on_unreadable_meminfo"]
size = "M"
done = "受付が配る枠が by_avail / by_token / by_cpu の 3 項の min になり、CPU 側で決まる fixture と memory 側で決まる fixture の両方が正しく、job 1 つの値段が max(1, floor(cores / gate.mutants_jobs)) の pure 関数 1 本で出て cores は実測（env を読まず rules 行も増えない）、受け付けた枠が jobs と thread を対で運んで縮退の周と cores を読めない周はどちらも jobs 1 かつ thread 1 になり、cores を読めない周の理由が受付の閉じた enum に 1 つ増えて record の slot_why= に固定の字面で残り、宣言の置ける穴が 3 つちょうどになって検出線の行が置換後に実効 jobs と実効 thread を両方持ち、xtask は受けた値を -- -- --test-threads へそのまま渡すだけで cores からの導出を持たず（渡されない周と数でない周は 1・既存の引数の対と -- が 2 つの形は不変）、枠が空くのを待つ完了 enum が CPU の材料も運んで待ちの観測が受付と同じ 3 項を測り、memory の 2 項を測る既存の歯 3 本（capacity の min / 0 の床 / 読めない meminfo）が 1 字も変わらずに緑である"

[[contract]]
id = "x"
title = "器の健康の遮断器 — 行を撃つ前に走行可能（/proc/loadavg の 4 番目の欄の分子）と待ち（procs_blocked）を読み、core あたりの倍率 2 本（rules 行 host.runnable_per_core = 4 / host.blocked_per_core = 1・裁定 id user 2026-09-20T15:23Z）を超えた周は空くまで待ち、gate.slot_wait_s を超えた周は行を撃たずに閉じた理由 1 つで INCONCLUSIVE（FAIL で終端させない・測れない周は待たずに進む）"
req = ["NFR6", "FR46"]
section = "32"
write-set = ["+crates/scribe2/src/pipe/health.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/polarity.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/rules-manifest.md", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail health_judge_", "cargo nextest run -p scribe2 --lib --no-tests=fail gate_busy_order_", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_wait_health_variant_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_health_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_embedded_manifest_declares_host_health_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_gate_health_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail polarity_external_form"]
size = "M"
depends = ["w"]
done = "host の健康を字面から判じる pure 関数 1 本が空いている / 混んでいる / 測れないの 3 値を返して走行可能を 4 番目の欄の分子から読み（分母に釣られず）待ちを procs_blocked の行から読み、閾値 = 倍率 × 実測の core 数でちょうどの値は混んでいるでなく、片方だけ超えた 2 形はどちらも混んでいるで、空・数でない・行が無いの 3 形はどれも測れないになり、行を撃つ前の待ちが完了 enum の variant 1 つ（pid を見張らない側で pid() が 0・in-file の歯で測る）で唯一の待機実装を通り、gate.slot_wait_s を超えた周は verify の行が 1 本も撃たれず record に閉じた印が任意 field で載って判定が箱の中で死んだの直後にその印を読み赤が 1 行在っても INCONCLUSIVE になって便が Gated に留まり（FAIL で終端しない）、印が無い周の判定順は 1 字も変わらず、どちらかの面を読めない周と数でない周は 3 値から行動と字面を出す pure 関数で撃つ側に落ちて record の字面が測れない（空でも 0 でもなく・混んでいるだけが待つ側）になり、倍率の rules 行 2 本（host.runnable_per_core = 4 / host.blocked_per_core = 1）が裁定 id user 2026-09-20T15:23Z と裁定日 2026-09-20 つきで増えて RuleKind の variant HostRunnablePerCore / HostBlockedPerCore と対になり外形の rows= と kinds= が 2 つずつ増え、極性一覧に in-loop / fail-open の guard が 1 つ gate の機械検証の段の直前に増えて外形の集計行の 4 数が動き、land の主実測も同じ 1 本を通る"

[[contract]]
id = "y"
title = "変異検査は baseline だけ道具の外で全数走らせ（--no-fail-fast・自前の baseline.log）、mutant の cargo test は fail-fast にして最初に落ちた binary で打ち切る（--baseline skip・撃墜 1 本の費用を 6 分から秒へ）"
req = ["NFR6", "FR46"]
section = "33"
write-set = ["crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/main.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail mutants_diff_fail_fast_", "cargo nextest run -p xtask --no-tests=fail no_fail_fast_", "cargo nextest run -p xtask --no-tests=fail mutants_diff_threads_flag_"]
size = "S"
done = "mutants-diff が cargo-mutants の前に同じ木で cargo test -p <scope> --no-run（build）と cargo test -p <scope> --no-fail-fast -- --test-threads <t>（test・壁時計の秒を測る）を 1 回ずつ撃って test の出力を作業 dir の baseline.log へ写し、rc ≠ 0 の周は cargo-mutants を起こさず従来と同じ rc 2 で終えて理由行の後ろに自前の baseline.log の末尾を添え、rc 0 の周は test の壁時計をミリ秒の整数で測り T = max(20, ceil(5 × ms / 1000)) を整数演算の pure 関数で導いて baseline.log の末尾に timeout=<T> の 1 行を足し cargo mutants --in-diff … -p <scope> --no-shuffle --copy-vcs true -o <out> --jobs <j> --baseline skip --timeout <T> -- -- --test-threads <t> を撃ち（--no-fail-fast は無い・build の timeout は渡さない）、outcomes.json の 5 数の読みと R-C12-1 の極性と record の 1 行の字面と --jobs / --test-threads の受け方は 1 字も変わらず、baseline の引数と mutant の引数と timeout の式（3000 ms で 20・4020 ms で 21・62000 ms で 310 の 3 点＝床・切り上げ・倍率）と baseline の rc の判定（rc ≠ 0 で Err の字面に自前の log の末尾が載り、rc 0 で Ok の対）が in-file の pure 関数の歯で pin されて行 j の歯は baseline の引数を pin する形で名を保つ"

[[contract]]
id = "z"
title = "xtask mutants-diff が --teeth <語列> を受け、受けた周の mutant の test を nextest の filter（kind(lib) | kind(bin) | test(/^(語…)/)）で回して記録の 1 行に teeth=<-|n> を足す（--teeth 無しの周は §33 の形を 1 字も変えない・語の形が悪い周は rc 2）"
req = ["NFR6", "FR46"]
section = "34"
write-set = ["crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/main.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail mutants_diff_teeth_", "cargo nextest run -p xtask --no-tests=fail mutants_diff_threads_flag_", "cargo nextest run -p xtask --no-tests=fail mutants_diff_fail_fast_"]
size = "S"
done = "mutants-diff が --teeth <語列>（, 区切り・語は英数字と _ だけ・- は空）を受け、受けた周は cargo mutants --in-diff … --jobs <j> --baseline skip --timeout <T> --test-tool nextest -- -E <式> --test-threads <t> を撃ち（式は kind(lib) | kind(bin) に語が在れば | test(/^(語1|語2)/) を足し、2 つ目の -- は無い）、--teeth 無しの周は §33 の引数と 1 語も違わず、語に英数字と _ 以外を含む周は pure 関数の Err で rc 2 の測れていないに倒れ、記録の 1 行が末尾に teeth=<-|n>（無し = -・空 = 0・語の数）を運んで 5 数と scope= と R-C12-1 の極性と baseline の形と timeout の導出は 1 字も変わらず、引数の 3 形（語あり・空・無し）と Err と teeth= の 3 形が in-file の pure 関数の歯で pin されて行 m の歯は --teeth の有無で末尾の 2 形を対で pin する形で名を保つ"

[[contract]]
id = "aa"
title = "宣言の検出線の穴に {teeth} を足し（閉じた 4 つ・末尾）、gate と land の主実測が契約の verify 行から nextest_filter で語を取って , で結んで置き（0 本は -）、.vessel.toml の検出線に --teeth {teeth} を足す（契約の verify 行には置けない・着地は PATH の器の入れ替えを伴う）"
req = ["NFR6", "FR46", "NFR4"]
section = "34"
write-set = [".vessel.toml", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/src/pipe/closure/derive.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail declaration_teeth_hole_", "cargo nextest run -p scribe2 --lib --no-tests=fail gate_fill_teeth_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_teeth_"]
size = "M"
depends = ["z"]
done = "BASE_HOLES が {base} {jobs} {threads} {teeth} の閉じた 4 つ（末尾が {teeth}）になって intake の unfit は検出線の {teeth} を通し契約の verify 行の {teeth} を断り、gate と land の主実測が Checks の契約の verify 行から nextest_filter で filter の語を取り宣言順に , で結んで {teeth} に置き（filter を持たない行は飛ばし・0 本は -）、.vessel.toml の検出線が cargo xtask mutants-diff --base {base} --jobs {jobs} --threads {threads} --teeth {teeth} の 1 行になり、toy repo の e2e で撃たれた行の record に契約の語が , で結ばれて載って {teeth} の字面が残らず、語を環境変数で渡さず、穴の 4 つと unfit の対と置換の 4 形（2 本・飛ばし・0 本・宣言順）が in-file の歯で pin される"

[[contract]]
id = "ab"
title = "rules 行 gate.slot_wait_s の value を 900 → 5400 にし、ruling / ruled_at を user 2026-09-21T09:41Z の裁定に更新する（歯の pin の該当行だけ変える・新しい歯は無い）"
req = ["NFR6", "FR46"]
section = "35"
write-set = ["rules/manifest.toml", "crates/scribe2/tests/e2e/rules.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_embedded_manifest_declares_the_gate_cost_rows"]
size = "S"
done = "rules 行 gate.slot_wait_s の value が 5400・ruling が user 2026-09-21T09:41Z・ruled_at が 2026-09-21 になり、歯の pin の表の該当行が同じ 3 値で緑・他の 5 行と外形 snapshot は不変"

[[contract]]
id = "ac"
title = "歯の道具箱の偽 binary を、同じ dir に一時の名で書いて実行権を付けてから rename で入れ替える — 走っている process が握っている本体を切り詰めず、明示の 4 つの口は呼出の字面を 1 か所も変えずに直る（器の src は 1 行も触らない歯だけの便・retroactive）"
req = ["NFR6", "FR46"]
section = "36"
write-set = ["crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/pipe.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail e2e_shim_atomic_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail e2e_toolbox_run_pipe_confines_the_common_verify_line"]
size = "S"
done = "(1) 道具箱の偽 systemd-run と偽 systemctl の入れ替えが、目的の名と同じ dir の一時の名へ本文を書いて実行権を付けてから rename する 1 つの手を通り、同じ bin dir へ記録 dir だけ替えて 2 度置いた周に 1 度目の本体へ張った hard link の本文が 1 度目の記録 dir を名指したまま 2 度目の字面を持たず、2 度目の後の偽 systemd-run を直に撃つと rc 0 で記録が 1 件増える (2) 2 度組んだ後の偽 systemd-run と偽 systemctl の inode 番号が 2 本とも変わり、明示の 4 つの口の呼出の字面は 1 か所も動かない (3) 2 度組んだ後の bin dir の entry がその 2 件ちょうど（一時の名の残骸 0）で、偽の script の本文と同じ名の 2 本目を断る性質と記録 dir の名と読み手と PATH の組み方と 3 つの口は 1 字も変わらず、既存の歯の総数と既存の assert も不変"

[[contract]]
id = "ad"
title = "歯の道具箱に台帳 client の既定名の見張りを 1 本置き、argv を 1 起動 1 file で記録してから台帳を解けない host と同じ形で断る — PATH に倒れた起動が実 client へ届かず、届きにいった回数が記録の件数で読める（器の src は 1 行も触らない歯だけの便・retroactive）"
req = ["FR50", "NFR6"]
section = "37"
write-set = ["crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail e2e_ledger_tripwire_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_terminal_dispatch_manual_turn_starts_the_runs_it_can"]
size = "S"
done = "(1) 歯の道具箱（tests/e2e/main.rs の toolbox_path）が偽 systemd-run と偽 systemctl に並べて台帳 client の既定名（seat/ledger.rs の DEFAULT_BD）の偽物を置き、呼ばれた argv を 1 起動 1 file で記録 dir へ写してから標準出力を空にして非 0 の rc で断る (2) --repo と --runner と台帳の待ち上限の行を持つ写しつきで --bd を渡さずに撃った列の 1 周が見張りの記録をちょうど 1 件残してその本文が読みの引数を持ち、--bd に偽 client の絶対 path を渡した周は見張りの記録が 0 件のまま偽 client 側に呼出が残り、既存の assert は動かない (3) helper 経由で 1 便を intake から land まで通した周は見張りの記録が 0 件で同じ便の道具箱の systemd-run の記録が 1 件以上在り、器の src と PATH の組み方と 3 つの口と明示の 4 つの口と --bd を渡す既存の起動と既存の歯の総数は 1 字も変わらない"
[[contract]]
id = "ae"
title = "止める口の終端で、作り手の process が死んでいる席の scope を畳む — unit 名の末尾から 2 番目の pid を pure に読み、active な scope の一覧から死んだ作り手のものだけを既存の片付け 1 本で kill し、畳んだ数と一覧の件数を行に出す（測れなかった周は 0 と融合しない）"
req = ["NFR6", "FR46"]
section = "38"
write-set = ["crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail pipe_scope_reap_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_scope_reap_"]
size = "S"
done = "(1) unit 名から作り手の pid を読む pure な 1 本が、素直な名と記号を畳んだ名の 2 形で末尾から 2 番目を返し、割れ数が足りない名と数でない名で None を返す (2) active な scope の一覧を取る呼出が --user list-units で始まり末尾の pattern を器の名の定数から導き、rc 非 0 の答えが空の一覧と弁別される測れなかった値になる (3) 一覧の行と生きている pid の集合（store.rs の started_ms が Started を返す pid・Unreadable は生きている側）から、死んだ作り手の unit だけが畳む相手に残り、生きた作り手の行と形に合わない行は残らない (4) 止める口が席の signal と運転手の後・RunStopped の前に、残った unit を既存の片付けで 1 本ずつ畳み、verify 行ごとの片付けの kill と reset-failed の並びは 1 字も動かない (5) 偽 systemctl が active な scope を 2 本（生きた pid と死んだ pid）返す周に、止める口の後の kill が死んだ側だけに 1 回・生きた側に 0 回で、行の末尾が scopes=1/2 になり rc と既存の token は不変"

[[contract]]
id = "af"
title = "検出線の判定行の末尾に、差分が触れた member のうち測る範囲の外の dir を root 相対で名指す 1 語を足す — 母集団 0 と生存 0 を字面で弁別し、既存 7 token と 5 数の読みと週次の記録の極性は 1 字も変えない"
req = ["NFR6", "FR46"]
section = "39"
write-set = ["crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/main.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail mutants_diff_outside_"]
size = "S"
done = "(1) 差分の本文と member の dir の列から触れた member の dir を root 相対・宣言順・重複無しで返す pure な 1 本が在り、member のどれにも属さない path を数えず、同じ member の 2 file を 1 件に畳む (2) その結果から core の dir を除いた残りが、core だけ・task runner だけ・両方・member の外だけの 4 形で順に 0 件・1 件・1 件・0 件になる (3) 判定行の末尾が outside=- と outside= + dir 1 件の 2 形で出て、先頭の 7 token（total= から teeth= まで）が 2 形とも 1 字も変わらない (4) 5 数の読みと rc と -p に渡す名と baseline の 2 手と歯の絞りの式と宣言 file の 4 つの穴は 1 字も変わらない"
[[contract]]
id = "ag"
title = "検出線の走らせる手から「的を絞った周」の 15 item（297 行）を子 module へ割る — 純移動・歯は 1 本も動かさず親に増えるのは mod 1 行と use 2 文だけ・親の余地を 42 から 300 以上へ戻す"
req = ["NFR6"]
section = "40"
write-set = ["-crates/xtask/src/mutantsdiff.rs", "+crates/xtask/src/mutantsdiff/aimed.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail mutants_targets_ mutants_in_diff_", "cargo nextest run -p xtask --no-tests=fail mutants_diff_fail_fast_ mutants_diff_teeth_ mutants_diff_outside_"]
size = "S"
done = "(1) 的を絞った周の 15 item（的の型・5 値・的ごとの分類・的の一覧の読み・差分の置き・引数の組み立て・的の周の後段）が + の file に名・本文・順序のまま在り、純移動の機械証明の残差が 0 (2) 親に増えた item は mod 宣言 1 つと use 文 2 つ（本体が呼ぶ 5 名の素の use と、歯だけが引く 6 名の #[cfg(test)] 付き use）だけで、run と judged と own_baseline の本体は 1 字も変わらず、#[cfg(test)] の use は歯の区間の直前に在って file の最初の行頭 #[cfg(test)] が src の本体の全 item より後に在り、cfg(test) の無い build で unused_imports が 0 件 (3) 歯の区間の #[test] が便の前後とも 20 本で 1 本も動かず、use super の名の列が 1 字も変わらず、mutants_targets_ の 5 本と mutants_in_diff_ の 2 本と mutants_diff_fail_fast_ の 4 本と mutants_diff_teeth_ の 5 本と mutants_diff_outside_ の 3 本が全部緑 (4) 札 flip-check: moved が親の歯の区間の先頭と + の file の先頭に対で在り (5) 親に残る diagnosed の doc の参照が子の path 形になり、本文の行は 1 字も変わらない (6) 親の正規化行数が 1458 から 1163 前後へ落ちて余地が 300 以上になり、判定行の 8 token と rc の極性は 1 字も変わらない"
[[contract]]
id = "ah"
title = "lens に渡す diff から rename の対の path 置換だけの docs/design の hunk を 1 行の印に畳む — 生 diff の記録（diff_bytes / patch_id）は変えず、cap の照合と lens の stdin だけ畳んだ本文にし、省いた周は通知に elided=<hunk 数>/<行数> を残す"
req = ["FR9", "NFR1"]
section = "41"
write-set = ["crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_elide_"]
size = "S"
done = "(1) diff の字面だけを読む pure な 1 本が、rename の header の対を集め、docs/design 配下の .md の hunk のうち - 行の列に置換を当てた結果が + 行の列と順序も本数も同じで、本文が context・- の連続・+ の連続・context の 1 塊で、- の各行が置換で 1 字以上変わるものだけを 1 行の印に置き換え、（本文, hunk 数, 行数）を返し、対が 0 の diff は 0/0 で本文そのまま (2) measure が diff の周だけそれを通して生 diff と lens 用の本文の両方を持ち、cap の照合と lens の stdin は lens 用の本文、verdict.json の diff_bytes と patch_id と検出線の持ち越しは生 diff のままで、純移動の要約の周は 1 字も変わらない (3) 省いた hunk が 1 つ以上の周だけ verify.stderr.log の行に elided=<hunk 数>/<行数> が付き、0 の周は従来の字面のままで既存の歯が全部緑、判定行の bytes= は畳んだ本文の byte (4) .rs の hunk の同じ置換は逐語のまま渡る (5) pipe_gate_elide_ の歯 8 本〔(a) 置換だけの docs は畳む・(b) 1 語の差が在れば逐語・(c) .rs は逐語・(d) rename 無しは従来の字面・(e) 生 diff が cap 超でも畳んだ本文が cap 内なら lens が呼ばれる・(f) 置換が効かない並べ替えだけの hunk は逐語・(g) 効く行と効かない行の混在は逐語・(h) 各行は置換で変わるが - と + の間に context が在る hunk は逐語〕が緑で、docs/design の .md の絞りを外すと (c) が、置換後の一致の判定を常に真にすると (b) が、常に偽にすると (a)(e) が、各行の置換の効きの判定を外すと (g) が、1 塊の判定を外すと (h) が落ち、(d)(f) は単独の変異を持たない回帰の歯"

[[contract]]
id = "ai"
title = "lens 入力の畳みを hunk の段ごとに取り（- の連続 k 行の直後の + の連続 k 行を段とし、段の間の context を許す）、移動で空になった dir の対（HEAD に配下の path が無く、配下の rename が全部同じ dir へ行く prefix）を rename の対から導いて置換の列に足す"
req = ["FR9", "NFR1"]
section = "42"
write-set = ["crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_gate_elide_"]
size = "S"
done = "(1) hunk の本文を - の連続 k 行の直後の + の連続 k 行の段に切り、段の間と前後の context を許し、段ごとに本数が同じで - の各行が置換で 1 字以上変わり置換後の列が + の列と同じなら hunk 全体を 1 行の印に畳み（件数は全段の合計）、- の連続の直後が context か段の本数が違えば逐語のまま (2) measure が HEAD の tracked path の列を渡し（読めない周は空の列）、pure な 1 本が rename の対ごとに末尾の共通 component を剥がした prefix の対を長い方から見て、HEAD に配下の path が無く配下の rename が全部同じ dir へ行く間だけ dir の対として足し、file の対と同じ列で長い旧 path から順に置換する (3) 通知の elided=<hunk 数>/<行数> と bytes= と diff_bytes の字面と意味は §41 のまま (4) §41 の歯 8 本は 1 字も変えず緑のまま、新しい歯 (i) 2 段の hunk は畳む・(j) 段の本数が違えば逐語・(k) 空になった dir を名指す置換は畳む・(l) 配下に path が残る dir は対にしない・(m) 配下の rename が別の dir へ行く dir は対にしない、の 5 本が緑で、段の切り分けを 1 塊に戻すと (i) が、段ごとの本数の照合を外すと (j) が、dir の対の導出を外すと (k) が、空の条件を外すと (l) が、一貫の条件を外すと (m) が落ち、(a)〜(h) はどの変異でも落ちない回帰の歯"

<!-- contracts:end -->
