# 設計: 作業記憶の退避・復元・消費 — 器の subcommand 3 つが機械層を担い、命令行の出所 pointer を形で検査する

- 要件: [FR23](../../design-intent/spec/srs.html#FR23) 作業記憶の作法 9 本 + subcommand + 暫定行 / [AC8](../../design-intent/spec/srs.html#AC8) / [FR26](../../design-intent/spec/srs.html#FR26) cap guard（不変）/ [FR27](../../design-intent/spec/srs.html#FR27) [FR29](../../design-intent/spec/srs.html#FR29) 合図（不変）/ [FR28](../../design-intent/spec/srs.html#FR28) cycle（不変）/ [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed。制約: CON2（PUBLIC）
- 憲法: [N2](../../design-intent/spec/constitution.html#n2) prose の規則は規則でない / [C2](../../design-intent/spec/constitution.html#c2) 宣言順・C2.2 env 不読 / [C3](../../design-intent/spec/constitution.html#c3) C3.3 typed / [C8](../../design-intent/spec/constitution.html#c8) 前の版を oracle にしない / [C11](../../design-intent/spec/constitution.html#c11) C11.2 極性 / [C12](../../design-intent/spec/constitution.html#c12) 歯は Rust / [C15](../../design-intent/spec/constitution.html#c15) 台帳に規律を置かない
- 決定: [ADR-0018](../../design-intent/decisions/ADR-0018-working-memory-subcommands-and-pointer-required-directives.html) / [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html)（sid は打刻から）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.2（enum の 4 つ組）/ [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.4 / §2.5 / [ADR-0031](../../design-intent/decisions/ADR-0031-working-memory-is-held-by-the-vessel-directives-status-and-hooks.html)（§12・器が持つ側を広げる: 直命の表・現在地 DATA・hook 強制と自動退避・置き場は state dir）
- 土台: [seat-autonomy.md](./seat-autonomy.md)（tick / cycle・退避物の走査 `scan_wm`・置き場）/ [seat-state.md](./seat-state.md)（打刻の schema・`sid`）。crate の形は [rules-manifest.md §2](./rules-manifest.md)。brief の user 面は [dialogue-surface.md](./dialogue-surface.md) §3。
- この設計から出る契約: §9（3 便・順序あり・Landed 済み）+ §12.7（ADR-0031 の便・段 1 の 4 便は並列）。

## 1. 何を解くか

開発 session（planner / 管理席）の作業記憶の**機械層**を器に入れる: 退避物の生成（externalize）・復元 DATA の出力（rebrief）・退避物の消費（consume）。brief の**文章と判断**は開発 session（LLM）が DATA から組む。命令行の出所 pointer を器が**形と実在**で検査し、無い行を暫定行として印付きで出し、次の cycle に引き継がない。前の版の script（python / bash）への依存を切る。

やさしく言うと: 「退避して・作り直して・思い出す」のうち、file を作る・事実を並べる・使い終えた file を移す、の 3 つを器がやる。「今どこで次に何をするか」を考えて文章にするのは session のまま。命令の各行には「規則はここ」という矢印を必ず付け、矢印の無い行は次の周に持ち越さない。

## 2. 置き場と名前（SRS FR23 v0.13 と ADR-0031 §2.4 が state dir を固定・器の現物は契約 (e)〔§12.7〕の Landed まで anchor 配下の `--wm-dir` で動く＝本節は両方を書き分ける）

- 退避物 = `<wm_dir>/working-memory.<sid>.md`（`<wm_dir>` は `--wm-dir` で受ける・既存の tick / cycle と同じ・anchor 配下の `.claude-session/` を user が指す）。消費済み = `working-memory.<sid>.consumed.md`（move）。**契約 (e) 以後（§12.4・要件 FR23 の定める形）**: `<state_dir>/seat/<潰した target>/wm/` に置き `--wm-dir` は廃止。旧置き場の退避物は doctor が `wm-legacy=<n>` で名指す（器は読まない・消さない）。
- 自席の弁別 = frontmatter の `seat:` が `--target` と一致（`scan_wm` と同じ・file 名の sid ではない）。
- 現在の sid = `<state_dir>/seat/<潰した target>/state.jsonl` の**最終打刻行の `sid`**（[seat-state.md](./seat-state.md) §2・hook の `session_id`）。打刻が無い / 読めない / **sid が空**の周は rc 非 0（`sid-missing` / `sid-unreadable` / `sid-empty`・hook は `session_id` 欠落を空で打刻する現物があるため・env と pane は読まない）。
- frontmatter（YAML 風の `key: value` 行・`---` で囲む）: `schema: 1`（新規・**任意**・無ければ 1 相当として読む＝前の版の skill が書いた退避物をそのまま読む・2 以上は Err）/ `seat:` / `role:`（表示用・弁別には使わない）/ `externalized_at:`（UTC）/ `trigger:` / `carry_source:` / `carry_items:` / `carry_user_directives:` / `consumed-from:`（consume が current sid 名義へ移すときだけ）。未知 key は拒まない（開発 session の注記を許す）が、`seat` が無い file は自席として数えない（`scan_wm` と同じ）。

## 3. 3 節の schema（前の版の lib と同じ見出し・器が検査する）

| 節 | 見出し（固定） | 書き手 | 中身 |
|---|---|---|---|
| 1 | `## user 直命（verbatim・言い換え禁止）` | carry-forward + 開発 session の追記 | `- [YYYY-MM-DD HH:MM] 「<逐語>」 → 状態: <未着手 / 着手中 <id> / 完了 <id> / user 撤回>` |
| 2 | `## 計画弧・次のステップ` | 開発 session（毎 cycle 上書き） | 自由文の項目行 |
| 3 | `## この effort を貫く命令・制約` | carry-forward + 開発 session の追記 | `- [auto\|confirm\|hard候補] [P0-P3] since=YYYY-MM-DD <本文> → SSOT: <参照>` |

- 節 3 の項目行の文法検査（**`--directives` の新規行にだけ**掛かる・carry 元には掛からない）: tag（3 語のいずれか）・`[P0-P3]`・`since=` の 3 要素。欠けた行は externalize が rc 非 0 で止まる（黙って通さない・前の版の「警告だけ」からの変更）。`→ SSOT:` の有無は検査で止めない: 無い行は暫定行として退避物に入り（rebrief が `pointer=none` で列挙）、次の carry-forward で落ちる（FR23）。carry 元の行は文法検査せず、pointer 検査だけで運ぶ / 落とすを決める。carry 元の項目行に `[P0-P3]` が無ければ P3 として扱う（前の版と同じ・落とさない）。従属行（先頭が空白）は直前の項目に連結。HTML コメントは捨てる（テンプレの説明文が命令欄へ流入しない）。
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

- 1 行に複数の参照が在れば**最も強い kind**（宣言順で最小）を行の kind とし、実在は各参照ごとに検査して 1 つでも Unresolved なら行を `unresolved` と印す（暫定ではない・「矢印は在るが先が無い」）。**carry-forward では暫定行と同じく落とす**（先の無い矢印は prose にしか無い規則・落とした件数は `dropped_unresolved` で出す）。
- 「user 裁定 <時刻>」だけの参照は台帳 id を伴わなければ到達できず暫定行になる（退避時に bead id を添える）。上の表は決定時点の**初期集合**で、正本は `PointerKind::ALL` と seat の外形 snapshot（ADR-0013 §2.1）。
- pointer が無い・どの形にも分類できない行 = **暫定行**（`pointer=none`）。
- 器は内容の食い違い（行の本文 vs 憲法）を判定しない。DATA に kind と resolution を載せ、brief（LLM）が優先順位で読む。
- 4 つ組（ADR-0013 §2.2）: (a) 閉じた enum・(b) `PointerKind::ALL` const slice・(c) 網羅 match・(d) 判別子順 pin の歯。`Marker`（§5.2）も同じ 4 つ組で持つ。

## 5. subcommand（`<NAME> seat …`・共通引数 `--target T --wm-dir D --state-dir S`・出力は emit 経由）

### 5.1 `seat externalize --plan FILE --directives FILE [--user FILE] [--retire FILE] [--trigger manual|tick]`
1. sid を打刻から得る。自席の未 consumed 退避物が在れば rc 1（`wm-exists`・二重退避を取り合わない）。
2. carry-forward: 自席の最新の `.consumed.md`（mtime 降順・`seat:` 一致）から節 1（全行・「完了」「user 撤回」は落とす）と節 3（pointer 検査 → **暫定行と unresolved の行を落とす**・残りを `[P0-P3]` 安定 sort）を運ぶ。上限 = rules 行 `seat.wm_directive_cap`（新 kind `WmDirectiveCap`・裁定 id 付き・C5）。超過は rc 1（`directive-cap`・黙って切らない）。
   - **退役（`--retire FILE`・契約 (d)）**: Resolved の行は上の検査では落ちない（pointer が実在する限り周を越えて運ばれ、cap に達すると新規行を足せない）。役目を終えた行は開発 session が `--retire` の file に**節 3 の項目行を逐語で**（rebrief の `[WM-DIRECTIVE] … line=` の値をそのまま）1 行 1 本で並べ、器が carry 元の項目行と**全文一致**（両端の空白だけ無視・従属行込み）で突合して落とす。落とした件数は `dropped_retired`。file の行のうち carry 元に一致しない行が 1 本でも在れば rc 1（`retire-unmatched`・行番号付きで全件 stderr へ・黙って残さない＝写し間違いで行が生き残る周を作らない）。退役は退避物の編集ではなく carry の入力である（退避物・`.consumed.md` は器だけが書く・§10）。節 1 は退役の対象外（逐語 carrier・落ちるのは「完了」「user 撤回」だけ・§3）。
3. 入力: `--plan`（節 2 の本文）・`--directives`（節 3 の新規行・文法検査）・`--user`（節 1 の追記行・逐語）・`--retire`（節 3 の退役行・突合）。検査に落ちた行は行番号付きで全件 stderr へ・rc 1。
4. file を `working-memory.<sid>.md` に書く（`create_new`・frontmatter 付き）。stdout 1 行 `seat: externalized file=<name> carried=<n> dropped_provisional=<k> dropped_unresolved=<u> dropped_retired=<r> directives=<m>`（`dropped_retired` は契約 (d) で増える field・`--retire` を渡さない周は 0）。
- 極性: 書けない・検査に落ちる → FailClosed（退避物を作らない・rc 1）。`wm-exists` / `directive-cap` / `retire-unmatched` / 文法の断りは**退避物の書込を止める判定**なので `Guard::Externalize`（InLoop / FailClosed）として極性一覧に載る（ADR-0014 §2.1・Guard は増えない＝境界 enum の variant が 1 つ増えるだけ）。

### 5.2 `seat rebrief [--anchor DIR]`（read-only）
- 出力は行頭 marker の typed 行（1 行 1 事実・値は enum の名か逐語）。marker は閉じた enum `Marker`（`ALL` + 網羅 match + 判別子順 pin の 4 つ組・出力順 = 宣言順・外形 snapshot で pin・C12.5）。
- 段: (1) `[SID]` (2) 退避物: `[WM] found file=…` / `[WM] candidate file=…`（`seat:` 一致・sid 違い＝`/clear` 後の通常形）/ `[WM] missing` (3) `[WM-PLAN]` 節 2 の各行（marker 前置）/ `[WM-USER-DIRECTIVE]` 節 1 の各行（**逐語**）/ `[WM-DIRECTIVE] kind=<PointerKind|none> resolution=<…> line=<逐語>` 節 3 の各行 + `[WM-DIRECTIVE-COUNT] total=<n> provisional=<k> unresolved=<u>`（件数と列挙の対）(4) `[ORPHAN-WM] file=… seat=<他席>` 別席の未 consumed 退避物（消費しない）(5) 台帳: `bd --readonly list --limit 0 --json` を子 process で読み `[BD-COUNT] open= in_progress= blocked=`・`[BD-INPROGRESS] <id> updated=<ts> <title>`・節 3 が言及する bead id の status を `[DIFF] <id> bd=<status>` で並べる (6) `[TICKET-CANDIDATE]` 節 3 の行のうち **tag が `[hard候補]` かつ kind が none**（typed な条件だけ・本文の意味は読まない）の行（起票候補・**起票しない**）。
- 出せない周（wm dir が読めない / 打刻が無い / bd の rc 非 0 か JSON 不能 / anchor 不在）は **DATA を 1 行も出さず** rc 2 + 理由 1 行（FailClosed・「BD-COUNT=0」に化けない）。`[WM] missing` は正常（DATA は出る）。
- bd の待ち上限は既存の `hook.timeout_s` を流用せず、専用の rules 行 `seat.ledger_timeout_s`（新 kind `LedgerTimeoutS`・値は契約 (b) が裁定 id 付きで manifest に置く＝本 doc は値を写さない・C1）。bd は器の外の OSS を子 process で呼ぶ初の例＝A3 の対象（承認 = user 裁定 2026-09-12・ADR-0018 §2.4）。rebrief は行為を止めないので Guard ではない（rc 2 は「DATA を出せない」の断り・ADR-0014 §2.1「状態を選ぶだけ」）。

### 5.3 `seat consume`
- 対象 = 自席の未 consumed 退避物 1 件（`seat:` 一致）。sid が現在と同じなら `working-memory.<sid>.consumed.md` へ rename。違うなら `working-memory.<現在 sid>.consumed.md` へ rename して frontmatter に `consumed-from: <元 sid>` を 1 行足す。
- 冪等: 対象なし かつ 現在 sid の `.consumed.md` が在る → rc 0 `already`。対象なし かつ 無い → rc 1 `wm-missing`。自席の未 consumed が 2 件以上 → rc 1 `wm-ambiguous`（消費しない）。**rename 先が既に在る → rc 1 `consumed-exists`（上書きしない・N1）**。他席の退避物は触らない。削除しない（move だけ・N1.2）。
- 極性 FailClosed。`wm-ambiguous` / `consumed-exists` は move を止める判定なので `Guard::Consume`（InLoop / FailClosed）として極性一覧に載る。

## 6. 開発 session 側の手順（skill の入口・plugin 同梱の skill 2 本・`skills/`）

- skill は plugin root の `skills/<name>/SKILL.md`（`skills/rebrief/` / `skills/ready-compaction/`・名前空間は `<NAME>:rebrief` / `<NAME>:ready-compaction`・憲法 C2.2）に置き、器の口と同じ repo で版を揃える。global 版は退役（別 repo の手番）。skill の本文は手順の散文であって規則ではない（憲法 N2）＝規則を述べる行は pointer を持つ。
- 退避: skill は計画弧と新規命令行を file に書き（scratchpad）、`<NAME> seat externalize …` を撃つ。退避物を直接 Write / Edit しない。
- 復元: skill は `<NAME> seat rebrief …` の DATA を読んで brief を組み（俯瞰 4 slot・3 値則は skill 側の手順）、提示の後に `<NAME> seat consume …` を撃つ。rc 2 の周は brief を出さない。
- tick / cycle が注入する字面（`/ready-compaction` / `/rebrief`）は変えない。

## 7. 極性一覧

- Guard を 2 つ足す（ADR-0014 §2.1「書込を止めうる判定」に当たる）: `Guard::Externalize`（境界 `seat::externalize::ExternalizeError`・InLoop / FailClosed）と `Guard::Consume`（境界 `seat::consume::ConsumeError`・InLoop / FailClosed）。`rebrief` は行為を止めないので載せない（`RebriefError` は境界 enum として `POLARITY = FailClosed` を持つが Guard ではない）。極性一覧 snapshot は 2 行増える（契約 (a) と (c)）。cap guard の `SeatDecision::Externalize`（FR26）は不変。

## 8. 歯（`crates/<NAME>/tests/e2e/seat.rs` に `seat_wm_` 接頭辞・tmp dir を `--wm-dir` / `--state-dir` で指す・名前の列は現物が SSOT）

何を測るか（契約 (a) externalize）: 打刻の sid で file 名が決まる・打刻不在は rc 1／未 consumed が在れば rc 1／consumed からの carry で暫定行が落ち・pointer 行は P 昇順で残り・節 1 は逐語で全行残る（「完了」だけ落ちる）／文法欠落行は行番号付きで全件 rc 1／上限超過は rc 1／frontmatter に `schema: 1` と `seat:`／`PointerKind` の判別子順 pin と 8 形の分類（property: 任意の参照文字列が高々 1 kind）／実在検査の 3 値（憲法 id の在・不在・repo 外）。／unresolved の行が carry で落ち `dropped_unresolved` に数えられる／sid が空の打刻で rc 1／極性一覧 snapshot に externalize の guard（in-loop / fail-closed）が 1 行増える。

何を測るか（契約 (b) rebrief）: 外形 snapshot（marker の並び）／`Marker::ALL` の判別子順 pin／`[WM] found` / `candidate` / `missing` の 3 形／`[WM-DIRECTIVE]` の kind・resolution が期待どおり・COUNT が列挙数と一致／orphan は列挙され file は不変（size・mtime）／偽の bd（`--bd PATH` で差す test helper binary）で BD-COUNT と DIFF・bd の rc 非 0 と JSON 不能で DATA 0 行 + rc 2／wm dir 不読で rc 2／read-only（wm dir・state dir の全 file の size・mtime 同一）。

何を測るか（契約 (c) consume）: 同 sid の rename／sid 違いの current 名義 + `consumed-from`／冪等 `already`／2 件で `wm-ambiguous` かつ不変／rename 先が既在で `consumed-exists` かつ不変／他席は不変／極性一覧 snapshot に consume の guard が 1 行増える。

AC8 の確認（SRS の FR23 の検証手法は I = 目視確認・2 つの開発 session で退避 → cycle → 復元を 1 回ずつ）は land 後に planner が行い bead notes に逐語で記帳する（歯にしない・CON2）。

## 9. 契約（3 便・この順）

- **(a)** `seat externalize` + `PointerKind` / `Resolution` + 3 節 schema の parser + carry-forward + rules 行 `seat.wm_directive_cap`（裁定 id が先・C5）+ `Guard::Externalize`。base で RED = externalize の歯（機能不在）+ `PointerKind` の判別子順 pin。write-set は構造の連鎖（`seat/` の新 module 宣言先 + `rules/mod.rs` + manifest + kind 件数の歯 + seat / rules の外形 snapshot + `polarity.rs` の Guard variant と極性 snapshot）。
- **(b)** `seat rebrief` + `Marker` enum + bd の子 process + rules 行 `seat.ledger_timeout_s`（裁定 id が先）。(a) の parser に依存。
- **(c)** `seat consume` + `Guard::Consume`。(a) に依存。land 後: global skill 2 本を器の入口へ書き換え（本 repo の外・user 手番）→ AC8 の確認（I）。
- **(d)** `--retire FILE`（§5.1 手順 2 の退役・`dropped_retired`・`retire-unmatched`）。(a) に依存。出所 = 退避物の命令行が cap に達し Resolved の行を落とす口が無かった周（台帳 s2-07l.235・user 承認 2026-09-14）。base で RED = `--retire` を渡す歯（機能不在で usage の断り）+ 一致しない行の rc 1 の歯。write-set = `seat/externalize.rs`（引数・突合・件数）+ seat の外形 snapshot（usage 行と stdout の field）+ `tests/e2e/seat.rs` の `seat_wm_externalize_` 歯。rules 行を足さない（cap の値は不変・A2 非該当）。

## 10. 却下案（ADR-0018 §5 の写しは持たない・設計固有のもの）

- 節 3 の tag / `[P0-P3]` / `since=` の欠落を警告だけで通す（前の版）: 形の崩れた行が周を越える。新規行は rc 1 で止める（pointer の有無は文法検査でなく暫定行の扱い・§3）。
- pointer の実在検査を全 kind で必須にする: Ledger / Memory / PR は repo 外か host 固有で、器が到達できない。3 値（Resolved / Unresolved / Unchecked）で持つ。
- rebrief が bd を直接 DB で読む: 台帳 adapter は MVP 外（ADR-0004 §2.2 面 3）。`bd --readonly` の子 process で読む（user 裁定）。
- DATA を JSON 1 行にする: 開発 session が読む面は行頭 marker の行が既存の skill と同型で、外形 snapshot で pin できる。
- 退避物の書き手を開発 session のまま（Write tool）にし器は検査だけ: 検査を通らない file が残る経路（Write は通り検査は後）。器の口だけが書く。
- 役目を終えた命令行の退役を、節 1 と同じ「→ 状態: 完了」を節 3 の行に付けて carry で落とす形にする（契約 (d) の却下案）: 節 3 の carry 元は開発 session が編集できず（器だけが書く・上の却下案と同じ線）、新規行として同文 + 状態を再投入する形は突合の規則が二重になる（新規行の文法検査と carry の一致）。突合の入力を `--retire` の 1 file に分けるほうが規則が 1 つで済む。
- 退役の一致を `since=` と本文の先頭 N 字で取る（契約 (d) の却下案）: N の値が新しい閾値になり、同じ since の行が複数在る周に誤って落とす。全文一致だけにする（rebrief の `line=` が逐語を出すので写せる）。

## 11. 後続

- transcript からの user 逐語の抽出（前の版の digest・config dir を要する）→ hook の payload 経由か v3（§12.1 の口は発言を受けた席が撃つ形で、抽出ではない）。
- 飛行中の Agent / Workflow の列挙（前の版の overlay）。
- cap guard の退避物編集の口の撤去（skill 退役後）。
- GitHub（PR・CI）を現在地 DATA に足す（A3 の裁定・ADR-0031 §5 (E)）。

## 12. 器が持つ側を広げる（ADR-0031・要件書の改訂の後に契約化）

やさしく言うと: 退避物に AI が手で写していた 3 つ（user の発言・今どこか・役割の既定）を器の記録と規則の表に移し、復元と退避の取得を hook で撃ち忘れられなくする。auto compaction が来ても器が最低限の退避物を残す。

### 12.1 直命の表（ADR-0031 §2.1）

- 記録 = event log の variant 2 つ: `DirectiveIssued { id, target, issued_at（器が打つ UTC）, text（逐語）, premise: Vec<Pointer>（§4 の `PointerKind` の再利用） }` / `DirectiveClosed { id, outcome: Done | Withdrawn | Expired, ruling }`。`EventKind` の末尾に宣言順で足す（`KINDS` の件数 pin・literal 構築点の歯・property の生成器が write-set）。
- 口 = `<NAME> seat directive add --state-dir S --target T --text "<逐語>" [--premise <pointer>]…`（発言を受けた席が 1 回撃つ・時刻は推定しない）/ `seat directive close <id> --outcome done|withdrawn|expired --ruling <id>` / `seat directive ls`（有効な直命の一覧・逐語）。
- rebrief の marker（宣言順・§5.2 の `Marker` enum に足す・外形 snapshot が動く）: `[DIRECTIVE] id=… issued=… premise=<kind:resolution>… line=<逐語>` / `[DIRECTIVE-COUNT] total=<n>` / `[DIRECTIVE-NONE]` / `[DIRECTIVE-REVIEW] id=… reason=<age|premise-closed|premise-superseded>` / `[DIRECTIVE-REVIEW-COUNT]` / `[DIRECTIVE-REVIEW-NONE]`。`age` の閾値 = rules 行 `directive.review_after_days`（新 kind `DirectiveReviewAfterDays`・`ValueShape::Int`・裁定 id = user 2026-09-15T07:22Z 問 1・値は manifest が持つ）。`premise-closed` は前提の台帳 id の status（rebrief の bd 読みと同じ 1 本の口）、`premise-superseded` は前提の ADR の `folio-status`（§4 の Adr の実在検査と同じ file を読む）。
- 退避物の節 1 は廃止（`--user` の引数を外す・carry-forward の対象外・`schema: 1` のまま「節が空」として読む）。器は直命の意味を判定しない（印を出すだけ）。

### 12.2 現在地の DATA（ADR-0031 §2.2）

- marker 4 種（`[RUN]` / `[SEAT]` / `[WIN]` は各 3 形: 列挙 / `-COUNT` / `-NONE`。`[MAIN]` は値 1 行ゆえ列挙だけで、`[WIN]` は着地を読めない周の `-UNKNOWN` 形を持つ＝variant は 11）: `[MAIN] sha=<短 sha> origin=<same|ahead|behind|unknown> porcelain=<n>`（anchor の git・既存の git の子 process）/ `[RUN] id=<run> stage=<Stage> account=<label> base=<sha> updated=<ts>`（event log の `RunStage` の最終値・終端〔Landed / Failed / Retired〕を除く）/ `[SEAT] target=… role=… state=<idle|busy> account=… model=…`（`SeatRegistered` の最終 row + state.jsonl の最終打刻）/ `[WIN] id=<bead> landed=<ts> sha=<短 sha>`（直近の自席の `.consumed.md` の `externalized_at` より後の Landed）。
- 読めない周は `-NONE` に潰さず `unknown` / `unreadable` の語で出す（C10）。GitHub は読まない（§11）。

### 12.3 hook による強制と自動退避（ADR-0031 §2.3・[vessel-hook.md](./vessel-hook.md) の面に 4 つ足す）

| hook | matcher | 器の口 | 注入 / 記録 | 極性 |
|---|---|---|---|---|
| `UserPromptExpansion` | 復元 skill 名 | `hook prompt-expansion` → `seat rebrief` | DATA 全行を `additionalContext` に（rc 2 は理由 1 行） | FailOpen（skill を止めない） |
| `UserPromptExpansion` | 退避 skill 名 | 同上 → §12.2 + §12.1 | 現在地 DATA と `[DIRECTIVE]` を注入（突合の強制） | FailOpen |
| `PreToolUse` | `Skill`（同じ 2 skill 名） | 同じ口 | AI が Skill tool で呼んだ周も同じ注入 | FailOpen |
| `SessionStart` | 既存（全 matcher） | `hook session-start` に 1 行 | `[WM] found\|missing\|unreadable`（実測） | 既存のまま |
| `PreCompact` | `auto` | `seat externalize --trigger auto` | 現在地 DATA + carry-forward だけの退避物（`trigger: auto`・入力 file なし）。`wm-exists` は成功の側 | FailOpen（exit 2 を返さない・user 裁定 2026-09-15T07:22Z 問 2） |
| `SessionEnd` | 全 | `hook session-end` | `reason` を state.jsonl に打刻（[seat-state.md](./seat-state.md) の schema・判定なし） | 記録のみ |

- 極性一覧に 4 面を載せる（in-loop の注入・C11.2 / C16.2）。hook の歯は stdin JSON の fixture で測る（既存の `hook_` の歯と同じ形・Claude Code は起こさない）。
- tick との融合: cycle の駆動は tick が既に持つ（[seat-autonomy.md](./seat-autonomy.md) §3・`wm-unconsumed` → `/clear` → 復元）。externalize の stdout に `next=/clear` を 1 行足し、skill は cycle 案内の散文を持たない。doctor は席ごとに「退避物の無いまま終わった session の件数」を名指す。

### 12.4 置き場（ADR-0031 §2.4・user 裁定 2026-09-15T07:22Z 問 3）

- `<state_dir>/seat/<潰した target>/wm/working-memory.<sid>.md`（打刻・tick・heartbeat と同じ席の dir）。`--wm-dir` は廃止し置き場は state dir から解く（unit は [seat-autonomy.md](./seat-autonomy.md) §8 の再生成で追随）。
- 同 sid の未 consumed が在る周の再退避 = 旧 file を `working-memory.<sid>.superseded.<ts>.md` へ rename して置き換える（台帳 s2-07l.289 の吸収・削除しない・N1.2）。
- 要件書 FR23（v0.13）が置き場を state dir に固定し、作法に可逆置換（上の rename）を持つ。契約 (e) の acceptance は FR23 の作法の本数を指す。

### 12.5 役割の既定（ADR-0031 §2.5）

- rules 行 `role.<役割>.model`（kind `RoleModel`・値は `Model` の別名）/ `role.<役割>.effort`（kind `RoleEffort`・閉じた enum）。値は裁定 id 付きで manifest が持つ（本 doc は写さない）。
- 起動行（[account-lifecycle.md](./account-lifecycle.md) §4・`derive_launch`）: 登録 row の `model`（s2-07l.313）が在ればそれ、無ければ行の既定を `--model` に、effort は行から。settings.json は読まない。

### 12.6 skill 2 本の縮小（ADR-0031 §2.6・[dialogue-surface.md](./dialogue-surface.md) §7 (h)）

- ready-compaction = `--plan`（次の 1 手・列の裁定・bead id）と `--directives`（作業中だけの制約・pointer 付き）の 2 file + 口 1 回。直命は §12.1 の口、現在地は §12.2、役割の既定は §12.5。
- rebrief = DATA から user 面 5 slot を組む手順 + consume。AI 面は表示しない。

### 12.7 契約（要件書の改訂の後・段の中は並列）

| 段 | 契約 | size | 依存 | write-set の芯 |
|---|---|---|---|---|
| 1 | (a) 直命の表: event variant 2 + `seat directive` 3 口 | M | SRS | `fleet/mod.rs`（`EventKind` + `KINDS` pin）/ `+seat/directive.rs`（新）/ `seat/cli.rs` / `Event` の literal 構築点（歯・property）/ seat 外形 snap |
| 1 | (c) 現在地 DATA: `[MAIN][RUN][SEAT][WIN]` | S | SRS | `seat/rebrief.rs`（`Marker` + `ALL`）/ rebrief 外形 snap / `tests/e2e/seat/wm.rs` |
| 1 | (f) 役割の既定: rules 行 2 種 + 起動行 | S | .313 .322 Landed | `rules/mod.rs` / `rules/manifest.toml` / `seat/cycle.rs`（`derive_launch`）/ rules 外形 snap |
| 1 | (g) planner の雛形 +4 行（dialogue-surface §7） | S | ADR-0032 | `seat/brief/planner.txt` / `hook_brief_planner` snap |
| 2 | (b) `[DIRECTIVE-REVIEW]` + rules 行 `directive.review_after_days` | S | (a) | `seat/rebrief.rs` / `rules/*` / snap |
| 2 | (d) hooks 4 面 + PreToolUse Skill | M | (c)・.303 / .304 と `hooks/hooks.json` で直列 | `hooks/hooks.json` / `hook/mod.rs`（event 名の enum）/ `+hook/prompt_expansion.rs`（新）/ `polarity.rs` + 極性 snap / `tests/e2e/hook.rs` |
| 3 | (e) 置き場を state dir へ + supersede + doctor 行 | M | SRS FR23 改訂・(d) | `seat/externalize.rs` / `seat/consume.rs` / `seat/tick.rs`（`--wm-dir` の撤去）/ `main.rs`（doctor 欄）/ snap |
| 3 | (h) skills 2 本の縮小 | docs | (a)(b)(c)(d) | `skills/*/SKILL.md` |
| 3 | (i) global の痩身（別 repo） | 外 | (g)(h) | — |

検証の形（base で RED）: (a) `seat_directive_add_records_issued_event` / (b) `seat_rebrief_marks_a_directive_for_review_after_the_threshold` / (c) `seat_rebrief_lists_live_runs_and_seats` / (d) `hook_prompt_expansion_injects_rebrief_data` + `hook_pre_compact_auto_writes_a_minimal_wm` / (e) `seat_externalize_writes_under_the_state_dir` / (f) `seat_launch_defaults_model_and_effort_from_the_role_rule` / (g) `hook_brief_planner`（snapshot + 4 行を名指す歯 1 本）。

### 12.8 却下案（設計固有・ADR-0031 §5 の写しは持たない）

- 直命の表を台帳（bead）で持つ: 台帳は task と裁定（C15）で、発言の逐語と前提 pointer の typed な列を持たない。event log は既に承認 event で逐語を持つ（C7.2）＝同じ側に置く。
- `[WIN]` を git log から取る: 便の Landed は event log が持ち、commit の subject の grep は偽陽性（bead id の `.` が regex）。event log だけを読む。
- PreCompact(auto) の退避物に AI の文を求める: hook の中で開発 session は動かない。器が持つ事実（現在地 + carry）だけで書き、`trigger: auto` で弁別する。

## 13. 退避の supersede — 同 sid の未 consumed を置き換える（契約表の行 d・`s2-07l.289`）

- 何が起きているか: admin 2026-09-14 19:05Z の退避 → cycle が input-busy で 40 分 back-off し、席は仕事を続けたが 2 通目の退避が `wm-exists` で断られ、差分を planner に散文で預けた（退避の一次面が席の外へ漏れる）。現物（verified）: `crates/scribe2/src/seat/externalize.rs` は `WmScan::Unconsumed(_)` を一律 `WmExists` にする（§5 手順 1「二重退避を取り合わない」）。consume は `working-memory.<sid>.consumed.md` へ move する形（`crates/scribe2/src/seat/consume.rs`）。
- 形: `externalize` が `Unconsumed` の退避物を見た周、その file の frontmatter の sid が**現在の sid と同じ**なら `working-memory.<sid>.superseded.<ts>.md` へ rename（write → rename の順・部分書きを残さない）してから新 file を書く。別 sid（`/clear` 後の候補・別席）は従来どおり `wm-exists`。記録の 1 行に `superseded=<旧 file>` を添える。`rebrief` / `consume` は superseded を走査から外す（`[WM]` の候補にしない）。
- 触らない: carry-forward・命令行の cap・consume の move。
- 却下案: 2 通目を consume → 書き直しで通す（「同 sid の consumed が在る」で断られる・consume は復元の合図であって更新ではない）／旧 file を上書き（不可逆・N1）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "直命の表 — EventKind の variant 2（DirectiveIssued / DirectiveClosed）と seat directive add / close / ls の 3 口・fleet record は直命の kind を断る"
req = ["FR65"]
section = "12"
touches = ["crate::fleet::EventKind", "crate::fleet::event::Event"]
write-set = ["+crates/scribe2/src/seat/directive.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/seat/state.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/pipe/lifecycle.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_directive_", "cargo nextest run -p scribe2 --no-tests=fail fleet_record_refuses_directive_"]
size = "M"
done = "seat directive add が DirectiveIssued を逐語で 1 行記し、close が閉じて未知と二重を typed に断り、ls が有効な直命を件数付きで列挙し、fleet record は直命の kind を断る"

[[contract]]
id = "c"
title = "現在地の DATA — seat rebrief が [MAIN][RUN][SEAT][WIN] を自分の記録と git から出す"
req = ["FR23"]
section = "12"
touches = ["crate::seat::rebrief::Marker"]
creates = ["crates/scribe2/src/seat/rebrief/status.rs"]
also = ["crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_rebrief_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_wm_rebrief_"]
size = "S"
done = "偽の event log と state.jsonl から [RUN][SEAT][WIN] が件数付きで出て、git の無い anchor では [MAIN] が unknown で rc 0"

[[contract]]
id = "d"
title = "自席・同 sid の未 consumed 退避物は superseded へ rename して置き換える — 別 sid は従来どおり wm-exists"
req = ["FR38", "FR23"]
section = "13"
write-set = ["crates/scribe2/src/seat/externalize.rs", "crates/scribe2/src/seat/rebrief.rs", "crates/scribe2/tests/e2e/seat/wm.rs", "docs/design/working-memory.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_wm_externalize_supersede_"]
size = "S"
done = "同 sid の退避が置き換えられ、旧版は superseded として残る"
<!-- contracts:end -->
