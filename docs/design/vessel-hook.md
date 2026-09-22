# 設計: `.vessel` marker と hook — 名乗り・in-loop guard・注入計測の slot

- 要件: [FR19](../../design-intent/spec/srs.html#FR19) marker と hook / [FR20](../../design-intent/spec/srs.html#FR20) in-loop guard / [FR21](../../design-intent/spec/srs.html#FR21) 注入計測の slot / [FR24](../../design-intent/spec/srs.html#FR24) 管轄外での沈黙 / [AC7](../../design-intent/spec/srs.html#AC7) / [NFR5](../../design-intent/spec/srs.html#NFR5)。制約: CON2 / CON3
- 憲法: [C2](../../design-intent/spec/constitution.html#c2) 名前は 1 定数・env 直読禁止 / [C6](../../design-intent/spec/constitution.html#c6) 消費の store は 1 つ / [C11](../../design-intent/spec/constitution.html#c11) 極性は型で / [C16](../../design-intent/spec/constitution.html#c16) 逸脱は編集の時点で止める
- 決定: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.2（面 1 = `.vessel` の字面）/ §2.4（policy と state dir の受け渡し）
- crate の形は [rules-manifest.md §2](./rules-manifest.md) に従う。
- この設計から出る契約: `s2-3ax`（plugin hooks + marker + guard）。極性一覧と C16.2 の CI は後続（§9）。

## 1. 何を解くか

3 つの面を 1 binary の `hook` subcommand で持つ。

1. **所属（跨版 面 1）**: repo root の固定名 marker `.vessel` が「自分の NAME」を言うときだけ仕え、それ以外は **stdout 0 byte・rc 0** で黙る（FR19 / FR24）。 例外は 1 つ: `--pane` が在る（登録されうる席）のに root が解けない周は黙らず、権能付きの操作を deny する（[seat-roles.md §4](./seat-roles.md)・FR45・FailClosed）。
2. **in-loop guard（C16 の MVP 形）**: 契約の write-set の外への Edit / Write を**編集の時点で** deny する（FR20）。policy が読めなければ fail-closed で deny。
3. **注入計測の slot（FR21）**: hook は自分の出力 1 行を schema 付きで記録し、v3 の計測 store を後付けできる形を保つ。

やさしく言うと: repo の根元にある小さな目印 file が「この器の担当」と言っているときだけ hook は働く。担当なら、契約に無い file を書こうとした瞬間に止める。

## 2. marker `.vessel`（面 1）と repo の初期化

- 中身は **2 行・LF・この順・末尾改行・余白なし**: `name=<NAME>` / `version=<N>`。tracked。
- `Marker::parse`（行末の空白だけ許す・未知 key / 欠落 / 順序違いは error）、`render()`。
- `served(root) -> Served`: `ByMe(version)`（marker 在 ∧ `name == NAME` ∧ state dir が設定済み）/ `ByOther(name)` / `Absent`（不在・読めない・parse 不能・state dir 未設定は全部 `Absent` ＝黙る側へ倒す・FR24）。
- `repo_root(cwd)`: `git rev-parse --show-toplevel` を std::process で（非 repo は None → `Absent`）。
- **state dir の紐づけ**: `vessel init --state-dir <dir> [--version N] [ROOT]` が marker を書き、同時に repo の local git config に `<NAME>.stateDir = <dir>` を書く（key 名は NAME から導出・C2.2）。`hook` と `pipe` は `git config --get <NAME>.stateDir` で読む。tracked file に path を書かない（CON2）。既存 marker が別 name なら rc 2 で何も書かない。
- CLI: `vessel init …` / `vessel show [ROOT]`（marker の 2 行と state dir を 1 行）/ `vessel check [ROOT]`（rc 0 = ByMe / 1 = Absent / 2 = ByOther）。
- **vessel 宣言 `.vessel.toml`**（marker の隣・tracked・[ADR-0010](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html)）: marker には行を足さない（2 行厳格 parse は跨版 面 1 ゆえ不変＝行を足すと両版の hook が `Absent` で黙る）。宣言は契約 file と同じ flat な TOML subset で `schema` / `allowed-commands` / `common-verify` の 3 key が必須。読み手は `pipe intake` だけ（hook は読まない）＝[pipeline.md §5.1](./pipeline.md)。
- **本 repo の root に `.vessel` を置くのは自己ホスト便（AC2・[pipeline.md §9](./pipeline.md)・`s2-07l.24`）の手番**。leg 5 では CLI と test の tmp repo で示す。

## 3. hook の入口

- `plugin/hooks/hooks.json`（生成 dir は core の `PLUGIN_DIR`・[consumer-sync.md](./consumer-sync.md) §17）は `cargo xtask gen-manifest` が NAME から生成する（手書きしない）。entry は 6 つ: `SessionStart`（command `"${<NAME_UPPER>_BIN:-<NAME>}" hook session-start --pane "$TMUX_PANE"`）と `PreToolUse`（matcher `Edit|Write|MultiEdit|NotebookEdit`・command `"${<NAME_UPPER>_BIN:-<NAME>}" hook pre-tool-use`）と `PermissionRequest`（matcher `Bash`・command `"${<NAME_UPPER>_BIN:-<NAME>}" hook permission-request`＝§6.5 の一律 deny）と `UserPromptSubmit`（command `… hook user-prompt-submit --pane "$TMUX_PANE"`）と `Stop`（command `… hook stop --pane "$TMUX_PANE"`）と `PreCompact`（matcher 無し＝`manual` / `auto` の両方・command `… hook pre-compact --pane "$TMUX_PANE"`＝圧縮の直前の 1 枠・[seat-roles.md §22](./seat-roles.md)・`s2-07l.489`）。打刻の 3 つ（SessionStart / UserPromptSubmit / Stop）は席の状態を `<state_dir>/seat/<target>/state.jsonl` へ typed に打つ（[seat-state.md](./seat-state.md) §2 / §3・ADR-0015・guard ではない＝極性一覧に載せない）。`PreCompact` は登録済みの席の周だけ transcript の末尾から直近の発言を `<state_dir>/seat/<target>/precompact`（1 枠・上書き）へ写し、**何が起きても圧縮を止めない**（rc 0・stdout 0 byte・guard ではない＝`NOT_A_GUARD` の 2 行）。`$TMUX_PANE` の展開も `${…_BIN}` と同じく shell が行う。timeout は rules 行 `hook.timeout_s` の値を xtask が写す。冪等（同 workspace から同 bytes）。生成物は tracked。
  - `${…_BIN:-<NAME>}` の展開は **Claude Code が hook を起動する shell** が行う。scribe2 自身は env を読まない（C2.2 に触れない）。既定は PATH 上の `<NAME>`（開発者は `cargo install --path` か PATH 追加で置く）。
- `<NAME> hook <event> [--state-dir D] [--project P]`: root は `--project`（生成 hooks.json の shell 行が渡す session の起動 dir・[seat-roles.md §4](./seat-roles.md)）があればそれ、無ければ stdin の JSON（Claude Code の hook payload・`cwd` があればそれ・無ければ process cwd）から解き、`served` が `ByMe` でなければ **stdout 0 byte・stderr 0 byte・rc 0**。未知 event も 0 byte・rc 0（fail-open・他の器と衝突しない）。
- timeout 到達は Claude Code 側で「判定の消失」＝fail-open である。guard の deny は時間切れに頼らず timeout の内側で返す（NFR5・要件カタログ R-K10）。

## 4. `session-start`

stdout に 1 行 `[<NAME>/SessionStart] served version=<N> root=<root>` を出し、§6 の record を 1 件書く。 名乗りの後に席の状態を Idle で打刻する（`--pane` が無い・空なら打刻せず黙る・[seat-state.md](./seat-state.md) §2）。登録済みの席は名乗りの後ろに指示文（[seat-roles.md §5](./seat-roles.md)）→ 圧縮の直前の 1 枠（payload の `source` が `compact` の周だけ・出した後に枠を消す・[seat-roles.md §22](./seat-roles.md)）→ 復帰の DATA（[seat-roles.md §21](./seat-roles.md)）の順で出す。

## 5. `pre-tool-use` = write-set guard（FR20・C16）

- **活性化**: 対象 worktree の git dir に policy file `<git-dir>/<NAME>/write-set.txt` が在ること。git dir は `git rev-parse --absolute-git-dir`（worktree なら `<repo>/.git/worktrees/<name>/`）。`--git-dir` は cwd 相対の `.git` を返しうるので、policy の path を組むには絶対 path を返すこちらを撃つ。**env は使わない**（C2.2・ADR-0004 §2.4）。policy file は pipeline の spawn が書く（tracked 面に触れない・`git status` を汚さない）。
- policy file の形: repo 相対 path 1 行 1 本。末尾 `/` は配下全部。glob 無し。
- 判定: payload の `tool_name ∈ {Edit, Write, MultiEdit, NotebookEdit}` で `tool_input.file_path` か `notebook_path` を repo 相対へ正規化し（`..` を含む・root 外・絶対 path で root 外 → **deny**）、allowlist の外なら **deny = rc 2 + stderr 1 行 `<NAME>: deny <path> は契約 write-set の外（C16）` + stdout 0 byte**（Claude Code の blocking error の形）。内側なら rc 0・0 byte。
- **極性**: policy file 不在 → 不活性（rc 0・0 byte＝開発 session が main を直接編集する場面）。policy file が在るのに読めない・空 → **deny（fail-closed・理由 `policy unreadable`）**。`Bash` と他 tool → write-set guard は rc 0・0 byte（interpreter 経路は v3）。`Bash` は次の command guard が見る。
- guard は C11.2 の極性一覧に `InLoop` / `FailClosed` として載る（一覧の生成と C16.2 の CI は §9 の後続契約）。
- **command guard（[ADR-0025 §2.2](../../design-intent/decisions/ADR-0025-denied-command-rows-and-bash-command-guard.html#s2-2-command-guard)・契約 = 台帳 `s2-07l.168`）**: `tool_name == "Bash"` の周に `tool_input.command` を shell の区切り（`;` `&&` `||` `|` 改行）で segment に分け、各 segment を空白で語に分け、rules 行 `runner.denied_commands`（語列の配列・[rules-manifest.md](./rules-manifest.md)）の各語列と突合する。当たり = 語列の先頭語が segment の先頭語と一致し、残りの語がすべて segment の語に含まれる（順序不問・`git push origin main --force` と `git push --force origin main` を 1 語列で当てる）。引用符の中身・変数展開・interpreter の引数（`sh -c "…"`）は解かない（v3）。当たれば **deny = rc 2 + stderr 1 行 `<NAME>: deny <語列> は rules 行 runner.denied_commands が禁じる（N1 / C16）— <次の一手>` + stdout 0 byte**・`inject.jsonl` に 1 行（`what=command-deny <語列>`）。通す周は 1 byte も書かない。判定の順序は write-set guard → seat guard → command guard → 権能 guard（閉じた列挙の宣言順・prose の順序注記を持たない）。活性化は marker が自分の NAME を言う repo だけ（FR24）。極性: rules が読めない・行が無い → **deny（fail-closed）**。全席共通（runner / planner / 管理席の弁別をしない・席ごとの例外行を持たない）。極性一覧に `Guard::Command`（`InLoop` / `FailClosed`）として 1 行増える。判定は 1 関数（`hook::command`・intake の unfit と共有・[pipeline.md §5.1](./pipeline.md#51-intakea)）。

## 6. 注入計測の slot（FR21・NFR5・C6.3）

- `pub struct InjectionRecord { schema: u64 (=1), who: String, what: String, when: String, bytes: u64, tokens: Option<u64>, wall_ms: u64, seat: Option<String>, ts: u64 }`。`seat` と `ts` は既存 key の後ろに足した optional な 2 列で、schema は 1 のまま（ADR-0004 §2.5 D-5・`s2-07l.150`）。
  - `seat` = どの席の記録か（潰した target）。hook の側は生成 hooks.json の全 entry が渡す `--pane`（`$TMUX_PANE`）から tmux の target を解いて潰す（ADR-0015 §2.2・打刻と同じ解き方）。`--pane` が無い・空・解けない周は `null`（空文字の席を作らない・`tokens` と同じく解いていない値を埋めない）。tmux を撃つのは記録を書く周だけ（毎編集には撃たない・NFR5）。
  - `ts` = 書いた時刻（1970 年からの秒・UTC）。席の打刻 `state.jsonl` の `ts` と同じ時計・同じ単位で、突合できる（fleet の RFC3339 文字列とは混ぜない）。
- `append(state_dir, &InjectionRecord)` → `<state_dir>/inject.jsonl`（flat JSON 1 行・fleet と同じ `json_lite` と lock）。state dir は §2 の git config から（`--state-dir` で上書き）。
- **C6.3 の「消費を記録する append-only store 1 つ」はこの file である**（events / verdicts は状態と審査結果）。
- `session-start` は自分の出力について `who="hook:session-start"` / `what="session-start-header"` / `when="SessionStart"` / `bytes=出力 byte 数` / `tokens=None` / `wall_ms=実測` を 1 件書く。`pre-tool-use` の deny も `who="hook:pre-tool-use"` / `what="deny"` で 1 件書く。
- 閾値 `hook.budget_ms` は rules 行（NFR5・記録との照合は検出線）。MVP は**記録だけ**。超過で止める形は v3。
- 理由（残す）: 観測 store は v3 へ送られたが、注入経路を作る本設計が schema slot を同時に予約する（後付けにしない最小履行）。

## 6.5 `permission-request` = 内蔵 guard の問いへの一律 deny（FR19 / FR21 / FR24・C11）

内蔵 Bash guard は、`rm` の path に変数展開や `$(…)` が混ざる周を筆頭に、bypassPermissions でも dialog を出す。対話 session はそこで**止まる**——無人の席では誰も答えず、席が沈黙したまま cycle が進まない。ゆえに器は **`Bash` の承認要求を一律 deny** し（答える範囲は matcher と同じ `Bash` 全体で、`rm` の周だけではない）、「allow 規則に合う形へ書き直せ」という**次の一手まで**返す（「駄目だ」だけでは席が止まる）。`rm` の literal path はその**従属句**として添える。

- 答えるのは `tool_name == "Bash"` の周だけ。stdout は**ちょうど 1 行**の `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":…}}}`・rc 0・`inject.jsonl` に 1 行（`who=hook:permission-request` / `what=deny`）。
- `Bash` 以外・payload 不能・marker が自分の NAME を言わない repo は **0 byte・rc 0** で黙る（FR24＝Claude Code の既定の問いへ戻す。器が引き受ける筋合いの無い承認まで奪わない）。
- 判定は `PermissionDecision::{Deny(String), Silent}` で、**`Allow` という variant を持たない**（憲法 C11）。承認を機械が与えると人間の承認 gate がここから空洞化する——止める側へ倒すのは安全だが、通す側へ倒すのは取り返しがつかない。
- `json_lite` は flat object 専用（`parse_object` は入れ子を error にする）ので、値の escape だけ `json_lite::quote` を通し、入れ子は組み立てる。歯は内側の決定 object を切り出して parse し、**escape が壊れていないこと**まで測る——e2e の周は文言が固定で `"` も `\` も動かないので、escape は `deny_line(&str)` へ任意の message を渡す unit の歯が測る。

## 7. 歯（契約 `s2-3ax` の検証・`tests/e2e/hook.rs` module・tmp git repo を `git init` + commit で作る・`vessel init --state-dir` で tmp を紐づける）

歯は `crates/<NAME>/tests/e2e/hook.rs` module に `hook_` / `hooks_` / `vessel_` の 3 接頭辞で置く（個々の名前はここに書かない。名前の列は現物が SSOT＝`cargo nextest list -p <NAME>`・ADR-0013 §2.1・`s2-07l.78`）。外形（usage と 1 行出力）は insta snapshot 1 本で pin する。flip 行は `cargo nextest run -p <NAME> -E 'test(hook_) | test(hooks_) | test(vessel_)' --no-tests=fail`（3 接頭辞の和）。`hooks_` を足すのは hooks.json の歯が 2 接頭辞の和から落ちるためである（`hook_` は `hooks_` に前方一致しない）。

何を測るか: SessionStart は marker が無い repo・他の name の marker・git config の無い repo（marker はあるが state dir 無し → 0 byte・rc 0）では何もせず、自分の marker が在れば stdout に `[<NAME>/SessionStart]` を出し `inject.jsonl` に 1 行（`schema=1`・`bytes>0`）を残す／guard は write-set の外への Edit を rc 2・stderr 非空・stdout 0 byte・inject.jsonl の deny 1 行で断り、root から抜ける path も断り、write-set の内側は通し、policy file が読めない周（file を dir にする）は rc 2 で fail-closed、policy file が無ければ不活性、Bash tool は見ない／`vessel init` は 2 行を出して state dir を git config へ書き、他の name の marker は `check` が rc 2・`init` は上書きを断る。

xtask 側: `crates/xtask/src/genmanifest.rs` の `#[cfg(test)]` に、render の bytes が tracked の `plugin/hooks/hooks.json` と一致し timeout が manifest の `hook.timeout_s` と一致することを測る歯を置く。

## 8. 却下案

- 活性化を env（`<NAME>_WRITE_SET_FILE`）で行う — C2.2。
- state dir を `.vessel` に書く — tracked file に path が載る（CON2）。git config（local・untracked）に置く。
- marker 名に版を入れる — 跨版契約は版番号に依らず固定（R-O3）。
- `hooks.json` を手書き — 名前の字面が散る（C2.2）。
- Bash command の parse guard — interpreter 経路（`sh -c` / script file / `python -c` の中身の解析）は v3。**語列の照合に限っては ADR-0025 が採用**（§5 の command guard・parse ではなく空白区切りの語の包含）。
- deny を stdout JSON 形にする — rc 2 + stderr の 1 形に閉じる。
- 本 repo root へ `.vessel` を leg 5 で置く — 自己ホスト便の手番と分ける（置いた瞬間から本 repo の編集に guard が効く）。

## 9. 後続（起票する契約）

- **極性一覧の build 時生成と C16.2 の CI**（C11.2 / C16.2・Always 条）: 全 guard を `InLoop` / `PostHoc` と `FailOpen` / `FailClosed` の型で列挙し build 時に一覧を生成、in-loop guard 0 件・PostHoc のみの構成を CI が RED にする。MVP の 8 契約の外なので別 bead として起票する。
- hook 予算の deny 化（rules 行の裁定 id 付き diff・C5）。他 event のうち `Stop` は打刻（ADR-0015）、`PreCompact` は圧縮の直前の 1 枠（[seat-roles.md §22](./seat-roles.md)・`s2-07l.489`）として着地済み。

## 10. 台帳 write の 4 形を起票の門で断る（契約表の行 a・`s2-07l.169`・裁定 user 2026-09-22T08:44Z）

- 出所: memo `s2-07l.169`（監査 2026-09-12 塊 13・N1 / C16 / C11.2）。**裁定 = user 2026-09-22T08:44Z**（4 形すべてを deny する）。
- **何が起きているか（現物・main f25084c・verified）**: memo の前提の前半は**偽**だった。`plugin/hooks/hooks.json` の 16 行の matcher は Bash を含み、Bash の門は 3 つ在る（`crates/scribe2/src/hook/command.rs` の語列の照合・`crates/scribe2/src/hook/role_guard.rs`・`crates/scribe2/src/hook/ledger_guard.rs`）。残るのは**起票の門が読む範囲**である: `crates/scribe2/src/hook/ledger_guard.rs` の 26 行の client の列は `bd` と `bdw` の 2 つ（path の末尾で照合）、29 行の subcommand の定数は `create` **1 つだけ**で、判定に載るのは memo の 4 節の見出しと契約の label の 2 つ（同 file 151 行の判定の純関数）。35 行の「値を取る flag」の列には `--notes` が**在る**が、それは値を title の候補に数えないための読み飛ばしで、断る経路は無い。`remember` / `recall` / `memories` と `--parent` の欠落は、同 file の 180 行の `create` の一致で判定に**載らずに通る**。
- **形**:
  1. **rules 行を 1 本足す**（裁定 id を持つ側）。id は `ledger.denied_writes`・値は断る形の**閉じた 4 語**（`notes-replace` / `memory-subcommand` / `create-without-parent` / `bd-outside-bdw`）・`enabled = true`・`ruling = "user 2026-09-22T08:44Z"`・`ruled_at = "2026-09-22"`。値は「どの形を断るか」の札で、判定そのものは code が持つ（語列の照合で書けるのは 4 形のうち 2 形だけで、残りの 2 形は**欠落**と**先頭語の弁別**という否定の条件ゆえ、`runner.denied_commands` の語列では表せない）。
  2. **rules の kind を 1 つ足す**（`crates/scribe2/src/rules/mod.rs` の 112 行の閉じた列挙に変種 1 つ・253 行の全数の列にも同じ 1 つ）。閉じた列挙ゆえ、足し忘れは `crates/scribe2/tests/e2e/rules.rs` の 466 行の歯（全数の列を回して 1 行 fixture を受理させる）が母集団ごと測る。
  3. **起票の門の判定を 4 形へ広げる**（`crates/scribe2/src/hook/ledger_guard.rs`）。29 行の subcommand の定数 1 つを、判定に載る subcommand の**閉じた列**へ替え、理由の列挙（同 file 52 行）に 4 つの変種を足す。各形の判定:
     - `notes-replace` = client が `bd` か `bdw` の segment の語に `--notes` が在る（`--notes=<値>` の連結形も、flag の側を切り出して**語全体で**照合する＝`--append-notes` には当たらない）。
     - `memory-subcommand` = subcommand が `remember` / `recall` / `memories` のどれか。
     - `create-without-parent` = subcommand が `create` で `--parent`（連結形を含む）を持たない。
     - `bd-outside-bdw` = segment の先頭語の path の末尾が `bd`（`bdw` **でない**）で、subcommand が書き込みの閉じた列に在る。書き込みの語の列は code の定数として持つ（道具の語彙であって裁定ではない＝`bd` に subcommand が増えた周は code に手が入る）。
  4. **断りの形は既存と同じ**: rc 2 + stderr 1 行 + stdout 0 byte、記録は `ledger-deny <理由の 1 語>`（`crates/scribe2/src/hook/mod.rs` の 666 行の経路・4 形とも次の一手を 1 行に持つ）。判定の順序（write-set guard → seat guard → command guard → 起票の門 → 権能 guard）も、極性（in-loop / fail-closed・境界の型は既存の 2 値のまま）も動かない＝極性一覧は 1 行も増えない。
  5. **rules を読めない周・行が無い周は deny**（fail-closed）。読む口は command guard と同じ（埋め込みの manifest・`--rules` が在ればそれ）。理由の 1 語は rules-unreadable と no-row（行が無い・不発効・値が列でない）で、rules を読むのは bd / bdw の segment を持つ command だけ（持たない command は読まずに通す＝他の Bash を巻き込まない）。memo の判定（既存の 3 つの理由）は 4 形より先に当たり、4 形の deny 文は memo の deny 文と頭を分ける（「台帳の write は起票の門が止める」）。
- **触らない**: §5 の write-set guard と command guard の判定・`runner.denied_commands` の値と語列の照合・極性一覧の行数と境界の型の名・memo の 4 節の判定と契約の label の判定（既存の 3 つの理由の字面）・`crates/scribe2/src/rules/manifest.rs` の読み（新しい kind は既存の解決を通る）・`crates/xtask/src/limits.rs`（閾値の行だけを写す面で、値が列の行は写さない＝実測）。
- **判定できない面（deny の射程の外・却下ではなく限界として残す）**: shell の別名と関数・変数展開と command 置換（門は字面のまま読む＝`crates/scribe2/src/hook/ledger_guard.rs` の 11 行の既存の宣言）・interpreter の引数の中身（`sh -c` の中の台帳の write は §8 の却下案のとおり v3）・台帳の道具の script が内側で起こす子 process（Bash tool の呼び出しではないので門に見えない＝**偽陽性にならない**側）。
- **却下**:
  - 4 形を `runner.denied_commands` の語列に足す — 語列は「先頭語の一致 + 残りの語の包含」で、**欠落**（`--parent` が無い）も**先頭語の弁別**（`bd` であって `bdw` でない）も表せない。連結形（`--notes=<値>`）も語として一致しないので `--notes` の半分が素通りする。
  - 判定を code の定数だけに置いて rules 行を持たない — 断る形の増減が裁定 id を持たなくなる（C5）。
  - 形ごとに rules 行を 4 本持つ — 1 つの裁定が 4 行に散り、`enabled` が 4 つに割れて「3 形だけ有効」という測っていない状態が作れる。
  - epic の create に例外を設ける — 席ごとの例外行と同じ形で門が緩む（C14）。epic は親を持たないので `create-without-parent` に当たるが、断り文が次の一手を返し、どうしても通す周は rules 行の値から語を 1 つ外す（＝裁定 id が付く）。
  - 台帳の道具の側（script）で止める — 器の外の道具に規律を預ける形で、道具を経ない呼び出しがそのまま残る（memo の出所がまさにこの形）。
- **歯（接頭辞 `hook_ledger_write_`・行の契約が持つ）**: `crates/` 全体で `hook_ledger_write_` を名に持つ fn は 0 件（実測）。in-file（`crates/scribe2/src/hook/ledger_guard.rs` の 287 行の `mod tests`）で判定の純関数を 4 形 × 当たる例 / 当たらない例で測り、e2e（`crates/scribe2/tests/e2e/hook.rs`・既存の起票の門の歯の隣・`hook_memo_guard_` の 3 本と同じ helper）で rc 2・stderr 1 行・stdout 0 byte・記録の 1 行までを測る。**空虚さの柵**: 当たらない例を形ごとに持つ（`--append-notes` は通る・`bdw` の書き込みは通る・`--parent` を持つ create は通る・読みの subcommand は通る）。rules の行が無い fixture と壊れた fixture で deny（fail-closed）に倒れることを別の歯で測る。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "台帳 write の 4 形（--notes の置換・記憶の subcommand・親を欠く create・台帳の script を経ない書き込み）を起票の門が断る — 断る形の閉じた列を裁定 id つきの rules 行に置き、判定は起票の門の 1 関数に足す"
req = ["FR20", "FR51"]
section = "10"
touches = ["crate::rules::RuleKind", "crate::hook::ledger_guard::Refusal"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/hook/ledger_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/tests/e2e/hook.rs", "docs/design/vessel-hook.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_ledger_write_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_kind_parity_every_kind_has_sample", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_external_form"]
size = "M"
done = "(1) 起票の門が 4 形を断る: bd と bdw のどちらでも --notes と --notes=<値> の両形が notes-replace で、remember / recall / memories が memory-subcommand で、--parent を持たない create が create-without-parent で、先頭語の末尾が bd の書き込みの subcommand が bd-outside-bdw で、それぞれ rc 2・stderr 1 行・stdout 0 byte・記録 1 行（what が ledger-deny + 理由の 1 語）になる (2) 当たらない例が形ごとに通る: --append-notes・bdw の書き込み・--parent を持つ create・読みの subcommand が rc 0 で 1 byte も書かない (3) rules 行 ledger.denied_writes が kind・値の 4 語・enabled・裁定 id user 2026-09-22T08:44Z の 4 面で引け、外形 snapshot の rows= と kinds= を持つ 2 行がどちらも 55 から 56 になる (4) 足した kind が全数の列に在って 1 行 fixture が受理される〔rules_kind_parity_every_kind_has_sample〕 (5) rules が読めない周と行が無い周は deny（fail-closed） (6) 既存の 3 つの理由（memo の 4 節・契約の label・本文が読めない）の字面と rc・判定の順序・極性一覧の行数が 1 字も変わらず、hook_memo_guard_ の 3 本が緑"
<!-- contracts:end -->
