# host-init — 新しい repo を器に載せる口を 1 発にする（`init` / `host init`・doctor の欠落の名指し・引数の無い席の起動）

台帳 `s2-07l.609`（持ち主の裁定 2026-09-24T13:02Z / 13:04Z・逐語は台帳）。[ADR-0063](../../design-intent/decisions/ADR-0063-a-new-repo-joins-the-vessel-with-one-command-inheriting-the-host-face-from-a-template-state-dir.html)。

## 1. 何を解くか

やさしく言うと: 今は新しい repo を器に載せるのに人が 7 手（置き場の dir・口座の配線・host の面の写し・git の設定・marker・宣言・tmux と席の長い起動）を打ち、1 つ抜けると席は黙って動かない。裁定は「`init` これで終わり」の簡便さと「人が打つ command は最小限」と「人間向けの説明」の 3 点。本設計は人が打つのを **`host init`（host に 1 回）→ `init`（repo ごとに 1 回）→ 困ったら `doctor`** の 3 語に閉じ、残りは器が行う。

- 出所（verified・2026-09-24）: scribe3 の立ち上げは手作業 7 手で、`vessel init` と `.vessel.toml` が抜けた。抜けた状態では plugin の SessionStart hook が「仕えない repo」として 0 byte で黙り、席に指示文が入らず、`seat launch` は `launch-unconfirmed` で保留になる（memo `s2-07l.604` の実地試験でも同じ）。project-k / paper の移行も散文の runbook（repo の外）を人が辿った（memo `s2-07l.491` の notes・15 段）。doctor はこの欠落を名指さない。
- 要件: [FR58](../../design-intent/spec/srs.html#FR58)（口座の登録は設定 dir を state dir の下に作り credential は読まず書かない）・[FR59](../../design-intent/spec/srs.html#FR59)（席の起動は役割を必須に受け、群の置き場は群の今の口座で起きる）・[FR61](../../design-intent/spec/srs.html#FR61)（doctor は導入先ごとに実測を名指す）。

## 2. 現物（verified・main 0b6af26）

- `vessel init --state-dir D [ROOT]`（`crates/scribe2/src/hook/vessel.rs` の `init`）は git の local 設定 `<NAME>.stateDir` → marker `.vessel`（`name=` / `version=` の 2 行）の順に書き、別名乗りの marker・設定を書けない周は 1 byte も書かず断る。env も HOME も読まない。
- host の面 `<state_dir>/host.toml`（`crates/scribe2/src/rules/manifest.rs` の `HostManifest`）が受けるのは `[[account]]`（`label`）・`[[plugin]]`（`dir`）・`[[launch-arg]]`（`value`）・`[[vessel]]`（`repo`・最大 1 行）・`[[account-group]]`（`name` / `anchors` / `accounts`）の 5 表。面が無い周は 0 宣言で止めない。`[[rule]]` は受けない。
- 口座の dir `<state_dir>/accounts/<label>` は `account add`（`crates/scribe2/src/account/mod.rs`・FR58）が実 dir として作り、席の前提の設定だけを書く。器は credential の在り処を持たない。host では複数の置き場が同じ口座を使うので、置き場の `accounts/<label>` は既存の設定 dir への symlink になっている（host の運用・器は知らない）。
- `seat launch` は `--state-dir S --role R --target S:W` を必須に受け（`crates/scribe2/src/seat/cli.rs` の `launch_flags`）、tmux の session は在る前提（`crates/scribe2/src/seat/cycle/launch.rs` の `prepare` が `has-session` で確かめ、無ければ `session-missing` で断る）。window は無ければ作る（`create_window`）。短い形の既定（`short_defaults`）は呼び手の pane の session と役割の字面から target を組む。
- doctor（`crates/scribe2-boundary/src/main.rs` の `render_doctor_with`）は席の登録 row・口座・導入先・配線の行を連ねるが、marker・宣言・host の面の欠落を名指す行は無い。
- SessionStart hook（`crates/scribe2/src/hook/mod.rs` の `anchor_of` → `vessel::served`）は marker が `ByMe` でない repo で 0 byte（設計 vessel-hook.md §2 のとおり）。
- 宣言 `.vessel.toml`（`crates/scribe2/src/pipe/declaration.rs`）の必須 key は `schema` / `allowed-commands` / `common-verify`、任意は `requirements` / `entrance-flip` 等。cargo の行を持つ宣言は入口の flip の行か `entrance-flip = "unmeasured"` の名乗りを要る。
- 子 process は core が `Invocation`（`crates/scribe2/src/invocation.rs`・ADR-0062）で記述し境界 crate が撃つ（core-spawn は deny）。

形（判定の順・番号は契約表の行と 1:1・§3〜§7）:

## 3. `host init <TEMPLATE>`（行 a・host に 1 回）

- 入力: 既存の置き場（state dir）1 つ。`<TEMPLATE>/host.toml` が読めない（`Unreadable`）周と dir が無い周は断る（`Absent` は受ける＝0 宣言の雛形）。
- 効果: git の **global** 設定 `<NAME>.template` に絶対 path を書く（host 単位の唯一の pointer・`vessel init` の local 設定と同じ道具・env と HOME を読まない）。既に同じ値なら書かず `unchanged`。
- 出力 1 行: `host: init template=<path> <written|unchanged>`。
- doctor は `host-template=<path|absent|unreadable>` の 1 行を出す（骨格の 2 行の直後・置き場を渡さない周も出る＝`init` の前に確かめられる）。
- doctor の行数と行の位置を pin する既存の歯（`crates/scribe2-boundary/tests/e2e/seat/register.rs`・`crates/scribe2-boundary/tests/e2e/seat.rs`・`crates/scribe2-boundary/tests/e2e/seat/account.rs`）は `host-template=` の 1 行分（行 d では `init=` の 1 行分も）だけ本数と位置を進める（同じ便で更新・行 a / 行 d の write-set）。
- doctor の外形は insta の snapshot 3 本（`crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap`・`scribe2__tests__ledger_form_doctor_external_form.snap`・`scribe2__tests__ledger_lint_doctor_external_form.snap`）が pin する。骨格の直後に 1 行を足すので、行 a はその 3 本を write-set に持ち、新しい形へ更新する（write-set の外の .snap を触らない）。行 d の `init=` も同じ 3 本を更新する。
- 要件との対応: 雛形の pointer そのものは SRS の語彙に無い。FR58（1 command の導入）を成す手段として本設計が決め（ADR-0063 §2）、doctor の 1 行は FR61（doctor の 1 項目）の項目の 1 つとして足す。

## 4. `init [ROOT]`（行 b・repo ごとに 1 回・段の順は固定・各段は冪等）

`ROOT` の既定は cwd。git の repo でない・`host-template` が無い周は 1 段目の前に断る（何も書かない）。各段は「既に在れば skip」で、2 度撃っても壊れない。出力は段ごとに 1 行（`init: <段> <ok|skip|failed:<理由>>`）で、最後に `next=` を 1 つ（全段 ok / skip なら `next=doctor`）。

1. **置き場**: 新しい state dir = `<TEMPLATE の親>/<TEMPLATE の dir 名>-<ROOT の dir 名>`（host の既存の名付けの規則を器の 1 関数にする・repo 名は path の最後の要素）。在れば skip。
2. **host の面の継承**: `<TEMPLATE>/host.toml` の `[[plugin]]` `[[launch-arg]]` `[[account]]` `[[vessel]]` `[[tick]]`（seat-heartbeat.md §5・雛形に在れば）の行をそのまま写す（`[[account-group]]` は写さない＝群は 3.2 の `--group` だけが足す・A1「使う」の裁定は command の引数で人が持つ）。新しい `host.toml` は既存の loader で検査してから rename で置く（`account add` の `stage_host` と同じ形）。在れば skip（既存の面は 1 字も変えない）。
3. **口座の配線**: 雛形の `accounts/<label>`（`[[account]]` の label ごと）が symlink ならその先へ、実 dir ならその dir へ、新しい置き場の `accounts/<label>` を symlink で結ぶ（credential は読まず写さない・FR58 の柵の内側）。雛形に dir が無い label は `failed:no-source` で名指し、続きの段は止めない。在れば skip。
4. **marker と設定**: `vessel init --state-dir <新しい置き場> ROOT` と同じ 1 本（local 設定 → marker）。既に `ByMe` なら skip、`ByOther` なら failed（何も書かない）。
5. **宣言の雛形**: `ROOT/.vessel.toml` が無ければ書く。`ROOT/Cargo.toml` が在る周は cargo の形（`allowed-commands = ["cargo", "git"]`・`common-verify` に nextest と clippy の 2 行・`entrance-flip = "unmeasured"`）、無い周は git の形（`allowed-commands = ["git"]`・`common-verify = ["git diff --quiet"]`・`entrance-flip = "unmeasured"`）。`requirements` は書かない（既定を使う）。値は上限（rules 行 `runner.allowed_commands`）の内側。在れば skip。
6. **群**（`--group <名>` の周だけ）: 雛形の `[[account-group]]` に `<名>` が無ければ failed。在れば、その群を宣言している **雛形と同じ親の下の全置き場の `host.toml`** の `anchors` に ROOT を足し（既に在れば skip）、新しい置き場の `host.toml` にも同じ行を写す。書く前に各面を loader で検査し、1 つでも落ちれば 1 面も書かない（全部か皆無か）。
7. **commit**: `.vessel` と `.vessel.toml` のうち本便が書いた file だけを `git add` して 1 commit（message は「chore(<NAME>): vessel marker and declaration」・書いた file が 0 なら skip・index に他の変更が在っても触らない＝`git add <file>` と `git commit -- <file>`）。
8. **tmux と席**: 行 c（§5）。

## 5. tmux の session と引数の無い席の起動（行 c）

- `init` の 8 段目: tmux の session `<ROOT の dir 名>` が無ければ `new-session -d -s <名> -n orchestrator -c ROOT` で作る（在れば skip・socket は既定）。次に行 c の `seat launch` の既定形を 1 回撃つ（登録 row を書き席を起こす・FR59）。tmux が撃てない周は `failed:tmux` で名指し、`next=` に `seat launch` を置く。
- **`seat launch` の既定**（引数の無い形）: `--state-dir` は cwd の repo の local 設定（`vessel` の読み）、`--role` は `orchestrator`、`--target` は `<repo の dir 名>:orchestrator`、`--account` は置き場が群に属せば群の今の口座（FR59 のまま）、属さなければ選定（FR36）。明示の引数は既定に勝つ。既定を解けない周は今の `defaults-unresolved` の断り（`missing=` に載せる）。長い形の判定・断りの字面は 1 字も変えない。

## 6. doctor の欠落の名指し（行 d）

- doctor に `init=` の 1 行を足す（`host-template=` の直後）: 置き場を渡した周は `init=<ok|missing:<項目,…>> next=<次の 1 手>`。項目は宣言順に `marker`（cwd の repo の `.vessel` が `ByMe` でない）・`declaration`（`.vessel.toml` が HEAD に無い）・`host-face`（`host.toml` が `Absent` / `Unreadable`）・`accounts`（`[[account]]` の label で `accounts/<label>` が無い）・`session`（登録 row の target の session が無い）・`registration`（orchestrator の登録 row が無い）。`next=` は最初の欠落を埋める 1 手（`init` か `host init` か `seat launch`）で、欠落 0 なら `next=-`。
- 「黙って 0 byte」は残す（hook の極性は vessel-hook.md §2 のまま）。名指すのは doctor の役。

## 7. 口座 × anchor の trust を器が起動の前に書く — 席の起動の 1 本が、選んだ口座の設定 dir の `.claude.json` に `projects[<anchor>].hasTrustDialogAccepted = true` を書いてから起動行を注入する（契約表の行 e・[ADR-0065](../../design-intent/decisions/ADR-0065-the-vessel-writes-the-trust-flag-before-launching-a-seat.html)・`s2-07l.609` / `s2-07l.604`）

やさしく言うと: 席が別の口座へ移るとき、その口座がその repo を一度も「信じる」と答えていないと、Claude Code は起動直後に trust の dialog（既定は No, exit）を出して席が立たない。群の自動の移動（account-lifecycle.md §20〜§22・seat-heartbeat.md §4）はここで人の手を待つ（2026-09-25 の本番の移動でも持ち主が手で承認した）。器は起動の直前に、その口座の設定 file の該当の印を true に置いてから起動行を送る。印を置けない周も起動は止めない（言葉で残す）。

- 出所: memo `s2-07l.604` の実地試験（穴 (a)）・台帳 `s2-07l.609` の notes の裁定（user 2026-09-24T22:56Z「推奨で良い」= 依存を足さず、既存の入れ子 JSON の読み手に書き手を足す形。2026-09-24T13:38Z の serde_json の受諾は前提の誤りで取り下げ）・2026-09-25T01:00Z の s3:design の移動（tick の `launched=launch-unconfirmed`・持ち主の手で trust を承認・台帳 `s2-07l.617` の notes）。
- 現物（verified・main 80270bd）:
  - 読み手: `crates/scribe2/src/fleet/json_tree.rs` の `parse` / `Tree`（RFC 8259 の 6 形・数は 10 進の字面のまま・重複 key を拒む）と `render`（木を JSON に戻す）。`crates/scribe2/src/account/mod.rs` の `read_tree` / `flag_at` が doctor の `trust=<accepted|missing|unreadable>` の行で `projects[<anchor>].hasTrustDialogAccepted` を読む（読むだけ・`probe_account`）。
  - 無いもの: 木の path に真偽を置く 1 関数と、置いた木を同じ file へ書き戻す 1 関数。
  - 席の起動の 1 本は `crates/scribe2/src/seat/cycle/launch.rs` の `launch`（model → 役割の既定 → 口座の選定 `pick_account` → 起動行の導出 → `prepare` が登録 row を書く → `boot` が注入）。呼び手は 3 つ: `crates/scribe2/src/seat/cli.rs`（長い形・短い形）・`crates/scribe2/src/pipe/dispatch/group.rs` の `relaunch`（群の起こし直し）・`crates/scribe2/src/seat/tick.rs`（tick の移動の周・seat-heartbeat.md §4）。席の立て直しの経路は ADR-0045 §2 (2) で消えており（`crates/scribe2/src/seat/cycle/relaunch.rs` が持つのは `boot` と初回の選定 `choose` だけ）、席の起こし直しは全部この 1 本を通る。`launch` は `prepare` の後で 2 つに分かれる: 呼び手の pane が target と同じ周（`replace_own`・約束 7）は `boot` を通らず自分の process を起動行へ exec で置き換えて返らない（stdout の 1 行も `Launched` も無い・記録は `record_launch` が exec の前に inject.jsonl へ 1 行）・それ以外は `boot` が注入する。結果は `Launched`（`Done(label, settled)` / `None` / `Refused` / `Failed`）。
  - 口座の dir は `<state_dir>/accounts/<label>`（実 dir か symlink・[account-autonomy.md](./account-autonomy.md) §5）。
- 形（1 つずつ歯が測る・行 e の done と 1:1）:
  1. **書く場所と順**: `launch` は `prepare` が通った後・置き換え（`replace_with`）と `boot` の分岐の**前**に 1 回だけ、選んだ口座の dir の `.claude.json` について `projects[<anchor の絶対 path>].hasTrustDialogAccepted` を `true` に置く（置き換えの周も注入の周も同じ 1 回を通る）。呼び手 3 つは変えない（1 本の内側なので群の起こし直しも tick の移動も同じ 1 回を通る）。
  2. **書き方**: file を同じ読み手で読み → 木の path に真偽を置く（途中の object が無ければ作る・兄弟の key と並びと値と数の字面は変えない・書式は読み手の `render` の形＝2 空白の入れ子で、末尾の改行は元の file に在れば保つ）→ `render` の本文を同じ dir の一時 file に書き → 一時 file を同じ読み手で読み直して印が `true` であることを確かめ → `rename` で置き換える。file が無い周は `projects` だけを持つ最小の木を同じ手で作る。既に `true` の周は 1 byte も書かない（mtime も動かない）。
  3. **言葉（閉じた列）**: `written`（置いた）／`created`（file を作って置いた）／`accepted`（既に true・書かない）／`unreadable`（file は在るが JSON でない・途中が object でない・末端が真偽でない＝書かない）／`unwritable`（一時 file の書き・読み直しの不一致・rename のどれかが失敗＝置き換えない）。
  4. **起動は止めない**: どの言葉でも起動行は送る（trust は起動の前提でなく穴埋め・`Refused` にしない）。言葉は起動の記録（`record_launch` が inject.jsonl に書く行の `what`）の末尾に `trust=<語>` として残す（置き換えの周も注入の周も同じ）。注入の周はさらに `Launched::Done` の 3 つ目の値として返し、`seat launch` の stdout の 1 行の末尾に `trust=<語>` を添え、tick は `launched=` の後ろに `trust=<語>` を足す。dispatch の起こし直しと tick の移動は言葉を判定に使わない。
  5. **doctor は変えない**（`trust=` の行は読むだけ・書いた後の周は `accepted` と読める＝それが確認）。`init`（§4）と `account add` も書かない（書くのは起動の 1 本だけ・書く相手が「今起こす口座 × 今の anchor」に閉じる）。
  6. **lock は持たない**: 同じ設定 dir を使って走る Claude Code とは lock を共有しない（読み → 一時 file → rename の間に Claude Code が同じ file を書いた周はどちらかが負ける）。印が消えた周は doctor の `trust=missing` が名指し、次の起動が置き直す（人の手で置いた他の key を器が消す方向には負けない: 器の書きは読んだ木 + 印 1 つ・C3.3 の柵の内側）。
- 触らない: `.credentials.json` と `settings.json`（書かない・ADR-0017 の fence のまま）・pane の字面で dialog に答える形（C3.3・§21 の却下のまま）・`Tree` の形と `parse` の判定・`prepare` と登録 row・`Launched` の他の variant。
- 却下: serde_json を境界 crate に足す（NFR3 = 実行時の直接依存 0 本・読み手も書き手も既に在る・C17.2）／`.claude.json` を丸ごと書き直す（Claude Code が持つ他の key と並びを壊す・読み直しで測れない）／dialog を pane の字面で読んで Enter を送る（C3.3・§21 の却下）／`init` の段で全口座 × anchor を先に trust する（信じていない folder を信じた印を全部の口座に置く・書く相手は起こす 1 組に閉じる）／trust を起動の前提にして dialog が出うる周を断る（人の手が要る周を器が増やす）／設定 dir の `.claude.json` を host の面の宣言に写す（Claude Code の私有形式の二重化）。
- 歯:
  - `crates/scribe2/src/fleet/json_tree.rs` の in-file の歯（`json_tree_set_` 接頭辞）: 入れ子の path に真偽を置くと途中の object が作られ兄弟の key と並びと数の字面（30 桁）が保たれ `render` → `parse` で同じ木に戻る／既に `true` の周は木が変わらない／途中が object でない周は Err（base では関数が無い ＝ RED）。
  - `crates/scribe2-boundary/tests/e2e/seat/launch.rs`（`seat_launch_trust_` 接頭辞・偽 tmux と偽 claude の fixture・口座の dir を tmp に作る・fixture の file は実物の `.claude.json` の書式〔2 空白・末尾改行あり〕で作る）: `.claude.json` に他の key と別 anchor の項目が在り当該 anchor が無い周の起動 → 起動行は送られ file は当該 anchor の印だけ増えて他の key・並び・値は `parse` で同じ木・末尾改行も同じ・stdout の末尾 `trust=written`・inject の記録の `what` の末尾も `trust=written`（base では file が変わらない ＝ RED）／同じ窓へ置き換える周（約束 7 の fixture・exec は偽の起動行）→ exec の前に印が置かれ inject の記録の `what` の末尾に `trust=<語>`（base では記録に `trust=` が無い ＝ RED）／既に true → 1 byte も変わらず（mtime 同じ）`trust=accepted`／file が無い → `projects` だけの最小の file が作られ `trust=created`／JSON でない file → 変わらず `trust=unreadable` で起動は送られる／dir が読み取り専用 → 変わらず `trust=unwritable` で起動は送られる。
  - `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（`pipe_dispatch_group_trust_` 接頭辞・§20 の fixture）: 群の起こし直しの周に移り先の口座の `.claude.json` へ群の anchor の印が置かれてから起動行が送られる（送りの記録の ts が file の書き換えより後・base では file が変わらない ＝ RED）。
- 後続: README の「新しい repo を器に載せる」（§8）に trust の 1 文（初回の席は器が印を置くので dialog は出ない・出たら doctor の `trust=` を見る）。

## 8. 人間向けの説明

README の先頭に「新しい repo を器に載せる」の節を置く: 打つのは `host init <既存の置き場>`（host に 1 回）・`init`（repo で 1 回）・`doctor`（困ったら）の 3 つ。長さはこの 3 つの説明で足りる分だけ（散文の runbook を人に辿らせない・N2 の趣旨）。行 d の land 後に orchestrator の docs PR で書く（歯は持たない・契約表の行にしない）。

## 9. 極性と失敗の型（[polarity.md](./polarity.md)）

- `host init` / `init` は fail-closed: 断る周は 1 byte も書かない（段の途中の failed は、その段だけ書かず続きの段へ進み、`next=` で名指す）。
- 群の面の書き換え（§4 の 6）は全部か皆無か（複数の置き場の宣言が食い違う周を作らない）。
- 子 process は全部 `Invocation` で記述し（git・tmux・自分自身の `seat launch`）、core は撃たない（ADR-0062）。

## 10. 歯（置き場は既存の file・接頭辞ごとに 1 file・base で RED）

- `crates/scribe2-boundary/tests/e2e/main.rs`（`host_init_` / `init_repo_` / `doctor_init_` 接頭辞・偽 git は不要＝toy の repo を作る・tmux は偽 socket の fixture）: `host init` が global 設定（`GIT_CONFIG_GLOBAL` を toy の file に向けた env で撃つ）を書き doctor が `host-template=` を出す ／ `init` が 7 段を順に通し、置き場・面・symlink・marker・宣言・commit が在り、2 度目は全段 skip ／ `Cargo.toml` の有無で宣言の形が変わる ／ `--group` が雛形と同じ親の下の 2 面の anchors に足し、1 面が壊れていれば 0 面 ／ `doctor` が欠落を宣言順に名指し `next=` を 1 つ出す。
- `crates/scribe2-boundary/tests/e2e/seat/launch.rs`（`seat_launch_default_` 接頭辞）: 引数の無い `seat launch` が cwd の repo の設定と repo 名から target を組んで登録 row を書く ／ 明示の引数が勝つ ／ 解けない周は `defaults-unresolved` の字面が不変。

## 11. 憲法・制約との整合

- C3（描画を読まない）: 読むのは file と git / tmux の rc だけ。
- C7 / A1: 口座を使う裁定は `--group` を打つ人が持つ（器は勝手に群へ入れない）。
- N3（host 固有の値で分岐しない）: 置き場の在り処は global 設定の pointer 1 つ・名付けは雛形から導く。env は読まない。
- FR58: credential を読まず書かない（symlink を結ぶだけ）。
- ADR-0062: 子 process は `Invocation`。

## 12. 却下案

- env（`SCRIBE2_HOME` 等）や固定 path（XDG）で置き場を知る: env-reads の門と N3 に反する。
- 雛形を host の根（`<親>/<NAME>-host`）の `host.toml` に置く: host の面は置き場ごとと決めた ADR-0049 / ADR-0026 の外形を変える。
- credential の dir を copy する: FR58 の柵の外・可逆でない。
- `init` が対話で聞く: 席から撃てない・1 発でない。
- `init` が群へ自動で入れる: A1「使う」を器が決める形になる。
- 人間向けの説明を契約表の行にする: 散文の歯は字面 pin になる。

## 13. 後続

- 移り先の口座の trust は §7 / 行 e（ADR-0065）が持つ（account-lifecycle.md §22 の後続の行き先）。
- 席の登録 row の退役の kind（`s2-07l.609` の notes (a)）と外部 API の鍵の欄（同 (c)）は本設計の外。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "host init — 既存の置き場を雛形として git の global 設定 <NAME>.template に絶対 path で書き（unchanged / written の 1 行）、doctor が host-template= の 1 行を出す（§3）"
req = ["FR61", "FR58"]
section = "3"
write-set = ["crates/scribe2/src/init.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/seat/register.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_init_"]
size = "S"
done = "(1) host init <TEMPLATE> は TEMPLATE が dir で host.toml が Absent か Present の周だけ git の global 設定 <NAME>.template に絶対 path を書き、同じ値なら書かず unchanged、dir が無い・Unreadable・引数欠けの周は 1 byte も書かず断る (2) 出力は host: init template=<path> <written|unchanged> の 1 行 (3) doctor は骨格の 2 行の直後に host-template=<path|absent|unreadable> の 1 行を置き場を渡さない周にも出す (4) env と HOME を読まず、git の呼び出しは Invocation で記述する (5) doctor の外形の insta snapshot 3 本を新しい形へ更新し write-set の外の .snap は触らない 歯: host_init_ の歯が GIT_CONFIG_GLOBAL を toy の file に向けて written / unchanged / 断り 3 形と doctor の行を測る（base では init の verb が無い ＝ RED）"

[[contract]]
id = "b"
title = "init [ROOT] — 雛形から新しい置き場を作り（名は <雛形>-<repo 名>）、host の面の 5 表を写し、口座の dir を symlink で結び、marker と local 設定と宣言の雛形（Cargo.toml の有無で形を選ぶ）を置き、--group の周は同じ親の下の全面の anchors に足し（全部か皆無か）、書いた file だけを 1 commit にする（§4）"
req = ["FR58", "FR61"]
section = "4"
write-set = ["crates/scribe2/src/init.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail init_repo_"]
size = "L"
growth = ["crates/scribe2/src/hook/vessel.rs:60", "crates/scribe2/src/rules/manifest.rs:40", "crates/scribe2/src/account/mod.rs:60", "crates/scribe2/src/pipe/declaration.rs:20", "crates/scribe2-boundary/src/main.rs:60"]
depends = ["a"]
done = "(1) ROOT が git の repo でない・host-template が無い周は 1 段目の前に断り何も書かない (2) 置き場は <雛形の親>/<雛形の dir 名>-<ROOT の dir 名> で在れば skip (3) host の面は雛形の [[plugin]] [[launch-arg]] [[account]] [[vessel]] [[tick]]（在れば）を写し [[account-group]] は写さず、loader で検査してから rename で置き、在れば 1 字も変えない (4) accounts/<label> は雛形の symlink の先か実 dir へ symlink で結び credential を読まず写さず、雛形に無い label は failed:no-source で名指して続きの段を止めない (5) marker と local 設定は vessel init と同じ 1 本で ByMe は skip・ByOther は failed (6) 宣言は Cargo.toml が在れば cargo の形（allowed-commands cargo と git・common-verify に nextest と clippy・entrance-flip unmeasured）、無ければ git の形（allowed-commands git・common-verify git diff --quiet・entrance-flip unmeasured）で、在れば skip (7) --group は雛形に無い群を failed、在れば同じ親の下で群を宣言する全面の anchors に ROOT を足し新しい面にも写し、1 面でも検査に落ちれば 0 面 (8) 本便が書いた .vessel と .vessel.toml だけを git add と git commit -- で 1 commit にし、0 file なら skip (9) 出力は段ごとに init: <段> <ok|skip|failed:<理由>> の 1 行と最後の next= 1 つで、2 度目は全段 skip 歯: init_repo_ の歯が 7 段の生成物と 2 度目の skip と Cargo.toml の有無の 2 形と --group の 2 面と 0 面と失敗の段の名指しを測る（base では init の verb が無い ＝ RED）"

[[contract]]
id = "c"
title = "tmux の session と引数の無い席の起動 — init の 8 段目が session <repo 名> を new-session -d で作り seat launch の既定形を 1 回撃ち、seat launch は引数無しで state dir を local 設定・role を orchestrator・target を <repo 名>:orchestrator・口座を群の今の口座か選定から解く（§5）"
req = ["FR59", "FR36"]
section = "5"
write-set = ["crates/scribe2/src/init.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_default_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail init_seat_"]
size = "M"
depends = ["b"]
done = "(1) 引数の無い seat launch は cwd の repo の local 設定から置き場を、orchestrator を役割に、<repo の dir 名>:orchestrator を target に、群の置き場は群の今の口座を、それ以外は選定を使って登録 row を書き席を起こし、明示の引数は既定に勝ち、解けない周は defaults-unresolved の断りで missing= に載せ、長い形の判定と断りの字面は 1 字も変わらない (2) init の 8 段目は session <ROOT の dir 名> が無ければ new-session -d -s <名> -n orchestrator -c ROOT で作り在れば skip、続けて (1) の既定形を Invocation で 1 回撃ち、tmux が撃てない周は failed:tmux で next= に seat launch を置く (3) 子 process は全部 Invocation で記述する 歯: seat_launch_default_ の歯が既定の 3 値と明示の勝ちと断りの不変を測り、init_seat_ の歯が偽 tmux の socket で new-session の 1 回と skip と登録 row を測る（base では引数無しの seat launch が usage で断る ＝ RED）"

[[contract]]
id = "d"
title = "doctor の init= 行 — marker / declaration / host-face / accounts / session / registration の欠落を宣言順に名指し next= に最初の欠落を埋める 1 手を置く（§6）"
req = ["FR61"]
section = "6"
write-set = ["crates/scribe2/src/init.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/seat/register.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail doctor_init_"]
size = "S"
depends = ["b"]
done = "(1) 置き場を渡した doctor は host-template= の直後に init=<ok|missing:<項目,…>> next=<1 手> の 1 行を出し、項目は marker・declaration・host-face・accounts・session・registration の宣言順で欠けたものだけを並べ、欠落 0 は init=ok next=- (2) next= は最初の欠落を埋める 1 手（host init・init・seat launch のどれか 1 語）で候補の一覧を出さない (3) 他の doctor の行と hook の 0 byte の極性は 1 字も変わらない 歯: doctor_init_ の歯が 6 項目それぞれ 1 つだけ欠けた toy と欠落 0 の toy で行と next= を測る（base では init= の行が無い ＝ RED）"

[[contract]]
id = "e"
title = "口座 × anchor の trust を器が起動の前に書く — 席の起動の 1 本が prepare の後・置き換えと boot の分岐の前に選んだ口座の .claude.json の projects[<anchor>].hasTrustDialogAccepted を true に置き（読み手に書き手を足す・一時 file → 読み直し → rename）、言葉 written / created / accepted / unreadable / unwritable を Launched::Done に返して stdout の末尾に添え、起動は止めない（§7・ADR-0065・s2-07l.609）"
req = ["FR59", "FR38", "NFR3"]
section = "7"
write-set = ["crates/scribe2/src/fleet/json_tree.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/host-init.md", "docs/design/account-autonomy.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail json_tree_set_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_trust_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_trust_"]
size = "M"
growth = ["crates/scribe2/src/fleet/json_tree.rs:80", "crates/scribe2/src/account/mod.rs:80", "crates/scribe2/src/seat/cycle/launch.rs:40", "crates/scribe2/src/seat/cli.rs:10", "crates/scribe2/src/pipe/dispatch/group.rs:10", "crates/scribe2/src/seat/tick.rs:10"]
done = "(1) launch は prepare が通った後・置き換え（replace_with）と boot の分岐の前に 1 回だけ、選んだ口座の dir の .claude.json の projects[<anchor の絶対 path>].hasTrustDialogAccepted を true に置き（置き換えの周も注入の周も同じ 1 回）、呼び手 3 つ（cli の長い形と短い形・dispatch の群の起こし直し・tick の移動）は変えない (2) 書き方は同じ読み手で読み → 木の path に真偽を置く（途中の object が無ければ作り兄弟の key と並びと値と数の字面は変えず書式は render の 2 空白の形で末尾の改行は元の file に在れば保つ）→ render の本文を同じ dir の一時 file に書き → 一時 file を読み直して true を確かめ → rename で置き換え、file が無い周は projects だけの最小の木を同じ手で作り、既に true の周は 1 byte も書かない (3) 言葉は閉じた列 written / created / accepted / unreadable / unwritable (4) どの言葉でも起動行は送り Refused にせず、言葉は起動の記録（record_launch の inject.jsonl の行の what）の末尾に trust=<語> として置き換えの周も注入の周も残し、注入の周はさらに Launched::Done の 3 つ目の値で返して seat launch の stdout の 1 行の末尾に trust=<語> を添え、tick は launched= の後ろに trust=<語> を足し、dispatch の起こし直しと tick は言葉を判定に使わない (5) doctor の trust= の行・init・account add・.credentials.json・settings.json・Tree の形と parse・prepare と登録 row は変えない (6) 同じ設定 dir を使う Claude Code と lock は共有せず、印が消えた周は doctor の trust=missing が名指して次の起動が置き直す 歯: json_tree_set_ の歯が入れ子の path への真偽の設置で途中の object の生成・兄弟の key と並びと数の字面の保存・render → parse の同値・既に true なら不変・途中が object でなければ Err を測り、seat_launch_trust_ の歯が他の key と別 anchor の項目を持つ file で当該の印だけ増えて他の key・並び・値は parse で同じ木かつ末尾改行も同じで stdout と inject の記録の what の末尾が trust=written・同じ窓へ置き換える周は exec の前に印が置かれて記録の what の末尾に trust=<語>・既に true で 1 byte も変わらず accepted・file 無しで最小の file と created・JSON でない file で unreadable かつ起動は送られる・読み取り専用 dir で unwritable かつ起動は送られることを測り、pipe_dispatch_group_trust_ の歯が群の起こし直しで移り先の口座の file に群の anchor の印が置かれてから起動行が送られることを測る"
<!-- contracts:end -->
