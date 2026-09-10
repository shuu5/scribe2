# 設計: `.vessel` marker と hook — 名乗り・in-loop guard・注入計測の slot

- 要件: [FR19](../../design-intent/spec/srs.html#FR19) marker と hook / [FR20](../../design-intent/spec/srs.html#FR20) in-loop guard / [FR21](../../design-intent/spec/srs.html#FR21) 注入計測の slot / [FR24](../../design-intent/spec/srs.html#FR24) 管轄外での沈黙 / [AC7](../../design-intent/spec/srs.html#AC7) / [NFR5](../../design-intent/spec/srs.html#NFR5)。制約: CON2 / CON3
- 憲法: [C2](../../design-intent/spec/constitution.html#c2) 名前は 1 定数・env 直読禁止 / [C6](../../design-intent/spec/constitution.html#c6) 消費の store は 1 つ / [C11](../../design-intent/spec/constitution.html#c11) 極性は型で / [C16](../../design-intent/spec/constitution.html#c16) 逸脱は編集の時点で止める
- 決定: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.2（面 1 = `.vessel` の字面）/ §2.4（policy と state dir の受け渡し）
- crate の形は [rules-manifest.md §2](./rules-manifest.md) に従う。
- この設計から出る契約: `s2-3ax`（plugin hooks + marker + guard）。極性一覧と C16.2 の CI は後続（§9）。

## 1. 何を解くか

3 つの面を 1 binary の `hook` subcommand で持つ。

1. **所属（跨版 面 1）**: repo root の固定名 marker `.vessel` が「自分の NAME」を言うときだけ仕え、それ以外は **stdout 0 byte・rc 0** で黙る（FR19 / FR24）。
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
- **本 repo の root に `.vessel` を置くのは自己ホスト便（AC2・[pipeline.md §9](./pipeline.md)・`s2-07l.24`）の手番**。leg 5 では CLI と test の tmp repo で示す。

## 3. hook の入口

- `hooks/hooks.json` は `cargo xtask gen-manifest` が NAME から生成する（手書きしない）。entry は 3 つ: `SessionStart`（command `"${<NAME_UPPER>_BIN:-<NAME>}" hook session-start`）と `PreToolUse`（matcher `Edit|Write|MultiEdit|NotebookEdit`・command `"${<NAME_UPPER>_BIN:-<NAME>}" hook pre-tool-use`）と `PermissionRequest`（matcher `Bash`・command `"${<NAME_UPPER>_BIN:-<NAME>}" hook permission-request`＝§7 の一律 deny）。timeout は rules 行 `hook.timeout_s` の値を xtask が写す。冪等（同 workspace から同 bytes）。生成物は tracked。
  - `${…_BIN:-<NAME>}` の展開は **Claude Code が hook を起動する shell** が行う。scribe2 自身は env を読まない（C2.2 に触れない）。既定は PATH 上の `<NAME>`（開発者は `cargo install --path` か PATH 追加で置く）。
- `<NAME> hook <event> [--state-dir D]`: stdin の JSON（Claude Code の hook payload・`cwd` があればそれ・無ければ process cwd）から root を解き、`served` が `ByMe` でなければ **stdout 0 byte・stderr 0 byte・rc 0**。未知 event も 0 byte・rc 0（fail-open・他の器と衝突しない）。
- timeout 到達は Claude Code 側で「判定の消失」＝fail-open である。guard の deny は時間切れに頼らず timeout の内側で返す（NFR5・要件カタログ R-K10）。

## 4. `session-start`

stdout に 1 行 `[<NAME>/SessionStart] served version=<N> root=<root>` を出し、§6 の record を 1 件書く。

## 5. `pre-tool-use` = write-set guard（FR20・C16）

- **活性化**: 対象 worktree の git dir に policy file `<git-dir>/<NAME>/write-set.txt` が在ること。git dir は `git rev-parse --absolute-git-dir`（worktree なら `<repo>/.git/worktrees/<name>/`）。`--git-dir` は cwd 相対の `.git` を返しうるので、policy の path を組むには絶対 path を返すこちらを撃つ。**env は使わない**（C2.2・ADR-0004 §2.4）。policy file は pipeline の spawn が書く（tracked 面に触れない・`git status` を汚さない）。
- policy file の形: repo 相対 path 1 行 1 本。末尾 `/` は配下全部。glob 無し。
- 判定: payload の `tool_name ∈ {Edit, Write, MultiEdit, NotebookEdit}` で `tool_input.file_path` か `notebook_path` を repo 相対へ正規化し（`..` を含む・root 外・絶対 path で root 外 → **deny**）、allowlist の外なら **deny = rc 2 + stderr 1 行 `<NAME>: deny <path> は契約 write-set の外（C16）` + stdout 0 byte**（Claude Code の blocking error の形）。内側なら rc 0・0 byte。
- **極性**: policy file 不在 → 不活性（rc 0・0 byte＝開発 session が main を直接編集する場面）。policy file が在るのに読めない・空 → **deny（fail-closed・理由 `policy unreadable`）**。`Bash` と他 tool → rc 0・0 byte（interpreter 経路は v3）。
- guard は C11.2 の極性一覧に `InLoop` / `FailClosed` として載る（一覧の生成と C16.2 の CI は §9 の後続契約）。

## 6. 注入計測の slot（FR21・NFR5・C6.3）

- `pub struct InjectionRecord { schema: u64 (=1), who: String, what: String, when: String, bytes: u64, tokens: Option<u64>, wall_ms: u64 }`。
- `append(state_dir, &InjectionRecord)` → `<state_dir>/inject.jsonl`（flat JSON 1 行・fleet と同じ `json_lite` と lock）。state dir は §2 の git config から（`--state-dir` で上書き）。
- **C6.3 の「消費を記録する append-only store 1 つ」はこの file である**（events / verdicts は状態と審査結果）。
- `session-start` は自分の出力について `who="hook:session-start"` / `what="session-start-header"` / `when="SessionStart"` / `bytes=出力 byte 数` / `tokens=None` / `wall_ms=実測` を 1 件書く。`pre-tool-use` の deny も `who="hook:pre-tool-use"` / `what="deny"` で 1 件書く。
- 閾値 `hook.budget_ms` は rules 行（NFR5・記録との照合は検出線）。MVP は**記録だけ**。超過で止める形は v3。
- 理由（残す）: 観測 store は v3 へ送られたが、注入経路を作る本設計が schema slot を同時に予約する（後付けにしない最小履行）。

## 6.5 `permission-request` = 内蔵 guard の問いへの一律 deny（FR19 / FR21 / FR24・C11）

内蔵 Bash guard は、`rm` の path に変数展開や `$(…)` が混ざると bypassPermissions でも dialog を出す。対話 session はそこで**止まる**——無人の席では誰も答えず、席が沈黙したまま cycle が進まない。ゆえに器が**一律 deny**で答え、「literal path で書き直せ」という**次の一手まで**返す（「駄目だ」だけでは席が止まる）。

- 答えるのは `tool_name == "Bash"` の周だけ。stdout は**ちょうど 1 行**の `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":…}}}`・rc 0・`inject.jsonl` に 1 行（`who=hook:permission-request` / `what=deny`）。
- `Bash` 以外・payload 不能・marker が自分の NAME を言わない repo は **0 byte・rc 0** で黙る（FR24＝Claude Code の既定の問いへ戻す。器が引き受ける筋合いの無い承認まで奪わない）。
- 判定は `PermissionDecision::{Deny(String), Silent}` で、**`Allow` という variant を持たない**（憲法 C11）。承認を機械が与えると人間の承認 gate がここから空洞化する——止める側へ倒すのは安全だが、通す側へ倒すのは取り返しがつかない。
- `json_lite` は flat object 専用（`parse_object` は入れ子を error にする）ので、値の escape だけ `json_lite::quote` を通し、入れ子は組み立てる。歯は内側の決定 object を切り出して parse し、**escape が壊れていないこと**まで測る。

## 7. 歯（契約 `s2-3ax` の検証・`tests/e2e/hook.rs` module・tmp git repo を `git init` + commit で作る・`vessel init --state-dir` で tmp を紐づける）

flip 行は `cargo nextest run -p <NAME> -E 'test(hook_) | test(vessel_)' --no-tests=fail`（2 接頭辞の和）。

`hook_session_start_is_noop_without_marker` / `hook_session_start_is_noop_for_other_name` / `hook_session_start_is_noop_without_state_dir`（marker はあるが git config 無し → 0 byte・rc 0）/ `hook_session_start_serves_own_marker`（stdout に `[<NAME>/SessionStart]` ∧ `inject.jsonl` に 1 行・`schema=1`・`bytes>0`）/ `hook_guard_denies_edit_outside_write_set`（rc 2 ∧ stderr 非空 ∧ stdout 0 byte ∧ inject.jsonl に deny 1 行）/ `hook_guard_denies_path_escaping_root` / `hook_guard_allows_edit_inside_write_set` / `hook_guard_fails_closed_when_policy_unreadable`（policy file を dir にする → rc 2）/ `hook_guard_is_inactive_without_policy_file` / `hook_guard_ignores_bash_tool` / `vessel_init_renders_two_lines_and_writes_state_dir_config` / `vessel_check_rc2_for_other_name` / `vessel_init_refuses_to_overwrite_other_name` / `vessel_external_form`（snapshot）。

xtask 側: `crates/xtask/src/genmanifest.rs` の `#[cfg(test)]` に `gen_manifest_hooks_json_is_idempotent`（render の bytes == tracked `hooks/hooks.json`・timeout は manifest の `hook.timeout_s` と一致）。

## 8. 却下案

- 活性化を env（`<NAME>_WRITE_SET_FILE`）で行う — C2.2。
- state dir を `.vessel` に書く — tracked file に path が載る（CON2）。git config（local・untracked）に置く。
- marker 名に版を入れる — 跨版契約は版番号に依らず固定（R-O3）。
- `hooks.json` を手書き — 名前の字面が散る（C2.2）。
- Bash command の parse guard — interpreter 経路は v3。
- deny を stdout JSON 形にする — rc 2 + stderr の 1 形に閉じる。
- 本 repo root へ `.vessel` を leg 5 で置く — 自己ホスト便の手番と分ける（置いた瞬間から本 repo の編集に guard が効く）。

## 9. 後続（起票する契約）

- **極性一覧の build 時生成と C16.2 の CI**（C11.2 / C16.2・Always 条）: 全 guard を `InLoop` / `PostHoc` と `FailOpen` / `FailClosed` の型で列挙し build 時に一覧を生成、in-loop guard 0 件・PostHoc のみの構成を CI が RED にする。MVP の 8 契約の外なので別 bead として起票する。
- hook 予算の deny 化（rules 行の裁定 id 付き diff・C5）。`PreCompact` / `Stop` 等の他 event（v3）。
