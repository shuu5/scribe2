# 設計: rules manifest — 規則の種類は閉じた enum・値は tracked な TOML 1 file

- 要件: [FR17](../../design-intent/spec/srs.html#FR17) rules manifest / [FR18](../../design-intent/spec/srs.html#FR18) rules の検査 / [AC6](../../design-intent/spec/srs.html#AC6) / [NFR3](../../design-intent/spec/srs.html#NFR3) 依存 0 本 / [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) 規則はデータ / [C5](../../design-intent/spec/constitution.html#c5) 規則変更 = manifest diff + 裁定 id / [C14](../../design-intent/spec/constitution.html#c14) 規律の 2 面 / [§3](../../design-intent/spec/constitution.html#s3-rules-rows) rules 行の初期値
- 決定: [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.3（TOML subset・std だけの scanner）
- この設計から出る契約: `s2-chg`（rules manifest）。後続の契約は末尾「後続」に列挙する。

## 1. 何を解くか

規則の**種類**を Rust の閉じた型で持ち、規則の**値**（閾値・有効 / 無効・裁定 id・裁定時刻）を repo 内の TOML 1 file に置く。機械が実行時に読む規則はこの file だけである（C1）。憲法 §3 の表は「manifest が存在しない間の唯一の存在箇所」と自らを定義しているので、本設計で manifest が生まれた時点から **§3 の各行は manifest の写し**になる。写しと正本の drift は C14.2 が CI の歯で止めると定めているが、その歯は本設計の射程外（後続）である。

やさしく言うと: 「何を守るか」の一覧は Rust の enum に、「いくつまでか」の数字は TOML に書く。数字を変えるときは TOML の diff に user の裁定 id を付ける。

## 2. 型（crate `<NAME>` の `rules` module）

- `pub enum RuleKind` — **閉じた列挙・`#[non_exhaustive]` 禁止**。variant を足したら網羅 `match` が compile error になる形を保つ（C1 / C11）。variant は §3 の行と MVP の運用値に 1:1 で対応する（§3 の表）。
- `pub const ALL: &[RuleKind]` — parity test の母集団。variant 数と要素数が一致しなければ test が落ちる。
- `pub enum RuleValue { Int(u64), Str(String), Mode(Mode), Policy(String) }`、`pub enum Mode { Off, On, Shadow }`。
  - `Int` = 閾値（単位は行 id と kind の doc コメントが持つ）。`Str` = 識別子（対話面の identity 等）。`Mode` = seam の on / off / shadow。`Policy` = 散文で書かれた選定規則や検出線の定義（機械は `enabled` だけを読む。値の意味は憲法の写し）。
- `pub trait Rule { fn kind(&self) -> RuleKind; fn validate(&self) -> Result<(), RuleError>; }` を `RuleRow` に実装する。kind ごとの値型の対応は **`match kind { … }` 1 箇所・wildcard `_` 無し**で書く。
- `pub struct RuleRow { id, kind, value, enabled, origin: Origin, ruling: Option<String>, ruled_at: Option<String>, line: u64 }`、`pub enum Origin { User, Ai }`。
- `RuleError` は `Display` で **1 件 1 行・`line=<N>` を含む**（行番号付きで全件を返す＝FR18）。

## 3. manifest（`rules/manifest.toml`・tracked・repo root 直下）

TOML subset（`[[rule]]` の array-of-tables・値は string / integer / bool のみ・未知 key は拒否）。loader は std だけで書く（ADR-0004 §2.3・NFR3）。

```toml
schema = 1

[[rule]]
id = "R-C4-1"
kind = "CoreLines"
value = 20000
origin = "user"
ruling = "RULING-v2-p1-exit-bundle 論点 2"
ruled_at = "2026-09-07"
```

key の意味:

| key | 型 | 必須 | 意味 |
|---|---|---|---|
| `id` | string | 必須・一意 | 行 id。§3 の行は `R-<条>-<番号>`、compound 行は `R-<条>-<番号>.<key>`、MVP の運用値は `<領域>.<名>` |
| `kind` | string | 必須 | `RuleKind` の variant 名（字面一致） |
| `value` | integer / string | 必須 | kind が定める型。`Mode` / `Policy` は string |
| `enabled` | bool | 省略時 true | false = 値は写すが機械は効かせない（未定・停止・v3 送り） |
| `origin` | string | 必須 | `"user"` / `"ai"` |
| `ruling` | string | origin=user なら必須 | 裁定 id。憲法 §3 と同じく裁定文書の論点番号、または裁定の UTC ts |
| `ruled_at` | string | origin=user なら必須 | 裁定の日付（UTC・分秒が資料に無ければ日まで） |

**書かない key**: user 逐語（本 repo は PUBLIC・CON2）。逐語は repo 外の裁定文書が持つ。

### 3.1 初期行（§3 の写し + MVP の運用値）

| id | kind | value | enabled | origin | 出所 |
|---|---|---|---|---|---|
| `R-C4-1` | CoreLines | 20000 | true | user | §3 論点 2 |
| `R-C4-2` | ModuleLines | 1500 | true | user | §3 論点 2 |
| `R-C4-3` | TestSrcRatioPct | 100 | true | user | §3 論点 2（比 1.0 = 100%） |
| `R-C4-4.fn-lines` | FnLines | 60 | true | user | §3 論点 2 |
| `R-C4-4.complexity` | FnComplexity | 15 | true | user | §3 論点 2 |
| `R-C4-4.args` | FnArgs | 5 | true | user | §3 論点 2 |
| `R-C6-1` | RunTokenCeiling | 0 | **false** | ai | §3「未定」＝値が決まるまで不発効 |
| `R-C7-1` | DialogueSurface | `"user-direct"` | true | user | ADR-0003 |
| `R-C8-1` | MaturityCondition | Policy（§3 の文） | **false** | user | ADR-0003 で停止・履歴 |
| `R-C9-1` | AccountSelection | Policy（§3 の文） | **false** | user | 口座は v3（SRS scope out） |
| `R-C12-1` | MutationSurvivalLine | Policy（§3 の文） | **false** | user | 検出線は未配線 |
| `R-C13-1` | DepBudget | 12 | true | user | §3 論点 3・4 |
| `R-C13-1.per-pr` | DepPerPr | 1 | true | user | 同上 |
| `R-C13-1.check-delta-ms` | CheckDeltaMs | 300 | true | user | 同上 |
| `R-C13-2` | CompileShape | Policy（§3 の文） | true | user | deny の形は xtask / CI が持つ |
| `R-C13-3` | CompileSeconds | Policy（§3 の文） | **false** | user | 検出線は未配線 |
| `gate.lens_count` | GateLensCount | 1 | true | user | grill U3（tier1 = 1 lens） |
| `gate.token_cap` | GateTokenCap | 150000 | true | user | grill U3（目標値・NFR1） |
| `hook.budget_ms` | HookBudgetMs | 2000 | true | ai | NFR5 |
| `hook.budget_bytes` | HookBudgetBytes | 16384 | true | ai | 注入 carrier の上限 |
| `tooling.rtk.mode` | ToolingRtkMode | `"off"` | true | ai | spike-2 の結論 |
| `tooling.graphify.mode` | ToolingGraphifyMode | `"off"` | true | user | 器 SPEC v0.4.3（seam 残置） |

`RuleKind` の variant はこの表の kind 列と 1:1（22 variant）。§3 の行 id と manifest の行 id は**字面一致**させる（後続の drift 歯がそのまま突合できる形）。

### 3.2 拒否 5 形（FR18・AC6・すべて `line=<N>` 付き・全件を集めて返す）

1. 未知 `kind`
2. `id` 重複
3. `origin = "user"` の行に `ruling` か `ruled_at` が無い
4. `value` の型が kind の対応と不一致（`Int` に string・`Mode` に off / on / shadow 以外 等）
5. 必須 key（`id` / `kind` / `value` / `origin`）の欠落、未知 key、`schema` 不在または 1 以外

**最初の 1 件で止めない**（silent drop 禁止・NFR4）。

## 4. CLI

- `<NAME> rules validate [PATH]`（既定 `rules/manifest.toml`）: rc 0 = `rules: ok rows=<N> kinds=<K>` の 1 行 / rc 1 = error 1 件 1 行。
- `<NAME> rules get <id> [PATH]`: 値を 1 行（`Policy` は本文）。無ければ `rules: no such id` + rc 1。`enabled = false` の行は `rules: disabled <id>` + rc 1（不発効の値を機械が黙って使わない）。
- 出力は既存の出力層 `emit` / `emit_err` 経由のみ。`process::exit` 不使用。

## 5. 歯（契約 `s2-chg` の検証）

- 統合 test は `crates/<NAME>/tests/e2e/main.rs` + `tests/e2e/rules.rs`（module）に置く（統合 test file は crate あたり 3 以下・憲法 R-C13-2）。test 名は `rules_` 接頭辞。fixture は test 内の文字列 literal。
- `rules_manifest_accepts_good_fixture` / `rules_manifest_rejects_unknown_kind` / `rules_manifest_rejects_duplicate_id` / `rules_manifest_rejects_user_row_without_ruling` / `rules_manifest_rejects_value_type_mismatch` / `rules_manifest_reports_all_errors_with_line_numbers`（欠陥 3 箇所 → error 3 行）/ `rules_kind_parity_every_kind_has_sample`（`ALL` の各 kind が 1 行 fixture で parse + validate を通る）/ `rules_tracked_manifest_is_valid_and_covers_all_kinds`（repo の manifest を実 loader で読み error 0 ∧ 全 kind ≥1 行 ∧ `origin = "user"` の行 ≥10）/ `rules_cli_get_returns_value` / `rules_cli_get_refuses_disabled_row`。
- xtask 側の drift 歯（最小形）: `crates/xtask/src/limits.rs` の `#[cfg(test)]` に `limits_match_rules_manifest`（`R-C4-1` / `R-C4-2` の値が `MAX_CORE_LINES` / `MAX_FILE_LINES` と一致。xtask 自身の `toml_lite` で読む＝crate 間 path 依存を作らない）。これが C14.2 の drift 歯の最初の 1 本。

## 6. 却下案

- `limits.rs` の数値を本設計で manifest へ移す — xtask との交差と射程拡大。写しの一致を歯で守る形にして移設は後続。
- manifest を JSON にする — 手書きの行に裁定 id を書く file は TOML の方が読める・既存 scanner（xtask `toml_lite`）と同形。
- `#[non_exhaustive]` — 網羅 match を壊す（C1 / C11）。
- SQLite / 外部 crate — NFR3（直接依存 0 本）。
- 逐語 `quote` key — PUBLIC repo に user 逐語を置けない（CON2）。

## 7. 後続（本設計から切り出す別契約）

- C14.2 の drift 歯: 憲法 §3 の表と manifest の行 id / 値の突合を CI で RED にする（§3 の HTML を機械で読む面が要る）。
- C1.2 の生成 doc: enum の doc コメント + manifest から人向け doc を生成し、手書き規範文 0 行を CI が数える。
- xtask `check` が閾値を manifest から読む（`limits.rs` の const を消す）。
