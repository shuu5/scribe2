# 設計: 案件の局面 — 部品ごとの局面・手番・理由・結びを器の関数で導き、state dir ごとに 1 つの局面の出力へ書く

- 要件: [FR90](../../design-intent/spec/srs.html#FR90) 局面の出力 / [FR91](../../design-intent/spec/srs.html#FR91) memo の局面 / [FR92](../../design-intent/spec/srs.html#FR92) 器の便でない着地の commit / [FR93](../../design-intent/spec/srs.html#FR93) memo の自動の close / [FR94](../../design-intent/spec/srs.html#FR94) 局面の出力の読み手 / [FR87](../../design-intent/spec/srs.html#FR87) memo の判定 / [NFR5](../../design-intent/spec/srs.html#NFR5) hook の予算。受入: AC60・AC61。
- 決定: [ADR-0088](../../design-intent/decisions/ADR-0088-case-positions-are-computed-once-by-the-vessel-and-read-from-one-file.html)（1 つの関数・出力の組・書き直しの契機と古さの印・切り替えの線）/ [ADR-0089](../../design-intent/decisions/ADR-0089-open-memos-sit-in-four-positions-and-close-by-three-reasons.html)（memo の 4 局面と閉じ方）/ [ADR-0097](../../design-intent/decisions/ADR-0097-the-close-gate-binds-declared-repos-and-seats-settle-named-terminals.html)（close-check）/ [ADR-0100](../../design-intent/decisions/ADR-0100-the-close-check-line-rides-the-cutover-event-in-its-detail.html)（close-check の線の記帳の形）。
- 裁定: rules 行 `lifecycle.closed_window_h` と `lifecycle.age_h.<語>` の値は user 2026-09-30T04:25Z。
- この設計から出る契約: §6〜§14 の 9 行と、§16 の行 f・§17 の行 g（11 行）。§1〜§5 は語と欄の正本で、§15 は読み手の行が執行する表。

## 1. 何を解くか

やさしく言うと: 器は案件（発話・問い・memo・契約・便・行・要件・epic・着地の commit）を 1 つずつ「今どの段にいて、誰の番で、なぜそこにいるか」に振り分けて、1 つの file に書く。今は判定が 4 か所に散り、どこにも出ない段がある。この doc は、振り分けの語と表（§2〜§4）・file の形（§5）・作る順の 9 行（§6〜§14）を決める。

- 何が起きているか（main b028af03・verified）:
  - 局面の出力の実装は無い。`lifecycle.json` と `lifecycle.stale` の字は src に 0 件。event の kind（`UtteranceReceived`・`UtteranceSorted`・`IntakeRefused`・`LifecycleCutover` ほか）は読み書きの両側に在るが、`LifecycleCutover` の書き手は 0 件。
  - 判定が 4 か所に散っている: 列の通知の語（`crates/scribe2/src/pipe/cli.rs` の `alarm_word`）・doctor の台帳の lint の (v)(vi)(vii)(viii)・管理 tick の alarm・消費側の面の自前の計算。
  - 局面の関数が使う読み手は在る:
    - 引き金の読み（`crates/scribe2/src/ledger/trigger.rs` の `read`・`Reading`）と close の理由の読み（`crates/scribe2/src/ledger/close_reason.rs` の `read`）
    - 種類の弁別（`crates/scribe2/src/ledger/form.rs` の `is_memo`・`is_question`・`pointer_text`・doctor の 4 象限の `judge`）
    - 台帳の読み（`crates/scribe2/src/seat/ledger.rs` の `issues_of` と `Issue`）と契約表の pointer の読み（`crates/scribe2/src/pipe/table/parse.rs` の `parse_pointer`・`.md` と `.toml` の 2 形）
    - 便の段（`crates/scribe2/src/fleet/mod.rs` の `Stage`・11 値・`STAGES`）
    - 待ちの理由（`crates/scribe2/src/pipe/dispatch.rs` の `WaitReason`・`WAIT_REASONS` の 8 語）
    - 終端の語（`crates/scribe2/src/pipe/land.rs` の `TERMINAL_TOKENS`・7 形）と発端の trailer の key（`crates/scribe2/src/pipe/land/finish.rs` の `source_key`）
    - 時刻（`crates/scribe2/src/fleet/cli.rs` の `format_utc`・`crates/scribe2/src/fleet/wait.rs` の `epoch_of`）と digest（`crates/scribe2/src/hook/vessel/digest.rs` の `fnv1a_64`）
    - 入れ子の JSON の読み書き（`crates/scribe2/src/fleet/json_tree.rs` の `parse` と `render`）
  - 抜けているもの: 局面の型・手番の表・引き金が満ちたかの判定（FR87 と共用・今は読むだけ）・昇格の行の読み手・台帳の時刻（`Issue` は bd の `created_at`・`closed_at` を読まない）。
- 本 doc の役: 局面の語と優先の順（§2）・手番（§3）・misfit の理由の語（§4）・出力の欄（§5）の正本。memo の局面の語と順だけは SRS FR91 が持ち、§2 はそれを写す。
- 関数の向き: 局面の関数は起動の列の 1 周の判定の結果を入力に受け、列は局面の関数を呼ばない（ADR-0088 (1)）。
- 導く関数は純関数である。I/O（台帳・event log・git・宣言・rules・前の出力・札の生死）は書き手（§12・行 c）が集めて渡す。
- module の置き方（9 行に共通）: 各行は自分の `+` の file を、既存の親 module（`crates/scribe2/src/lib.rs`・`crates/scribe2/src/ledger/mod.rs`・`crates/scribe2/src/fleet/mod.rs`・`crates/scribe2/src/hook/mod.rs`）に兄弟と同じ `pub mod` の 1 行で宣言する。後の行は親を触らずに crate の path で呼ぶだけで、私的な module に届くために write-set の外の親を直す形を作らない。
- 歯の置き方（9 行に共通・flip-check が base の木へ写せる形）:
  - 新しい `+` の file の歯は、その file の末尾の `#[cfg(test)] mod tests` に置く（file の頭に置かない）。そのうえで、行の歯を少なくとも 1 本、base に在る file の test 区間に置く。
  - 理由: flip-check は、base に無く名が歯の file でない src の file を base の木へ写さない（`crates/xtask/src/flipcheck.rs` の `overlay`）。新しい file の中の歯だけの行は `not-flippable` で落ち、新しい親が末尾で宣言する兄弟の歯の file は、宣言ごと base に写らず `green-on-base` と判じられる（使い捨ての workspace で現物の xtask を撃って確かめた・2026-09-30）。
  - 兄弟の歯の file（名が `_tests.rs` で終わる新しい file）は使わない。親の test 区間に足す `#[cfg(test)]` 付きの `mod` 宣言は、flip-check が宣言の file を本体の木へ同梱する形（test 区間の差の行が全部 `mod x;`）を `#[cfg(test)]` の行で外れる。兄弟の file は宣言ごと base に写らず、単独で撃たれて `green-on-base` になる（行 b1 の便が 2026-09-30 に gate の flip-check で落ちた）。
  - 既存の file の歯が 1 本在れば通るのは、新しい `+` の src file の中の歯（`not-flippable` の型）だけである。e2e の file と base に在る file の test 区間は、file ごとに単独に撃たれ、base で緑の歯が 1 本でも在れば落ちる。だから歯を置く file ごとに、その file の歯だけで base で RED になる理由（新しい関数・欄・型を呼ぶので base で compile できない、または期待が base の振る舞いと違う）が立つ。否定だけを測る歯（「進まない」「付かない」）は、同じ歯の中で肯定と組にする。本文だけ直して base で緑のままの既存の歯には retroactive の札を付ける。
  - 行ごとの置き場と、file ごとの base で RED の理由は、各 § の歯の項が名指す。
  - 歯の名に `contract_` の字を含めない。[contract-source.md](./contract-source.md) の行 a の verify の filter `contract_` は名の途中にも当たり、当たった歯の file が行 a の write-set の外と数えられて `contracts check` が findings 1 になる（行 b1 の便の歯 `phase_main_row_lands_through_the_contract_trailer` が 2026-10-01 に gate の共通の verify で落ちた）。契約は `row_` か `pointer_` の語で書く。

## 2. 局面の語と優先の順（閉じた 38 語・宣言順＝優先の順）

部品の種類は閉じた 9 つ（`utterance`・`question`・`memo`・`contract`・`run`・`row`・`requirement`・`epic`・`commit`）。語は ASCII の小文字と `-` で、種類の接頭辞を持ち、種類をまたいで一意（`misfit` を除く）。1 つの部品は宣言順で最初に当たる語を取り、どれにも当たらなければ `misfit`（§4）。「終わり」の語にだけ窓（rules 行 `lifecycle.closed_window_h`）が掛かる。

- bead の種類の判定（この順）: 問いの label か decision の型 → question ／ memo の label → memo ／ epic の型 → epic ／ それ以外 → contract。doctor の 4 象限（`judge`）と列の候補（memo と問いを外す）と同じ弁別に揃える。
- 部品に載せる bead: 閉じていない全部と、窓の内に閉じたもの。閉じの misfit（§4）は窓を掛けずに残す。

| 種類 | 語（宣言順） | 終わり | 当たる条件（要約） |
|---|---|---|---|
| utterance | utterance-open ／ utterance-sorted | sorted | 仕分けの event か bind の結びが無い ／ 在る |
| question | question-open ／ ruling-unreflected ／ question-closed | closed | open ／ 閉じて未反映の置き場（FR84）に在る ／ 閉じた |
| memo | memo-promoting ／ memo-asking ／ memo-actionable ／ memo-waiting ／ memo-closed | closed | FR91 の 4 局面（順は FR91）／ 閉じた |
| contract | contract-running ／ contract-refused ／ contract-queued ／ contract-closed | closed | 運転手の札の生きた便が在る ／ 列の理由が admission か便の後に起きていない受付の断り ／ 列のほかの理由 ／ 閉じた |
| run | run-intake ／ run-reviewed ／ run-review-failed ／ run-blocked ／ run-implementing ／ run-asking ／ run-rate-limited ／ run-gating ／ run-landing ／ run-gate-failed ／ run-ci-waiting ／ run-landed-open ／ run-stopped ／ run-failed | — | §2.1（開いた契約の最新の便だけ） |
| row | row-unbeaded ／ row-beaded ／ row-landed | landed | pointer の bead が台帳にも着地の trailer にも無い ／ bead が開いている ／ 着地した |
| requirement | requirement-unrowed ／ requirement-rowed | — | どの行の req にも無い ／ 在る |
| epic | epic-closable ／ epic-open ／ epic-closed | closed | 開いて子が全部閉じた ／ 開いた ／ 閉じた |
| commit | commit-landed | landed | 切り替えの線より後の、器の便でない着地の commit |
| （全部） | misfit | — | §4 |

### 2.1 便の段の写し（`Stage` の 11 値を漏れなく写す）

| 段 | 条件 | 語 | 理由 |
|---|---|---|---|
| Intake | — | run-intake | `Intake` |
| Reviewed | 審査の判定が PASS ／ PASS でない | run-reviewed ／ run-review-failed | `Reviewed` ／ 判定の語 |
| Blocked | — | run-blocked | `Blocked` |
| Spawned | — | run-implementing | `Spawned` |
| Questioned | — | run-asking | `Questioned` |
| RateLimited | — | run-rate-limited | `RateLimited` |
| Implemented | — | run-gating | `Implemented` |
| Gated | 判定が PASS ／ PASS でない | run-landing ／ run-gate-failed | `Gated` ／ 判定の語 |
| Landed | 運転手の札が生きている | run-ci-waiting | 最新の終端の語か `Landed` |
| Landed | 札が無いか死んだ | run-landed-open | 最新の終端の語（`TERMINAL_TOKENS` の 7 形の 1 つ） |
| Stopped | — | run-stopped | `Stopped` |
| Failed | — | run-failed | 最後の detail の頭（例 `rebase-conflict`） |

- 終端が closed / closed:no-ci で台帳が閉じた契約の便は「開いた契約の最新の便」でないので、部品に載らない。

## 3. 手番（閉じた 6 値 user・seat・vessel・runner・ci・none）

| 語 | 手番 | 注 |
|---|---|---|
| utterance-open | seat | 仕分けか bind を待つ |
| utterance-sorted・question-closed・memo-closed・contract-closed・row-beaded・row-landed・requirement-rowed・epic-open・epic-closed・commit-landed | none | |
| question-open | user | |
| ruling-unreflected | seat | FR84 |
| memo-promoting | none か vessel | 理由 `contract-open` は none（契約と便の部品が手番を持つ）・`close-due`（FR93 の自動の close を待つ）は vessel |
| memo-asking | user | |
| memo-actionable | seat | |
| memo-waiting | none | 引き金は器が毎周に判じる（誰の手も待たない） |
| contract-running | none | 理由＝最新の便の局面の語（例 `run-gating`）。手番は便の部品が持つ（二重に数えない） |
| contract-refused | seat | 理由＝受付の断りの名 |
| contract-queued | 理由の表（下） | |
| run-intake・run-reviewed・run-rate-limited・run-gating・run-landing | vessel | |
| run-implementing | runner | |
| run-blocked | user | 3 クラスの承認 |
| run-asking | seat | 問いへの答えは席の権能 |
| run-ci-waiting | ci | |
| run-review-failed・run-gate-failed・run-landed-open・run-stopped・run-failed | seat | |
| row-unbeaded・epic-closable・misfit | seat | |
| requirement-unrowed | seat | 閾値越えに数えない（FR90） |

contract-queued の理由から手番（語の集合が `WAIT_REASONS` を含むことを歯で測る。dispatcher の後の行が足す語 unreflected-ruling・floor と、[row-review.md](./row-review.md) の行 f・g が足す語 reserved・sibling は先に置く）:

| 理由 | 手番 |
|---|---|
| dependency・overlap・host-busy・launched | vessel |
| hold・no-design-pointer・unreflected-ruling・floor・reserved・sibling | seat |
| settled | none（最新の便の部品が手番を持つ） |

- `admission` は contract-refused に当たり、queued の部品にはならない。ただし表は `WAIT_REASONS` の 8 語を全部覆う（語が増えたら表の歯が落ちる）ので、admission を contract-refused と同じ手番 seat で持つ（表は 10 語・row-review.md の行 f と g が 1 語ずつ足して 12 語）。表に無い語は misfit `no-phase` へ倒す（fail-closed）。

## 4. misfit の理由の語（閉じた 15 語）

| 語 | 種類 | 線 | 出所 | 行 |
|---|---|---|---|---|
| no-phase | 全部 | 依らない | どの語にも当たらない | a1・b |
| form-both | memo | 依らない | memo の label と設計 pointer の両方を持つ（doctor の 4 象限の both） | a1 |
| form-neither | contract | 依らない | 開いて、問い・decision・memo・epic でなく、設計 pointer を持たない（同 neither） | a1 |
| memo-no-trigger | memo | 依らない | 読める引き金の行が 0 で promoting にも asking にも当たらない（FR91） | a1 |
| close-kind-mismatch | 閉じた bead | 掛かる | 種類と理由の頭の食い違い・頭が読めない（空・9 頭の外） | a1 |
| close-unresolved | 閉じた bead | 掛かる | 理由の bead id（重複・後継・まとめた・昇格済み）が形に合わないか台帳に無い | a1 |
| merged-into-not-open | 閉じた memo | 掛かる | まとめ先が開いた memo でない | a1 |
| promoted-unmet | 閉じた memo | 掛かる | 閉じた時点で FR93 の条件（処置の無い判定を除く）を満たさない | a1 |
| promoted-list-mismatch | 閉じた memo | 掛かる | 理由の契約 id の列が最後の昇格の行の列と違う | a1 |
| close-ruling-unresolved | 閉じた問い / memo | 掛かる | 裁定・見送りの 1 語目の値が FR83 の解け方で解けない | a2 |
| close-ruling-not-bound | 閉じた問い | 掛かる | 自分の裁定の行にも裁定 event にも無い裁定 id | a2 |
| deferred-not-child-ruling | 閉じた memo | 掛かる | 見送りの裁定が子の問いの裁定でない | a2 |
| source-unresolved | commit | 掛かる | 発端の id の 1 本でも台帳に無い（FR92） | b1 |
| run-trailer-unknown | commit | 掛かる | `run:` の便が event log に無い（FR92） | b1 |
| commit-no-trailer | commit | 掛かる | 発端の trailer も器の便の trailer も無い（FR92） | b1 |

- 判じる順: 形の misfit（form-both・form-neither・memo-no-trigger）は種類の局面より先。閉じの misfit は `*-closed` より先。`no-phase` は最後。
- 「線が掛かる」語:
  - 切り替えの線より後の記録だけに掛ける。
  - 閉じに由来する語は、さらに main の先端の宣言が close-check を true で持つ repo の、close-check の線より後の閉じだけに掛ける（FR90）。
  - 線より前の同じ閉じは判じず、その種類の終わりの語（`*-closed`）に置く。
- 裁定と見送りの理由は 1 語目の値だけを読み、後ろの語は読まない（close の理由の読み `read` の今の振る舞い）。`裁定 <id> 束 batch:<字>` は `<id>` で解ける。
- 1 語目に字が続く古い形（`裁定 <id>・束 …`）は値が裁定 id の形でない。線より前の閉じは判じず、線より後なら close-ruling-unresolved の 1 語で数える（行 a2）。語を足さず、閉じた頭の外の形を黙って読み飛ばさない。
- 台帳の接頭辞が解けない周は、bead id と裁定 id を値に持つ形を判じず、`unmeasured` に `ledger-prefix` を名指す（§5.2）。

## 5. 出力の欄（lifecycle.json と lifecycle.stale）

### 5.1 ADR-0088 が決めた範囲（本 doc は写すだけ）

- 置き場は `<state_dir>/fleet/lifecycle.json` と、同じ dir の `lifecycle.stale` の組。state dir ごとに 1 つ。
- どちらも版の欄を持つ。版は語と欄を足すだけでは上げない。
- 本体が持つもの: 語の一覧・入力の印（台帳の印・event log の長さ・main の sha）・生成の時刻・席の手番の閾値越えの件数と最古の 1 件・部品ごとの局面／手番／since／理由／結び。
- 書き方: 一時 file からの rename で丸ごと入れ替える・書き手は lock で 1 つずつ・入力の印が今の file より古い書きは捨てる。
- 古さの印の種類は 3 つ（起票の門・merge の門・読めない周）。種類ごとに 1 つまで持つ。
- 発話の部品は ts・session・行き先だけを持ち、逐語を持たない。
- 読み手の約束: 知らない語と欄は「まだ分からない」、無いか読めない出力は「測れない」と描く。
- 出力は線（切り替えの線・close-check の線）を持たない。線は event log に在る（記帳の形は ADR-0100）。

### 5.2 本 doc が決める欄（跨版の面・key は英小文字の 1 語か snake で安定）

```
lifecycle.json
{ "version": 1,
  "generated_at": "YYYY-MM-DDTHH:MM:SSZ",
  "scope": "full" | "partial",
  "full_at": "<時刻>",                     // 台帳と main から判じた部分を作った全部の書き直しの時刻
  "interval_s": <整数> | null,              // 部分の書き直しの周期の約束（管理 tick の周期の写し・無ければ null）
  "closed_window_h": <整数> | null,         // rules 行 lifecycle.closed_window_h の写し
  "inputs": { "ledger": { "form": "noms", "root": "<字>", "gen": "<16 字の 16 進>", "chunks": <整数> }
                      | { "form": "files", "len": <整数>, "mtime_ns": <整数> },
              "events": { "len": <byte 長>, "head": "<1 行目の ts>" | null },
              "main":   { "ref": "refs/remotes/origin/main", "sha": "<40 桁の小文字の 16 進>" } },
  "unmeasured": [ { "part": "<種類>", "reason": "<語>" } ],
  "phases": ["utterance-open", …, "misfit"],        // §2 の 38 語を宣言順に
  "owned": { "count": <n>, "unset": <n>, "unknown": <n>,
             "oldest": { "part": "<種類>", "id": "<id>", "phase": "<語>", "since": "<時刻>" } | null },
  "parts": [
    { "part": "<種類>", "id": "<id>", "phase": "<語>", "turn": "<6 値>",
      "since": "<時刻>" | null, "reason": "<語>" | null, "closed": <真偽>, "overdue": <真偽> | null,
      "links": { … },
      … 種類ごとの欄 } ] }

lifecycle.stale
{ "version": 1,
  "marks": [ { "kind": "ledger-gate" | "merge-gate" | "unreadable", "at": "<時刻>",
               "inputs": { "ledger": {…} } | { "main": {…} } | null,
               "reason": "<語>" | null } ] }
```

- 部品の共通の欄は全部が必須（値の無いものは null）。`links` の中の空の key だけは省き、読み手は欠けた key を空の列と読む。
- 時刻は UTC の秒まで（`2026-09-30T04:25:39Z`）。utterance の id だけは発話の ts の字のまま（秒より下の桁を持つ）。
- 部品の id の形:
  - question・memo・contract・epic は bead id。run は run id。commit は 40 桁の sha。
  - row は便の `--design` の pointer の字のまま（`.md#<行 id>` と `.toml#<行 id>` の 2 形・`parse_pointer` が受ける字）。
  - requirement は要件面の id（`FR<n>`・`NFR<n>`）。
- `closed`: bead の部品は台帳で閉じたか。run と commit は閉じた契約に結ばれたか（commit は契約の trailer が台帳で閉じた bead を名指すか）。row は row-landed だけ真。utterance と requirement は常に偽。
- `links` の 8 key:
  - `source`＝発端・`questions`＝子の問い・`rulings`＝問いに結んだ裁定 id・`promoted`＝昇格した契約・`runs`＝便・`commits`＝着地の commit
  - `destination`＝仕分け済みの発話の行き先（`[{"to":"memo"|"ruling"|"chat","id":"<id>"|null}]`・字の形から推さない）
  - `on`＝契約の待ちの理由が dependency・overlap・reserved のときの相手の bead id（reserved は行を予約した bead・§18）
  - 結びの先が部品として載っているとは限らない（窓の外・裁定 id）。読み手は無いことを誤りと読まない。
- 種類ごとの欄:

| 種類 | 欄 | 値 |
|---|---|---|
| utterance | `session` ／ `channel` | 字か null ／ `"chat"` か `"gui"` |
| memo | `due` | 満ちていない期日の最も早い値か null |
| memo | `triggers`（任意） | `[{"form":"<引き金の形の語>","value":"<値の字>","met":<真偽>}]`（FR87 と同じ判定の写し） |
| memo | `keep`（任意） | keep の記帳が在るか |
| contract | `pointer`（任意） | 設計 pointer の字か null |
| run | `bead` | 便が属する契約の bead id |

- `since` の決め方:
  - 導ける部品は入力から導く（閉じた部品は `closed_at`・期日の満ちは期日・便は最新の段の event の ts）。
  - 導けない部品は、前の出力に同じ (part, id, phase) が在ればその値を継ぐ。無ければ、前の出力が在るときはこの書きの `generated_at`（遅くともこの時刻・年齢は短く見える側にしか倒れない）、前の出力が無いか読めないときは null。
  - 継ぎは書き手（行 c）が行い、純関数は `since` を `Option` で返す。
- `overdue` と `owned`:
  - 手番が seat で `lifecycle.age_h.<語>` の行を持ち、since が在る部品は、年齢（`generated_at` − `since`）が値を越えれば true、越えなければ false。ほかは null。
  - `owned.count`＝true の件数・`unset`＝手番が seat で行の無い部品の件数・`unknown`＝行が在って since が null の件数・`oldest`＝true の最古。requirement はどれにも数えない。
- `unmeasured` の reason の閉じた語: `srs-unreadable`・`table-unreadable`・`ledger-prefix`・`multi-anchor`・unreflected-unreadable（§17）。空の列なら 9 種とも測れた。出力に大きさの上限は置かず、切り詰めない。
  - `srs-unreadable` と `table-unreadable` は、main の SRS か契約表が無いか器の読める形でない repo（消費側の形の違いを含む）で、書き直しは続ける。読みそのものが落ちた周（git が撃てない・file を読めない）は §12 の読めない周で、書き直さない。
- 書き手の約束（行 c・d・e が執行・読み手と組にする）:
  - 一時 file は頭が `.` の名（`.lifecycle.json.<pid>.tmp`・`.lifecycle.stale.<pid>.tmp`）、lock は `lifecycle.lock` と `lifecycle.stale.lock`。読み手は `lifecycle.json` と `lifecycle.stale` の 2 つの名だけを見る。
  - 全部の書き直しは json を rename してから stale の消す印を消す。読み手は stale → json の順に読む（途中の状態は「新しいのに古いと出る」側にしか倒れない）。
  - 最初の全部の書き直しで `{"version":1,"marks":[]}` を作り、以後は消さない（無いのと 0 件を読み手が見分ける）。
  - `generated_at` のほかに何も変わらない書きは rename しない。

### 5.3 入力の印の組み方と順（読み手が stat と小さい読みだけで同じ値を組める形）

- `ledger` の `noms`（`<ledger_dir>/metadata.json` の `dolt_mode` が `embedded` のとき）:
  - 読むのは `dolt_database` の db の `<ledger_dir>/embeddeddolt/<db>/.dolt/noms/manifest` の 1 file だけ（bd も git も撃たない）。manifest は `:` で割った 1 行で、5 つ目が root、6 つ目が gc の世代、7 つ目から先が「file 名:chunk 数」の組の列。
  - `root` は manifest の root の字。`gen` は gc の世代の字と、journal（名が全部 `v` の file）を除く file 名の列を改行でつないだ bytes の `fnv1a_64`。`chunks` は chunk 数の和。
  - 順: gen が同じなら chunks の大小（同じ chunks で root が違う組は順を持たない）。gen が違う組は順を持たない。
  - 実測（bd 1.1.0・使い捨ての台帳・2026-09-30）: `bd --readonly list`・`bd --readonly show`・`bd list` は root・gen・chunks を動かさず、manifest の更新時刻だけを動かした（同じ字で置き替える）。create・`update --append-notes`・close は root を替え chunks を増やした（1858 → 1897 → 1928 → 1966 → 1992）。`--readonly` の無い `bd ready` も root を替え chunks を増やした。gc の無い間 gen は動かなかった。更新時刻は使わない。
- `ledger` の `files`: store が無い台帳。`<ledger_dir>/issues.jsonl` の byte 長と更新時刻（ns）。順は長さの大小、同じ長さは更新時刻の大小。
- `events`: `<state_dir>/fleet/events.jsonl` の byte 長と 1 行目の `ts`（畳みで長さが縮んでも別の log と見分ける）。順: head が同じなら len の大小。head が違う組は順を持たない。
- `main`: 器の land が基にする anchor の `refs/remotes/origin/main`（字は `crates/scribe2/src/pipe/queue.rs` の私有の const `ORIGIN_MAIN_REF` と同じだが、私有で使えないので、行 c の印の読み手の `+` の file が自前の const で持つ・読むだけで fetch しない）。loose の ref の file を先に、無ければ packed-refs の行を読む。worktree は `.git` の file が指す common dir を読む。順は git の祖先の関係で、判じるのは全部の書き直しだけ。
- 「順を持たない」組は、捨てる判定と Coalesced（§12）では「古くない」と読み（書く側へ倒す）、印を消す判定（§12 約束 6）では gen か head が違えば「新しい」と読む。

### 5.4 ADR と委任

- §5.2 の欄の名と JSON の形は、ADR-0088 が「設計 doc が持ち、下書きを消費側の席に先に見せる」と委ねた範囲である。新しい ADR は要らない。
- close-check の線の記帳の形は ADR-0100 が決めた: `LifecycleCutover` の 1 件に、既存の任意の key detail の閉じた 1 語 close-check を持たせる。切り替えの線は detail を持たない最初の行、close-check の線は detail が close-check の最初の行。新しい key も kind も足さない（event の 1 行の読み `from_line` は `KNOWN_KEYS` の外の key の行を断り、event log の読みは 1 行でも読めなければ全部を Err にするので、新しい key では旧い版の器が log の全部を読めなくなる）。

## 6. 局面の型と手番の表と共用の読み手（行 a）

やさしく言うと: §2〜§4 の語を code の型にし、「この語なら誰の番か」の表と、memo の引き金が満ちたかの判定と、昇格の行の読み手を置く。後の 8 行は全部これを使う。

- 何が起きているか（verified）:
  - 局面・種類・手番の型は src に 0 件。§2〜§4 の語の正本を code が持たない。
  - `crates/scribe2/src/ledger/trigger.rs` は 5 形を読むだけで、満ちたかの判定を持たない（FR87 と FR91 の両方が要る）。
  - `昇格:` の行を読む code は 0 件。
  - `crates/scribe2/src/seat/ledger.rs` の `Issue` は 10 欄で時刻を読まない。`bd list --json` の要素は `created_at` と、閉じた bead は `closed_at` を持つ（bd 1.1.0）。`Issue` を字で組む所は 4 か所（`issues_of` と、歯の fixture の 3 つ: `crates/scribe2/src/pipe/dispatch/precheck.rs`・`crates/scribe2/src/ledger/form.rs`・`crates/scribe2/src/hook/graph_guard.rs`）。dispatcher の後の行も同じ 4 か所に欄を足すので、列の overlap で直列になる。
- 約束（番号は done と 1:1）:
  1. 行 a の write-set の `+` の file（`crates/scribe2/src/case/mod.rs`）が §2 の 38 語（宣言順の const の列と字の関数）・9 つの種類・6 つの手番・§4 の misfit の 15 語（行 a1・a2・b1 が出す語も先に全部置く）・部品の型（§5.2 の共通の欄と種類ごとの欄）を持つ。部品の型は、§5.2 の共通の欄の 9 key（part・id・phase・turn・since・reason・closed・overdue・links）と `links` の 8 key（source・questions・rulings・promoted・runs・commits・destination・on）の字を、宣言順の const の列で持つ（書き手の行 c はこの列で key を書く）。語と key の字は §2〜§5 の表と 1 字も違わない。`crates/scribe2/src/lib.rs` に `pub mod case;` を 1 行足し、歯の module は file の末尾で宣言する。
  2. 語から手番を返す 1 関数が §3 の表を網羅の match で持つ。contract-queued は理由の字の表で引き、表の語の集合は `WAIT_REASONS` の 8 語と unreflected-ruling・floor を含む。表に無い語は None を返し、呼び手は misfit `no-phase` に置く。
  3. `crates/scribe2/src/ledger/trigger.rs` に純関数 met を足す。入力は引き金 1 つと「世界」（notes の再発の行の本数・開いた契約の write-set の項目・閉じた bead id の集合・閉じた bead の設計 pointer の集合・周の時刻）。満ちの規則:
     - 再発: 本数が値以上。同梱: 印（`+` `-` `=`）を外した項目が値の path と等しいか、値が `/` で終わって項目がそれで始まる。
     - 依存: 値の bead が閉じた。期日: 周の時刻が値以後。着地: 値の pointer を持つ bead が 1 本以上閉じた。
     - FR87 の判定（dispatcher の後の行）と、部分の書き直しの期日の移り（行 d）は同じ関数を使い、写しを持たない。
  4. 行 a の write-set の `+` の file（`crates/scribe2/src/ledger/promotion.rs`）が memo の notes の行頭 `昇格:` の行を出てきた順に読む。形は `昇格: 全部 <契約 id の列>` と `昇格: 一部 <契約 id の列>`（id は空白で割り、`,` は区切りでない・1 本以上・同じ台帳の bead id の形）。読めない行は字と理由（語の欠け・全部 / 一部の外・id の形）を持つ。判じるのは最後の行（ADR-0089）。
  5. `Issue` に `created_at` と `closed_at`（どちらも字の `Option`）を足し、`issues_of` が読む。無い要素は None（ほかの欄の読みは変えない）。4 か所の組みを直す（構築点は便の始めに今の main で数え直す）。
- 閉包: 新しい file は `Issue` を字で組まず、`WaitReason`・`Stage`・`EventKind` の変種を名指さない（それらを touches に持つ行の閉包を広げない）。歯の fixture の `Issue` は bd の JSON の字を `issues_of` で読んで作る。
- 歯（接頭辞・母集団・base で RED の理由）:
  - `phase_table_`（9 本・case の `+` の file の末尾の歯の区間）: (a) 38 語が ASCII の小文字と `-`・一意で、§2 の表で自分の種類の名を頭に持たない 2 語（question の `ruling-unreflected`・全部の種類に共通の `misfit`）を除く 36 語が `<種類>-` を頭に持つ。除く 2 語は歯に字で写す (b) 38 語の列が §2 の表の字と宣言順に 1 字も違わない（期待の列を歯に写す） (c) 手番の 6 語 (d) contract-queued の表が `WAIT_REASONS` を全部と unreflected-ruling・floor を含む (e) 表に無い語は None (f) misfit の 15 語の字 (g) 部品の共通の欄の 9 key と `links` の 8 key の const の列が、§5.2 の字と順に 1 字も違わない (h) 9 つの種類の語の字と順 (i) 語から手番の関数が §3 の表の全行と一致する（38 語の各語、memo-promoting の理由 2 つ〔contract-open・close-due〕、contract-queued の理由 10 語の各手番を期待の表として歯に写す・全部を 1 つの手番に倒す実装を落とす）。
  - `promotion_line_`（7 本・昇格の行の読み手の `+` の file の末尾の歯の区間）: 全部と一部／2 行で最後が勝つ／最後の行が読めず前の行が読める notes は読めない（前の行へ倒れない）／読めない 3 形のそれぞれの理由の語と行の字／行頭でない `昇格:` は読まない／`,` を区切りと読まない。
  - `trigger_met_`（8 本・trigger.rs の歯の区間）: 5 形の満ちと満ちない各 1 組（再発は本数＝値で満ち・値−1 で満ちない、期日は周の時刻＝値で満ち・1 秒前で満ちない）／同梱の dir の前方一致と印の外し／値が `/` で終わらない同梱は前方一致で満ちない（値の字が項目の字の頭と一致するだけで、等しくない組）。
  - `issue_times_`（2 本・seat/ledger.rs の歯の区間）: 時刻の 2 欄を読む／無い要素は None でほかの欄は同じ。
  - 置き場と file ごとの base で RED の理由（§1）: `trigger_met_` は trigger.rs の test 区間（新しい `met` を呼ぶので base で compile できない）、`issue_times_` は seat/ledger.rs の test 区間（`Issue` の新しい 2 欄を読むので同じ）。組みを直す 3 file（precheck.rs・form.rs・graph_guard.rs）の歯の区間も `Issue` の新しい欄を書くので base で compile できない。
  - base で RED: 4 つとも歯の名が base に 0 本（rc 4・機能不在）。既存の `ledger_trigger_`・`seat_ledger_`・`precheck_intake_` は期待を変えずに緑（組みの直しの非回帰）。
- 触らない: 引き金の読み（`read` と `Reading`）・close の理由の読み・列の判定・event の kind・rules 行。
- 限界: keep の記帳と引き金の結び（どの満ちに対する keep か）は FR87 の行が決める。行 a1 は keep の行が 1 本以上在れば満ちた引き金を keep 済みと読む。
- 却下: 語を `WaitReason` や `Stage` の enum から生成する形（語の正本は本 doc・入力の enum が増えたら表の歯が落ちる形の方が判じ手 1 本に合う）。

## 7. 台帳の側の部品と閉じの misfit（行 a1）

やさしく言うと: 台帳だけで決まる部品（問い・memo・epic・閉じた契約）の段を 1 つの関数で決め、閉じ方の誤りを線より後の閉じだけ数える。

- 何が起きているか（verified）: doctor の 4 象限（`judge`）は both / neither を出すが、局面は持たない。閉じの理由の読みは、裁定と見送りの 1 語目の値だけを読み、後ろの語を読まない。
- 約束（番号は done と 1:1）:
  1. 行 a1 の write-set の `+` の file（`crates/scribe2/src/ledger/phase.rs`）の純関数 1 本が、台帳の読み（`Issue` の列）・台帳の接頭辞・周の時刻・窓の秒・2 つの線の時刻（切り替え・close-check。close-check を true で持たない repo は None）・未反映の裁定 id の列（FR84・dispatcher の後の行が渡すまで空）・処置の無い判定を持つ memo id の列（FR87・後の行が渡すまで空）・開いた契約の write-set の項目を受け、question・memo・epic の部品と、閉じた contract の部品と、形と閉じの misfit を返す。開いた契約の局面は行 b が持つ。`crates/scribe2/src/ledger/mod.rs` に `pub mod` の宣言を 1 行と、既存の `mod tests` に歯を足す（§1）。
  2. 種類の判定は §2 の順。memo の局面は FR91 の順に判じる:
     - form-both → memo-promoting（辿れる開いた契約が 1 本以上・理由 `contract-open`・手番 none。または FR93 の条件を満たす・理由 `close-due`・手番 vessel）→ memo-asking（子の開いた問い）→ memo-no-trigger → memo-actionable → memo-waiting → no-phase。memo-waiting は前の段に当たらない開いた memo の全部を受ける（理由 null）ので、行 a1 の部品は no-phase に落ちない（no-phase は §3 の判じる順の最後の受けで、行 a1 では到達しない）。
     - memo-actionable の理由: `trigger-met`（満ちて keep の無い引き金）・`verdict`（処置の無い判定）・`promotion-unmet`（昇格の行を持ち、辿れる開いた契約が無く、FR93 を満たさない・辿れる契約 0 本を含む）・`promotion-unreadable`（最後の昇格の行が読めない）。
     - memo-waiting の理由は、満ちた引き金に keep が付いていれば `keep`、ほかは null。
     - 「辿れる契約」は、その memo を `discovered-from` で指す contract の種類の bead。「子の問い」は `parent-child` でその memo を親に持つ問い。contract でない bead（memo など）が `discovered-from` で指しても辿れる契約に数えず、`parent-child` でない依存で結ぶ問いは子の問いに数えない。
  3. FR93 の条件（memo の close を待つ判定）を 1 つの純関数が持つ: 最後の昇格の行が全部・その列と辿れる契約の集合が等しく 1 本以上・その全部が着地の形（`landed`）で閉じた・子の開いた問いが無い・処置の無い判定が無い。関数は crate の中から呼べる可視性で置き、memo の自動の close の行（ledger-form の後の行・land の終端の経路から呼ぶ）が同じ関数を使う。行 a1 の歯は `crates/scribe2/src/ledger/mod.rs` の既存の `mod tests`（`phase` の外）から直に呼んで 5 つの条件を測る。ledger の外の module から呼べることは呼び手の行の compile が測る（限界）。
     - 可視性と `mod tests` からの直の呼びは compile が測るので done にしない（器の門が測る）。done (3) は 5 つの条件だけを持つ。
  4. 問い: question-open（手番 user）／閉じて未反映の id の列に在る → ruling-unreflected（seat）／question-closed。`links.rulings` は閉じの理由の裁定 id、`links.source` は親の memo。
  5. epic: 開いて子が 1 本以上で全部閉じた → epic-closable（seat）／epic-open／epic-closed。
  6. 閉じの misfit（行 a1 の 5 語）は、2 つの線がどちらも在り、閉じた時刻が両方より後の閉じにだけ判じる。ほかの閉じは `*-closed` に置いて窓を掛け、misfit の閉じには窓を掛けない。
     - 種類ごとの頭: contract＝landed・重複・後継・取り下げ／question＝裁定／memo＝昇格済み・まとめた・見送り／epic＝完了・取り下げ。外なら close-kind-mismatch。空と 9 頭の外と着地の値の崩れも close-kind-mismatch。
     - 重複・後継・まとめた・昇格済みの値の崩れか、台帳に無い id → close-unresolved。
     - まとめた: まとめ先が memo でないか、この閉じより前に閉じていた → merged-into-not-open。
     - 昇格済み: この閉じの時刻に FR93 の条件（処置の無い判定を除く）を満たさない（辿れる契約の閉じがこの閉じより後を含む）→ promoted-unmet。理由の列の集合が最後の昇格の行の列と違う → promoted-list-mismatch。
     - 裁定と見送りの値の崩れは行 a2 が判じる（行 a1 は `*-closed` に置く）。
     - 接頭辞が解けない周は bead id と裁定 id の値を判じず、`unmeasured` に question・memo・contract・epic を `ledger-prefix` で名指す。
  7. since は §5.2 の規則で導ける値だけを返す（問いと epic の閉じは `closed_at`・question-open は `created_at`・promoting は辿れる契約の `created_at` の最新・close-due と epic-closable は閉じの最新・期日の満ちは期日・依存と着地の満ちは相手の `closed_at`）。ほかは None。
  8. memo の `due`・`triggers`・`keep` と、閉じた contract の `pointer` を埋める。
- 歯（接頭辞 `phase_ledger_`・(3) の 6 本は `crates/scribe2/src/ledger/mod.rs` の既存の `mod tests` に〔行 a1 の新しい関数を呼ぶので base で compile できず RED〕、ほかは `+` の file の末尾の歯の区間に置く・§1・fixture の `Issue` は JSON の字から `issues_of` で作る）:
  - 歯の置き場（既存の file の test 区間に置く歯）は compile・`cargo xtask check` の非 Rust 実行物の分類・flip-check が測るので done にしない（器の門が測る）。
  - どの fixture も、部品の局面・手番・理由の 3 つを測る（局面だけを見ない）。
  - (1) 入力の組の不足で落ちない・開いた契約を返さない（1 本）。
  - (2) 種類の判定 4 つと重なり 2 形（問いの label と memo の label の両方 → question・epic の型と memo の label → memo）と form-both・form-neither／memo の 4 局面の各 1 fixture と、2 局面に当たる fixture 3 本（promoting ∧ asking・asking ∧ actionable・actionable ∧ waiting）で上が勝つ（AC61）／引き金の行の無い memo が memo-no-trigger・actionable の条件にも当たる fixture も misfit（AC61）／actionable の理由 4 語の各 1／keep の付いた満ちが waiting／形と no-trigger の位置の 3 本: form-both ∧ promoting の memo は form-both、引き金の行の無い promoting の memo は promoting、引き金の行の無い asking の memo は asking（no-trigger を promoting か asking より先に判じる実装と、form-both を promoting より後に判じる実装を落とす）／数えの外の 2 本: 別の memo だけが `discovered-from` で指す memo は promoting にならず、`parent-child` でなく `discovered-from` で結ぶ開いた問いだけを持つ memo は asking にならない。
  - (3) FR93 の条件の関数を `mod tests` から直に呼び、5 つの条件を満たす memo で真・5 つの欠けの各 1 で偽。同じ fixture を局面の関数に通すと、満たす memo は close-due の promoting（手番 vessel）で、欠けの 5 つは close-due にならない（6 本）。
  - (4) 問いの 3 局面と links（閉じた問いの `rulings` が閉じの理由の裁定 id・`source` が親の memo）（1 本）。(5) epic の 3 局面と、子の無い開いた epic が epic-open（1 本）。
  - (6) 閉じの 5 語の各 1 fixture（close-kind-mismatch は 4 形〔種類と頭の食い違い・空・9 頭の外・着地の値の崩れ〕、close-unresolved は 2 形〔値の崩れ・台帳に無い id〕）／close-check の線より前の同じ閉じは `*-closed` で、線の後へ動かすと 1 件（AC61）／close-check が None の repo は数えない／`裁定 <id> 束 batch:x` の閉じは question-closed／窓の外の閉じは載らず misfit の閉じは残る／接頭辞が None で `unmeasured`／merged-into-not-open の 2 形（まとめ先が memo でない・この閉じより前に閉じていた）の各 1／値の崩れた見送りの閉じは memo-closed（a2 の語で、行 a1 は判じない）／切り替えの線より前で close-check の線より後の閉じは `*-closed`（切り替えの線を見ない実装は 1 件にする）。
  - (7) since の導ける 8 つの部品（閉じた問い・閉じた epic・question-open・promoting・close-due・epic-closable・期日の満ち・依存と着地の満ち）の各 1 と、導けない部品の None（1 本の歯に 9 つの fixture）。(8) memo の 3 欄（`due` は満ちていない期日の最も早い値で、より早い期日が満ちた memo では次の期日）と契約の pointer（1 本）。
  - base で RED: 歯の名が 0 本（rc 4・機能不在）。行 a の `phase_table_` は緑のまま。
- 限界: 閉じた時点の notes は台帳に残らないので、昇格の行は今の notes で判じる。FR93 の条件の関数を ledger の外の module から呼べることは、行 a1 の歯では測れない（`mod tests` は ledger の中）。呼び手の行（memo の自動の close）の compile が測る。
- 却下: 古い形の閉じに新しい misfit の語を足す形（a2 の 1 語で数えられる）・線より前の閉じを判じる形（消費側の台帳に古い形の閉じが多く在り、直す手が無い）。

## 8. 裁定の閉じの misfit（行 a2）

やさしく言うと: 問いと memo の閉じの理由に書かれた裁定 id が、本当に在る裁定かを確かめ、解けない閉じを線より後だけ数える。

- 前提: 裁定 id の解け方（FR83）の 1 関数と notes の裁定の行の読みは dispatcher.md 行 ak が置く（未着地）。裁定の行と裁定 event を書く bind の口は fleet-event-log.md 行 h が `crates/scribe2/src/seat/ruling.rs` に置いた（着地済み・裁定 event の本体は `Case::Ruling`）。本行は行 ak の着地の後に走り（doc を跨ぐ順は台帳の依存で表す）、その関数と読みを呼ぶだけで、写しを持たない。
- 約束（番号は done と 1:1）:
  1. 行 a2 の write-set の `+` の file（`crates/scribe2/src/ledger/phase_ruling.rs`）の純関数 1 本が次を受け、misfit の (bead id・語) の列を返す。行 a1 の関数と file は変えず（行 a2 の write-set に無い）、書き手（行 c）が行 a1 の部品のうち名指された閉じた部品を misfit に置き換える。`crates/scribe2/src/ledger/mod.rs` に `pub mod` の宣言を 1 行足す。
     - 行 a1 の file を変えないことは、その file が write-set の外で runner の guard が測るので done にしない（器の門が測る）。
     - 台帳の読み（`Issue` の列）の全部。解け方が全部の bead の notes を読むので、閉じた問いと memo に絞らない。
     - 台帳の接頭辞（解けなければ None）と 2 つの線の時刻。
     - 裁定 event の結び（問い id と裁定 id の組の列）。呼び手が event log の `Case::Ruling` から集めて渡す。問いの notes の裁定の行は、本関数が行 ak の読みで読む。
     - 1 つの閉じに返す語は 1 つまでで、§4 の表の順（close-ruling-unresolved → close-ruling-not-bound → deferred-not-child-ruling）の先の語。
  2. 閉じた問いの裁定と閉じた memo の見送りの 1 語目の値が FR83 の解け方で解けない → close-ruling-unresolved。1 語目に字が続く古い形（`裁定 <id>・束 …`）もここで数える。種類と頭の食い違う閉じ（見送りで閉じた問い・裁定で閉じた memo）は行 a1 の close-kind-mismatch が持ち、本行は判じない。
  3. 閉じた問いの裁定 id が、その問いの notes の裁定の行にも裁定 event の結びにも無い → close-ruling-not-bound。
  4. 閉じた memo の見送りの裁定 id が、子の問い（行 a1 と同じく `parent-child` でその memo を親に持つ問い）に結んだ裁定 id（約束 3 と同じ集め方）のどれでもない → deferred-not-child-ruling。`discovered-from` で結ぶ問いは子の問いに数えない。
  5. 線の規則は行 a1 と同じ（2 つの線がどちらも在り、閉じた時刻が両方より後の閉じだけ）。接頭辞が None の周は空の列を返す（`unmeasured` の `ledger-prefix` は行 a1 が名指す）。
- 閉包: 歯の fixture の `Issue` は JSON の字から `issues_of` で作り、字で組まない。裁定 event の結びは組の列で受け、fixture に `Event` を組まない。
- 歯（接頭辞 `phase_ruling_`・(1) は `crates/scribe2/src/ledger/mod.rs` の既存の `mod tests` に〔行 a2 の新しい関数を呼ぶので base で compile できず RED〕、(2)〜(5) は `+` の file の末尾の歯の区間に置く・§1）:
  - 歯の置き場（宣言の形・file の頭の字・既存の file の test 区間に置く歯）は compile・`cargo xtask check` の非 Rust 実行物の分類・flip-check が測るので done にしない（器の門が測る）。
  - (1) 関数が (bead id・語) の列を返し、1 つの閉じに 1 語だけ: 解けず結ばれてもいない問いの裁定は close-ruling-unresolved だけ・解けず子の問いの裁定でもない見送りは close-ruling-unresolved だけ（語を全部返す実装と、表の順を逆に判じる実装を落とす）。
  - (2) 解けない値と古い形の各 1／種類と頭の食い違う閉じ（`裁定 <解けない id>` で閉じた memo・`見送り <解けない id>` で閉じた問い）は 0 件（行 a1 の語を二重に数える実装を落とす）。
  - (3) 自分の notes の裁定の行にも裁定 event にも無い裁定 id の問いが close-ruling-not-bound／同じ id を notes の裁定の行だけに持つ問いと、裁定 event の結びだけに持つ問いは 0 件（片方の経路だけを読む実装を落とす）。
  - (4) `discovered-from` で結ぶ問いの裁定で閉じた見送りが deferred-not-child-ruling／同じ裁定を `parent-child` の子の問いが持つ見送りは 0 件。
  - (5) 線の対: close-check の線より前の閉じは数えず、同じ閉じを線より後へ動かすと 1 件／close-check が None の周は数えず、同じ閉じに線を渡すと 1 件／接頭辞が None の周は空の列で、同じ fixture に接頭辞を渡すと数える。
  - (6) 対照: 解けて結ばれた裁定の閉じた問いと、子の問いの裁定の見送りの memo は misfit 0 件（(2)〜(4) の各歯の中に置き、全部を misfit にする実装を落とす）。
  - base で RED: 歯の名が 0 本（rc 4・機能不在）。
- 却下: 行 a1 の関数を広げる形（行 a1 を FR83 の行〔dispatcher.md 行 ak〕と結びの行〔fleet-event-log.md 行 h〕の着地まで待たせない）。

## 9. 便と列と発話の側の部品（行 b）

やさしく言うと: 開いた契約が「走っている・断られた・列で待っている」のどれかと、便の段と、発話が仕分け済みかを決める。

- 何が起きているか（verified）:
  - 便の段は `Stage` の 11 値と `STAGES`。終端の語は `TERMINAL_TOKENS` の 7 形。
  - 待ちの理由は `WaitReason` の 8 語（`WAIT_REASONS`）で、dependency は相手の列・overlap は相手の便の run id と file 数・admission は断りの名・hold と launched は時刻・settled は sha と段を持つ。`render` の字は `overlap:<run id>/<file 数>`・`admission:<名>` の形。
  - event の本体 `Case` の形のうち、本行が読むのは発話（`Utterance`）・仕分け（`Sorted`・request / chat の 2 値）・受付の断り（`Refused`）・bind の結び（`Case::Ruling`・発話の ts と裁定 id を持つ・fleet-event-log.md 行 h が着地済み）の 4 つ。
  - 列の候補は、閉じていない契約のうち status が open のものだけ（`crates/scribe2/src/pipe/dispatch/candidates.rs`）。便の走る in_progress の契約は列の判定に無い。
  - 仕分け済みかの純関数 1 本は dialogue-surface.md 行 i が置いた（着地済み）: `crates/scribe2/src/utterance.rs` の pub な `sorted_of`（発話の ts と event の列を受け、閉じた 3 値 `Standing` を返す。`Unsorted`＝要望も答えも会話の札も無い・`ChatOnly`＝会話の札だけ・`Linked`＝要望か答えが在る〔memo の id の列と裁定 id の列を持つ〕）。`utterance` は lib の pub な module で、本行の `+` の file から可視性を変えずに呼べる（本行は utterance.rs を変えない）。FR88 は turn の終わりの判定と局面の出力が同じ入力に同じ結果を出すことを求め、行 i はその 2 つの呼び手がこの関数だけを呼ぶと書く。本行は行 i の着地の後に走る（doc を跨ぐ順は台帳の依存で表す）。
- 約束（番号は done の (1)〜(4) と 1:1・done (5) は閉包の約束）:
  1. 行 b の write-set の `+` の file（`crates/scribe2/src/fleet/phase.rs`）の純関数 1 本が次を受け、開いた契約（設計 pointer を持つもの）・その最新の便・発話の部品を返す。`crates/scribe2/src/fleet/mod.rs` に `pub mod` の宣言を 1 行と、file の末尾に新しい test 区間（`#[cfg(test)] mod tests`）を 1 つ足して歯 (3) の 1 本を置く（§1・この file は今 test 区間を持たない）。
     - 列の 1 周の判定の結果（bead id → 理由の名と値の字・`dispatch ls` の `reason=` と同じ字）
     - 開いた契約の列（閉じていない契約の bead id と、設計 pointer を持つか・呼び手が台帳から渡す）
     - bead ごとの最新の便（閉じた契約の bead も含みうる・run id・段・審査と門の判定の語・最後の detail の頭・最新の終端の語・運転手の札が生きているか・最新の段の event の ts）と、便の run id → bead id の対応
     - 受付の断りの最新（bead ごと・断りの名と ts）と、その後の便の起動の有無
     - 発話・仕分け・bind の結びの event の列（本体の `Case` で見分ける）
     - 切り替えの線の時刻・周の時刻・窓の秒
  2. 開いた契約（渡された開いた契約の列のうち設計 pointer を持つもの）:
     - 札の生きた便が在れば contract-running（理由＝その便の局面の語・手番 none・`links.runs`）。
     - 無ければ、列の理由が admission か、便の後に起きていない受付の断りが在れば contract-refused（手番 seat）。理由は、列の理由が admission ならその値の断りの名、受付の断りなら event の断りの名。refused は queued より上で、便の後に起きていない受付の断りを持つ契約は列の理由が dependency でも refused。
     - ほかは contract-queued（理由＝列の語・手番は §3 の表）。列の理由が dependency なら `links.on` は相手の列、overlap なら相手の便の run id を便の対応で引いた bead id（引けない run id は載せない）。
     - 開いた契約の列に在って列の判定に無い契約（in_progress で札の死んだ便の契約など）は `no-phase`。
     - 設計 pointer を持たない開いた契約は行 a1 の form-neither が持ち、行 b は列の理由が何でも部品を作らない。開いた契約の列に無い bead（閉じた契約）の便は、最新の便に在っても部品にしない。
  3. 便の段の写しは、便 1 本の最新の段の値だけを受ける別の純関数 1 本が §2.1 の表を `Stage` の網羅の match で持つ（段が増えると compile が落ちる）。表の写し（正本は §2.1・段 → 語 ／ 理由）: Intake → run-intake ／ `Intake`・Reviewed → 判定が PASS なら run-reviewed ／ `Reviewed`、PASS でなければ run-review-failed ／ 判定の語・Blocked → run-blocked ／ `Blocked`・Spawned → run-implementing ／ `Spawned`・Questioned → run-asking ／ `Questioned`・RateLimited → run-rate-limited ／ `RateLimited`・Implemented → run-gating ／ `Implemented`・Gated → 判定が PASS なら run-landing ／ `Gated`、PASS でなければ run-gate-failed ／ 判定の語・Landed → 運転手の札が生きていれば run-ci-waiting ／ 最新の終端の語か `Landed`、札が無いか死んでいれば run-landed-open ／ 最新の終端の語（`TERMINAL_TOKENS` の 7 形の 1 つ）・Stopped → run-stopped ／ `Stopped`・Failed → run-failed ／ 最後の detail の頭。部分の書き直し（行 d）は event log の末尾だけからこの関数を呼ぶ。`bead`＝契約の id・since＝最新の段の event の ts・`closed`＝false。
     - 段の値だけを受ける別の関数に分けることは歯 (3) の直の呼びの compile が測り、`Stage` の網羅の match は挙動に差が出ないので、どちらも done に載せない（便の diff の設計適合は gate の審査で見る）。done (3) は §2.1 の表の写しだけを持つ。
  4. 発話: 切り替えの線より後の発話の event を載せる。仕分け済みかと行き先は、発話の ts ごとに、その ts を指す仕分けと bind の結びの event を dialogue-surface.md 行 i の純関数 `sorted_of` に渡して決める（自前の判定を持たない・FR88）。`Unsorted` → 未仕分け、`ChatOnly` → 会話だけ、`Linked` → 結びあり。
     - 未仕分け → utterance-open（seat）。
     - 会話だけ → utterance-sorted（`links.destination` は chat と null の 1 つ）。
     - 結びあり → utterance-sorted（`links.destination` は memo の id の各々〔memo〕と裁定 id の各々〔ruling〕の全部で、会話の札は載せない）。
     - sorted に窓を掛ける。id は ts の字のまま・`session` と `channel` を写す・逐語は持たない。since は開きなら受けた ts、仕分けなら最も早い仕分けか bind の event の ts。
     - 行 i の純関数を呼び自前の判定を持たないことは、同じ結果を出す自前の判定と挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。done (4) は仕分けと行き先の結果だけを持つ。
- 閉包: 本行の `+` の file は `Stage` の変種を名指す（網羅の match）。`Stage` を touches に持つ contract-source.md 行 c の write-set に、本行の `+` の file を同じ docs PR で `+` 付きで宣言した。`WaitReason` と `EventKind` の変種は名指さない（理由は字で受け、event は本体の `Case` で見分ける）。歯の fixture の event は JSON の字を `from_line` で読んで作る。
  - `WaitReason` は dispatcher.md 行 w の、`EventKind` は dispatcher.md 行 a・行 ap の touches に在り、本行の `+` の file が名指すとその行の閉包に入って、現物の契約表の閉包の検査（gate の共通の verify の歯 `contract_closure_ext_real_table_has_zero_findings` と同じ `contracts check`）が赤になる。器の閉包の検査が測るので verify に置く: 便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (5) の歯にする（歯の e2e を verify に置くと歯の file を write-set に載せて行どうしが交差で直列になるので、command の 1 行にする・dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた）。
- 歯（接頭辞 `phase_event_`・(3) の `STAGES` の全部の写しの 1 本は `crates/scribe2/src/fleet/mod.rs` の末尾の新しい test 区間に〔行 b の段の写しの関数を呼ぶので base で compile できず RED〕、ほかは `+` の file の末尾の歯の区間に置く・§1）:
  - 歯の置き場（宣言の形・file の頭の字・既存の file の test 区間に置く歯）は compile・`cargo xtask check` の非 Rust 実行物の分類・flip-check が測るので done にしない（器の門が測る）。
  - (1) 入力の組が不足でも落ちない。
  - (2) 開いた契約:
    - contract-running の理由が便の語で手番 none／札の生きた便と admission の理由が同時に在る契約は running（上が勝つ）。
    - refused の 2 経路（列の理由 admission の理由は断りの名・受付の断りの event の理由は event の断りの名・手番 seat）／列の理由が dependency で、便の後に起きていない受付の断りを持つ契約は refused（queued を先に判じる実装を落とす）・同じ契約の断りの後に便が起きたら queued。
    - queued の理由 8 語（dependency・overlap・host-busy・hold・launched・settled・unreflected-ruling・floor）の各 1 の手番（§3）／dependency の `links.on` が相手の列・overlap の `links.on` が相手の便の bead（便の対応に無い run id の overlap は `links.on` を持たない）。
    - 開いた契約の列に在り列の判定に無い契約は no-phase／設計 pointer の無い開いた契約は列の理由が dependency でも部品にならず、同じ契約が pointer を持てば queued／閉じた契約の最新の便は載らず、同じ便の契約が開いた契約の列に在れば載る。
  - (3) `STAGES` の全部が §2.1 の語と手番（§3）に写る／Reviewed と Gated の PASS と PASS でない（理由は判定の語）／Landed の札の生死（ci-waiting と landed-open・理由は終端の語）／Failed の理由が最後の detail の頭。
  - (4) 発話: open・会話だけの sorted・結びありの sorted（memo 2 つと裁定 1 つの行き先を全部持つ）／会話の後に要望が足された発話は memo だけを行き先に持ち chat を持たない／id が発話の ts の字のまま（秒より下の桁を持つ）で session と channel の写し／since は開きなら受けた ts・仕分けなら最も早い仕分けの ts／線より前の発話は載らない／窓の外の sorted は載らない／逐語が出力に無い。
  - base で RED: 歯の名が 0 本（rc 4・機能不在）。行 a の `phase_table_` は緑のまま。
- 触らない: 列の判定（局面の関数は列を呼ばない・ADR-0088 (1)）・event の kind と key・終端の語。
- 却下: contract-running の手番を便と同じにする形（owned が二重に数える）。

## 10. main の側の部品と着地の commit の misfit（行 b1）

やさしく言うと: main に入った commit・契約表の行・要件の段を決め、器の便でない commit の trailer の誤りを線より後だけ数える。

- 何が起きているか（verified）: 器の便の着地の commit の本文は `run: <run id>`・契約と要件の trailer を持ち、発端の trailer の key は `source_key`。merge の trailer の門（vessel-hook.md 行 mg）が着地済み。契約表は `.md` と `.toml` の 2 形を契約表の読み手が読む。
- 約束（番号は done と 1:1）:
  1. 行 b1 の write-set の `+` の file（`crates/scribe2/src/ledger/phase_main.rs`）の純関数 1 本が次を受け、commit・row・requirement の部品を返す。`crates/scribe2/src/ledger/mod.rs` に `pub mod` の宣言を 1 行と、既存の `mod tests` に歯を足す（§1）。
     - 切り替えの線の main の sha から先端までの first-parent の commit（sha・時刻・発端の id の列・`run:` の値・契約の trailer の値）。線より後の commit だけを渡すのは呼び手（行 c の全部の書き直し）で、この関数は受けた commit を全部判じる。
     - 切り替えの線の時刻（row の閉じの読みに使う）
     - event log の run id の集合・台帳の読み・契約表の行（pointer・req）・要件 id の列（読めなければ None）
  2. commit: `run:` が event log に在る便の commit は部品にせず、返す結び（便の run id と契約の trailer の bead id ごとの commit の sha の列）に載せる。書き手（行 c）がその便と契約の `links.commits` に写す。`run:` が event log に無い → run-trailer-unknown。発端の id が 1 本でも台帳に無い → source-unresolved。発端の trailer も `run:` も無い → commit-no-trailer。ほかは commit-landed（窓を掛ける・`links.source`）。
  3. row: pointer を持つ bead が開いていれば row-beaded（since＝その bead の `created_at`）。pointer を持つ bead が着地の形で閉じたか、main の commit の契約の trailer が pointer を持つ bead を名指せば row-landed（窓を掛ける）。どちらも無ければ row-unbeaded（seat）。切り替えの線より前の閉じは形を問わず着地と読む。
  4. requirement: どの行の req にも無い要件 → requirement-unrowed（手番 seat）・在る → requirement-rowed。`owned` に数えないのは数えを持つ書き手（行 c・§5.2）で、この関数は `owned` を持たない。
     - この関数が `owned` を持たないことは挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。
  5. 要件 id の列が無いか読める形でなければ requirement を `unmeasured` の `srs-unreadable`、契約表が同じなら row を `table-unreadable` で名指し、0 件と書かない。
  6. `closed`（§5.2）: commit の部品は、その commit の契約の trailer が台帳で閉じた bead を名指すときだけ真（trailer が無いか、名指す bead が開いているか台帳に無ければ偽・語が commit-landed でも misfit でも同じ規則）。row は row-landed だけ真。requirement は常に偽。
     - 便 s2-07l.738.38.5-20260930T172347Z の gate の審査が、§5.2 が row と requirement の closed を決めておらず、commit の closed を測る歯も無いと INCONCLUSIVE にした。
- 閉包: 歯の fixture の `Issue` は JSON の字から `issues_of` で作る。
- 歯（接頭辞 `phase_main_`・(1) は `crates/scribe2/src/ledger/mod.rs` の既存の `mod tests` に〔行 b1 の新しい関数を呼ぶので base で compile できず RED〕、(2)〜(5) は `+` の file の末尾の歯の区間に置く・§1・歯の置き場は compile・`cargo xtask check` の非 Rust 実行物の分類・flip-check が測るので done にしない〔器の門が測る〕）: (1) 入力の不足で落ちない (2) commit の 4 語の各 1 fixture・`run:` が event log に無く発端の id も台帳に無い commit は run-trailer-unknown（先の語）・器の便の commit は commit の部品にならず、返す結びの便の run id と契約の bead id の両方にその sha が載る（結びを捨てる実装と便の id だけに結ぶ実装を落とす） (3) row の 3 局面（`.md` と `.toml` の pointer）・row-beaded の since が bead の `created_at`・trailer の経路（pointer を持つ bead が線より後に取り下げで閉じ、main の commit の契約の trailer がその bead を名指す行は row-landed、trailer の無い同じ行は row-unbeaded）・線の前後の対（線より前の取り下げの閉じは着地で row-landed、線より後の取り下げの閉じは row-unbeaded・線を見ずに閉じを全部着地と読む実装を落とす） (4) requirement の 2 局面（requirement-unrowed の手番は seat） (5) `unmeasured` の 2 語（その周は requirement と row の部品を 1 件も出さない） (6) `closed` の 5 形: 閉じた bead を契約の trailer で名指す commit-landed が真・開いた bead を名指す commit-landed と trailer の無い commit（commit-no-trailer）が偽・row-landed が真で row-beaded と row-unbeaded が偽・requirement の 2 局面が偽（commit を常に偽にする実装と、終わりの語だけで真にする実装を落とす）。base で RED: 歯の名が 0 本（rc 4・機能不在）。
- 限界: 消費側の SRS が器の読めない形なら requirement は常に `unmeasured`（読み手を足すのは別の行）。線より前の commit を渡さないのは呼び手（行 c）で、この行の歯は測らない（行 c の書き手が切り替えの線の sha から先端までを読む）。
- 却下: row-beaded を閉じた bead にも当てる形（行の大半が窓を持たずに出力に残る）。

## 11. 線の読みと記帳（行 c1・ADR-0100）

やさしく言うと: 「この版の器が局面の出力を始めた時」と「閉じを数え始める時」の 2 本の線を event log から読む関数と、1 度だけ書く関数を置く。書き手（行 c）はこれを呼ぶ。

- 何が起きているか（verified）:
  - `LifecycleCutover` の本体は key version・main で、書き手は 0 件。案件の一生の kind の本体の読みは任意の key detail を受け、`LifecycleCutover` の本体の読みは detail を見ない。
  - detail は kind に依らず `Event` の欄が持つ（`crates/scribe2/src/fleet/event.rs`）。1 行の読み `from_line` は任意の detail を欄へ読み（`optional_text` の 1 か所）、1 行の書き `to_line` は欄が `Some` の周に key detail を書く。案件の一生の kind の本体の読みは「登録・列の印の key を持たず、detail は任意」で、`LifecycleCutover` の行が detail を持っても読める。だから本行は event.rs を触らずに、`Event` の欄 detail に close-check を入れて書き、読んだ欄で線を見分ける（event.rs は材料として write-set に `=` で載せる）。
  - event の 1 行の読み `from_line` は `KNOWN_KEYS` の外の key と表の外の kind の行を読めない行にし、event log の読み（`crates/scribe2/src/fleet/store.rs` の `read_all`）は 1 行でも読めなければ全部を Err で返す（NFR4）。
  - 条件付きの追記 `append_if` の `Condition` は閉じた enum で値は `NotStopped` の 1 つ、match は store.rs の 1 か所だけ。
- 約束（番号は done と 1:1）:
  1. 行 c1 の write-set の `+` の file（`crates/scribe2/src/fleet/lifecycle_line.rs`）の読みの純関数 1 本が、event の列から 2 つの線（version・main・ts）を返す: 切り替えの線は detail を持たない最初の `LifecycleCutover` の行、close-check の線は detail が close-check の最初の行。detail がほかの値の行はどちらの線にも数えない。`crates/scribe2/src/fleet/mod.rs` に `pub mod` の宣言を 1 行足す。
  2. store の `Condition` に値を 1 つ（無いときだけ足す・述語を持つ）足し、event の lock の内側で述語に当たる event が無いときだけ追記する（`Condition` の match は store.rs の 1 か所だけなので、値を足す手は store.rs に閉じる）。
  3. 記帳の関数 1 本が、線の種類（切り替え・close-check）・器の版（`CARGO_PKG_VERSION`）・main の sha を受け、約束 2 の値で 1 件だけ足す。close-check の線は detail に close-check を持ち、event log に切り替えの線が在るときだけ足す（同じ lock の内側の述語で見る・無ければ何も足さない）。両方を足す周は切り替えの線を先に足す（close-check の線は切り替えの線より前にならない）。既に在る線は動かさない。
  4. event の読み手の既知の key と kind の表（`KNOWN_KEYS` と case の kind の key の表）と `EventKind` は変えない。close-check の線の行は今の読み手で読める。
- 閉包: 本行の `+` の file は `LifecycleCutover` の event を組むので `EventKind` の変種を名指す。`EventKind` を touches に持つ dispatcher.md 行 a の write-set に、本行の `+` の file を同じ docs PR で `+` 付きで宣言した。
- 歯（接頭辞 `cutover_line_`・(2) は `crates/scribe2/src/fleet/store.rs` の既存の test 区間に〔`Condition` の新しい値を使うので base で compile できず RED〕、ほかは `+` の file の末尾の歯の区間に置く・§1）: (1) 2 つの線の読み・最初の行が勝つ・detail がほかの値の行は数えない (2) 2 本の thread が同じ述語で足すと 1 件だけ (3) 1 度だけ・既に在れば足さない・同じ周は切り替えの線が先・close-check の線は detail を持つ・記帳した行の version が器の版（`CARGO_PKG_VERSION`）で main が渡した sha・切り替えの線の無い log への close-check の線の記帳は何も足さず、切り替えの線を足した後の同じ記帳は 1 件足す（対） (4) detail が close-check の `LifecycleCutover` の行の字が `Event::from_line` で読め、同じ行に既知の表の外の key を 1 つ足した字は読めない（読み手の表を変えないことを振る舞いで測る・`KNOWN_KEYS` は私有の const で、兄弟の module から数えられない）。base で RED: 歯の名が 0 本（rc 4・機能不在）。
- 触らない: 局面の判定・書き直し・宣言の読み（呼び手の行 c が読んで渡す）。
- 限界: 旧い版の器は close-check の線の行を読めるが線として扱わない。1 つの state dir が 2 つ以上の repo を持っても線は 1 本（FR90）。
- 却下: key line を足す形・新しい kind を足す形（ADR-0100・旧い版の器が log の全部を読めなくなる）。

## 12. 局面の出力の書き手と全部の書き直し（行 c）

やさしく言うと: 行 a〜c1 の関数を呼んで出力の file を丸ごと書き直す書き手 1 本と、その書き直しを撃つ時（dispatch の周・台帳を書いた直後・口）と、古さの印の読み書きを置く。

- 何が起きているか（verified）:
  - lock の実装は store の `acquire_with` の 1 本。死んだ所有者だけを外す取り方（`Reclaim` の `DeadOnly`）が在る。既定の取り方は rules 行 `fleet.lock_stale_ms`（30 秒）より古い lock を生きた所有者からも外すので、長い書き直しには使えない。
  - 入れ子の JSON は `crates/scribe2/src/fleet/json_tree.rs` の `parse` と `render` で読み書きでき、実行時の依存は 0 本（NFR3）。
  - dispatch の `fire` は、台帳の全件と event の列を 1 回だけ読む。land の終端の close は 2 か所に在る: `crates/scribe2/src/pipe/land/finish.rs` の `close_bead` と `crates/scribe2/src/pipe/retire.rs` の close。bind の口は fleet-event-log.md 行 h が `crates/scribe2/src/seat/ruling.rs` に置いた（着地済み）。答えの口は dialogue-surface.md 行 j が同じ file に置いた（着地済み・5220b839）。
  - 観測の 1 周（`dispatch ls` が撃つ列の判定・起こさない）も `crates/scribe2/src/pipe/dispatch.rs` に在り、撃つには同じ file の `Input` を組む。
  - `fleet` の口の verb は `crates/scribe2/src/fleet/cli.rs` の閉じた列（record・show・export・usage・select）。
- 約束（番号は done の (1)〜(10) と 1:1・done (11) は閉包の約束）:
  1. 入力の印は §5.3 の 3 つで、読み手は行 c の write-set の `+` の file（`crates/scribe2/src/fleet/lifecycle_mark.rs`）に置く。台帳は manifest の 1 file だけを読み、main は git を撃たずに loose の ref・packed-refs・worktree の common dir の順に読む。順の比べも同じ file に置く。
  2. 書き手は 1 本（行 c の write-set の `+` の file `crates/scribe2/src/fleet/lifecycle.rs`）。
     - `<state_dir>/fleet/lifecycle.lock` を死んだ所有者だけ外す取り方で取る。
     - 全文を同じ dir の一時 file `.lifecycle.json.<pid>.tmp` に `render` で書き、fsync の後 `lifecycle.json` へ rename する。
     - rename の前に今の file の印を読み、どれかが自分の印より新しければ書かない（`Discarded`・順を持たない組は古くないと読む）。
     - 中身が今の file と `generated_at` の他に同じなら rename しない（`Unchanged`）。
     - 返りは閉じた 6 値: `Written`・`Unchanged`・`Coalesced`・`Discarded`・`Busy`・`Unreadable(<語>)`。
     - 後の行が呼ぶ口（行 d と §15 の読み手の行）: 書き手 1 本は出力の型の値と scope（全部か部分）を受け、同じ 6 値を返す。出力を型の値へ読み戻す読み 1 本（閉じた 3 値: 無い・読めない・読めた出力）と、年齢（`overdue`）と `owned` を rules の `lifecycle.age_h.*` と時刻で数え直す関数 1 本を、書き手と同じ file に置く。3 本は fleet の兄弟の module から呼べる可視性で置く。呼べることは呼び手の行の compile が測り、本行の歯では測れないので done に載せない（便 s2-07l.738.38.8 と .38.9 の事前審査が、行 d・e が呼ぶ行 c の口の名・署名・可視性がどこにも無いと名指した）。
     - 書き手が 1 本であること・一時 file の名・fsync は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。返りの値の名は歯が名指すので compile が測り、done にしない（器の門が測る）。
  3. 全部の書き直し:
     - lock を取ってから入力を読む。入力は FR90 の 5 つ（台帳の全部・event log・main の設計 doc の契約表と SRS・main の commit の trailer・main の先端の vessel 宣言）。
       - 台帳は印（§5.3・`--repo` の `.beads` の下の file を読むだけ・子 process を撃たない）を先に読み、読めなければ bd を撃たずに `Unreadable(ledger)` を返す（約束 4）。印を読めた周だけ bd で全件を読む（契機 (a) は `fire` の 1 回の読みを借りる）。
       - この順で、land の e2e の偽 bd の argv の記録の最後の呼びを読む既存の歯（`crates/scribe2-boundary/tests/e2e/pipe/land/order.rs` と `crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs`・偽 bd は呼びのたびに記録を上書きする）は、toy の repo に `.beads` が無いので契機 (d) の書き直しが bd を撃たず、緑のまま（main d5e407cd で確かめた）。
     - 宣言は、読んだ main の sha の tree から読む。宣言の読み `crates/scribe2/src/pipe/declaration/optional_keys.rs` の `close_check` は HEAD を読むので使わず、その隣に sha を名指す読みを 1 本足す（`git show <sha>:<宣言 file>` を同じ読みに掛け、同じ閉じた 3 値 `Joins`・`Exempt`・`Unreadable` を返す）。
       - `optional_keys` は私有の module で、外からは `crates/scribe2/src/pipe/declaration.rs` の `pub use` の列（`floor_check_at`・`ruling_keys_at` と同じ列）だけで見えるので、sha の読みも同じ列に 1 本足す。declaration.rs を write-set と growth の 1 行に置く（main de76f797 で `mod optional_keys;` が私有と確かめた・便 s2-07l.738.38.7-20260930T202411Z の契約の審査が、この連鎖が write-set の外だと名指した）。
     - 部品は行 a1・a2・b・b1 の関数で導き、行 a2 が名指す閉じた部品を misfit に置き換える。線は行 c1 の読みで取る。
     - 行 b に渡す bead ごとの最新の便（`Latest`）は、event の列を受ける 1 本の関数で組む: 段と detail と時刻は fleet の pub の `replay` の便の表から、判定の語・detail の頭・終端の語は段の行の detail から、運転手の札の生死は pipe の pub の `live_driver`（札の file と /proc を読むだけ・子 process を撃たない）から。行 d が末尾の行で同じ 1 本を呼ぶので、書き手と同じ file に fleet の兄弟の module から呼べる可視性で置く。組み立ての置き場は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る・札の生死で局面が分かれることは行 b の歯 (3) が測る）。
     - 行 b に渡す列の判定は、契機 (a) では `fire` の 1 周の結果を借り、ほかの契機では観測の 1 周（`dispatch ls` と同じ判定・起こさない）を撃って受ける。観測の 1 周を置き場・repo・rules・bd から撃つ pub の関数を 1 本、dispatch の子 module `crates/scribe2/src/pipe/dispatch/candidates.rs` に置き、dispatch.rs の `pub use` の 1 行で外へ出す（dispatch.rs は R-C4-2 の上限まで 26 行しか余地が無い・main d5e407cd の実測）。`Input` の src の literal は今 cli.rs の 1 か所で、本行は candidates.rs に 1 か所足す（`Input` はどの行の touches にも無い）。
     - 行 a2 に渡す裁定 event の結びは、event log の `Case::Ruling` の (bead・ruling) から集める。
     - 行 b1 が返す結び（便の run id と契約の bead id ごとの commit の sha）を、その便と契約の部品の `links.commits` に写す。
     - 行 b1 に渡す main の commit の 3 つの trailer の key: `run:` は `crates/scribe2/src/pipe/land.rs` の `RUN_TRAILER`、発端は `source_key`、契約は finish.rs の私有の `trailer_key` と `CONTRACT_TRAILER` から組む。finish.rs に `source_key` と同じ形の crate の中の関数を 1 本足し、land.rs の `source_key` の再輸出の 1 行に並べる（land.rs を write-set に置く・finish は land.rs の私有の module）。契約の trailer の値は設計 pointer の字（着地の commit は契約の設計 pointer を書く）なので、台帳の issue の設計 pointer（`pointer_of`）で bead id へ引き直して行 b1 に渡し、引けない pointer は渡さない。
     - 行 b の発話の部品の行き先の memo の各々について、その memo の部品の `links.source` に発話の ts を足す（発端の正本は仕分けの event・FR88）。
     - `full_at` は全部の書き直しの `generated_at`、`interval_s` は rules 行 `seat.tick_interval_s` の値、`closed_window_h` は rules 行の値を写す。
     - `since` を導けない部品は、前の file の同じ (part, id, phase) から継ぐ（§5.2）。
     - 窓（`lifecycle.closed_window_h`）は、終わりの局面の閉じた部品を載せるかにだけ掛ける。
     - owned を数える（§5.2）。
     - lock と読みの前後・部品を導く関数の呼び先・契機 (a) が `fire` の 1 周を借りることと観測の 1 周の置き場は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。done は契機 (a)(d)(e) の出力が列の判定を持つことだけを約束し、sha の読みの返りの 3 値は歯が名指すので compile が測る（器の門が測る）。
  4. 入力を読めない周は書き直さない。
     - `unreadable` の印（入力の印を持たない）を付け、理由を 1 語だけ持たせる。
     - 理由の語は閉じた 6 語で、読む順に `ledger`・`events`・`main`・`table`・`srs`・`declaration`。読めなかった最初の 1 語を名指す。
     - 宣言が在って読めない周は `declaration`。SRS と契約表が無いか読める形でない周は読めない周でなく、§5.2 の `unmeasured` で書き直す。
  5. lifecycle.stale の読み書きは 1 本の関数（`lifecycle_mark.rs`）。
     - `lifecycle.stale.lock` を短く取り、読み・足し・消し・一時 file からの rename をする。
     - 印の種類は閉じた 3 つ（`ledger-gate`・`merge-gate`・`unreadable`）。種類ごとに 1 つまでで、後の印が前の印を置き換える。
     - 最初の `Written` で `{"version":1,"marks":[]}` を作り、それからは消さない。
     - 書く順は json の rename → stale の消し。
     - 読み書きが 1 本の関数で `lifecycle.stale.lock` の内側に在ることは、本行の順に撃つ歯では挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る・lock を取れない周の挙動は行 e の歯 (7) が測る）。
     - 後の行が呼ぶ口（行 d・e）: 印を 1 つ足す関数 1 本は、置き場・印の種類・印の値（台帳の印か main の sha か読めない理由の語）・時刻を受け、閉じた 4 値を返す: 付けた・出力が無い（`lifecycle.json` が無い）・lock を取れない（`lifecycle.stale.lock` を死んだ所有者だけ外す取り方で短く取り直しても取れない）・読めない（`lifecycle.stale` が読める形でない）。約束 1 の台帳の印の読みと main の sha の読みと共に、hook の module から呼べる可視性（crate の中の pub）で置く。呼べることは呼び手の行（行 d・e）の compile が測り、返りの 4 値の挙動は行 e の歯 (4)(7) が測るので、本行の done に載せない。
  6. 印を消すのは全部の書き直しだけ（`Written` か `Unchanged` の後）。消すのは、書き直しの開始（lock の後）が印の `at` より後で、次に当たる印だけ。
     - `ledger-gate`: 読んだ台帳の印が印の値より新しい（§5.3 の順・gen が違えば新しい）。
     - `merge-gate`: 読んだ main が印の sha の真の子孫（event log だけ進んだ周は消えない）。
     - `unreadable`: 5 つの入力を全部読めた。
     - 部分の書き直しは消さない。
  7. 線: `Written` か `Unchanged` の周に、行 c1 の記帳の関数で、切り替えの線が無ければ足し、読んだ main の先端の宣言が close-check を true で持ち close-check の線が無ければ足す（切り替えの線が先）。後の書き直しは読むだけ。宣言が在って読めない周は約束 4 で書き直さないので記帳しない。
     - 記帳が行 c1 の関数を呼ぶことは挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。
  8. 全部の書き直しの契機（どれも約束 3 の 1 本を呼ぶ・呼び手の rc と stdout の字は変えず、`Written`・`Unchanged`・`Coalesced` の外の返りは stderr に `lifecycle=<語>` の 1 行を出す）:
     - (a) `fire` の事前審査の後。同じ 1 回の読みを借りる。
       - stderr の 1 行の置き場: lib は stderr に書かない（Cargo.toml の lint `print_stderr` が deny）。`fire`（dispatch.rs）は書き手の返りのうち `Written`・`Unchanged`・`Coalesced` の外の語を `Turn` の新しい欄 1 つ（`Option` の語・ほかの周は `None`）に載せ、`crates/scribe2/src/pipe/cli.rs` の列の 1 周の組み立て（`turn_lines` と終端の周の同じ組み立て）がその語を Outcome の stderr の `lifecycle=<語>` の 1 行にする。
       - `Turn` の literal の構築点は dispatch.rs の外に 2 か所（`crates/scribe2/src/pipe/dispatch/candidates.rs` と `crates/scribe2/src/hook/utterance.rs` の歯の区間）在り、欄を足すと compile できないので、どちらにも `None` の欄を足す。cli.rs と 2 つの file を write-set に置く（main be490f8 で `Turn {` を grep した・便 s2-07l.738.38.7-20260930T195514Z の契約の審査が、この置き場が write-set の外だと名指した）。
     - (b)(c) bind の口の記帳の後と答えの口の記帳の後の契機は、本行に持たない（本行の write-set を広げない）。2 つの口（fleet-event-log.md 行 h・dialogue-surface.md 行 j・どちらも `crates/scribe2/src/seat/ruling.rs`）は着地済みで、2 つの口の契機は本行の着地の後に本 § へ足す行が持つ。
     - (d) land の終端の close の Ok の後（`close_bead` と retire.rs の close）。置き場は終端の周の知らせより前とする（知らせの語が出力を読むのは §15 の読み手の行で、順の歯はその行が持つ）。
       - stderr の 1 行の置き場: `close_bead` は閉じた 7 値の `Terminal` を返すだけで、Outcome を組むのは finish.rs の `terminal` の呼び手の 2 か所（finish.rs の着地の本体と、`pipe land --terminal-only` の `crates/scribe2/src/pipe/cli/step.rs` の `terminal_only`）。書き直しは `close_bead` の中でなく、2 か所の呼び手が `terminal` の返りの `rc` が 0 の周（close した 2 値だけが 0）に、行 c の書き手の file の 1 本の関数（全部の書き直しを撃ち、`Written`・`Unchanged`・`Coalesced` の外の語だけを返す・crate の中から呼べる可視性）を呼び、その語を Outcome の stderr に足す。`Terminal` に欄を足さず、変種も名指さない。step.rs を write-set と growth に置く（main a32953b4 で確かめた・便 s2-07l.738.38.7-20260930T203430Z の契約の審査が、`terminal_only` の Outcome の組み立てが write-set の外だと名指した）。retire.rs の close は retire.rs の中で Outcome を組み、同じ 1 本の関数を呼ぶ。
       - 契機 (d) の中継の関数を finish.rs に置かない: `finish` は `crates/scribe2/src/pipe/land.rs` の私有の module で、step.rs からは land.rs の再輸出の列（`terminal` を含む）越しにしか見えない（main d5e407cd で確かめた・便 s2-07l.738.38.7-20260930T204910Z の契約の審査が名指した）。2 か所の呼び手は行 c の書き手の file の関数を直に呼ぶ。
     - (e) 書き直しの口（約束 9）。
     - `dispatch ls` は撃たない。memo の自動の close の後の契機は、その close を足す ledger-form の後の行が同じ 1 本を呼ぶ。
     - 後の行が持つ契機（(b)(c) と memo の自動の close）は本行の挙動でないので done に載せない。
  9. 口は既存の `fleet` の口の verb を 1 つ足す（新しい top-level の口は作らない・R-C4-5）。
     - `scribe2 fleet lifecycle write --state-dir S --repo R [--bd CMD] [--wait-ms N]`
       - 消費側の面が自分の台帳を書いた直後に撃ってよい。
       - 撃った時に入力の印を先に読み、lock を N ms（既定 rules 行 `fleet.lock_retry_ms`）まで待つ。
       - 取れた時、file の印が撃った時の印より古くなければ、書き直さずに `Coalesced` を返す。
       - 取れなければ rc 1 `lifecycle=busy`。印は付けない。
       - rc 0 は `Written`・`Unchanged`・`Coalesced`・`Discarded`。ほかは rc 1。
     - `scribe2 fleet lifecycle show --state-dir S`
       - 同じ renderer で今の組を出し、書き直さない。
       - 無い周は rc 1 `lifecycle=absent`、読めない周は rc 1 `lifecycle=unreadable`。
     - text の字:
       - 頭の行: `lifecycle version=… generated=… scope=… ledger=<root>/<chunks>|<len> events=<len> main=<sha> stale=<種類の列|->`
       - 部品の行: `part=… id=… phase=… turn=… since=…|- reason=…|-`
     - 使い方の 1 行と help の表（`crates/scribe2/src/help.rs`）に `lifecycle <write|show>` を足し、外形 snapshot を直す。
  10. rules 行 2 種（裁定 user 2026-09-30T04:25Z・行の ruling の字は `user 2026-09-30T04:25Z 項 lifecycle`）。
      - `lifecycle.closed_window_h`（kind 1 つ）= 72。
      - `lifecycle.age_h.<語>`（kind 1 つ・id の後ろは §3 で手番が seat の語）を 13 行:
        - 2: utterance-open・run-landed-open
        - 4: contract-refused・run-review-failed・run-gate-failed・run-stopped・run-failed
        - 24: ruling-unreflected・memo-actionable・contract-queued・row-unbeaded・misfit
        - 72: epic-closable
      - 語の外の後ろを持つ行は rules の検査が断る。行の無い seat の語のうち run-asking は `owned.unset` に数え、requirement-unrowed は数えない（§5.2・requirement はどれにも数えない）。
      - ruling の字を裁定 id のままにしない: base の行 `floor.timeout_s` が同じ裁定 id を持ち、`cargo xtask rules-diff` は新設の行が base の行の裁定を使い回すと `new-row-reuses-ruling` で落とす（`crates/xtask/src/rules_diff.rs`）。同じ裁定の項を名指す後ろの語（`項 <語>`）は manifest に前例が在る。
      - 後ろの語の検査は行の検査（`crates/scribe2/src/rules/mod.rs` の `validate`）に置く。
      - kind は `ALL` の末尾に、行は manifest の末尾に足す。末尾の位置と数を pin する既存の歯を直す: rules の e2e の `rules_floor_timeout_row_precedes_the_drafts_cap_rows`・`rules_drafts_cap_rows_are_the_last_two_kinds_and_rows`・`class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id`（末尾の位置）・`rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all`（末尾の列）・`rules_embedded_manifest_is_valid_and_covers_all_kinds` と `rules_embedded_manifest_declares_one_capability_row_per_role`（行と kind の数）・外形の snapshot の 2 行。全部の kind を id probe の 1 行で検査する `rules_kind_parity_every_kind_has_sample` も、age_h の kind だけ語の内の id で組むように直す。
      - 母集団の数え方: 本行を撃つ時の main で rules の e2e（rules.rs と rules/embedded.rs）の rev・take・len の数の pin を grep で全部数える（main d5e407cd で上の 7 本と snapshot の 2 行）。どれも新しい kind を名指すか数が違うので base で落ち、retroactive の札は要らない。
- 閉包:
  - 本行の `+` の file は `EventKind` を名指さない（行 b の §9 の閉包と同じ形）。発話・仕分け・bind の結び・受付の断りは event の本体 `Case` で見分け、便の現在地（bead・段・detail・時刻）は fleet の pub の `replay` が返す便の表から読む。`Case` でも便の表でも見分けられない kind（受付の断りの後の便の起動を測る `RunCreated` など）は、行の kind の字（`as_str` の値）で比べる。歯の fixture の event は JSON の字を `from_line` で読んで作る。
    - `EventKind` を touches に持つ行は dispatcher.md 行 a と行 ap の 2 本。行 a の write-set には行 c・d の `+` の file を前の docs PR で宣言したが、行 ap の write-set には無い。ap に `+` で足す形は着地の順で壊れる（行 c か d が先に着地すると ap の受付で `+` の file が既に在って断られ、ap が先に着地すると現物の表の検査が ap の閉包の欠けを赤にする）ので、名指さない形にした（2026-09-30 の probe: `EventKind` の match の arm を 1 本持つ書き手の file の木で、contracts check が行 ap の write-set-incomplete を名指した）。
  - `Stage` は値のまま行 b の関数へ渡し、変種を名指さない（`Stage` の変種の match の arm を書かない）。
  - 新しい file と歯の fixture に、`WaitReason` の変種の literal と `crates/scribe2/src/pipe/dispatch.rs` の `Turn` の literal を書かない（contract-source.md 行 c・dispatcher.md 行 w・consumer-sync.md 行 g の閉包を広げない）。列の判定は `WaitReason::render` の字で受ける。
  - rules 行は id の字で引く（`RuleKind` の変種を新しい file で名指さない）。
  - `EventKind`（dispatcher.md 行 a・行 ap）・`Stage`（contract-source.md 行 c）・`WaitReason`（dispatcher.md 行 w）・`Turn`（consumer-sync.md 行 g）・`RuleKind`（rules 行を足す行）は touches に在り、本行の `+` の file が arm・literal・変種で名指すとその行の閉包に入って、現物の契約表の閉包の検査（gate の共通の verify の歯 `contract_closure_ext_real_table_has_zero_findings` と同じ `contracts check`）が赤になる。器の閉包の検査が測るので verify に置く: 便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (11) の歯にする（歯の e2e を verify に置くと歯の file を write-set に載せて行どうしが交差で直列になるので、command の 1 行にする・dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた）。
- 歯:
  - lib（行 c の書き手の `+` の file の末尾の歯の区間・接頭辞 `lifecycle_writer_`）: (2) 書きかけの不可視・古い書きの捨て 3 通りと順を持たない組・`Unchanged` で rename しない・生きた pid の lock は rules 行 `fleet.lock_stale_ms` より古くても外さず `Busy`・死んだ pid の lock は外して `Written` (3) 窓は終わりの局面の閉じた部品だけに掛かる（窓より古く閉じた ruling-unreflected の問いは残る）・since の継ぎの 3 形（前の出力の同じ (part, id, phase) から継ぐ・前の出力が在って同じ組が無ければ `generated_at`・前の出力が無ければ null）・owned の 4 欄（count・unset・unknown・oldest・requirement は数えない）と、unset が行の無い seat の語 run-asking の部品で増え requirement-unrowed の部品で増えない対・overdue が手番の seat でない部品と行の無い部品で null・行 b1 の結びが便と契約の `links.commits` に載る（結びを捨てる実装を落とす）・request の仕分けの発話の ts がその memo の `links.source` に載る・`full_at` と `interval_s` と `closed_window_h` の写し・worktree の HEAD の宣言が close-check = true で読んだ main の sha の宣言が false の toy repo で close-check の線が足されない（HEAD の宣言を読む実装を落とす）・行 a2 の関数が名指した行 a1 の閉じた部品が misfit に替わる（同じ id の部品が 1 つだけで、局面は misfit・理由は a2 の語）(4) 読めない 6 語の各 1 fixture と印（2 つの入力が同時に読めない周は読む順で先の語）・SRS の読みが落ちた周は `srs` の印で書かず、SRS の無い repo は unmeasured で書く（対） (5) 置き換えの向き 2 通り・空の marks の作りと不消・json の rename が落ちた周（`lifecycle.json` の名に dir を置いた fixture）は stale の印を消さず、同じ歯の中で rename の通る周は消す (6) 印の消えの表（3 種 × {開始が印より前・印の後で同じ入力・印の後で新しい入力}）と merge-gate の event log だけの進み・`Discarded` と `Busy` の周は条件に当たる印も消さない（同じ歯の中の `Written` の周は消す） (7) 線の 2 置き場の記帳と不動・読めない宣言で記帳しない・宣言が false の repo は close-check の線を足さない・`Discarded` と `Busy` の周は線を足さない・`render` の字が `parse` で読める。
  - lib（行 c の印の読み手の `+` の file の末尾の歯の区間・接頭辞 `lifecycle_mark_`）: (1) manifest の fixture 3 形（journal だけ・table file つき・gc の世代つき）と files の形・順の比べ（同じ gen の chunks・gen の違い・head の違い）・loose の ref と packed-refs と worktree の gitfile（fixture の .git は ref の file・packed-refs・gitfile だけを持ち HEAD と object を持たないので、git を撃つ実装は読めずに落ちる）。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/fleet.rs`・接頭辞 `fleet_lifecycle_`）: (9) write と show が 1 字も違わない・頭の行と部品の行の字が §12 の形（key の順・値の無い欄の `-`）・absent と unreadable・`--wait-ms` の busy と coalesced・使い方の行と `scribe2 help fleet` の頁（FORM と SUBCOMMANDS）が `lifecycle` を持つ (8) 契機 (a)(d)(e) で generated が進み、`dispatch ls` では進まない・(a)(d)(e) の 3 契機の周の出力に、依存を待つ開いた契約（偽の台帳）が contract-queued・理由 dependency で出る（契機ごとに列の判定を渡さない実装を落とす）・(d) は `pipe land --terminal-only` の終端の close（`close_bead`）と `pipe retire` の close の 2 経路で進み、偽 bd の close が落ちた周は進まない（同じ歯の中の肯定と組）・偽 bd が落ちる周の印と理由 `ledger`・契機 (a) の `dispatch` と (d) の 2 経路で、`Written` の周と `lifecycle.lock` を生きた pid で持たせた周の呼び手の rc と stdout の字が等しく `lifecycle=` を持たず、stderr の `lifecycle=` の行は busy の周の `lifecycle=busy` の 1 行だけ。新しい e2e の file は作らない。
  - lib（`crates/scribe2/src/pipe/declaration/optional_keys.rs` の既存の test 区間・接頭辞 `close_check_at_sha_`・2 本）: (3) 2 つの commit の toy repo（1 つ目の宣言は close-check = false・2 つ目は true）で、sha の読みが 1 つ目で `Exempt`・2 つ目で `Joins`、HEAD を 1 つ目へ戻しても 2 つ目の sha の読みは `Joins`／宣言 file の無い sha は `Exempt`・型の違う宣言の sha は `Unreadable`。
  - rules（既存の rules の e2e・接頭辞 `rules_lifecycle_rows_`・2 本）: (10) kind 2 つと行 14 本・行ごとの値（72 と 2・4・24・72）と裁定 id と ruled_at・語の外の後ろを断る。あわせて kind と行の数の pin（`rules_embedded_manifest_`）と外形 snapshot を直す。直す既存の歯は数の pin で base で落ちるので、札を付けずに flip-check の RED-on-base を通る（retroactive の札は base で緑のままの歯の逃がし・`crates/xtask/src/flipcheck.rs`・base で緑のままの歯が在る周だけ、その歯に札）。札の欠けは gate の flip-check が赤で落とすので、done の項目にしない（器の門が測る）。
  - 外形の snapshot の歯（既存の `fleet_external_form`・`crates/scribe2-boundary/tests/e2e/fleet.rs`）を verify に入れる。使い方の行が `lifecycle` を持つので snapshot が変わり、base で落ちる。
  - base で RED の理由は機能不在: lib は歯の名が 0 本（rc 4）、e2e は未知の verb の断りの rc 1（`crates/scribe2/src/fleet/cli.rs` の `dispatch` が `RC_REFUSED` と使い方の行を返す）、rules は行が無い。
  - 置き場と file ごとの base で RED の理由（§1）: `close_check_at_sha_` は optional_keys.rs の test 区間（新しい sha の読みを呼ぶので base で compile できない）。e2e の `fleet_lifecycle_` はどの歯も `fleet lifecycle` の口か出力の在ることを測る（base は未知の verb の断りの rc 1 か出力が無い）。「`dispatch ls` では進まない」は契機 (a) で進む肯定と同じ歯に置く。`rules_lifecycle_rows_` は行が無い。直す `rules_embedded_manifest_` の pin は数が違う。
- 触らない: 局面の判定（行 a〜b1）・線の読みと記帳（行 c1）・部分の書き直し（行 d）・門の印付け（行 e）・読み手（§15）・dispatch の判定と rc・land の終端の段と字。
- 限界:
  - dispatch の周は timer を持たない（ADR-0088）。手当ては書き直しの口。
  - 全部の書き直しの間は、部分の書き直しが `Busy` で飛ぶ。飛んだ分は次の周が拾う。
  - 入れ替えの後、最初の全部の書き直しまでの閉じは数えない。入れ替えの手順に書き直しの口を 1 回足す。
  - `--readonly` の無い bd の読み（`bd ready` など）も台帳の印を進める（§5.3 の実測）。起票の門の印の後・門が通した書きの前にそれが走ると、その後の全部の書き直しが印を消し、書きの入る前の出力が古さの印を持たないことがある。次の全部の書き直しで追いつく。
  - files の形は、読みで更新時刻が動けば印を早く消しうる。
  - 全部の書き直し 1 回の所要は fixture で測り、PR の本文に写す。
  - bind の口と答えの口の後の契機は後の行が足す。それまで、2 つの口の記帳は次の dispatch の周か書き直しの口の周まで出力に遅れる。
  - AC60 の全部の書き直しの契機 6 つのうち、本行が持つのは 3 つ（dispatch の周・land の終端の close・全部を書き直す口）で、bind・答えの口・memo の自動の close の後の 3 つは後の行が持つ。
  - `unmeasured` の `multi-anchor` は本行が出さない（本行は `--repo` の 1 つだけを読み、1 つの state dir が 2 つ以上の anchor を持つ周の判じ方は後の行が決める）。
  - 出力の読み（stale → json の順・`fleet lifecycle show` が使う 1 本）と入力の印の読みは、fleet の兄弟の module から呼べる可視性で置く（ledger-form.md 行 o の読み手が呼ぶ）。呼べることは本行の歯では測れず、呼び手の行の compile が測る。
  - anchor が main の先端より遅れていても、宣言は読んだ main の sha の tree から読むので、close-check の線はその sha の宣言で引く。
- 却下: 新しい top-level の口（R-C4-5）・固定の名の一時 file・印を lifecycle.json に持つ（門が大きな file を読み書きする・NFR5）・線を出力に持つ（ADR-0088）・部分の書き直しが印を消す（ADR-0088 (3)）・既定の取り方の lock（生きた書き手から奪われる）・台帳の印に更新時刻を使う（読みでも動く）・呼び手の stdout の行に `lifecycle=<語>` を足す（既存の完全一致の歯と消費側の面の key を動かす）。

## 13. 部分の書き直し（行 d）

やさしく言うと: 台帳も git も撃たずに、event log の増えた末尾と今の出力だけから、発話・便・期日・年齢の部分を書き直す。管理 tick の周に撃つ。

- 何が起きているか（verified）:
  - 部分の書き直しは無い。event log の読みは `read_all` の 1 本だけで、毎回全部を読む。
  - 管理 tick は `crates/scribe2/src/seat/tick.rs` に在る。歯の module を子の file へ割る純移動が着地し、file は約 1230 行（余地は約 270 行）。本行が足すのは周の呼び出しの数行だけで、本体は行 d の `+` の file に置く。
  - hook の予算は NFR5: 2.0 秒以内で、読む byte が event log の大きさで変わらないこと。
  - 行 b の便の段の関数 `run_part` は `Latest` を受け、`Latest` は運転手の札の生死（Landed の便の ci-waiting と landed-open を分ける）と、段の行の detail の判定の語・detail の頭・終端の語を持つ。札の生死は pipe の pub の `live_driver` が札の file と /proc を読むだけで返す（子 process を撃たない）。`Latest` の組み立ては行 c の書き手の file の 1 本（§12 約束 3）。
  - 行 b の発話の部品は切り替えの線より後だけで、線が無ければ 1 件も載らない（`phases` の入力の `line`）。出力は線を持たない（ADR-0088）。線は全部の書き直しが出力を書いた後に足す（§12 約束 7）ので、出力を書いた最初の全部の書き直しの直後の末尾にだけ在り、その後の末尾には無い（線は末尾より前に在る）。
  - 便 s2-07l.738.38.8 の事前審査が、`run_part` の要る札の生死が約束 4 の読みに無いことと、線が末尾にも出力にも無いことを名指した。
- 約束（番号は done の (1)〜(7) と 1:1・done (8) は閉包の約束）:
  1. 契機は管理 tick の周の 1 つ（呼び手の rc と字を変えない）。FR90 の残りの 2 契機（発話の記帳の直後・仕分けの記帳の直後）は fleet-event-log.md 行 g と dialogue-surface.md 行 i の `+` の file に呼び出しを足す手で、その 2 行の着地の後に本 § へ足す行が持つ（今は名指せない）。便の段の記帳の直後の契機は FR90 の契機の列に無いので、SRS の追加の round の後の行にする。
     - 後の行が持つ契機は本行の挙動でないので done に載せない。tick の rc と字を変えないことは歯 (1) が同じ歯の中で測る。
  2. 出力が無い置き場では何もしない（`Absent`）。作るのは全部の書き直しだけ。
  3. lock は短く待つ: `lifecycle.lock` を死んだ所有者だけ外す取り方で 200 ms まで取り直し、取れなければ `Busy` で飛ぶ。
  4. 読むのは次の 6 つだけ: 今の `lifecycle.json`・event log の 1 行目（4 KiB まで）・`inputs.events.len` の直前の 1 byte・`inputs.events.len` から末尾まで（store に足す末尾の読み・`read_all` は変えない）・埋め込みの rules（`lifecycle.age_h.*`）・末尾に段の行を持つ便の運転手の札（`live_driver`・便ごとに 1 つ）。台帳・git・宣言・契約表・SRS は読まず、子 process を 1 本も撃たない。
  5. log が繋がらない周（1 行目の ts が `inputs.events.head` と違う・長さが `len` より短い・直前の byte が改行でない）は書き直さず、行 c の印の関数で `unreadable`（理由 `events`）を付ける。
  6. 書き直す部分（FR90 の列）。判定はどれも行 a・b の関数を呼び、自前の判定を持たない:
     - 判定の呼び先は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。部品が log の全部を行 b の発話の関数に渡した結果と一致することは歯 (6) が測る。
     - 発話: 末尾の発話・仕分け・bind の結びの event から、行 b の発話の関数（仕分け済みかは dialogue-surface.md 行 i の純関数）で発話の部品を導き直す。末尾より前に受けた発話に末尾で仕分けか結びが足された周も、log の全部の event を行 b の発話の関数に渡した結果と同じ部品（局面・since・行き先）にする。request の仕分けは、出力に在るその memo の `links.source` に発話の ts を足す。
       - 線: 末尾に切り替えの線（detail の無い最初の `LifecycleCutover`）が在ればその ts を、無ければ出力の `inputs.events.head`（log の 1 行目の ts・末尾の全部の event はそれより後）を `line` に渡す。
       - 末尾の発話は `phases` に空の契約の列と末尾の event の列を渡して導く。末尾より前に受けた発話（出力に部品が在る）は `sorted_of` を末尾の event の列で呼び、前の部品と合わせる: 行き先は前の部品と末尾の結果の memo と ruling の和（和が空でなければ chat を落とす）、局面は和が空でなければ結びあり・前か末尾が会話だけなら会話だけ・どちらも無ければ未仕分け、since は前の部品の since と末尾の最も早い仕分けか結びの ts の早い方。窓は合わせた since に掛ける。
       - 末尾にも出力にも無い発話を指す仕分けと結びは読まない（線より前か窓の外の発話で、全部の書き直しも載せない）。
     - 便: 末尾に event の在る便のうち、出力の閉じていない契約の部品に属する便を、行 b の便の段の関数で導き直す。末尾で新しく起きた便はその契約の最新の便として部品を足し、同じ契約の前の便の部品を外す。閉じた契約か出力に無い契約の便は載せない。
       - 便の `Latest` は、末尾に段の行（段の欄を持つ行）を 1 つ以上持つ便だけを、その便の末尾の行の全部で §12 約束 3 の組み立ての 1 本に通して作る（札の生死は `live_driver`）。末尾に段の行を持たない便は部品を動かさない（`overdue` だけ数え直す）。
     - 期日: 前の全部の書き直しが残した `due` が今以前の memo のうち、memo-waiting で `keep` の無いものを、行 a の `met` で満ちと判じて memo-actionable（理由 `trigger-met`・since は期日）へ移し、`due` を次の満ちていない期日か null にする。`keep` の在る memo は memo-waiting（理由 keep）のまま、memo-waiting でない memo（promoting・asking・misfit）は動かさない（FR91 の順）。
     - 年齢と owned を rules の `lifecycle.age_h.*` で数え直す（どの部品の `overdue` も数え直す）。
     - `scope` を `partial` にし、`inputs.events` だけを進め、ledger と main と `full_at` は前の値のまま持つ。契約・問い・行・要件・epic・commit の部品は、局面・手番・since・理由・結びを動かさない（`overdue` だけ数え直す）。
  7. 書くのは行 c の書き手の同じ 1 本（`Unchanged` なら rename しない）。出力の読み戻しと年齢と `owned` の数え直しも行 c の同じ file の 1 本ずつを呼び、読めない印は行 c の印を 1 つ足す関数で付ける（§12 約束 2・5）。部分の書き直しは印を消さない。
     - 書き手が行 c と同じ 1 本であることは挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る・`Unchanged` の判定の一致は歯 (4) が測る）。
- 閉包: 本行の `+` の file は `EventKind` を名指さない（§12 の閉包と同じ形）。末尾の発話・仕分け・bind の結びは本体の `Case` で見分け、末尾の便は行の段（`stage` の欄）と kind の字（`as_str` の値）で見分ける（末尾だけを `replay` に通すと、段を持たない行しか末尾に無い便が既定の段で生まれるので、便の段は段の欄を持つ行だけから読む）。歯の fixture の event は JSON の字を `from_line` で読んで作る。`Stage` の arm・`WaitReason` の変種の literal・`Turn` の literal は書かない（§12 の閉包と同じ）。
  - `EventKind` を touches に持つ行は dispatcher.md 行 a と行 ap の 2 本。行 a の write-set には行 c・d の `+` の file を前の docs PR で宣言したが、行 ap の write-set には無い。ap に `+` で足す形は着地の順で壊れる（行 c か d が先に着地すると ap の受付で `+` の file が既に在って断られ、ap が先に着地すると現物の表の検査が ap の閉包の欠けを赤にする）ので、名指さない形にした（2026-09-30 の probe: `EventKind` の match の arm を 1 本持つ書き手の file の木で、contracts check が行 ap の write-set-incomplete を名指した）。
  - 4 つの型は dispatcher.md 行 a・行 ap（`EventKind`）・contract-source.md 行 c・dispatcher.md 行 w・consumer-sync.md 行 g の touches に在り、本行の `+` の file が名指すとその行の閉包に入って、現物の契約表の閉包の検査（gate の共通の verify の歯 `contract_closure_ext_real_table_has_zero_findings` と同じ `contracts check`）が赤になる。器の閉包の検査が測るので verify に置く: 便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (8) の歯にする（歯の e2e を verify に置くと歯の file を write-set に載せて行どうしが交差で直列になるので、command の 1 行にする・dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた）。
- 歯:
  - lib（store の歯の区間・接頭辞 `store_read_after_`・4 本）: (4) 読みを数える包みで、10 MB と 20 MB の log（同じ 1 行目・同じ末尾）の読む byte が一致し、末尾の長さ + 1 と等しい (5) 繋がらない 3 形で読めない。
  - lib（行 d の write-set の `+` の file の歯の区間・接頭辞 `lifecycle_partial_`）: (2) `Absent` (3) 生きた pid の lock で 200 ms の後に `Busy`・死んだ pid の lock は外して書き直す (4) 開いた部品 200 本と窓の中の閉じた部品 700 本の出力に 10 MB と 20 MB の log を当て、読む byte・書いた中身（`generated_at` と `inputs.events.len` を除く）・`Unchanged` の判定が一致する (6) 発話の 3 形と逐語の不在・末尾より前に受けた発話に末尾で要望が足された fixture と、会話の後に要望が足された fixture と、末尾に線の在る fixture（末尾の線より前の発話は載らない）と線が末尾より前の fixture（末尾の発話が載る）で、発話の部品が log の全部を行 b の発話の関数に渡した結果と一致する・札の生きた Landed の便は ci-waiting で死んだ札の同じ便は landed-open（対）・request の仕分けで出力に在る memo の `links.source` に発話の ts が足される・便の段の移りと、末尾で新しく起きた便が同じ契約の前の便の部品を置き換え、閉じた契約の便は載らない（対）・期日の移り（理由 `trigger-met`・since が期日・`due` が次の期日）と、同じ期日の `keep` の在る memo と memo-asking の memo は動かない（対）・年齢の閾値を越えた部品が overdue true になり `owned.count` が増える・`scope` が partial で `inputs` の ledger と main と `full_at` は前の値のまま・契約の部品の局面と理由の不動 (7) 印を消さない・繋がらない 3 形のどれでも書かず `unreadable`（理由 `events`）の印。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/seat/tick.rs`・接頭辞 `seat_tick_rewrites_lifecycle_`・2 本）: (1) tick の周で期日の memo が移る歯と、便の局面が移る歯（どちらも tick の rc は 0 のままで、stdout と stderr は `lifecycle` の字を持たない・字を足す実装を落とす）。どちらも同じ歯の中で、出力の在る置き場の周の bd と git の shim の呼びの数が出力の無い置き場の周と等しい（撃ち 0）ことも測る（否定だけの歯を作らない・§1）。
  - 置き場と file ごとの base で RED の理由（§1）: `store_read_after_` は store.rs の test 区間（新しい末尾の読みを呼ぶので base で compile できない）。tick の e2e は base で出力が動かない。
  - base で RED の理由は機能不在: 歯の名が 0 本（rc 4）・tick の周で出力が動かない。
- 触らない: tick の合図と alarm・全部の書き直し（行 c）・印付け（行 e）・`read_all` とその呼び手・発話と仕分けの記帳の本体・`emit` の門（`NotStopped`）。
- 限界:
  - 契約の局面（queued → running など）は次の全部の書き直しまで遅れる（FR90 の部分の範囲の外）。dispatch の周の直後に全部を書き直すので、遅れは 1 周に収まる。
  - contract-running の理由（便の局面の語）も同じく次の全部の書き直しまで遅れ、その間は便の部品の局面と食い違いうる。
  - 末尾に段の行を持たない便の札の生死の変化（札が死んで ci-waiting が landed-open になる）は次の全部の書き直しまで遅れる。
  - 全部の書き直しが出力を書いた後、線を足す前に落ちた周は、線が log に無いまま部分の書き直しが末尾の発話を載せ、次の全部の書き直しが線を足した後の出力から消える。
  - tick 自身の他の読み（`read_all`）は log の大きさで伸びる。NFR5 の hook の予算には入らない。
  - 出力の大きさは窓の中の閉じた部品で決まる。窓の値を上げれば伸びる。
- 却下: 差分の file を別に持つ（組の 2 file を越え、ADR が要る）・lifecycle.stale を消す（ADR-0088 (3)）・契機を store の追記の口に置く（hook の記帳を含む全部の追記で撃ち、NFR5 を食う）・lock を待つ（hook の予算を食う）。

## 14. 門が通した周の古さの印（行 e）

やさしく言うと: 席が台帳を書く command と、器の便でない merge の command を門が通した時に、書き直さずに「出力は古い」の印を付ける。

- 何が起きているか（verified）:
  - hook の PreToolUse は `crates/scribe2/src/hook/mod.rs` で、Bash の周に command の門・起票の門・台帳のグラフの門・anchor の門・merge の門を判じ、その後に役割と live row を判じる（道具に依らない選択式の問いの門と write-set の guard が先）。最後の allow は live row の Pass の腕。門が通した後に局面の出力へ知らせる手は無い。
  - 起票の門の片の割り `segments` と片の読み `write_of` は pub で、bd の書きの subcommand の列 `WRITES` は crate の中から読める。`write_of` は bd か bdw の片なら読みだけの片（`bd --readonly list`・`bd show`）にも値を返すので、値を返すかでは書きを判じられない。
  - merge の門の `decide` は通すか断るかの 2 値で、merge でない command も通す。`gh pr merge` の片かの見分けは anchor の門の `is_pr_merge`（crate の中から呼べる）で、merge の門も同じ 1 本を呼ぶ。
  - anchor の門は `gh pr merge` の周に着地列の窓を読むので git を撃ち、台帳のグラフの門は閉じた 6 つの書きの周だけ bd を撃つ。
  - 器の便の merge と close は driver の process が撃つので、hook を通らない。
  - 印の読み（台帳の印・main の sha）と印を 1 つ足す関数は、行 c の印の読み手の file に hook の module から呼べる可視性で置かれる（§12 約束 5・名と署名と返りの 4 値もそこに在る）。本行は行 c だけに依る（便 s2-07l.738.38.9 の事前審査が、呼ぶ関数の名・署名・可視性・lock を取れない周と読めない周の返りがどこにも無いと名指した）。
- 約束（番号は done の (1)〜(7) と 1:1・done (8) は閉包の約束）:
  1. 印を付けるのは、hook の PreToolUse の Bash の道が最後に allow と決めた周だけ。`crates/scribe2/src/hook/mod.rs` の allow の出口に呼び出しを 1 行置き、判じは行 e の write-set の `+` の file（`crates/scribe2/src/hook/stale_gate.rs`）に置く。どこかの門が断った周は付けない。
     - 呼び出しの置き場（allow の出口の 1 行と子 module）は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。
  2. ledger-gate: 片の割り（`segments`）と片の読み（`write_of`）で、subcommand が書きの列（`WRITES`）に在る片を 1 つ以上持つ command に付ける（`write_of` が値を返すかでは判じない）。印の値は行 c の台帳の印の読み（§12 約束 1・5 の hook から呼べる口）の書きの前の値。
  3. merge-gate: 片の割り（`segments`）と anchor の門の見分け（`is_pr_merge`・merge の門が呼ぶのと同じ 1 本）で `gh pr merge` の片を持つ command に付ける（2 本目の見分けを書かない）。印の値は行 c の main の読み（§12 約束 1・5 の hook から呼べる口・git を撃たない・worktree の common dir を含む）の sha。
     - 約束 2・3 の片の割りと見分けの関数の名指し（`segments`・`write_of`・`is_pr_merge` の同じ 1 本）は挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。挙動は歯 (2)(3) の読みだけの bd・`gh pr view`・2 種の片を持つ command が測る。
  4. 出力（`lifecycle.json`）の無い置き場では付けない。
  5. 書くのは行 c の印を 1 つ足す関数 1 本（§12 約束 5・`lifecycle.stale.lock` と一時 file の rename・種類ごとに 1 つ・後の印が置き換え）。返りの閉じた 4 値のうち付けた以外（出力が無い・lock を取れない・読めない）は、どれも allow を変えずに印を付けない（約束 4・7）。書き直しは撃たない。
     - 書くのが行 c の印の関数 1 本であることは挙動に差が出ないので done に載せない（便の diff の設計適合は gate の審査で見る）。
  6. 予算（NFR5）: 足す読みは metadata.json と manifest か ref の file と、lifecycle.json の stat と、lifecycle.stale の読み書きだけで、印付けが撃つ子 process は 0 本（門が撃つ git と bd は変えない）。
  7. 印を付けられない周（値を読めない・stale の lock を取れない）は allow を変えずに付けない（fail-open）。
- 閉包: 本行の `+` の file は起票の門の片の読みが返す値の subcommand と書きの列だけを使い、起票の門と merge の門の断りの型の変種を名指さない。
  - 起票の門の断りの型 `Refusal` と片の読みが返す `Write` は ledger-form.md 行 g（`Refusal` は vessel-hook.md 行 a も）の touches に在り、本行の `+` の file が literal・arm・変種で名指すとその行の閉包に入って、現物の契約表の閉包の検査（gate の共通の verify の歯 `contract_closure_ext_real_table_has_zero_findings` と同じ `contracts check`）が赤になる。器の閉包の検査が測るので verify に置く: 便の木で `contracts check` を撃つ 1 行を verify の最終行に置き、done (8) の歯にする（歯の e2e を verify に置くと歯の file を write-set に載せて行どうしが交差で直列になるので、command の 1 行にする・dispatcher.md 行 al の 2 本目の便が同じ形の約束を破って gate で落ちた）。merge の門の断りの型は touches に無いので done に載せない。
- 歯:
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/hook.rs`・接頭辞 `hook_stale_mark_`・bd と git の shim は呼びを file へ記す）: (1)(2) bdw の update が通った周に ledger-gate の印が付き値が fixture の manifest の印・断られた書きと読みだけの bd（`bd --readonly list`・`bd show`）では付かない（`write_of` が値を返すかで判じる実装を落とす） (3) `gh pr merge` が通った周に merge-gate の印が付き値が fixture の loose の ref（packed-refs だけの fixture と worktree の fixture でも同じ）・台帳の書きと `gh pr merge` の片を両方持つ command では 2 種の印が付き、`gh pr view` では付かない (4) 出力の無い置き場では付かず、同じ置き場に出力を作った後の同じ command では付く (5) 2 度の書きで印が 1 つ・2 度目の値・印を付けた周に `lifecycle.json` の bytes が変わらない（書き直しを撃たない）・印の後の manifest を動かさない `fleet lifecycle write` では消えず、manifest の chunks を進めた後の同じ口で消える（対）・merge の印は event log だけ進めた後の `fleet lifecycle write` では消えず、main を印の sha の子へ進めた後の同じ口で消える（対）。印の後の部分の書き直しで消えないことは行 d の歯 (7) が、印より前に始まった全部の書き直しで消えないことは行 c の歯 (6) が測る（本行は行 c だけに依る） (6) 印が付いた周の bd と git の shim の呼びの数が、出力の無い置き場の同じ command の周と等しい（印付けが撃つのは 0・anchor の門が窓の読みで撃つ git は両方に在る・(1)〜(3) の歯の中で測る） (7) stale の lock を持った周と、manifest を壊して台帳の印を読めない置き場の周は、allow が変わらず印も付かず、lock を外した後と manifest を直した後の同じ command では付く。
  - 否定だけの歯を作らない（§1）: 「付かない」「呼びが 0」「allow が変わらない」は、どれも印が付く肯定と同じ歯に置く。
  - base で RED の理由は機能不在: e2e の hook.rs はどの歯も印が付く肯定を持ち、base では付かない。
- 触らない: 門の判定と断りの字（起票の門・anchor の門・merge の門の file は呼ばれるだけで 1 行も変えない）・印を消す条件（行 c）・読み手の比べ（§15）・host-guard。
- 限界:
  - 消費側の面の server が bdw で書く台帳の書きは席の道具の呼び出しでないので、印も付かず書き直しも起きない。手当ては行 c の書き直しの口と、読み手の台帳の印の比べ（§15）。
  - 書きが落ちた周の印は、次に台帳か main が動くまで残る（FR90）。
  - reftable の repo は main を読めない周として印を付けない（約束 7）。
- 却下: 印の周に書き直す（hook の中で台帳と git を撃つ・NFR5）・PostToolUse で書きの後に付ける（FR90 は「門が通したとき」）・起票の門の file の中に置く（余地が無い）・merge の門に `gh pr merge` の 2 本目の見分けを足す（`is_pr_merge` が在る）。

## 15. 読み手ごとに比べる印の組（FR94・読み手の行が執行）

やさしく言うと: 出力を読む 7 つの面が、自分で安く読める今の印と出力の印を比べて、古い出力を「古い」と出すための表。行は dispatcher・seat-heartbeat・ledger-form の後の行が持つ。

| 読み手 | 比べる印 | 古いときの見せ方 |
|---|---|---|
| 便の終端の周の通知の 1 語 | event log の長さ | 語に `:stale` を添える |
| doctor の 3 行（未仕分けの発話・処置の待ちの memo・席の手番の閾値越え） | 台帳・event log・main | 行に `stale` の語を添える |
| 管理 tick の alarm の語 unsorted と owned | event log の長さと古さの印 | 閉じた 1 語 stale を添える（FR27） |
| 直近の流れの事実行 | 台帳と main | 行に `stale` の語を添える |
| memo の判定の通知の処置の待ちの memo の数 | 台帳 | 数に `:stale` を添える |

- 古さの印が 1 つでも在る周は、印の組に依らず古いと示す。
- 出力が無いか読めない周は `unreadable` と示し、0 件と書かない。
- 全部の書き直しを、終端の周の知らせより前に撃つ（§12 約束 8 (d)）。
- 例外: 出力の file が無い置き場の終端の行は送らない — [dispatcher.md](./dispatcher.md) §43 行 ar。

## 16. 台帳の印の読み手を実物の manifest の形に合わせる（契約表の行 f・memo s2-07l.738.38.11）

やさしく言うと: 台帳が変わったかを安く知るために、器は台帳の store の目次の file（manifest）を 1 行だけ読む。その 1 行の欄の数え方が 1 つずれていて、実物の store では「読めない」になっていた。局面の出力と古さの印は、そのせいで実物の repo で働いていない。欄の位置を実物に合わせ、歯の見本も実物の形に直す。

- 何が起きているか（main 610411be・verified）:
  - `crates/scribe2/src/fleet/lifecycle_mark.rs` の `noms_of` は manifest を `:` で割り、5 つ目を root・6 つ目を gc の世代・7 つ目から先を「file 名:chunk 数」の組と読む。
  - 本 repo の bd 1.1.0 の embedded の store の manifest は `5:__DOLT__:<lock>:<root>:<gc の世代>:<file 名>:<chunk 数>:…` の形で、root は 4 つ目、gc の世代は 5 つ目、組は 6 つ目から始まる。
  - そのため実物では組の数の読み（数でない字を数として読む）で崩れて `None` を返し、`read_ledger` は台帳の印を読めない。局面の出力の書き手（`crates/scribe2/src/fleet/lifecycle.rs` の gather）と印の読み（read_marks）は、最初に ledger の語で止まる。置き場の fleet の下に lifecycle.json は無い。
  - 歯の見本は実物に無い語 `nbs` を 1 つ多く持つ（`5:nbs:__DOLT__:lock:rootA:gc0:…`・lifecycle_mark.rs の in-file の歯と e2e の hook.rs の helper `stale_manifest`）ので、歯は緑のまま通っていた。
- 約束（番号は done と 1:1）:
  1. `noms_of` は 4 つ目を root、5 つ目を gc の世代、6 つ目から先を「file 名:chunk 数」の組と読む。印の値の形（`Ledger::Noms` の欄）と gen・chunks の数え方は変えない。
  2. 2 つ目が `__DOLT__` でない manifest は読めない（`None`）とする（形の違う store を別の位置で読まない・fail-closed）。欄が足りない・組が奇数・数でない・root が空の形も、今のとおり読めない。この判定の歯は、2 つ目だけを別の版の語（例 `__LD_1__`）に替え、ほかの欄は読める形のままの見本で測る（旧い見本の `nbs` を挟んだ形は直した位置で組の欄が奇数になり、判定を外しても None になるので、この判定の歯にならない）。
  3. 歯の見本を実物の形（`5:__DOLT__:lock:rootA:gc0:…`）に直す。既存の 3 形（journal だけ・table つき・gc の世代違い）は同じ root・gen・chunks を返す。
  4. 門が通した周の古さの印の歯（§14 行 e）の helper の manifest も実物の形に直し、歯は緑のまま。
- 歯: lib は `crates/scribe2/src/fleet/lifecycle_mark.rs` の既存の歯の区間・接頭辞 `lifecycle_mark_ledger_`（既存 2 本の見本を直し、実物の 1 行の形の歯を 1 本足す）。e2e は `crates/scribe2-boundary/tests/e2e/hook.rs` の `hook_stale_mark_`（helper を直すだけ）。直した期待は base で落ちる（base は 5 つ目を root と読むので、実物の形で root が違うか読めない）ので、retroactive の札は要らない。
- 触らない: `Ledger` の型と欄・`ledger_order`・`files_of`・`read_events`・`read_main`・局面の出力の書き手と読み手・古さの印の門。
- 限界: bd の版が manifest の形を変えた周は読めない（ledger の語で止まる）。形の違いは読みの失敗として見え、別の位置の値を印として使わない。
- 却下: 欄の位置を `__DOLT__` の語の位置から相対に読む案（形の違う版を推測で読む。読めない形は止める方が C10 に合う）。

## 17. 局面の導出に未反映の問いと処置の無い判定の memo を渡す（契約表の行 g・memo s2-07l.738.38.10）

やさしく言うと: 局面を決める関数（§7）は、「文書に写されていない裁定を持つ閉じた問い」と「審査役が上げる・閉じると判じたのに、席がまだ何もしていない memo」の 2 つの一覧を受け、問いを ruling-unreflected に、memo を memo-actionable（理由 verdict）に置く。ところが出力を書く側（§12）は 2 つとも空の一覧を渡していて、この 2 つは出力に 1 度も出ない。書く側が、未反映の裁定の置き場・event log の審査の判定・台帳から 2 つの一覧を組んで渡す。

- 何が起きているか（main 4c6fe0d7）:
  - verified:
    - 書き手 `crates/scribe2/src/fleet/lifecycle.rs` の derive（684〜736 行）は、行 a1 の導出（`crates/scribe2/src/ledger/phase.rs` の derive）の入力 unreflected と unjudged を `&[]` の固定で渡す（697・698 行）。
    - 導出の側は 2 つの列を受ける形を持つ。unreflected は閉じて未反映の裁定を持つ問いの bead id で（phase.rs 83〜84 行）、閉じた問いの id がこの列に在れば ruling-unreflected に置く（354〜355 行・窓より前に判じる）。unjudged は処置の無い判定を持つ memo の bead id で（85〜86 行）、memo-actionable の理由 verdict（510〜511 行）と、FR93 の条件 `close_due` の「処置の無い判定が無い」（114〜116 行）に効く。行 a1 の歯 phase_ledger_ は、2 つの列を渡した fixture で局面を測っている（phase.rs 868〜959 行）。
    - 未反映の裁定の置き場（`<state_dir>/pipe/unreflected`・FR84）は、問いの id でなく裁定 id の列を持つ（`crates/scribe2/src/pipe/dispatch/unreflected.rs` 44〜53 行）。外から読める読みは 3 本で、どれも id の列を返さない: `involved`（関わる契約の bead → id の表・着地の留め）・`count`（件数・管理 tick）・`doctor_line`（doctor の 1 行）。file の読み（106 行の stored と 82 行の judged_of）は私有。
    - 置き場を書くのは起こす側の周（`fire`）だけで（`crates/scribe2/src/pipe/dispatch.rs` 641〜643 行）、同じ周の全部の書き直し（契機 (a)）はその後に撃つ（686〜687 行）。観測の 1 周（`observe_round`・契機 (d)(e)）は未反映を判じるが、返すのは台帳の全件と列の判定だけで（`crates/scribe2/src/pipe/dispatch/candidates.rs` 388〜405 行）、置き場を書かない。
    - FR84 の母集団（閉じた・effect が document の・label の問い）は dispatch.rs 566〜571 行が持ち、effect の字は dispatch.rs 85 行の私有の const。問いの notes から裁定の行の id を読むのは unreflected.rs の judge（137〜141 行・close の理由の読みの `ruling_row`）。
    - memo の審査の判定は、`crates/scribe2/src/pipe/dispatch/memo_lens.rs` の record（172〜203 行）が置き場の verdict の file を書いた後に、event MemoJudged を 1 行足す（bead＝memo の id・detail＝判定の語・ts＝記帳の時刻）。event log の読みはこの行を本体 `Case::Judged` と bead と detail で読む（`crates/scribe2/src/fleet/event.rs` 509〜516 行）。判定の語は閉じた 4 値の `Word`（promote・close・keep・unparsed・`crates/scribe2/src/pipe/dispatch/memo.rs` 47〜71 行）。
    - 判定の後に処置が付いたかを示す記録は、event log にも置き場にも無い。台帳の `Issue` は 13 欄で、bd の JSON の updated_at を読まない（`crates/scribe2/src/seat/ledger.rs` 69〜96 行・`issues_of` の 110〜138 行）。`Issue` を字で組む所は 4 か所: `issues_of`・`crates/scribe2/src/hook/graph_guard.rs` 607 行（src）・歯の fixture の `crates/scribe2/src/ledger/form.rs` 357 行と `crates/scribe2/src/pipe/dispatch/precheck.rs` 603 行。
    - 本 repo の台帳（bd 1.1.0・2026-10-01・988 本・読むだけ）で、updated_at は全部 UTC の秒までの Z の形で、同じ値を持つ bead は 0 組（全件を一度に動かす書きは無い）。閉じた 896 本のうち 821 本は updated_at が closed_at と同じで、75 本は閉じた後の書きで closed_at より後（書きが updated_at を進める）。discovered-from で memo を指す契約の起票は memo の updated_at を動かさない（48 組のうち 4 組で memo の updated_at が契約の作られた時刻より前）。
    - 層の向き: fleet の書き手は既に pipe を呼ぶ（lifecycle.rs 624 行の `observe_round`・`crates/scribe2/src/fleet/lifecycle_mark.rs` 24〜27 行）。dispatch の子 module の memo と unreflected は pub（dispatch.rs 60・66 行）。
    - 行数: lifecycle.rs は幅で数えて 1425 行（R-C4-2 の 1500 まで 75 行）で、derive は 53 行（R-C4-4 の 60 行まで 7 行）。lifecycle_mark.rs は 1131 行で、全部の書き直しの入力を event log から組む純関数（`bindings_of`・`refusals_of`）を test 区間の前の末尾の区間に持つ。
  - deduced: このため doctor の lifecycle-memo の actionable（ledger-form.md 行 o）・管理 tick の alarm の owned・通知の memos=（dispatcher.md 行 ar）は、処置の無い判定の memo と未反映の裁定の問いを数えない（ledger-form.md §19 の限界が名指す）。
- 約束（番号は done と 1:1）:
  1. 未反映の問い: 全部の書き直しは、置き場の裁定 id の列を、閉じた台帳の問い（label の問い）のうち notes の裁定の行にその id を持つ問いの id の列へ引いて、行 a1 の導出の unreflected に渡す。
     - 引きは unreflected.rs に足す読み 1 本が持つ。入力は置き場・台帳の接頭辞・閉じた問いの id と notes の組の列（unreflected.rs は台帳の 1 件の型を名指さない今の形のまま）、返りは閉じた 2 値（読めない・問いの id の列〔入力の順〕）。置き場の file の読みは私有の読みを、notes の裁定の行の読みは judge と同じ 1 か所を使い、写しを持たない。置き場の無い周は空の列。
     - 母集団の effect は判じ直さない（置き場の列は FR84 の母集団の裁定 id だけを持つ）。
     - 契機 (a) は同じ周の `fire` が書いた置き場を、契機 (d)(e) は前の起こす側の周が書いた置き場を読む。観測の 1 周の判定は使わない（§2 の表の「置き場に在る」と、doctor・管理 tick・着地の留めが読むのと同じ列）。
  2. 置き場が在って読めない周は、書き直しを止めず、問いの列を空で渡し、`unmeasured` に part question・reason unreflected-unreadable を 1 件名指す。§5.2 の `unmeasured` の reason の閉じた語に unreflected-unreadable を足す（同じ docs PR で §5.2 の列を直す・語を足すだけで版は上げない・§5.1）。
  3. 処置の無い判定: 全部の書き直しは、開いた memo（memo の label を持ち閉じていない）のうち、event log の最後の判定の行（本体が `Case::Judged` で bead がその memo の行の最後の 1 行）の語が promote か close で、台帳の updated_at がその行の ts より後でない memo の id の列（台帳の順）を、行 a1 の導出の unjudged に渡す。
     - 最後の判定の語が keep か unparsed の memo・判定の後に台帳が書いた memo（updated_at がその ts より後）・閉じた memo・判定の行の無い memo は渡さない。updated_at が無いか読めない memo は渡す（処置を測れない周に判定を落とさない）。
     - 処置（FR87 の 4 種: 昇格・close・引き金の書き直し・keep の記帳）は、どれも台帳のその memo への書きで付くので、「判定の後にその memo が書かれた」を処置の印に読む（過大の向きは限界）。
     - 語は `Word` の字で比べ、字を写さない。判定は event log（FR90 の入力）だけから読み、置き場の verdict の file を memo ごとに読まない。
  4. `Issue` に updated_at（字の `Option`）を足し、`issues_of` が読む。無い要素は None で、ほかの欄の読みは変えない。組みの 4 か所を直す（構築点は便の始めに今の main で数え直す）。
- 設計の線（歯を持たない・審査が読む）:
  - 2 つの列の組み立ては lifecycle_mark.rs の末尾の区間（全部の書き直しの入力の読み・test 区間の前）に置く。lifecycle.rs は gather で組んで全部の書き直しの世界の欄に持ち、derive は渡すだけにする（derive の余地は 7 行）。読めない置き場の `unmeasured` の 1 件も gather の側で作る。
  - 後の行が呼ぶ口（memo の自動の close〔FR93・ledger-form の後の行〕が land の終端の経路から `close_due` へ同じ列を渡す）: 処置の無い判定の組み立て 1 本は台帳の全件と event の列を受けて memo の id の列を返す純関数で、未反映の問いの組み立て 1 本は置き場・接頭辞・台帳の全件を受けて閉じた 2 値（読めない・問いの id の列）を返す。どちらも lifecycle_mark.rs に crate の中から呼べる可視性で置く（呼べることは呼び手の行の compile が測る）。
  - 新しい code は `EventKind` を名指さず（MemoJudged は本体 `Case::Judged` で見分ける・§12 の閉包と同じ）、`Issue` を literal で組まない（歯の fixture は bd の JSON の字を `issues_of` で、event は JSON の字を `Event::from_line` で読む）。`EventKind`（dispatcher.md 行 a・行 ap）と `Issue`（contract-source.md 行 bo）は touches に在り、器の閉包の検査が測るので、verify の最終行に contracts check を置く。
  - 行 a1 の導出（phase.rs）は変えない。
- 歯（e2e の部品の行はどれも局面・手番・理由の 3 つを測る）:
  - lib（lifecycle_mark.rs の既存の test 区間・接頭辞 unreflected_questions_）: (1)(2) 置き場の無い置き場で空の列・読めない置き場（JSON でない字）で読めない・読める置き場（未反映 1 つと写った 1 つの母集団）で、未反映の id の裁定の行を持つ閉じた問いだけが列に在る。写った id の行を持つ閉じた問い・notes の散文にだけその id を書く閉じた問い・同じ行を持つ開いた問い・問いの label の無い閉じた bead は無い。列の全体を等しさで比べる。
  - lib（同じ test 区間・接頭辞 verdict_unhandled_）: (3) 最後の判定が promote と close の memo は在り、keep と unparsed の memo は無い／同じ memo の判定が promote → keep の順なら無く、keep → promote の順なら在る／updated_at が判定の ts と同じ memo は在り、1 秒後の memo は無い（対）／updated_at の欄の無い memo は在る／閉じた memo と memo でない bead の判定は無い。列の全体を等しさで比べる（値の slice への contains にしない）。
  - lib（`crates/scribe2/src/seat/ledger.rs` の既存の test 区間・接頭辞 issue_updated_at_・1 本）: (4) updated_at を字のまま読み、要素の無い bead は None で、同じ bead の id・status・created_at・closed_at の読みは変わらない（1 本の中で在る bead と無い bead の対）。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/fleet.rs`・接頭辞 fleet_lifecycle_feeds_・2 本・§12 の偽の台帳と toy repo）:
    - (1)(3) 偽の台帳に、裁定の行を持つ effect が document の閉じた問い（閉じた時刻は窓より古い）と、effect が operation の閉じた問いと、満ちない期日の引き金の行を持つ開いた memo 3 つを置き、event log に MemoJudged を 3 行書く（promote・keep・promote の後に updated_at の進んだ memo）。書き直しの口（契機 (e)）を先に撃つと、置き場が無いので document の問いの部品は無く（窓より古い終わりの局面）、promote の memo は memo-actionable・理由 verdict・手番 seat。`pipe dispatch`（契機 (a)）の後は、document の問いが ruling-unreflected・手番 seat で載り（窓に依らない・AC60）、operation の問いは question-closed、keep の memo と処置の後の memo は memo-waiting。出力を消して書き直しの口をもう 1 度撃つと、前の周の置き場から同じ局面が出る。
    - (2) 起こす側の周の後に置き場の file を読めない字へ書き換え、出力を消してから書き直しの口を撃つと（出力を残すと印が同じで Coalesced になり書き直さない）、rc 0 で、lifecycle.json の `unmeasured` に question と unreflected-unreadable の 1 件が在り、document の問いの部品は無い。次の `pipe dispatch` の後は名指しが消えて ruling-unreflected に戻る（同じ歯の中の対）。
  - 置き場と file ごとの base で RED の理由（§1）: lib の 2 つは lifecycle_mark.rs の test 区間（新しい組み立てを呼ぶので base で compile できない）、issue_updated_at_ は seat/ledger.rs の test 区間（`Issue` の新しい欄を読むので同じ）。e2e の 2 本は、base の書き手が 2 つの列を空で渡し unreflected-unreadable の語を持たないので、ruling-unreflected・verdict・unreflected-unreadable の肯定で落ちる（期待が base の振る舞いと違う）。組みを直す 3 file（form.rs・precheck.rs・graph_guard.rs）は `Issue` の新しい欄を書くだけで、歯の本文を変えない。
  - base で RED: lib は歯の名が 0 本（rc 4・機能不在）、e2e は局面が base と違う。既存の phase_ledger_・lifecycle_writer_・lifecycle_mark_・fleet_lifecycle_ と seat/ledger.rs の issue_times_ は期待を変えずに緑（非回帰）。
  - 接頭辞の衝突（main 4c6fe0d7 で数えた）: 4 つの接頭辞を持つ既存の歯は 0 本。全部の契約表の verify の filter のうち 4 つの接頭辞の歯の名に当たるのは行 c の fleet_lifecycle_ だけで、fleet.rs は行 c の write-set に在る。
- 触らない:
  - 行 a1 の導出（phase.rs の derive・`close_due`・入力の形・判定の順・理由の語）。
  - 未反映の判定の振る舞いと置き場の書き（judge は裁定の行の読みを 1 か所に寄せるだけ・FR84 の母集団）と、置き場の今の 3 つの読み（`involved`・`count`・`doctor_line`）。
  - memo の審査（memo_lens.rs の材料・verdict の file・MemoJudged の記帳）と判定の読み（memo.rs の `judgement`）。
  - event の kind と key（event.rs）・列の判定（dispatch.rs）・部分の書き直し（lifecycle_partial.rs）・読み手（lifecycle_read.rs）。
  - 出力の text の字と JSON の欄の形（`unmeasured` の語を 1 つ足すだけ）。
- 限界:
  - 処置は「判定の後に台帳がその memo を書いた」の印で読むので、処置でない書き（[再発] の行・label・priority）も処置と読む（過大）。その memo は判定から外れ、審査の間隔（rules 行 memo.triage_interval_h）の後の審査（dispatcher.md 行 aq）が同じ判定を返せば戻る。処置でない書きで外さない形は、書きの種類を残す仕組みが要り、本行は持たない。
  - lens の撃ち中（材料を書いた後・判定の記帳の前）に席が付けた処置は、updated_at が判定の ts より前なので処置と読まない（過小）。判定は出続け、席が次にその memo を書くと外れる。
  - updated_at が無いか形の違う bd では処置を測れず、判定は処置の後も出続ける（黙って落とす側へは倒さない）。
  - 判定の event を記帳できず verdict の file だけが在る周（record が event の追記で落ちた周）は判定を渡さない（判定の出所は event log）。
  - 契機 (d)(e) は前の起こす側の周の置き場を読むので、main が裁定を写した直後の周は、次の dispatch の周まで ruling-unreflected が残りうる。
  - 置き場の無い置き場（その state dir で起こす側の周が 1 度も回っていない・実装役の口の無い周は列を測らない）は未反映を空と読み、`unmeasured` に名指さない（doctor の行・管理 tick の数え・着地の留めと同じ読み）。
  - 部分の書き直し（行 d）は判定も置き場も読まない。tick の周に足された MemoJudged は、次の全部の書き直しまで出ない（FR90 の部分の範囲の外）。
  - 閉じが行 a2 の misfit に当たる問いは、§12 の置き換えで misfit が勝つ。
  - 同じ裁定 id の裁定の行を別の閉じた問いの notes が写していれば、その問いも ruling-unreflected に出る（effect を判じ直さない）。
- 却下:
  - 処置を、置き場の審査の材料（material）の字と今の memo の字の比べで読む案。材料は審査を撃つたびに判定より先に書き直され、口座の候補が無く判定を書かない周にも書き換わる（memo_lens.rs の judge）。処置の後の字を判定の時の字と読み、処置済みの判定を戻して席の手番に残し続ける（過小）。材料は lens の入力で、跨版の約束を持たない。
  - 処置を台帳の今の状態だけで読む案（keep の行・昇格の行・引き金の行が在れば処置済み）。判定より前の keep の記帳や一部の昇格の行を処置と読み、判定を黙って落とす（ADR-0089 の DR1）。
  - 前の出力の memo の triggers と keep を、判定の時の写しとして比べる案。出力を状態の置き場にし、出力の無い周と読めない周に判定が消える（ADR-0088 の導出物）。
  - 判定を verdict の file（memo.rs の `judgement`）で memo ごとに読む案。FR90 の入力の外の file を memo の数だけ読む。event log の MemoJudged が同じ語を同じ記帳の中で持つ。
  - 観測の 1 周が判じた未反映を `Observed` と書き手の `Round` に載せて渡す案。§2 の表の「置き場に在る」と、doctor・管理 tick・着地の留めが読む列と食い違いうるうえ、candidates.rs の構造体と構築点を write-set に足す。
  - 書き手が FR84 の数え（citation.rs の cited_at）で未反映を判じ直す案。FR90 の入力の外の main の追跡された file の全部を読み、dispatch の周の判定と 2 本になる（C2）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "局面の型と手番の表と共用の読み手 — §2 の 38 語と 9 種類と 6 手番と misfit の 15 語の型・語から手番の表・引き金の満ちの純関数 met・昇格の行の読み手・台帳の時刻 2 欄（case-lifecycle §6・FR90 / FR91 / FR87）"
req = ["FR90", "FR91", "FR87"]
section = "6"
write-set = ["+crates/scribe2/src/case/mod.rs", "+crates/scribe2/src/ledger/promotion.rs", "crates/scribe2/src/ledger/trigger.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/hook/graph_guard.rs", "crates/scribe2/src/lib.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail phase_table_", "cargo nextest run -p scribe2 --lib --no-tests=fail promotion_line_", "cargo nextest run -p scribe2 --lib --no-tests=fail trigger_met_", "cargo nextest run -p scribe2 --lib --no-tests=fail issue_times_"]
size = "M"
growth = ["crates/scribe2/src/case/mod.rs:600", "crates/scribe2/src/ledger/promotion.rs:185", "crates/scribe2/src/ledger/trigger.rs:150", "crates/scribe2/src/seat/ledger.rs:30", "crates/scribe2/src/ledger/mod.rs:1", "crates/scribe2/src/lib.rs:1", "crates/scribe2/src/pipe/dispatch/precheck.rs:2", "crates/scribe2/src/ledger/form.rs:2", "crates/scribe2/src/hook/graph_guard.rs:2"]
done = "(1) case の新しい module が §2 の 38 語（宣言順）・9 種類・6 手番・§4 の misfit の 15 語・§5.2 の部品の型と、共通の欄の 9 key と links の 8 key の const の列を持ち、字は表と 1 字も違わず、lib.rs に 1 行・歯の module は file の末尾 (2) 語から手番の 1 関数が §3 の表を網羅の match で持ち、contract-queued の理由の表は WAIT_REASONS の 8 語と unreflected-ruling・floor を含み、表に無い語は None (3) trigger.rs の純関数 met が 5 形の満ちを世界から判じる（同梱は印を外した等しさと dir の前方一致） (4) 昇格の行の読み手が 全部 / 一部 の 2 形を読み、最後の行が勝ち、読めない行は字と理由を持ち、行頭でない行と , を区切りと読まない (5) Issue が created_at と closed_at を Option で持ち、issues_of が読み、無い要素は None で、組みの 4 か所（便の始めに数え直す）を直す 歯: phase_table_ 9（(a) は種類の名を頭に持たない 2 語〔ruling-unreflected・misfit〕を除く 36 語の接頭辞・(g) は key の列・(h) 種類・(i) §3 の表の全行）・promotion_line_ 7（最後の行が読めない形）・trigger_met_ 8（境界と / の無い同梱・trigger.rs の test 区間）・issue_times_ 2（seat/ledger.rs の test 区間）が base で 0 本（rc 4・機能不在）、既存の ledger_trigger_・seat_ledger_・precheck_intake_ は期待を変えずに緑"

[[contract]]
id = "a1"
title = "台帳の側の部品 — question・memo・epic と閉じた contract の局面・手番・理由・結びを 1 つの純関数で導き、形の misfit と閉じの misfit 5 語を線の規則で数え、FR93 の条件を 1 関数に持つ（case-lifecycle §7・FR90 / FR91 / FR93）"
req = ["FR90", "FR91", "FR93"]
section = "7"
depends = ["a"]
write-set = ["+crates/scribe2/src/ledger/phase.rs", "crates/scribe2/src/ledger/mod.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail phase_ledger_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/ledger/phase.rs:1000", "crates/scribe2/src/ledger/mod.rs:220"]
done = "(1) ledger の新しい module の純関数 1 本が台帳・接頭辞・時刻・窓・2 つの線・未反映の id・処置の無い判定の id・開いた契約の write-set を受け、question・memo・epic と閉じた contract の部品と misfit を返し、開いた契約は返さない (2) 種類の判定は §2 の順、memo は form-both → promoting → asking → no-trigger → actionable（理由 4 語）→ waiting（keep・ほかの全部）の順で、辿れる契約は contract の種類の discovered-from だけ・子の問いは parent-child だけを数え、行 a1 の部品は no-phase に落ちない (3) FR93 の条件の 1 関数が 5 つの条件を持つ (4) 問いの 3 局面と links (5) epic の 3 局面 (6) 閉じの 5 語を 2 つの線の両方より後の閉じだけに判じ、ほかは *-closed、接頭辞が None なら unmeasured に ledger-prefix (7) since は導ける 8 つの部品の値だけで、ほかは None (8) memo の due・triggers・keep と閉じた契約の pointer 歯: phase_ledger_（入力の不足・局面と手番と理由・種類の重なりと memo の局面と上の勝ち・form-both と no-trigger の位置の 3 本・数えの外の 2 本・FR93 の関数の直の呼び出しと条件の欠け・問いと links・子の無い epic・閉じの 5 語〔close-kind-mismatch 4 形・close-unresolved 2 形・merged-into-not-open 2 形〕と見送りの崩れと 2 つの線の前後と close-check が None・since の 8 つ・欄と次の期日）が base で 0 本（rc 4・機能不在） (9) 歯の fixture は Issue を literal で組まず JSON の字から issues_of で作り、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "a2"
title = "裁定の閉じの misfit 3 語 — 裁定と見送りの値が FR83 の解け方で解けない・結んだ裁定 id に無い・子の問いの裁定でない閉じを線の後だけ数える純関数（case-lifecycle §8・FR90 / FR91 / FR83）"
req = ["FR90", "FR91", "FR83"]
section = "8"
depends = ["a1"]
write-set = ["+crates/scribe2/src/ledger/phase_ruling.rs", "crates/scribe2/src/ledger/mod.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail phase_ruling_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
growth = ["crates/scribe2/src/ledger/phase_ruling.rs:260", "crates/scribe2/src/ledger/mod.rs:30"]
done = "(1) ledger の新しい module の純関数 1 本が台帳の全件・接頭辞・2 つの線・裁定 event の結びを受け、misfit の (bead id・語) の列を 1 つの閉じに 1 語まで §4 の表の順で返す (2) 閉じた問いの裁定と閉じた memo の見送りの値が解けないか古い形なら close-ruling-unresolved、種類と頭の食い違う閉じは判じない (3) 自分の notes の裁定の行（行 ak の読み）にも裁定 event にも無い裁定 id の問いが close-ruling-not-bound (4) parent-child の子の問いに結んだ裁定でない見送りが deferred-not-child-ruling (5) 線の規則は行 a1 と同じで、接頭辞が None なら空の列 歯: phase_ruling_（1 つの閉じに 1 語の 2 形・食い違う閉じの 0 件・notes と event の各経路の 0 件・discovered-from の問いと子の問いの対・線と close-check と接頭辞の前後の対・(2)〜(4) は解けて結ばれた対照の 0 件と組）が base で 0 本（rc 4・機能不在） (6) 歯の fixture は Issue を literal で組まず JSON の字から issues_of で作り、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "b"
title = "便と列と発話の側の部品 — 開いた契約の局面を列の判定と札の生死から 1 語で出し、便の段を Stage の網羅で写す関数を別に持ち、発話の開きと仕分けを event と bind の結びから導く（case-lifecycle §9・FR90 / FR88）"
req = ["FR90", "FR88"]
section = "9"
depends = ["a"]
write-set = ["+crates/scribe2/src/fleet/phase.rs", "crates/scribe2/src/fleet/mod.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail phase_event_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/fleet/phase.rs:800", "crates/scribe2/src/fleet/mod.rs:30"]
done = "(1) fleet の新しい module の純関数 1 本が列の判定（理由の名と値の字）・開いた契約（pointer の有無）・bead ごとの最新の便と便の対応・受付の断り（名と ts）・発話と仕分けと bind の結びの event・線と時刻と窓を受け、開いた契約と便と発話の部品を返す (2) 開いた契約は running（理由は便の語・手番 none）→ refused（理由は admission の断りの名か便の後に起きていない断りの名）→ queued（理由 8 語・§3 の表と links.on・overlap は相手の便の bead）→ no-phase、pointer の無い契約と閉じた契約の便は部品にしない (3) 便の段の写しは段の値だけを受ける別の純関数が STAGES の全部を §2.1 の語と手番に写す (4) 発話は線より後だけで、仕分け済みかと行き先は会話だけは chat・結びありは memo と裁定の全部で chat を持たない、sorted に窓・open は seat・逐語なし 歯: phase_event_（上の勝ちと refused が queued より上・理由 8 語の手番・overlap の bead の引きと引けない run id・列に無い契約の no-phase・pointer の無い契約と閉じた契約の便の不載・発話の行き先の全部と会話の外し・id と since を含む）が base で 0 本（rc 4・機能不在）、fixture の event は JSON の字を from_line で読む (5) fleet の新しい module phase.rs は WaitReason と EventKind を名指さず、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "b1"
title = "main の側の部品 — 切り替えの線より後の着地の commit を発端と便の trailer で判じて misfit 3 語を数え、契約表の行と要件の局面を台帳と trailer から導き、読めない面を unmeasured に名指す（case-lifecycle §10・FR90 / FR92）"
req = ["FR90", "FR92"]
section = "10"
depends = ["a"]
write-set = ["+crates/scribe2/src/ledger/phase_main.rs", "crates/scribe2/src/ledger/mod.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail phase_main_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/ledger/phase_main.rs:520", "crates/scribe2/src/ledger/mod.rs:60"]
done = "(1) ledger の新しい module の純関数 1 本が線より後の commit（呼び手が渡す）・切り替えの線の時刻・event log の run id・台帳・契約表の行・要件 id を受け、commit・row・requirement の部品と、便の run id と契約の bead id ごとの commit の結びを返す (2) 器の便の commit は部品にせず結びの run id と bead id の両方に載せ、run-trailer-unknown・source-unresolved・commit-no-trailer・commit-landed を判じる (3) row の 3 局面（.md と .toml・着地の形の閉じか main の契約の trailer で row-landed・線より前の閉じは形を問わず着地・線より後の着地でない閉じは着地でない） (4) requirement の 2 局面（requirement-unrowed の手番は seat） (5) 要件と契約表が無いか読めない形なら unmeasured の srs-unreadable と table-unreadable (6) closed は commit が契約の trailer で閉じた bead を名指すときだけ真・row は row-landed だけ真・requirement は常に偽 歯: phase_main_（2 語に当たる commit の先の語・結びの 2 つの id・row-beaded の since・trailer の経路の row-landed と trailer の無い対・線の前後の取り下げの閉じの対・unmeasured の周の部品 0・closed の 5 形を含む）が base で 0 本（rc 4・機能不在） (7) 歯の fixture は Issue を literal で組まず JSON の字から issues_of で作り、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "c1"
title = "線の読みと記帳 — 切り替えの線は detail の無い最初の LifecycleCutover・close-check の線は detail が close-check の最初の行と読み、store の Condition の無いときだけ足す値で 1 度だけ記帳する（case-lifecycle §11・ADR-0100・FR90 / AC61）"
req = ["FR90", "AC61"]
section = "11"
write-set = ["+crates/scribe2/src/fleet/lifecycle_line.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/store.rs", "=crates/scribe2/src/fleet/event.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail cutover_line_"]
size = "S"
growth = ["crates/scribe2/src/fleet/lifecycle_line.rs:205", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/fleet/store.rs:45"]
done = "(1) fleet の新しい module の読みの純関数が event の列から切り替えの線（detail の無い最初の LifecycleCutover）と close-check の線（detail が close-check の最初の行）を返し、detail がほかの値の行は数えない (2) store の Condition に述語を持つ値を 1 つ足し、event の lock の内側で述語に当たる event が無いときだけ追記する (3) 記帳の関数が器の版と main の sha で 1 件だけ足し、close-check の線は detail に close-check を持って切り替えの線が在るときだけ足し、同じ周は切り替えの線が先、在る線は動かさない (4) KNOWN_KEYS と kind の key の表と EventKind は変えず、close-check の線の行が from_line で読め、表の外の key を足した行は読めない 歯: cutover_line_（切り替えの線の無い log の close-check の記帳は足さず線の後は足す対を含む・(2) は store.rs の test 区間）が base で 0 本（rc 4・機能不在）"

[[contract]]
id = "c"
title = "局面の出力の書き手 1 本と全部の書き直し — 順を持つ入力の印 3 つ・lock と一時 file の rename・古い書きの捨てと中身の同じ書きの不 rename・lifecycle.stale の印の読み書きと消し・線の記帳・4 契機と fleet lifecycle write / show の口・rules 行 2 種（case-lifecycle §12・ADR-0088・FR90 / FR94 / AC60・user 2026-09-30T04:25Z）"
req = ["FR90", "FR94", "AC60"]
section = "12"
depends = ["a2", "b", "b1", "c1"]
write-set = ["+crates/scribe2/src/fleet/lifecycle.rs", "+crates/scribe2/src/fleet/lifecycle_mark.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/help.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/hook/utterance.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/cli/step.rs", "crates/scribe2/src/pipe/retire.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", "crates/scribe2/src/rules/mod.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail lifecycle_writer_", "cargo nextest run -p scribe2 --lib --no-tests=fail lifecycle_mark_", "cargo nextest run -p scribe2 --lib --no-tests=fail close_check_at_sha_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_lifecycle_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_lifecycle_rows_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_floor_timeout_row_precedes_the_drafts_cap_rows", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_drafts_cap_rows_are_the_last_two_kinds_and_rows", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_kind_parity_every_kind_has_sample", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/fleet/lifecycle.rs:1120", "crates/scribe2/src/fleet/lifecycle_mark.rs:260", "crates/scribe2/src/fleet/mod.rs:2", "crates/scribe2/src/fleet/cli.rs:60", "crates/scribe2/src/help.rs:2", "crates/scribe2/src/pipe/dispatch.rs:20", "crates/scribe2/src/pipe/cli.rs:12", "crates/scribe2/src/pipe/dispatch/candidates.rs:30", "crates/scribe2/src/hook/utterance.rs:1", "crates/scribe2/src/pipe/land.rs:1", "crates/scribe2/src/pipe/land/finish.rs:16", "crates/scribe2/src/pipe/cli/step.rs:5", "crates/scribe2/src/pipe/retire.rs:4", "crates/scribe2/src/pipe/declaration.rs:1", "crates/scribe2/src/pipe/declaration/optional_keys.rs:40", "crates/scribe2/src/rules/mod.rs:40", "crates/scribe2-boundary/tests/e2e/fleet.rs:320"]
done = "(1) 入力の印は ledger {noms: root・gen・chunks | files: len・mtime_ns}・events {len・head}・main {ref refs/remotes/origin/main・sha} で、台帳は manifest の 1 file、main は git を撃たずに loose・packed-refs・worktree の common dir を読み、同じ gen / head なら数の大小・違えば順を持たない (2) lifecycle.lock は生きた所有者からは rules 行 fleet.lock_stale_ms より古くても外さず Busy、死んだ所有者からは外して取り、書きかけは lifecycle.json の名に見えず（一時 file からの rename）、新しい印の file には Discarded・generated_at の他に同じ中身は Unchanged で rename しない (3) 全部の書き直しは FR90 の 5 入力を読み、宣言は読んだ main の sha の tree から optional_keys.rs に足す sha の読みで読み（worktree の HEAD の宣言を読まない）、契機 (a)(d)(e) の出力が列の判定を持ち、a2 の misfit を置き換え、b1 の結びを links.commits に・request の仕分けを memo の links.source に写し、since を継ぎ、窓を終わりの閉じた部品だけに掛け、owned を数え、full_at・interval_s・closed_window_h を写す (4) 読めない周は書かず unreadable の印（理由は ledger・events・main・table・srs・declaration の最初の 1 語）、SRS と契約表の無い repo は unmeasured で書く (5) lifecycle.stale は種類 3 つ・種類ごとに 1 つ・後の印が置き換え、最初の Written で空の marks を作って消さず、json の rename の後に消す (6) 印を消すのは Written か Unchanged の全部の書き直しで、開始が印の at より後 ∧ 種類ごとの条件の印だけ (7) Written か Unchanged の周に切り替えの線と、宣言が true なら close-check の線を足す (8) fire の後・land の終端の close の後（着地の本体の finish.rs と step.rs の terminal_only の 2 つの呼び手が terminal の rc 0 の周に撃ち stderr に足す）・retire.rs の close の Ok の後・口の契機で撃ち、呼び手の rc と stdout を変えず、Written・Unchanged・Coalesced の外は stderr に lifecycle=<語> の 1 行、dispatch ls は撃たない (9) fleet lifecycle write（--wait-ms・Coalesced・busy）と show（absent・unreadable）の text の 2 種の行が §12 のとおりで、使い方と help と外形 snapshot が lifecycle を持つ (10) rules 行 lifecycle.closed_window_h（72）と lifecycle.age_h.<語> 13 行が ruling user 2026-09-30T04:25Z 項 lifecycle（裁定 user 2026-09-30T04:25Z の項・base の行と別の字）で ALL と manifest の末尾に在り、語の外の後ろは行の検査が断り、末尾の位置と数を pin する既存の rules の歯 7 本と snapshot を直す 歯: lifecycle_writer_（a2 の misfit の置き換え・lock の生死・窓と since の 3 形と owned の 4 欄と unset の対・links.commits と memo の links.source・HEAD と main の sha の宣言の食い違い・読めない 6 語の各 1 と 2 入力の先の語と srs の読めないと不在の対・rename の落ちた周と Discarded と Busy の周の印の不消・false と Discarded と Busy の周の線を含む）・lifecycle_mark_（HEAD と object の無い .git の fixture を含む）・close_check_at_sha_（optional_keys.rs の test 区間）が base で 0 本（rc 4）、fleet_lifecycle_（busy の周の stderr の 1 行・Written と busy の周の呼び手の rc と stdout の一致・text の形・help の頁・close の 2 経路と落ちた close・(a)(d)(e) の周の依存待ちの契約の queued を含む）が未知の verb の断りの rc 1、fleet_external_form が外形の snapshot の違い、rules_lifecycle_rows_（値と裁定 id）が行の不在で RED（機能不在） (11) 新しい file の lifecycle.rs と lifecycle_mark.rs は EventKind を名指さず（event は Case と replay の便の表と kind の字で見分ける）、Stage の arm・WaitReason と Turn の literal・RuleKind の変種を書かず、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "d"
title = "部分の書き直し — 管理 tick の周に、台帳も git も撃たず event log の末尾と今の出力だけを読んで、発話・発端の結び・便・期日・年齢と owned を書き直し、log が繋がらない周は読めない印を付け、印は消さない（case-lifecycle §13・FR90 / FR27 / NFR5 / AC60）"
req = ["FR90", "FR27", "NFR5", "AC60"]
section = "13"
depends = ["c"]
write-set = ["+crates/scribe2/src/fleet/lifecycle_partial.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/store.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/seat/tick.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail store_read_after_", "cargo nextest run -p scribe2 --lib --no-tests=fail lifecycle_partial_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_rewrites_lifecycle_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/fleet/lifecycle_partial.rs:540", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/fleet/store.rs:60", "crates/scribe2/src/seat/tick.rs:4"]
done = "(1) 管理 tick の周の契機で撃ち、tick の rc と字を変えない（rc 0 のまま stdout と stderr に lifecycle の字を足さない） (2) 出力の無い置き場は Absent (3) lifecycle.lock を死んだ所有者だけ外す取り方で 200 ms まで取り直し、取れなければ Busy (4) 読むのは今の lifecycle.json・log の 1 行目・len の直前の 1 byte・store に足す末尾の読みによる len から末尾・埋め込みの rules・末尾に段の行を持つ便の運転手の札だけで、子 process を撃たない (5) head が違う・短い・直前が改行でない周は書かず unreadable（理由 events） (6) 発話（log の全部を行 b の発話の関数に渡した結果と同じ部品・線は末尾の切り替えの線か出力の head・末尾より前の発話は前の部品と末尾の仕分けを合わせる）と memo の発端の結び・閉じていない契約の最新の便の局面（末尾に段の行を持つ便だけ・札の生死を読む・末尾で起きた便は前の便を置き換え）・keep の無い memo-waiting の memo の due の過ぎた memo-actionable への移り・年齢と owned を書き直し、scope は partial・inputs.events だけを進め、ledger と main と full_at とほかの部品の局面と理由を動かさない (7) 印を消さない 歯: store_read_after_（数える包みで 10 MB と 20 MB の読む byte が末尾の長さ + 1 で一致・繋がらない 3 形）・lifecycle_partial_（Absent・lock の生死・200 + 700 部品で 10 MB と 20 MB の読む byte と中身〔generated_at と events の len を除く〕の一致・発話 3 形と逐語の不在・末尾より前の発話の仕分けと線の在る末尾と線が末尾より前の log が全部の log の結果と一致・発端の結び・便の移りと札の生死の対と新しい便の置き換えと閉じた契約の便の不載・期日の移りと keep と asking の不動・年齢と owned・scope と inputs と full_at・契約の不動・印の不消と繋がらない 3 形の読めない印）・seat_tick_rewrites_lifecycle_（tick の周の移り 2/2 と、同じ歯の中の shim の呼びの数の一致と rc 0 と lifecycle の字の不在）が base で RED（機能不在） (8) 新しい file の lifecycle_partial.rs は EventKind を名指さず（event は Case と段の欄と kind の字で見分ける）、Stage の arm・WaitReason と Turn の literal を書かず、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "e"
title = "起票の門と merge の門が通した周に古さの印を付ける — hook の Bash の allow の出口で、台帳の書きか create の片を持つ command に ledger-gate を、gh pr merge の片を持つ command に merge-gate を行 c の印の関数で付け、台帳も git も撃たず、出力の無い置き場と値を読めない周は allow を変えずに付けない（case-lifecycle §14・FR90 / AC60）"
req = ["FR90", "AC60"]
section = "14"
depends = ["c"]
write-set = ["+crates/scribe2/src/hook/stale_gate.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2-boundary/tests/e2e/hook.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_stale_mark_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
growth = ["crates/scribe2/src/hook/stale_gate.rs:120", "crates/scribe2/src/hook/mod.rs:4", "crates/scribe2-boundary/tests/e2e/hook.rs:290"]
done = "(1) hook の PreToolUse の Bash の道が最後に allow と決めた周だけ印を付け、門が断った周は付けない (2) subcommand が書きの列 WRITES に在る片を持つ command に行 c の台帳の印（書きの前の値）の ledger-gate を付け、読みだけの bd の片では付けない (3) gh pr merge の片を持つ command に行 c の main の読みの sha の merge-gate を付ける (4) lifecycle.json の無い置き場では付けない (5) 印は種類ごとに 1 つ・後の印が置き換え・書き直しは撃たない (6) 印付けの子 process は 0 本（門が撃つ分は変えない） (7) 値を読めない周と stale の lock を取れない周は allow を変えずに付けない 歯: hook_stale_mark_（ledger-gate と merge-gate の値・packed-refs と worktree・2 種の片を持つ command の 2 印・印の周の lifecycle.json の不変・読めない manifest の fail-open・断られた書きと読みだけの bd と gh pr view と出力の無い置き場で付かない・置き換え・manifest を動かさない書き直しで消えず進めた後で消える対・event log だけの進みで消えず main の進みで消える対・shim の呼びの数が出力の無い置き場と等しい・lock を持った周の allow。否定はどれも印が付く肯定と同じ歯に置く）が base で RED（機能不在） (8) hook の新しい子 module stale_gate.rs は Write と Refusal を名指さず、verify の最終行の contracts check が便の木で findings 0"

[[contract]]
id = "f"
title = "台帳の印の読み手を実物の bd の manifest の形に合わせる — noms_of の root・gc の世代・組の位置を 1 つ前へ直し、2 つ目が __DOLT__ でない形を読めないとし、歯の見本を実物の形に直して実物の 1 行の形の歯を足す（§16）"
req = ["FR90", "FR94"]
section = "16"
write-set = ["crates/scribe2/src/fleet/lifecycle_mark.rs", "crates/scribe2-boundary/tests/e2e/hook.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail lifecycle_mark_ledger_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_stale_mark_"]
size = "S"
growth = ["crates/scribe2/src/fleet/lifecycle_mark.rs:30"]
done = "(1) 実物の bd の manifest の形（`5:__DOLT__:<lock>:<root>:<gc の世代>:` の後に file 名と chunk 数の組）の 1 行を持つ embedded の台帳で、read_ledger が Noms を返し、root は 4 つ目の字、chunks は組の数の和、gen は gc の世代と journal でない file 名の digest〔lifecycle_mark_ledger_ の新しい歯 1 本・32 桁の root と 32 桁の 0 の gc の世代と table の組 2 つと journal の組 1 つの見本〕 (2) 2 つ目だけを `__LD_1__` に替えた形（ほかの欄は (1) の読める見本と同じ・組は偶数）は None で、同じ見本の 2 つ目が `__DOLT__` なら Noms（対照）。欄が足りない・組が奇数・数でない・root が空の実物の形も None〔lifecycle_mark_ledger_refuses_broken_manifests_and_metadata の見本を実物の形に直し、2 つ目の語の対を足す〕 (3) journal だけ・table つき・gc の世代違いの 3 形が実物の形の見本で同じ root・gen・chunks を返す〔lifecycle_mark_ledger_noms_reads_the_manifest_of_three_shapes の見本を直す〕 (4) 門が通した周の古さの印の歯が実物の形の manifest の見本で緑〔hook_stale_mark_ の helper stale_manifest を直す〕 base は 5 つ目を root と読むので実物の形で root が違うか None を返し、(1)〜(4) が RED"

[[contract]]
id = "g"
title = "局面の導出に未反映の問いと処置の無い判定の memo を渡す — 全部の書き直しが未反映の裁定の置き場の id を閉じた台帳の問いへ引き、最後の MemoJudged が promote か close で判定の後に台帳が書いていない開いた memo を組んで渡し、読めない置き場を unmeasured に名指す（case-lifecycle §17・memo s2-07l.738.38.10）"
req = ["FR90", "FR84", "FR87", "FR91", "AC60", "AC61"]
section = "17"
write-set = ["crates/scribe2/src/fleet/lifecycle.rs", "crates/scribe2/src/fleet/lifecycle_mark.rs", "crates/scribe2/src/pipe/dispatch/unreflected.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/hook/graph_guard.rs", "crates/scribe2/src/ledger/form.rs", "crates/scribe2/src/pipe/dispatch/precheck.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail unreflected_questions_", "cargo nextest run -p scribe2 --lib --no-tests=fail verdict_unhandled_", "cargo nextest run -p scribe2 --lib --no-tests=fail issue_updated_at_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_lifecycle_feeds_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/fleet/lifecycle.rs:15", "crates/scribe2/src/fleet/lifecycle_mark.rs:190", "crates/scribe2/src/pipe/dispatch/unreflected.rs:30", "crates/scribe2/src/seat/ledger.rs:15", "crates/scribe2/src/hook/graph_guard.rs:1", "crates/scribe2/src/ledger/form.rs:1", "crates/scribe2/src/pipe/dispatch/precheck.rs:1", "crates/scribe2-boundary/tests/e2e/fleet.rs:150"]
done = "(1) 全部の書き直しが、未反映の裁定の置き場（state dir の pipe/unreflected）の裁定 id の列を、閉じた台帳の問い（label の問い）のうち notes の裁定の行にその id を持つ問いの id の列へ unreflected.rs に足す読み 1 本で引いて行 a1 の導出に渡し、その問いが ruling-unreflected・手番 seat で出る（窓より古く閉じた問いも載る）。置き場に無い id の問い・notes の散文にだけ id を書く問い・開いた問いは渡さず、置き場の無い周は空の列で、契機 (a) は同じ周の fire が書いた置き場を、(d)(e) は前の起こす側の周の置き場を読む (2) 置き場が在って読めない周は書き直しを止めず問いの列を空で渡し、unmeasured に question と unreflected-unreadable を 1 件名指し、置き場が読める周は名指さない (3) 開いた memo のうち event log の最後の判定の行（本体が Case の判定・bead がその memo）の語が promote か close で、台帳の updated_at がその行の ts より後でない memo の id の列を行 a1 の導出に渡し、その memo が memo-actionable・理由 verdict・手番 seat で出る。最後の判定が keep か unparsed の memo・判定の後に台帳が書いた memo・閉じた memo・判定の行の無い memo は渡さず、updated_at の無い memo は渡す (4) Issue が updated_at を字の Option で持ち、issues_of が読み、無い要素は None で、組みの 4 か所（issues_of・graph_guard.rs・form.rs と precheck.rs の歯の fixture・便の始めに数え直す）を直す 歯: unreflected_questions_（lifecycle_mark.rs の test 区間・無い置き場の空・読めない置き場・未反映の id の行を持つ閉じた問いだけの列を全体の等しさで比べ、写った id の問い・散文だけの問い・開いた問い・問いの label の無い bead を外す）・verdict_unhandled_（同じ test 区間・promote と close の在りと keep と unparsed の無し・判定の順の 2 通り・updated_at が ts と同じ memo の在りと 1 秒後の無しの対・updated_at の無い memo の在り・閉じた memo と memo でない bead の無し）・issue_updated_at_（seat/ledger.rs の test 区間・在る bead と無い bead の対とほかの欄の不変）が base で 0 本（rc 4・機能不在）、fleet_lifecycle_feeds_（e2e 2 本: 契機 (e) を先に撃つと置き場が無く document の問いの部品は無く promote の memo が verdict・pipe dispatch の後に document の問いが窓より古くても ruling-unreflected で operation の問いは question-closed・keep と処置の後の memo は memo-waiting・出力を消した後の契機 (e) が前の置き場から同じ局面を出す／読めない置き場で出力を消した後の契機 (e) が unmeasured に unreflected-unreadable を持ち、次の dispatch の後に名指しが消えて ruling-unreflected に戻る）は base の書き手が 2 つの列を空で渡すので RED（機能不在）"

[[contract]]
id = "h"
title = "局面の出力の contract-queued の部品で、列の理由が reserved のとき links.on に行を予約した bead を載せる（§18）"
req = ["FR90"]
section = "18"
write-set = ["crates/scribe2/src/fleet/phase.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail phase_event_links_on_"]
size = "S"
growth = ["crates/scribe2/src/fleet/phase.rs:4"]
done = "(1) 列の理由が reserved の契約の部品の links.on は、理由の値（<予約した bead>/<file 数>・読めない周は末尾に /unset）の最初の / より前の bead 1 つで、値に / が無い周も値の全体を 1 つ載せる (2) dependency と overlap の links.on は今のまま 歯: phase.rs の既存の mod tests に置く lib の歯（接頭辞 phase_event_links_on_）の (a)〔(1)〕値 b-6/2 の reserved の契約の links.on が b-6 だけ・値 b-6/2/unset の契約も b-6 だけ・値 b-6 の契約も b-6 だけ、(2) は既存の歯 phase_event_links_on_follows_dependency_and_overlap_partner が本文を変えずに緑 base は reserved の links.on が空なので (a) が RED"
[[contract]]
id = "i"
title = "管理 tick が台帳か main の印の動いた周に全部を書き直す — 出力の印と等しさで比べ（bd も git も撃たない）、前の全部の書き直しと tick の撃った記録から rules 行 lifecycle.full_min_s（300 秒・裁定 user 2026-10-01T15:27Z）以上後の周だけ §12 (e) の 1 本を撃ち、記録 fleet/lifecycle.tick を書き、tick の rc と字は変えない（§19・FR90 / FR27・memo s2-07l.738.42.1）"
req = ["FR90", "FR27"]
section = "19"
depends = ["j"]
touches = ["crate::rules::RuleKind"]
write-set = ["crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/help.rs", "crates/scribe2/src/fleet/lifecycle_partial.rs", "crates/scribe2/src/rules/mod.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "=crates/scribe2-boundary/tests/e2e/main.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_full_lifecycle_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_lifecycle_full_min_s_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_rewrites_lifecycle_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_floor_timeout_row_precedes_the_drafts_cap_rows", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_drafts_cap_rows_are_the_last_two_kinds_and_rows", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_usage_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail subcommands_are_gone_from_the_usage", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_inject_subcommand_is_gone_from_the_usage", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_pages_match_the_live_form_and_every_subcommand", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_text_is_ascii_and_fits_the_width", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/seat/tick.rs:30", "crates/scribe2/src/seat/cli.rs:6", "crates/scribe2/src/help.rs:2", "crates/scribe2/src/fleet/lifecycle_partial.rs:60", "crates/scribe2/src/rules/mod.rs:6", "crates/scribe2-boundary/tests/e2e/seat/tick.rs:420", "crates/scribe2-boundary/tests/e2e/seat.rs:30", "crates/scribe2-boundary/tests/e2e/fleet.rs:20", "crates/scribe2-boundary/tests/e2e/rules.rs:40"]
done = "(1) 埋め込み manifest に rules 行 lifecycle.full_min_s（kind LifecycleFullMinS・形 Int だけ・値 300・enabled・裁定の字 user 2026-10-01T15:27Z・裁定日 2026-10-01・裁定の字は base の行 floor.timeout_s と違う）が lifecycle.age_h.epic-closable の行の直後（memo.notes_max_bytes の前）に 1 本在り、kind は ALL の LifecycleAgeH の直後（MemoNotesMaxBytes の前）で字面から引け、その後ろの memo の 3 行と index の 2 行（各 kind）は順も数も本行を撃つ時の main のまま〔rules_lifecycle_full_min_s_ と、直す既存の歯 7 本（数と skip の字は本行を撃つ時の main の値から 1 つ増やす: 行数の rules_embedded_manifest_is_valid_and_covers_all_kinds と kind 数の rules_embedded_manifest_declares_one_capability_row_per_role の 2 本・末尾からの窓が LifecycleFullMinS の挿入点より前に掛かる rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling・rules_floor_timeout_row_precedes_the_drafts_cap_rows・rules_drafts_cap_rows_are_the_last_two_kinds_and_rows・class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id の skip・LedgerDeniedWrites からの列の LifecycleAgeH の直後に LifecycleFullMinS を足し末尾の窓の skip も増やす rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all）と rules_external_form の snapshot（rows= と kinds= の 2 行を main の値 +1）・窓が挿入点より後ろだけに掛かる rules_memo_rows_are_the_last_three_kinds_and_rows と rules_index_rows_are_the_last_two_kinds_and_rows は本文を変えずに緑〕 (2) 管理 tick の周は判定・打刻・部分の書き直しの後に、manifest を読めて target の登録 row が在り・置き場の出力が読め・tick の manifest に lifecycle.full_min_s の行が在り・今が出力の full_at と撃った記録の ts の新しい方から lifecycle.full_min_s 秒以上後で・登録 row の anchor の台帳の印か main の sha が出力の inputs.ledger か inputs.main と等しくない（等しさだけで比べ bd も git も撃たない・読めない印は撃たない・event log の印は比べない）周だけ、§12 (e) の口と同じ全部の書き直しの 1 本を観測の 1 周・coalesce・lock を待たない形で、repo = anchor・台帳 client = seat tick の --bd（無ければ bd）で撃つ〔seat_tick_full_lifecycle_ の (a) 台帳を伸ばした周に scope full・full_at の前進・inputs.ledger が今の印・偽 bd の呼び 1 回以上と、同じ歯の中の印を動かさない周の偽 bd の不増と full_at の不動 (b) main の ref を別の commit へ進めた周も撃つ (c) event log だけを伸ばした周は撃たず次に台帳を伸ばした周は撃つ (d) full_at が 30 秒前の周と記録の ts が 30 秒前の周は撃たず両方を 2 時間前へ戻した周は撃つ (e) 出力の無い置き場（出力を作らない）・行の無い写し・anchor の台帳を読めない置き場は撃たず、同じ歯の中で直した周は撃つ〕 (3) 撃った周は返りに依らず置き場の fleet/lifecycle.tick を ts=<epoch 秒> wrote=<返りの語> の 1 行で一時 file → rename で書き、撃たない周は書かない〔(a) の wrote=written・(f) --bd を渡さず PATH に bd の無い env の周の wrote=unreadable と出力の理由 ledger の読めない印・(g) lifecycle.lock を生きた pid で持たせた周の待たない wrote=busy と出力の bytes の不変〕 (4) tick の rc と stdout の判定行と stderr は撃った周も撃たない周も変えない〔(a) の 2 周の rc と判定行と stderr（置き場の path を伏せた字）の一致・(f)(g) の rc と判定行と stderr が (a) の撃つ周と等しい〕 (5) seat の使い方の 1 行と help の seat の頁の tick の口が tick --state-dir S --target S:W [--rules F] [--bd B] になる〔seat_usage_external_form の snapshot と、seat.rs の tick の口の字の定数を読む 3 本（seat_autonomy_subcommands_are_gone_from_the_usage・seat_inject_subcommand_is_gone_from_the_usage・seat_working_memory_subcommands_are_gone_from_the_usage）と、help の seat の頁の FORM が使い方の行の逐語の写しのままであること（既存の cli_help_pages_match_the_live_form_and_every_subcommand が緑）〕 (6) 既存の歯 seat_tick_rewrites_lifecycle_ は本文を変えずに緑（anchor が在らない path で印を読めず撃たない） (7) 管理 tick の全部の書き直しは (2) の 5 つを全部満たした周にだけ §20 の数え census_anchors（行 j）を置き場・登録 row の anchor・tick がその周の判定の前に読んだ event の列で撃ち、One を返す周だけ撃ち、Many と Foreign の周は撃たず撃った記録も書かない〔seat_tick_full_lifecycle_ の (h) 登録 row の anchor の toy repo の設定 <NAME>.stateDir に別の在る dir を書いた置き場は (a) の撃つ形の周も撃たず（偽 bd の呼びが増えず・記録が無く・full_at が動かない）、同じ歯の中で設定を置き場の dir に書き直した周は撃ち（wrote=written）、撃つ形を戻して tick と別の target で設定の無い別の anchor の登録 row を足した周は tick の target の anchor が toy のままで撃たず、その row の退役の行を足した周は撃ち、どの周も rc と判定行が等しい・(a)〜(h) の歯は頭で toy repo の設定を tick の置き場の dir に書き直し、その値を assert する〕 歯: seat_tick_full_lifecycle_（e2e・(a)〜(h) の 8 本・どれも --rules の写しで lifecycle.full_min_s を 60 にし撃つ形を 1 つ以上持つ）と rules_lifecycle_full_min_s_（rules の e2e・1 本）が base で RED（機能不在: base は --bd と LifecycleFullMinS の kind を知らず rc 1・行が無い）"

[[contract]]
id = "j"
title = "1 つの置き場に 2 つ以上の repo の席が在る周の判じ方 — 置き場の anchor を登録 row と state_dir の名乗りで数える crate の中の 1 本（閉じた 3 値 One・Many・Foreign・後の行 i が呼ぶ口）を置き、全部の書き直しは Many の周に unmeasured へ multi-anchor を 8 つの種類で名指す（§20）"
req = ["FR90"]
section = "20"
write-set = ["crates/scribe2/src/fleet/lifecycle_mark.rs", "crates/scribe2/src/fleet/lifecycle.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail anchor_census_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail anchor_census_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_lifecycle_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/fleet/lifecycle_mark.rs:120", "crates/scribe2/src/fleet/lifecycle.rs:6", "crates/scribe2-boundary/tests/e2e/fleet.rs:80"]
done = "(1) lifecycle_mark.rs の pub(crate) の関数 census_anchors が置き場の path・repo の path・event の列の 3 つを受け、pub(crate) の閉じた enum AnchorCensus（変種 One・Many・Foreign の 3 つだけ）を返す。置き場の anchor は replay の registrations の anchor から、state_dir が返す path の正規化が置き場の正規化と違う在る dir の anchor を除いたもの（設定が無い・設定を読めない・名乗る path が無い anchor は数える）で、置き場の anchor と repo の正規化の和が 2 つ以上なら Many・1 つで repo が別の置き場を名乗るなら Foreign・ほかは One（lib: どの歯も census_anchors を名で呼び返りを変種と等しさで比べる。登録が無い・R だけ・.. を含む R の path は One で設定の無い B を足すと Many／B の設定が別の在る dir なら One で置き場なら Many・在らない path の C は Many・B の退役で One／R が別の在る dir を名乗り登録が R だけなら Foreign で設定の無い B を足すと Many） (2) 全部の書き直しの gather は自分が読んだ event の列で (1) を撃ち、Many の周だけ unmeasured の末尾に reason multi-anchor を part question・memo・contract・run・row・requirement・epic・commit の順に 1 件ずつ足し、utterance は名指さず、One と Foreign の周は足さず、どの周も書き直しを止めない（e2e: Life の置き場に設定の無い tmp の repo を anchor にした登録 row を積んだ fleet lifecycle write が rc 0 で toy-c2 の部品を書き multi-anchor の項が 8 件・種類の順・utterance 無しで、その repo の設定に別の在る dir を書き台帳の file を伸ばした後（generated_at の前進を前提に assert）と Life の repo だけの登録の置き場では 0 件・既存の fleet_lifecycle_ は本文を変えずに緑） (3) 新しい code は EventKind・Stage・WaitReason・Turn の変種と literal と RuleKind の変種を名指さず Registration を literal で組まず、verify の最終行の contracts check が便の木で findings 0 base は新しい 1 本と enum が無く lib が compile できず、書き直しが multi-anchor を出さないので RED（機能不在）"

[[contract]]
id = "k"
title = "結びと答えの口が台帳を書いた直後の全部の書き直しを切り離した子で起こす — seat ruling bind と seat ruling answer が Ok で返った周に、自分の binary を fleet lifecycle write の子として待たずに起こし（after_close は呼ばない）、起こせなかった周は書く前の台帳の印の ledger-gate の印を 1 つ足し、口の rc・stdout・stderr と待ち時間は変えない（台帳の印を読めない repo と Ok 以外の周は起こさない・§21）"
req = ["FR90", "AC60"]
section = "21"
write-set = ["crates/scribe2/src/seat/cli.rs", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_rewrites_lifecycle_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_bind_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_ruling_answer_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
growth = ["crates/scribe2/src/seat/cli.rs:30", "crates/scribe2-boundary/tests/e2e/seat/ruling.rs:180"]
done = "(1) seat ruling bind は結ぶ前に repo の台帳の印を読み、Ok で返った周は自分の binary を fleet lifecycle write --state-dir <置き場> --repo <repo>（--bd を受けた周は同じ --bd）の子として入出力を捨てて 1 回起こし、待たず、起こすのは裁定 event を書いた後（e2e (a): 印の在る置き場の結びが rc 0・既存の歯と同じ stdout・stderr 0 byte で、期限 10 秒の内に close の後の --readonly list の撃ちが 1 回現れ list.rulings が 1） (2) seat ruling answer が Ok で返った周も同じ 1 本で子を起こす（e2e (b): list.hold を置いた印の在る置き場の答えが list.go の前に rc 0・裁定 id の 1 行・stderr 0 byte で wall 10 秒未満で返り、list.go の後に期限の内に list.ended が現れ list.rulings が 1） (3) Ok 以外で返った周と書く前の台帳の印を読めない周は起こさない（e2e (c): 印の在る置き場の閉じた問いの結び・空白だけの答え・close を 1 回落とした結びは期限まで待って list 0 で stderr は今の 1 行だけ、同じ歯の撃ち直しの結びは期限の内に list を撃ち、出力の fixture を置いた印の無い置き場の結びは rc 0 で期限まで待って lifecycle.stale に unreadable の印が無く、その後に同じ置き場で口を通さずに撃った fleet lifecycle write は理由 ledger の unreadable の印を 1 つ足す・既存の seat_ruling_bind_ と seat_ruling_answer_ は本文を変えずに緑） (4) 子を起こせなかった周は書く前の台帳の印を値にした ledger-gate の印を行 c の印を 1 つ足す関数で 1 つ足し、起こせた周は足さない（e2e (d): 出力の fixture を置いた印の在る置き場で argv[0] を在らない path にした結びは rc 0・(a) と同じ stdout・stderr 0 byte で、lifecycle.stale の ledger-gate の印の値が結ぶ前の read_ledger の値と等しく結んだ後の値と違い、期限まで待って list 0、同じ歯の本物の argv[0] の写しは期限の内に list を撃ち ledger-gate の印が無い） (5) 2 つの口の rc・stdout・stderr は起こした周も起こさない周も起こせなかった周も変えず、待ち時間に子の書き直しを足さない（(a)〜(d) の rc と stdout と stderr 0 byte と (b) の wall） (6) 新しい code は EventKind・Stage・WaitReason・Turn の変種と literal・RuleKind の変種を名指さず、印の種類は ledger-gate だけを名指し、verify の最終行の contracts check が便の木で findings 0 base は 2 つの口が子を起こさず印も足さないので (a)(b)(c) の期限の内の list と (d) の ledger-gate の印が無く RED（機能不在）"

<!-- contracts:end -->

## 18. 列の理由 reserved の links.on に行を予約した bead を載せる（契約表の行 h）

やさしく言うと: 落ちた便の bead が write-set を先に取っている（予約している）ので待つ契約は、局面の出力に「reserved で待つ」とだけ出て、誰が取っているかが出ない。依存や重なりで待つ契約と同じく、相手の bead を links.on に載せる。

- 何が起きているか（main 04eec3cf・verified）:
  - 局面の導出の `links_on`（`crates/scribe2/src/fleet/phase.rs`）は、列の理由が dependency なら値の bead の列、overlap なら相手の便の bead を links.on に載せる。そのほかの理由は空。
  - 列の理由 reserved の値は `<予約した bead>/<file 数>` で、読めない周は末尾に `/unset` が付く（`crates/scribe2/src/pipe/dispatch/reserve.rs` の `Held` の値）。dispatch ls の行（reason=reserved:<値>）には相手が出るが、局面の出力には出ない。
  - 局面の出力を読む面（隣の project の板）が、待つ相手を名指すのに links.on を読む。
- 約束（番号は done と 1:1）:
  1. 列の理由が reserved の契約の部品の links.on は、値の最初の / より前の bead 1 つ。値に / が無い周は値の全体を 1 つ載せる（値を捨てない）。
  2. dependency と overlap の links.on は今のまま。
- 歯: phase.rs の既存の mod tests に lib の歯を 1 本置く（接頭辞 phase_event_links_on_）。(a) 値 b-6/2・b-6/2/unset・b-6 の reserved の 3 契約の links.on が、どれも b-6 だけ。既存の歯 phase_event_links_on_follows_dependency_and_overlap_partner は本文を変えずに緑。
- 触らない: 列の理由の語と値の字（dispatch の `WaitReason`）・手番の表（§3）・links の 8 key の名・dispatch ls の行。

## 19. 管理 tick が台帳か main の印の動いた周に全部を書き直す（契約表の行 i・下限は rules 行 `lifecycle.full_min_s`・FR90 / FR27・[ADR-0108](../../design-intent/decisions/ADR-0108-the-management-tick-rewrites-case-positions-when-the-ledger-or-main-moves.html)（proposed）・memo `s2-07l.738.42.1`）

やさしく言うと: 局面の出力を全部書き直すのは、便の列を回す周・land の終端・口を撃った時だけで、便を回さない置き場（隣の project の板が読む置き場）では台帳が変わっても出力が古いまま残る。管理 tick の周に、台帳と main の印（小さい file の読みだけ）を出力の印と比べ、違っていれば全部を書き直す。重い書き直しが続かないように、前の全部の書き直しから最短でも rules 行の秒数（300 秒）を空ける。

- 出所: 隣の project の設計席の頼み（2026-10-01・板の一覧が局面の出力を読み、便を回していない間も台帳の変化を出したい・5 分の遅れで足りる）。memo `s2-07l.738.42.1`。下限の rules 行の値は user の裁定（台帳の問い `s2-07l.738.42.3`・裁定 `s2-07l.738.42.3:20261001T1527Z-1`・値 300 秒）。
- 何が起きているか（main 417e4754・verified）:
  - 全部の書き直しの契機は §12 約束 8 の (a)(d)(e) だけで、管理 tick の周は §13 の部分の書き直ししか撃たない（`crates/scribe2/src/seat/tick.rs` の `run` は判定と打刻の後に部分の書き直しを撃ち、返りを捨てる）。ADR-0088 と FR90 は、管理 tick の周が台帳も git も撃たないと決めている。
  - 全部の書き直し 1 回は、本 repo の台帳（chunk 約 36 万）・event log 9.3 MB・設計 doc 30 本で wall 4.3 秒・user 3.8 秒・最大 RSS 275 MB（2026-10-01 の実測・空いた host）。gate が走る host では wall 14.3 秒・user 3.7 秒。
  - 全部の書き直しは自分の bd の読み（`--readonly`）で台帳の印を動かさない: 同じ置き場へ 2 回続けて撃つと 2 回目は `Coalesced` で `generated` が進まなかった（2026-10-01 の実測）。
  - 管理 tick は systemd の user の timer の oneshot で走り、その PATH に台帳 client の `bd` が無い。PATH に bd の無い env で `fleet lifecycle write` を撃つと `lifecycle=unreadable`（理由 `ledger`）、`--bd` に絶対 path を渡すと書けた（2026-10-01 の実測）。
  - 入力の印の読み（`crates/scribe2/src/fleet/lifecycle_mark.rs`）: 台帳は `.beads` の manifest の 1 file（embedded）か `issues.jsonl` の stat（files）・main は `refs/remotes/origin/main` の loose の ref か packed-refs の読みで、bd も git も撃たない。順の比べ（`main_order`）だけが git を撃つ。
  - 既存の tick の歯 `seat_tick_rewrites_lifecycle_` の置き場の登録 row の anchor は在らない path（`/repo`）で、台帳の印も main の印も読めない。
- 約束（番号は done の (1)〜(5) と 1:1・約束 6 は done (7)・done (6) は既存の歯 `seat_tick_rewrites_lifecycle_` の緑）:
  1. rules 行 `lifecycle.full_min_s`（kind `LifecycleFullMinS`・形 Int・値 300・enabled・裁定の字 `user 2026-10-01T15:27Z`・裁定日 2026-10-01）を、埋め込み manifest の `lifecycle.age_h.epic-closable` の行の直後（`memo.notes_max_bytes` の前）に 1 本置き、kind は `ALL` の `LifecycleAgeH` の直後（`MemoNotesMaxBytes` の前）に置く。字面から引け、形は Int だけ。memo の 3 行は manifest と `ALL` の末尾のまま。
  2. 管理 tick の周は、判定・打刻・部分の書き直しの後に、次の 5 つを全部満たす周だけ全部の書き直しを 1 回撃つ。
     - manifest を読めた周（rc 1 の error の周でない）で、target の登録 row が在る。
     - 置き場の出力が読める（無い・読めない周は撃たない・出力を作らない）。
     - tick の manifest（`--rules` の写しか埋め込み）に `lifecycle.full_min_s` の行が在る（無い写しは撃たない）。
     - 今の時刻が、出力の `full_at` と tick の撃った記録（約束 3）の ts の新しい方から `lifecycle.full_min_s` 秒以上後（`full_at` が null か読めない周は 0 と読む・記録が無いか読めない周は記録を数えない）。
     - 登録 row の anchor の台帳の印か main の sha が、出力の `inputs.ledger` か `inputs.main` と等しくない。比べは値の等しさだけで、bd も git も撃たない。どちらかの印を読めない周は撃たない。event log の印は比べない（event log と時刻で決まる部分は部分の書き直しが持つ）。
     - 判定と撃つ 1 本は部分の書き直しと同じ file（`crates/scribe2/src/fleet/lifecycle_partial.rs`）に置き、書き手の file（`crates/scribe2/src/fleet/lifecycle.rs`・上限の余地 58 行・main 417e4754）には足さない。
     - 撃つ全部の書き直しは §12 約束 8 (e) の口と同じ 1 本で、観測の 1 周を撃ち、撃った時の印より古くない出力には書かない（coalesce）・lock は待たない（`Busy` で返る）。repo は登録 row の anchor、台帳 client は `seat tick` の新しい flag `--bd B`（無ければ `bd`）。
  3. 撃った周は、返りの 6 値に依らず、置き場の `fleet/lifecycle.tick` を 1 行 `ts=<epoch 秒> wrote=<返りの語>`（語は `Wrote` の語の 6 つ）で一時 file → rename で書く。撃たない周は書かない。読み手は約束 2 の 4 つ目だけ。
  4. tick の rc・stdout の判定行・stderr は、撃った周も撃たない周も変えない（部分の書き直しと同じく返りは字にしない）。
  5. `seat` の使い方の 1 行と help の seat の頁の `tick --state-dir S --target S:W [--rules F]` を `tick --state-dir S --target S:W [--rules F] [--bd B]` にする。help の seat の頁の FORM は使い方の行の逐語の写しのまま（`crates/scribe2-boundary/tests/e2e/main.rs` の `cli_help_pages_match_the_live_form_and_every_subcommand` が一致を測る・本文は変えない）。
  6. 管理 tick の全部の書き直しは、約束 2 の 5 つを全部満たした周にだけ §20 の数えの 1 本（`crates/scribe2/src/fleet/lifecycle_mark.rs` の census_anchors・行 j）を撃ち、One を返す周だけ撃つ。引数は置き場・登録 row の anchor（repo）・tick がその周の判定の前に読んだ event の列（event log を読み直さない）。Many と Foreign の周は撃たず、撃った記録も書かない（約束 3 の撃たない周）。5 つの比べは今のまま bd も git も撃たない（数えの git は 5 つを満たした周だけ）。done は (7)（done (6) は既存の歯）。
- 閉包: rules 行は id の字で引く（新しい code は `RuleKind` の変種を名指すのは rules の module と歯だけ）。`EventKind`・`Stage`・`WaitReason`・`Turn` を名指さない（§12 の閉包と同じ）。`RuleKind` は touches に置く。
- 歯:
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/seat/tick.rs`・接頭辞 `seat_tick_full_lifecycle_`）。置き場は既存の tick の fixture（`crates/scribe2-boundary/tests/e2e/seat.rs` の置き場）に、登録 row の anchor を toy repo にする形を足して使う。toy repo と偽 bd は既存の `fleet_lifecycle_` の歯の fixture（`crates/scribe2-boundary/tests/e2e/fleet.rs`・`.beads` の files の形・偽 bd は `--readonly list` に空の列を返す）を開いて使う。どの歯も `--rules` の写しで `lifecycle.full_min_s` を 60 にし、撃つ形を 1 つ以上持つ（base は `--bd` も行の kind も知らず rc 1 で撃たない＝RED）。toy repo は作る時の `vessel init` で自分の tmp の置き場を git の設定 `<NAME>.stateDir` に名乗る（tick の置き場とは別の在る dir＝そのままでは約束 6 の数えが Foreign で撃たない）ので、どの歯も頭でその設定を tick の置き場の dir に書き直し、設定の値が置き場の dir と等しいことを前提として assert する。tick の PATH は本物の git を引く（`seat_tick_rewrites_lifecycle_` の偽の git は rc 0 で空を返し、設定を読めない）。
    - (a) `full_at` が 2 時間前の出力の置き場で、`issues.jsonl` を伸ばした後の tick の 1 周: 出力の `scope` が full・`full_at` が進み・`inputs.ledger` が今の印・偽 bd の呼びが 1 回以上・`fleet/lifecycle.tick` が `wrote=written`。同じ歯の中で、印を動かさずにもう 1 周撃つと（記録の ts を 2 時間前へ戻した後）偽 bd の呼びが増えず `full_at` が動かない。2 周の rc と判定行と stderr（置き場の path を伏せた字）が等しい。
    - (b) 台帳を動かさず、origin/main の ref を toy repo の別の commit へ進めた周も撃つ。
    - (c) event log だけを伸ばした周（台帳と main は出力と同じ）は撃たず（偽 bd 0 回・記録無し）、同じ歯の中で次に台帳を伸ばした周は撃つ。
    - (d) 印が違っても、`full_at` が 30 秒前の周と、`full_at` が 2 時間前で記録の ts が 30 秒前の周は撃たない（2 形）。同じ歯の中で、両方を 2 時間前へ戻した周は撃つ。
    - (e) 出力の無い置き場（出力を作らない）・`lifecycle.full_min_s` の行の無い写し・登録 row の anchor の台帳を読めない置き場は撃たない（3 形）。同じ歯の中で、出力を置き・行を戻し・台帳を置いた周は撃つ。
    - (f) `--bd` を渡さず PATH に bd の無い env の周は、記録が `wrote=unreadable` で出力が理由 `ledger` の読めない印を持ち、tick の rc と判定行と stderr（置き場の path を伏せた字）は (a) の撃つ周と等しい。
    - (g) `lifecycle.lock` を生きた pid で持たせた周は待たずに記録が `wrote=busy` で出力の bytes が変わらず、tick の rc と判定行と stderr（置き場の path を伏せた字）は (a) の撃つ周と等しい。
    - (h)〔約束 6・done (7)〕登録 row の anchor の toy repo の設定 `<NAME>.stateDir` に別の在る dir を書いた置き場で、(a) の撃つ形の周（台帳を伸ばし、`full_at` と記録の ts が 2 時間前）は撃たない（偽 bd の呼びが増えず・記録が無く・`full_at` が動かない＝Foreign）。同じ歯の中で、設定を置き場の dir に書き直した周は撃つ（`wrote=written`＝One）。続けて撃つ形を戻し、tick の target と別の target（別の window）で、設定の無い別の tmp の repo を anchor にした登録 row を足した周は撃たず（Many）、その row の退役の行を足した周は撃つ。足した周は、tick の target の登録 row（seq の最大）の anchor が toy のままであることを前提として assert する（同じ target の row を足すと tick の anchor が設定の無い repo に替わり、台帳を読めない理由で撃たない＝数えに届かない）。どの周も tick の rc と判定行（置き場の path を伏せた字）が等しい。(a)〜(g) の置き場の登録 row は toy repo を anchor にした 1 件だけで、toy repo の設定は頭の書き直しで tick の置き場を名乗る（数えが One）。
  - rules（既存の `crates/scribe2-boundary/tests/e2e/rules.rs`・接頭辞 `rules_lifecycle_full_min_s_`・1 本）: 約束 1 の行の id・kind・形・値・enabled・裁定の字と裁定日・置き場（manifest の `lifecycle.age_h.epic-closable` の直後で `memo.notes_max_bytes` の前・`ALL` の `LifecycleAgeH` の直後で `MemoNotesMaxBytes` の前）・字面から引けること・文字列の値の写しが形で断られること・裁定の字が base の行 `floor.timeout_s` と違うこと。
  - 直す既存の歯（どれも新しい kind を名指すか数か窓が変わるので base で落ち、retroactive の札は要らない）。数と skip の字は書かず、本行を撃つ時の main の値から 1 つ増やす（rules 行を足し引きする便が先に着地すると値が動く）:
    - 数の pin: `rules_embedded_manifest_is_valid_and_covers_all_kinds`（行数）・`rules_embedded_manifest_declares_one_capability_row_per_role`（kind の母集団）・`rules_external_form` の snapshot（rows= と kinds= の 2 行）を、それぞれ main の値 +1 にする。
    - 末尾からの窓（`.rev().skip(n)`）が `LifecycleFullMinS` の挿入点より前に掛かる歯の n を 1 つ増やす: `rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling`（kind と行）・`rules_floor_timeout_row_precedes_the_drafts_cap_rows`（kind と行）・`rules_drafts_cap_rows_are_the_last_two_kinds_and_rows`（kind と行）・`class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id`（kind）・`rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all`（`LedgerDeniedWrites` からの列に `LifecycleFullMinS` を `LifecycleAgeH` の直後へ足し、末尾の窓の n も 1 つ増やす）。
    - 窓が挿入点より後ろだけに掛かる歯は直さない: `rules_memo_rows_are_the_last_three_kinds_and_rows`（memo の 3 つ）と `rules_index_rows_are_the_last_two_kinds_and_rows`（末尾の index の 2 つ）。
    - `seat_usage_external_form` の snapshot と、`crates/scribe2-boundary/tests/e2e/seat.rs` の tick の口の字の定数（約束 5 の字にする・読む歯は `seat_autonomy_subcommands_are_gone_from_the_usage`・`seat_inject_subcommand_is_gone_from_the_usage`・`seat_working_memory_subcommands_are_gone_from_the_usage` の 3 本で、base の使い方の行に新しい字が無いので base で落ちる）。
    - 母集団の数え方: 本行を撃つ時の main で rules の e2e（rules.rs と rules/embedded.rs）の `.rev()`・`ALL.len()`・`rows().len()` の pin を grep で全部数え、窓が挿入点より前に掛かるものを上の列と照らす（main 7581c7f2 では直す歯が上の 7 本と snapshot の 2 行で、後ろだけに掛かるのが memo と index の 2 本）。
  - 既存の歯 `seat_tick_rewrites_lifecycle_` は本文を変えずに緑（anchor が在らない path で印を読めず撃たない・同じ歯の中の bd と git の呼び 0 のまま）。
- 触らない: §12 の書き手と契機 (a)(d)(e)・§13 の部分の書き直し・印の読みと順の比べ・古さの印・`fleet lifecycle write` の口・tick の判定の列と判定行の形・tick の unit の導出（台帳 client を unit に載せるのは seat-heartbeat.md §25 の行 ad）。
- 限界:
  - 台帳か main の変化が出力に入るのは、最長で下限の 300 秒と tick の周期（`seat.tick_interval_s`）の和の後。
  - 書き直しの間（空いた host で約 4 秒・gate が走る host で約 14 秒）は lock を持つので、ほかの書き手は `Busy` で飛び、次の周が拾う。
  - 撃った記録は返りに依らず書くので、`Busy`・`Unreadable` の周も次の試みは下限の後になる。
  - 撃つ周の tick の process は、書き直しの間だけ約 275 MB の RSS と 4〜14 秒の延長を抱える。判定と合図の注入は書き直しの前に済むが、書き手が panic すればその周の tick は判定行を出さずに異常終了する（src は unwrap と expect を lint で持たない）。
  - 置き場の anchor が 2 つ以上の置き場（§20 の Many）と、別の置き場を名乗る repo の席（Foreign）の tick は全部を書き直さない（約束 6）。その置き場の出力はほかの契機（§12 約束 8）でだけ進み、Foreign の席の tick はその repo の置き場の出力も書き直さない（tick は自分の置き場だけを書く）。撃つ周は anchor の数だけ設定の読みを撃つ。
  - systemd の timer の PATH に bd が無い host では、行 ad が unit に台帳 client を載せるまで記録が `wrote=unreadable` のまま。
  - 登録 row の anchor の台帳の印か main の印を読めない置き場と、`lifecycle.full_min_s` の行の無い写しでは、tick は全部の書き直しを撃たず、撃った記録も書かない（何もしない側に倒す）。その置き場の出力は、ほかの契機（§12 約束 8）の全部の書き直しでだけ進む。
- 却下:
  - event log の印も比べる（event は tick の周ごとに伸びうるので、台帳が動かなくても下限ごとに全部を書き直す・event と時刻の部分は部分の書き直しが持つ）。
  - 印を順（git の祖先の関係）で比べる（tick の周ごとに git を撃つ・「動いた」は等しさで足り、順は書き手が lock の中で比べる）。
  - 消費側が口（`fleet lifecycle write`）を周期で撃つ手順にする（散文の手順で器が測れない・N2）。
  - 書き直し専用の timer unit を足す（置き場ごとに 2 本目の unit・ADR-0030 は席ごとに 1 組）。
  - 下限を持たない（台帳が数秒ごとに動く周に毎周 4 秒の CPU）・下限を code の定数にする（裁定を通らない閾値・C5）。
  - tick が `fleet lifecycle write` の子 process を撃つ（読みは同じで process が 1 つ増える。口の出力は `Written`・`Unchanged`・`Coalesced`・`Discarded` を同じ show の字で出すので、撃った記録の返りの語を 6 つに分けられず、分けるには口の出力の形を変えることになる）。
  - systemd の user の env（environment.d）で PATH を足す手順にする（器が見えない host の前提で、器は env を読まない・C2.2）。
  - 数えを 5 つの比べの中で撃つ: 撃たない周にも設定の読みを撃つ（5 つの比べは bd も子 process も撃たない）。

## 20. 1 つの置き場に 2 つ以上の repo の席が在る周の判じ方 — 置き場の anchor を登録 row と repo が名乗る置き場で数える 1 本を置き、2 つ以上なら全部の書き直しが `unmeasured` に `multi-anchor` を名指す（契約表の行 j・FR90・§5.2・§12 と §19 の限界・memo s2-07l.739）

やさしく言うと: 局面の出力は置き場（state dir）ごとに 1 つで、全部の書き直しは 1 つの repo の台帳と main しか読まない。ところが 1 つの置き場には、別の repo の席も登録されうる（本 repo の置き場には 3 つの repo の席が在る）。どの repo が「この置き場の repo」かを、その repo の設定が名乗る置き場で決める。この置き場の repo が 2 つ以上なら、出力に「この種類は 1 つの repo しか測っていない」と書く。数える 1 本は、後の行 i（§19）の管理 tick も「全部を書き直してよい周か」を決めるのに呼ぶ（出力が repo の間を行き来しないように）。

- 何が起きているか（main be838e18・verified）:
  - §5.2 の `unmeasured` の閉じた語に `multi-anchor` が在るが、出す code は 0 件（crates に字が無い）。§12 の限界が「1 つの state dir が 2 つ以上の anchor を持つ周の判じ方は後の行が決める」、§19 の限界が「anchor の違う席が 2 つ以上在ると、tick ごとに違う repo の印と比べ、下限ごとに違う repo で書き直しうる」と書く。
  - 席の登録 row は event log に在り、`replay` の `registrations`（鍵は役割と anchor・退役の行が同じ鍵を外す）に畳まれる（`crates/scribe2/src/fleet/replay.rs` の `apply_registration`）。
  - repo が名乗る置き場の読みは `crates/scribe2/src/hook/vessel.rs` の `state_dir` の 1 本（git の設定 `<NAME>.stateDir` を git の 1 回で読む・未設定は None・env も HOME も読まない）。hook と pipe の口は同じ 1 本で置き場を解く。
  - 本 repo の置き場（2026-10-02・event log を読んだ実測）: 生きた登録の鍵 7 つ・anchor 3 つ。3 つとも dir が在り `.beads` を持つ。本 repo の anchor の設定だけがこの置き場を名乗り、ほかの 2 つは別の在る置き場を名乗る（その 2 つの repo の便と局面の出力はその置き場に在る）。
  - 全部の書き直し（`crates/scribe2/src/fleet/lifecycle.rs` の `gather`）は `unmeasured` に問いの unreflected-unreadable を足し、行 a1 と行 b1 の関数が ledger-prefix・srs-unreadable・table-unreadable を足す。部分の書き直し（行 d）は前の `unmeasured` をそのまま持つ。発話の部品は置き場の event log の全部の発話から作る（repo に依らない）。`gather` は台帳の次に event log を読む。
  - 行 i（§19・未着地）の管理 tick は、登録 row の anchor の印が出力の印と違う周に、その anchor を repo にして全部を書き直す。数えが無いまま本 repo の置き場で行 i が着地すると、別の置き場を名乗る 2 つの repo の席の tick が、この置き場の出力をその repo の部品で書き直し、本 repo の席の tick と下限（300 秒）ごとに入れ替わる。そこで本行を先に着地させ、行 i は本行の数えを呼んで 1 つの周だけ撃つ（§19 約束 6・行 i の depends に本行）。
  - `lifecycle.rs` は幅で数えて 1442 行（R-C4-2 の 1500 まで 58 行）・`crates/scribe2/src/fleet/lifecycle_mark.rs` は 1290 行（余地 210 行）。
- 前提: 無い（main の上に乗る・depends を持たない）。
- 約束（番号は done と 1:1）:
  1. **置き場の anchor の数え 1 本**（lifecycle_mark.rs の全部の書き直しの入力の読みの区間に置く・後の行 i が呼ぶ口）:
     - 名と形: 関数 census_anchors（可視性 pub(crate)）が、置き場の path（`&Path`）・書き直しの repo の path（`&Path`）・event の列（`&[Event]`）の 3 つを受け、閉じた enum AnchorCensus（可視性 pub(crate)・Debug・Clone・Copy・PartialEq・Eq を derive）を返す。変種は 3 つだけ: One（1 つ）・Many（2 つ以上）・Foreign（外の repo）。台帳も event log の file も読まず（event は呼び手が渡す）、anchor ごとに `state_dir` の 1 本で git を 1 回撃つ。後の行が呼ぶ形はこの 3 つの引数と 3 つの変種で固定し、本行の後の行は名・引数・変種を変えない。
     - 置き場の anchor: event の列を `replay` に通した `registrations` の anchor のうち、別の置き場を名乗る anchor（`state_dir` が返す path を正規化した〔std の canonicalize〕字が、置き場を正規化した字と違う在る dir）を除いたもの。設定が無い・git を撃てない・名乗る path が無い anchor は置き場の anchor に数える（測れない anchor を「別の置き場」に倒さない）。
     - 比べる集合は、置き場の anchor と repo の正規化した字の和（正規化できない path は字のまま）。要素が 2 つ以上なら Many。1 つで repo が別の置き場を名乗るなら Foreign。ほかは One。
  2. **全部の書き直しの名指し**（lifecycle.rs の `gather`）: `gather` が読んだ event の列で約束 1 を撃ち（event log を 2 回読まない）、Many を返す周は `unmeasured` の末尾に reason `multi-anchor` を、part を question・memo・contract・run・row・requirement・epic・commit の 8 つ（§2 の種類の順・utterance を除く）で 1 件ずつ足す。utterance は置き場の event log の全部の発話から作るので名指さない。One と Foreign の周は足さない。どの周も書き直しは止めない（repo の部品は測れている）。
- 設計の線（歯を持たない・審査が読む）: 約束 1 の 1 本は置き場の名乗りの読みを `state_dir` の 1 本で撃つ（2 本目の git の設定の読み手を書かない）。lifecycle.rs の足しは `gather` の数行に収め（余地 58 行を行 r と分ける）、判定は lifecycle_mark.rs に置く。名指しの 8 つの種類の列と語 `multi-anchor` の字は lifecycle_mark.rs の const に置き、`unreflected-unreadable` の const の隣に並べる。lib の 3 本は、tmp に git の repo を作り設定を書く helper 1 本と、登録の event の JSON の字を組む helper 1 本を共有し、lifecycle_mark.rs の足しは歯を含めて 120 行以内に収める（未着地の行 cc〔contract-source.md〕が同じ file に 60 行を見込み、1290 行と足して R-C4-2 の 1500 行に 30 行を残す）。
- 閉包: 新しい code は `EventKind`・`Stage`・`WaitReason`・`Turn` の変種と literal、`RuleKind` の変種を名指さない（§12 の閉包と同じ形）。登録 row は `replay` の返りの欄で読み、`Registration` を literal で組まない（歯の event は JSON の字で置くか e2e の既存の helper `register_anchored_account` で積む）。器の閉包の検査が測るので、verify の最終行に contracts check を置く。
- 歯（接頭辞 anchor_census_〔未着地の名なので backtick で書かない〕・2026-10-02 に crates の fn 名で 0 件・全部の設計 doc の契約表の verify の filter のどれも名に当たらない）:
  - lib（lifecycle_mark.rs の既存の test 区間）: tmp に git の repo を 3 つ（R・B・C）作り、git の設定 `<NAME>.stateDir` を歯ごとに書き分け、登録の event は JSON の字から `Event` の 1 行の読みで作る。どの歯も census_anchors を名で呼び、返りを AnchorCensus の変種と等しさで比べる。
    - (a)〔done (1)〕登録が無い周・R だけ（役割 2 つ）の周・R を `..` を含む path で渡した周は One。同じ歯で、設定の無い B を足すと Many。
    - (b)〔done (1)〕B の設定を別の在る dir にすると One、同じ B の設定を置き場にすると Many（対）。C の設定を在らない path にすると Many（測れない anchor を数える）。B の退役の行を足すと One に戻る。
    - (c)〔done (1)〕R の設定を別の在る dir にして登録が R だけの周は Foreign、同じ置き場に設定の無い B を足すと Many（先に Many を判じる）。
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/fleet.rs`・`Life` の置き場と helper `register_anchored_account`）:
    - (d)〔done (2)〕`Life` の置き場に、設定の無い tmp の git の repo を anchor にした登録 row を積んで `fleet lifecycle write` を撃つと、rc 0 で依存待ちの契約 toy-c2 の部品は書かれ、出力の `unmeasured` の multi-anchor の項が 8 件・part が §2 の種類の順で utterance を持たない（項の列の全体を等しさで比べる）。同じ歯で、その repo の設定に別の在る dir を書き、台帳の file（`.beads/issues.jsonl`）を伸ばした後の口では multi-anchor の項が 0（設定の書きは出力の印を動かさず、口は印の古くない出力を書き直さないので、出力の `generated_at` が進んだことを同じ周の前提として assert する）、`Life` の repo を anchor にした登録 row だけの置き場でも 0。
  - base で RED の理由: lib は新しい 1 本と enum を名で呼ぶので base で compile できない（機能不在）。(d) は base の書き直しが multi-anchor を出さない。否定（0 件）は同じ歯の肯定と組。
  - 既存の歯（本文を変えない・verify に持つ）: `fleet_lifecycle_` の e2e（`Life` の置き場は登録 row を持たない＝One）。
- 触らない: §5.2 の欄と版と語（`multi-anchor` は既に在る）・`state_dir` の読み・登録 row と `replay`・全部の書き直しの判定と書き手と契機（§12）・部分の書き直し（§13・`crates/scribe2/src/fleet/lifecycle_partial.rs`）・管理 tick（`crates/scribe2/src/seat/tick.rs`・数えを tick から呼ぶのは行 i）・口（`fleet lifecycle write`）・`Wrote` の 6 値。
- 限界:
  - 置き場の anchor が 2 つ以上の置き場（同じ置き場を名乗る repo が 2 つ以上）では、dispatch・land の終端・口の書き直しが repo ごとに出力を入れ替える（出力は置き場ごとに 1 つ・FR90）。本行はそれを名指すだけで止めない。管理 tick が数えを呼んでその置き場で撃たないのは行 i（§19 約束 6）。
  - 外の repo（別の置き場を名乗る repo）を repo に口や dispatch で撃った書き直しは、名指さずにその repo の部品で書く（その repo の部品は測れていて、置き場の anchor の部品が欠けることは Many の判定が持つ）。
  - 名乗りの読みは anchor ごとに git の 1 回で、全部の書き直しの周に anchor の数だけ git を撃つ。
  - 部分の書き直しは前の名指しを持ち、登録 row の変化は次の全部の書き直しで出る。
  - 名乗る path が在らない anchor（置き場を消した repo）は置き場の anchor に数え、Many に倒れうる（名指しが出る側）。
  - 本行の着地から行 i の着地までは、管理 tick は全部を書き直さない（main の今のまま）ので、tick による入れ替わりは起きない。
- 却下:
  - 登録 row の anchor をそのまま数える（名乗りを見ない）: 本 repo の置き場は別の置き場の repo の席を 2 つ持つので常に multi-anchor になり、名指しが意味を失う。tick の入れ替えも止まらない。
  - 出力に repo の欄を足し、tick は出力の repo と同じ anchor の周だけ撃つ: §5.2 の跨版の欄を足し、最初に書いた repo が出力を持つ。別の置き場を名乗る repo を見分けられない。
  - repo ごとに出力を分ける: ADR-0088 と FR90 の「state dir ごとに 1 つ」を変える（SRS と ADR の改訂）。
  - Many と Foreign の周は全部の書き直しも書かない: `Wrote` の閉じた 6 値と stderr の語と tick の記録の語を足し、出力が古いまま残る。
  - 数えを行 i の中で書く（行 i に数えも tick の判定も持たせる）: 行 i の着地までの間も、口と dispatch の書き直しは multi-anchor を名指せない。行 i が先に着地すると、数えの無い tick が本 repo の置き場の出力を repo の間で入れ替える。

## 21. 結びと答えの口が台帳を書いた直後に局面の出力の全部の書き直しを切り離した子で起こす（契約表の行 k・FR90・AC60・§12 約束 8・§14・memo s2-07l.739）

やさしく言うと: 局面の出力は、器が台帳を書いた直後に全部を書き直す約束（FR90）だが、問いを裁定で閉じる 2 つの口（`seat ruling bind` と `seat ruling answer`）の後には書き直しが無く、閉じた問いは次の別の契機まで出力に開いたまま残る。2 つの口が通った（rc 0）直後に、全部の書き直しの口（`fleet lifecycle write`）を自分の binary の子として切り離して起こし、待たずに返る。答えの口は隣の project の面から撃たれるので、口の rc・stdout・stderr と待ち時間は変えない。子を起こせなかった周は、出力に「古い」の印（§14 の ledger-gate）を 1 つ残す。

- 出所: memo s2-07l.739 の穴の 1 つ（FR90 の「直後」の 4 つの契機のうち、結びと答えの口に行が無い）。SRS の字は直さない。
- 何が起きているか（main be838e18・verified）:
  - FR90 は「器が台帳を書いた直後（bind・裁定面の答えの口・land の終端の close・memo の自動の close）」に全部を書き直すと言い、AC60 は 6 つの契機で全部が書き直されることを求める。
  - §12 約束 8 の契機は (a) dispatch の `fire`・(d) land の終端の close の後（着地の本体・`--terminal-only`・`pipe retire` の 3 経路が `after_close` を呼ぶ）・(e) 口（`fleet lifecycle write`）の 3 つ。行 i（§19・未着地）の管理 tick は、最長で下限 300 秒と tick の周期の和の後に書き直すので、「直後」には当たらない。
  - 結びの 1 本（`crates/scribe2/src/seat/ruling.rs` の `bind`）は notes → close（理由 `裁定 <id>`）→ 裁定 event の順に書き、答えの口（同じ file の `answer`）は発話を書いてから同じ `bind` を呼ぶ。どちらの後にも局面の出力の書き直しは無い。呼び手は `crates/scribe2/src/seat/cli.rs` の `ruling_bind` と `ruling_answer`。
  - 全部の書き直し 1 回は wall 4〜14 秒（§19 の実測）で、同じ process で撃つと答えの口の待ち時間がその分延びる。
  - 自分の binary を子として切り離して起こす形は 2 つ在る: `crates/scribe2/src/pipe/dispatch.rs` の `spawn_self`（頭の語が `pipe` に固定・可視性は pipe の中だけ・子の stderr を置き場の `launch.log` へ）と、`crates/scribe2/src/hook/group.rs` の `measure_later`（`fleet usage` の子・`myself` の program・新しい process group の leader・入出力は全部捨てる・待たない・起こせたかの真偽だけを返す）。
  - `after_close` は `round` を通り、行 r（ledger-form §21・未着地）が着地すると `round` は全部の書き直しの前に memo の自動の close も撃つ。
  - 古さの印（§12 約束 4・5）: `unreadable` の印の理由は閉じた 6 語（読む順の `ledger`〜`declaration`）。`ledger-gate` の印は値に台帳の書きの前の印を持ち、それより新しい台帳を読んだ全部の書き直しが消す（§12 約束 6）。門が通した席の台帳の書きには、hook がこの印を付ける（§14・`crates/scribe2/src/hook/stale_gate.rs`）。2 つの口は bd の片でないので、hook はこの印を付けない。
  - 既存の結びと答えの口の歯の偽の台帳（`crates/scribe2-boundary/tests/e2e/seat/ruling.rs` の `Fake`）の repo は `.beads` を持たず、歯は stderr が 0 byte で台帳の読みが 1 回であることを測る。
- 約束（番号は done と 1:1）:
  1. `seat ruling bind` は、結ぶ前に repo の台帳の印（lifecycle_mark.rs の `read_ledger`・子 process を撃たない）を読んでおき、`Ok` で返った周（rc 0）は、自分の binary（`myself`）を `fleet lifecycle write --state-dir <置き場> --repo <repo>`（口が `--bd` を受けた周は同じ `--bd` を足す）の子として 1 回起こす。子の入出力は全部捨て、口は子を待たない（子の起こし方の残りは設計の線）。起こすのは `bind` が返った後＝裁定 event を書いた後なので、子の書き直しは閉じた問いと裁定を両方読む。
  2. `seat ruling answer` が `Ok` で返った周（rc 0）も、約束 1 と同じ 1 本で子を起こす（台帳の印は発話を書く前に読む）。
  3. 起こさない周: 2 つの口が `Ok` 以外（断り・台帳を読めない・event log を読めない・notes の失敗・途中の止まり・発話を書けない）で返った周と、書く前の台帳の印を読めなかった周。途中の止まり（close の後に裁定 event が落ちた周を含む）は、同じ組の撃ち直しが `Ok` で返った周に起こす。
  4. 子を起こせなかった周は、書く前の台帳の印を値にした `ledger-gate` の印を 1 つ足す（行 c の印を 1 つ足す関数 1 本・lock の待ち方は埋め込みの規則の 2 行・返りの 4 値のうち付けた以外は何もしない＝出力の無い置き場では付かない）。印は次に新しい台帳を読んだ全部の書き直しが消す。起こせた周は印を足さない。
  5. 2 つの口の rc・stdout・stderr は、起こした周も起こさない周も起こせなかった周も変えない。口の待ち時間に子の書き直しの時間を足さない。
- 設計の線（歯を持たない・審査が読む）: 起こす 1 本と印を足す 1 本は cli.rs の 2 つの口の `Ok` の腕から呼ぶ 1 つの private 関数に置き（2 つの口で同じ 1 本）、子の起こし方は `measure_later` と同じ形（器の起動の差し替え口 `Invocation`〔core の src の本体に std の `Command::new` を書くと xtask check の core-spawn が赤〕・`myself`・新しい process group の leader・入出力を捨てる・`spawn` の真偽だけ）にする。process group は歯を持たない（e2e の (b) の wall が測るのは入出力の継承まで）。`spawn_self` は広げない（頭の語が `pipe` に固定で、可視性と `launch.log` を変えると dispatch.rs に触る）。`after_close` は呼ばない: 行 r の着地の後の `after_close` は memo の自動の close も撃つが、FR93 の契機は land の終端と dispatch の周だけで、結びと答えの口は memo を閉じない（本行と行 r はどちらが先に着地しても互いの歯を動かさない）。lifecycle.rs と ruling.rs は触らない。
- 閉包: 新しい code は `EventKind`・`Stage`・`WaitReason`・`Turn` の変種と literal・`RuleKind` の変種を名指さない。印の種類は `ledger-gate` の 1 つだけを名指す（§14 の hook の印付けと同じ 1 つ）。器の閉包の検査が測るので、verify の最終行に contracts check を置く。
- 歯（接頭辞 seat_ruling_rewrites_lifecycle_〔未着地の名なので backtick で書かない〕・2026-10-02 に crates の fn 名で 0 件・全部の設計 doc の契約表の verify の filter 1047 個のどれも名に当たらない）:
  - e2e（既存の `crates/scribe2-boundary/tests/e2e/seat/ruling.rs`・`Fake` の置き場）。`Fake` の偽の bd に 2 つ足す: (i) `--readonly` の 2 語目が `list` の撃ちで、その時点の裁定 event の件数を `list.rulings` に残し、`list.hold` が在る周は `list.go` が置かれるまで（上限 30 秒）待ってから `list.ended` を置き、返りは今のまま rc 1 (ii) `close` の撃ちで repo の `.beads` の下の `issues.jsonl` が在れば 1 行足す（台帳の印を書きの前と後で違える）。repo の `.beads` の下に `issues.jsonl`（files の形の台帳の印）を置いた置き場を「印の在る置き場」と呼ぶ。**裏の子の撃ちは calls.log と記録の file を期限（10 秒）つきで待ち、撃たないことを測る側も同じ期限まで待つ**（行 aq の歯の型）。
    - (a)〔done (1)〕印の在る置き場で開いた問いを結ぶと、rc 0・stdout は既存の結びの歯と同じ 1 行・stderr 0 byte で、期限の内に close の後の `--readonly list` の撃ちが 1 回現れ `list.rulings` が 1（裁定 event の後）。
    - (b)〔done (2)・(5)〕印の在る置き場に `list.hold` を置いて `seat ruling answer` を撃つと、口は `list.go` を置く前に rc 0・stdout は裁定 id の 1 行・stderr 0 byte で返り、口の wall は 10 秒より短い（子を待つ実装は上限 30 秒まで返らない）。返った後に `list.go` を置くと、期限の内に `list.ended` が現れ `list.rulings` が 1。
    - (c)〔done (3)〕印の在る置き場で、閉じた問いの結び（断り closed）・逐語が空白だけの答え（断り words-empty）・close を 1 回落とした結び（途中の止まり stage=close）は、どれも期限まで待っても `--readonly list` の撃ちが 0 で、stderr は今の 1 行だけ。同じ歯の中で、途中で止まった組の撃ち直しの結び（rc 0）は期限の内に `--readonly list` を撃つ。さらに同じ歯の中で、印の無い置き場（repo に `.beads` を置かず、置き場に (d) と同じ出力の fixture を置く）で開いた問いを結ぶと rc 0 で、期限まで待っても置き場の `lifecycle.stale` に `unreadable` の印が無い（子が起きれば、子の全部の書き直しは台帳の印を読めずに bd を撃つ前に返り、理由 `ledger` の `unreadable` の印を出力に足す。`--readonly list` の撃ちの数ではこの周を測れない）。その後に同じ置き場で口を通さずに `fleet lifecycle write --state-dir <置き場> --repo <repo>` を撃つと、理由 `ledger` の `unreadable` の印が 1 つ付く（子が起きれば印が見えることの前提の assert）。
    - (d)〔done (4)・(5)〕印の在る置き場に出力の fixture（行 c の書き手の描きの 1 本 `render_output` で組む `lifecycle.json`）を置き、argv[0] を在らない path にして（`arg0`）結ぶと、子を起こせず、rc 0・stdout は (a) と同じ 1 行・stderr 0 byte で、置き場の `lifecycle.stale` に `ledger-gate` の印が 1 つ在り、値は結ぶ前の repo の台帳の印（`read_ledger` の値・close が足した行の前）と等しく、結んだ後の印とは違う。期限まで待っても `--readonly list` の撃ちが 0。同じ歯の中で、同じ置き場の写しを本物の argv[0] で結んだ周は期限の内に `--readonly list` を撃ち、`ledger-gate` の印が無い。
  - base で RED の理由: base の 2 つの口は子を起こさず印も足さないので、(a)(b)(c) の期限の内の `--readonly list` の撃ちと (d) の `ledger-gate` の印が無い（機能不在）。否定（撃ち 0・印が無い・stderr 0 byte）はどれも同じ歯の肯定と組。
  - 既存の歯（本文を変えない・verify に持つ）: `seat_ruling_bind_` と `seat_ruling_answer_` の e2e（`Fake` の repo は `.beads` を持たず、約束 3 で起こさない＝stderr 0 byte と台帳の読み 1 回のまま・偽の bd の 2 つの足しは list の撃ちと `.beads` の在る repo にしか効かない）。
- 触らない: `bind` と `answer` の断りの順・書きの順・stdout の字・rc（ruling.rs）・全部の書き直しの書き手と契機 (a)(d)(e)（§12）・口（`fleet lifecycle write`）の形・`spawn_self`・`after_close`・行 r の memo の自動の close・行 i の管理 tick・§14 の hook の印付け。
- 限界:
  - 子の書き直しは口が返った後に走り、出力が閉じた問いを映すまで 4〜14 秒かかる。その間に読んだ面は前の出力を見る（台帳の印の比べ〔§15〕で古さは読める）。
  - 子が lock を取れない周（`Busy`）と読めない周（`Unreadable`）は、子の口の形のまま（`Busy` は印を足さず、`Unreadable` は理由の印を足す）。子の結果は口の stdout にも stderr にも出ない。
  - 子を起こせなかった周の `ledger-gate` の印は、出力の無い置き場には付かない（出力はまだ誰も読んでいない）。
  - 答えを続けて撃つと子が重なりうる。どの子も lock を取って全部を書き直すか `Busy` で返る。
- 却下:
  - 口の process の中で全部の書き直しを撃つ（`after_close` と同じ形）: 答えの口の待ち時間が 4〜14 秒延び、隣の project の面を止める。
  - `bind` の中（ruling.rs）で起こす: `bind` は台帳と event log の書き手で、口の腕の 1 本で同じ効きになる。
  - `spawn_self` を広げて使う: 頭の語の `pipe` と可視性と `launch.log` を変え、dispatcher の行と write-set が重なる。
  - `after_close` を呼ぶ: 行 r の着地の後に結びと答えの口が memo を閉じ、FR93 の契機を黙って広げる。
  - 起こせなかった周に `unreadable` の印を足す: 理由の閉じた 6 語に語を足す（§5 の跨版の語）。`ledger-gate` は「器が台帳を書いたのに書き直しが読んでいない」をそのまま表す。
  - 起こせなかった周に stderr の 1 行を足す: 口の stderr を変える（面が rc 0 の stderr を読まない保証が無い）。
  - 印を読めない repo でも起こす: 既存の歯（stderr 0 byte・読み 1 回）の置き場で子が台帳の無い dir に読めない印を足すだけになる。
