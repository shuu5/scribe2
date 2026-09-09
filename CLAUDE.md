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
