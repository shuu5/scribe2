# 設計: gate の費用構造 — 変異の並列度は project 横断（host 単位）の受付の実測で決め、器の子 process は cgroup で封じ、木が同じ main 実測は検出線を撃ち直さず、着地は gate 済みの便を先に通す

- 要件: [FR8](../../design-intent/spec/srs.html#FR8) [FR9](../../design-intent/spec/srs.html#FR9) gate の機械検証と lens / [FR10](../../design-intent/spec/srs.html#FR10) [FR11](../../design-intent/spec/srs.html#FR11) land の前提と squash / [FR34](../../design-intent/spec/srs.html#FR34) 追随 / [NFR1](../../design-intent/spec/srs.html#NFR1) lens 予算 / [NFR4](../../design-intent/spec/srs.html#NFR4) 読めない store は rc 2。host の資源を枯渇させない要件は SRS に無く、v0.7 の材料（planner state dir・NFR 候補）として user の手番に出す。
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) / [C5](../../design-intent/spec/constitution.html#c5) 値は rules 行・裁定 id 付き / [C3.4](../../design-intent/spec/constitution.html#c3) 待ちは完了 enum の 1 実装 / [C2.2](../../design-intent/spec/constitution.html#c2) env を読まない・置き場は NAME から導く / [C6](../../design-intent/spec/constitution.html#c6) 起動口は 1 つ・Budget は Precheck から / [C10](../../design-intent/spec/constitution.html#c10) 宣言値・測定値・実効値を型で分ける / [C11.2](../../design-intent/spec/constitution.html#c11) 境界は極性を持つ / [C12.4](../../design-intent/spec/constitution.html#c12) 変異の生存は検出線 / [C12.6](../../design-intent/spec/constitution.html#c12) main は常に緑 / [N1](../../design-intent/spec/constitution.html#n1) 不可逆に消さない。
- 決定: [ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-measured-and-confined.html)（本 doc の決定の正本・ADR-0009 §2.4 と ADR-0010 §2.1 / §2.4 を §2.6 で部分 supersede）/ [ADR-0009](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html) §2.3 / §2.4（変異検査と共通 verify の順序）/ [ADR-0010](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html)（宣言 file）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)（極性一覧）/ [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-overlap-at-intake-and-runner-resolves-conflicts.html)（追随）。
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

manifest に行が載るまでは ADR-0021 の予定行（C14.2 の相互参照は行が在って成立・ADR-0018 §4 と同じ）。

### 3.2 受付（project 横断・host 単位の枠）

- **置き場**: `<state_dir の親>/<NAME>-host/slots/`。state dir は project ごとに違う（`<NAME>-v2-state` / `<NAME>-v2-state-<project>`）が、同じ host の state dir は 1 つの親（host の state root）に置く運用なので、その親から導けば project をまたいで 1 つになる。env（`XDG_RUNTIME_DIR` / `HOME` / `TMPDIR`）は読まない（C2.2・land.rs の tmp dir と同じ理由）。親が違う state dir を使う project は別の受付になる＝その運用は本 doc の外（§8）。
- **受付札**（語彙の `lease` は fleet の割当で別の実体・使わない）: 枠 1 組 = file 1 つ `slots/<pid>-<run>.slot`（内容 = `schema` / `pid` / `run` / `jobs` / `ts`・state dir と同じ TOML subset）。生きている札 = `/proc/<pid>` が在り、その process の起動時刻（`/proc/<pid>/stat` の starttime）が札の `ts` 以前のもの（pid の再利用を弁別）。死んだ札と**読めない札**（schema / 数値の壊れ）は受付が次に走ったとき回収し、record に `slot=reclaimed:<n>` を残す（黙って落とさない・止めない・NFR4）。削除でよい: 札は器が管理する「物」ではなく受付の一時的な印（N1 の対象外・ADR-0021 §5 (D)）。
- **容量の測定**（受付のたびに測る・C10）:
  - `avail = MemAvailable(/proc/meminfo) − host.reserve_memory_mb`
  - `by_avail = floor(avail / gate.job_memory_mb)`（いま実際に空いている分。他 project や host の他 process が使った分は自然に減る）
  - `by_token = floor((MemTotal − host.reserve_memory_mb) / gate.job_memory_mb) − Σ 生きている札の jobs`（受け付けたがまだ常駐していない分を数える＝2 つの gate が同時に測って両方が満額を取る競合を塞ぐ）
  - `free = min(by_avail, by_token)`
- **取得**: slot dir の lock（fleet の event log と同じ lock file の 1 本・retry / stale は rules 行 `fleet.lock_retry_ms` / `fleet.lock_stale_ms`・flock ではない・第 2 の lock 実装を持たない〔C6.3〕）の内側で測り、`jobs = min(gate.mutants_jobs, free)` の札を書く。`free == 0` なら lock を離して待つ。**待ちは完了 enum の variant 1 つ**（`Completion::SlotFree { slots_dir, want }`・現物の variant は pid を運ぶが本 variant は置き場と要る枠を運ぶ）を足して唯一の wait 実装を通す（C3.4・第 2 の poll loop を書かない・周期 = `fleet.lock_retry_ms`）。`gate.slot_wait_s` を超えたら `jobs = 1` で進み、record に `slot=degraded` を記す。**0 で走らせない・断らない**。
- **解放**: verify 行の終了で札を消す（Drop でも消す）。器が死んだ周は次の受付が pid で回収する。
- **極性**: 受付は行為を止めうる判定を持たない（縮退する）ので ADR-0014 §2.1 の guard ではなく、極性一覧に載せない。測れない周（`/proc/meminfo` が読めない・lock が取れない）は `jobs = 1` で進み `slot=unmeasured` を記す（縮退＝従来の費用）。slot dir は真実を持たない印の置き場で NFR4 の「store」ではない（読めない札は回収して記録・rc は変えない）。

### 3.3 実効 jobs の渡し方

- 宣言 file の共通 verify と検出線の穴を `{base}` と **`{jobs}`** の 2 つにする（declaration.rs `Holes::Base` → 穴の列挙を「宣言の行に置ける穴」の閉じた集合にする・ADR-0010 §2.1 の部分 supersede）。scribe2 自身の宣言は `cargo xtask mutants-diff --base {base} --jobs {jobs}`。
- gate は受付で得た jobs を `{jobs}` に置換して撃つ。`{jobs}` を持たない行は受付を通らない（枠を取らない＝mutants を持たない consumer は費用を払わない）。
- xtask `mutants-diff` は `--jobs N` を cargo-mutants の `--jobs` にそのまま渡す（値は持たない）。
- env で渡さない（C2.2 の精神・折り返しの裏口を作らない）。

## 4. 封じ込め（ADR-0021 §2.2）

### 4.1 何を封じるか

器が起こす子 process の起動点は 4 つ（gate.rs `run_line_captured`〔verify 行・gate と main 実測の共通の 1 本〕・spawn.rs `launch_runner`〔runner〕・headless/mod.rs `build`〔claude = runner と lens〕・land.rs〔`--pr-cmd`〕）。verify 行と runner / lens の 3 つを scope に入れる（`--pr-cmd` は host の gh を呼ぶだけで軽い・射程外）。

### 4.2 形

- `systemd-run --user --scope --quiet --unit=<NAME>-<run>-<段>-<n> -p MemoryMax=<上限> -p CPUWeight=<gate.cpu_weight> -- sh -c <line>`。`MemoryHigh` は付けない（係数を持たない・rules 行を増やさない）。
- **上限は 2 種**: `{jobs}` を持つ行 = `実効 jobs × gate.job_memory_mb`。それ以外（`{jobs}` を持たない verify 行〔workspace の nextest / clippy 等〕・runner・lens）= `MemTotal − host.reserve_memory_mb`（host の予約分だけを守る箱。行ごとの値を持たない＝rules 行を増やさない）。
- 止められたのは scope の内側の process だけで、席・他の便・他の project は影響を受けない。scope の `memory.events` の `oom_kill` が 1 以上の周は、その行を rc に依らず「測れなかった」に倒す（gate は INCONCLUSIVE・main 実測は `main-unmeasured`・赤に化けさせない）。
- 実測（2026-09-12・本 host）: cgroup v2・user scope に memory / cpu / pids の controller が委譲されていて上限が効く。
- **無い host**（`systemd-run` が無い・scope を作れない）: 封じ込めなしで**並列度 1** で走り、record に `confined=false reason=<閉じた enum の名>` を記す（理由は自由文にしない・C3.3）。止めない（systemd の無い host で便が 1 本も流れない形を作らない）。

### 4.3 測定の環（宣言値を測定値で置き換えるため）

scope の終了時に `memory.peak` を読み、record に `peak_mb=<n> jobs=<k>` を残す（cgroup の path は `/proc/self/cgroup` の自分の path から user manager の prefix を取り、`--unit` の名で辿る。`memory.peak` の無い kernel は field を欠く＝0 と書かない）。`gate.job_memory_mb` の宣言値は、peak の測定が溜まった後に裁定で置き換える（C10: 宣言値を実効に上げるのは測定を通してだけ）。台帳 s2-07l.152（検出行の記録）と同じ行に載せる。

### 4.4 極性

封じ込めは行為を止めうる判定を持たない（縮退するだけ）ので、ADR-0014 §2.1 の guard の定義（行為を止めうる判定を返す境界）に当たらず、**極性一覧に載せない**（受付と同じ）。可視化は record の `confined=` / `reason=`。「止めない」を選ぶ理由: 止めると systemd の無い host で便が流れず、縮退の並列度 1 は従来の費用と同じで安全側。

## 5. main 実測は木が同じなら検出線を撃ち直さない（ADR-0021 §2.4）

- 宣言 file に **`detection-verify`**（検出線の行の列・`{base}` `{jobs}` の穴を置ける・**任意の key**＝無ければ③は空・toy repo の宣言は不変・ADR-0010 §2.1 の部分 supersede）を足す。scribe2 自身は `cargo xtask mutants-diff --base {base} --jobs {jobs}` をここへ移し、`common-verify` から外す。検出線の rules 行（R-C12-1）が deny に昇格した周は、同じ便でその行を `common-verify` へ戻す（deny する行は撃ち直す側・ADR-0021 §2.4）。検出線 = 落ちても deny しない行（C12.4・rc≠0 は「測れていない」の印で、gate はそれを従来どおり赤に数える＝測れなかったを通ったに化けさせない）。
- gate は ① write-set 照合 → ② common-verify → ③ detection-verify → ④ 契約 verify の順で撃つ（ADR-0009 §2.4 の順序に③を挿す・ADR-0021 §2.6）。verify.jsonl の record は schema 1 のまま**任意 field を足す**（`kind` / `jobs` / `peak_mb` / `confined` / `reason` / `slot` / `skipped` / `tree`・古い読み手は無視・ADR-0017 §2.1 の event と同じ足し方）。
- gate は verdict.json に **`tree`**（gate を撃った HEAD の `^{tree}` の sha）を残す（schema は 1 のまま field を足す・読み手は未知の field を無視する）。
- land の main 実測は record を **`verify-main.jsonl`**（gate と同じ record 形・別 file・gate の周の `n` と重ねない・現物の main 実測は record を書いていない）に書く。`git rev-parse <new>^{tree}` が verdict の `tree` と一致する周は **detection-verify を撃たず** `skipped=detection tree=<sha>` を記し、①②④ は従来どおり撃つ。一致しない周（在りえないが在れば）は全部撃つ。`tree` が無い verdict（旧 gate）も全部撃つ。
- 節約の実測（.136 run 2）: main 実測 78 分のうち変異検査が約 75 分。

## 6. 着地は gate 済みの便を先に通す（ADR-0021 §2.5・機構は .147）

- 原則: `Gated` ∧ verdict PASS の run が在る間、他の run の land は待つ（先に gate を通った便を先に着地させ、stale の連鎖を止める）。
- 機構（順序の判定・待ちの上限 rules 行・詰まりの解き方）は s2-07l.146（衝突の起こし直し）の land 後に .147 で設計する。本 doc は原則だけを持つ。

## 7. 歯（契約ごと・in-file の unit と `tests/e2e/` の e2e の分担）

e2e は binary を spawn し外部 command は PATH 先頭の stub で差し替える（pipe.rs / seat.rs の慣行）。`/proc/meminfo` と cgroup の file を binary の外から差し替える口は持たない（C2.2・裏口を作らない）ので、**測る計算は pure 関数にして in-file の歯が fixture 文字列で測り、e2e は stub の引数・record の field・札 file の実在だけを測る**。

- 受付（in-file・pipe/slots.rs）: `capacity(meminfo_text, rules, live_jobs)` の式（by_avail / by_token の min・reserve の差引・0 の床）・読めない meminfo → `unmeasured`・壊れた札の parse → 回収の数。
- 受付（e2e・`pipe_slots_`）: tmp の state root に state dir 2 つ〔project 2 つ〕で同じ slots dir を見る・自 pid の札で `by_token` を 0 にすると待ち、rules fixture の `slot_wait_s = 1` で `jobs=1 slot=degraded`・存在しない pid の札が回収され `slot=reclaimed:1`・`{jobs}` の無い行は札を作らない・終了で札が消える・置換後の `cmd` に実効 jobs が載る。
- 封じ込め（e2e・`pipe_confine_`・偽 `systemd-run` = 引数を file に写して `sh -c` を exec する stub）: `-p MemoryMax=` が `{jobs}` 行で `jobs × job_memory_mb`、それ以外の行で `MemTotal − reserve`・`CPUWeight=` が rules 行・stub 不在で素の `sh -c` と `confined=false reason=`・runner / lens の起動も wrap を通る。**歯で測れるのは引数まで**: scope の外が死なないこと・`memory.peak` / `memory.events` が読めることは実 host の 1 回を契約の done に入れる（planner の再実測・host 依存）。peak / oom_kill の読みは in-file（cgroup file の fixture 文字列）。
- main 実測（e2e・`pipe_detection_`・verify 行は「呼出回数 file に 1 行足す」stub）: verdict.json の `tree` が gate の HEAD の木と一致・一致する周は③の stub が呼ばれず `verify-main.jsonl` に `skipped=detection` が載り**②④は呼ばれる**（回数 file を行ごとに分ける）・`tree` を壊した fixture では③も呼ばれる・`tree` 無しの verdict でも呼ばれる・検出線の rc≠0 は gate FAIL のまま。
- 宣言（in-file・declaration.rs）: `detection-verify` の穴（`{base}` `{jobs}` 以外は Unfit）・key の無い宣言は従来どおり通る・空配列は不備のまま（ADR-0010 §2.1）。
- rules: 5 行の kind 件数と外形 snapshot・欠落は RuleError（既存の型）。
- 完了 enum: `Completion::SlotFree` の網羅 match（compile）と wait の唯一性（`wait` の呼び手が 1 本・grep でなく型で）。

## 8. 射程外

- 複数 host にまたがる受付（受付は host 単位・event log も host ごと）。
- state root を分けた project 同士の受付の統合（運用で親を揃える）。
- CPU の上限（重みだけ）・swap の制御・cargo の `-j`（cargo-mutants の `--jobs` の内側は cargo の既定）。
- 便の同時本数の上限（admission control・口座の自律制御 s2-07l.142 と併せて別途）。
- 30 秒の実待ちの歯の改修（s2-07l.151・本 doc より先に流す）。
- 検出線の deny 化そのもの（C12.4 の裁定。本 doc は昇格時の置き場だけ）。
- 検出行の値の記録（s2-07l.152・§4.3 の行に相乗りする）。

## 9. 契約（bead）

1. **(a) 実効 jobs + 封じ込め**（M）: rules 5 行 + `RuleKind` 5 variant + 外形 snapshot / kind 件数の歯・`{jobs}` の穴・xtask `--jobs`・`Confinement`（guard ではない・極性一覧に載せない）・record の `jobs= confined= reason= peak_mb=`・oom_kill → unmeasured。runner / lens の起動点（spawn.rs / headless/mod.rs）も同便。**受付が入るまでは上限をそのまま実効に使わない**: (a) 単独では `jobs = 1` のまま（並列を上げるのは (b) の後）。
2. **(b) 受付**（M）: slot dir・受付札・容量の測定（pure 関数）・`Completion::SlotFree` と唯一の wait・縮退・回収・歯。land 後に (a) の実効 jobs が上限まで上がる。
3. **(c) main 実測の検出線省略**（S）: `detection-verify`（任意 key）・verdict `tree`・`verify-main.jsonl`・land の skip・歯。
4. **(d) land の順序**（.147・.146 の後）。

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
