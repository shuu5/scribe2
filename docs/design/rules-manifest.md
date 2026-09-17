# 設計: rules manifest — 規則の種類は閉じた enum・値は tracked な TOML 1 file

- 要件: [FR17](../../design-intent/spec/srs.html#FR17) rules manifest / [FR18](../../design-intent/spec/srs.html#FR18) rules の検査 / [AC6](../../design-intent/spec/srs.html#AC6) / [NFR3](../../design-intent/spec/srs.html#NFR3) 依存 0 本 / [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed。制約: CON2（PUBLIC）/ CON3（歯は Rust だけ）
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) 規則はデータ / [C5](../../design-intent/spec/constitution.html#c5) 規則変更 = manifest diff + 裁定 id / [C12](../../design-intent/spec/constitution.html#c12) 外形 snapshot / [C14](../../design-intent/spec/constitution.html#c14) 規律の 2 面 / [§3](../../design-intent/spec/constitution.html#s3-rules-rows) rules 行の初期値
- 決定: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.3（TOML subset・std だけの scanner・binary への埋め込み）
- この設計から出る契約: `s2-chg`（rules manifest）。後続の契約は末尾「後続」に列挙する。

## 1. 何を解くか

規則の**種類**を Rust の閉じた型で持ち、規則の**値**（閾値・有効 / 無効・裁定 id・裁定時刻）を repo 内の TOML 1 file に置く。機械が実行時に読む規則はこの file だけである（C1）。**行はすべて user の裁定 id を持つ**（C5・裁定 id の無い行は loader が拒む）。AI が提案した数値は、user の裁定を得てから行にする。憲法 §3 の表は「manifest が存在しない間の唯一の存在箇所」と自らを定義しているので、本設計で manifest が生まれた時点から §3 の各行は manifest の写しになる。写しと正本の drift は C14.2 が CI の歯で止めると定めているが、その歯は本設計の射程外（後続）である。

やさしく言うと: 「何を守るか」の一覧は Rust の enum に、「いくつまでか」の数字は TOML に書く。数字を書く行には必ず user がいつ決めたかを添える。

## 2. crate の形（本設計で決める・4 設計 doc に共通）

- `crates/<NAME>/` は **lib target + bin target** にする。lib は `src/lib.rs`（`pub mod rules;` 等の module 列挙）、bin は薄い `src/main.rs`（引数の dispatch と出力層 `emit` / `emit_err` だけ）。統合 test は lib の `pub` API と `env!("CARGO_BIN_EXE_<NAME>")` の binary の両方を叩く。
- lib target の名前は `[lib] name` で **NAME の字面を含まない別名**（例: `vessel`）にする。理由: `cargo xtask check` の `name-literal` は `crates/*/src` の `.rs` に NAME の字面が `name.rs` 以外に無いことを測るので、`main.rs` が `use <NAME>::…` と書くと落ちる。`manifest-name` は `[package] name` だけを見るので `[lib] name` は自由（実測: xtask `check.rs` の `core_package_field("name")`）。
- `name.rs` は lib へ移す（NAME の字面を持つ `.rs` は引き続き 1 本）。
- 出力層: stdout は `emit`、stderr は `emit_err`（`#[expect(clippy::print_stderr, reason = …)]`・xtask と同形）。**`emit_err` は本 leg で core crate に新設する**（現状は xtask にしか無い）。
- 統合 test は `crates/<NAME>/tests/e2e/main.rs` の **1 target**（`mod rules; mod fleet; …` の module 形）。憲法 R-C13-2「統合 test file 3 以下」は cargo の integration test **target** の数で数える（`tests/<dir>/main.rs` は 1 target・実測: `cargo metadata` の targets は bin 1 + test 1・解釈の記録は ADR-0007）。tmp dir は std だけの helper を `tests/e2e/main.rs` に 1 本置く（`tempfile` は A3 ゆえ足さない・xtask `check.rs` の `make_tmp_dir` と同形）。
- 外形（CLI の usage / 1 行出力の形）は insta snapshot 1 本で固定する（C12.5・既存の `doctor_external_form` と同形）。

## 3. 型（`rules` module）

- `pub enum RuleKind` — **閉じた列挙・`#[non_exhaustive]` 禁止**。variant を足したら網羅 `match` が compile error になる形を保つ（C1 / C11）。variant は §4.1 の行と 1:1。
- `pub const ALL: &[RuleKind]` — parity test の母集団。
- `pub enum RuleValue { Int(u64), Str(String), Policy(String), List(Vec<String>) }`。`Int` = 閾値（単位は kind の doc コメント）。`Str` = 識別子（対話面の identity）。`Policy` = 散文で書かれた選定規則や検出線の定義（機械は `enabled` だけを読む。本文は憲法 §3 の写し）。`List` = 文字列の列（allowlist・共通 verify のような**順序のある複数値**・ADR-0009）。**空の配列は受けない**——「規則が無い」を空で表せると、書き間違いの `value = []` が空の allowlist を効かせる。要素の空文字も受けない（何もしない口を規則の顔で並べない）。配列を**切る**実装（`elements` / `quoted_once`）は `rules::manifest` の 1 組で、`pipe` の契約 file もこれを呼ぶ（同じ入力を別々に切って、区切り忘れの扱いが片側だけ直る事故を防ぐ）。ただし切った後の**方針**——空を拒むか・要素の空文字を拒むか・違反を全件集めるか最初の 1 件で止めるか——は呼び手ごとに違い、manifest 側は `list`、契約 file 側は`value_of` が持つ（**方針の層は 1 本ではない**・畳むなら別便）。
- `pub trait Rule { fn kind(&self) -> RuleKind; fn validate(&self) -> Result<(), RuleError>; }` を `RuleRow` に実装。kind ごとの値型の対応は **`match kind { … }` 1 箇所・wildcard `_` 無し**。
- `pub struct RuleRow { id, kind, value, enabled, ruling: String, ruled_at: String, line: u64 }`。
- `RuleError` は `Display` で **1 件 1 行・`line=<N>` を含む**。
- **閉じた enum と const slice の突合**（`s2-07l.177`）: xtask の enum-slices は集合の一致に加えて**順序**（同じ添字で同じ名）も測り、順序違いを添字付きで名指す。極性を持つ境界（`crates/*/src` の `const <NAME>: Polarity` の宣言 site・名は `POLARITY` に限らず〔現物は別名 5 件〕、`impl <Type> {` の中の関連 const は crate::<mod>::<Type>::<NAME> の path で数える）は `Guard` の網羅 match が参照する path の集合と両方向で突合し、guard でない境界は `polarity.rs` の閉じた const slice（`NOT_A_GUARD`・型は `&[Polarity]`・要素は site の path）に載せる（doc コメントで除外しない）。site も arm も 0 の木は 0 で通す（enum-slices の母集団 0 と同じ）。
- **CLAUDE.md の生成区間**（`s2-07l.173`）: 憲法の区間に加えて「done の定義」も `.github/workflows/ci.yml` の `run: cargo …` 行（job の宣言順・`${{ … }}` の穴は引用符ごと `<base>`）から xtask が生成する区間（`<!-- done:begin -->` … `<!-- done:end -->`）にし、tracked との差分を xtask check の `claude-md-done` が落とす（違う行を両側の字面で名指す）。正本を CI の側に置くのは、CI の定義が job の条件と cache の行を持ち逆向きの生成にならないからである。生成区間の外の規範行（散文の門と同じ印・pointer 無し）は検出線 `claude-md-prose=<該当行>/<区間外の非空行>` が件数を出す（rc は変えない・deny 化は C12.4 の型で裁定を経る）。

## 4. manifest（`rules/manifest.toml`・tracked・repo root 直下）

TOML subset（先頭に `schema = 1`・`[[rule]]` の array-of-tables・値は string / integer / bool と**文字列の配列**〔1 行で閉じる〕・未知 key は拒否）。loader は std だけで書く（ADR-0004 §2.3・NFR3）。

```toml
schema = 1

[[rule]]
id = "R-C4-1"
kind = "CoreLines"
value = 20000
enabled = true
ruling = "RULING-v2-p1-exit-bundle 論点 2"
ruled_at = "2026-09-07"
```

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `id` | string | 必須・一意 | 行 id。§3 の行は `R-<条>-<番号>`、compound 行は `R-<条>-<番号>.<key>`（§3 の行 id が**接頭辞**として一致）、MVP の運用値は `<領域>.<名>` |
| `kind` | string | 必須 | `RuleKind` の variant 名（字面一致） |
| `value` | integer / string / 文字列の配列 | 必須 | kind が定める型。`Policy` は string、`List` は 1 行で閉じる string array（空・空要素は loud） |
| `enabled` | bool | **必須** | false = 値は写すが機械は効かせない（停止・v3 送り）。省略は拒む（裁定 `user 2026-09-11T23:59Z`）＝書き忘れた行を true で埋めると、発効が既定に化ける |
| `ruling` | string | **必須** | 裁定 id。憲法 §3 と同じく裁定文書の論点番号、または裁定の UTC ts |
| `ruled_at` | string | **必須** | 裁定の日付（UTC・分秒が資料に無ければ日まで） |

**書かない key**: user 逐語（PUBLIC・CON2）。**置かない行**: 値が未定の行（§3 R-C6-1）・実装が無い seam の行（rtk / graphify のモード・C10.3「未 wire の設定」）・AI が置いただけで裁定の無い行。

**行の読み手と 2 面の突合**（`cargo xtask check` の検出線・deny 化は rules 行と裁定 id で行う）: enabled な行のうち `crates/*/src` に const の値として現れ使われる読み手が無い行は `rules-wired` が名指す（`s2-07l.160`・fact は `rules-wired=<読み手の無い本数>/<enabled の本数> ids=<列>`・宣言側の 4 file〔`rules/mod.rs` / `rules/manifest.rs` / `genmanifest.rs` / `rules_diff.rs`〕・test 区間・`enabled = false` の行は母集団外・rc は変えない）。憲法 §3 の `<tr id="r-…">` の id 集合と manifest の `R-…` 行の接頭辞集合は `rules-parity` が双方向に突合し、片側だけの id を名指す（`s2-07l.164`・運用行は母集団外・HTML の読み手は `claude_md.rs` を共有する）。畳み方（.164 run 1 審査 FAIL 2026-09-16「契約の畳み方と §4.1 の行が食い違う」の解として本文に置く）: manifest の id は **§3 の行 id の形 `R-<条>-<番号>`（`^R-C\d+(\.\d+)?-\d+`）に一致する接頭辞**だけを畳む（compound 行 `R-C4-4.fn-lines` → `R-C4-4`）。その形を持たない `R-…` 行（現物: `R-C4.line-width`・§3 に対応する行 id が無い）は**自身の id のまま manifest-only に数える**（`R-C4` に畳まない・0 に潰さない）。現物の期待値 = `doc-only=1 ids=R-C6-1` / `manifest-only=1 ids=R-C4.line-width`（両方とも検出線の記録で rc は変えない・`R-C4.line-width` の id の形を §3 の行に揃えるかは憲法 §3 の行の追加＝user 裁定・別便）。`R-C13-1.per-pr` / `R-C13-1` の読み手は `xtask deps-delta`（`--base` との直接依存の差分・超えれば PR の入口で落ちる）で、依存を足した便の `check-delta-ms` は検出線として判定行に残る（`s2-07l.161`）。`deps-delta` の判定行は 1 形 `deps-delta: base=<sha> added=<n> limit=<R-C13-1.per-pr> total=<n> budget=<R-C13-1> ids=<列|-> check-delta-ms=<ms|skipped|-> check-limit-ms=<R-C13-1.check-delta-ms>`（直接依存は (section, crate 名) の対・節の分類は check の `deps-empty` と同じ 5 形・base に無い `Cargo.toml` は空集合）。rc は **deny の面だけ**で決まる: `added` が per-pr を超える、または `total` が budget を超えれば rc 1、そうでなければ rc 0。`check-delta-ms` は C13.5 の検出線で、`added` = 0 の便は `skipped`、測れなかった便（cargo を spawn できない・base か HEAD の `cargo check` が rc 非 0）は **`-`** で判定行に残し stderr の診断 1 行を添えるだけで rc に触れない（測れなかった compile 秒で PR を止めない・`-` は数値でないので測定に化けない = C10）。rc 2 は §4.3 と同じく**突合の鍵が壊れた周だけ**（`--base` 不在・base の sha が解けない・HEAD の `Cargo.toml` が読めない）で、判定行を出さず `deps-delta: unmeasurable reason=<base-unreadable|head-unreadable>` を stderr へ出す。 閉包の面: `Limits` の読み手は無い行を拒むので、check の歯の fixture（`check_tests.rs` の `rules_manifest`）が実 repo と同じ閾値の行を持つ＝行を足す便は fixture に同じ 2 行を足す。CI の run 行を足す便は CLAUDE.md の done 区間（`gen-claude-md` の生成物・`claude-md-done` の drift 歯）を同じ PR で再生成する（手編集しない）。PUBLIC 面の門 `private-clean` の needle は **4 形**（email / users-path / ledger-id-v1 / state-dir-path・閉じた enum の variant 1 つずつ・§11・`s2-07l.174`）。

### 4.1 初期行（§3 の写し + MVP の運用値・全行に裁定 id）

| id | kind | value | enabled | 裁定（ruling / ruled_at）・design-intent 側の出所 |
|---|---|---|---|---|
| `R-C4-1` | CoreLines | 60000 | true | user 裁定 2026-09-15T11:2xZ（A2・**一時的**な緩和・s2-07l notes）・定期 refactor は .198（前値 40000 = user 裁定 2026-09-14〔A2・s2-07l notes〕・初期値 20000 = 論点 2 / 2026-09-07・憲法 §3）・**母集団は core crate の src の本体だけ**（in-file の歯〔`#[cfg(test)]` 区間〕は数えない＝R-C4-3 が数える側・user 裁定 2026-09-15・ADR-0033・[core-boundary.md](./core-boundary.md) §2） |
| `R-C4-2` | ModuleLines | 1500 | true | 同上 |
| `R-C4-3` | TestSrcRatioPct | 100 | true | 同上（比 1.0 = 100%） |
| `R-C4-4.fn-lines` | FnLines | 60 | true | 同上 |
| `R-C4-4.complexity` | FnComplexity | 15 | true | 同上 |
| `R-C4-4.args` | FnArgs | 5 | true | 同上 |
| `R-C4.line-width` | LineWidth | 120 | true | user 裁定 2026-09-14（s2-07l notes・逐語「推奨で良い」）・**行の数え方の正規化**: 1 行の文字数が値を超える行は ceil(文字数 ÷ 値) 行に数える（値以下の行は 1 行）。R-C4-1 / R-C4-2 / R-C4-3 の行数（xtask check の core-lines / file-lines / test-src-ratio）と契約表の上限の余地（[contract-source.md](./contract-source.md) §3）が同じ式で数える＝1 行に詰め込んでも上限は逃げない。式は core と xtask の 2 か所（互いに依存しない）に在り、同じ fixture の歯が一致を守る。契約 = 台帳 `s2-07l.254` |
| `pipe.size_s_lines` / `pipe.size_m_lines` / `pipe.size_l_lines` | PipeSizeSLines / PipeSizeMLines / PipeSizeLLines | 100 / 300 / 800 | true | user 裁定 2026-09-14（s2-07l notes・逐語「推奨でよいのだが…」）・契約表の行の `size`（S / M / L）↔ 1 file あたりの増分の見積（行）。契約表の上限の余地（contract-source.md §3）が読む。契約 = 台帳 `s2-07l.249` |
| `R-C7-1` | DialogueSurface | `"user-direct"` | true | ADR-0003 / 2026-09-09・憲法 §3 |
| `R-C8-1` | MaturityCondition | Policy（§3 の文） | **false** | 論点 9・ADR-0003 / 2026-09-09（停止・履歴） |
| `R-C9-1` | AccountSelection | Policy（§3 の文） | **false** | 論点 8・U5 / 2026-09-07（口座は v3） |
| `R-C12-1` | MutationSurvivalLine | Policy（§3 の文） | **false** | 論点 7 / 2026-09-07（検出線は未配線） |
| `R-C13-1` | DepBudget | 12 | true | 論点 3・4 / 2026-09-07 |
| `R-C13-1.per-pr` | DepPerPr | 1 | true | 同上 |
| `R-C13-1.check-delta-ms` | CheckDeltaMs | 300 | true | 同上 |
| `R-C13-2` | CompileShape | Policy（**§3 の逐語**・target 数で数える解釈は §2 に置き manifest には書かない） | true | 同上 |
| `R-C13-3` | CompileSeconds | Policy（§3 の文） | **false** | 同上（検出線は未配線） |
| `gate.lens_count` | GateLensCount | 1 | true | grill U3 / 2026-09-07・SRS scope「lens 1 本」 |
| `gate.token_cap` | GateTokenCap | 400000 | true | user 2026-09-15T23:31Z（純移動でない 169 KB の diff s2-07l.209 を lens に通す一時の上げ・戻しは s2-07l.376 = 行 i・前例 s2-07l.265 / .267・上げ前の値 150000 = grill U3 / 2026-09-07）・SRS NFR1（目標値 150000 へは行 i で戻す） |
| `hook.budget_ms` | HookBudgetMs | 2000 | true | SRS NFR5 の user 承認 / 2026-09-09・要件カタログ R-K22 |
| `pipe.stop_grace_ms` | StopGraceMs | 2000 | true | user 2026-09-09T09:08Z / 2026-09-09 |
| `fleet.lock_retry_ms` | LockRetryMs | 5000 | true | user 2026-09-09T09:08Z / 2026-09-09 |
| `fleet.lock_stale_ms` | LockStaleMs | 30000 | true | user 2026-09-09T09:08Z / 2026-09-09 |
| `hook.timeout_s` | HookTimeoutS | 10 | true | user 2026-09-09T09:08Z / 2026-09-09（hooks.json の timeout・xtask がここから写す） |
| `runner.allowed_commands` | RunnerAllowedCommands | `["cargo", "git", "bats"]` | true | user 裁定 2026-09-14（uns の vessel 宣言のため・bash / sh は足さない・`s2-07l.271`。初期値 `["cargo", "git"]` は user 裁定 2026-09-10・ADR-0009 §2.1）。**意味は上限**（[ADR-0010 §2.2](../../design-intent/decisions/ADR-0010-vessel-declaration-holds-allowlist-and-common-verify.html#s2-2-manifest-ceiling)）＝各 repo の vessel 宣言 `allowed-commands` はこの部分集合でなければ intake が拒む |
| `runner.denied_commands` | RunnerDeniedCommands | `["cargo mutants", "cargo publish", "git push --force", "git push -f", "git reset --hard", "git branch -D", "git clean -f", "git stash drop", "git stash clear"]` | true | user 裁定 2026-09-14（台帳 `s2-07l.168` notes・[ADR-0025 §2.1](../../design-intent/decisions/ADR-0025-denied-command-rows-and-bash-command-guard.html#s2-1-denied-row)）。**上限側の禁止**＝`runner.allowed_commands` と対で読み、vessel 宣言は緩められない。hook の command guard（[vessel-hook.md §5](./vessel-hook.md)）と intake の unfit（[pipeline.md §5.1](./pipeline.md)）が同じ 1 関数で語列（先頭語一致 + 残りの語の包含・順序不問）を当てる。manifest への追加は契約 `s2-07l.168` の便（行・variant・件数の歯・外形 snapshot を 1 PR で） |
| `repo.non_rust_exec_allow` | RepoNonRustExecAllow | `["scripts/bdw", "design-intent/assets/mermaid.min.js"]` | true | user 裁定 2026-09-10（`s2-07l` notes 10:4xZ (2)・[ADR-0009 §2.5](../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html)）。**tracked な非 Rust 実行物の例外**＝`cargo xtask check` の measure `non-rust-exec` が、閉じた分類器（先頭 2 byte `#!` / index の mode 100755 / 拡張子 sh bash zsh py bats pl rb js ts mjs）に当たる path のうち**この列に完全一致で載るものだけ**を通す（載らない 1 件で rc≠0・母集団 0 は「測れなかった」で rc≠0）。**分類器を緩めず例外を 1 面へ集める**のが趣旨で、`js` を分類器から外すと次の asset が黙って通る。同じ便で足した `ci-shell-lines` は `.github/workflows/*.yml` の `run:` 行（block scalar の継続行を含む）を数える**検出線**で、deny しない |
| ~~`gate.common_verify`~~ | ~~GateCommonVerify~~ | — | — | **除去済み**（`s2-07l.57`・ADR-0010 §2.2・裁定 id = ADR-0010）＝共通 verify の値は対象 repo の vessel 宣言 `common-verify` が持ち、gate と land は**便の写し** `vessel.toml` から読む。行・variant・`ALL` の 3 点を 1 PR で除いた |

variant の母集団は `rules::ALL`（`crates/scribe2/src/rules/mod.rs`）・行の一覧は tracked の現物 `rules/manifest.toml` が正本（この表は写しで、件数はここに書かない＝2 面の drift は `cargo xtask check` の `rules-wired` / `rules-parity` が判定行に出す・§4）・§3 の行 id は manifest の行 id の接頭辞（畳み方は §4）。

### 4.2 拒否 5 形（FR18・AC6・すべて `line=<N>` 付き・全件を集めて返す）

1. 未知 `kind`
2. `id` 重複
3. `ruling` か `ruled_at` の無い行（C5・全行）
4. `value` の型が kind の対応と不一致
5. 必須 key（`id` / `kind` / `value` / `enabled` / `ruling` / `ruled_at`）の欠落、未知 key、`schema` 不在または 1 以外

**最初の違反で止めない**（silent drop 禁止・NFR4）。

### 4.3 差分の門（C5 の「変える」側・`xtask rules-diff`・監査 2026-09-12 塊 4 = `s2-07l.162`）

§4.2 の拒否 5 形は行の**静的な形**（key の在不在・型）だけを見る。値を変えて裁定 id を据え置く便（`value` を動かし `ruling` を古いまま出す）は loader も CI も通り、AI が単独で規則の値を動かせる。C5 は「行を足す / 変える」という **diff の事実**に裁定を結びつける条なので、その面は base との比較でしか測れない。

- 口: `cargo xtask rules-diff --base <sha>`（flip-check / mutants-diff と同じ `--base`・PR job で撃つ＝push(main) には base が無い）。base 側の manifest は `git show <base>:rules/manifest.toml` で読み、HEAD 側は tracked の現物を読む。両方を xtask の scanner（`toml_lite`・section と key = value の字面だけを読む・§4.2 の判定は持たない）で行に分ける。§4.2 の 5 形は loader の門（core・埋め込みと `--rules` の両経路）が別に守るので本門の射程外だが、**`id` の重複だけは突合の鍵を壊す**ので、base か HEAD に重複が在れば突合の前に rc 2 で止める。
- 判定（行 id で突合・列挙順は HEAD の行順）: (i) base と HEAD の両方に在り `value` か `enabled` が違う行は、`ruling` の字面が base の同じ行と同じなら **違反**、字面が変わっていても base の**別の行**の `ruling` と同じなら **違反**（隣の行の裁定を貼る形・(ii) と同じ相乗り）。`ruled_at` だけの打ち直しは変化に数えない（裁定の証拠は id の側）。(ii) HEAD にだけ在る行（新設）は、`ruling` の字面が base の**別の行**の `ruling` と同じなら **違反**（過去の裁定への相乗り）。同じ便で新設した複数の行が 1 つの裁定を共有する形は違反にしない（1 裁定で複数の値を決めた周の通常形・§4.1 の S / M / L）。(iii) base にだけ在る行（除去）は本門の対象外（除去は kind の variant と 1 PR で行い、§4.1 の除去済み行の型で残す）。(iv) `id` / `value` / `enabled` / `ruling` 以外の key の差は見ない。
- 出力と極性: 判定行 1 本 `rules-diff: base=<sha> rows=<HEAD の行数> changed=<(i)+(ii) の母集団> violations=<件数>` の後に違反 1 件 1 行（`rules/manifest.toml:<line> <id> <(i)|(ii)> ruling=<字面>`）。違反 0 で rc 0・1 件以上で rc 1・base の manifest を読めない / scanner が行に分けられない / `id` 重複の周は rc 2（**測れないを緑に化けさせない**・mutants-diff の rc 2 と同じ慣例）。数値の閾値を持たない（母集団と件数を出すだけ）。
- 射程外: 裁定 id の**様式**（user ts 形か ADR id か）と、裁定 id が指す裁定が**実在するか**は本門で見ない（裁定の正本は design-intent の側・C14）。値を戻す便（`value` を base の値へ戻す）も (i) の通常形として裁定 id を要る側に倒す。`--base` は PR の base の tip であって merge-base ではない（flip-check と共有の既知の限界）。行を除いて別 id で立て直す形は (iii) の除去 + (ii) の新設として (ii) の相乗り検査だけが効く。

憲法 §3 rules 表の閾値セルは manifest の**写し**であり（値の正本は manifest 側）、両者の一致は xtask の歯 `constitution_thresholds_match_rules_manifest` が守る（R-C4-1 / R-C4-2 / R-C4-3 と R-C4-4 の 3 値＝計 6 個を順序込みで突合・**閾値の数値を変える手編集は RED**・行の重複や死骸で隠す形も RED・改訂形の `<del>` は落として `<ins>` 側を読む＝整合した改訂は緑で `<ins>` だけ変えた周は RED）。**測っていない面**は 3 つある: 条の向き（`以下` → `以上`）・桁区切りの位置（`20,000` → `2,0000`＝`,` を落として読むため値は同じ）・§3 に**新しい数値行を足した**周（歯が見るのは `r-c4-1`〜`r-c4-4` の 4 行に固定）。数値を持たない prose 行（R-C6-1 以降・R-C13-*）も突合できず射程外である。

## 5. 実行時に読む manifest の場所

- tracked な `rules/manifest.toml` を **build 時に binary へ埋め込む**（`include_str!`）。C1「機械が読む規則はこの manifest 1 file だけ」を、別 repo（toy repo の worktree）で走る `pipe gate` でも path に依存せず満たす。binary は自分を build した manifest の版と一体になる（単一 static binary・ADR-0001 と同じ向き）。
- `--rules <path>` で file から読む override を全 subcommand に持つ（test が tmp の manifest で `gate.token_cap = 1` 等を撃つため）。override は埋め込みと同じ loader・同じ拒否 5 形。
- host 固有の宣言値（口座 label・席の plugin dir・起動引数）は manifest の **host の面** `<state_dir>/host.toml` が持ち、同じ loader が読む（rules 行は置けない・[account-lifecycle.md](./account-lifecycle.md) §2・ADR-0026 §2.1）。

## 6. CLI

- `<NAME> rules validate [--rules PATH]`: rc 0 = `rules: ok rows=<N> kinds=<K>` の 1 行 / rc 1 = error 1 件 1 行（stderr）。
- `<NAME> rules get <id> [--rules PATH]`: 値を 1 行（`Policy` は本文）。無ければ `rules: no such id` + rc 1。`enabled = false` の行は `rules: disabled <id>` + rc 1（不発効の値を機械が黙って使わない）。
- 出力は `emit` / `emit_err` 経由のみ。`process::exit` 不使用。

## 7. 歯（契約 `s2-chg` の検証・`tests/e2e/rules.rs` module・`rules_` 接頭辞・fixture は文字列 literal）

歯は `crates/<NAME>/tests/e2e/rules.rs` module に `rules_` 接頭辞で置く（個々の名前はここに書かない。名前の列は現物が SSOT＝`cargo nextest list -p <NAME>`・ADR-0013 §2.1・`s2-07l.78`）。外形（usage と 1 行出力）は insta snapshot 1 本で pin する。

何を測るか: 良い fixture を受理する／未知の kind・重複 id・裁定の無い行・値の型違いを拒む／欠陥 3 箇所を行番号付きの error 3 行で全件報告する／`ALL` の各 kind が 1 行 fixture で parse + validate を通る（kind parity）／埋め込み manifest を実 loader で読み error 0 ∧ 全 kind ≥1 行／CLI の get が値を返し、不発効の行を断り、`--rules` の tmp manifest の値が埋め込みより優先される。

さらに「黙って入力を捨てる」形を塞ぐ（lens の指摘を再現してから足した）: schema の欠落（拒否 5 形の 5 番目）／行の中の重複 key（`enabled = true` の次に `false` を書くと不発効の行が有効なまま読まれていた）／schema の重複（後勝ちで版が黙って差し替わる）／空の id／PATH の無い `--rules`（埋め込みへ無言 fallback して rc 0 を返していた）。

xtask 側の drift 歯（最小形）: `crates/xtask/src/limits.rs` の `#[cfg(test)]` に、xtask 自身の `toml_lite` で manifest を読み `R-C4-1` / `R-C4-2` を const と突合する歯を置く（TOML scanner は MVP では core と xtask の 2 実装を許し、core 側が育ったら xtask がそれを使う）。

## 8. 却下案

- `limits.rs` の数値を本設計で manifest へ移す — xtask との交差と射程拡大。写しの一致を歯で守る形にして移設は後続。
- manifest を JSON にする — 手書きの行に裁定 id を書く file は TOML の方が読める・既存 scanner と同形。
- `#[non_exhaustive]` — 網羅 match を壊す（C1 / C11）。
- SQLite / 外部 crate — NFR3。
- `origin = "ai"` の行（裁定 id 無し）を許す — C5 に反する（lens 指摘で却下）。AI 提案の数値は user 裁定を先に取る。
- 実行時に cwd 相対の `rules/manifest.toml` を読む — 別 repo で走る gate が manifest を見失う（lens 指摘で却下）。
- `quote` key — PUBLIC repo に user 逐語を置けない（CON2）。

## 9. 後続

- C14.2 の drift 歯: 憲法 §3 の表と manifest の行を接頭辞で group 化して突合し CI で RED（§3 の HTML を機械で読む面が要る）。
- C1.2 の生成 doc: enum の doc コメント + manifest から人向け doc を生成し、手書き規範文 0 行を CI が数える。
- xtask `check` が閾値を manifest から読む（`limits.rs` の const を消す）。R-C4-3 の母集団は現状 `crates/*/src` だけで `tests/` を数えない（統合 test は比の外）。母集団を広げる判断は裁定材料として残す。
- property test（C12.7）は `proptest` が A3 に当たるため、依存の裁定を通す周まで後続。

## 10. xtask の check_tests.rs の分割（契約表の行 f・`s2-07l.370`）

- 何が起きているか: `crates/xtask/src/check_tests.rs`（`check.rs` の `#[path]` mod・約 1320 行）は xtask 系の便 4 本（.161 / .164 / .170 / .177）が同時に write-set に持つ hub で、R-C4-2 の余地が 176 行しか無く size M の便を受付が断る（.161 の受付拒否 2026-09-15）。.363（`pipe/closure.rs` → `pipe/closure/derive.rs`）と同型の純移動で余地を作る。
- 形: 歯を 2 つの子 module へそのまま移す（名・本文・assert・順序を変えない）: (a) check_nonrust_tests.rs = non_rust_exec_ と ci_shell_lines_ の歯 (b) check_prose_tests.rs = prose_gate_ と claude_md_ の歯と専用 fixture。宣言は check_tests.rs の末尾に `flipcheck_tests.rs` と同じ `#[path]` の形。共有 helper（make_tmp_dir / write_at / check_fixture / summary_fixture / assert_single 等）は親に残し、子は `use super::…` で読む（可視性を pub(super) に上げる以外は触らない・複製しない）。
- 札: `// flip-check: moved s2-07l.370` を 3 file の test 区間に 1 行ずつ（親は mod tests の中・子は file 先頭の module doc の直後）。既存の `.257` の札は持ち越し。純移動の機械証明は [pipeline.md](./pipeline.md) §5.3。
- 触らない: `check.rs` の本体・xtask の他 file・歯の中身。

## 11. private-clean の needle の追加（契約表の行 g・`s2-07l.174`）

- 何が起きているか: `crates/xtask/src/private_clean.rs` の needle は Email / UsersPath の 2 形だけで、PUBLIC 面へ出てはいけない v1 台帳 id 形（`sc-` + 英数 5 字）と state dir の絶対 path 形（home 直下から state dir へ至る接頭・字面は `concat!` で分けて private_clean.rs に置く）を機械が止めない（監査 2026-09-12 塊 20・NFR6）。tracked で当たるのは `SPIKE-tooling-report.md`（3 行）と `design-intent/research/SPIKE-folio-report.html`（1 行）＝needle を入れると赤になる 2 file は同じ便で掃除する（user 裁定 2026-09-15 18:2xZ）。
- 形: 閉じた enum に variant 2 つ（as_str = ledger-id-v1 / state-dir-path）。字面は `concat!` で分けて自分を撃たない。state dir は絶対形だけ（相対形 `.local/state/` は ADR-0004 が持つ＝当てない）。掃除は id を「v1 の台帳の便」の語に置き換える（文の意味は残す・research html に生成元 file は無い＝掃除の後に folio build の drift 検査が無差分であれば足りる）。要件は暫定で FR52（CI が tracked file を検査し違反を file と行で名指して非 0 で止める形）を当てる: PUBLIC 面の門を名指す要件は SRS の制約 CON2 だけで契約の req に取れないため、次版の SRS 改訂周で「PUBLIC 面の門」の FR を足して差し替える。
- 却下: 免除 list（散文・N2）／相対形も当てる（frozen の ADR-0004 が赤になる）。digest 方式と token の newtype は別便。

## 12. rules_wired の字下げ #[cfg(test)] を test 区間の印に数えない（契約表の行 j・`s2-07l.350`）

- 何が起きているか: admin の 1 行当て A/B（2026-09-15 14:3xZ・`.160` run 2）で撃墜 10 / 生存 1（母集団 11）。生存 = `crates/xtask/src/rules_wired.rs` の `line.starts_with(TEST_MOD_MARK)` → `contains` への変異で、字下げした `    #[cfg(test)]`（行頭でない）を test 区間の印に誤って数える（no-op でないことは実測済み: 元 rc 0・変異 rc 100）。
- 形: `rules_wired.rs` の in-file の歯に「字下げした `#[cfg(test)]`（行頭でない）は test 区間の印に数えない」fixture を 1 本足す（実装は変えない・module doc の「行頭の印」を pin する）。`// flip-check: retroactive s2-07l.350` の札を付ける。
- 触らない: `rules_wired.rs` の実装本体。
- 依存: `.160` Landed 後。Landed 後に admin が同じ変異（`starts_with` → `contains`）を A/B して撃墜 1/1 を notes に写す（歯にしない）。

## 13. session 用の閾値の行の値を 95 に上げる（契約表の行 k・`s2-07l.447`）

- 何が起きているか（planner 席 2026-09-17・verified）: rules 行 `R-C9-1`（kind `AccountSelection`・Int・85）の 1 値が、数える窓の全部（5 時間・7 日・モデル別 7 日）に同じく効く。7 日窓とモデル別窓は全量が大きく、85 で席を退避させ候補から外すと席が使える口座が足りない。窓別の閾値は [seat-autonomy.md](./seat-autonomy.md) の窓別の閾値の便（`s2-07l.434`）が持つが、型と読み手を変える複数便でまだ着地しない。user 裁定 2026-09-17T07:30Z（逐語は台帳 `s2-07l.434`）の要旨: 窓別の口が入るまでの特例として、値を今すぐ 95 に上げる（全窓 95 を受け入れる・5 時間窓を 85 に戻すのは窓別の便の着地）。
- 形: `rules/manifest.toml` の `R-C9-1` の行の `value` を 95 に・`ruling` を上の裁定 id に・`ruled_at` をその日付に書き換える（行の id・kind・`enabled` は不変・行は増やさない・C5）。読み手（`fleet/cli.rs` の `threshold_of`・席の側の `int_rule_of`）と選定の純関数は 1 字も変えない。値と裁定 id を pin する歯（`tests/e2e/rules.rs` の埋め込み manifest の歯）と、`rules get R-C9-1` の 1 行を含む外形 snapshot を新しい値に直す。**席の e2e は埋め込み manifest の値に依る**（実測: 席の tick は閾値を埋め込み manifest の `R-C9-1` から読み、歯が `--rules` で渡す fixture はこの行を持たない）＝閾値の両側を撃つ歯（`tests/e2e/seat/account.rs` の実測値 85〜94 を置く歯と合図の字面の「閾値 85%」・`tests/e2e/seat/cycle.rs` の予備の口座 90・`tests/e2e/seat.rs` の helper）を新しい値の両側（94 / 95 以上）へ同じ便で直す。`tests/e2e/fleet.rs` の選定の歯は fixture が行を持つので依らない見込み（便が base で実測する）。
- 触らない: 行の id と kind・他の行・読み手と選定の code・退避の合図の字面・便用の選定（閾値を読まない）・`seat.context_cap_pct`。
- 歯（`rules_embedded_manifest_declares_account_selection_threshold` を直す＝値 95 と新しい裁定 id を assert・base は 85 で RED／`rules_external_form` の snapshot）。閾値の上側を 90 で置く歯は `tests/e2e/seat/tick.rs` にも 1 本在る（合図の back-off が口座の軸にも掛かる歯）＝同じ便で直す。flip-check は変えた test file を 1 本ずつ単独で base に重ねて RED を求めるので、値替えだけでは base（85）でも挙動が変わらない file（`tests/e2e/seat/cycle.rs`・`tests/e2e/seat/tick.rs`）と `tests/e2e/seat/account.rs` には、85 と 95 を弁別する歯（実測値 90・歯の名は seat_threshold_95_ で始める）を 1 本ずつ足す: 予備の口座が 90 の周は候補になり立て直しが走る／登録 row の口座が 90 の席に口座を起点とする退避の合図が出ない（base 85 ではどちらも逆の結果で RED）。足す歯は tmux を立てるので、nextest の tmux の test-group の登録（`.config/nextest.toml`・xtask check の nextest-tmux-group が drift を落とす）も同じ便で足す。
- 却下: repo の外の rules の写しを `--rules` で席の tick に読ませる（規則の値が manifest の外に住む・C1 / C5・宣言を別の置き場の値で上書きする型）／host の面（`host.toml`）に閾値の上書きを足す（host 固有の値ではない・N3）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "読み手の無い enabled な rules 行を xtask check の検出線 rules-wired が名指す"
req = ["FR17"]
section = "4"
write-set = ["crates/xtask/src/rules_wired.rs", "crates/xtask/src/check.rs", "crates/xtask/src/main.rs", "crates/xtask/src/check_tests.rs", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail rules_wired_"]
size = "S"
done = "xtask check の判定行に rules-wired の fact（unwired の本数と id）が出て rc は変わらない"

[[contract]]
id = "b"
title = "憲法 §3 の行 id と manifest の R-* 行を双方向に突合する検出線 rules-parity・設計 doc の手書きの件数の撤去"
req = ["FR17", "FR18"]
section = "4"
write-set = ["crates/xtask/src/rules_parity.rs", "crates/xtask/src/check.rs", "crates/xtask/src/main.rs", "crates/xtask/src/claude_md.rs", "crates/xtask/src/check_tests.rs", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail rules_parity_"]
size = "S"
done = "判定行に doc-only / manifest-only の id が出て、設計 doc から手書きの variant の件数が消える"
depends = ["a"]

[[contract]]
id = "c"
title = "1 便あたりの依存の増分を R-C13-1.per-pr と突合する xtask deps-delta・依存を足した便の check-delta-ms を検出線に記録"
req = ["FR7", "FR17", "NFR3"]
section = "4"
write-set = ["crates/xtask/src/deps_delta.rs", "crates/xtask/src/main.rs", "crates/xtask/src/limits.rs", "crates/xtask/src/check_facts.rs", "crates/xtask/src/check_tests.rs", ".github/workflows/ci.yml", "CLAUDE.md", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail deps_delta_"]
size = "S"
done = "per-pr を超えて依存を足す PR が CI の入口で落ち、足した便の check-delta-ms が判定行に残る"
depends = ["a"]

[[contract]]
id = "d"
title = "CLAUDE.md の done の定義を ci.yml から生成する区間にし、生成区間の外の規範行を検出線 claude-md-prose で数える"
req = ["FR17"]
section = "3"
write-set = ["crates/xtask/src/claude_md.rs", "crates/xtask/src/check.rs", "crates/xtask/src/prose_gate.rs", "crates/xtask/src/genmanifest.rs", "crates/xtask/src/check_tests.rs", "CLAUDE.md", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail claude_md_done_ claude_md_prose_"]
size = "S"
done = "done の定義が ci.yml から生成され drift が xtask check で落ち、区間外の規範行の件数が判定行に出る"
depends = ["a"]

[[contract]]
id = "e"
title = "enum-slices を順序一致に強め、極性の宣言 site と Guard の網羅を両方向で突合する（免除は NOT_A_GUARD の closed slice）"
req = ["FR17"]
section = "3"
touches = ["crate::polarity::Guard"]
write-set = ["crates/xtask/src/enum_slices.rs", "crates/xtask/src/polarity.rs", "crates/xtask/src/check.rs", "crates/xtask/src/check_tests.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/json_tree.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "docs/design/polarity.md", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail enum_slices_order_", "cargo nextest run -p xtask --no-tests=fail polarity_sites_"]
size = "S"
done = "順序違いの slice が添字付きで落ち、Guard に無い極性 site と site の無い Guard が両方向で名指され、免除は closed slice 1 本"
depends = ["a"]

[[contract]]
id = "f"
title = "xtask の check_tests.rs（1319 行・xtask 便の hub）を子 module 2 つ（nonrust / prose）に割る — 純移動・札 moved"
req = ["FR17"]
section = "10"
write-set = ["-crates/xtask/src/check_tests.rs", "+crates/xtask/src/check_nonrust_tests.rs", "+crates/xtask/src/check_prose_tests.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail check::tests::nonrust:: check::tests::prose::"]
size = "S"
done = "check_tests.rs の余地が 600 行以上に戻り、歯が 2 つの子 module に移って本数と中身が不変"

[[contract]]
id = "g"
title = "private-clean の needle に v1 台帳 id 形と state dir の絶対 path 形を足し、該当する tracked 2 file を掃除する — needle は閉じた enum の variant 1 つずつ"
req = ["FR52"]
section = "11"
write-set = ["crates/xtask/src/private_clean.rs", "SPIKE-tooling-report.md", "design-intent/research/SPIKE-folio-report.html", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail private_clean_ledger_ private_clean_state_dir_ private_clean_relative_state_dir_"]
size = "S"
done = "2 形の needle が在り、tracked に該当 0 で cargo xtask check が緑（ADR-0004 の相対形の言及は当てない）"

[[contract]]
id = "h"
title = "gate.token_cap を 150000 → 400000 に一時的に上げる — 純移動でない 169 KB の diff（.209）を lens に通す・裁定 id 付き・戻しは行 i"
req = ["FR9"]
section = "4"
write-set = ["rules/manifest.toml", "crates/scribe2/tests/e2e/rules.rs", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail rules_"]
size = "S"
done = "manifest の行の値と裁定 id が新しく、埋め込み値の pin が 400000 で緑、§4.1 の表が同じ値と裁定を写し、src は不変"

[[contract]]
id = "i"
title = "gate.token_cap を 400000 → 150000 に戻す — .209 Landed 後・行 h の対・裁定 id は行 h と同じ承認"
req = ["FR9"]
section = "4"
write-set = ["rules/manifest.toml", "crates/scribe2/tests/e2e/rules.rs", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail rules_"]
size = "S"
done = "manifest の行の値が 150000 に戻り裁定 id が戻しの字面で、埋め込み値の pin が 150000 で緑、§4.1 の表が同じ値と裁定を写し、src は不変"

[[contract]]
id = "j"
title = "rules_wired の in-file の歯に「字下げした #[cfg(test)] は test 区間の印に数えない」fixture を足す — 実装は不変・retroactive 札"
req = ["FR17"]
section = "12"
write-set = ["crates/xtask/src/rules_wired.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail rules_wired_indented_"]
size = "S"
done = "変異 starts_with → contains が赤になる歯が在る"

[[contract]]
id = "k"
title = "session 用の閾値の行 R-C9-1 の値を 95 に上げる — 値と裁定 id と ruled_at だけを書き換え、値を pin する歯と rules の外形 snapshot を直す（特例・裁定 user 2026-09-17T07:30Z・窓別は s2-07l.434）"
req = ["FR36", "FR38"]
section = "13"
write-set = ["rules/manifest.toml", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/seat/account.rs", "crates/scribe2/tests/e2e/seat/cycle.rs", "crates/scribe2/tests/e2e/seat/tick.rs", "crates/scribe2/tests/e2e/fleet.rs", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_embedded_manifest_declares_account_selection_threshold", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_account_relaunch_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_account_tick_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_threshold_95_"]
size = "M"
done = "埋め込み manifest の R-C9-1 が値 95 と裁定 id user 2026-09-17T07:30Z を持ち、rules get R-C9-1 の外形が 95 を出し、読み手と選定の code は不変"
<!-- contracts:end -->
