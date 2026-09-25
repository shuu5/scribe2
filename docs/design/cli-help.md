# cli-help — 人が読む案内 `scribe2 help` と、AI が読む最小の使い方の 1 行

## 1. 何を解くか（裁定と現物）

やさしく言うと: 今の `scribe2` は引数が無い・違う周に「使い方の 1 行」（形だけ）を出す。形は正確だが、人には何をする口か分からない（持ち主 2026-09-25: 「ユーザーフレンドリーじゃなさすぎて分かりづらい。もっと詳しく。英語でいい」）。一方で席（AI）や hook がこの口を読む周は、今の 1 行が最小で最も正確なので変えない（同日「ユーザーが使う時限定・AI が使うときは必要最低限で AI が理解できるように」）。答えは口を 2 つに分けること: **`scribe2 help [<command>]` だけが人向けの案内（英語・説明つき）を出し、それ以外の周（引数なし・違う引数・`--help` / `-h`）は今の 1 行のまま**（`--help` / `-h` は 1 行の下に案内への pointer を 1 行足すだけ）。裁定の逐語は台帳（契約の bead の notes）。

- 出所: 持ち主の 2026-09-25 の 2 発言（UTC の分は台帳の notes）。
- 現物（verified・main d451ffe）:
  - 頂点の使い方の 1 行は `crates/scribe2-boundary/src/main.rs` の `render_usage`（`usage: scribe2 <name|--version|doctor|account|rules|fleet|vessel|hook|host-guard|pipe|runner|lens|seat|polarity|contracts>`）。`help` / `--help` / `-h` は未知の引数と同じ扱いでこの 1 行が出る（rc 0）。
  - 各 module の使い方の 1 行は module 自身が持つ（`crates/scribe2/src/account/cli.rs` / `rules/cli.rs` / `fleet/cli.rs` / `pipe/cli.rs` / `seat/cli.rs` / `headless/runner.rs` / `headless/lens.rs` / `hook/vessel.rs` / `init.rs` / `ledger/memo.rs` / `cli_args.rs` ほか・`usage: <module> <a|b|…> …` の形）。引数なしで撃つとその 1 行が出る口と、出ない口（`name` / `doctor` / `polarity` / `hook` / `host-guard` は引数なしでも本来の出力か断りを出す）が混在する。
  - doctor の external form の snapshot（`crates/scribe2-boundary/src/snapshots/`）は頂点の 1 行を含む＝引数なしの出力は 1 字も変えられない。
  - 境界 crate の本体には上限がある（rules 行 R-C4-5・core-boundary.md 行 i）＝長い文は core crate に置く。
  - 使い方の 1 行に日本語の注記が混じる口がある（`pipe` の `--repo R（cwd は読まない＝…）` など）。本設計はその字面を変えない。

## 2. 形（契約表の行 a・1 つずつ歯が測る・done と 1:1）

1. **口は 2 つ**: `scribe2 help` と `scribe2 help <command>` だけが人向けの案内を出す（rc 0）。`scribe2 --help` / `scribe2 -h` と `scribe2 <command> --help` / `-h` は今の使い方の 1 行に **pointer の 1 行**（`run: scribe2 help <command>` の形・英語）を足した 2 行（rc 0）。引数なし・未知の引数・足りない引数の周は今のまま 1 字も変えない（席と hook が読む最小の形・doctor の snapshot も不変）。`scribe2 help <unknown>` は今の未知の引数と同じ（使い方の 1 行・rc 1）。
2. **案内の中身（英語・説明つき・1 command 1 面）**: 見出しの順は固定＝`NAME`（`scribe2 <command> — <1 文の目的>`）→ `WHAT`（2〜4 文: 何をするか・人がいつ撃つか・何をしないか）→ `FORM`（その口の使い方の 1 行の逐語＝今の 1 行と同じ字面）→ `SUBCOMMANDS`（使い方の 1 行の最初の `<a|b|…>` の語ごとに 1 行の説明）→ `FLAGS`（旗ごとに 1 行の意味）→ `EXAMPLES`（1〜3 本・実在の形）→ `SEE`（設計 doc の path と §）。頂点の `scribe2 help` は command ごとに 1 行の目的と、`scribe2 help <command>` で続きが読める旨の 1 行。
3. **文は記述であって規則ではない**（憲法 C1 / N2）: 何をする口か・何をしない口かを書き、「〜せよ」「must」の規範文は書かない。規則の在処は `SEE` の設計 doc への pointer だけ。字は英語・幅 100 字以内・1 面 60 行以内（頂点は 40 行以内）。ただし `FORM` の 1 行は生きた使い方の 1 行の逐語の写し（§1 の日本語の注記と 100 字を超える口を含む）なので、幅と字種は測らず逐語の一致だけを測る（字下げもしない）。
4. **置き場は core crate の 1 file**（行 a の write-set の `+` の file・境界 crate の上限を増やさない）: 1 つの表（`&[Entry]`・欄は name / purpose / what / form / subcommands / flags / examples / see）が正本で、描画の関数は表を読むだけ。境界 crate の `main.rs` は `help` / `--help` / `-h` の arm を足すだけ（数行）。各 module の使い方の 1 行は今の場所のまま（`FORM` は表に写し、歯が生きた出力と突き合わせる＝ずれたら赤）。
5. **母集団は生きた使い方の 1 行から引く**（parity・手書きの一覧を持たない）: 頂点の表は `render_usage` の `<…|…>` の全語（`--version` を含む 15 語）を 1 つずつ持ち、各 command の `SUBCOMMANDS` はその module の使い方の 1 行の最初の `<…|…>` の全語を持つ。引数なしで使い方を出さない口（`name` / `--version` / `doctor` / `polarity` / `hook` / `host-guard` の 6 口＝`name` は名・`--version` は版の行・`doctor` は健康の面・`polarity` は極性の一覧・`hook` は 0 行・`host-guard` は断りを出す）は `FORM` を表だけが持ち、`SUBCOMMANDS` の突き合わせは免除（歯は口ごとに免除を名指す）。使い方を出す口の `FORM` の突き合わせ先は「引数なしの生きた出力のうち `usage:` で始まる最初の行」（`runner` / `lens` は 1 行目が欠けた旗の断りで 2 行目が使い方）。
6. **AI 向けの 1 行は変えない・増やさない**: 席の brief や rules 行に `help` を足さない（AI は今の 1 行で足りる）。`help` の面は人が端末で打つ口＝pipeline の runner / lens / hook の経路のどこからも呼ばれない（呼ぶ場所が無いことを歯が数える: core と境界 crate の本体に `help` の描画の呼び出しは境界 crate の dispatch の 1 か所だけ）。

## 3. 触らない

引数なしの出力・未知の引数の rc と字面・各 module の使い方の 1 行の字面と置き場・doctor の snapshot・極性一覧・rules 行（足さない）・席の brief と雛形。

## 4. 却下

- 端末か（isatty）で長短を切り替える: 人が `| less` に流す周と AI の Bash が同じ側に落ちる・歯が端末を作れない＝決定的でない。明示の語 `help` が最も単純。
- rustdoc の doc コメントから生成する: build 時の道具が増える（C17 の段で最小実装より重い）。表 1 つで足りる。
- 各 module の cli.rs に案内を分散させる: 面が 15 file に散り、幅や順の規律を歯で守れない。表 1 つ・描画 1 本。
- 日本語の案内: 持ち主が英語を選んだ（端末の幅と検索性）。
- 境界 crate に置く: R-C4-5 の上限に当たる。
- `--help` を長い案内にする: `--help` は AI と script が反射で打つ旗＝最小の形のまま pointer だけ足す。

## 5. 歯（`crates/scribe2-boundary/tests/e2e/main.rs` に `cli_help_` 接頭辞・実 binary を撃つ・lib は行 a の `+` の file の中に `help_table_` 接頭辞）

- (a) `scribe2 help` → 頂点の 15 語（引数なしの `scribe2` の出力の `<…|…>` から歯が切り出す）が各 1 行の目的つきで在り、`scribe2 help <command>` への案内の 1 行が在る（base では使い方の 1 行だけ ＝ RED）。
- (b) 15 語のそれぞれで `scribe2 help <語>` が `NAME` / `WHAT` / `FORM` / `SUBCOMMANDS` / `FLAGS` / `EXAMPLES` / `SEE` をこの順で持ち、`FORM` の行は引数なしで使い方を出す口の生きた出力の `usage:` で始まる最初の行と逐語で一致し（免除の 6 口は表の字面だけ）、`SUBCOMMANDS` は最初の `<…|…>` の全語を持つ（免除の 6 口を除く）。
- (c) `scribe2 --help` / `-h` / `<command> --help` / `<command> -h` → 使い方の 1 行 + pointer の 1 行の 2 行・rc 0（base では 1 行 ＝ RED）。
- (d) 引数なしの `scribe2` と未知の引数は 1 行のまま・rc は今のまま（不変）。`scribe2 help nosuch` → 使い方の 1 行・rc 1。
- (e) 幅 100 字以内・頂点 40 行以内・各面 60 行以内・全部 ASCII（英語）・規範の語 must を持たない（`FORM` の 1 行は (b) の逐語の一致で測り、幅と字種の対象から外す・§2 の 3）。
- (f) lib: 表の name が一意で、全欄が非空で、`see` の path が repo に実在する（`docs/design/` の下・歯は file の実在で測る）。
- (g) 呼び出しの場所: `help` の描画を呼ぶ site は境界 crate の dispatch の 1 か所（core の本体に呼び出しが無い＝grep の母集団を歯が数える）。

## 6. 後続

- 使い方の 1 行に混じる日本語の注記（`pipe` の `--repo`）を英語に揃えるかは別の周（AI 向けの形は本設計で触らない）。
- `scribe2 help` の面を design-intent の生成物に写すか（folio の generator）は v3 の話。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "scribe2 help [<command>] — 人向けの英語の案内（NAME / WHAT / FORM / SUBCOMMANDS / FLAGS / EXAMPLES / SEE）を core の表 1 つから描き、--help / -h は 1 行 + pointer、引数なしと未知は不変（§2・裁定 2026-09-25）"
req = ["FR67", "NFR4"]
section = "2"
write-set = ["+crates/scribe2/src/help.rs", "crates/scribe2/src/lib.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/cli-help.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail help_table_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail cli_help_"]
size = "M"
growth = ["crates/scribe2/src/lib.rs:2", "crates/scribe2-boundary/src/main.rs:8"]
done = "(1) scribe2 help が頂点の 15 語（引数なしの出力の <…|…> の全語）に 1 行の目的を出し、scribe2 help <command> への案内の 1 行を持つ (2) scribe2 help <command> が NAME / WHAT / FORM / SUBCOMMANDS / FLAGS / EXAMPLES / SEE の順の面を出し、FORM は引数なしで使い方を出す口の生きた出力の usage: で始まる最初の行と逐語で一致し（runner / lens は 2 行目）、SUBCOMMANDS は最初の <…|…> の全語を持つ（name / --version / doctor / polarity / hook / host-guard の 6 口は FORM を表だけが持ち SUBCOMMANDS の突き合わせを免除） (3) --help / -h（頂点と各 command の後ろ）は使い方の 1 行 + pointer の 1 行の 2 行・rc 0 (4) 引数なし・未知・足りない引数の出力と rc と doctor の snapshot は 1 字も変わらず、scribe2 help <unknown> は使い方の 1 行・rc 1 (5) 案内は英語・ASCII・幅 100 字以内・頂点 40 行以内・各面 60 行以内で、規範文（must / 〜せよ）を持たず SEE は docs/design の実在する path (6) 表は core crate の + の file の 1 つで、境界 crate は help / --help / -h の arm だけ、描画の呼び出しは境界 crate の dispatch の 1 か所 (7) rules 行・席の brief・雛形・極性一覧は不変 歯: cli_help_ の歯が (a) 頂点の 15 語の目的と案内の行（base では 1 行だけ ＝ RED）(b) 15 語の各面の見出しの順と FORM の逐語一致と SUBCOMMANDS の全語（免除 6 口を名指す）(c) --help / -h の 2 行（base では 1 行 ＝ RED）(d) 引数なし・未知の不変と help nosuch の rc 1 (e) 幅・行数・ASCII (g) 呼び出しの site 1 か所 を測り、lib の help_table_ の歯が name の一意・全欄の非空・see の path の実在を測る"
<!-- contracts:end -->
