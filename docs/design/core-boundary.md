# 設計: core の境界 — core-lines は本体だけを数え、子 process を撃つ I/O 面は境界 crate へ分ける（binary は 1 つ）

- 要件: [FR47](../../design-intent/spec/srs.html#FR47) 契約表 / [FR48](../../design-intent/spec/srs.html#FR48) 受付の余地 / [NFR2](../../design-intent/spec/srs.html#NFR2) 大きさの上限
- 憲法: [C4](../../design-intent/spec/constitution.html#c4) core の大きさは CI の deny gate・上限の変更は A2 / [C13.2](../../design-intent/spec/constitution.html#c13) 境界 crate / [C2.2](../../design-intent/spec/constitution.html#c2) crate 名は NAME から導く / [C11](../../design-intent/spec/constitution.html#c11) lint / [C12.3](../../design-intent/spec/constitution.html#c12) 歯の比 / [A2](../../design-intent/spec/constitution.html#a2) 閾の変更は user 裁定 / [N4](../../design-intent/spec/constitution.html#n4) C4 の上限を超える変更は拒む
- 決定: [ADR-0033](../../design-intent/decisions/ADR-0033-core-lines-count-source-only-and-io-lives-in-a-boundary-crate.html)（core-lines の母集団と境界 crate・user 裁定 2026-09-15）/ [ADR-0001](../../design-intent/decisions/README.html)（単一 binary・不変）/ [ADR-0009](../../design-intent/decisions/README.html)（Rust で閉じる）
- 台帳: `s2-07l.198`（本設計の契約群の親）・出所の実測 = `s2-07l.303` run 2 / run 3（受付の core の余地で 2 度断られた 2026-09-15）
- 土台: [rules-manifest.md](./rules-manifest.md) §4（R-C4 行と行の数え方）/ [contract-source.md](./contract-source.md) §3（受付の余地）/ [pipeline.md](./pipeline.md) §5.3（純移動の機械証明）

やさしく言うと: 器の本体（core）の行数が上限の 9 割に来て、大きめの契約が受付で止まるようになった。上限は上げず、(1) 数え方から「test の行」を外し（test は別の上限が縛っている）、(2) 外の process を起こす部分だけを別の箱（境界 crate）に移して、本体を「判定と型」に絞る。実行 file は 1 つのまま。

## 1. 何を解くか（実測 2026-09-15・main 17943e7）

| 事実 | 値 |
|---|---|
| core-lines（R-C4-1 の測定値 / 上限） | 36,142 / 40,000（90.3%） |
| うち in-file の歯（`#[cfg(test)]` 区間） | 8,591 行（24%） |
| 子 process を撃つ file（`Command::new` の在る `.rs`） | 14 file / 9,888 行（file 全体・撃つ関数だけならその一部） |
| 受付が size M の契約に許す core の `.rs` 本数（300 行 × N ≤ 余地 3,858） | 12 本 |

- 受付（[contract-source.md](./contract-source.md) §3）は core の余地に「write-set の core の `.rs` 本数 × size」を当てる。余地が細るほど大きい契約が受付で止まり、契約を割る手作業が planner に戻る（`s2-07l.303` は 3 度目で通った）。
- 歯は R-C4-3（歯 / src の比）でも数えている＝R-C4-1 と二重計上（`check_sizes.rs` は `split_test_src` を持つが core-lines がそれを使っていない）。
- 上限 R-C4-1 を上げる案は採らない（憲法の順位: 成長の抑止が 2 位・上げても同じ問題を先送りする）。

## 2. core-lines の母集団 = src の本体（user 裁定 2026-09-15・A2）

- `measure_core_lines` は各 file の `split_test_src(width)` の **src 側だけ**を合計する（in-file の歯を外す）。上限 40,000 と行の数え方（幅の正規化・R-C4.line-width）は不変。
- 歯の量は従来どおり R-C4-3 が縛る（in-file の歯は今も test 側に数えている＝母集団の移動ではなく二重計上の解消）。
- 受付の余地（contract-source.md §3）は同じ式を core 側から呼ぶので自動で追随する（式は 2 か所・fixture で突合する歯が守る）。
- **core-spawn の検出線**（§5 の表の 2 行目・§6 (1) の便＝行 a が同じ便で足す）: measure `measure_core_spawn` を `measure_core_lines` の直後に宣言順で足す。母集団 = `core_dir/src` の `.rs` で `Command::new` を含む行、fact 行は `core-spawn=<n>/<files>`（件数 / file 数）、ok は**常に true**（数だけ出す検出線・値を持たない・deny へ倒すのは最後の移動の便＝§5 の bullet）。check の measure の列と check_tests の `SUMMARY_PIN` に core-lines の直後で載せる。
- 効果（実測）: 36,142 → 27,551（68.9%）。余地 3,858 → 12,449。
- 裁定 id = `user 2026-09-15T09:5xZ`（逐語は `s2-07l.198` notes・AskUserQuestion 問 1「in-file の歯を core-lines から外す (Recommended)」）。manifest の R-C4-1 行は値を変えないので行は不変（C5 の対象外）。数え方の文は [rules-manifest.md](./rules-manifest.md) §4 に 1 行足す。

## 3. crate の構成（user 裁定 2026-09-15・A2・ADR-0033）

```mermaid
flowchart LR
  core["crates/&lt;NAME&gt;<br/>lib vessel = core<br/>型・判定・file の読み書き"]
  boundary["crates/&lt;NAME&gt;-boundary<br/>lib + bin &lt;NAME&gt;<br/>子 process を撃つ面"]
  xtask["crates/xtask"]
  boundary -->|depends| core
  xtask -.->|measures| core
  xtask -.->|measures| boundary
```

- **core** = `crates/<NAME>`（lib `vessel`・現行）。型・閉じた enum・判定の純関数・state dir と repo の file の読み書き・event log。**`std::process::Command` を持たない**（xtask check の measure `core-spawn=0/N`・deny・§5）。
- **境界 crate** = `crates/<NAME>-boundary`（新規・名は NAME 定数から導く・C2.2）。子 process を起こす面 = tmux（`seat` の注入・立て直し）/ git（land・flip-check の base・vessel update）/ claude（headless の runner・lens・席の起動行）/ curl（fleet usage）/ cargo（vessel update・gate の verify 行）/ systemd（tick の unit）/ `sh -c`（gate の verify 行の実行）。**binary `<NAME>` は境界 crate が持つ**（`[[bin]] name = "<NAME>"`・`main.rs` を移す）。ADR-0001 の単一 binary は不変（crate は 2 つ・実行 file は 1 つ）。
- 依存は一方向: 境界 → core。core は境界を知らない（core の判定関数は「撃った結果」を値で受ける＝いまの `Outcome` / `Step` / `Measured` の形をそのまま使う）。
- **xtask の Layout**: `core_dir` = `crates/<NAME>`（NAME を持つ `name.rs` の在る member）・`boundary_dir` = `crates/<NAME>-boundary`（在れば）。R-C4-1 は `core_dir/src` だけ・R-C4-2（file-lines）と R-C4.line-width は全 member の `src`（現行どおり）・R-C4-3 は core + 境界の合計。**歯の置き場**: binary を引く歯（`tests/e2e/` の全体と snapshots・`CARGO_BIN_EXE_<NAME>` は同じ package の bin にしか渡らない＝実測 2026-09-15・8 file）は bin と一緒に境界 crate の `tests/` へ純移動する。in-file の歯（`#[cfg(test)]`）は module と一緒に動く。以後の契約の verify 行は e2e の歯を `-p <NAME>-boundary`・in-file の歯を `-p <NAME>` で名指す。
- **境界 crate の上限**（抜け穴の fence）: rules 行 `R-C4-5`（kind `BoundaryLines`・`ValueShape::Int`・deny）。core の外へ押し出して逃げる形（core を減らすために境界へ判定を持ち込む）を塞ぐ。行を足すのは段 3 (i)（値 = 移した直後の実測 × 1.2 を切り上げ・同じ裁定 id で manifest と憲法 §3 の cell に書く・C10）。段 2 の純移動は機械証明（`MoveSummary`）で判定を境界へ持ち込めないので、行の無い期間に抜け穴は無い。裁定 id = `user 2026-09-15T09:5xZ`（問 2「境界 crate へ分ける (Recommended)」）。
- 憲法 §3 の rules 表（constitution の閾値セル）は manifest の写し＝R-C4-5 を足す周は生成区間を再生成する（xtask の drift 歯）。C4 の条文は「core size / module size / test-to-source ratio / function granularity」の 4 つを名指す＝境界 crate の上限は C4 の「module size」の系ではなく新しい bound なので、**ADR-0033 が C4 の適用を記録し、条文は変えない**（N4 に当たらない: 条文の改訂でも C4 の bound の超過でもない）。

- tests/ の対の一覧は行 b の起票時点の HEAD を写したもので、その後の着地で `crates/scribe2/tests/` と `crates/scribe2/src/snapshots/` に増えた file は同じ `~旧` / `+新` の対で行 b の write-set に足す。`src/snapshots/` の新しい名は insta が bin の module path から付けるので、bin 名が `scribe2` のままの間は接頭辞も `scribe2__tests__` のまま（crate 名 `scribe2_boundary` にはならない）（2026-09-22 に e2e の .rs 8 本と snapshot 4 本を追加・便の worktree の HEAD が母集団）。

- 移す file を write-set か散文で名指す設計 doc は、同じ便で新しい path に書き換える（行 b の write-set の docs/ の列がその母集団・起票後に行が増えた doc は同じ列に足す＝2026-09-22 に ledger-form / pipeline-question / vessel-hook の 3 本を追加。便の worktree で `contracts check` が findings 0 になることが完了の形）。
- bin 本体の `args` の読みが境界 crate へ移ると core の env_reads の母集団が 1 減る（6 → 5）。`crates/xtask/src/env_reads.rs` の in-file の歯が持つ母集団の下限は移動後の数へ合わせる（0/0 の緑を塞ぐ目的は保つ・下限は 1 以上のまま）。

- 入口の flip-check は移した file を rename の対で読む（[pipeline.md](./pipeline.md) §53 / 行 av・本行はその着地後に撃つ）。移動で test 区間に差が出る file（include_str の相対 path が動く e2e 等）には `moved` の札を置いて RED の要求を免除する（札の本数は rules 行の上限の内側）。
- gate の lens 入力は docs の path 置換を畳んで渡す（[gate-cost.md](./gate-cost.md) §41 / 行 ah・本行はその着地後の世代の器で gate を撃つ）。本行の便 153839Z（2026-09-22）は verify 9/9 rc 0 のまま lens 入力 792551 byte が cap 150000 を超えて Gated INCONCLUSIVE になり、内訳は docs/design の契約表の行の path 置換が 754352 byte・code 側が 38199 byte だった。畳んだ後の本文は code 側の diff と印の行だけになる。
- 本行の便 171521Z（2026-09-22・行 ah の着地後の器）は elided=226/484 でも lens 入力 281110 byte が cap を超えて Gated INCONCLUSIVE になった。残ったのは隣り合う契約行の同時書き換え（1 塊でない hunk 19）と、移動で空になった dir の名指し（13）で、[gate-cost.md](./gate-cost.md) §42 / 行 ai がその 2 つを畳む（写しで 86507 byte）。本行はその着地後の世代の器で gate を撃つ。

## 4. 境界の判定（何を境界 crate へ移すか・閉じた規則）

移す = **`std::process::Command` を構築する関数と、その引数を組み立てるためだけの関数**。判定（rc や出力を読んで typed な値へ倒す関数）は core に残す。

| core（残す） | 境界 crate（移す） |
|---|---|
| `Outcome` / `Step` / `Fired` / `Measured` / `Verdict` / `Refuse` の型と判定順（`decide` 等） | `run_line_captured`（`sh -c` の実行）/ `confine` の `systemd-run` の起動 / `admission` の probe |
| `derive_launch` / `with_model` / `fill_launch`（起動行の純関数） | tmux の `send-keys` / `list-panes` / `new-window` / `display-message` |
| `select` / `choose_or_wait` / `until` | `fleet::usage` の curl 起動・`headless::build` が組んだ `Command` の spawn |
| flip-check の判定（xtask・対象外） | land の `git`（archive / worktree / push）・`vessel update` の git と cargo |
| 台帳の数えの組み立て（席の指示文の `{ledger}`） | 台帳の読みが撃つ `bd --readonly` の子 process |

- 規則は 1 つ: **core に `Command::new` が 0**（xtask check の measure・§5）。「どの関数が I/O か」の判断を散文で持たない（C1.2 / N2）＝lint で決まる。
- 境界 crate の関数は「引数 → 子 process の起動 → 生の結果（rc / stdout / stderr の bytes）」だけを返し、解釈しない（解釈は core の純関数）。これも lint で守る: 境界 crate から core の判定関数を呼ぶのは可、core から境界を呼ぶのは依存の向きで不可（Cargo が拒む）。
- 例外なし。`env!` / `std::fs` / `std::net` は core に残る（C2.2 の env は元々読まない・fs は state dir と repo の読み書きで core の責務・net は無い）。

## 5. xtask check の measure（構造で止める）

| measure | 母集団 | 判定 |
|---|---|---|
| `core-lines` | `core_dir/src` の src 側（§2） | ≤ R-C4-1（deny・既存） |
| `core-spawn` | `core_dir/src` の `.rs` で `Command::new` を含む行 | 0 / N（deny・新規・値は持たない＝0 固定・`env-reads` と同型） |
| `boundary-lines` | `boundary_dir/src` の src 側 | ≤ R-C4-5（deny・新規・R-C4-5 が無い周は measure を出さない） |
| `file-lines` / `line-width` / `test-src-ratio` | 全 member の `src`（現行） | 不変 |

- `core-spawn` は §6 (1) の便で足し、移動が終わるまでは **検出線**（数だけ出す・`R-C12-1` と同型の enabled=false）として記録し、最後の移動の便で deny へ倒す（同じ裁定 id・C12.4 の型を借りるが rules 行は持たない＝0 固定の shape 検査）。
- 歯: `check_sizes.rs` の in-file の歯（fixture の `#[cfg(test)]` 区間が core-lines に入らない / boundary の上限を超える fixture / core に `Command::new` の在る fixture が名指される）。

- `core_dir/tests/e2e` を直に読む measure が 2 本在る（`crates/xtask/src/polarity.rs` の `measure`＝極性の snapshot と e2e の file の走査・`crates/xtask/src/check_facts.rs` の e2e の走査）。tests/ が境界 crate へ移る便（行 b）は、この 2 本を `boundary_dir` が在れば `boundary_dir/tests/e2e`、無ければ従来の `core_dir/tests/e2e` を読む形に直す（Layout の `boundary_dir` の 1 判定・2 本目の読み手を作らない）。移した後に `cargo xtask check` が赤にならないことを行 b の done で見る。

## 6. 契約の列（段の中は並列・全部 `s2-07l.198` の子）

| 段 | 契約 | size | 依存 | write-set の芯 |
|---|---|---|---|---|
| 1 | (a) core-lines の母集団を src 側に（§2）+ `core-spawn` の検出線 | S | ADR-0033 | `xtask/check_sizes.rs` / `rules-manifest.md` の 1 行は planner |
| 1 | (b) workspace に `crates/<NAME>-boundary` を足す（lib + `main.rs` の移動 + `[[bin]]`）+ `crates/<NAME>/tests/` の純移動（binary を引く e2e の歯と snapshots・§3）+ Layout の `boundary_dir` | M（純移動） | ADR-0033・A3 = 非該当（依存 OSS を足さない）・走行中の便が全部 Landed した後（verify 行の `-p` を壊さない） | `Cargo.toml` / `crates/<NAME>-boundary/Cargo.toml` / `crates/<NAME>/Cargo.toml` / `xtask/workspace.rs` / `tests/` の移動 |
| 2 | (c)〜(h) 純移動 6 便（module ごと: `pipe/confine+admission` / `pipe/land+follow+stop+mod` / `seat/cycle+mod` / `fleet/usage+cli` / `headless` / `hook/vessel+account`） | S〜M（各 ≤ 5 file） | (b) | 移す関数の file と境界 crate の新 module・呼び手の `use` |
| 3 | (i) `core-spawn` を deny に・`R-C4-5` 行を足す（値は実測で確定・憲法 §3 の cell も同じ周） | S | (c)〜(h) | `xtask/check_sizes.rs` / `rules/manifest.toml` / `rules/mod.rs` |

- 純移動の便は [pipeline.md](./pipeline.md) §5.3 の純移動の機械証明（`MoveSummary`）で lens に渡る。crate を跨ぐ移動は `use` の path が必ず変わる＝残差分に `use` を許す既存の規則の内側。
- 各便の size は「1 file あたりの増分の見積」で、**消える側は `~` 接頭辞**（着地で消える file・[contract-source.md](./contract-source.md) §24・`+新` と `~旧` の対が純移動の宣言形）、受ける側は新規 file（`+`・余地 = 全量）。縮むが残る面だけが `-` である。
- **(b) の面の測り直し（verified 2026-09-20・main f678bd0）**: 移る対象は `crates/scribe2/tests/` の tracked な **35 項目**（歯の file 21 + 外形 snapshot 14）で、記録当時（2026-09-15）の一覧とは違う——`s2-07l.479` の席の自律機能の削除と ADR-0045 の役割の統合で、消えた席の面の歯の file と役割別の brief の snapshot が落ち、起動の列と停止と口座の再開の歯の file と統合後の brief の snapshot が増えた。**焼く直前に一覧を測り直す**（行に写した 35 項目は測った日の値であって固定の規範ではない）。
- **(b) が設計 doc に及ぶ範囲（行の write-set が 14 doc を持つ理由）**: 他の設計 doc の契約表の行が `crates/scribe2/tests/…` の path を write-set に持っており、移した後は契約表の検査がそれらを `write-set-item-unresolved` で落とす（CI が永久に赤・[contract-source.md](./contract-source.md) §24 の memo の型そのもの）。したがって (b) は**その日に該当する設計 doc 全部**の行の path を同じ便で置き換える。この面が行を事実上すべての doc と交差させるので、(b) は走行中の便が全部 Landed した後の単独の周に置く（順序の制約は設計上のもので、実際の時機は起こす側が決める）。
- 検証の形（base で RED）: (a) `core_lines_exclude_in_file_tests` / (b) `layout_finds_the_boundary_crate`（xtask・新しい歯）+ 境界 crate の e2e が bin を引ける（移動した歯が移動先で全部緑・本数が base と同じ・**行の verify は filter を置かず境界 crate の e2e を丸ごと撃つ**＝移動先で 1 本でも落ちれば赤）/ (c)〜(h) 移動ごとに「core の `Command::new` の件数が N → N-k」を pin する歯（§5 の検出線の値・母集団つき）/ (i) `core_spawn_is_denied_when_nonzero`。
- **段 2 / 段 3 の形は §9 が上書きする**（2026-09-24 の測り直し）: 撃つ関数は core の中から呼ばれているので純移動は成立せず、段 2 は「起動の記述と差し替え口」の置換 6 便（行 c〜h）、段 3 は行 i になる。上の表の段 2 の行と、(c)〜(h) の件数を pin する歯の形は記録当時の見積である。

## 7. 却下案（設計固有）

- **R-C4-1 を上げる（44,000 等）**: 成長の抑止に反し、同じ受付の断りを数か月先送りするだけ。A2 の grill で「上げない」を推奨し裁定された。
- **pure な判定を新 crate `<NAME>-core` へ出す（I/O を残す）**: 移動量が 27k 行で純移動の証明が長引く。I/O を出す方が 3〜5k 行で済み、`Command::new` 0 の lint で境界が機械的に決まる。
- **crate を分けず module の分割と削減だけ**: 数百行しか減らず 90% は続く。上限に近い状態で受付が契約を割らせる運用が残る。
- **境界 crate に上限を置かない**: core から境界へ判定を押し出して core-lines を下げる抜け穴が開く（AI は抜け穴を突く・user 指摘 2026-09-14）。

## 8. 後続

- 契約表の行（contract-source.md §3 の Derived 形）は本 doc の §6 を出所に planner が起票する（`touches` / `surfaces` で write-set を導出）。
- v3 の材料（`s2-07l.42`）: 境界 crate の関数の形（引数 → 生の結果）は folio2 と共有できる「器の I/O 面」の芽。

## 9. 段 2 / 段 3 の測り直しと行 c〜i（2026-09-24・main b82be74・§4 の「移す関数」と §6 の段 2 / 段 3 の形を上書きする）

やさしく言うと: 外の process を起こす関数は、本体（core）のあちこちから呼ばれていた。そのまま別の箱へ移すと呼び手が壊れるので、本体には「何を起こすかの記述」と「起こす人を差し替える口」だけを置き、実際に起こすのは箱（境界 crate）の 1 か所にする。本体の行数はほとんど減らないが、本体が process を直接起こさなくなり、test が偽物を差して測れるようになる。

- 出所: `s2-07l.198` の段 2（(c)〜(h)）と段 3（(i)）を契約表の行へ詰める周。§1〜§3 の決定（core-lines の母集団・境界 crate・R-C4-5）は不変で、本節は段 2 / 段 3 の**形**だけを上書きする。
- **母集団（実測・main b82be74・`cargo xtask check` の core-spawn=43/25 と一致）**: core の src で `Command::new` を含む行は 43 行 / 25 file。うち src の本体（最初の行頭 cfg(test) より前）が **33 行 / 21 file**、歯の区間が 10 行 / 8 file（git の fixture を作る helper 4・死んだ pid を作る true の起動 4・包みの歯が渡す引数 2）。歯の区間だけに持つ file は 4 本（`pipe/follow.rs`・`pipe/follow_step.rs`・`pipe/admission.rs`・`fleet/store.rs`）。本体 33 行の道具別の内訳は git 10・systemctl 4・tmux 3・sh 3・systemd-run 2・kill 2・bd 2・自分自身 2・claude 1・curl 1・hostname 1・cargo 1・CI の照会 1。
- **決定的な事実（呼び手の数・grep で実測）**: 撃つ関数は core の中から呼ばれている。`pipe/mod.rs` の git の 3 関数は core の 16 file から、`seat/mod.rs` の tmux の 2 関数は 6 file から呼ばれ、`headless/mod.rs` の構築点は 3 file へ、`pipe/confine.rs` の sh の行の包みは 4 file へ Command を値で返し、構築点は包みへ Command を渡す。core は境界 crate に依存できない（依存は一方向・§3）ので、**撃つ関数だけを境界 crate へ純移動すると呼び手が compile できない**。呼び手ごと移すと land / gate / train / follow 等の driver（判定と撃つ面が交互に並ぶ）が丸ごと動き、判定を境界へ押し出す（R-C4-5 が塞ぐ抜け穴そのもの）。＝§6 の「純移動 6 便」は成立せず、pipeline.md §5.3 の純移動の機械証明も使えない（core に残る関数の本文が変わる）。
- **採る形（推奨・user 裁定と ADR の land が前提）＝起動の記述と差し替え口**:
  1. core に**起動の記述**（型 1 つ）を置く。std の Command と同じ builder の面（new・arg・args・current_dir・env・env_remove・stdin・stdout・stderr・process_group・get_program・get_args・get_envs＝core の現 site が呼ぶ面の全部・実測）と同じ 4 終端（output・status・spawn・exec）を、同じ名・同じ受け手の形で持つ。終端は差し替え口へ渡すだけで、解釈しない。**site の書き換えは構築の字面と use の行だけ**で、呼び方の形・判定・呼び手の signature は 1 字も変わらない（型の位置で Command を名指す `pipe/confine.rs` の包みと `headless/mod.rs` の構築点の戻り値は、型の字面を置き換える）。
  2. core に**差し替え口**（trait 1 つ・method は 4 終端と 1:1）と、process に 1 回だけ据える関数を置く（2 回目は据えない）。据えていない周の終端は io の Unsupported を返す＝各 site の既存の「撃てない」分岐に落ちる（fail-closed・新しい分岐を足さない）。
  3. 境界 crate に**実物**（差し替え口の実装 1 つ・起動の記述を std の Command へ写して撃つ）を置き、bin の main の先頭で据える。本行群の後、workspace の本番の src で `Command::new` を持つのは境界 crate のこの 1 file だけになる。
  4. **core の歯は据えずに撃てる**: core の cfg(test) の build だけ、据えていない周の代わりに歯の区間の実物を使う（置き場は `pipe/mod.rs` の歯の区間の既存の fixture module・同じ型の先例）。git の fixture で実 repo を作る既存の歯はそのまま動く。新しい歯は記録する stub（撃たれた program と引数を覚え、決めた結果を返す）を据えて「その site が差し替え口を通る」ことを測る。base の site は std の Command を直に撃つので stub に記録が残らず RED（字面の pin ではなく挙動の歯）。nextest は歯ごとに process を分けるので、据えるのは歯ごとに 1 回で衝突しない。
  5. **置き場の罠（実測の規則）**: 起動の記述の file は歯の区間を持たない（新規 file の歯の区間は flip-check の not-flippable）。cfg(test) の側の実物を引く use は file の末尾に「cfg(test) だけの行 + use の行」の 2 行で置く（次の非空行が mod でないので flip-check の歯の区間の始点にならず、xtask の区間の切れ目は file の末尾の 2 行だけを歯に数える）。cfg(not(test)) の側の 1 関数は本体に置く（区間の切れ目は行頭の cfg(test) の字面なので当たらない）。
- **効果と代償**: core の本体の行数はほぼ減らない（起動の記述と差し替え口で +150 行前後・site の置換は差 0・境界 crate は +110 行前後）。§1 の「≈2800 行が core の外へ」は撃つ関数を file ごと移す見積で、上の事実で成立しない。core の余地は段 1 (a) が既に戻した（実測 core-lines=43595/60000）。段 2 が作るのは (1) core の本体の `Command::new` が 0 (2) 起動の口が境界 crate の 1 か所 (3) core の歯が stub で撃てる（`s2-07l.198` notes の head_of の空 sha の生存変異を行 h の歯が殺す）の 3 つである。
- **却下（ADR に残す）**: (A) 撃つ関数の純移動＝呼び手が compile できない。(B) driver ごと境界 crate へ＝判定と撃つ面の切り分けが散文の判断になり（ADR-0033 の DR3 に反する）、境界の上限が数千行に膨らみ、便が L 級になる。(C) 道具ごとの typed な口（git / tmux / systemctl…）＝閉じた集合で強いが、子を流しながら読む面（`headless/lens.rs`・`pipe/spawn.rs`・`seat/ledger.rs` が Child を持つ）は結局 spawn の口が要り、口の定義 file を全行が触るので並列に流せない（本節の後続の候補）。(D) 段 2 をやめ core-spawn を検出線のまま残す＝user に問う選択肢。
- **行の切り方（write-set が互いに交わらない・呼び手は signature が不変なので触らない）**:

| 行 | 面 | 本体の site | 歯の区間の site | file |
|---|---|---|---|---|
| c | 口の新設 + `pipe/mod.rs` の git の 3 関数 | 3 | 2 | pipe/mod.rs + 新設 2（core / 境界）+ lib 2 + main |
| d | pipe の driver（dispatch・stop・gate・land の finish・follow・follow_step・admission） | 4 | 3 | 7 |
| e | 包みと claude の構築点（confine・headless・その Command に process_group を呼ぶ 2 file・claude-spawn-points） | 9 | 2 | 5 |
| f | 席と tmux と台帳の読み（seat の mod・launch・ledger・recent・account の mod・ledger の mod） | 7 | 0 | 6 |
| g | fleet（usage の read・cli・wait・store） | 3 | 1 | 4 |
| h | hook と導入先（vessel・host_guard・group・consumers） | 7 | 2 | 4 |
| 計 | | 33 | 10 | 25 file（site を持つ file） |

- **構造の連鎖（実測・行の write-set に入れた理由）**: (1) `headless/mod.rs` の構築点と `pipe/confine.rs` の包みは Command を引数と戻り値で受け渡す＝同じ行 e。(2) その戻り値に process_group を呼ぶ `pipe/spawn.rs` と `fleet/usage.rs` は std の CommandExt の use が不要になり unused_imports で clippy が落ちる＝行 e に同梱（`fleet/usage.rs` の kill の site も行 e）。(3) claude-spawn-points（`crates/xtask/src/spawn_points.rs`・deny）は headless/ の構築の字面が mod.rs に 1 つであることを求め、0 は fail-closed の違反＝構築の字面が変わる行 e で、数える字面に起動の記述の構築を足す（2 つの字面の合計が headless/ で 1・その 1 が mod.rs）。(4) exec と process_group のために CommandExt を use する `pipe/dispatch.rs`・`hook/group.rs`・`seat/cycle/launch.rs` は各行が同じ file の use を外す。(5) e2e が lib を直に呼んで起動に届く経路は host 名の読みの fallback（/etc/hostname が読めない host だけ hostname を撃つ）の 1 つで、据えていない e2e の process ではその周だけ unknown に倒れる（bin は据えるので値が割れて e2e が声を上げて落ちる・CI の host は /etc/hostname を持つ）。(6) pipe の driver・headless の runner / lens・gate の verify / lens・review は起動の記述を値で受けて同じ名の method を呼ぶだけなので 1 字も触らない。
- **歯の置き場**: 各行の新しい歯は write-set の file の歯の区間に置き、名は行ごとの接頭辞（invocation_ に続けて pipe_git_ / pipe_driver_ / wrap_ / seat_ / fleet_ / hook_）で、互いに部分文字列にならない（現物に invocation を含む歯の名は 0・実測）。歯の区間を持たない file（`fleet/cli.rs`・`fleet/usage/read.rs`・`hook/vessel.rs`・`pipe/land/finish.rs`・`seat/ledger.rs`・`seat/recent.rs`）は、撃つ関数が私有か pub(super) なので同じ file の末尾に歯の区間を足す（既存 file なので flip-check が写せる）。host 名の読みは /etc/hostname を先に読むので stub が届かず RED の歯を作れない＝行 g の RED は同じ行の curl と CI の照会の歯が持ち、host の site は done の「その file に std の Command が残らない」で測る。
- **行 i（段 3）**: core-spawn の母集団を core の src の**本体**に（core-lines と同じ切り方・§2 の裁定と同じ理由＝歯の区間は R-C4-3 が縛る）し、1 以上を deny にする（fact の書式は不変）。歯の区間には cfg(test) の実物と fixture の起動が残る。ADR-0033 の「core は Command::new を 0 本しか持たない」を「core の本体は」と読む解釈は、行 c の前に land する ADR に書く。R-C4-5（kind BoundaryLines・Int・deny）を manifest に足す。値は行 i の便の base で境界 crate の src の本体を測った値 × 1.2 の切り上げで、**仮置き 341**（今の本体 174 + 実物の口の見込み 110 = 284 の 1.2 倍の切り上げ）。裁定 id は `user 2026-09-15T10:07Z`（台帳 `s2-07l.198` notes の字面「裁定 id = user 2026-09-15T10:07Z」・§2 / §3 が書く 09:5xZ は同じ日の前の是認で、境界 crate の上限を決めた AskUserQuestion の裁定はこちら）で、manifest の他の行の ruling と重ならない（rules-diff の相乗りに当たらない・実測）。rules 行を足す面は manifest・`crates/scribe2/src/rules/mod.rs`（variant・ALL・名・値の形）・`crates/scribe2-boundary/tests/e2e/rules.rs`（kind の match の面）・`crates/xtask/src/limits.rs`（値を読む・値の個数の歯）・`crates/xtask/src/check_sizes.rs` と `crates/xtask/src/check.rs`（measure 2 本と列）・`crates/xtask/src/check_tests.rs`（SUMMARY_PIN の core-spawn の直後に boundary-lines）・[rules-manifest.md](./rules-manifest.md) の表。**憲法 §3 の cell は design-intent/spec の編集で folio-architect（user が起動する）の別の周**（rules-parity は検出線なので manifest だけの周も門は赤にならない）。
- **前提（行 c の前）**: (1) user 裁定 1 問（採る形 = 推奨 / 却下 (B) / 却下 (D) のどれか・A2 ではなく目的と価値の選択）(2) 新しい ADR（ADR-0033 の §4 の「撃つ関数を移す」を「起動の記述と差し替え口」で実現する部分の supersede・core-spawn の母集団を本体に読む解釈・却下 4 案・同じ PR で vocabulary〔起動の記述 / 差し替え口〕と decisions/README）。行 c〜i は ADR の land の後に起票する。
- 触らない: 判定の関数・呼び手の signature・pipe の driver の本文・歯の本文（歯の区間の site の置換を除く）・§1〜§3 の決定・R-C4-1 の値。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "core-lines の母集団を src の本体に（in-file の歯を外す）+ core-spawn の検出線 — 受付の core の余地も同じ式に"
req = ["NFR3"]
section = "2"
write-set = ["crates/xtask/src/check_sizes.rs", "crates/xtask/src/check.rs", "crates/xtask/src/check_tests.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/declaration/write_set.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail sizes_core_", "cargo nextest run -p scribe2 --no-tests=fail pipe_intake_core_headroom_"]
size = "S"
done = "core-lines が in-file の歯を除いた src 側の合計になり、core-spawn の検出線が fact 行に出て、受付の core の余地が同じ式で数えられる（両側の式の一致を同じ fixture の歯が守る）"

[[contract]]
id = "b"
title = "境界 crate の新設 — lib + bin（main.rs の移動）+ tests/ の純移動 + xtask Layout の boundary_dir + 契約表の行の path 置換"
req = ["NFR3"]
section = "3"
write-set = ["Cargo.toml", "Cargo.lock", "+crates/scribe2-boundary/Cargo.toml", "+crates/scribe2-boundary/src/lib.rs", "+crates/scribe2-boundary/src/main.rs", "+crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "~crates/scribe2/src/main.rs", "~crates/scribe2/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2/Cargo.toml", "~crates/scribe2/tests/e2e/fleet.rs", "~crates/scribe2/tests/e2e/headless.rs", "~crates/scribe2/tests/e2e/hook.rs", "~crates/scribe2/tests/e2e/main.rs", "~crates/scribe2/tests/e2e/pipe.rs", "~crates/scribe2/tests/e2e/pipe/dispatch.rs", "~crates/scribe2/tests/e2e/pipe/gate.rs", "~crates/scribe2/tests/e2e/pipe/intake.rs", "~crates/scribe2/tests/e2e/pipe/land.rs", "~crates/scribe2/tests/e2e/pipe/launch_failure.rs", "~crates/scribe2/tests/e2e/pipe/ratelimit.rs", "~crates/scribe2/tests/e2e/pipe/spawn.rs", "~crates/scribe2/tests/e2e/pipe/stop.rs", "~crates/scribe2/tests/e2e/polarity.rs", "~crates/scribe2/tests/e2e/prop.rs", "~crates/scribe2/tests/e2e/rules.rs", "~crates/scribe2/tests/e2e/seat.rs", "~crates/scribe2/tests/e2e/seat/account.rs", "~crates/scribe2/tests/e2e/seat/launch.rs", "~crates/scribe2/tests/e2e/seat/register.rs", "~crates/scribe2/tests/e2e/seat/rules.rs", "~crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__hook__hook_brief_orchestrator.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__hook__vessel_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__pipe__gate__pipe_gate_move_summary_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__pipe__gate__pipe_record_show_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "~crates/scribe2/tests/e2e/ledger.rs", "~crates/scribe2/tests/e2e/ledger_form.rs", "~crates/scribe2/tests/e2e/ledger_memo.rs", "~crates/scribe2/tests/e2e/notify.rs", "~crates/scribe2/tests/e2e/pipe/contracts.rs", "~crates/scribe2/tests/e2e/pipe/refuse.rs", "~crates/scribe2/tests/e2e/pipe/review.rs", "~crates/scribe2/tests/e2e/seat/ruling.rs", "~crates/scribe2/tests/e2e/snapshots/e2e__headless__lens_promise_prompt_external_form.snap", "~crates/scribe2/tests/e2e/snapshots/e2e__ledger_memo__ledger_memo_plan_usage_external_form.snap", "~crates/scribe2/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "~crates/scribe2/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "+crates/scribe2-boundary/tests/e2e/fleet.rs", "+crates/scribe2-boundary/tests/e2e/headless.rs", "+crates/scribe2-boundary/tests/e2e/hook.rs", "+crates/scribe2-boundary/tests/e2e/main.rs", "+crates/scribe2-boundary/tests/e2e/pipe.rs", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "+crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "+crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "+crates/scribe2-boundary/tests/e2e/pipe/land.rs", "+crates/scribe2-boundary/tests/e2e/pipe/launch_failure.rs", "+crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "+crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "+crates/scribe2-boundary/tests/e2e/pipe/stop.rs", "+crates/scribe2-boundary/tests/e2e/polarity.rs", "+crates/scribe2-boundary/tests/e2e/prop.rs", "+crates/scribe2-boundary/tests/e2e/rules.rs", "+crates/scribe2-boundary/tests/e2e/seat.rs", "+crates/scribe2-boundary/tests/e2e/seat/account.rs", "+crates/scribe2-boundary/tests/e2e/seat/launch.rs", "+crates/scribe2-boundary/tests/e2e/seat/register.rs", "+crates/scribe2-boundary/tests/e2e/seat/rules.rs", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_contract_prompt_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__hook__hook_brief_orchestrator.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__hook__vessel_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__gate__pipe_gate_move_summary_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__gate__pipe_record_show_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "+crates/scribe2-boundary/tests/e2e/ledger.rs", "+crates/scribe2-boundary/tests/e2e/ledger_form.rs", "+crates/scribe2-boundary/tests/e2e/ledger_memo.rs", "+crates/scribe2-boundary/tests/e2e/notify.rs", "+crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "+crates/scribe2-boundary/tests/e2e/pipe/refuse.rs", "+crates/scribe2-boundary/tests/e2e/pipe/review.rs", "+crates/scribe2-boundary/tests/e2e/seat/ruling.rs", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_promise_prompt_external_form.snap", "+crates/scribe2-boundary/tests/e2e/snapshots/e2e__ledger_memo__ledger_memo_plan_usage_external_form.snap", "+crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "+crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "crates/xtask/src/workspace.rs", "crates/xtask/src/mutantsdiff.rs", "crates/xtask/src/check_sizes.rs", "crates/xtask/src/polarity.rs", "crates/xtask/src/check_facts.rs", "crates/xtask/src/env_reads.rs", ".config/nextest.toml", "docs/design/account-autonomy.md", "docs/design/account-lifecycle.md", "docs/design/consumer-sync.md", "docs/design/contract-source.md", "docs/design/core-boundary.md", "docs/design/dialogue-surface.md", "docs/design/dispatcher.md", "docs/design/fleet-event-log.md", "docs/design/fleet-usage.md", "docs/design/gate-cost.md", "docs/design/pipeline-conflict.md", "docs/design/pipeline.md", "docs/design/rules-manifest.md", "docs/design/seat-roles.md", "docs/design/ledger-form.md", "docs/design/pipeline-question.md", "docs/design/vessel-hook.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail layout_finds_the_boundary_crate", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail"]
size = "M"
done = "境界 crate が bin と e2e の歯（歯の file 29 + 外形 snapshot 19（e2e 16・src 3）= 48 項目の純移動（xtask の polarity と check_facts の e2e の読み先は boundary_dir へ切り替わり cargo xtask check が緑）（write-set の ~ は bin 本体の main.rs を加えて 49 本））を持ち、移動した歯が移動先で全部緑で本数が base と同じ（母集団は notes）、core 側に tests/ と main.rs と doctor の snapshot が 1 つも残らず、xtask の Layout が境界 crate の dir を返し、17 の設計 doc（write-set の docs/design/ の列の本数）の契約表の行の path 置換の後に contracts check が findings 0・insta の unreferenced が 0（入口の flip-check は pipeline.md 行 av の着地後の世代で撃ち、test 区間に差が出る移動 file は moved の札で免除する）（gate の lens 入力は gate-cost.md 行 ah の畳みで docs の path 置換の hunk を 1 行の印にし、行 ai の段ごとの対と空になった dir の対で隣り合う契約行の同時書き換えと dir の名指しも畳み、code 側の diff だけが lens へ渡って cap に収まる＝行 ai の着地後の世代の器で gate を撃つ）"

[[contract]]
id = "c"
title = "起動の記述と差し替え口 — core に起動の記述（std の Command と同じ builder の面と 4 終端）と差し替え口を置き、境界 crate の実物を bin の main の先頭で据え、pipe/mod.rs の git の 3 関数と歯の区間の true の起動 2 本を置換する（段 2 の 1 便目・呼び手と signature は不変）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
write-set = ["+crates/scribe2/src/invocation.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/pipe/mod.rs", "+crates/scribe2-boundary/src/spawner.rs", "crates/scribe2-boundary/src/lib.rs", "crates/scribe2-boundary/src/main.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail invocation_pipe_git_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail"]
size = "M"
done = "(1) core に起動の記述と差し替え口が在り、起動の記述は core の現 site が呼ぶ builder の面（new・arg・args・current_dir・env・env_remove・stdin・stdout・stderr・process_group・get_program・get_args・get_envs）と 4 終端（output・status・spawn・exec）を std の Command と同じ名と受け手の形で持ち、据えていない周の終端は io の Unsupported を返す (2) 差し替え口は process に 1 回だけ据わり、境界 crate の実物が bin の main の先頭で据えられ、境界 crate の src の本体で std の Command を構築するのは + の境界の file の 1 か所だけ (3) pipe/mod.rs の git の 3 関数と歯の区間の true の起動 2 本が起動の記述を通り、3 関数の signature と呼び手は 1 字も変わらず、pipe/mod.rs に Command::new が 0 (4) pipe/mod.rs の歯の区間の既存の fixture module に cfg(test) の実物と記録する stub が在り、起動の記述の file は歯の区間を持たず、その cfg(test) の use は file の末尾の 2 行（cfg(test) だけの行と use の行） (5) 記録する stub を据えた歯が git の 3 関数の program と引数と結果の読み（rc 非 0 は None / false・空の stdout の 1 行読みは None）を測り、base で RED (6) 境界 crate の e2e が全部緑（bin が据え忘れると撃つ面が全部落ちる）で、core の in-file の歯が全部緑（実 repo を作る既存の歯は cfg(test) の実物で動く）"

[[contract]]
id = "d"
title = "pipe の driver の起動の置換 — dispatch / stop / gate / land の finish の本体 4 site と follow / follow_step / admission の歯の区間 3 site を起動の記述へ（呼び手と signature は不変・CommandExt の use を外す）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
write-set = ["crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/stop.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2/src/pipe/follow.rs", "crates/scribe2/src/pipe/follow_step.rs", "crates/scribe2/src/pipe/admission.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail invocation_pipe_driver_"]
depends = ["c"]
size = "S"
done = "(1) write-set の 7 file に Command::new と std の Command の use が 0（本体 4 site と歯の区間 3 site が起動の記述を通る）で、関数の signature・呼び手・判定は 1 字も変わらない (2) dispatch.rs の CommandExt の use が消え、clippy の unused_imports が 0 (3) 記録する stub を据えた歯が自分自身の起動（spawn の失敗で偽）・kill の引数・git patch-id の起動（spawn の失敗で None）・PR を開く sh の行の起動を測り、base で RED（land の finish は file の末尾に歯の区間を足す） (4) core の in-file の歯が全部緑"

[[contract]]
id = "e"
title = "包みと claude の構築点の起動の置換 — confine の systemctl / systemd-run / sh と headless の構築点を起動の記述へ、その戻り値に process_group を呼ぶ spawn.rs と fleet/usage.rs の CommandExt を外し、claude-spawn-points が起動の記述の構築を数える（呼び手は不変）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
write-set = ["crates/scribe2/src/pipe/confine.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/fleet/usage.rs", "crates/xtask/src/spawn_points.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail invocation_wrap_", "cargo nextest run -p xtask --no-tests=fail spawn_points_counts_the_invocation_constructor_as_the_build_point"]
depends = ["c"]
size = "S"
done = "(1) confine.rs・headless/mod.rs・fleet/usage.rs に Command::new と std の Command の字面が 0（confine の本体 7 site と歯の区間 2 site・headless の構築点 1 site・fleet/usage の kill 1 site）で、包みの関数と構築点は型の字面だけが起動の記述に替わり、argv・env の外し方・scope の引数は 1 字も変わらない (2) spawn.rs と fleet/usage.rs の CommandExt の use が消え（fleet/usage.rs の ExitStatusExt は残る）、gate の verify / lens・review・headless の runner / lens は 1 字も変わらず compile する (3) claude-spawn-points が headless/ の構築の字面を std の Command と起動の記述の 2 つで数え、合計がちょうど 1 でそれが mod.rs に在ることを求め、flag の字面の検査は不変で、cargo xtask check が緑 (4) 記録する stub を据えた歯が systemctl の kill と reset-failed の 2 起動・構築点の program と flag・kill の引数を測り base で RED、xtask の歯は起動の記述の構築 1 つの fixture を健全と読み base で RED (5) core の in-file の歯が全部緑"

[[contract]]
id = "f"
title = "席と tmux と台帳の読みの起動の置換 — seat の tmux 2 関数・席の起動の exec・台帳の bd 2 site・recent の git・口座の pane の読みを起動の記述へ（呼び手は不変・launch の CommandExt を外す）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
write-set = ["crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/seat/recent.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/ledger/mod.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail invocation_seat_"]
depends = ["c"]
size = "S"
done = "(1) write-set の 6 file に Command::new と std の Command の use が 0（本体 7 site）で、関数の signature・呼び手・判定は 1 字も変わらない (2) launch.rs の CommandExt の use が消え、exec は起動の記述の終端を通る (3) 記録する stub を据えた歯が tmux の socket の引数の付け方（-S の有無）・pane の読みの引数・exec の失敗の理由・bd の起動（spawn の失敗の型）・recent の git の引数を測り base で RED（seat/ledger.rs と seat/recent.rs は file の末尾に歯の区間を足す） (4) core の in-file の歯が全部緑（tmux の実 socket を撃つ既存の歯は cfg(test) の実物で動く）"

[[contract]]
id = "g"
title = "fleet の起動の置換 — usage の curl・host 名の読みの hostname・CI の照会・store の歯の true の起動を起動の記述へ（呼び手は不変）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
write-set = ["crates/scribe2/src/fleet/usage/read.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/fleet/store.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail invocation_fleet_"]
depends = ["c"]
size = "S"
done = "(1) write-set の 4 file に Command::new と std の Command の use が 0（本体 3 site と歯の区間 1 site）で、関数の signature・呼び手・判定は 1 字も変わらず、host 名の読みは /etc/hostname を先に読む順を保つ (2) 記録する stub を据えた歯が curl の起動（stdin と stdout の形・rc 非 0 の読み）と CI の照会の program と cwd を測り base で RED（fleet/usage/read.rs は file の末尾に歯の区間を足す） (3) core の in-file の歯と境界 crate の e2e が全部緑"

[[contract]]
id = "h"
title = "hook と導入先の起動の置換 — vessel の git 3 関数と cargo・host_guard の git ls-files・group の自分自身の起動・consumers の HEAD の読みを起動の記述へ、HEAD の読みの空 sha の歯を足す（呼び手は不変・group の CommandExt を外す）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
write-set = ["crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/hook/host_guard.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/account/consumers.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail invocation_hook_"]
depends = ["c"]
size = "S"
done = "(1) write-set の 4 file に Command::new と std の Command の use が 0（本体 7 site と歯の区間 2 site）で、関数の signature・呼び手・判定は 1 字も変わらない (2) group.rs の CommandExt の use が消える (3) 記録する stub を据えた歯が vessel の git と cargo の引数・git ls-files の -z の読み・自分自身の起動（spawn の失敗で偽）を測り base で RED（hook/vessel.rs は file の末尾に歯の区間を足す） (4) HEAD の読みの歯が rc 0 で空の stdout を返す stub で Unknown を返すことを測る（台帳 s2-07l.198 notes の生存変異 1 本を殺す） (5) core の in-file の歯が全部緑"

[[contract]]
id = "i"
title = "core-spawn を deny に・R-C4-5 を足す — core-spawn の母集団を core の src の本体にして 1 以上を deny、境界 crate の src の本体を数える boundary-lines と manifest の R-C4-5（BoundaryLines・値は本行の base の実測 × 1.2 の切り上げ・仮置き 341・裁定 user 2026-09-15T10:07Z）"
req = ["FR48", "FR47", "NFR2"]
section = "9"
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/xtask/src/limits.rs", "crates/xtask/src/check_sizes.rs", "crates/xtask/src/check.rs", "crates/xtask/src/check_tests.rs", "docs/design/rules-manifest.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail sizes_core_spawn_denies_src_body_sites sizes_boundary_lines_over_the_limit_is_denied limits_read_carries_the_boundary_lines_row"]
depends = ["d", "e", "f", "g", "h"]
size = "S"
done = "(1) core-spawn が core の src の本体だけを数え（歯の区間の起動は数えない＝core-lines と同じ切り方）、1 以上で cargo xtask check が rc 1 になり、fact の書式は不変で、本行の base で本体の件数が 0 (2) boundary-lines が境界 crate の src の本体を数え、R-C4-5 を超えると deny、R-C4-5 が無い周は measure を出さず、check_tests.rs の SUMMARY_PIN が core-spawn の直後に boundary-lines を持つ (3) manifest に R-C4-5（kind BoundaryLines・Int・発効・値は本行の base で測った本体 × 1.2 の切り上げ・ruling は user 2026-09-15T10:07Z）が在り、cargo xtask rules-diff の相乗りに当たらず、rules/mod.rs の variant・ALL・名・値の形と e2e の kind の match が揃う (4) limits が R-C4-5 を読み、値の個数の歯が 1 つ増えた数を測る (5) rules-manifest.md の R-C4 の表に行が在る（憲法 §3 の cell は folio-architect の別の周・rules-parity は検出線） (6) 歯 3 本（本体の起動を deny し歯の区間を数えない fixture・上限超えの fixture・現物の manifest の R-C4-5）が base で RED"
<!-- contracts:end -->
