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
- `pub enum RuleValue { Int(u64), Str(String), Policy(String) }`。`Int` = 閾値（単位は kind の doc コメント）。`Str` = 識別子（対話面の identity）。`Policy` = 散文で書かれた選定規則や検出線の定義（機械は `enabled` だけを読む。本文は憲法 §3 の写し）。
- `pub trait Rule { fn kind(&self) -> RuleKind; fn validate(&self) -> Result<(), RuleError>; }` を `RuleRow` に実装。kind ごとの値型の対応は **`match kind { … }` 1 箇所・wildcard `_` 無し**。
- `pub struct RuleRow { id, kind, value, enabled, ruling: String, ruled_at: String, line: u64 }`。
- `RuleError` は `Display` で **1 件 1 行・`line=<N>` を含む**。

## 4. manifest（`rules/manifest.toml`・tracked・repo root 直下）

TOML subset（先頭に `schema = 1`・`[[rule]]` の array-of-tables・値は string / integer / bool のみ・未知 key は拒否）。loader は std だけで書く（ADR-0004 §2.3・NFR3）。

```toml
schema = 1

[[rule]]
id = "R-C4-1"
kind = "CoreLines"
value = 20000
ruling = "RULING-v2-p1-exit-bundle 論点 2"
ruled_at = "2026-09-07"
```

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `id` | string | 必須・一意 | 行 id。§3 の行は `R-<条>-<番号>`、compound 行は `R-<条>-<番号>.<key>`（§3 の行 id が**接頭辞**として一致）、MVP の運用値は `<領域>.<名>` |
| `kind` | string | 必須 | `RuleKind` の variant 名（字面一致） |
| `value` | integer / string | 必須 | kind が定める型。`Policy` は string |
| `enabled` | bool | 省略時 true | false = 値は写すが機械は効かせない（停止・v3 送り） |
| `ruling` | string | **必須** | 裁定 id。憲法 §3 と同じく裁定文書の論点番号、または裁定の UTC ts |
| `ruled_at` | string | **必須** | 裁定の日付（UTC・分秒が資料に無ければ日まで） |

**書かない key**: user 逐語（PUBLIC・CON2）。**置かない行**: 値が未定の行（§3 R-C6-1）・実装が無い seam の行（rtk / graphify のモード・C10.3「未 wire の設定」）・AI が置いただけで裁定の無い行。

### 4.1 初期行（§3 の写し + MVP の運用値・全行に裁定 id）

| id | kind | value | enabled | 裁定（ruling / ruled_at）・design-intent 側の出所 |
|---|---|---|---|---|
| `R-C4-1` | CoreLines | 20000 | true | 論点 2 / 2026-09-07・憲法 §3 |
| `R-C4-2` | ModuleLines | 1500 | true | 同上 |
| `R-C4-3` | TestSrcRatioPct | 100 | true | 同上（比 1.0 = 100%） |
| `R-C4-4.fn-lines` | FnLines | 60 | true | 同上 |
| `R-C4-4.complexity` | FnComplexity | 15 | true | 同上 |
| `R-C4-4.args` | FnArgs | 5 | true | 同上 |
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
| `gate.token_cap` | GateTokenCap | 150000 | true | grill U3 / 2026-09-07・SRS NFR1（目標値） |
| `hook.budget_ms` | HookBudgetMs | 2000 | true | SRS NFR5 の user 承認 / 2026-09-09・要件カタログ R-K22 |
| `pipe.stop_grace_ms` | StopGraceMs | 2000 | true | user 2026-09-09T09:08Z / 2026-09-09 |
| `fleet.lock_retry_ms` | LockRetryMs | 5000 | true | user 2026-09-09T09:08Z / 2026-09-09 |
| `fleet.lock_stale_ms` | LockStaleMs | 30000 | true | user 2026-09-09T09:08Z / 2026-09-09 |
| `hook.timeout_s` | HookTimeoutS | 10 | true | user 2026-09-09T09:08Z / 2026-09-09（hooks.json の timeout・xtask がここから写す） |

`RuleKind` の variant はこの表の kind 列と 1:1（22 variant）。§3 の行 id は manifest の行 id の**接頭辞**として一致する（compound 行は `.` で枝分かれ）。後続の drift 歯は接頭辞で group 化して突合する。

### 4.2 拒否 5 形（FR18・AC6・すべて `line=<N>` 付き・全件を集めて返す）

1. 未知 `kind`
2. `id` 重複
3. `ruling` か `ruled_at` の無い行（C5・全行）
4. `value` の型が kind の対応と不一致
5. 必須 key（`id` / `kind` / `value` / `ruling` / `ruled_at`）の欠落、未知 key、`schema` 不在または 1 以外

**最初の 1 件で止めない**（silent drop 禁止・NFR4）。

憲法 §3 rules 表の閾値セルは manifest の**写し**であり（値の正本は manifest 側）、両者の一致は xtask の歯 `constitution_thresholds_match_rules_manifest` が守る（R-C4-1 / R-C4-2 / R-C4-3 と R-C4-4 の 3 値＝計 6 個を順序込みで突合・**閾値の数値を変える手編集は RED**・行の重複や死骸で隠す形も RED・改訂形の `<del>` は落として `<ins>` 側を読む＝整合した改訂は緑で `<ins>` だけ変えた周は RED）。**測っていない面**は 3 つある: 条の向き（`以下` → `以上`）・桁区切りの位置（`20,000` → `2,0000`＝`,` を落として読むため値は同じ）・§3 に**新しい数値行を足した**周（歯が見るのは `r-c4-1`〜`r-c4-4` の 4 行に固定）。数値を持たない prose 行（R-C6-1 以降・R-C13-*）も突合できず射程外である。

## 5. 実行時に読む manifest の場所

- tracked な `rules/manifest.toml` を **build 時に binary へ埋め込む**（`include_str!`）。C1「機械が読む規則はこの manifest 1 file だけ」を、別 repo（toy repo の worktree）で走る `pipe gate` でも path に依存せず満たす。binary は自分を build した manifest の版と一体になる（単一 static binary・ADR-0001 と同じ向き）。
- `--rules <path>` で file から読む override を全 subcommand に持つ（test が tmp の manifest で `gate.token_cap = 1` 等を撃つため）。override は埋め込みと同じ loader・同じ拒否 5 形。

## 6. CLI

- `<NAME> rules validate [--rules PATH]`: rc 0 = `rules: ok rows=<N> kinds=<K>` の 1 行 / rc 1 = error 1 件 1 行（stderr）。
- `<NAME> rules get <id> [--rules PATH]`: 値を 1 行（`Policy` は本文）。無ければ `rules: no such id` + rc 1。`enabled = false` の行は `rules: disabled <id>` + rc 1（不発効の値を機械が黙って使わない）。
- 出力は `emit` / `emit_err` 経由のみ。`process::exit` 不使用。

## 7. 歯（契約 `s2-chg` の検証・`tests/e2e/rules.rs` module・`rules_` 接頭辞・fixture は文字列 literal）

`rules_manifest_accepts_good_fixture` / `rules_manifest_rejects_unknown_kind` / `rules_manifest_rejects_duplicate_id` / `rules_manifest_rejects_row_without_ruling` / `rules_manifest_rejects_value_type_mismatch` / `rules_manifest_reports_all_errors_with_line_numbers`（欠陥 3 箇所 → error 3 行）/ `rules_kind_parity_every_kind_has_sample`（`ALL` の各 kind が 1 行 fixture で parse + validate を通る）/ `rules_embedded_manifest_is_valid_and_covers_all_kinds`（埋め込み manifest を実 loader で読み error 0 ∧ 全 kind ≥1 行）/ `rules_cli_get_returns_value` / `rules_cli_get_refuses_disabled_row` / `rules_cli_rules_flag_overrides_embedded`（tmp manifest の値が返る）/ `rules_external_form`（usage と 1 行出力の insta snapshot）。

さらに「黙って入力を捨てる」形を塞ぐ 5 本を足す（**計 17 本**・lens の指摘を再現してから追加した）: `rules_manifest_rejects_missing_schema`（拒否 5 形の 5 番目）/ `rules_manifest_rejects_duplicate_key_in_row`（`enabled = true` の次に `false` を書くと不発効の行が有効なまま読まれていた）/ `rules_manifest_rejects_duplicate_schema`（後勝ちで版が黙って差し替わる）/ `rules_manifest_rejects_empty_id` / `rules_cli_refuses_rules_flag_without_path`（PATH の無い `--rules` が埋め込みへ無言 fallback して rc 0 を返していた）。

xtask 側の drift 歯（最小形）: `crates/xtask/src/limits.rs` の `#[cfg(test)]` に `limits_match_rules_manifest`（xtask 自身の `toml_lite` で manifest を読み `R-C4-1` / `R-C4-2` を const と突合。TOML scanner は MVP では core と xtask の 2 実装を許し、core 側が育ったら xtask がそれを使う）。

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
