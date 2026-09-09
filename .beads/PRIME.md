[bd prime] このリポの永続タスク台帳のワークフロー文脈。ポリシーの SSOT はこの `.beads/PRIME.md`（`bd prime` 出力を上書き）。出力が切れていたら全文を読んでから続行すること。

# Beads Workflow Context (scribe2)

> SessionStart hook が `bd prime` を自動実行し、この内容を注入する（新規・resume・clear・compaction 後の再開）。

## この repo の前提（最重要）
- **scribe v1 plugin は積まない**（2026-09-09 user 直命）。役割別の外部注入は無い＝**本 PRIME が bd 運用の唯一の SSOT** である。
- 起票・依存・close はすべて作業 session が自分で行う。「誰が」を分ける規約は持たない（席が 1 つしかない）。
- 役割を超えた開発規約（作業の流れ・done の定義・review・ADR を書く条件）の SSOT は repo の `CLAUDE.md` と `design-intent/spec/`。

## 役割分担
- **タスクと契約と裁定 → beads**: セッションを越えて残す作業と、契約（acceptance）と、裁定の記帳先。
- **知識・知見 → repo tracked な carrier**: 原理と決定は `design-intent/`（憲法 / ADR / research）、要件は `design-intent/spec/srs.html`、設計は `docs/design/`。
- **beads は規律を持たない**（憲法 C15）。規律との接続は id 参照 2 本だけ——契約が要件 id / 設計 doc を指す・規則を変える PR・bead が裁定 id を指す。
- **`bd remember` / `bd recall` / `bd memories` は使わない**（consolidation 機構が無く肥大化する）。
- **後継を持たない機能が 2 つある**: 機械横断な事実の共有と semantic 検索には後継なし——能力喪失として受容する。代替は `git grep` と ADR 索引の語による探索で、届くのは repo tracked な carrier まで。auto-memory は非 tracked ゆえ母集団に入らない（host 固有の事実はそこに留め、昇格させない）。

## 台帳の規則（本文 SSOT・pointer 先は持たない）
- **R0** 読みは `bd --readonly <sub>` 形。1 bead = 1 PR。close は merge の後。
- **R1** notes は `--append-notes`（`--notes` は置換＝過去の記帳を消す）。description は `--body-file`（heredoc の引用崩れを避ける）。
- **R2** 新規 bead は必ず epic に属させる（`--parent`）。**親の label は継承されるので起票直後に labels を実測する。** 所属 = parent-child ／ blocks = 順序専用。
- **R3** memo 段階（契約未確定）は label `intake:memo` で名乗る。
- **R4** priority は P0〜P4 の 5 段（`--priority 0..4`）。"high"/"medium"/"low" は不可。
- **R5** `bd --readonly ready` の既定 limit は 100。「ready に入っていない」判定は `--limit 0` で行う。
- **R6** acceptance は field を正とする（description の写しを機械が読むことはない）。検証は「base で RED になる test」で表す。

## write の形
- 本 repo の台帳は remote（`.beads/.env` の `BD_SYNC_REMOTE`・tracked config へは書かない）へ同期する。**write は `scripts/bdw` 経由**（flock 直列化・実体は host の beads-bdw に path 解決）。`bd dolt push` は session の終端で 1 回。
- 破壊的な bd 操作（台帳の削除・強制 import 等）は host の guard が止める。止められたら回避策を打たず、guard が返す代替ルートに従う。
- `bd edit` は使わない（$EDITOR を開き agent をブロックする）。

## 人間確認の発火条件
人間確認が要るのは憲法 Ask-first（A1〜A4）が定める場合だけ: **消す / 出す / 使う** の 3 クラス（A1）・憲法改訂と閾値変更（A2）・依存 OSS の増減（A3）・自己開発の解禁（A4）。それ以外は AI が決めて進む。順序や是認だけを求める問いは出さない。複数の妥当な設計が併存し選択が目的・価値観に依存するときは、承認要求でなく grill（1 論点 1 質問・推奨 1 つ）として上げる。

## ⚙️ バージョン管理・保守
- **bd はピン解除**。ただし `bd upgrade` / `npm install -g @beads/bd` を実行する**前に**、アップグレード先バージョンに問題（特に migration によるマルチマシン同期破壊）が無いかを検証してから上げる。npm global bd は **OS ユーザー単位で共有**されるため、任意アカウントの upgrade が全アカウントへ波及する。
- **remote-backed bd DB の schema 移行は単一指定移行者のみ**が `BD_ALLOW_REMOTE_MIGRATE=1 bd migrate` → `bd dolt push` で行う（他 clone は `bd bootstrap` 再取得で未 push を喪失しうるため、先に `bd export` で backup を取る）。
- 参考: v1.0.5+ の migration 0043 が過去にマルチマシン同期を破壊した（upstream #4259）。
- この台帳は `bd init --skip-agents --skip-hooks` で導入済（bd に CLAUDE.md/AGENTS.md を汚染させない）。本 PRIME.md がポリシー SSOT で、bd は再生成しない。

## Essential Commands（要点。全コマンド・詳細は `bd --help` / `bd <cmd> --help`）
- 探す: `bd --readonly ready --limit 0` / `bd --readonly list --limit 0` / `bd --readonly show <id>` / `bd --readonly search <query>` / `bd --readonly dep tree <id>`
- 作る/更新: `scripts/bdw create --title="..." --body-file F --type=task|bug|feature --priority=2 --parent <epic>` / `scripts/bdw update <id> --claim` / `scripts/bdw update <id> --acceptance "$(cat F)" --append-notes "..."`
- 完了/依存: `scripts/bdw close <id> --reason="..."` / `scripts/bdw dep add <issue> <depends-on>`
- 同期/健全: `bd dolt push` / `bd dolt pull` / `bd --readonly stats` / `bd doctor`

<!-- beads-init-template v:2 — このファイルは scribe:setup（旧 beads-init）skill 由来。skill はこの marker の `v:N` バージョン番号で「我々の版か」と「role 中立版か」を判定する。この行（特に `v:N`）を残せば手動編集しても上書きされない。scribe2 は v1 plugin を積まないので本 PRIME が唯一の SSOT（role 別注入は無い）。 -->
