# scribe2 — project instructions

scribe2 は scribe v1 を捨てて作り直す「次の器」（Rust・単一 binary・codename `scribe2`）。
**開発は素の Claude Code session 1 つ + beads（bd）+ repo tracked の design-intent（folio）だけ**で行う。
前の器（scribe v1 plugin）の役割席・gate 席・QA 席・契約押印・tier ラベル・spawn worktree・外部 orchestration は使わない。
作法の SSOT は本 repo の中にしか無い（外部注入に依存しない）。

## SSOT（本 file は本文を持たない。迷ったら下を読む）
- **憲法** = `design-intent/spec/constitution.html` — 折れない線。改訂は user 裁定のみ（Ask-first A2 / Never N4）。
- **要件（SRS）** = `design-intent/spec/srs.html` — MVP のゴール。無ければ最初に書く。契約（bead）は要件 id を指す。
- **決定** = `design-intent/decisions/ADR-*.html` — 既存 ADR は frozen（改訂は supersede する新 ADR）。
- **語彙** = `design-intent/vocabulary.yaml` — 用語は必ずここに合わせる。
- **設計** = `docs/design/<題>.md` — 設計 1 本から契約（bead）が複数出る。各 bead の acceptance が pointer を持つ。
- **タスク・契約** = beads（prefix `s2-`）。bd 運用の SSOT は `.beads/PRIME.md`。
- `design-intent/spec/` の編集は **`/folio-architect` 経由でしか通らない**（folio の PreToolUse guard・仕様）。`decisions/` `research/` は対象外。

## 作業の流れ（設計 → 契約 → 実装 → 検証 → land）
1. **設計**: 要望を SRS の要件 id に結び、`docs/design/<題>.md` に書く。下の「ADR を書く条件」に当たれば ADR を先に land。
2. **契約 = bead 1 本**: acceptance に「何を作るか / write-set / 設計 doc の pointer / done」を書く。
   **検証は「base で RED になる test」で表す**（文字述語〔grep pin〕の検証行は書かない）。
3. **実装**: `bd --readonly ready --limit 0` から 1 本選び `bd update <id> --claim`。先に落ちる test を書き **RED を実測**
   （RED の理由も弁別する: 機能不在 / 道具不在 / 環境）→ 実装 → `cargo xtask flip-check --base origin/main` で入口確認。
4. **検証**: 自分の diff を lens 1 本の敵対 review に 1 周（findings は**自分で再現してから**直す）→ 下の「done の定義」を全部 GREEN。
5. **land**: PR → CI 緑 → squash merge → `bd close <id> --reason "…"`。**1 bead = 1 PR**。close は merge の後。

## done の定義（1 つでも赤なら close しない）
```
cargo nextest run --workspace --no-tests=fail
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask check
cargo deny check bans licenses sources
```
- `design-intent/` を触った便は追加で `folio validate` が clean・`folio build --check` が drift 無し。
- `git status --porcelain` が空 ∧ PR が merge 済み ∧ main の CI が緑。
- **working tree の緑は緑ではない**（未 commit の緑を成果と数えない）。

## review の形
- diff が 1 file・機械的修正だけなら single-pass で可。それ以外は lens 1 本（契約適合 / 歯の非空虚性 / 憲法条との整合）を 1 周。
- lens の findings は**そのまま採らない**。各 finding を自分で再現してから直す。
- 「0 件」は変化なしではなく**測れていない**かもしれない。count は必ず母集団の件数を同時に出す。
- 並列 agent は見積ってから回す。1 呼出しの出力は file へ落として path を返す（無界出力は memory 予算を焼く）。

## ADR を書く条件（どれか 1 つでも該当したら実装前に書く）
1. 憲法条の解釈が要る／条に触れる  2. 外部依存の追加・削除（Ask-first A3）
3. schema・on-disk 形式・跨版契約を決める  4. 却下案を残す価値がある分岐
それ以外は bead の notes に 3 行で足りる。**ADR を書いたら同じ PR で vocabulary と decisions/README も更新する。**

## 台帳の書き方
- write は `scripts/bdw` 経由（flock 直列化）。notes は `--append-notes`（`--notes` は置換＝過去の記帳を消す）。
- `bd dolt push` は session の終端で 1 回。
- bead が持つのは **task と裁定だけ**（憲法 C15）。規律は憲法・rules へ、知見は ADR / research へ。
- 新規 bead は epic（`--parent`）に属させる。**親の label は継承されるので起票直後に labels を実測する。**
- `bd remember` / `bd recall` / `bd memories` は使わない。

## user に聞くこと（それ以外は AI が決めて進む）
- **消す / 出す / 使う** の 3 クラス（憲法 A1）。**本 repo は PUBLIC** ＝ public 面の情報を増やす便は必ず聞く。
- 憲法条文の改訂・C4 / C13 の閾値変更（A2）／依存 OSS の増減（A3）／自己開発の解禁（A4）。
- 複数の妥当な設計が併存し、選択が目的・価値観に依存するとき（1 論点 1 質問・推奨 1 つ。順序や是認だけを求める問いは出さない）。

## やらないこと
- 役割席 / gate 席 / QA 席 / 契約押印 / snapshot 印 / tier ラベルの再導入。前の器の docs を読んで作法を持ち込まない。
- bash / bats の歯を書く（憲法 C12: 歯は Rust 1 framework）。
- 手書きの規範文を doc に増やす（憲法 C1 / N2）。
- 絶対 path・host 名・口座名・user 逐語を tracked file に書く（PUBLIC repo）。起動コマンドは repo に入れない。

## 憲法の規範文（生成区間）
<!-- 本区間は `cargo xtask gen-claude-md` の生成物である。手で編集しない（`cargo xtask check` の claude-md-constitution が drift を落とす）。正本は `design-intent/spec/constitution.html` の機械層。 -->
<!-- constitution:begin -->
順位: WHEN two lines of this constitution conflict in a decision, scribe2 SHALL resolve the conflict in this order: prohibition of ad-hoc fixes first, prevention of growth second, compile speed third.

C1: scribe2 SHALL represent every rule kind as a variant of one closed type with a mandatory exhaustive check implementation, SHALL keep every rule value (threshold, enabled flag, ruling id, ruling time, verbatim text) in exactly one manifest file inside the repository, SHALL keep only runtime state in the database, and SHALL read rules at run time only from that manifest.

C1.2: scribe2 SHALL derive human-readable rule documentation from the rule type and the manifest, and SHALL contain zero hand-written normative lines in that generated rule documentation.

C2: scribe2 SHALL implement selection, exclusion and routing each as exactly one function over exactly one enumeration, SHALL take the application order solely from the declaration order of the stage enumeration, SHALL NOT carry a prose note on ordering, and SHALL express a new reason only as one new variant.

C2.2: scribe2 SHALL read configuration only from the manifest structure, SHALL NOT read environment variables directly, SHALL NOT introduce a new environment seam, and SHALL derive the plugin name, crate name, skill namespace, state directory, environment prefix and terminal-multiplexer server name from one NAME constant.

C3: scribe2 SHALL keep the state of hosts, accounts, seats, leases and retirements in exactly one database file carrying a host column, and SHALL NOT place truth in per-host directories, TSV files or caches.

C3.2: scribe2 SHALL provide one doctor command that reconciles version, accounts and retirements across all hosts.

C3.3: scribe2 SHALL carry seat state as a typed enumeration, and SHALL NOT use terminal rendering or free text as judgment input.

C3.4: scribe2 SHALL express completion conditions only as values of one completion enumeration, and SHALL provide exactly one wait implementation that accepts only those values, and SHALL NOT expose a wait entry point that takes an arbitrary predicate.

C4: scribe2 SHALL enforce upper bounds on core size (R-C4-1), module size (R-C4-2), the test-to-source line ratio (R-C4-3) and function granularity (R-C4-4) as CI-measured deny gates, SHALL NOT silence a granularity lint with an allow attribute, and SHALL change any such bound only through A2.

C5: IF a change adds or modifies a rules row without an accompanying user ruling id, THEN scribe2 CI SHALL reject the change.

C6: scribe2 SHALL make a Budget constructible only by consuming a Precheck measurement, and SHALL expose exactly one spawn entry point, which requires a Budget.

C6.2: IF a run exceeds the per-run token ceiling (R-C6-1), THEN scribe2 SHALL stop the run without manual intervention.

C6.3: scribe2 SHALL record consumption in exactly one append-only store.

C7: scribe2 SHALL accept a user approval only through exactly one dialogue surface, and SHALL take the identity of that surface from the rules row R-C7-1 rather than from this article.

C7.2: scribe2 SHALL record every accepted approval as an approval event carrying the user's verbatim words.

C8: scribe2 SHALL be developed under its design-intent (constitution, ADRs, requirements) as the binding constraint, and SHALL NOT require a previous version to act as oracle or to hold stop authority.

C8.2: WHEN an edit conflicts with design-intent, scribe2 SHALL stop the edit at edit time rather than defer the judgement to a later gate of a previous version.

C8.3: scribe2 SHALL record every change to design-intent as an ADR or a versioned document.

C9: WHEN a seat stops, scribe2 SHALL detect the stop without manual intervention, SHALL resume the seat's work, and SHALL preserve its results.

C9.2: scribe2 SHALL consume each account until the end of its allowance window, following the selection rules of R-C9-1.

C10: scribe2 SHALL distinguish declared values, measured values with provenance, derived values and effective values by type, and SHALL promote a declared value to effective only through a measurement.

C10.2: scribe2 SHALL keep host-specific values only in the manifest, SHALL produce reports and listings only as generated artifacts, and SHALL NOT accept a hand-written verified status.

C10.3: scribe2 CI SHALL reject unwired configuration.

C11: scribe2 SHALL deny by workspace lint the use of unwrap, expect, panic, todo, unimplemented, unreachable, process exit, slice indexing, debug macros and direct stdout / stderr printing, SHALL deny unused must-use values, SHALL forbid unsafe code, and SHALL allow a lint exception only through an expect attribute carrying a written reason, apart from the test-scope exceptions declared in the lint configuration.

C11.2: scribe2 SHALL model failures as one enumeration per boundary carrying a FailOpen / FailClosed polarity type, and SHALL generate the polarity list of all guards at build time.

C11.3: scribe2 SHALL model a timeout as a Result that forces the caller to branch, and SHALL NOT use an untyped error type in core.

C12: scribe2 SHALL use exactly one test framework (Rust native tests), and SHALL NOT keep shell-based tests.

C12.2: scribe2 CI SHALL verify by a flip check that every change starts from a failing test: the test diff of the PR applied alone to base fails at least one new or changed test, and HEAD passes.

C12.3: IF a PR carries no test diff and is not classified docs-only, THEN scribe2 CI SHALL reject the PR.

C12.4: scribe2 SHALL record mutation survival as a detection line (R-C12-1), SHALL NOT hand-write mutation controls, and SHALL promote that detection line to a deny gate only through a user ruling carrying a ruling id (C5).

C12.5: scribe2 SHALL snapshot external form (CLI output, generated documentation, polarity list), and SHALL reject unreferenced snapshots.

C12.6: scribe2 SHALL NOT keep a known-red ledger, and SHALL keep the main branch green at all times.

C12.7: scribe2 SHALL mass-produce test cases by property testing.

C13: scribe2 SHALL manage dependencies by an allow-list with a budget (R-C13-1), and SHALL add dependencies only within the per-PR budget of R-C13-1, with user approval (A3) and a recorded incremental compile-time delta.

C13.2: scribe2 SHALL confine derive macros to boundary crates, SHALL NOT enable full feature sets, and SHALL audit dependencies (duplicate versions, vulnerabilities, licenses) in CI.

C13.3: scribe2 SHALL NOT use async in core.

C13.4: scribe2 CI SHALL deny only the deterministic shape constraints of compile time (R-C13-2).

C13.5: scribe2 SHALL treat compile-time seconds as detection lines recorded at the interval defined in R-C13-3, SHALL file an issue automatically on regression beyond that line, and SHALL NOT block a PR on those lines.

C14: scribe2 SHALL keep discipline in exactly two faces, documents (design-intent: constitution, ADRs and SRS, the face humans read and rule on) and data (the rules manifest, the face the machine enforces), SHALL treat documents as the source of truth for principles and rulings, and SHALL treat the manifest as the source of truth for enforced rules.

C14.2: scribe2 SHALL attach a ruling id to the source-of-truth side of its kind (the document version for principles and rulings, the manifest row for enforced rules), and scribe2 CI SHALL reject drift between the two faces (a rule written in a document but absent from the manifest, or a manifest row not referenced from any document).

C15: scribe2 SHALL use the ledger only for tasks and rulings, and SHALL NOT place discipline in the ledger.

C15.2: scribe2 SHALL connect the ledger to discipline only through id references: a contract SHALL reference SRS requirement ids, a pull request or ledger item that changes a rule SHALL reference a ruling id (C5), and scribe2 SHALL reject a missing reference by lint.

C16: scribe2 SHALL stop a deviation from the specification at edit time, and SHALL NOT substitute post-hoc verification for that stop.

C16.2: scribe2 SHALL list every in-loop guard in the build-time polarity list (C11.2) as in-loop, and scribe2 CI SHALL reject a configuration whose in-loop guard count is zero or whose guards are all post-hoc.

A1: WHEN an operation of one of the three classes (delete, send out, consume) is about to be performed, scribe2 SHALL obtain the user's approval through the dialogue surface designated by R-C7-1 (C7) before performing it.

A2: WHEN an amendment to a constitution article or a change to a threshold governed by C4 or C13 is proposed, scribe2 SHALL first perform a grill verification of the claimed benefit, and SHALL obtain a user ruling (carrying a ruling id) before applying the change.

A3: WHEN an OSS dependency is to be added or removed, scribe2 SHALL obtain the user's approval through the dialogue surface designated by R-C7-1 (C7) first, and SHALL follow the C13 procedure (C13, R-C13-1).

A4: scribe2 SHALL permit self-development by default, without a maturity declaration and without a user go.

A4.2: WHEN an operation is irreversible (destroy, publish or incur paid usage), scribe2 SHALL ask the user first through the dialogue surface of C7, and SHALL accept the answer only as a recorded approval event.

A4.3: scribe2 SHALL treat merge, dispatch onto its own repository, and dependency-free code changes as reversible for the purpose of A4.2.

N1: IF an operation would irrecoverably delete any object managed by scribe2, THEN scribe2 SHALL reject the operation.

N1.2: scribe2 SHALL perform any retirement only as a reversible move.

N2: IF a change would introduce a rule that exists only in prose, memory or chat, THEN scribe2 SHALL NOT treat it as a rule, and scribe2 CI SHALL reject the change (C1.2: hand-written normative lines are zero).

N3: IF code would branch on a host-specific value, THEN scribe2 SHALL reject the change.

N4: IF a change inside the current version would amend the constitution without a new ADR, inline delta markers and a user ruling id, or would break schema compatibility or exceed a C4 bound, THEN scribe2 SHALL reject the change in the current version.

N4.2: scribe2 SHALL carry out a constitution amendment only with a new ADR, inline delta markers in this file and a user ruling id (A2), and SHALL carry out schema-incompatible changes and changes that would exceed a C4 bound only as a new version.
<!-- constitution:end -->
