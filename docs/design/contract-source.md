# 設計: 契約の正本と席の作業の境界 — 契約は設計 doc の契約表 1 行、器が契約 file を生成し、審査の段を通してから実装役を起こし、着地の終端（push・CI・台帳の close）まで器が持つ

- 要件: [FR47](../../design-intent/spec/srs.html#FR47) 契約の正本と生成 / [FR48](../../design-intent/spec/srs.html#FR48) write-set の閉包 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査の段 / [FR50](../../design-intent/spec/srs.html#FR50) land の終端 / [FR51](../../design-intent/spec/srs.html#FR51) 台帳の lint / [AC21](../../design-intent/spec/srs.html#AC21) [AC22](../../design-intent/spec/srs.html#AC22) [AC23](../../design-intent/spec/srs.html#AC23) [AC24](../../design-intent/spec/srs.html#AC24)・既存 [FR2](../../design-intent/spec/srs.html#FR2) / [FR4](../../design-intent/spec/srs.html#FR4) / [FR9](../../design-intent/spec/srs.html#FR9) / [FR12](../../design-intent/spec/srs.html#FR12) / [FR31](../../design-intent/spec/srs.html#FR31) / [FR32](../../design-intent/spec/srs.html#FR32) / [FR39](../../design-intent/spec/srs.html#FR39) / [NFR1](../../design-intent/spec/srs.html#NFR1) / [NFR4](../../design-intent/spec/srs.html#NFR4)
- 決定: [ADR-0023](../../design-intent/decisions/ADR-0023-contract-source-is-the-design-document-table.html)（本 doc の決定の正本・§2.1〜§2.7）/ [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.3（契約 file = TOML subset・本 doc では生成物）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.2（閉じた enum の充足形・字面走査）/ [ADR-0019](../../design-intent/decisions/ADR-0019-parallel-runs-exclude-at-intake-and-runner-resolves-conflicts.html) §2.1（intake の排他）/ [ADR-0021](../../design-intent/decisions/ADR-0021-gate-cost-is-bounded-by-admission-and-containment.html) §2.6（record の任意 field）
- 土台: [pipeline.md](./pipeline.md) §3（契約 file）/ §5.1（intake）/ §5.4（land）・[pipeline-question.md](./pipeline-question.md)（質問と回答）・[pipeline-conflict.md](./pipeline-conflict.md) §3（起こし直し）・[gate-cost.md](./gate-cost.md) §6（着地の順序）・[rules-manifest.md](./rules-manifest.md)（TOML subset の parser）・[fleet-event-log.md](./fleet-event-log.md)・[seat-roles.md](./seat-roles.md)（権能・本 doc はその外側）
- 位置づけ: 席の産物（契約）と手順（写し・回答の適用・着地の終端・台帳）のうち**器が持つ範囲**を決める。権能（seat-roles.md）は「誰がしてよいか」、本 doc は「器が代わりに持つもの」。裁定 = user 2026-09-13（Q1〜Q3・台帳 s2-07l.197 notes に逐語）。

## 1. 何を解くか

便の一周（要望 → 要件 → 設計 → 契約 → 受付 → 実装 → gate → land → 終端 → 台帳）のうち、器が構造で持つのは受付の排他（FR39）・実装役の write-set guard（FR20）・gate（FR8 / FR9）・land の CAS と順序（FR10〜12・gate-cost.md §6）だけで、残りは席の判断・散文・手写しである（2026-09-13 の実測: 15 手順中 5 つ）。特に契約は **3 段の写し**（設計 doc の契約の割り〔散文〕→ 台帳の acceptance〔散文〕→ 契約 file〔管理席の手写し〕）で、各段で write-set の落ち（5 例）と設計の inline（gate で初めて見つかる穴 2 つ）が起きた。台帳には設計本文を持つ memo が 40 本溜まる（設計 doc に置き場が無い）。

本設計は (1) 契約の正本を設計 doc の**契約表**（機械が読む 1 区間）に置き、器が契約 file を**生成**する (2) 契約表を CI と受付が**検査**する（閉包・実在・形）(3) 実装役を起こす前に**審査の段**を置く (4) land の**終端**（push・CI の照合・台帳の close）を器が持つ (5) 台帳を doctor の項目で lint する。planner の産物に門が付き、管理席の手写しが消える。

## 2. 契約表（FR47）

- **置き場**: 設計 doc（`docs/design/<題>.md`）の末尾に機械が読む区間 `<!-- contracts:begin -->` … `<!-- contracts:end -->`（CLAUDE.md の憲法区間と同じ marker 形・読み手は xtask claude_md と同型の行走査）。区間の本文は **TOML subset**（rules manifest と同じ parser・`[[contract]]` の表・ADR-0004 §2.3）。設計 doc 1 本に区間は 0 か 1 つ。
- **行の field**（現物の契約 file の `REQUIRED` と 1:1 + 宣言 2 つ）: `id`（doc 内で一意・`a` `b` …）/ `title` / `req`（要件 id の列）/ `section`（本 doc の節 anchor・生成時に節の本文を `goal` へ写す＝説明文を二重に書かない）/ `touches`（閉じた型の宣言の列・`crate::module::Type` の形・空可・§3）/ `write-set`（path の列）/ `verify`（positional filter 形の列・`(` を含まない）/ `size` / `done`（1 行）/ `depends`（同 doc の契約 id の列・順序）/ `classes`・`opens`（optional・既存の形）。`owner` / `disposition` は生成時に固定値（現物の contract.rs が要求する field を埋める）。
- **台帳の bead**: title・status・裁定（notes）・acceptance は `design = docs/design/<題>.md#<id>` の **1 行だけ**。契約の改訂 = 設計 doc の改訂（PR・folio と CI の門を通る）。台帳の acceptance に本文を書く形は §9 (a) の land 後に止める（FR51 の lint が名指す）。
- **生成**: `<NAME> pipe intake --design docs/design/<題>.md#<id> --bead <bead id> --repo R [--rules PATH]`。器は base（`--repo` の HEAD）の設計 doc から区間を読み、行 1 つを契約 file（run dir の `contract.toml`・field は現物の REQUIRED + `design` = pointer + `touches`）へ写す。**`--contract PATH` は廃止**（手書きの契約 file を受け付けない・FR47）。歯の toy repo は設計 doc の fixture を持つ。
- **表の検査**（`<NAME> contracts check --repo R`・CI の 1 job・xtask check は core に依存しないので撃たない）: 全 tracked 設計 doc の区間を parse し、id の一意・`req` の id が SRS（`design-intent/spec/srs.html` の anchor）に実在・`section` が同 doc に実在し本文が非空・`verify` の形・`depends` の解決・`touches` の閉包 ⊆ `write-set`（§3）を全件・行番号付きで出す（FR18 と同じ「全件・黙って落とさない」）。intake は同じ関数を 1 行に対して撃つ（1 実装・C2）。

## 3. write-set の閉包（FR48）

- **入力**: `touches` の各型（`crate::fleet::Stage` の形）と base の tree。
- **導出**（字面走査・`pipe/closure.rs`・pure 関数・I/O は呼び手）: core と歯の `.rs` を全部読み、型ごとに次を持つ file を集める。(i) **literal 構築** = `Type {`（struct・`pub struct` の宣言行は除く）(ii) **match の arm** = `Type::` を `=>` の左に持つ行（網羅 match の面・`as_str` / `parse` / 段の分岐）(iii) **件数 pin** = その型の const slice（enum-slices の対応: `pub const NAME: &[Type]`）の `NAME.len()` を持つ行 (iv) const slice の宣言 file。読めない file・区間の parse 失敗は違反（fail-closed・NFR4）。
- **判定**: 閉包 ⊄ write-set なら `Refuse::WriteSetIncomplete { run, missing: Vec<path> }`（typed・足りない file を全部名指す・FR39 の `WriteSetOverlap` の隣）。
- **限界（残す側）**: 字面走査は「型の名が別名で現れる形（`use … as`・generic の中）」を見ない＝閉包の**下界**。上界を求めるには構文木が要り A3 の依存になる（却下・§11）。見落とした構築点は実装役の質問（FR31）で出て planner が設計 doc を直す（§7）＝運用ではなく質問 record と PR に落ちる。
- CI（§2 の `contracts check`）と受付が同じ関数を撃つ。

## 4. 契約の審査の段（FR49）

- **段**: `Stage::Reviewed`（宣言順は `Intake` の直後・`as_str` = `Reviewed`）。`pipe intake` の直後に器が lens を 1 回撃ち、verdict を run dir の `review.json`（gate の `verdict.json` と同型・tmp → rename の atomic 書き）と `RunStage stage=Reviewed detail=verdict:<PASS|FAIL|INCONCLUSIVE>` に残す。
- **lens の口**: 既存の `<NAME> lens` に雛形を 1 枚足す（`headless/lens-contract.txt`・穴 = `{contract}`〔生成した契約 file〕/ `{design}`〔`section` の本文〕/ `{requirements}`〔`req` の要件本文・SRS から抜く〕・diff は無い）。観点は 3 つ（契約と設計の節の適合・設計が名指す状態遷移の一周〔段・完了 enum・列の所属〕・write-set の連鎖〔§3 の閉包に無い構造の落ち〕）。verdict は既存の 3 値。予算は NFR1 の cap をそのまま使う（契約 + 節 + 要件で cap を超えたら INCONCLUSIVE＝FR9 の極性）。
- **効き方**: `pipe run` / `pipe resume` は `Reviewed` かつ verdict PASS の run だけを spawn する（現物の `launch(.., &[Stage::Intake])` の入口を `Reviewed` に改める）。FAIL / INCONCLUSIVE は終端（`live` は false・retire 可）。直しは設計 doc の改訂 → PR → 再 intake（run 2）。
- **人の関与 0**: 審査を人が飛ばす口は無い（`--no-review` を作らない・C16）。

## 5. land の終端（FR50）

Landed（gate-cost.md §6 の CAS の後）に続けて器が行う。各段は typed な event（`RunDone` の `detail` で弁別・schema 1 のまま）。

1. **push**: `git push <remote> main:main`（子 process・remote 名は `.vessel.toml` の宣言 `remote`〔optional・無ければ `origin`〕）。失敗は `RunDone detail=push:failed:<reason>` で止める（close しない・rc 1）。
2. **CI の照合**: 唯一の wait 実装に `Completion::CiResult { repo, sha }` を足し、forge の CLI（`gh run list --commit <sha> --json status,conclusion`・子 process・`.vessel.toml` の `ci-cmd`〔optional・無ければ既定の 1 行〕）を deadline（rules 行 `pipe.ci_wait_s`・Int・裁定 id）まで待つ。結果は 3 値（success / failure / unmeasurable）。**success 以外は close しない**（FailClosed）・記帳して rc 1。
3. **台帳の close**: 台帳 adapter（§6）で `close <bead> --reason "landed <sha> ci=success"`。adapter が撃てない・rc ≠ 0 なら `RunDone detail=close:failed` で止める（着地は成立している＝やり直しは `pipe land --terminal-only <run>` で終端だけ再実行・冪等）。
4. **binary の世代**: record（verdicts.jsonl の行）に `generation=<landed sha>` を足す。自分の版が landed sha より古い周に起動を断るかは後続（§12）。

`--pr-cmd` の形（自 repo への PR）は終端を持たない（従来どおり）。

## 6. 台帳 adapter（FR50 / FR51）

- **置き場**: 新 module `ledger/`（core）。読み = `bd --readonly show <id> --json` / `bd --readonly list --status open --limit 0 --json`（子 process・git / tmux と同型・crate 依存なし・出力は既存の `json_lite` で読む）。書き = `bd close <id> --reason <text>` の **1 種だけ**（起票・acceptance・裁定は席）。binary の名は const・path は PATH 解決（env を読まない・C2.2）。
- **lint**（`<NAME> doctor --state-dir S --repo R` の項目 1 行・C3.2）: open の bead を全件読み、(i) 契約（acceptance が `design =` で始まる bead）で pointer が解けない（doc が無い・区間に id が無い）(ii) memo（label `intake:memo`）で本文に機械が読む設計の見出し（固定の 1 つ・`## memo`）が在り `design =` / `research =` の pointer 行が無い、を名指す。件数と母集団を同じ行に出す（`ledger: open=N contracts=K unresolved=U memos=M unpointed=P`）。管理 tick の pointer 注入（FR43）は doctor のこの行を席へ渡す。
- **CI は撃たない**（private な台帳に届かない）。

## 7. 質問と契約の改訂（.133 の解消）

回答（FR32）は逐語のまま。**回答が write-set を広げる形は持たない**: 実装役が「write-set に file が足りない」を質問したら、planner は設計 doc の契約表を直す（PR）→ 再 intake（run 2・base は新 main）。写しは生成物なので手で直さない（管理席の手順 #9 が消える）。§3 の閉包が先に拾うので、この形の質問は「字面走査の下界の外」だけになる。

## 8. 極性（[polarity.md](./polarity.md)）

| guard | 段 | 極性 | 何を止めるか |
|---|---|---|---|
| `ContractTable` | in-loop（intake・`contracts check`） | FailClosed | 区間が無い・parse できない・id が無い・req / section が解けない・閉包 ⊄ write-set・verify の形が違う契約 |
| `Review` | in-loop（`Reviewed` の段） | FailClosed | verdict が PASS でない run の spawn |
| `LandTerminal` | in-loop（land の終端） | FailClosed | push 失敗・CI が success でない・adapter が撃てない周の close |

`ledger lint`（doctor の項目）は guard ではない（行為を止めうる判定を返さない・ADR-0014 §2.1）。

## 9. 歯（`crates/<NAME>/tests/e2e/` に `contract_` / `pipe_review_` / `pipe_terminal_` / `ledger_` 接頭辞・名前の列は現物が SSOT）

- 契約表: toy repo の設計 doc（区間 1 つ・3 行）から `intake --design` が正常の 1 行で run を作り contract.toml の field が REQUIRED 全部 + design + touches を持つ／閉包が足りない行は `WriteSetIncomplete` で足りない file を全部名指し run dir が増えない／section が無い行・req が SRS に無い行・区間の無い doc・`--contract` の形はいずれも typed に断る／`contracts check` が全 doc を全件・行番号付きで出す（AC21）。
- 閉包（in-file・pure）: literal 構築・match の arm・件数 pin・const slice の 4 形を固定 fixture で pin・読めない file は違反・別名の形は下界の外（歯で「拾わない」を pin し限界を残す）。
- 審査: 偽 lens が FAIL を返す run は `Reviewed(FAIL)` で止まり構築点の呼出 0・PASS は Spawned へ・`review.json` と event・`--no-review` の引数は usage で断る（AC22）。
- 終端: 偽 remote（bare repo）+ 偽 CI cmd + 偽 adapter（stub が argv を写す）で Landed の後に push → CI（success）→ close の 3 event・CI が failure の fixture は close されず rc 1・adapter が rc 1 の周は `close:failed` と `--terminal-only` の冪等（AC23）。`pipe.ci_wait_s` の欠落は `RuleError`。
- lint: 偽 adapter の出力（memo 2 本 pointer 無し・契約 1 本 pointer 不解決）で doctor の行が件数と母集団を出す・全件揃えば 0（AC24）。
- 極性一覧 snapshot に 3 行（件数 +3・N = K + M）。`STAGES` の宣言順（`Reviewed` は `Intake` の直後）。
- 実地（done の一部・歯にしない）: 本 doc 自身の契約表から (a) を intake し Landed → 終端 → close まで席の手順なしに通す（AC22 の D・AC23 の D）。

## 10. 憲法・制約との整合

C15 / C15.2（台帳は task と裁定・pointer の欠落は lint）・C16 / C16.2（受付・審査・終端で止める in-loop guard・極性一覧）・C2（`Refuse` / `Stage` / `Completion` に variant を足す・1 関数）・C2.2（env を読まない・remote / ci-cmd は宣言 file・binary の名は const）・C3 / C3.4（段は event log・待ちは唯一の wait 実装）・C5（`pipe.ci_wait_s` は裁定 id）・C10（record は typed・`unmeasurable` を success に読み替えない）・C11.2 / C11.3（極性の定数・Timeout は Result）・C12.5（区間の parse・doctor の行・review.json の外形 snapshot）・C12.7（閉包の property）・N1（終端の失敗は着地を取り消さない・retire は move）・A3（構文木 crate を採らない・forge CLI と bd は子 process）。

## 11. 却下案（ADR-0023 §5 の写しは持たない・設計固有のもの）

- 台帳の acceptance を typed にして正本のままにする。却下: 台帳は機械が読む形式を持たず（free text）、器が読むには adapter が要り、C15 に逆行する。
- 入口の検査だけ足して 3 段の写しを残す。却下: 写しの各段で drift する（今日 5 例）・設計の inline を止められない。
- 閉包を構文木（syn 等）で求める。却下: A3 の依存・NFR3（実行時依存 0）。字面の下界 + 質問 record で足りる。
- 契約の審査を人が回す（dispatch 前に planner が lens）。却下: N2（散文の規則）・C16（後段で代替しない）。
- CI が台帳 lint を撃つ。却下: CI は private な台帳に届かない。doctor + tick。
- 回答で write-set を広げる口（`pipe answer --write-set-add`）。却下: 写しは生成物・正本は設計 doc（§7）。

## 12. 後続

binary の世代で起動を断る（§5 4.）・契約表から台帳の bead を起票する口（台帳 write の 2 種目・A1 の「出す」に当たらないが scope の改訂が要る）・`Reviewed` の lens の観点を rules 行にする（.176）・memo 40 本の設計内容を各設計 doc の「未契約の機構」表へ移す（planner の一括作業・§6 の lint が残りを名指す）。

<!-- contracts:begin -->
[[contract]]
id = "a"
title = "契約表の parser と検査（contracts check）・台帳の pointer 形"
req = ["FR47", "FR48"]
section = "2-契約表"
touches = ["crate::pipe::refuse::Refuse", "crate::polarity::Guard"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail contract_"]
size = "M"
done = "contracts check が本 doc の区間を全件通し、閉包が足りない fixture を名指す"
depends = []

[[contract]]
id = "b"
title = "intake の生成（--design）と --contract の廃止"
req = ["FR47", "FR48", "FR39"]
section = "2-契約表"
touches = ["crate::pipe::refuse::Refuse"]
write-set = ["crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/snapshots"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_intake_design_"]
size = "M"
done = "toy repo の設計 doc から契約 file が生成され、--contract は usage で断られる"
depends = ["a"]

[[contract]]
id = "c"
title = "契約の審査の段（Stage::Reviewed・lens-contract.txt・review.json）"
req = ["FR49", "FR9"]
section = "4-契約の審査の段"
touches = ["crate::fleet::Stage", "crate::polarity::Guard"]
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/headless.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/snapshots"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_review_"]
size = "M"
done = "偽 lens FAIL で構築点の呼出 0・PASS で Spawned"
depends = ["b"]

[[contract]]
id = "d"
title = "land の終端（push・CI の照合・台帳の close）と rules 行 pipe.ci_wait_s"
req = ["FR50", "FR12"]
section = "5-land-の終端"
touches = ["crate::fleet::Completion", "crate::rules::RuleKind", "crate::polarity::Guard"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/land.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/pipe.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail pipe_terminal_"]
size = "M"
done = "偽 remote + 偽 CI + 偽 adapter で Landed → close の 3 event・failure は close しない"
depends = ["b"]

[[contract]]
id = "e"
title = "台帳 lint（doctor の項目）"
req = ["FR51"]
section = "6-台帳-adapter"
touches = []
write-set = ["crates/scribe2/src/ledger/mod.rs", "crates/scribe2/src/ledger/lint.rs", "crates/scribe2/src/main.rs", "crates/scribe2/tests/e2e/ledger.rs", "crates/scribe2/src/snapshots"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail ledger_lint_"]
size = "S"
done = "偽 adapter の出力で doctor の行が件数と母集団を出す"
depends = ["d"]
<!-- contracts:end -->
