# 設計: gate の費用構造 — 変異の並列度は project 横断（host 単位）の受付の実測で決め、器の子 process は cgroup で封じ、木が同じ main 実測は検出線を撃ち直さず、着地は gate 済みの便を先に通す

- 要件: [FR8](../../design-intent/spec/srs.html#FR8) [FR9](../../design-intent/spec/srs.html#FR9) gate の機械検証と lens / [FR10](../../design-intent/spec/srs.html#FR10) [FR11](../../design-intent/spec/srs.html#FR11) land の前提と squash / [FR34](../../design-intent/spec/srs.html#FR34) 追随 / [NFR1](../../design-intent/spec/srs.html#NFR1) lens 予算 / [NFR4](../../design-intent/spec/srs.html#NFR4) 読めない store は rc 2。host の資源を枯渇させない要件は SRS に無く、v0.7 の材料（planner state dir・NFR 候補）として user の手番に出す。
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) / [C5](../../design-intent/spec/constitution.html#c5) 値は rules 行・裁定 id 付き / [C3.4](../../design-intent/spec/constitution.html#c3) 待ちは完了 enum の 1 実装 / [C2.2](../../design-intent/spec/constitution.html#c2) env を読まない・置き場は NAME から導く / [C6](../../design-intent/spec/constitution.html#c6) 起動口は 1 つ・Budget は Precheck から / [C10](../../design-intent/spec/constitution.html#c10) 宣言値・測定値・実効値を型で分ける / [C11.2](../../design-intent/spec/constitution.html#c11) 境界は極性を持つ / [C12.4](../../design-intent/spec/constitution.html#c12) 変異の生存は検出線 / [C12.6](../../design-intent/spec/constitution.html#c12) main は常に緑 / [N1](../../design-intent/spec/constitution.html#n1) 不可逆に消さない。
- 決定: [ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html)（本 doc の決定の正本・ADR-0009 §2.3 / §2.4 と ADR-0010 §2.1 / §2.3 / §2.4 / §2.5 を §2.6 で部分 supersede）/ [ADR-0009](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html) §2.3 / §2.4（変異検査と共通 verify の順序）/ [ADR-0010](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html)（宣言 file）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)（極性一覧）/ [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)（追随）。
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
| `pipe.max_live` | `PipeMaxLive`（Int） | host で同時に走る便（live な便）の本数の**最大値**（[ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)・値は user 裁定 id 付き）。受付が便を作る前に live な便を数え、値以上の周は typed に断る（§24）。変異検査の並列度（`gate.mutants_jobs`）や memory の枠（§3.2）とは別の軸で、走行中の便には効かない。 |

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
- **`slot=` の値**: `granted` / `degraded` / `unmeasured`、回収が在った周は `reclaimed:<n>`（枠を配れた周）か `<degraded|unmeasured>,reclaimed:<n>`（縮退と重なった周）。測れなかった理由は閉じた enum で `slot_why=<slots-dir|lock|meminfo>` に残す。meminfo が読めない周は札を回収しない（回収の数を残す前に縮退するため）。縮退（`degraded`）の周も 1 枠の札を置く。
- **包めない周（`Unconfined`）は 1 枠だけを取りにいく**（札は置く）。箱の無い行に並列度を上げると、溢れたときに殺されるのが席の側になる。
- **受付を通るのは gate の共通 verify の `{jobs}` 行だけ**。land の main 実測（`run_checks`・land.rs）は受付を持たず `jobs = 1` のまま撃つ（gate.rs `UNADMITTED_JOBS`・§3.3 の errata の `EFFECTIVE_JOBS` の改名）。main 実測の検出線は (c) で撃たなくなる。
- **受付の 4 行（`gate.mutants_jobs` / `gate.job_memory_mb` / `host.reserve_memory_mb` / `gate.slot_wait_s`）は `--rules` の manifest から読む**（pipe/cli.rs `limits_of`）。封じ込めの 3 線（§4.4・埋め込みだけ）と読み面が違うのは、待ちの上限を振る歯の fixture が gate へ届く口がここだけだからである。

### 3.3 実効 jobs の渡し方

- 宣言 file の共通 verify と検出線の穴を `{base}` と **`{jobs}`** の 2 つにする（declaration.rs `Holes::Base` → 穴の列挙を「宣言の行に置ける穴」の閉じた集合にする・ADR-0010 §2.1 の部分 supersede）。scribe2 自身の宣言は `cargo xtask mutants-diff --base {base} --jobs {jobs}`。
- gate は受付で得た jobs を `{jobs}` に置換して撃つ。`{jobs}` を持たない行は受付を通らない（枠を取らない＝mutants を持たない consumer は費用を払わない）。
- xtask `mutants-diff` は `--jobs N` を cargo-mutants の `--jobs` にそのまま渡す（値は持たない）。
- env で渡さない（C2.2 の精神・折り返しの裏口を作らない）。
- errata（s2-07l.157 の現物）: 置ける穴は declaration.rs の**閉じた集合**（`BASE_HOLES` = `{base}` `{jobs}`）1 本が持ち、`unfit` の判定と gate の置換が同じ列を読む（片側だけに足すと、intake を通った行が穴のまま撃たれる）。受付が入るまでの実効 jobs は gate.rs の `EFFECTIVE_JOBS = 1`（§9 (a)）で、xtask 側の既定も 1（`--jobs` 無し・読めない字面・0 は 1 へ落とす＝道具に「速い既定」を持たせない）。

## 4. 封じ込め（ADR-0021 §2.2）

### 4.1 何を封じるか

器が起こす子 process の起動点は 4 つ（gate.rs `run_line_captured`〔verify 行・gate と main 実測の共通の 1 本〕・spawn.rs `launch_runner`〔runner〕・headless/mod.rs `build`〔claude = runner と lens〕・land.rs〔`--pr-cmd`〕）。verify 行と runner / lens の 3 つを scope に入れる（`--pr-cmd` は host の gh を呼ぶだけで軽い・射程外）。

### 4.2 形

- `systemd-run --user --scope --quiet --unit=<NAME>-<run>-<段>-<n> -p MemoryMax=<上限> -p CPUWeight=<gate.cpu_weight> -p OOMPolicy=continue -- sh -c <line>`。`MemoryHigh` は付けない（係数を持たない・rules 行を増やさない）。`OOMPolicy=continue` は包みを systemd の OOM 停止から外すため（既定の `stop` では kernel が箱の中の 1 process を殺した直後に unit ごと止められ、包みが終端行を出す前に SIGTERM で死ぬ・本 host 実測 2026-09-12: 既定で 3 回中 1 回が終端行なし、continue で 3/3 が終端行あり・rc 137）。
- **上限は 2 種**: `{jobs}` を持つ行 = `実効 jobs × gate.job_memory_mb`。それ以外の verify 行（`{jobs}` を持たない行〔workspace の nextest / clippy 等〕）= `MemTotal − host.reserve_memory_mb`（host の予約分だけを守る箱。行ごとの値を持たない＝rules 行を増やさない）。runner / lens / claude の包み = `1 × gate.job_memory_mb`（§12・契約表の行 c・裁定 id user 2026-09-15T18:2xZ。行 c の land までの現物は `MemTotal − host.reserve_memory_mb`〔ADR-0021 §2.2 の割り当て〕）。
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

- 宣言 file に **`detection-verify`**（検出線の行の列・`{base}` `{jobs}` の穴を置ける・**任意の key**＝無ければ③は空・toy repo の宣言は不変・ADR-0010 §2.1 の部分 supersede）を足す。scribe2 自身は `cargo xtask mutants-diff --base {base} --jobs {jobs}` をここへ移し、`common-verify` から外す（ADR-0009 §2.3 の置き場の 1 文を ADR-0021 §2.6 (v) で読み替える）。③の行は②と同じく写し（intake が凍結した宣言）から読む。検出線の rules 行（R-C12-1）が deny に昇格した周は、同じ便でその行を `common-verify` へ戻す（deny する行は撃ち直す側・ADR-0021 §2.4）。検出線 = 落ちても deny しない行（C12.4）。rc は 3 値で極性が違う: rc 0 = 測定（生存は行に載るだけ）／ rc 1 = R-C12-1 が deny に昇格した周だけ現れ、gate は赤に数える／ rc 2 = 「測れなかった」（道具の不在・baseline 落ち）で、gate は赤に数えず INCONCLUSIVE へ倒す（`Gated` に留まり測り直せる・[pipeline.md](./pipeline.md) §5.3 の判定順・`s2-07l.331`。以前は rc≠0 を一律に赤に数え、測れなかった周が FAIL → run N+1 で runner 1 周を払っていた〔.329 run 1・2026-09-15〕）＝測れなかったを通ったにも赤にも化けさせない。
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
- **形（何を作るか）**: (1) 上の呼び手 4 か所の limit を `HostReserve` から `PerJob(1)` に替える＝上限 = 1 × gate.job_memory_mb（値は manifest が持つ）。`Limit` の variant は増やさない（§4.2「2 種」のまま）。`limit_of` と gate の verify 行の箱は変えない。`confine.rs` は `HostReserve` の doc の 1 行（「runner・lens」の語を外す）だけ。(2) 禁じる語列の rules 行・`RuleKind` の variant・hook の deny は ADR-0025 / `s2-07l.168` で既着＝本便は行を増やさず、既存の歯が緑のままであることを回帰の柵にする。(3) runner の雛形（`crates/scribe2/src/headless/runner.txt`）の「実行してよい command」節に「検出線（cargo mutants）は gate が撃つ・runner は撃たない（禁じる語列で止まる）」の 1 行を足す。(4) 設計の写し: §4.2 の割り当ての句（本 doc）と pipeline.md §6 の封じ込めの pointer に同じ 1 句。
- **歯**（呼び手 4 か所に 1 本ずつ・どれか 1 か所を `HostReserve` のまま残すと赤になる）: `crates/scribe2/tests/e2e/pipe/spawn.rs`（runner の unit の MemoryMax）/ `crates/scribe2/tests/e2e/pipe/gate.rs`（gate の lens の unit の MemoryMax ∧ 同じ gate の `{jobs}` 無しの verify 行は host の箱のまま・両方向を 1 本で）/ `crates/scribe2/tests/e2e/pipe/intake.rs`（審査の lens の unit `-review-1` の MemoryMax・審査の歯は既存の pipe_review_ 接頭辞と同じ file）/ `crates/scribe2/tests/e2e/headless.rs`（claude の unit の MemoryMax・雛形の 1 行）。偽 systemd-run は gate.rs の歯の stub が private なので、spawn.rs / intake.rs / headless.rs の歯は同型の stub を歯の中で書く（§13 と同じ・module の可視性を触らない）。
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

- **出所・現物**: admin の観測（2026-09-14）で、純移動 40 項目の便の検出線が 2 周とも大きな母集団を回し、生存 1 本は移した行の既存の弱さで新しい情報を出さないまま時間と memory を払っていた。現物: gate の検出線は diff の追加行を母集団にして撃つ経路を持ち、`crates/scribe2/src/pipe/move_proof.rs` の `judge` は移動した item の名と本文の一致を純関数で証明して `LensInput` を組み、`keep` がその要約を run dir に残す。
- **形（何を作るか）**: gate が検出線を撃つ前に、`LensInput::Summary` から一致と証明された item の head 側の行範囲を取り、その範囲の hunk を落とした母集団用の diff を組んで検出線へ渡す。残る追加行が 0 本になる純移動だけの便は、`Check::Detection`（`crates/scribe2/src/pipe/gate/verify.rs`）を赤にも測定未了にもせず、純移動として名指す記号を record に残す。`LensInput::Summary` にならない便は従来どおり全ての追加行を母集団にする。
- **触らない**: 検出線を実行する xtask 側の実装、lens の入力の判定、`judge` / `keep` の証明そのもの。
- **却下案**: 純移動便の検出線を全部 skip する案は、移動でない追加行（mod 宣言の追加や可視性の変更）まで母集団から落としてしまうため不採用。除外の判定を xtask 側に置く案は、証明が core の `crates/scribe2/src/pipe/move_proof.rs` に既にあり、同じ判定を 2 か所に持つことになるため不採用。

## 15. gate の周ごとの検出線の出力を run dir へ写し、show はその写しから読む（契約表の行 f・`s2-07l.298`）

- **出所・現物**: admin の提案（2026-09-14・.286 run 1 の実測）で、検出線の出力が便の worktree の out にだけ在り、追随周の撃ち直しが out を作り直すと前の周の生存の一覧が消えることが分かった。gate の record（`crates/scribe2/src/pipe/gate/record.rs` が書く verify.jsonl）は行ごとの rc を残すが生存の一覧は残していない。pipe show の判定行は `detection_lines`（`crates/scribe2/src/pipe/cli/show.rs`・`s2-07l.349` の純移動で `cli.rs` から移った）が verify.jsonl の detection record の line を逐語で写す。
- **形（何を作るか）**: gate が検出線を撃った直後に、その周の判定行（record の `line=` と同じ字面）と出力（`outcomes.json` と missed.txt・無ければ不在を表す marker）を run dir 配下の周ごとの置き場へ写す（上書きせず周ごとに別の置き場へ）。`detection_lines` の読み口を写しの判定行へ向け、verify.jsonl を読む経路と worktree の out を直接読む形を持たない。数は数え直さない（total / caught / missed の数え手は `crates/xtask/src/mutantsdiff.rs` の 1 つのまま・C2）ので、偽の検出線が outcomes.json を書かず判定行だけを出す既存の歯の字面は変わらない。判定行も無い周は不在と分かる 1 行（0 件と弁別・判定行の形と衝突しない字面）。
- **触らない**: 検出線の実行そのもの・判定・verify.jsonl の record。
- **却下案**: admin が Gated の時点で手で写す運用は散文の手順になり、追随の再 gate が同じ秒に起きると間に合わないため不採用。worktree の out を周ごとに別名で残す案は、worktree が retire で畳まれるため置き場として不適で不採用。

## 16. 契約が名指した生存行に変異を当てて outcomes の 4 kind + 不在の 5 値で記す（契約表の行 g・`s2-07l.341`）

- **出所・現物**: .338（歯だけの便）の gate で検出線が 2 周とも母集団 0 になった（admin 実測 2026-09-15）。diff が mod tests の中だけで、検出線が変異を生やす本体の行を持たなかったため。歯だけを足す便が base の生存行を撃ち落としたかどうかを、器がこれまで測っていなかった。現物: 契約 file（`crates/scribe2/src/pipe/contract.rs` の `Contract`・write_set field を含む）は変異の的を宣言する field を持たず、`crates/xtask/src/mutantsdiff.rs` の検出線を撃つ口も diff の追加行を母集団にする経路しか持たない。
- **形（何を作るか）**: (1) 契約 field を 1 つ新設し、契約が生存行（ファイル・行・変異の名）を的として名指せるようにする（契約表の行の欄の正本は `crates/scribe2/src/pipe/table.rs` の `FIELDS`・読み手は同 file の `ContractRow` と 1 欄ずつ読む `crates/scribe2/src/pipe/table/parse.rs`・欄の追加は `FIELDS` の tracked な生成物 `contracts/schema.toml` の描き直しを伴う〔xtask check が render と tracked の差分 0 を測る〕・契約 file の側は `contract.rs` の optional の欄・intake が写しへ運ぶ受付の歯は `crates/scribe2/tests/e2e/pipe/intake.rs`）。(2) `crates/xtask/src/mutantsdiff.rs` に的を直接絞って撃つ口を新設し、的ごとの分類を閉じた enum 1 つで記す判定行を出す。値は cargo-mutants の outcomes の 4 kind（caught / missed / unviable〔コンパイル不能〕/ timeout＝現物の `Counts` が読む 4 つの数と同じ語）+ 的が outcomes に当たらない absent（file・行・変異の名が現物とずれた）の 5 値で、母集団 = 的の本数・5 値の和 = total。noop（変異前後で挙動差なし）は outcomes の上では missed と同じで測れないため分類に持たない（挙動差の A/B は本節の射程外）。(3) gate はこの field が在る便では的を絞った口で検出線を撃ち、無い便は従来どおり diff の追加行を母集団にする。verdict の判定は変えない（検出線は deny ではない）。
- **触らない**: diff の追加行を母集団にする従来の経路、verdict の 3 値、挙動差の A/B（手順のまま・別便）。
- **依存**: `crates/scribe2/src/pipe/gate.rs` / `crates/scribe2/src/pipe/gate/record.rs` で契約表の行 e・f と交差するため、それらの後に流す。
- **却下案**: 歯が名指す関数の本体全体を母集団に加える案は、宣言した的を測定するという型に合わないため不採用。noop を分類に入れる案は、outcomes だけでは missed と区別する規則が無く（挙動差は変異前後の実 binary の A/B でしか測れない）偽の outcomes に札を貼るだけの空虚な歯になるため不採用。admin の手作業を続ける案は散文の手順になり、便が増えると追いつかないため不採用。

## 17. 歯の fixture の dir と器の systemd scope を終端で必ず片付ける（契約表の行 h・`s2-07l.343`）

- **出所・現物**: admin の実測（2026-09-15）で、e2e の fixture の一時 dir が多数残り、scribe2 の systemd scope も running / failed / dead を合わせて多数残っていた。掃除そのもの（消す操作）は user の承認（A1）を要するため本便の外だが、残る原因は器の側にある。現物: `crates/scribe2/tests/e2e/main.rs` の一時 dir の作り手は複数箇所から呼ばれ、削除は歯の中で成功した経路だけが呼ぶため panic した歯は dir を残す。`crates/scribe2/src/pipe/confine.rs` の `release_scope` は scope を止める呼び出しは持つが、failed のまま残った scope を戻す呼び出しは持たない。
- **形（何を作るか）**: (1) 一時 dir を包む型を新設し、Drop で削除する（panic でも unwind の途中で消える）。既存の呼び手の変更は最小にし、残したい歯だけ明示的に保持を選べるようにする。(2) `release_scope` の終端で、子の停止に続けて failed 状態を戻す呼び出しを行う（順序を固定し、結果は record の語 1 つに残す）。(3) e2e の疑似 seat（`crates/scribe2/tests/e2e/seat.rs` が使う独立 socket 上の tmux server）も同じ Drop の仕組みで畳む。
- **触らない**: 封じ込めの形（scope の unit 名・上限）、歯の assert。既存に残っている dir・scope・process の掃除自体（A1 の後に別途行う）。
- **却下案**: 依存 crate を足して一時 dir を管理する案は、依存の追加が承認事項（A3）になり標準ライブラリの Drop で足りるため不採用。歯の終端で個別に手で消す既存のやり方を続ける案は、失敗した歯が残す問題を解かないため不採用。

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

- 何が起きているか: admin の実測 2026-09-16 04:5xZ（#246〔docs-only〕の CI で `tests/e2e/hook.rs` の歯 `hook_brief_planner_carries_the_dialogue_surface_lines` が `guard.ready()`〔assert の字面「独立 socket に session を立てられる」〕で落ち、main で単独なら PASS・本日 2 例目の「並列が高いときに fixture の tmux 席が立たない」型・1 例目は検出線の rc 2 = `.390`）。現物（verified・main aee95a3）: `tests/e2e/seat.rs` の `start_seat_sized` が独立 socket に `new-session` を立てた後、`capture` の末尾が prompt の字になるまで **`PROMPT_WAIT` = 5 秒**を 100 ms 刻みで待ち、届かなければ `ready = false` の guard を返す（helper は panic せず呼び側の `#[test]` が落とす）。`ready()` の呼び手は e2e の 7 file・100 箇所（`hook.rs` 8 / `seat.rs` 2 / `seat/account.rs` 13 / `seat/cycle.rs` 35 / `seat/launch.rs` 14 / `seat/register.rs` 5 / `seat/tick.rs` 23・grep）＝同じ 1 定数が全部の tmux fixture の起動待ちを決める。負荷下（CI の並列・便の gate と build の同時走行）では `sh -i` が prompt を描くまで 5 秒を超える周が在る。
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

- 何が起きているか（user 直命 2026-09-16 05:5xZ / 裁定 06:39Z・11:14Z・逐語は台帳 `s2-07l` notes・決定は [ADR-0035](../../design-intent/decisions/ADR-0035-live-run-cap-is-one-rules-row.html)）: 並列度を上げた周の実測（ThinkPad・16 core）は load 18〜26・CPU 81 ℃で memory は 10 / 62 GB＝受付（§3.2）は memory の枠だけで本数を絞るので CPU と温度の逼迫が受付に映らない。user は、走っている便は止めず次に走らせる分から絞る → 同時本数の最大値を 1 つ器の規則として持つ・値は 16、と裁定した（裁定 id = user 2026-09-16T11:14Z・逐語は台帳・CON2）。暫定の上限は admin の launcher の変数と live を数え直す script（器の外・C2.2 / N2・ADR-0034 §1 が事故として挙げた型）に在り、器には無い。ADR-0034 の決定文と SRS FR68 の「数値上限を持たない」句は ADR-0035 が部分 supersede する（SRS の同句は user の /folio-architect の周）。
- 形: (1) rules 行 `pipe.max_live`（kind `PipeMaxLive`・Int・本・**値は user 裁定**・C5）を §3.1 の表と manifest に足す（連鎖は行 j〔[account-autonomy.md](./account-autonomy.md) §13〕と同型: `RuleKind` の variant・Int の列・manifest の行・[rules-manifest.md](./rules-manifest.md) §4 の表・歯の kind 件数）。(2) 受付（`pipe/cli/intake.rs`・交差の判定 `exclude_overlap` と同じ段・`--design` / 従来形の両方が通る同じ関数）が、交差と同じ live の判定（`pipe/cli/state.rs` の `live`・終端でない run・段の網羅 match）で state dir の live な便を数え、本数 ≥ 値の周は `Refuse` に足す variant 1 つ（live の本数と上限を運ぶ・slug `max-live`・stderr の 1 行 `pipe: max-live live=<n> cap=<c>`）で断る（run を作らず event を書かない・rc は既存の拒否と同じ 1）。live を読めない便が 1 つでも在れば交差と同じく `WriteSetUnreadable` 側（rc 2・fail-closed・NFR4）。数える順は交差の前。**短絡しない**: contract-source.md §21（`s2-07l.394`・受付の `judge` は各判定関数を全部撃って断りを列に積む＝preflight の一覧性）に合わせ、上限で断る周も交差の判定はそのまま撃ち、交差の組は列に並ぶ（上限の断りが先頭・交差は後続の行・上限で断る周に交差を並べても害は無い＝性能の話に留まる）。(3) 数えるのは便を作る前だけ＝走行中の便には効かず、`pipe resume` と追随の起こし直しは新しい便を作らないので数えない。(4) dispatcher の列の理由（[dispatcher.md](./dispatcher.md) §3 の閉じた型）に「上限で待つ」variant 1 つを足すのは行 a の Landed 後の別の行（本行は受付だけ）。(5) 一時的な引き下げは rules 行の値の改訂（裁定 id 付きの PR）でだけ行い、env・launcher の変数・host.toml から読まない（C1 / C2.2）。
- 触らない: 受付の memory の枠（§3.2）と `gate.mutants_jobs`・交差の判定 `overlaps`・`pipe run` / `pipe intake` の外形（usage）・段の enum・`Refuse` の既存 variant と rc の語彙。
- 歯（`pipe_intake_max_live_` 接頭辞・`tests/e2e/pipe/intake.rs`・toy repo・tmp の manifest を `--rules` で渡す）: `pipe.max_live = 1` で live 1 本の下の 2 本目の intake が slug `max-live` と `live=1 cap=1` の 1 行で断られ、run dir も event も増えない／その live の便を `stop --run` で終端に倒すと同じ契約が通る／Gated で verdict FAIL の便は live に数えず上限 1 でも通る／写しを読めない live の便が在る周は `write-set-unreadable`（rc 2）で断る。rules 行は kind 件数の pin + 外形の歯（行 j と同型）。
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
- 形 (2) **claude の usage**（行 r・M）: 出所は claude の result record の `usage` object（4 値）と top-level の `num_turns` / `duration_ms`（`total_cost_usd` は CLI の見積＝派生値ゆえ運ばない・C10）。(a) runner: `headless/runner.rs` の `Watched` が result record を見る周（`is_result_record`）に usage を読み（入れ子の object を読む口は `find_key` / `top_level_string` と同じ深さの規則で `usage` の直下だけ・flat parser を使わない）、`conclude` の要約行に `usage=in:<n>,out:<n>,cache_read:<n>,cache_create:<n> turns=<n> wall_ms=<n>` を足す（読めない周は field を欠く・rc は変えない）。(b) lens: `Call` の `streaming`（bool）を**閉じた 3 値**（text / json / stream-json・名は実装が決める）に替え、lens は json（1 object・`result` の text と `usage` を持つ）で起こし、判定は従来どおり `result` の text の最後の JSON 行から読む。**読みの分岐は 1 つ**: stdout の最後の JSON object が `type` = `result` の封筒（claude の json 出力）ならその `result` の text の最後の JSON 行を判定に、封筒でなければその object をそのまま判定に読む（従来の text の形）＝偽 lens の fixture（`tests/e2e/pipe/gate.rs` / `intake.rs` / `land.rs` / `ratelimit.rs` / `spawn.rs` / `tests/e2e/pipe.rs` の 6 file・verified・裸の判定 JSON を 1 行出す）は不変で write-set に入れない。`Call` の構築点は 7 か所（`headless/mod.rs` の 4・`headless/runner.rs` の 1・`headless/lens.rs` の 1・`fleet/usage.rs` の 1・verified）で、型の変更ゆえ全部を新しい値に写す（`fleet/usage.rs` の refresh は text の値・runner は stream-json の値＝外形は不変でも file は触る）。lens は判定の JSON object に `usage` の 4 値と `turns` / `wall_ms` を足して stdout に写し、`pipe/gate/lens.rs` の読み手は `findings` / `population` と同じ形で `usage` を読む（**無くても INCONCLUSIVE にしない**＝古い lens・偽 claude の周は field を欠くだけ）。(c) 記録: `fleet/mod.rs` の `EventKind` に variant 1 つ（消費の 1 件・名は実装が決める・`as_str` の 1 腕・`replay` は段を変えない腕 1 つ）を足し、`Event` に `allowance` / `registration` と同じ形の任意 field 1 つ（閉じた型 Cost: 出所〔runner / lens / review の閉じた 3 値〕・token 4 値・turn・wall_ms）を足す＝C6.3 の「append-only の store 1 つ」は `fleet/events.jsonl` で、run dir には別 file を作らない。`Event` の field を足すと **`Event` の literal 構築点の全部**（struct update の `..` を使う場所は無い・verified）に 1 行足す: src は `fleet/usage.rs` / `pipe/mod.rs`（write-set 内）+ `account/mod.rs` / `seat/role.rs` / `fleet/cli.rs` / `pipe/queue.rs`（in-file の歯）/ `seat/rebrief/status.rs`（in-file の歯）、tests は `tests/e2e/fleet.rs` / `tests/e2e/pipe/ratelimit.rs`（write-set 内）+ `tests/e2e/prop.rs`（property の strategy）/ `tests/e2e/seat.rs` / `tests/e2e/seat/wm.rs`（母集団 = `Event {` の grep 13 file・main e9add0f）＝行 r の write-set はこの 8 file を含む。書く口は `pipe/mod.rs` に **消費専用の 1 関数**を足す（`emit` の隣・`Emit` は触らない＝`Emit` の literal 構築点は src に 29 か所在り〔verified〕、欄を足すと閉包が全 file に広がる・消費の event は `Event` を直接組んで同じ store の append に渡す）。書く側は 3 か所: `pipe/spawn.rs` が runner の要約行を読んで `Implemented` / `Questioned` の event の前に 1 件、`pipe/gate.rs` が lens の verdict を書く周に 1 件、`pipe/review.rs` の審査の周に 1 件（review は lens と同じ `headless/lens.rs` の口）。(d) 読む側: `pipe show --run` は消費の event を 1 行ずつ写し、`pipe report` の行に `cost: with_usage=<便数> out=<token> cache_read=<token> gate_secs=<秒の和>` の 1 行を足す（母集団 = 便数を同じ行に）。読む側の字面の pin は 2 系統（verified）: `pipe report` の行を逐語で持つ歯は `tests/e2e/pipe/spawn.rs` と `tests/e2e/pipe/ratelimit.rs`、`pipe show` の record の描画は形 (1) の外形 snapshot `pipe_record_show_external_form`（`tests/e2e/pipe/gate.rs`）＝行 r の write-set は両方の file と snapshot を持つ（消費の行が無い run の描画は不変でも、動く周に同じ便で更新する）。
- 触らない: 判定の意味（verdict の 3 値・rc）・runner の rate limit の読み（`decide`）・record の既存 field・event の schema 番号・rules 行（R-C6-1 は測れた後に C5 の裁定 id で足す別の行）・`fleet usage` の起動（text のまま）。
- 歯（`gate_secs_` 接頭辞 = 行 q / `run_cost_` 接頭辞 = 行 r）: 行 q = (a) in-file（`pipe/gate/verify.rs`）: 撃った段の `Step` が秒を持ち write-set 照合の `Step` は持たない／(b) e2e（`tests/e2e/pipe/gate.rs`）: gate の `verify.jsonl` の撃った record 全部に `secs=` が在り skip record には無い（母集団 = record 数を同じ assert に）・`pipe show` の外形 snapshot。行 r = (c) in-file（`headless/runner.rs`）: result record の 1 行から usage 4 値と turns / wall_ms を読む・`usage` が無い record は `None`・入れ子の `usage.iterations[]` の中の数に釣られない／(d) e2e（`tests/e2e/headless.rs`・偽 claude が usage 付きの result record を出す）: runner の要約行に `usage=` が載る・lens の判定 object に `usage` が載る／(e) e2e（`tests/e2e/pipe/spawn.rs` / `tests/e2e/pipe/gate.rs`）: 便 1 本で消費の event が runner 1 件 + lens 1 件（review を通す周は +1）書かれ token の値が偽 claude の出した数と一致・`pipe show --run` と `pipe report` の行（母集団 = event 数）／(f) 偽 claude が usage を出さない周は event を書かず gate の verdict は変わらない（fail-open ではなく「測れなかった」を field の不在で運ぶ・C10）。
- 却下: run dir に usage.json を置く（C6.3 の store が 2 つになる）／`total_cost_usd` を記す（CLI の見積＝派生値・口座の種別で意味が変わる）／lens を stream-json にする（最後の JSON 行が claude の record になり判定が埋もれる・`headless/lens.rs` の実測 2026-09-10）／runner の要約行を parse せず run dir の `runner.stdout.log` から後で拾う（log は要約だけで record を持たない・verified）／R-C6-1 を同じ便で足す（値の裁定が先・C5）。

## 27. 主実測は着地する木が gate の木と同じなら全段を撃たない — record 1 本 `kind=main skipped=main tree=<sha>` で main-green にする（契約表の行 s・`s2-07l.464`・[ADR-0043](../../design-intent/decisions/ADR-0043-same-tree-main-check-is-one-record.html)）

- 何が起きているか（planner の実測 2026-09-17・main 12e64cc・verified）: land の主実測（`verify_main`・§5）は着地する木が gate の verdict の `tree` と同じ周でも ① write-set 照合・② 共通 verify・④ 契約 verify を撃ち直し、③ 検出線だけを `skipped=detection reason=same-tree` で省く。母集団 = `verify-main.jsonl` を持つ着地 39 便のうち理由を持つ 36 便: `same-tree` 27 / `outside-scope` 9。same-tree の 27 便で squash の commit 時刻から主実測の record の終端までの壁時計は中央値 14.9 分（p25 5.6 / p75 15.9 / 最大 30.6）＝③を省いた後に残る ①②④ の時間。着地 45 便の Gated(PASS) → Landed は中央値 10.4 分（p25 5.1 / p75 15.9）・1 便 126 分の約 1 割。木の sha が同じ＝gate が測った木と byte 単位で同一（content-addressed）ゆえ、①②④ は同じ木の同じ測定の重複で情報を足さない。
- 形（行 s・S・ADR-0043 §2.1 / §2.2）: `verify_main` は tmp worktree を切る**前に** `main_detection` と同じ比較（verdict の `tree` と `<new>^{tree}`）を読み、**同じ周は tmp worktree も verify の段も撃たず** `verify-main.jsonl` に record 1 本（`kind=main skipped=main tree=<sha> reason=same-tree`・schema 1 のまま既存 field の組だけ）を書いて緑の `MainCheck` を返す（finish へ進む・Landed の detail と stdout は不変）。record は `pipe/gate/record.rs` の `Skipped` に構築の口を 1 つ足す（段の閉じた値に主実測の 1 つを足す・理由は既存の `same-tree` を再利用・木は必ず持つ＝どの木の測定を再現と見なしたかを残す・C10）。一致しない周（`outside-scope` と面の内）と verdict に `tree` が無い周は従来どおり全段（③ は §5 の規則のまま）。候補の木（[pipeline.md](./pipeline.md) §40）の主実測も同じ 1 関数を通る＝候補の先端と着地の先端の木が同じ周は record 1 本。ADR-0021 §03 (C) が「untracked に依って通った便を main 実測が捕まえる」として全省略を却下した前提は、gate の前提検査（`precheck`・`status --porcelain` が空でなければ撃たない・verdict の `tree` はその木）が既に untracked と未 commit を断るので今は無い。残るのは ignore された file に依る周だけで、その面は main の CI（FR50）が持つ。
- 触らない: gate の段と順序（§5）・③ の省き方（`outside-scope` の面・`DETECTION_SCOPE`）・CAS と anchor 同期・`finish` と `Landed` の detail・`MainCheck` の 3 値と極性（`Unmeasurable` の周は不変）・`verify.jsonl` の record・record の schema 番号・行 q の `secs`（撃たない周は持たない＝skip record の規則のまま）・`pipe show` の読み（`skipped=` の非空で従来どおり拾う）。
- 置き場（`s2-07l.457` Landed a3e29b9 の後・verified）: 主実測の群（`verify_main` / `main_detection` / `record_main` / `materials` / `check_path` / `VERIFY_MAIN_FILE`）は `crates/scribe2/src/pipe/land/verify.rs`（215 行・[pipeline.md](./pipeline.md) 行 aj の純移動）に在り、`MainCheck` と呼び手 `land` は `land.rs` に残る（本便は `land.rs` を触らない＝呼び手の形は不変）。
- 歯（`pipe_main_same_tree_` 接頭辞・`tests/e2e/pipe/gate.rs`・`pipe_detection_scope_` の歯と同じ fixture〔tmp git repo + 偽 verdict.json + 撃った段を `added` に残す verify 行〕／in-file は `main_skip_record_` 接頭辞・`pipe/gate/record.rs`）: (a) verdict の `tree` と着地の木が同じ周は `verify-main.jsonl` が 1 行（key 列 = `schema` / `n` / `kind` / `skipped` / `tree` / `reason`・`kind=main skipped=main tree=<sha> reason=same-tree`・`n` は 1）で verify の cmd は 1 本も走らず（`added` が空）・Landed の detail と main の先端は従来の形／(b) `tree` が違う周は record が従来の段数で並び主実測の skip record は無い／(c) verdict に `tree` が無い周は全段撃つ（record の形は (b) と同じ）／(d) in-file: 主実測の skip record の字面が固定で、木の無い構築は口が取らない。**変更する既存の歯 2 本**（同じ file・§5 の same-tree の期待を持つ）: `pipe_detection_land_skips_detection_when_tree_matches` と `pipe_detection_scope_same_tree_records_reason` は「②④ だけを撃つ・③ の位置に `skipped=detection`」の期待を (a) の形へ写す（test diff だけを base に当てると base は `kind=main` を書かないので RED）。`outside-scope` と読めない周の歯（同 file）は不変。base は `verify_main` が木の比較の前に worktree を切って全段撃つので (a) も RED。
- 却下（ADR-0043 §03）: ① だけ残す（数秒だが経路が 2 本になり、木の一致で守れている diff を 2 度測る）／主実測を廃止し gate 後は常に着地（`outside-scope` の 9/36 = 木が違う周に main が未検査の木になる・C12.6）／主実測を Landed 後に非同期で撃つ（赤の周に main が赤のまま・C12.6）／②④ のうち clippy / deny だけ省く（手書きの選別・C2）。

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
write-set = ["crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "crates/scribe2/tests/e2e/headless.rs", "docs/design/gate-cost.md", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_confine_runner_limit_", "cargo nextest run -p scribe2 --no-tests=fail pipe_confine_lens_box_", "cargo nextest run -p scribe2 --no-tests=fail pipe_confine_review_box_", "cargo nextest run -p scribe2 --no-tests=fail headless_runner_box_"]
size = "S"
done = "runner / lens / claude の包みの MemoryMax が 1 × gate.job_memory_mb に揃い、gate の verify 行の箱は不変、runner の雛形に検出線の 1 行が在る（rules 行は増やさず、cargo mutants の deny は既着の歯が緑のまま）"

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
write-set = ["crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_detection_pure_move_"]
size = "S"
done = "純移動の便は移した行を検出線の母集団から外し、実変更の行だけを撃つ。純移動だけの便は赤にも測定未了にもならない"

[[contract]]
id = "f"
title = "gate の周ごとの検出線の出力を run dir へ写し、show はその写しから読む"
req = ["FR8", "FR22"]
section = "15"
write-set = ["crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_detection_copy_"]
size = "S"
done = "検出線の出力が周ごとに run dir へ残り、pipe show が worktree の out でなくその写しから判定行を出す"

[[contract]]
id = "g"
title = "契約が名指した生存行に変異を当てて outcomes の 4 kind（caught / missed / unviable / timeout）+ 不在（absent）の 5 値で記す"
req = ["FR8"]
section = "16"
write-set = ["crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "contracts/schema.toml", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/xtask/src/mutantsdiff.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/intake.rs", "docs/design/gate-cost.md", "docs/design/contract-source.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_gate_targets_", "cargo nextest run -p xtask --no-tests=fail mutants_targets_", "cargo nextest run -p scribe2 --no-tests=fail pipe_intake_targets_"]
size = "M"
done = "歯だけの便で契約が名指した生存行が的になり、caught / missed / unviable / timeout / absent の 5 値（和 = 的の本数）で記録され、field の無い契約は従来どおり動く"

[[contract]]
id = "h"
title = "歯の fixture の一時 dir と器の systemd scope を終端で必ず片付ける"
req = ["NFR3", "FR46"]
section = "17"
write-set = ["crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "docs/design/gate-cost.md", "docs/design/pipeline.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_confine_release_ e2e_fixture_"]
size = "S"
done = "panic した歯も一時 dir を残さず、release の終端で scope の failed が戻り、疑似 seat の tmux server も drop で畳まれる"

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
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/rules-manifest.md", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/pipe/intake.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_intake_max_live_"]
size = "S"
done = "rules 行 pipe.max_live が裁定 id 付きで 1 本増え、live な便が値以上の周の intake は max-live の 1 行で断られて run dir も event も増えず、live の便を止めれば同じ契約が通り、Gated FAIL の便は数えられず、写しを読めない周は write-set-unreadable で止まり、走行中の便と受付の memory の枠は不変"

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
write-set = ["crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/headless/runner.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/gate/lens.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/report.rs", "crates/scribe2/src/pipe/cli/show.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/seat/rebrief/status.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/seat/wm.rs", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__pipe__gate__pipe_record_show_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail run_cost_", "cargo nextest run -p scribe2 --no-tests=fail run_cost_"]
size = "M"
depends = ["q"]
done = "runner の要約行と lens の判定 object が usage 4 値と turns / wall_ms を運び、便 1 本で消費の event が出所ごとに 1 件ずつ fleet/events.jsonl に書かれて token の値が claude の record と一致し、usage の無い周は event を書かず判定も rc も変わらず、pipe show と pipe report が消費の行を母集団つきで写し、run dir に別 file は増えない"
[[contract]]
id = "s"
title = "主実測は着地する木が gate の verdict の tree と同じ周は全段を撃たず record 1 本（kind=main skipped=main tree=<sha> reason=same-tree）で main-green にする — 違う周と tree の無い周は従来どおり全段（ADR-0043・ADR-0021 §2.4 の部分 supersede・.457 Landed 後）"
req = ["FR34", "FR12", "FR50"]
section = "27"
write-set = ["crates/scribe2/src/pipe/land/verify.rs", "crates/scribe2/src/pipe/gate/record.rs", "crates/scribe2/tests/e2e/pipe/gate.rs", "docs/design/gate-cost.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_main_same_tree_", "cargo nextest run -p scribe2 --lib --no-tests=fail main_skip_record_", "cargo nextest run -p scribe2 --no-tests=fail pipe_detection_land_skips_detection_when_tree_matches", "cargo nextest run -p scribe2 --no-tests=fail pipe_detection_scope_same_tree_records_reason"]
size = "S"
done = "着地する木が verdict の tree と同じ周は verify-main.jsonl が skip record 1 本で verify の cmd が 1 本も走らず Landed の形は不変、違う周と tree の無い周は従来の段数で撃ち、skip record は木を必ず持つ"

<!-- contracts:end -->
