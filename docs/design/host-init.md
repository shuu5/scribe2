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

## 7. 口座 × anchor の trust を器が先に埋める（後続の ADR・持ち主の再裁定待ち・契約表の行はまだ無い）

- 出所: memo `s2-07l.604` の実地試験。移り先の口座がその anchor を一度も trust していないと、席の起動で Claude Code の trust dialog（既定 `No, exit`）が出て席が立たず `launch-unconfirmed` の保留になる。公式 doc が案内する唯一の口は口座の設定 dir の `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` を `true` にすること。
- 現物: 器は入れ子の JSON の**読み手**を既に持つ（`crates/scribe2/src/account/mod.rs` の `read_tree` / `Tree` / `flag_at`・doctor の `trust=` の行が読む）。無いのは**書き手**（木を JSON に戻す 1 関数）だけ。account-autonomy.md §10 は「器が trust の印を書く」を却下している（当時は trust を user の宣言と読んだ）。
- 裁定の経緯: 2026-09-24T13:38Z の裁定（逐語は台帳 `s2-07l.609` の notes）は「入れ子の JSON を書く道具が無い」という誤った前提で上げた推奨（境界 crate に serde_json）への受諾だった。serde_json は SRS の NFR3（実行時の直接依存 0 本・std だけで build）と食い違い、NFR3 の改稿（/folio-architect）が要る。代替は既存の読み手に書き手を足す形（依存 0・NFR3 のまま・書き戻しは一時 file → rename・rename の前に書いた本文を同じ読み手で読み直して flag を確かめる）。どちらを採るかは再裁定（1 論点）で決め、決まった側を後続の ADR に書き、契約表に行を足す（席の起動・群の起こし直し・`init` の段 3 の直後・doctor の `init=` の `trust` 項目）。それまで §3〜§6 の行は trust に触れない（`init` の後の初回の席は trust dialog で止まりうる＝doctor が名指す・行 d の項目には入れない）。

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

- 移り先の口座の trust は §7（後続の ADR・再裁定待ち）が持つ（account-lifecycle.md §22 の後続の行き先）。
- 席の登録 row の退役の kind（`s2-07l.609` の notes (a)）と外部 API の鍵の欄（同 (c)）は本設計の外。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "host init — 既存の置き場を雛形として git の global 設定 <NAME>.template に絶対 path で書き（unchanged / written の 1 行）、doctor が host-template= の 1 行を出す（§3）"
req = ["FR61", "FR58"]
section = "3"
write-set = ["+crates/scribe2/src/init.rs", "crates/scribe2/src/lib.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_init_"]
size = "S"
done = "(1) host init <TEMPLATE> は TEMPLATE が dir で host.toml が Absent か Present の周だけ git の global 設定 <NAME>.template に絶対 path を書き、同じ値なら書かず unchanged、dir が無い・Unreadable・引数欠けの周は 1 byte も書かず断る (2) 出力は host: init template=<path> <written|unchanged> の 1 行 (3) doctor は骨格の 2 行の直後に host-template=<path|absent|unreadable> の 1 行を置き場を渡さない周にも出す (4) env と HOME を読まず、git の呼び出しは Invocation で記述する (5) doctor の外形の insta snapshot 3 本を新しい形へ更新し write-set の外の .snap は触らない 歯: host_init_ の歯が GIT_CONFIG_GLOBAL を toy の file に向けて written / unchanged / 断り 3 形と doctor の行を測る（base では init の verb が無い ＝ RED）"

[[contract]]
id = "b"
title = "init [ROOT] — 雛形から新しい置き場を作り（名は <雛形>-<repo 名>）、host の面の 5 表を写し、口座の dir を symlink で結び、marker と local 設定と宣言の雛形（Cargo.toml の有無で形を選ぶ）を置き、--group の周は同じ親の下の全面の anchors に足し（全部か皆無か）、書いた file だけを 1 commit にする（§4）"
req = ["FR58", "FR61"]
section = "4"
write-set = ["+crates/scribe2/src/init.rs", "crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/host-init.md"]
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
write-set = ["+crates/scribe2/src/init.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_default_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail init_seat_"]
size = "M"
depends = ["b"]
done = "(1) 引数の無い seat launch は cwd の repo の local 設定から置き場を、orchestrator を役割に、<repo の dir 名>:orchestrator を target に、群の置き場は群の今の口座を、それ以外は選定を使って登録 row を書き席を起こし、明示の引数は既定に勝ち、解けない周は defaults-unresolved の断りで missing= に載せ、長い形の判定と断りの字面は 1 字も変わらない (2) init の 8 段目は session <ROOT の dir 名> が無ければ new-session -d -s <名> -n orchestrator -c ROOT で作り在れば skip、続けて (1) の既定形を Invocation で 1 回撃ち、tmux が撃てない周は failed:tmux で next= に seat launch を置く (3) 子 process は全部 Invocation で記述する 歯: seat_launch_default_ の歯が既定の 3 値と明示の勝ちと断りの不変を測り、init_seat_ の歯が偽 tmux の socket で new-session の 1 回と skip と登録 row を測る（base では引数無しの seat launch が usage で断る ＝ RED）"

[[contract]]
id = "d"
title = "doctor の init= 行 — marker / declaration / host-face / accounts / session / registration の欠落を宣言順に名指し next= に最初の欠落を埋める 1 手を置く（§6）"
req = ["FR61"]
section = "6"
write-set = ["+crates/scribe2/src/init.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_form_doctor_external_form.snap", "crates/scribe2-boundary/src/snapshots/scribe2__tests__ledger_lint_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/host-init.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail doctor_init_"]
size = "S"
depends = ["b"]
done = "(1) 置き場を渡した doctor は host-template= の直後に init=<ok|missing:<項目,…>> next=<1 手> の 1 行を出し、項目は marker・declaration・host-face・accounts・session・registration の宣言順で欠けたものだけを並べ、欠落 0 は init=ok next=- (2) next= は最初の欠落を埋める 1 手（host init・init・seat launch のどれか 1 語）で候補の一覧を出さない (3) 他の doctor の行と hook の 0 byte の極性は 1 字も変わらない 歯: doctor_init_ の歯が 6 項目それぞれ 1 つだけ欠けた toy と欠落 0 の toy で行と next= を測る（base では init= の行が無い ＝ RED）"

<!-- contracts:end -->
