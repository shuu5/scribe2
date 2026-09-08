# SPIKE-tooling-report — rtk / graphify を leg 0 の実 Rust workspace で測る

bd `s2-07l.2`（[v2][spike][P1][S] tooling spike-2）の成果物。
目的は rtk / graphify の採否そのものではなく、**器 SPEC §5 が「day-1 optional adapter・採否は K 指標」と
置いたまま未定義だった K 指標を、実測値つきの候補として定義できる状態にすること**である。

測定対象は leg 0 の land commit `9fe8d55`（cargo workspace 骨格・Rust 1,059 行 / `.rs` 7 本）。
v1 の実測（`sc-ol6t9` の bash corpus / `sc-a1nex` の Python 自リポ micro-bench）は**持ち越していない**。
本書の数字はすべて本 spike が Rust workspace + cargo 出力に対して取り直したものである。

---

## 1. 版 pin と入手経路

### rtk

| 項目 | 実測値 |
| --- | --- |
| 版 | **v0.48.0**（`/repos/rtk-ai/rtk/releases/latest` が返した stable） |
| 公開日 | 2026-09-04T14:35:14Z |
| artifact | `rtk-x86_64-unknown-linux-musl.tar.gz`（4,616,651 B） |
| sha256 | `e4e650fa1677c0de2f6839a6040d7b17f312d32f163c402b75af70e9e5af1a91` |
| checksums 照合 | `grep -F <artifact> checksums.txt \| sha256sum -c -` → `rtk-x86_64-unknown-linux-musl.tar.gz: OK`（rc=0） |
| 配置 | `out/bin/rtk`（10,507,680 B・musl 静的単一 binary） |
| 版の自己申告 | `rtk --version` → `rtk 0.48.0` |

入手経路は **GitHub release の pre-built artifact 1 つだけ**。`curl … | sh` 形も cargo 経路も踏んでいない。
`checksums.txt` は artifact と同じ release ページから同じ経路で取ったものなので、
この一致が保証するのは「download が壊れていないこと」までで、供給元の真正性ではない（v1 の留保をそのまま引き継ぐ）。

### graphify

| 項目 | 実測値 |
| --- | --- |
| PyPI 配布名 | **`graphifyy`**（`graphify` は PyPI に存在しない） |
| 版 | **0.9.56**（pin install） |
| wheel | `graphifyy-0.9.56-py3-none-any.whl`（1,402,428 B） |
| sha256 | `4ea42e90d2fdaf932d9ca0c5606da7dc3552b042e1705f6e82964d4d092e8c75`（PyPI JSON API の `digests.sha256`） |
| 配置 | `out/venv`（168 MB / 2,588 file・依存 32 package・うち多数が tree-sitter grammar） |
| CLI | `out/venv/bin/graphify` / `graphify-mcp` |
| installer | **踏んでいない**（`graphify install` は platform config を書くため使わない） |
| LLM key | `env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u CODEX_API_KEY` で遮断。`update` は `no LLM needed` 経路で rc=0 |

`out/` 配下に閉じたことの機械確認（本 spike 終了時点）:
`$HOME/.local/bin/rtk` / `$HOME/.cargo/bin/rtk` / `$HOME/.local/bin/graphify` /
`$HOME/.local/bin/graphify-mcp` / `$HOME/.graphify` / `$HOME/.rtk` / `$HOME/.config/rtk` /
`$HOME/.claude/plugins/graphify` は**いずれも非存在**。
共有 `.git/hooks` には `*.sample` 以外の file が無い（graphify / rtk とも hook を置いていない）。

なお契約の検証行 6（`git status --porcelain -- ':!out' …` が空）は、この worktree では**素の porcelain**ゆえ
CC sandbox が worktree へ注入する dotfile 12 件（`.bashrc` / `.claude/` / `.vscode` / `.mcp.json` …）を必ず拾う。
これは spike の産物ではなく**起動時点で既に在ったもの**で、boot 直後に取った `.scribe-untracked-baseline`
（22 entry）と突き合わせると **12 件すべてが baseline に一致し、差分は 0 件**である。
self-test はこの literal な出力（12 行）を表示したうえで baseline を差し引き、残り 0 件を PASS の条件にしている
（protocol §2「素の `git status --porcelain` の空を clean 判定に使わない — sandbox の HOME 生成物で構造的に偽になる」）。
repo 側の `.gitignore` をこれらの dotfile で膨らませる直し方は採らなかった — `.gitmodules` / `.mcp.json` は
他の repo では実在の設定 file であり、環境由来の雑音を消すために repo の方針 file を書き換えるのは代償が大きい。

---

## 1.5 実行形の逸脱（契約と実際の差・admin 裁定を要する）

**契約が指定した実行形と、実際に走った実行形は違う。**この節はその差を先に開示する（黙って進めない）。

| 面 | 契約の指定（bd s2-07l.2「## 実行形」） | 実際 |
| --- | --- | --- |
| session 形 | ad-hoc session 1 本（**admin lane・worker cell ではない**） | **worker cell**（spawn prompt で dispatch） |
| 作業 dir | **spike 用 fresh clone dir**（host の home 配下・契約が名前で指定） | **anchor の worktree**（`.worktrees/spawn/<branch>` 形） |
| branch | `spike/tooling` | `spawn/s2-07l.2-111448`（admin が gate 後に merge する前提の cell branch） |
| 本体への書込み | 「scribe2 本体・sc リポ・host には **1 byte も書かない**」 | anchor の worktree に commit `f1fafed`（50 file・+8,580 行） |
| push | しない | していない（worker cell は push しない＝**この面は守られている**） |

**原因は選択ではなく強制である。** CC の sandbox は worktree の外を read-only で mount しており、
契約が指定した spike dir を作ること自体ができない（実測）:

```
$ mkdir -p <spike 用 fresh clone dir>
mkdir: cannot create directory ‘…/scribe2-spike2’: Read-only file system   (rc=1)
```

したがって「fresh clone で測る」は本 dispatch 形では実行不能で、測るなら anchor の worktree しか無い。
worker cell は自 worktree から出ない規律も同じ方向に効く。
なお **測定対象そのものは契約どおり**である — この worktree は leg 0 land commit `9fe8d55` の checkout で、
`git rev-parse origin/main` = `9fe8d55` と一致する（＝「leg 0 の実 workspace」という契約の中身は満たしている）。

**論点 3 点と、その裁定**

1. **`out/` を main に載せるか**。一度は 49 file・約 262 KB を tracked にしていた。
   → **user 裁定 (A)・2026-09-08: 載せない。** `out/` と `graphify-out/` を tracked から外し
   （契約 v1.1 (g)）、main へ載せるのは本 report だけとした。一次データは cell branch の
   commit `f1fafed`（PR head ref）に残り、そこから復元できる。
2. **生ログの絶対 path**。cargo が診断に出す manifest dir の絶対 path が生ログに焼き込まれ、
   本 cell の commit が持ち込んだ「host の home 配下の絶対 path を持つ tracked file」は
   **実測 11 本**だった（当初の申し送りは 9 本で、report 本体 3 箇所と `sanitize-paths.py` 2 箇所を
   数え落としていた。leg 0 時点の同種 tracked file は本 spike の産物としては 0 件）。
   public repo へ push すれば public 面に載るため class=(b)「出す」として user 裁定へ回した。
   → **user 裁定 (A)・2026-09-08: 載せない**（論点 1 と同じ裁定で解消）。`out/` を untracked へ
   戻したうえで、本 report 内の絶対 path も一般語へ置換して **0 件**にした（契約 v1.1 (h)）。
3. **使い捨て branch の残置**。`spike/red-nextest` / `-clippy` / `-build` / `-xtask` / `-all` の 5 本が
   共有 `.git` に在る（push していない・`out/red-apply.py --yes <defect>...` で再生成できる＝消してよい）。

## 2. 測り方（この節の定義が数字の意味を決める）

- **corpus 4 コマンド**: `cargo nextest run --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` /
  `cargo build --workspace` / `cargo xtask check`。
- **計測面 = stdout と stderr の合流**。cargo の診断は大半が stderr に出るため、片方だけ測ると
  「agent の context に実際に載る量」とずれる。harness は `> file 2>&1` で 1 本にまとめて数える。
- **キャッシュの平準化**: 各計測の前に同じコマンドを 1 回空回しして cargo の増分状態を揃える
  （初回コンパイルの `Compiling …` 行が A/B の片側にだけ乗るのを防ぐ）。
- **filtered 側 = `rtk <cmd>` の直叩き**。hook 形（`rtk init -g`）は host を汚すため踏まない
  （v1 `sc-brjbv` で hook 形 arm は機械確定 NG 済み）。
- **red の作り方**: leg 0 land commit から生やした使い捨て branch 5 本（`spike/red-*`・**push しない**）に
  scratch commit を 1 本ずつ置いた。欠陥は `out/red-apply.py` が機械的に注入する。
  この注入器は **tracked source を壊す道具なので既定で refuse する** — `--target <path>`（temp copy へ書く）か
  `--yes`（既定 target を in-place で壊す明示同意・かつ未 commit の変更が無いときだけ）のどちらかが要る。
  本 spike の red 作成は `out/red-apply.py --yes <defect>...` の形で行った。

  | state | 中身 | 狙い |
  | --- | --- | --- |
  | `red` | 欠陥 **1 件だけ**の commit 4 本（落ちる test / clippy 警告 / compile error / xtask 違反）を、対応する 1 コマンドで測る | 各コマンドの filter を**遮蔽なしで**測る |
  | `red-all` | 4 欠陥を**同時に**入れた commit 1 本を 4 コマンドで測る | 遮蔽が起きたとき filter が何を落とすかを測る |

  この 2 段にしたのは、compile error を 1 つ入れると cargo が他 3 つの欠陥を**上流で**遮蔽するためである
  （型エラーは lint pass と test 実行の前に落ちる）。単一 state だけで測ると「rtk が落とした」と
  「cargo がそもそも出していない」を区別できず、K2 が測定不能になる。
  契約 (a) が要求する 8 行（green 4 + red 4）は `red` 側が満たし、`red-all` は同じ物差しの追加 4 行として並べてある。

生データは `out/raw/<state>.<cmd>.<base|rtk>.txt`、集計は `out/rtk-ab.tsv` / `out/rtk-literals.tsv` /
`out/metrics.json`、再現手順は `out/ab-run.sh` / `out/red-apply.py` / `out/literals-check.py` / `out/metrics.py` にある。

---

## 3. (a) rtk A/B 実測

`rtk_raw` = rtk の出力そのまま。`rtk_net` = そこから **hook 未 install の banner 1 行（78 B）** を除いた値。
banner は `[rtk] /!\ No hook installed — run 'rtk init -g' for automatic token savings` で、
**毎回 stderr に必ず出る**。`RTK_DISABLED` / `RTK_TELEMETRY_DISABLED` 等の env では消せず、
消す手段は hook を install することだけ＝本 spike の fence では消せない。
したがって **CLI 直叩きで運用する限り `rtk_raw` が実効値**であり、`rtk_net` は「hook を入れたら届きうる上限」である。

| state | cmd | base B | rtk_raw B | rtk_net B | raw 削減 | net 削減 | rc(base/filtered) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| green | nextest | 1057 | 123 | 45 | **-88.4%** | -95.7% | 0 / 0 ✓ |
| green | clippy | 72 | 108 | 30 | +50.0% | -58.3% | 0 / 0 ✓ |
| green | build | 72 | 150 | 72 | +108.3% | ±0% | 0 / 0 ✓ |
| green | xtask | 203 | 281 | 203 | +38.4% | ±0% | 0 / 0 ✓ |
| red | nextest | 1882 | 809 | 731 | **-57.0%** | -61.2% | 100 / 100 ✓ |
| red | clippy | 837 | 722 | 644 | -13.7% | -23.1% | 101 / 101 ✓ |
| red | build | 536 | 393 | 315 | -26.7% | -41.2% | 101 / 101 ✓ |
| red | xtask | 178 | 256 | 178 | +43.8% | ±0% | 1 / 1 ✓ |
| **計（green+red 8 run）** | | **4837** | **2842** | **2218** | **-41.2%** | **-54.1%** | **8/8 一致** |
| red-all | nextest | 719 | 79 | 1 | -89.0% | -99.9% | 101 / 101 ✓ |
| red-all | clippy | 675 | 457 | 379 | -32.3% | -43.9% | 101 / 101 ✓ |
| red-all | build | 536 | 393 | 315 | -26.7% | -41.2% | 101 / 101 ✓ |
| red-all | xtask | 178 | 256 | 178 | +43.8% | ±0% | 1 / 1 ✓ |

読み取り:

- **利得の絶対量が小さい**。green+red の 8 run 合計で 4,837 B → 2,842 B（1,995 B 減）、1 run あたり平均 249 B。
  この器の cargo 出力がそもそも小さい（green の build / clippy は 72 B = `Finished` 1 行）ためで、
  **削減率より「削る対象がほとんど無い」ことが本体の事実**である。
- **baseline が小さいと純増する（8 run 中 4 run）**。増えたのは green clippy（72→108 B・+50.0%）/
  green build（72→150 B・+108.3%）/ green xtask（203→281 B・+38.4%）/ red xtask（178→256 B・+43.8%）の 4 件。
  原因は filter の有無ではなく **banner 78 B/回の固定費**である: baseline が 72〜203 B しかない run では、
  filter が何 B 削っても banner がそれを上回る。実際、増えた 4 件のうち clippy と build は
  **専用 filter を持つ対応コマンド**であり（green clippy は `Finished` 行を `cargo clippy: No issues found` に
  置き換えて実体を 72→30 B に縮めているのに、banner 込みでは 108 B で純増する）、
  純粋な「filter が無いから素通し」なのは xtask の 2 件だけである
  （`cargo xtask check` に専用 filter は無く `rtk rewrite -- cargo xtask check` も何も返さない＝実体は ±0 B）。
- **`rtk cargo` は未知サブコマンドを素通しする**。`rtk cargo --help` は build/test/clippy/check/install/nextest の
  6 本しか挙げないが、`rtk cargo xtask check` は rc も出力もそのまま通る（落ちない）。
- **`rtk rewrite` の対応表は subcommand 集合と一致しない**。`cargo clippy` / `cargo build` は rtk 形を返すが、
  `cargo nextest run --workspace` は**何も返さない**（専用 filter が実在するのに）。
  hook 形はこの `rewrite` が単一の SSOT なので、hook を入れても nextest は filter されない公算が高い。
- **rc は 12/12 で透過**（0 / 1 / 100 / 101 すべて一致）。

---

## 4. (b) 失敗情報の生存

宣言 literal（「失敗の所在」を特定するのに要る字面）を fixed-string で filtered 出力に照合した。
判定は `out/rtk-literals.tsv`（列 = state / cmd / literal / kept|lost・18 行）。
`out/rtk-literals-attribution.tsv` は同じ 18 件に **baseline に在ったか**を足し、
lost を「rtk が落とした」と「cargo がそもそも出していない」に分ける。

| state | cmd | 宣言 literal | 生存 |
| --- | --- | --- | --- |
| red | nextest | `red_scratch_name_length` / `red-scratch: NAME 長が期待と違う` / `crates/scribe2/src/main.rs:108` | kept ×3 |
| red | clippy | `needless_return` / ``unneeded `return` statement`` / `crates/scribe2/src/main.rs:25:5` | kept ×3 |
| red | build | `E0308` / `mismatched types` / `crates/scribe2/src/main.rs:25:32` | kept ×3 |
| red | xtask | `name-literal` / `crates/scribe2/src/main.rs が NAME の字面を持つ` | kept ×2 |
| red-all | nextest | `E0308` / `mismatched types` | **lost ×2（rtk 起因）** |
| red-all | clippy | `E0308` kept / `needless_return` lost（**上流 cargo 起因**・baseline にも無い） | 1 kept / 1 lost |
| red-all | build | `E0308` / `mismatched types` | kept ×2 |
| red-all | xtask | `name-literal` | kept |

- **欠陥 1 件ずつ（`red`）では 11/11 = 100% 生存**。test 名・assert 本文・panic の `file:line`・
  clippy の lint 名と `file:line:col`・xtask の違反 1 行、いずれも欠けない。
- **`rtk cargo nextest` は build 失敗時に出力を全損する（最大の欠陥）**。
  `red-all` の nextest では baseline に E0308 の診断が 719 B / 12 行あるのに、
  rtk の出力は **banner 1 行だけ（79 B・実体 1 B）** で、compile error が丸ごと消える。
  rc は 101 を透過するので「失敗したのは分かるが、何が失敗したかが出力から完全に失われる」形になる。
  agent にとっては最悪の失敗様態（rc を見て「もう一度素で回す」しかなくなり、往復が増えて token は逆に増える）。
- `red-all` clippy で `needless_return` が消えるのは **rtk のせいではない**。型エラーが lint pass より先に
  落ちるため baseline にも出ていない。attribution file がこれを機械で弁別している。
- `rtk cargo build` は診断本体を保つ一方で `For more information … rustc --explain E0308` と
  `error: could not compile …` の 2 行を落とし、`cargo build: 1 errors, 0 warnings (1 crates)` に置き換える。
  **落ちているのは冗長行だけ**で、所在情報は残る。

---

## 5. (c) graphify 実測

```
graphify nodes=3132 edges=10107 rs_files=7 rs_kinds=file,function,method,struct secs=9.33 bytes=5049099
```

（`graphify update .` を leg 0 workspace の root で 1 回。rc=0。生成物は `graphify-out/`＝
`graph.json` 5,049,099 B / `graph.html` 3,784,653 B / `GRAPH_REPORT.md` 27,757 B / `manifest.json` / `cache/`、合計 8.9 MB。
`out/graphify/graph.json` に保全済み。full graph は 5 MB あるので tracked にはせず、
`.rs` 由来の 87 node と接続 332 edge を抜いた `out/graphify/rs-subgraph.json` を検品用に置いた。）

### 何が graph に入ったか

| 面 | 実測 |
| --- | --- |
| node 総数 | 3,132 |
| うち `.rs` 由来 | **87（2.8%）** |
| 最大寄与 file | `design-intent/assets/mermaid.min.js` が **2,989 node（95.4%）** |
| `.rs` file 被覆 | 7/7（workspace の `.rs` 全数） |
| `.rs` の node 種 | file 7 / function 76 / method 5(表記は `.name()`) / struct 4 |
| `.rs` に接する edge | 332（references 143 / calls 98 / contains 75 / imports_from 11 / method 5） |

- **node の 95.4% が vendored の minified JS 1 本**。`out/` `target/` `.git/` `node_modules/` `graphify-out/` 等は
  既定の `_SKIP_DIRS` で除かれるが、repo が抱える minified asset は除かれない。
  「Rust の構造を読ませる」用途で回すと、**graph のほぼ全部が読ませたくないものになる**。
- **宣言の被覆は 80.8%（80/99）で、欠落の中身が悪い**（`out/rust-coverage.txt`）:

  | item | 宣言数 | graph 内 | 被覆 |
  | --- | ---: | ---: | ---: |
  | `fn` | 76 | 76 | 100% |
  | `struct` | 4 | 4 | 100% |
  | `const` | 12 | 0 | **0%** |
  | `mod` | 7 | 0 | **0%** |
  | 計 | 99 | 80 | 80.8% |

  落ちている 12 個の `const` には **`NAME`（この器の名前の単一 SSOT）** と
  `MAX_CORE_LINES` / `MAX_FILE_LINES` / `REQUIRED_LINTS`（`cargo xtask check` の閾値 SSOT）が含まれる。
  leg 0 の設計上いちばん重要な不変条件（名前の字面は `name.rs` の 1 行だけ・閾値は `limits.rs` だけ）を
  担う item 種が、まるごと graph に存在しない。`name.rs` は **file node 1 個だけ**で中身が空である。

### query A/B（同一 file: `crates/xtask/src/check.rs`）

| 面 | bytes | lines | `measure_name_literal` | `split_test_src` | `MAX_FILE_LINES` |
| --- | ---: | ---: | --- | --- | --- |
| A: `cat crates/xtask/src/check.rs` | 30,180 | 834 | kept | kept | kept |
| B: `graphify explain "check.rs" --budget 2000` | 1,743 | 31 | **lost** | **lost** | **lost** |

削減は **-94.2%** だが、**宣言 identifier 3/3 が消えた**。原因は budget ではなく
**接続の固定 20 件打ち切り**である（出力末尾が `... and 36 more`）。実測で
`--budget 200` / `2000` / `20000` の 3 通りとも出力は **1,743 B で不変**＝
契約が指定した `explain --budget 2000` の `--budget` は **explain では no-op** である
（`--budget` は help 上 `query` 系の option で、`explain` は受理はするが効かない）。

node を名指しした `explain "measure_name_literal()"` は 485 B で
`crates/xtask/src/check.rs L236` と 4 本の接続（`check.rs` contains / `Measured` `Layout` `SourceFile` references）を返す。
**「どの node を見るか既に分かっている」ときは有効、「file の中身を知りたい」ときは無効**、という形である。

---

## 6. (d) K 指標候補（採否の物差し）

各行は `K<番号> <名前>: <実測値> — <採用条件の案>`。実測値は本書 §3〜§5 の数字である。

**⚠ 番号空間が既存 SSOT と衝突する（leg 2 が解く）。** `design-intent/vocabulary.yaml:83-86` は canonical
「K 指標」を **「器の成熟と運用の健全さを測る指標群 (K1〜K3)。閾値は rules 行。成熟条件と day-1 optional
adapter の採否判断に使う」** と定義しており、**K1〜K3 という番号を既に予約している**
（ただし K1 / K2 / K3 の中身は repo のどこにも書かれていない＝予約済みで未実装）。
本書の K1〜K7 は「day-1 optional adapter の採否」という**用途の片方だけ**を占める別の集合であり、
番号が正面から重なる。本 spike は契約 acceptance が `K<番号> <名前>:` の字面を要求するため
この形で書いたが、**leg 2 は焼く前に番号空間を解くこと** — 選択肢は
① 既存の K1〜K3（成熟・運用健全さ）に本書の 7 本を K4 以降として連結する、
② 本書の集合を別名（例 `KA1`〜`KA7` = adapter 採否）へ改番して vocabulary に 2 群として登録する、
の 2 つで、どちらを採るかは vocabulary の所有者（leg 2）の裁定である。


K1 出力削減率（byte・corpus 合計）: rtk = green+red 8 run で 4,837 B → 2,842 B（**-41.2%**、banner を除けば -54.1%）。ただし 8 run 中 **4 run が増加**（green clippy +50.0% / green build +108.3% / green xtask +38.4% / red xtask +43.8%）。増加は「未対応コマンドだから」ではなく **banner 78 B/回の固定費が小さい baseline を上回るから**で、4 件のうち clippy と build は専用 filter を持つ対応コマンドである。graphify explain = 30,180 B → 1,743 B（-94.2%）。 — 採用条件の案: 「corpus 合計で raw 削減率 ≥ 30%」**かつ**「単一コマンドでも出力が baseline を上回らない（増加 0 件）」の AND。前段だけを見て採ると、出力の小さいコマンドで静かに token を増やす adapter を通してしまう。rtk は前段を満たし後段で落ちる。

K2 失敗情報の生存率: rtk = 欠陥 1 件ずつの `red` で **11/11（100%）**、遮蔽ありの `red-all` を含めた全 18 件で 15/18、**rtk 帰責の loss は 2 件（いずれも `rtk cargo nextest` の build 失敗時）**。 — 採用条件の案: 「宣言 literal の **rtk 帰責 loss が 0 件**」を**サブコマンド単位**の必須条件にする（corpus 平均で丸めない）。1 件でも帰責 loss があるサブコマンドは、削減率がいくら良くても不採用にする。`rtk cargo nextest` はこの条件で失格。

K3 rc 透過: rtk = **12/12 一致**（rc 0 / 1 / 100 / 101 のすべて）。graphify は該当なし（読取り系）。 — 採用条件の案: 「rc 透過 100% を無条件の必須要件とする（1 件でも不一致なら即不採用・例外なし）」。rc が透過しない wrapper は CI と gate の判定面を壊すため、削減率とトレードオフにしてはならない。rtk は満たす。

K4 版の安定（出力形に literal 依存できるか）: rtk = stable 8 release / 68 日（0.12 rel/日）に加え **prerelease 92 本 / 70 日（1.3 rc/日）**、v0.45.0（v1 実測時）→ v0.48.0（本 spike）が **28 日で minor 3 本**。graphifyy = **224 release / 155 日（1.45 rel/日）・直近 30 日で 21 本**、0.9.50 → 0.9.56。両者とも 0.x 帯。 — 採用条件の案: 「出力形の literal に依存する判定（gate の grep 照合など）は、**0.x かつ 30 日で minor / patch が 2 本以上上がる依存の上に置かない**。置くなら ① 版を pin し ② 出力形の回帰 test を自前で持ち ③ 版上げを人手 gate にする、の 3 点セットを同時に満たすときだけ条件付き採用」。本 spike の 2 つはどちらも「置かない」側に落ちる。

K5 対象言語の読解（宣言被覆と node の純度）: graphify = **item 被覆 80/99 = 80.8%**（fn 76/76・struct 4/4・**const 0/12**・**mod 0/7**）、`.rs` 由来 node は全 node の **2.8%**（95.4% は vendored の `mermaid.min.js` 1 本）。 — 採用条件の案: 「① 対象言語の宣言被覆 ≥ 95%、② **器の不変条件を担う item 種の被覆が 0% でないこと**（本器では `const`＝`NAME` と閾値群）、③ 対象言語由来の node が全 node の 50% 以上（= 除外設定で雑音を落とせること）」の AND。graphify は 3 条件すべてで落ちる。

K6 生成コストと生成物の重さ: graphify = 再生成 **9.33 s**、生成物 8.9 MB（`graph.json` 5.05 MB / `graph.html` 3.78 MB）、install 面積 **168 MB / 2,588 file / 32 package**。rtk = 生成物なし、install 面積 **単一静的 binary 10.5 MB**。 — 採用条件の案: 「① 1 回の再生成 ≤ 60 s、② **repo に tracked で残す生成物 ≤ 1 MB**（超えるなら gitignore + CI 生成に回す）、③ install 面積が単一 binary か、`uv` 等で `out/` に閉じられること」。rtk は 3 つとも通る。graphify は ① を通り ② で落ちる（本 spike も `graph.json` を untracked にした）。

K7 token 換算: **未測定＝byte を代理とする**。tokenizer（tiktoken 等）を spike dir に入れると被験対象でない依存が増えるため測っていない。代理が妥当なのは「同一 corpus・同一文字種での相対比較」までで、cargo 診断（ASCII 主体）と `xtask check` の違反行（日本語混在）は byte/token 比が異なるため**両者を横断して比べてはならない**。 — 採用条件の案: 「K1 / K2 で足切りを通った候補についてのみ、採否の最終判定を **token で測り直す**。byte 代理での差が ±10% 以内なら有意と見なさず、token 実測が出るまで採用しない」。

---

## 7. 摩擦（実際に踏んだもの）

**install 面積**

- rtk: musl 静的単一 binary 1 本（10.5 MB）を展開するだけ。`$HOME` にも `.git/hooks` にも何も書かない（§1 で機械確認）。
- graphify: **`python3 -m venv` が使えなかった**。この host の Python 3.12.3 には `ensurepip` が無く
  （`No module named ensurepip`）、venv 作成が失敗する。`apt install python3.12-venv` は host 変更なので踏めない。
  回避は既存の `uv`（0.11.26）で、`UV_CACHE_DIR` / `UV_PYTHON_INSTALL_DIR` を `out/` へ向けた上で
  `uv venv` + `uv pip install --python out/venv/bin/python graphifyy==0.9.56`（5.5 s）。
  結果 168 MB / 2,588 file。依存の大半は tree-sitter grammar（各言語 1 package）で、
  **Rust だけ欲しくても全言語分が入る**。

**key 遮断**

- `graphify update` は `no LLM needed` 経路で、5 種の API key を `env -u` で落としても rc=0 で完走する。
  ただし完了時に `Tip: set GEMINI_API_KEY or GOOGLE_API_KEY to use Gemini for semantic extraction.` を出す＝
  **AST 抽出だけが動いており、semantic 抽出は未実行**である。本書の graph はすべて AST 由来（`_origin: "ast"`）。
- `graphify install` / `graphify-mcp` は踏んでいない（platform config と MCP 登録を書くため）。

**0.x 帯の出力形**

- rtk の banner が **消せない**。`RTK_DISABLED` / `RTK_TELEMETRY_DISABLED` / `RTK_NO_HOOK_WARNING` / `RTK_QUIET` を
  試したが、どれでも 78 B の banner は出る。消す唯一の手段が「hook を install する」＝
  **CLI 直叩き運用に対して恒久の課税**になっている。小さい出力ほど相対的な害が大きく、
  この器の green build（72 B）では**出力が 2 倍以上に膨らむ**。
- rtk の `rewrite`（hook の SSOT）が subcommand 集合と食い違う（§3）。
- graphify の `explain --budget N` が **no-op**（§5）。契約が指定した invocation がそのまま効かない形で、
  0.x 帯の option が「受理されるが効かない」ことの実例である。
- graphify は自前の署名 file（`.graphify_labels.json.sig`）と `cache/` を生成物 dir に置く。

---

## 8. 推奨と、leg 2 の rules 行へ焼く候補の文面

### rtk → **見送り**（SPEC §5 の seam は残すが既定 off・leg 2 で有効化しない）

自分が定義した K 指標をそのまま自分に適用すると、rtk は次の順で落ちる。**推奨を実測より甘くしない**。

1. **K1 で落ちる**。corpus 合計は -41.2% だが、採用条件の後段「単一コマンドでも増加 0 件」に対して
   **増加が 4 件**ある（green clippy / green build / green xtask / red xtask）。
2. **K2 で `rtk cargo nextest` が落ちる**。build 失敗時に compile error が rtk 帰責で全損する
   （baseline 719 B / 12 行 → rtk の実体 1 B・rc だけ透過）。
   そして削減の大半はこの nextest が稼いでいる — **nextest だけで 2,939 B → 932 B（-68.3%）**、
   corpus 全体の削減 1,995 B のうち **2,007 B が nextest 由来**（他は合計で純増）。
   **いちばん効くサブコマンドが、いちばん危ないサブコマンドである。**
3. **残りに意味のある利得が無い**。nextest を外し、失敗情報を守れる `cargo build` / `cargo clippy` だけに
   絞った場合の実測は **1,517 B → 1,373 B（-9.5%・4 run で 144 B）**。
   内訳は green が 144 B → 258 B（**+79.2%**）、red が 1,373 B → 1,115 B（-18.8%）で、
   **成功時は増え、失敗時だけ減る**。K7 の採用条件「byte 代理での差が ±10% 以内なら有意と見なさない」に
   照らすと -9.5% は**有意でない**。`cargo xtask` に至っては 381 B → 537 B（+40.9%）で常に純増する。

つまり「条件付き採用」と書けるだけの利得が実測に無い。SPEC §5 が day-1 optional adapter の
**seam を置くこと自体は定めている**ので seam は残すが、leg 2 で有効化はしない、が実測に合う結論である。

leg 2 の rules 行へ焼く候補（そのまま貼れる文面）:

> `tooling.rtk.mode = "off"`（既定・leg 2 では有効化しない）。seam は SPEC §5 の day-1 optional adapter として残す。
> `cargo nextest` への rtk 適用は**恒久に禁止**する — build 失敗時に compile error 出力が全損し（rc だけ透過）、
> 失敗の所在が context から消えるため（spike s2-07l.2 実測: baseline 719 B / 12 行 → rtk の実体 1 B）。
> 有効化を再検討してよいのは次の 3 つが**同時に**満たされたときだけである —
> ① hook 未 install の banner（78 B/回）が env で抑止できるようになる、
> ② `rtk cargo nextest` が build 失敗時にも診断を透過する、
> ③ 対象 corpus の出力が本器より 1 桁大きく、{build, clippy} だけで削減 ≥ 30% を実測できる
> （本器の実測は -9.5% で、K7 の有意水準 ±10% に届かない）。
> 版は pin し、上げるときは本 rules 行の前提（rc 透過・失敗 literal 生存）を再実測してからにする。

### graphify → **見送り**（seam も置かない）

根拠: K5 の 3 条件すべてで落ちる（宣言被覆 80.8% < 95%、`const` 被覆 **0%**、`.rs` 由来 node が **2.8%**）。
落ちている `const` に **`NAME` と `xtask` の閾値群**という、この器の不変条件そのものを担う item が入っている。
生成物の重さ（K6 の条件 ②）でも落ちる — tracked に置けない 8.9 MB の生成物である。
`explain` は「見る node が既に分かっている」ときだけ有効で、その状況では `rg` + `sed -n` の方が安く正確である。

leg 2 の rules 行へ焼く候補（そのまま貼れる文面）:

> `tooling.graphify` の seam は置かない（day-1 optional adapter から外す）。
> 再評価の条件は 3 つで、**すべて満たされたときだけ**本 spike をやり直す —
> ① Rust の `const` / `mod` が node として抽出されること（現状 0/12・0/7）、
> ② 抽出対象を dir / 言語で絞る手段があること（現状 node の 95.4% が vendored な minified JS 1 本）、
> ③ `explain` の `--budget` が実際に効くこと（現状 200/2000/20000 で出力 1,743 B 不変の no-op）。

### K 指標そのもの → **採用**（K1〜K7 を leg 2 の物差しとして焼く）

K3（rc 透過 100%）と K2（rtk 帰責 loss 0・サブコマンド単位）を**必須の足切り**、
そのうえで K1 / K5 / K6 を**採否の本体**、K4 を**運用可否**、K7 を**最終判定の測り直し義務**として据える形を推す。
本 spike の 2 候補は、この物差しに掛けると **どちらも見送り**に落ちる（rtk = K1 で増加 4 件・K2 で nextest が全損・残る利得 -9.5% は K7 の有意水準未満 / graphify = K5 の 3 条件すべてと K6 ② で落ちる）。**物差しが自分の推したい結論を否定できている**ことを、K 指標そのものを採る根拠に置く。

---

## 9. (iv) 測れなかった項目

1. **token 実測**（K7）。byte 代理で通した。tokenizer を入れると被験でない依存が増えるため。
2. **hook 形の rtk**（`rtk init -g`）。host の設定を書くため踏めない（v1 `sc-brjbv` で arm は機械確定 NG）。
   よって「banner が消えた後の実効削減率」は未測定で、本書の `rtk_net` 列は**上限の推定**にすぎない。
   加えて hook 形の対応表 `rtk rewrite` が nextest を返さない（§3）ため、hook を入れても nextest が
   filter されるかは未確認である。
3. **graphify の semantic 抽出**。LLM key を遮断した AST 経路だけを測った。key を与えた場合の被覆は未測定。
4. **MCP / skill 統合形の token 収支**。tool schema の prompt 増分を含めた比較は両者とも未測定。
5. **スケール**。leg 0 の Rust は 1,059 行 / 7 file しかない。出力が大きくなる workspace
   （依存を持つ crate の初回ビルド、warning が数十件出る状態）での削減率は外挿していない。
   本書の数字は「小さい workspace の warm cache」に限った実測である。
6. **cold cache / 初回コンパイル**の出力。harness は各計測前に空回しして warm 状態に揃えている。
7. **rtk の他 60+ サブコマンド**（git / gh / rg / find / read 等）。契約の corpus が cargo 4 本だったため測っていない。
8. **並列度・実行時間**。本書は bytes / lines / rc だけを測り、wall-clock の増分（rtk が挟まる分）は測っていない。
9. **契約が指定した実行形（fresh clone `scribe2-spike2` / branch `spike/tooling`）での挙動**。sandbox が worktree の外を read-only で mount するため実行不能だった（§1.5 に実測と admin 裁定事項）。本体 worktree で測ったことが数字に与える影響は、cargo 診断に載る絶対 path の長さだけである。

## 10. (e) session の消費

- **所要: 約 51 分（90 分の上限内）**。2026-09-08 11:14 JST 起動 → 11:34:39 に本体 commit `f1fafed`、
  自己点検 WF の完走を挟んで 12:05:24 に fix commit `d36f7b2`。
- **token: 契約 (e) の上限 300k を超過している（約 2.8 倍）**。
  自己点検で cell-quality WF（`wf_186d9117-9fd`）を **1 本起動**し、その中で agent 18 本が走って
  **`subagent_tokens` = 831,177**（所要 1,253 s）を消費した。831,177 / 300,000 ≒ **2.8 倍**である。
  これは subagent 分だけの実測値で、session 本体（main loop）の消費はこれに加算されるため、
  **総消費はさらに大きい**（正確な総量は admin が完了通知 / 窓の footer で読む）。
  超過の主因は自己点検 WF 1 本で、spike の測定作業そのもの（rtk / graphify の導入と A/B）ではない。

  ⚠ 本節は当初「WF を 1 本も起動せず、上限内で完了」と書いていた。これは WF を起動する**前**に書いた文を
  起動後に更新し忘れたもので、実測と正反対の誤りだった（独立 gate が検出・bd `s2-07l.2` の DONE note 側は
  当初から正しく `wf_186d9117-9fd` / 831,177 token を記帳していた）。上の 3 行が実測である。

---

## 付録: 一次データの所在

**main に載せるのは本 report だけである（user 裁定 (A)・2026-09-08・class=(b)「出す」）。**
実測の生ログ・集計 tsv・再現 harness（`out/` 一式）と graphify の生成物（`graphify-out/`）は
**tracked から外してある**（契約 v1.1 (g)）。理由は §1.5 の論点 2 — cargo が診断に出す
host の home 配下の絶対 path が生ログに焼き込まれ、public repo へ push すると public 面に載るためである。

一次データは失われていない。所在は次のとおり:

| 何が | どこに |
| --- | --- |
| 生ログ 24 run（baseline / rtk 各 12）・集計 tsv 3 本・機械導出 json・再現 harness 6 本 | **cell branch の commit `f1fafed`**（PR head ref から復元できる） |
| 本書が引く数字そのもの | **本 report の §3〜§6 の表**（(a) の A/B 12 行・(b) の literal 生存 8 行・(c) の graphify 行と query A/B 表・(d) の K1〜K7） |
| graphify の full graph（5.05 MB）・rtk の binary・venv・API 生応答 | どこにも tracked しない（§2 の手順で再生成する。版は §1 に pin してある） |

つまり **report だけを読めば (a)〜(d) の実測値は全部読める**状態にしてある
（tsv は report の表と同じ数字を持つ二重化であり、外したことで失われる測定内容は無い）。

使い捨て branch `spike/red-nextest` / `spike/red-clippy` / `spike/red-build` / `spike/red-xtask` / `spike/red-all`
は local に残してある（**push していない**）。欠陥は `out/red-apply.py --yes <defect>...` が再生成できるので、消してよい。

### 付記: 注入器の guard は机上の話ではない

自己点検 WF が `out/red-apply.py` を「tracked source を無 guard で in-place 破壊する」と指摘したので
既定 refuse（`--target` / `--yes`）へ直したが、**その guard を外す変異を self-test へ流した瞬間に
`crates/scribe2/src/main.rs` が実際に 18 行の欠陥入りへ書き換わった**（`git restore` で復旧）。
無 guard 版が worktree に居るかぎり、self-test を 1 回回すだけで tracked source が壊れうる、という
指摘そのものがセッション内で再現された形である。現在の self-test は temp copy にしか書かないので、
落ちても tracked file は 1 byte も動かない（`repro_apply` が `git diff --quiet` で毎回それを assert する）。
