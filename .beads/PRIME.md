[bd prime] このリポの永続タスク台帳のワークフロー文脈。ポリシーの SSOT はこの `.beads/PRIME.md`（`bd prime` 出力を上書き）。出力が切れていたら全文を読んでから続行すること。

# Beads Workflow Context (scribe2)

> SessionStart hook が `bd prime` を自動実行し、この内容を注入する（新規・resume・clear・compaction 後の再開）。compaction 後は PostCompact(ready-compaction) で Working Memory が、SessionStart で本文脈が復元される。

## 役割分担（最重要）
- **タスクと規律（soft-rule）→ beads**: 着手中 / 保留 / 依存のある作業に加え、セッションを越えて効かせたい**規律（soft-rule）**も bd issue で追跡する。セッションを跨いで永続する。
- **知識・知見 → 外部ストアでなく repo と host の既存 carrier**: 教訓（事故・実測の知見）は repo 内の教訓 doc へ、その repo × その host の作業文脈は auto-memory へ置く（規律（soft-rule）の置き場は上の 1 行目が定めるとおり対の bd issue）。**`bd remember` / `bd recall` / `bd memories` は使わない**（consolidation 機構が無く肥大化するため）。beads が持つのはタスクと規律（soft-rule）で、知識・知見は持たない。
- **後継を持たない機能が 2 つある**: 機械横断（host を跨ぐ）な事実の共有と `semantic`（意味類似）検索には**後継なし**——能力喪失として受容する。代替は `git grep` と教訓 doc の索引による語での探索だが、これが届くのは repo tracked な carrier までで、**auto-memory は非 tracked ゆえ `git grep` の母集団に入らない**（その探索面は各 host の `MEMORY.md` 索引だけである）。意味類似の想起そのものは代替が担わない。host 固有の事実は auto-memory に留め、昇格させない。
- 一時的なセッション内 TODO（TodoWrite）や workflow / Agent オーケストレーションは併用してよい。ただし**セッションを越えて残すべき作業は必ず bd issue 化**する。

## セッション終了時の bd（role 中立の基礎）
- 完了した issue は `bd close <id> [--reason "..."]` で閉じる。
- コードは標準 PR ワークフロー（`main` へ直 push しない）。

> **役割を帯びた規約の SSOT は scribe plugin の role 別 SessionStart 注入**（admin / worker / consult）。「誰が `bd create` / `bd dep` / `bd dolt push` / close するか・残作業のフォローアップ起票の可否・終了プロトコルの全手順・close→gate の順序」は role ごとに scribe が配る（規約本文は scribe plugin の `docs/protocol.md`）。本 PRIME は role 中立な bd 基礎のみを持ち、役割を帯びた指示を全セッションへ一律注入しない（案 A 責務分割＝worker への過剰注入が `bd create` 逸脱の構造原因。scribe `docs/role-context-spec.md` §0）。scribe plugin を導入していないプロジェクトでは、本節は「単独 worker は素の bd で運用」と読み替えてよい。

## 承認体制（人間確認の発火条件）

裁定本文（一次 SSOT = scriptorium bd `orch-vhiu`）の **canonical 3-クラス block v2** を **verbatim 搬送**する（言い換え・reflow・行結合・箇条書き記号の付加を禁止）。改訂は本 template ではなく裁定 SSOT 側で行い、ここへは搬送のみする。搬送形の byte 同一性は「各行から定数接頭辞 `> `（空行は末尾スペース無し）を除いた結果の sha256 が正本 18 行と一致する」ことで定義する。

<!-- canonical-3class-block-v2:begin prefix="> " -->
> 【人間確認が要るのは「取り消せない」3 クラスのみ】
> (a) 消す — データ / repo / 履歴 / live 成果物の破壊（第一防衛線は機械 guard 層）
> (b) 出す — public 化・外部公開・外部サービスへの送信（scriptorium 核② private 保証はこの型）。判定単位は repo でなく「情報」＝public 面の情報集合を増やすかで判定する。private 配備層から public engine への同期のような境界事案は機械 2 条件〔① 配備層 file を touch しない ② private 実名 DATA literal が 0 hit〕が両方 green なら非該当（AI 判断で merge）・どちらかが赤 or 機械照合できないなら (b) として人間確認へ倒す fail-safe。「既に public な repo だから非該当」という repo 単位の断定は禁止（public repo の中に private 由来の同期先が在りうる＝実例 scribe 内 scriptorium-engine）。〔裁定 R-A・2026-07-26〕
> (c) 使う — 追加課金が発生する操作（従量課金 API 呼出 / 有料サービスの新規契約 / クラウド資源の課金発生）。定額プラン内は対象外＝token 消費それ自体は非該当（Workflow を何 M token 回しても (c) に当たらない）。旧文言「大きな金銭コスト（承認でなく予算上限で制御）」は観測できず死文化するため廃止した。〔裁定 R-B・2026-07-26〕
>
> それ以外（規約ファイル・全ホスト配布物・事前合意逸脱を含む）は AI 敵対 gate 通過をもって AI 判断で merge する。
>
> 【聞かないこと】順序・選択肢の是認だけを求める問いは出さない（AI が推奨を出し、決めて進む）。
> 【上げること】複数の妥当な設計が併存し、選択が人間の目的・価値観に依存するとき＝承認要求ではなく grill 提案として上げる。事実で決まるなら止めない。
>
> 【本裁定で緩めないもの（fence）】
> AI 敵対 gate / write-isolation（foreign 台帳 write 禁止）/ 完了 truth=bd（終端宣言）/ 破壊操作の機械 guard / 核② private 保証（orch-ufz・orch-xkec boundary）/ gate 分離（worker は自己 merge しない・gate-pending funnel）/ 承認要求の可視性様式（🔴 バナー・会話ログ面 text 既定〔AskUserQuestion は材料不要の例外限定・裁定 2026-08-22〕・安売り禁止）
> ※様式は存続。変わるのは発火条件（④ 該当 → 3 クラス該当）だけ。
>
> 【必ず添える 3 つの誤読防止句】
> 1. 人間承認を外しても gate は外さない（gate が実効安全弁になったので強化側）。
> 2. 「worker が自己 merge してよい」ではない（gate 分離＝独立レビューは不変）。
> 3. front-load / バナーは廃止でなく scope 縮小（user 裁定 2026-07-17 の可視性要件を壊さない）。
<!-- canonical-3class-block-v2:end -->

**bridge（block の外の註）**: block 内の 核② / orch-ufz / orch-xkec は scriptorium 側の private 保証境界を指す外部参照（本 project に定義を持たない）。「④ 該当」は旧カテゴリ番号への参照で、本 PRIME に ①〜④ の番号体系は無い。gate 分離・完了 truth=bd 等の運用定義は、scribe plugin を導入している project では role 別 SessionStart 注入（`docs/protocol.md`）が SSOT。

## ⚙️ バージョン管理・保守（重要）
- **bd はピン解除**。ただし `bd upgrade` / `npm install -g @beads/bd` を実行する**前に**、アップグレード先バージョンに問題（特に migration によるマルチマシン同期破壊）が無いかを検証してから上げる。npm global bd は **OS ユーザー単位で共有**されるため、任意アカウントの upgrade が全アカウントへ波及する点に注意。
- **remote-backed bd DB の schema 移行は単一指定移行者のみ**が `BD_ALLOW_REMOTE_MIGRATE=1 bd migrate` → `bd dolt push` で行う（他 clone は `bd bootstrap` 再取得で未 push を喪失しうるため、先に `bd export` で backup を取る）。
- 参考: v1.0.5+ の migration 0043 が過去にマルチマシン同期を破壊した（upstream #4259）。現行は移行検証を経て運用する（旧・v1.0.4 ピンは撤廃）。
- このプロジェクトの beads は `bd init --skip-agents --skip-hooks` で導入済（bd に CLAUDE.md/AGENTS.md を汚染させない）。本 PRIME.md がポリシー SSOT で、bd は再生成しない。

## Core Rules
- 着手する issue は `bd update <id> --claim` で in_progress 化する（新規 issue の起票 `bd create`・依存 wire は role 規約に従う＝上記の scribe role 注入を参照）。
- セッション開始時は `bd ready` で着手可能タスクを確認。
- `bd edit` は使わない（$EDITOR を開きエージェントをブロックする）→ `bd update <id> --description/--title/--notes` でインライン更新。
- priority は 0-4 / P0-P4（値そのものの意味定義の SSOT は「台帳構造」節 R8）。"high"/"medium"/"low" は不可。
- 依存は blocks / parent-child 等。`bd ready` をブロックするのは blocking 系（blocks/parent-child/conditional-blocks/waits-for）のみ。
- **並列 spawn（同一マシンで複数 worker が同時稼働）時は bd の write（`--claim` / `--notes` / `close` 等）を直列化**する（embeddeddolt は single-writer ＝同時 write は lost-update）。anchor リポに flock ラッパ（`scripts/bdw` 等）があればそれ経由で write する。逐次 1-worker は素の bd で可、read-only worker は `bd --readonly`。

## 台帳構造（graph の形・role 中立）

台帳を平場にしないための構造規約。**本文 SSOT = `docs/contracts/ledger-rules.md`**（下は索引で、判断が要るときはそちらを Read＝本 file に本文を複製しない）。role 別義務は持たない。

- **R0** 読みは bd-native 第一選択（`bd --readonly <sub>` 形）。
- **R1** epic 帰属が原則。孤立 issue は lint が warn で数える。
- **R2** 所属 = parent-child ／ blocks = 順序専用。
- **R6** 適用は新規から（一括再編しない）。
- **R7** memo 段階は label `intake:memo` で名乗る（無印は memo 側へ倒す）。
- **R8** priority は P0〜P4 の 5 段で、leaf の属性として読む。
- **R9** FU 受け皿 epic への新規流入は停止（新規 FU は発生元 program へ `--parent` 直付け・受け皿は既存在庫の cohort 漸進消化のみ）。
- **fallback**: scribe plugin 未導入 project では pointer が解決しない。そのときは本索引の各 1 行を規則本文として読む。

## Essential Commands（要点。全コマンド・詳細は `bd --help` / `bd <cmd> --help`）
- 探す: `bd ready`（着手可能）/ `bd list [--status=open|in_progress]` / `bd show <id>` / `bd search <query>` / `bd blocked`
- 作る/更新: `bd create --title="..." --description="..." --type=task|bug|feature --priority=2` / `bd update <id> --claim`（in_progress 化）/ `bd update <id> --description/--notes/--title`（インライン更新）
- 完了/依存: `bd close <id> [--reason="..."]` / `bd dep add <issue> <depends-on>`（issue が depends-on に依存）
- 同期/健全: `bd dolt push` / `bd dolt pull`（refs/dolt/data 同期）/ `bd stats` / `bd doctor`

<!-- beads-init-template v:2 — このファイルは scribe:setup（旧 beads-init）skill 由来。skill はこの marker の `v:N` バージョン番号で「我々の版か」と「role 中立版か（N が現行 role 中立版の最小バージョン以上か）」を判定する（本文の自然言語フレーズには依存しない）。この行（特に `v:N`）を残せば手動編集しても上書きされない。役割を帯びた規約は scribe plugin の role 別 SessionStart 注入が SSOT（本 PRIME は role 中立）。 -->
