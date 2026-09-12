# 設計: 作業記憶の退避・復元・消費 — 器の subcommand 3 つが機械層を担い、命令行の出所 pointer を形で検査する

- 要件: [FR23](../../design-intent/spec/srs.html#FR23) 作業記憶の作法 6 本 + subcommand + 暫定行 / [AC8](../../design-intent/spec/srs.html#AC8) / [FR26](../../design-intent/spec/srs.html#FR26) cap guard（不変）/ [FR27](../../design-intent/spec/srs.html#FR27) [FR29](../../design-intent/spec/srs.html#FR29) 合図（不変）/ [FR28](../../design-intent/spec/srs.html#FR28) cycle（不変）/ [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed。制約: CON2（PUBLIC）
- 憲法: [N2](../../design-intent/spec/constitution.html#n2) prose の規則は規則でない / [C2](../../design-intent/spec/constitution.html#c2) 宣言順・C2.2 env 不読 / [C3](../../design-intent/spec/constitution.html#c3) C3.3 typed / [C8](../../design-intent/spec/constitution.html#c8) 前の版を oracle にしない / [C11](../../design-intent/spec/constitution.html#c11) C11.2 極性 / [C12](../../design-intent/spec/constitution.html#c12) 歯は Rust / [C15](../../design-intent/spec/constitution.html#c15) 台帳に規律を置かない
- 決定: [ADR-0018](../../design-intent/decisions/ADR-0018-working-memory-subcommands-and-pointer-required-directives.html) / [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html)（sid は打刻から）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.2（enum の 4 つ組）/ [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.4 / §2.5
- 土台: [seat-autonomy.md](./seat-autonomy.md)（tick / cycle・退避物の走査 `scan_wm`・置き場）/ [seat-state.md](./seat-state.md)（打刻の schema・`sid`）。crate の形は [rules-manifest.md §2](./rules-manifest.md)。
- この設計から出る契約: §9（3 便・順序あり）。

## 1. 何を解くか

開発 session（planner / 管理席）の作業記憶の**機械層**を器に入れる: 退避物の生成（externalize）・復元 DATA の出力（rebrief）・退避物の消費（consume）。brief の**文章と判断**は開発 session（LLM）が DATA から組む。命令行の出所 pointer を器が**形と実在**で検査し、無い行を暫定行として印付きで出し、次の cycle に引き継がない。前の版の script（python / bash）への依存を切る。

やさしく言うと: 「退避して・作り直して・思い出す」のうち、file を作る・事実を並べる・使い終えた file を移す、の 3 つを器がやる。「今どこで次に何をするか」を考えて文章にするのは session のまま。命令の各行には「規則はここ」という矢印を必ず付け、矢印の無い行は次の周に持ち越さない。

## 2. 置き場と名前（SRS FR23 が固定・変えない）

- 退避物 = `<wm_dir>/working-memory.<sid>.md`（`<wm_dir>` は `--wm-dir` で受ける・既存の tick / cycle と同じ・anchor 配下の `.claude-session/` を user が指す）。消費済み = `working-memory.<sid>.consumed.md`（move）。
- 自席の弁別 = frontmatter の `seat:` が `--target` と一致（`scan_wm` と同じ・file 名の sid ではない）。
- 現在の sid = `<state_dir>/seat/<潰した target>/state.jsonl` の**最終打刻行の `sid`**（ADR-0015・hook の `session_id`）。打刻が無い / 読めない周は rc 非 0（`sid-missing` / `sid-unreadable`・env と pane は読まない）。
- frontmatter（YAML 風の `key: value` 行・`---` で囲む）: `schema: 1`（新規・必須）/ `seat:` / `role:`（表示用・弁別には使わない）/ `externalized_at:`（UTC）/ `trigger:` / `carry_source:` / `carry_items:` / `carry_user_directives:` / `consumed-from:`（consume が current sid 名義へ移すときだけ）。未知 key は拒まない（開発 session の注記を許す）が、`schema` と `seat` が無い file は「読めない」扱い。

## 3. 3 節の schema（前の版の lib と同じ見出し・器が検査する）

| 節 | 見出し（固定） | 書き手 | 中身 |
|---|---|---|---|
| 1 | `## user 直命（verbatim・言い換え禁止）` | carry-forward + 開発 session の追記 | `- [YYYY-MM-DD HH:MM] 「<逐語>」 → 状態: <未着手 / 着手中 <id> / 完了 <id> / user 撤回>` |
| 2 | `## 計画弧・次のステップ` | 開発 session（毎 cycle 上書き） | 自由文の項目行 |
| 3 | `## この effort を貫く命令・制約` | carry-forward + 開発 session の追記 | `- [auto\|confirm\|hard候補] [P0-P3] since=YYYY-MM-DD <本文> → SSOT: <参照>` |

- 節 3 の項目行の文法検査: tag（3 語のいずれか）・`[P0-P3]`・`since=`・`→ SSOT:` の 4 要素。欠けた行は externalize が rc 非 0 で止まる（黙って通さない・前の版の「警告だけ」からの変更）。従属行（先頭が空白）は直前の項目に連結。HTML コメントは捨てる（テンプレの説明文が命令欄へ流入しない）。
- 節 1 は逐語 carrier: 状態が「完了」「user 撤回」の行だけ carry から落としてよい。それ以外は 1 行も落とさない・言い換えない。

## 4. 出所 pointer（ADR-0018 §2.2）

```rust
/// 宣言順 = 優先順位（C2・散文の注記を持たない）。
pub enum PointerKind { Constitution, Adr, Design, Manifest, Ledger, RepoPath, Memory, PullRequest }
pub enum Resolution { Resolved, Unresolved, Unchecked }
```

| kind | 形（分類） | 実在検査 |
|---|---|---|
| `Constitution` | `憲法 <id>` / `<id>` ∈ `C\d+(\.\d+)?` `A\d(\.\d)?` `N\d(\.\d)?` | `design-intent/spec/constitution.html` に `id="<小文字 id>"` が在る → Resolved / 無い → Unresolved |
| `Adr` | `ADR-\d{4}`（§ 付きを許す） | `design-intent/decisions/ADR-<n>-*.html` が在る |
| `Design` | `docs/design/<名>.md`（§ 付きを許す） | file が在る |
| `Manifest` | `rules 行 <id>` / manifest の行 id（`R-…` / `<領域>.<名>`） | `rules/manifest.toml` の `id =` に在る |
| `Ledger` | `<prefix>-<id>`（台帳 prefix は `.beads` から解く・notes の時刻を許す） | Unchecked（rebrief の bd 読みとは独立・実在は brief 側で `bd --readonly show`） |
| `RepoPath` | repo 内の相対 path（`#節` を許す） | file が在る |
| `Memory` | `auto-memory <slug>` / `memory <slug>.md` | Unchecked（repo 外・host 固有） |
| `PullRequest` | `PR #<n>` / `#<n>` | Unchecked |

- 1 行に複数の参照が在れば**最も強い kind**（宣言順で最小）を行の kind とし、実在は各参照ごとに検査して 1 つでも Unresolved なら行を `unresolved` と印す（暫定ではない・「矢印は在るが先が無い」）。
- pointer が無い・どの形にも分類できない行 = **暫定行**（`pointer=none`）。
- 器は内容の食い違い（行の本文 vs 憲法）を判定しない。DATA に kind と resolution を載せ、brief（LLM）が優先順位で読む。
- 4 つ組（ADR-0013 §2.2）: `PointerKind::ALL` const slice・網羅 match・判別子順 pin の歯・`as_str`。

## 5. subcommand（`<NAME> seat …`・共通引数 `--target T --wm-dir D --state-dir S`・出力は emit 経由）

### 5.1 `seat externalize --plan FILE --directives FILE [--user FILE] [--trigger manual|tick]`
1. sid を打刻から得る。自席の未 consumed 退避物が在れば rc 1（`wm-exists`・二重退避を取り合わない）。
2. carry-forward: 自席の最新の `.consumed.md`（mtime 降順・`seat:` 一致）から節 1（全行・「完了」「user 撤回」は落とす）と節 3（pointer 検査 → **暫定行を落とす**・`[P0-P3]` 安定 sort）を運ぶ。上限 = rules 行 `seat.wm_directive_cap`（新 kind `WmDirectiveCap`・裁定 id 付き・C5）。超過は rc 1（`directive-cap`・黙って切らない）。
3. 入力: `--plan`（節 2 の本文）・`--directives`（節 3 の新規行・文法検査）・`--user`（節 1 の追記行・逐語）。検査に落ちた行は行番号付きで全件 stderr へ・rc 1。
4. file を `working-memory.<sid>.md` に書く（`create_new`・frontmatter 付き）。stdout 1 行 `seat: externalized file=<name> carried=<n> dropped_provisional=<k> directives=<m>`。
- 極性: 書けない・検査に落ちる → FailClosed（退避物を作らない・rc 1）。

### 5.2 `seat rebrief [--anchor DIR]`（read-only）
- 出力は行頭 marker の typed 行（1 行 1 事実・値は enum の名か逐語）。marker は閉じた enum `Marker` の `as_str`（外形 snapshot で pin・C12.5）。
- 段: (1) `[SID]` (2) 退避物: `[WM] found file=…` / `[WM] candidate file=…`（`seat:` 一致・sid 違い＝`/clear` 後の通常形）/ `[WM] missing` (3) `[WM-PLAN]` 節 2 の各行（marker 前置）/ `[WM-USER-DIRECTIVE]` 節 1 の各行（**逐語**）/ `[WM-DIRECTIVE] kind=<PointerKind|none> resolution=<…> line=<逐語>` 節 3 の各行 + `[WM-DIRECTIVE-COUNT] total=<n> provisional=<k> unresolved=<u>`（件数と列挙の対）(4) `[ORPHAN-WM] file=… seat=<他席>` 別席の未 consumed 退避物（消費しない）(5) 台帳: `bd --readonly list --limit 0 --json` を子 process で読み `[BD-COUNT] open= in_progress= blocked=`・`[BD-INPROGRESS] <id> updated=<ts> <title>`・節 3 が言及する bead id の status を `[DIFF] <id> bd=<status>` で並べる (6) `[TICKET-CANDIDATE]` 節 3 の行のうち「器を直す」旨で bead pointer を持たない行（起票候補・**起票しない**）。
- 出せない周（wm dir が読めない / 打刻が無い / bd の rc 非 0 か JSON 不能 / anchor 不在）は **DATA を 1 行も出さず** rc 2 + 理由 1 行（FailClosed・「BD-COUNT=0」に化けない）。`[WM] missing` は正常（DATA は出る）。
- bd の timeout は rules 行 `hook.timeout_s` を流用せず、`fleet.usage_timeout_s` とも別の行 `seat.ledger_timeout_s`（新 kind・裁定 id 付き）。

### 5.3 `seat consume`
- 対象 = 自席の未 consumed 退避物 1 件（`seat:` 一致）。sid が現在と同じなら `working-memory.<sid>.consumed.md` へ rename。違うなら `working-memory.<現在 sid>.consumed.md` へ rename して frontmatter に `consumed-from: <元 sid>` を 1 行足す。
- 冪等: 対象なし かつ 現在 sid の `.consumed.md` が在る → rc 0 `already`。対象なし かつ 無い → rc 1 `wm-missing`。自席の未 consumed が 2 件以上 → rc 1 `wm-ambiguous`（消費しない）。他席の退避物は触らない。
- 極性 FailClosed。

## 6. 開発 session 側の手順（skill の入口・本 repo の外・形だけ）

- 退避: skill は計画弧と新規命令行を file に書き（scratchpad）、`<NAME> seat externalize …` を撃つ。退避物を直接 Write / Edit しない。
- 復元: skill は `<NAME> seat rebrief …` の DATA を読んで brief を組み（俯瞰 4 slot・3 値則は skill の規律）、提示の後に `<NAME> seat consume …` を撃つ。rc 2 の周は brief を出さない。
- tick / cycle が注入する字面（`/ready-compaction` / `/rebrief`）は変えない。

## 7. 極性一覧

- Guard は足さない（退避・復元・消費は行為を止める判定ではない）。境界の enum（`ExternalizeError` / `RebriefError` / `ConsumeError`）は各 `POLARITY = FailClosed`。cap guard の `SeatDecision::Externalize`（FR26）は不変。

## 8. 歯（`crates/<NAME>/tests/e2e/seat.rs` に `seat_wm_` 接頭辞・tmp dir を `--wm-dir` / `--state-dir` で指す・名前の列は現物が SSOT）

何を測るか（契約 (a) externalize）: 打刻の sid で file 名が決まる・打刻不在は rc 1／未 consumed が在れば rc 1／consumed からの carry で暫定行が落ち・pointer 行は P 昇順で残り・節 1 は逐語で全行残る（「完了」だけ落ちる）／文法欠落行は行番号付きで全件 rc 1／上限超過は rc 1／frontmatter に `schema: 1` と `seat:`／`PointerKind` の判別子順 pin と 8 形の分類（property: 任意の参照文字列が高々 1 kind）／実在検査の 3 値（憲法 id の在・不在・repo 外）。

何を測るか（契約 (b) rebrief）: 外形 snapshot（marker の並び）／`[WM] found` / `candidate` / `missing` の 3 形／`[WM-DIRECTIVE]` の kind・resolution が期待どおり・COUNT が列挙数と一致／orphan は列挙され file は不変（size・mtime）／偽の bd（`--bd PATH` で差す test helper binary）で BD-COUNT と DIFF・bd の rc 非 0 と JSON 不能で DATA 0 行 + rc 2／wm dir 不読で rc 2／read-only（wm dir・state dir の全 file の size・mtime 同一）。

何を測るか（契約 (c) consume）: 同 sid の rename／sid 違いの current 名義 + `consumed-from`／冪等 `already`／2 件で `wm-ambiguous` かつ不変／他席は不変。

AC8 の実演（D・2 つの開発 session で退避 → cycle → 復元を 1 回ずつ）は land 後に planner が行い bead notes に逐語で記帳する（歯にしない・CON2）。

## 9. 契約（3 便・この順）

- **(a)** `seat externalize` + `PointerKind` / `Resolution` + 3 節 schema の parser + carry-forward + rules 行 `seat.wm_directive_cap`（裁定 id が先・C5）。base で RED = externalize の歯（機能不在）+ `PointerKind` の判別子順 pin。write-set は構造の連鎖（`seat/` の新 module 宣言先 + `rules/mod.rs` + manifest + kind 件数の歯 + 外形 snapshot + polarity の境界 enum）。
- **(b)** `seat rebrief` + `Marker` enum + bd の子 process + rules 行 `seat.ledger_timeout_s`（裁定 id が先）。(a) の parser に依存。
- **(c)** `seat consume`。(a) に依存。land 後: global skill 2 本を器の入口へ書き換え（本 repo の外・user 手番）→ AC8 の実演。

## 10. 却下案（ADR-0018 §5 の写しは持たない・設計固有のもの）

- 節 3 の文法欠落を警告だけで通す（前の版）: 出所の無い行が周を越える経路が残る。rc 1 で止める。
- pointer の実在検査を全 kind で必須にする: Ledger / Memory / PR は repo 外か host 固有で、器が到達できない。3 値（Resolved / Unresolved / Unchecked）で持つ。
- rebrief が bd を直接 DB で読む: 台帳 adapter は MVP 外（ADR-0004 §2.2 面 3）。`bd --readonly` の子 process で読む（user 裁定）。
- DATA を JSON 1 行にする: 開発 session が読む面は行頭 marker の行が既存の skill と同型で、外形 snapshot で pin できる。
- 退避物の書き手を開発 session のまま（Write tool）にし器は検査だけ: 検査を通らない file が残る経路（Write は通り検査は後）。器の口だけが書く。

## 11. 後続

- transcript からの user 逐語の抽出（前の版の digest・config dir を要する）→ hook の payload 経由か v3。
- auto-compact の検出（PreCompact hook の打刻 → rebrief の `[MODE] force-recovery`）。
- 飛行中の Agent / Workflow の列挙（前の版の overlay）。
- cap guard の退避物編集の口の撤去（skill 退役後）。
