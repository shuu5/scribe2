# 設計: 口座の生涯 — 口座の宣言は host の manifest が持ち、登録・一覧・退役・席の起動を器の口が担う（外の wrapper に頼らない）

- 決定: [ADR-0026](../../design-intent/decisions/ADR-0026-account-lifecycle-host-manifest-and-seat-launch.html)（§2.1 host の manifest / §2.2 口座の口 / §2.3 席の起動 / §2.4 supersede）・[ADR-0028](../../design-intent/decisions/ADR-0028-consumer-sync-is-measured-and-updated-by-the-vessel.html) §2.5（役割なしの起動行・本 doc §4.5）
- 要件: SRS FR33（口座残量の計測・credential の場所）/ FR36（口座の選定）/ FR38（席の退避と立て直し）/ FR40（席の登録）/ FR59（役割付きの起動）/ [FR60](../../design-intent/spec/srs.html#FR60)（役割なしの起動・§4.5）/ AC13（別口座での立て直し）/ [AC30](../../design-intent/spec/srs.html#AC30) が正本。口座の登録・退役そのものを名指す要件は SRS の次の改訂で足す（材料は planner の置き場・改訂は user の folio-architect）。改訂までは (a)〜(c) の契約は上の id を指す。
- 土台: [fleet-usage.md](./fleet-usage.md) §2（口座の列挙と credential の場所・本設計が §2 を改める）・[account-autonomy.md](./account-autonomy.md) §3（選定）/ §5（立て直し・雛形の穴）/ §11（後続「席の起動と初回の口座選択」= 本設計）・[seat-roles.md](./seat-roles.md) §2（登録 row）・[rules-manifest.md](./rules-manifest.md) §5（実行時に読む manifest の場所・本設計が host の面を足す）
- 語彙: `design-intent/vocabulary.yaml`（host の manifest・口座の登録・席の起動・起動行の導出）

## 1. 何を解くか

記録時点の口座の「生涯」（登録 → 準備 → 計測 → 選定 → 席の起動 → 退役）のうち、器が持つのは真ん中の計測（FR33）と選定（FR36）と立て直し（FR38）だけで、両端は器の外に在る（2026-09-14 実測）:

- **登録**: 口座の設定 dir を作る・login させる・settings の前提（agent view off）を仕込む・trust を通す・`[[account]]` 行を足す・`<state_dir>/accounts/<label>` を張る、のすべてが user と planner の手作業（7 口座の link を planner が手で差し替えた・[account-autonomy.md](./account-autonomy.md) §5 (1)）。
- **宣言の置き場**: `[[account]]` 行は tracked の manifest（公開面）と host の私用 manifest（tracked の rules 部の**写し** + 口座節）の 2 か所に割れ、rules 行が動くたびに写しを手で再生成している（台帳 `s2-07l.220`・C1 / C10.2 の外に第 2 の写しが在る形）。
- **席の起動**: user が席を初めて起こす口が器に無く、別 repo の口座切替 wrapper と前の版の launcher（cgroup の防壁・plugin の自動検出）に乗っている。器が起こす claude（runner / lens・立て直し）は既にその経路を使わないので、残る依存は user の手打ちだけ。SRS の scope はこれを「席の起動（v3）」として扱わない側に置いている。
- **一覧・退役**: 口座の一覧（前提の充足・残量）と退役（可逆 move・N1.2）の口が無い。doctor の口座行（`s2-07l.233`）が一覧の半分を持つ。

本設計は、宣言の置き場を 1 つにし（host の manifest）、口座の口 4 つ（add / ls / retire / restore）と席の起動の口 1 つ（`seat launch`）を器に足して、外の wrapper への依存を切る。運用の散文（「link を張る」「wrapper で起こす」）を器の口に置き換える形で、規則の値は増やさない（rules 行を足さない・C5 非該当）。

## 2. host の manifest（ADR-0026 §2.1・台帳 `s2-07l.220` の裁定）

- **file**: `<state_dir>/host.toml`。TOML subset（[rules-manifest.md](./rules-manifest.md) §4 と同じ reader・`schema = 1`）。持てる表は array-of-tables 3 種だけ（field は各 1 つ・型は文字列）:
  - `[[account]] label = "<label>"` — 口座の宣言（[fleet-usage.md](./fleet-usage.md) §2 と同じ意味・label は不透明）。
  - `[[plugin]] dir = "<dir>"` — 席に積む plugin dir（host 固有の場所・絶対 path はこの file にだけ書く）。
  - `[[launch-arg]] value = "<arg>"` — 席の起動行に足す引数（例: permission の指定）。順序 = 宣言順。
- **読み手**: manifest の loader 1 本（`rules/manifest.rs`）が tracked の manifest（埋め込み・`--rules PATH` の override は従来どおり rules の検査用）と host の manifest を**同じ拒否形**で読む（未知 key・型違い・label の重複・schema の欠落は行番号付きで全件・rc 1）。両面に同じ label が在る周は重複として拒む。host の manifest が**無い**周は 0 口座・0 plugin・0 引数として続く（計測は「宣言なし」を出す・止めない）。**読めない**（在るが壊れている・権限）周は typed に断って止める（FailClosed・下の §7）。
- **場所の解決**: `--state-dir S` からだけ解く（env を読まない C2.2・[account-autonomy.md](./account-autonomy.md) の `accounts/<label>` と同じ根）。`--rules PATH` は rules 部の override のままで、host の面を差し替える引数は持たない（test は tmp の state dir に `host.toml` を置く）。
- **tracked の manifest**: `[[account]]` の表は文法として残す（ADR-0017 §2.3 の拡張は不変）が、本 repo の tracked の manifest からは口座行を**すべて外す**（公開面の情報が減る側・A1 非該当）。以後、口座行を tracked に足す変更は A1 の対話面で聞く（[fleet-usage.md](./fleet-usage.md) §2 の既存の規律）。
- **移行**: 私用 manifest（rules 部の写し + 口座節）は `host.toml`（口座節だけ）へ置き換え、旧 file は mv で退避（削除しない・N1）。tick の unit（[seat-autonomy.md](./seat-autonomy.md) §8）は `--rules` を渡していないので変更なし＝以後は `host.toml` の宣言を tick と `fleet usage` が同じ関数で読む（`s2-07l.224` の「別の宣言を読まない」を保つ）。
- **doctor**: `doctor --state-dir S` の行に `host-manifest=<present|absent|unreadable>` を 1 行足す（読むだけ・判定しない・C10.2）。

## 3. 口座の口（ADR-0026 §2.2・subcommand `<NAME> account …`・`--state-dir S` 必須）

- **`account add <label> [--anchor DIR] [--target T]`**: (1) label を manifest の規則で検査し、宣言済み（退役中を含む）なら typed に断る（`exists`）。(2) `<state_dir>/accounts/<label>/` を dir として作る（既に dir か link が在れば `dir-exists`・中身は読まない）。(3) 直下に `settings.json` を書く（内容は器が起こす席の前提だけ = agent view を切る 1 項目・[account-autonomy.md](./account-autonomy.md) §5 (4) と、statusline を器の口で描く 1 項目 `statusLine = {type: command, command: "<NAME> seat statusline"}`・ADR-0029 §2.3・`command` の語は NAME 定数から導き `refreshInterval` は書かない。user が置いた既存 dir には書かない＝(2) で断るので到達しない。既存の口座 dir は `account statusline <label>` が `statusLine` の key 1 つだけを置換し、読めない設定は断って上書きしない）。(4) `host.toml` に `[[account]]` 行を 1 つ足す（読み → 検査 → 一時 file → rename・部分書きを残さない）。(5) login は **user の手番**（器は credential に触れない・代筆しない・ADR-0017 §2.3）: `--target T` が在れば、起動行 `CLAUDE_CONFIG_DIR=<dir> <claude>`（`--anchor` を cwd として・trust の dialog を同じ session で通すため）を T の shell へ §5 の門を通して注入し、無ければ同じ行を stdout に出す（`account: prepared <label> next=<行>`）。login の完了は器が待たない（`account ls` / doctor の `credential=` が示す）。
- **`account ls`**: doctor の口座行（`s2-07l.233` の形・label ごとに 1 行・`dir` / `credential` / `config` / `agentview` / `statusline` / `trust`・`statusline=<vessel|other|absent|unreadable>` は `statusLine.command` が器の行と一致するか（ADR-0029 §2.3・`absent` に潰さない）と**同じ関数**で行を作り、`retired=<yes|no>` と最新の実測行の要約（`five_hour=<pct|unmeasured> seven_day=<pct|unmeasured>`・event log を読むだけ・計測は撃たない）を足す。judgement を持たない（C10.2）。
- **`account retire <label>`**: (1) 宣言済みで未退役であること（`unknown` / `already-retired`）。(2) 登録 row（[seat-roles.md](./seat-roles.md) §2）のどれかがその label を持つ周は断る（`in-use`・席が使っている口座を外さない）。(3) `<state_dir>/accounts/<label>` を `<state_dir>/accounts/.retired/<label>.<ts>` へ **mv**（link は link のまま動かす・N1.2）。(4) fleet の event log に `AccountRetired { label }` を 1 件（`EventKind` の新 variant・宣言順の末尾・schema 1 のまま）。`host.toml` の行は消さない（宣言は残り、有効な口座の集合 = 宣言 − 退役 を replay が導く・C3「退役は DB」）。
- **`account restore <label>`**: 退役中であること（`not-retired`）・`.retired/<label>.<ts>` の最新を元の場所へ mv・`AccountRestored { label }` を 1 件。
- **有効な口座の集合**（1 関数・`fleet` の replay）: 宣言（tracked + host）から退役中（最後の `AccountRetired` の後に `AccountRestored` が無い label）を除いたもの。計測（`fleet usage`）・選定（`fleet select`）・tick の逼迫度・doctor・`account ls` の `retired=` はこの 1 関数を読む（退役中の口座は測らず選ばない）。

## 4. 席の起動（ADR-0026 §2.3・`<NAME> seat launch`）

- **口**: `seat launch --state-dir S --role <planner|admin> --target S:W [--account L] [--anchor DIR] [--model M] [--restore CMD]`。`--role` は閉じた `Role`（[seat-roles.md](./seat-roles.md) §2）の名で必須。`--anchor` の既定は cwd の repo root（`seat register` と同じ・env を読まない）。
- **口座**: `--account L` が在ればそれ（有効な口座でなければ `account-unknown` / `account-retired`）。無ければ [account-autonomy.md](./account-autonomy.md) §3 の **session 用の純関数**で選ぶ（model = `--model`・除外 = 他の席の登録 row が持つ口座・候補なしは `no-account` で断る・起こさない）。
- **起動行の導出**（pure な関数 1 本・`derive_launch`）: `CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} <claude> --plugin-dir <anchor> [--plugin-dir <host.toml の [[plugin]] dir>…] [<host.toml の [[launch-arg]] value>…]`。`<claude>` は語 `claude`（shell の PATH が解く・器は claude の場所を持たない・ADR-0011 と同じ）。器自身の plugin は anchor（main checkout・`plugin.json` を持つ）を積む（NAME 定数から導く・C2.2）。穴は `{account_dir}` 1 つのまま（`fill_launch` / `Holes` は不変）。雛形 file は書かない・読まない。
- **登録**: `SeatRegistered` を 1 件（鍵 = (role, anchor)・`target` = `--target`・`account` = 選んだ label・`launch` = 導出した行（穴を埋める前）・`model` = `--model`）。書き手は器の内部の同じ 1 関数（[seat-roles.md](./seat-roles.md) §2「書き手は 2 つ」に **launch を足して 3 つ**）。打刻の条件（SessionStart の stamp）は session の側の登録に課すもので、launch には掛からない。`sid` は **任意**（`Registration.sid: Option<String>`・launch が書く row は `None`・`seat register` の row は従来どおり有効な sid）。schema 1 のまま（値の省略・既存 row は読める）。
- **起動**: (1) target の tmux session が無ければ `session-missing` で断る（session は作らない）。window が無ければ `new-window -t <session> -n <window>` で作る。(2) [account-autonomy.md](./account-autonomy.md) §5 の **shell への注入の門**（前面 process が shell ∧ 可視域の最後の非空行が shell の prompt 末尾）を通して、穴を埋めた起動行を `send-keys` で注入する。門を通らない周は 1 key も送らない（`input-busy` / `input-unknown`・row は書き終えているので次の周に再実行できる）。(3) `--restore CMD` が在れば、立て直しと同じ `restore_when_ready`（SessionStart の打刻を待って注入）で復元の command を送る。**経路は立て直し（`relaunch_held`）と同じ 1 本**で、launch は「row を先に書く」「window を作れる」の 2 点だけが違う。
- **記録**: `inject.jsonl` に `kind=launch` を 1 行（`InjectKind` の新 variant・宣言順の末尾）。tick の立て直しの入口 (1)「直近の注入が externalize / exit」は launch を含めない（launch 直後の停止は「器が起こしたのでない停止」と同じ扱い・§11）。
- **`seat register` の扱い**: 残す（hook の走った session が自分で登録する口・AC13 の実演と既存 row の更新に使う）。launch で立てた席は登録済みなので `seat register` を撃つ必要が無い。
- **外の wrapper との関係**: 前の版の launcher が持つ cgroup の防壁は席には持ち込まない（席は shell の子で器の子ではない・立て直しと同じ・NFR6 の「器の子 process」に当たらない）。plugin の自動検出は `[[plugin]]` 行の宣言で置き換える（走査しない・C3）。

## 4.5 役割なしの起動（ADR-0028 §2.5・SRS FR60 / AC30・`<NAME> account shell`・台帳 `s2-07l.268`）

- **何を解くか**: 席の起動は §4 の `seat launch`（役割必須）だけで、役割を持たない素の対話 session（前の版の口座切替 wrapper `cla <label> [--resume <sid>]` 相当・不測の事態に planner / admin が立たないときの手動の入口）は器の外に在る。user 裁定 2026-09-14「役割なしにも対応しておいてほしい」。
- **口**: `account shell <label> [--state-dir S] [--anchor DIR] [--resume SID] [--target S:W]`。label は必須（口座の選定は使わない・FR36 の外）。`--anchor` の既定は cwd の repo root（`seat launch` と同じ・env を読まない）。
- **起動行の導出**: §4 の `derive_launch` と**同じ 1 関数**から、役割に依る 3 要素（役割・登録 row・権能）を除いた形。`CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} <claude> [--resume <sid>] --plugin-dir <anchor> [--plugin-dir <[[plugin]] dir>…] [<[[launch-arg]] value>…]`。`--resume <sid>` は同じ口座でその session を再開する行（sid が別口座の session かは器に判別できない＝断らない・混線の fence は §12 の後続のまま）。
- **登録・権能**: `SeatRegistered` を書かない（役割が無いので権能も無い・role guard は `unregistered` で code の編集を止める＝素の session は repo の外と design-intent / docs/design も書けない・意図どおり）。`inject.jsonl` には `kind=launch` を 1 行（§4 と同じ variant・役割の有無は記録の `role=` の欄が `-`）。
- **起動**: `--target` が在れば §4 (1)(2) と同じ経路（session が無ければ `session-missing`・window が無ければ作る・shell への注入の門）で穴を埋めた起動行を注入する。`--target` が無ければ stdout に起動行を 1 行出して自分では起動しない（`account add` の login 用の行と同じ形）。口座が宣言に無い・退役中・設定 dir が無い周は typed に断り key を 1 つも送らない（`account-unknown` / `account-retired` / `account-dir-missing`）。
- **前の版の wrapper**: 本便の Landed 後に消費者側（wrapper の repo）で退役する（s2 の便ではない）。

## 5. 注入の門（共通・値を持たない）

[account-autonomy.md](./account-autonomy.md) §5 の「shell への注入の門」をそのまま使う（前面 process が shell・prompt 末尾の閉じた列・特定できない周は送らない）。`account add --target` と `seat launch` と立て直しの 3 つが同じ関数を呼ぶ（新しい判定を足さない）。

## 6. 極性（[polarity.md](./polarity.md)）

- host の manifest が読めない（在るが壊れている）: **FailClosed**（既存の fail-closed の断り〔`fleet usage` / `fleet select` は `UsageError::Manifest`・tick は `no-rule:manifest-unreadable` の 1 行〕に host の面が加わる・manifest の拒否と同じ極性・NFR4・Guard の variant は増えない＝極性一覧は不変。ADR-0026 §2.1 の「`Guard::Rules`」は実在しない variant 名で、正しくはこの既存の断りを指す〔.243 run 2 の runner 実測 2026-09-14〕）。無い周は縮退（0 宣言・止めない）。
- `account add` / `retire` / `restore` の前提違反（`exists` / `in-use` / `unknown` …）: 断って何も書かない（file も event も・部分書きなし）。guard ではない（行為を止めうる判定ではなく入力の拒否・ADR-0014 §2.1）。
- `seat launch` の門: `Guard::Inject` の既存の極性（送らない側）。新しい Guard variant は足さない＝極性一覧の行数は不変。

## 7. 失敗の型

- `HostManifest`: `Absent`（縮退）/ `Unreadable`（FailClosed）/ 検査の error は manifest の `RuleError`（行番号付き）をそのまま使う。
- `AccountError`（closed enum・`as_str`）: `exists` / `dir-exists` / `unknown` / `already-retired` / `not-retired` / `in-use` / `label-invalid` / `write-failed`。
- `LaunchError`（closed enum・`as_str`）: `no-account` / `account-unknown` / `account-retired` / `session-missing` / `input-busy` / `input-unknown` / `register-failed`。§4.5 の役割なしの起動は同じ enum に `account-dir-missing` を 1 つ足す（宣言順の末尾）。
- すべて Result で呼び手に分岐を強いる（C11.3）。

## 8. 歯（`crates/<NAME>/tests/e2e/` に `rules_host_` / `account_` / `seat_launch_` 接頭辞・名前の列は現物が SSOT）

- host の manifest: tmp の state dir に `host.toml`（3 種の表）を置き `rules validate --state-dir` が両面を数える／未知 key・label の重複（面をまたぐ重複を含む）・schema 欠落を行番号付きで全件拒む／無い周は 0 宣言で `fleet usage` が「宣言なし」を出す／壊れた file で `fleet usage` / `seat tick` が typed に止まる／tracked の manifest の `[[account]]` が 0 行であること（本 repo の埋め込み manifest を読む歯）。
- `account add`: dir と `settings.json` と `host.toml` の行が揃う／既存 dir・宣言済み label は断り何も書かない／`--target` の偽 tmux に起動行が 1 回だけ届く／`--target` 無しは stdout の 1 行。
- `account retire` / `restore`: mv の前後で dir が 1 つだけ在る／event が 1 件ずつ／登録 row が持つ label は `in-use`／退役中の口座は `fleet select` の候補に出ず `fleet usage` が測らない／`KINDS` の件数 pin が +2・末尾の順。
- `seat launch`: 偽 tmux（window 無し → `new-window` が 1 回）と偽 `claude`（argv と env を写す stub）で、導出した行に `CLAUDE_CONFIG_DIR` と agent view の env と anchor の `--plugin-dir` と `[[plugin]]` / `[[launch-arg]]` の順序が在る／`SeatRegistered` が 1 件（`sid` = none）／`--account` 無しは session 用の選定が別席の口座を除外する／候補なし・session 無し・入力欄に文字・退役中の口座はそれぞれ typed に断り row も key も出さない／`derive_launch` の pure な歯（in-file）。
- 実地（done の一部・歯にしない）: 本 host で `account add` → user の login → `account ls` の `credential=present` → `seat launch --role planner --target …` で席が立ち、SessionStart の打刻と登録 row が揃うこと（外の wrapper を 1 度も使わない）。

## 9. 憲法・制約との整合

C1（rules 行の値は tracked の manifest のまま・host の面は宣言値だけ）・C2 / C2.2（loader 1 本・`--state-dir` からだけ解く・env を読まない・子へ env を**設定**するのは従来どおり）・C3（退役は event log・宣言と状態を混ぜない・dir を走査しない）・C3.2（doctor に host の面と退役の列）・C10.2（host 固有の値 = plugin dir / 起動引数 / 口座 label は host の manifest にだけ）・C11.2 / C11.3（typed な断り・FailClosed は読めない周だけ）・C16.2（Guard は増減なし）・N1 / N1.2（退役は mv・削除の口を持たない）・A1（tracked に口座行を足す変更は聞く・本設計は減らす側）・CON2（口座名・host 名・絶対 path は tracked に置かない＝`host.toml` にだけ書く）。

## 10. 契約（3 便・この順・実装は pipeline）

- **(a) host の manifest**（M）: §2。write-set = `rules/manifest.rs`（`Section::Plugin` / `Section::LaunchArg`・host の面の読み・面をまたぐ重複）・`rules/mod.rs`（loader の入口に state dir を渡す）・`rules/cli.rs`（`validate --state-dir`）・`fleet/usage.rs` / `fleet/select.rs` / 管理 tick の module〔削除済み〕 / `main.rs`（doctor の行）の呼び手・`rules/manifest.toml`（口座行を外す）・`tests/e2e/rules.rs` / `fleet.rs`・外形 snapshot（`src/snapshots/`・doctor と usage）。依存: `.233`（doctor の口座行）の Landed 後（doctor の行の隣に足す・`src/snapshots/` の交差）。base で RED = `rules validate --state-dir` が host の面を数える歯（機能不在）。
- **(b) 席の起動**（M）: §4 / §5。write-set = `seat/cli.rs`（`launch` の flag と usage）・`seat/cycle.rs`（`derive_launch`・launch と relaunch の共通経路・`new-window`）・`seat/inject.rs`（`InjectKind::Launch`）・`fleet/mod.rs`（`Registration.sid: Option<String>`・literal 構築点・KINDS は不変）・`seat/role.rs`（sid の読み手）・`tests/e2e/seat.rs`・seat の外形 snapshot。依存: (a)。base で RED = `seat launch` の偽 tmux の歯 + `derive_launch` の in-file の歯（機能不在）。
- **(c) 口座の口**（M）: §3。write-set = 新 module `account/`（`mod.rs` / `cli.rs`・歯は in-file `#[cfg(test)]` + `tests/e2e/account.rs` は足さない〔統合 test target の上限・`fleet.rs` に置く〕）・`main.rs`（dispatch と usage）・`fleet/mod.rs`（`EventKind::AccountRetired` / `AccountRestored`・replay の退役集合・`KINDS` +2）・`fleet/select.rs` / `fleet/usage.rs`（有効な口座の集合を読む）・`main.rs` の doctor の行（`retired=`）・`tests/e2e/fleet.rs`・外形 snapshot。依存: (a)・(b) と `fleet/mod.rs` で交差するので直列（(b) の後）。base で RED = `KINDS.len()` の pin を 15 にする歯 + `account add` の歯（機能不在）。

- **(d) 役割なしの起動**（S・`s2-07l.268`・ADR-0028 §2.5）: §4.5。write-set = `account/cli.rs`（`shell` の flag と usage）・`account/mod.rs`（`login_line` の隣に役割なしの起動行の導出・`derive_launch` を呼ぶ側）・`seat/cycle.rs`（`derive_launch` の役割なしの形＝役割に依る 3 要素を外す引数・`LaunchError::AccountDirMissing`）・`seat/inject.rs`（`kind=launch` の記録の `role=` 欄）・`tests/e2e/fleet.rs`（`account_` の歯の置き場・現物の module）・`tests/e2e/seat.rs`（`derive_launch` の歯）・account / seat の外形 snapshot。依存: (c) Landed（済み）。base で RED = 偽 tmux + 偽 `claude` で `account shell` を撃つと登録 row 0 のまま起動行が 1 回だけ差し込まれる歯（機能不在・AC30 の 5 件 = 差し込み 1/1 + stdout 1/1 + resume 1/1 + 拒否 2/2）。

順序の理由: (a) は `.220` の穴を塞ぐ土台で他の 2 便が読む。(b) が user 裁定 2026-09-14 の「席の起動を `.222` の後・`.208` run 5 の前」の便。(c) は (b) と `fleet/mod.rs` で交差するので後。(d) は (a)〜(c) の Landed 後で、`seat/cycle.rs` を触るので `.279`（tick の分割・cycle.rs は触らない）とは交差しないが `.307`（relaunch の口座）とは交差する＝直列。

## 11. 却下案（ADR-0026 §5 の写しは持たない・設計固有のもの）

- `account add` が login の session を器の子 process として起こす。却下: 器の子 process は封じ込めの箱の中（NFR6）で、対話の login を箱に入れる理由が無い。席と同じく shell へ注入する（子にしない）。
- `<state_dir>/accounts/` を走査して口座を数える。却下: ADR-0017 §2.3（真実は宣言・C3）。dir が在っても宣言の無い口座は「無い」。
- 退役を `host.toml` の行の削除で表す。却下: 退役は状態（C3「退役は DB」）で、宣言 file の書き換えは可逆 move にならない（行を失う）。event + mv。
- 起動行の雛形 file を `seat launch` の引数に残す（`--launch FILE`）。却下: 雛形が手書きに戻り host 固有の値が file に散る（C10.2）。host 固有の値は `[[plugin]]` / `[[launch-arg]]` の行で持ち、行は器が組む。
- 席の起動に cgroup の防壁を持ち込む。却下: 席は shell の子で器の子ではない（立て直しと同型）。防壁が要るのは便の runner で、それは [gate-cost.md](./gate-cost.md) の封じ込めが持つ。
- 前の版の launcher と別 repo の口座切替 wrapper を器から呼ぶ。却下: 器の視野の外の script に依存する形（ADR-0009 §1 の根因と同型）。

## 12. 後続

user-scope MCP 設定の同期・別口座への `--resume` の混線 fence・primary 口座の写像（user 裁定 2026-09-14「持ち込まない」）／再 login（credential の失効・user の手番・doctor が示す）／退役した口座の dir の削除（A1・削除の口は持たない）／launch 直後に止まった席の起こし直し（[account-autonomy.md](./account-autonomy.md) §11 の crash と同じ）／tracked の manifest に口座行を戻す形（A1）。

## 13. 役割なしの起動の口 account shell（契約表の行 a・`s2-07l.268`）

- 何が起きているか: user の問い 2026-09-14「cla は今のところ消さないが、最終的には scribe v2 にどこかで完全に互換機能が搭載されるよね？」への **user 裁定 2026-09-14 12:5xZ**「まあ確かに役割なしにも対応しておいてほしいかな。何か不測の事態のときのために。」を承けた便。現物: 席の起動は `seat launch --role`（役割必須・`.244` Landed）だけで、役割を持たない対話 session（前の版の口座切替 wrapper `cla` 相当）は器の外に在る。`account add` は login 用の起動行 `login_line`（`account/mod.rs`）を既に持つ。設計は §4.5「役割なしの起動」が正本（ADR-0028 §2.5）。
- 形: 口 `account shell <label> --state-dir S [--anchor DIR] [--resume SID] [--target S:W] [--tmux-socket P]` を `account/cli.rs` の verb に足す（`SHELL_FLAGS` の閉じた列・`--state-dir` は他の verb と同じく必須＝穴 `{account_dir}` は置き場無しに埋まらない）。起動行の導出は §4 の `derive_launch` と**同じ 1 関数**に `resume: Option<&str>` を足して得る（`None` は従来の行）。引数の出所は §4 と同じ base の `pub` の口だけで、可視性を 1 つも変えない: anchor は `seat/role.rs` の `anchor_of`（`--anchor` か cwd の repo root）・plugin と起動引数は `crate::rules::read` が返す `Manifest` の `plugins()` と `launch_args()`（C2.2・manifest を読めない周は `add` と同じ `write-failed` で rc 2・key 0）・model は持たない（`--model` を足さない）。`--target` 無しは穴を埋めた起動行を stdout に 1 行、在れば `seat launch` と同じ shell への注入の門を通して注入する。window は `seat launch` の `open_window`・記録は `record_launch` を `pub(crate)` にして `cycle.rs` の再輸出から呼ぶ（`record_launch` の引数は `&Launch` を要らない形に落とす＝役割の起動の値〔manifest・役割・刻み〕を持たない口座側から呼べる形にする＝記録の書き手は 1 つのまま）。`inject.jsonl` の `kind=launch` 行に `role=` 欄を足し、役割なしは `role=-`。**登録 row（`SeatRegistered`）は書かない**（権能なし・role guard が `unregistered` で編集を止める）。断りは `AccountError` に variant を足す（`account-dir-missing`）。
- 触らない: `seat/tick*`・`fleet/mod.rs`（event の形は不変）・`hook/`・`account/mod.rs` の `add` / `ls` / `retire` / `restore`・`docs/`・`design-intent/`・`prop.rs`（共有）。
- 依存: docs PR #185 merged ∧ `.307`（合図の出所と立て直しの口座優先）Landed 後（`seat/cycle.rs` で交差）。`.303` / `.304` とは交差 0（`hook/` / `seat/tick*` / `fleet/mod.rs` を触らない）。

## 14. 席の起動の短い形（契約表の行 b・`s2-07l.404`）

- 何が起きているか: §4 の `seat launch` は引数 4〜5 個（置き場・役割・target・口座・model）を毎回書かせる。置き場は `seat heartbeat` が git 設定から解けるのに launch は必須 flag、target と model は同じ鍵（役割 × anchor）の登録 row が既に持つ値。user 直命 2026-09-16 08:2xZ（逐語は台帳 `s2-07l.404`）: 口座 label と役割の flag だけの 1 行で planner / admin を起こせる形が要る。
- 形: `seat <label> (--planner|--admin) [--target S:W] [--model M] [--anchor DIR] [--restore CMD] [--state-dir S]`。第 1 token が既知の verb でなく `--` で始まらなければ口座 label と読む。役割の flag は**ちょうど 1 つ**（0 か 2 は使い方の誤り・rc 1）。既定は全部 1 関数で導く: 置き場 = state_dir_of（`--state-dir` > git 設定・解けなければ `state-dir`）／anchor = `--anchor` か cwd の repo root（`seat register` / `seat launch` と同じ）／target と model = 同じ鍵（役割 × anchor）の**登録 row の値**（`seat/role.rs` の registration_of_target の隣に鍵で引く読み手を 1 本置く・row の `model` が無ければ `--model` が要る）。row が無く flag も無い周は `defaults-unresolved` で typed に断る（足りない flag の名を行に載せる・1 key も送らず row も書かない）。断りの行は `seat launch: refused reason=defaults-unresolved missing=<flag[,flag]>`（`missing=` は足りない flag の名を宣言順 `--target` → `--model` で `,` 区切り・`target=` は載せない＝target が解けない周にも出る断りに未確定の値を置かない・rc は既存の `RC_REFUSED`・字面の定数は `seat/cli.rs` に 1 つ）。明示の flag は row の値に勝つ。導いた値で §4 と**同じ `LaunchFlags` を組み同じ経路**を通る（`seat/cli.rs` の launch_of の本体を flags を受ける 1 関数に括る）＝短い形と長い形は同じ Registration・同じ起動行を作る。使い方の行に短い形を足す（外形 snapshot が動く）。
- 触らない: `seat/cycle/launch.rs`（起動の本体・derive_launch）・登録 row の schema・`account shell`（§4.5・役割なし）・tick の立て直し・rules 行。
- 歯（`seat_launch_short_` 接頭辞・`crates/scribe2-boundary/tests/e2e/seat/launch.rs`）: 登録 row が在る周に短い形が長い形と同じ row と同じ注入行を作る（両方を偽 tmux と偽 claude で撃ち、inject.jsonl の what と row の差分 0）／row が無く `--target` `--model` も無い周は `defaults-unresolved` + 0 key + row 0／役割の flag が 0 か 2 は使い方 rc 1／既知の verb（`launch` ほか）は従来どおり通る。
- 却下: session 名を NAME 定数から導く（今の席は別名の session に居る＝改名は移行で本便の外・値を code に焼くのは N3）／host.toml に target を手書き（登録 row が既に持つ値の二重化・C3）／`seat launch` の flag を任意化するだけ（人が打つ形が長いまま）／短い形を `account` の verb に置く（役割の起動は §4 の領分）。
- 後続: 起動行に effort を運ばせる形（役割ごとの値は rules 行・裁定 id 要・別便）。

## 15. 口座の OAuth 墓標を器が名指し、席が login 画面で止まった周を人間へ上げる（契約表の行 c・`s2-07l.420`）

- 何が起きているか（un:planner の relay 2026-09-16・uns 台帳 un-5u0d・実事故は別 host の管理席・別口座で 1 時間 3 分・verified）: 口座の OAuth session が死ぬ（credential の `claudeAiOauth.expiresAt == 0` の墓標・自動 refresh 不能・対話の claude は login 画面で無言停止・`-p` は「OAuth session expired and could not be refreshed」の 1 行）と、席は login の modal で止まる。管理 tick は毎分 `decision=noop reason=account-unmeasured … account=<label>:unmeasured state=idle` を 63 周繰り返し、器は 1 時間だれにも告げなかった＝user が手で気づいて再 login した。現物（main fffa8bb）: 口座の probe（`account/mod.rs` の `Presence`）は credential の**在る / 無い**だけを見る。計測（`fleet/usage.rs` の `token_of`）は墓標を先に見て `UnmeasuredReason::Tombstone` で「測れなかった」を記録する＝計測の側は名指しているが、tick の口座の軸（管理 tick の口座の軸〔削除済み〕 の 口座の軸の「測れない」〔管理 tick ごと削除済み〕）は理由を畳み、一時的な unmeasured（429 / timeout）と墓標を区別せず、人間へ上げる口も無い。
- 形: (1) **probe の 3 値**: `Presence` に variant `Dead` を足し（present / missing / dead・閉じた enum・`as_str` = `dead`）、credential を read-only で読んで `expiresAt == 0` の周だけ `Dead`（読めない・欠けは `Present`＝「在るが読めない」を墓標に読み替えない・書かない）。墓標の判定は計測の `token_of` と**同じ 1 本**（`fleet/usage.rs` の読みを `fleet` の共有の関数に寄せ、probe と計測の両方が呼ぶ・C2）。`account ls` / doctor の口座行の `credential=` にそのまま載る（外形 snapshot が動く）。(2) **tick の弁別**: 口座の軸は測れない周に replay の最新の Unmeasured の理由を読み、`Tombstone` の周だけ `NoopReason` の新しい variant `AccountDead`（字面 `account-dead`・記録の `what` に載る）に倒す（他の理由は従来どおり `account-unmeasured`）。(3) **呼び鈴**: `account-dead` が同じ席の tick.jsonl で連続 N 周（rules 行 `seat.account_dead_rounds`・kind `SeatAccountDeadRounds`・Int・**値 = 3**〔user 裁定 2026-09-16T22:0xZ・記帳は台帳 `s2-07l.420`〕・C5）に達した周に 1 回だけ、対話面（R-C7-1 = orchestrator の登録 row を持つ席・同じ anchor の `Role::Orchestrator` の row・自席がそれなら自席）へ `NEEDS-USER: account=<label> credential=dead target=<seat> rounds=<N> — 再 login が要る` の 1 行を注入し（注入の経路は `s2-07l.479.3` で口ごと消えた＝この呼び鈴の送り先と手順は未定・(3) は超過の側）、その後は N 周ごとに繰り返す（黙らない・毎周は鳴らさない）。その row が無い周は注入せず記録だけ（注入先を推測しない）。(4) **doctor / account ls**: 口座行は同じ 1 行の形（`account/mod.rs` の probe の行）なので `credential=dead` がそのまま出る（追加の機構なし・人が `doctor` で気づける）。
- 触らない: login の自動化（器は再 login しない・人間の操作）・期限切れ時刻（`expiresAt` の未来 / 過去）を根拠にした警報（false alarm 源・un-5u0d の裁定）・`Presence` の他の値の意味・選定（FR36・墓標の口座は `unmeasured` のまま候補から外れる＝従来）・退避の合図の brake。
- 歯（`account_credential_dead_` / `seat_tick_account_dead_` 接頭辞・`tests/e2e/seat/account.rs` と 管理 tick の歯の file〔削除済み〕）: 墓標の credential を置いた口座の `account ls` が `credential=dead`／`accessToken` が無いだけの credential は `present`／偽 usage で `unmeasured reason=tombstone` の口座を持つ席の tick が `account-dead` を記録し、N 周目に対話面の target へ NEEDS-USER の 1 行を注入（N-1 周では注入 0）、N+1〜2N-1 周は注入せず 2N 周目に再び 1 行／429 の unmeasured は従来どおり `account-unmeasured`（呼び鈴 0）／rules 行は kind 件数の pin と外形の歯。
- 却下: 期限切れ時刻の警報（false alarm）／tick が自分で re-login を試みる（人間の操作・器の外）／口座の軸の「測れない」〔管理 tick ごと削除済み〕 に理由の String を持たせる（閉じた enum の理由を文字で運ぶ・C3.3）／毎周鳴らす（planner の入力欄を埋める・§10 の型の事故）。

## 16. 席の実口座を SessionStart が測って記録し、登録 row との照合を指示文で見せる（契約表の行 d / e・`s2-07l.438`）

- 何が起きているか（別 repo の planner 席からの relay 2026-09-17・出所と記録の字面は台帳 `s2-07l.438`）: 席を credential dir だけ替えて手で起こし直すと、実 session は口座 A で動くのに登録 row（`SeatRegistered` の account）は口座 B のまま残る。当時の現物（main 3f1eec8）では管理 tick の口座の軸が **row の口座**の逼迫度だけを読み、B が閾値以上の周に context 14% の席へ `origin=account` の退避の合図を注入した（誤発火）。その軸・退避の合図・席の立て直しは ADR-0045 §2 (2) で消えた（FR27 / FR38 は恒常の不在・歯が不在を確かめる）ので誤発火は再発しないが、穴の残り 2 つは今も在る: 実口座を測って残す書き手が無く（SessionStart は row を書かず、打刻 dir にも口座は無い）、SessionStart の指示文（`seat/brief/`）は row の値だけを出して食い違いを名指す面が 1 つも無い（doctor の席の行も row の口座を写すだけ・2026-09-21 の口座移動で registered=3 live=1 missing=2 のまま旧口座を出した）。
- 形 (1) **実口座の記録（行 d）**: SessionStart hook は入力 JSON の `transcript_path`（`hook/mod.rs` が既に `KEY_TRANSCRIPT` で読む key・transcript は credential dir の下に置かれる）から実口座を導く。path が字面で `<state_dir>/accounts/<label>/` の下に在り、`<label>` が 1 要素で `.` 始まりでない周だけ label を得る（それ以外・key が無い周は unknown）。**env は読まない・hooks.json の shell 行も足さない**（C2.2＝既存の入力 JSON の内側で閉じる）。`--pane` から target を解けた周に、席の打刻 dir（sid の打刻・読み込み元の記録 `plugin` と同じ置き場）へ 1 file 1 行の記録を**毎 SessionStart に上書き**する（schema・口座 label か unknown・sid・ts。unknown も書く＝前の session の値を残さない）。読む側は「記録が在る / 無い / 読めない」を型で分ける（`hook/vessel/digest.rs` の `PluginRecord` と同型・C10: 測った値・出所 = hook）。書けない周は席を止めず stderr に 1 行（読み込み元の記録と同じ）。
- 形 (2) **row は上書きしない・記録を読んで席を止めない（行 d）**: row の account は宣言から導いた実効値の写し（[seat-roles.md](./seat-roles.md) §15 (5)）で、実測で書き換えない（C10）。記録の読み手は形 (3) の指示文だけで、記録を読んで合図を撃つ・席を止める・立て直す経路は置かない（ADR-0045 §2 (2) が消した機構＝FR27 / FR38 の恒常の不在を破らない）。記録が無い・unknown・読めない周は指示文が「unrecorded / unknown / unreadable」と出すだけで、旧 session・hook の載らない席をどの段も止めない。
- 形 (3) **見える化（行 e）**: 指示文の雛形（planner / admin）に出所 pointer 付きの 1 行を足し、穴 3 つ（row の口座・実測の口座〔label / unknown / unrecorded / unreadable〕・照合〔match / mismatch / unknown の閉じた字面〕）を `HOLES` に足す（4 → 7・xtask の `BRIEF_HOLES` も同じ列）。行は直し方の pointer（`seat launch --account`＝row も更新する正規の口）を持つ。hook は記録を書いた**後**に指示文を組む。出す面は指示文の 1 つだけである（復元の DATA の `[SEAT]` 行に同じ値を足す案は、その DATA ごと超過した・ADR-0045 §2 (2)・`s2-07l.479.2`）。
- 触らない: 登録 row の schema と書き手（`seat register` / `seat launch` / 立て直し）・口座の選定（FR36）・役割の口座（seat-roles §15〜§18）・`state.jsonl` の schema（打刻の行に口座を足さない）・doctor の席の行・生成 hooks.json・rules 行。
- 歯（`seat_account_mismatch_` 接頭辞）: 行 d = `tests/e2e/hook.rs` に `seat_account_mismatch_record_`（偽の `transcript_path` が置き場の accounts の下 → label を記録／外 → unknown を記録して前の値を消す／`.` 始まり・key 無し → unknown／pane 無し → 書かない／記録の file は `plugin` の記録と同じ席の打刻 dir に在り、row と `state.jsonl` は 1 byte も変わらない）。行 d に tick・合図・立て直しの歯は無い（撃つ相手が不在）。行 e = `seat_account_mismatch_shown_`（指示文に 3 つの値が出る・食い違う周は mismatch）・in-file の `seat_brief_holes_`・xtask の `seat_brief_`・指示文の snapshot。
- 却下: row を実測で追記更新する（memo の案 (a)・宣言から導いた値を測定で上書きする形＝C10 と seat-roles §15 (5) に反し、手起動 1 回で役割の口座の宣言とも食い違う row が出来る）／記録を読んで合図を撃つ・席を止める・立て直す経路を足す（ADR-0045 §2 (2) が消した管理 tick の口座の軸の再導入＝FR27 / FR38 の恒常の不在を破る・`.533` run 1 の審査 FAIL literal-mismatch の根）／hooks.json の shell 行に credential dir の env を渡す flag を足す（入力 JSON で取れる値に新しい口を足さない・C2.2）／`accounts/` を走査して実体の path で照合する（§11 の却下と同じ・字面で当たらない手起動は unknown に倒れて従来どおり）／`state.jsonl` の行に口座を足す（on-disk の schema 変更・毎 turn の行に不変の値を運ぶ）。
- 依存: 行 d に先行は無い（行 c `s2-07l.420` は着地済み・交差していた管理 tick の module は削除済み・行 d は `tests/e2e/seat/account.rs` を触らない）。行 e は行 d の Landed 後（記録の読み手を使う・`hook/mod.rs` で交差）。
- 後続: 食い違いが続く周の呼び鈴（§15 (3) と同じ型・閾値は rules 行＝裁定 id 要）／字面で当たらない credential dir（link 越しの手起動）の照合。

## 17. 席の口座を持つ単位は project の群 — host の面に群の宣言を 1 表足し、doctor が群ごとに 1 行を出し、便用の選定が群の候補の口座を host 全体で外す（契約表の行 f・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html)・`s2-07l.491`）

- 何を解くか: 席（orchestrator）がどの口座で立つかは置き場ごとに閉じていて、複数の置き場の席を同じ口座にまとめる宣言が無い。決定は ADR-0049 で、本 § はその **第 1 段**（宣言と点検と便用の除外）の実装の形である。第 2 段（閾値越えの通知）と第 3 段（自動の移動）は本 § に含めない。
- やさしく言うと: 「どの project をひとかたまりにして、どの口座を順に使うか」を machine に 1 つだけ在る宣言 file に書けるようにし、点検コマンドがそれを 1 行で見せ、便にはそのかたまりの候補口座を渡さないようにする。移る動きはまだ作らない。
- 実測（main の現物・2026-09-20）:
  - host の面の読み手は `crates/scribe2/src/rules/manifest.rs` の 1 本で、受ける表は `[[account]]` / `[[plugin]]` / `[[launch-arg]]` / `[[vessel]]` の 4 つ。表の名は閉じた列挙 `Section` が持ち、`Section::header` / `Section::known_keys` / `Section::required_keys` の 3 つの網羅 match と、面ごとの受理を決める `collect` の大きな match が同じ列挙に掛かる。tracked の面にだけ在る表（`[[rule]]`）を host の面で断る枝は在るが、**host の面にだけ在る表**の枝はまだ 1 つも無い。
  - 面の合わせは `Manifest::joined`、host の面の読みの 3 値は `HostManifest::read`（不在 = 0 宣言 / 読めた / 読めない = typed に止まる）。
  - 便用の候補を作る口は **2 つ**在る: `crates/scribe2/src/fleet/replay.rs` の `select_for_run`（除外は `registered_accounts(Some(repo))`）と、`crates/scribe2/src/fleet/cli.rs` の `select_account` の `Purpose::Run` の枝（同じ除外を**自分で**組む）。`select_for_run` を呼ぶのは `crates/scribe2/src/fleet/wait.rs` と `crates/scribe2/src/pipe/ratelimit.rs` の 2 file（どちらも自分の除外は組まない）。
  - doctor の口座の行は `crates/scribe2/src/account/mod.rs` の `doctor_lines` が組み、その中で合わせた面を既に持っている。席の行は `crates/scribe2/src/seat/role.rs` が先に出し、並べる `render_doctor_with`（`crates/scribe2-boundary/src/main.rs`）は 3 つの出所を足すだけである。
  - 群という概念は器に 1 つも無い（`group` の語は process の group にしか当たらない）。ADR-0036 / ADR-0041 が決めた宣言 file と実効の記録も code には 1 行も無い。
- 約束（1 つずつ歯が測る・行 f の done と 1:1）:
  1. **host の面に群の表が 1 つ増える**: 表は `[[account-group]]`、key は名 `name`（host で一意）・置き場の列 `anchors`（anchor の列・1 つ以上）・候補の口座 label の列 `accounts`（順序が候補の順・1 つ以上）の 3 つ。読み手は既存の 1 本のままで、新しい reader も新しい file も足さない。file が無い host は 0 群として続き、在るのに読めない host は今までどおり typed に止まる。
  2. **tracked の面には置けない**: tracked の manifest に群の表が在る周は未知の表として行番号付きで断る（`[[rule]]` を host の面で断るのと対称の、この読み手で初めての「host の面にだけ在る表」）。
  3. **宣言の欠陥を行番号付きで全件断る**: 同じ名が 2 行・同じ置き場が 2 つの群に在る・候補の label が合わせた面の口座の表に無い・置き場の列が空・候補の列が空・未知の key の 6 種。段は host の面の既存の拒否形と同じ（面の中の欠陥で止まった周は合わせの検査へ進まず、先に落ちた段の全件を出す）。
  4. **便用の選定が群の候補の口座を host 全体で外す**: 便用の候補から、宣言のどの群の候補の label も外す。除外は**次の選定から**効き、走行中の便は止めない。便の置き場の席の登録 row の除外（[account-autonomy.md](./account-autonomy.md) §14）はそのまま残り、群の除外がその上に重なる。
  5. **除外は上の 2 つの口の**両方**に効く**: 実測のとおり便用の候補を作る口は 2 つ在るので、片方だけ直すと `fleet select` の口から群の口座が漏れる。両方が同じ除外を持つ。
  6. **session 用の選定と並べ順は 1 行も変えない**: 席を起こす口座の選び方（session 用）・便用の並べ順・1 口座あたりの上限を置かないこと（[account-autonomy.md](./account-autonomy.md) §19）は不変。群の今の口座を器が**書く**のは第 3 段で、本便は 1 件も書かない（宣言値だけを読む）。
  7. **doctor が群ごとに 1 行を出す**: 口座の行の後ろに、宣言された群 1 つにつき 1 行を宣言順で出す。項目は名・候補の label の列・置き場の数・その群の置き場の席の登録 row が持つ口座 label の列（重複は畳む・1 つも無ければ無しを表す語 `none`・event log を読めない周は `unreadable`）の 4 つで、形は `group=<名> accounts=<候補の列> anchors=<置き場の数> seat-accounts=<席の口座の列>`。読むだけで判定しない。
  8. **群を 1 つも宣言しない host は 1 語も変わらない**: 群の行を 1 本も出さず、便用の除外も今のままで、既存の doctor の外形 snapshot は動かない（＝既存の consumer と本 repo は無変更）。
- 触らない: 口座の登録・退役・復帰・一覧の口（§3）・席の起動の口座の解き方（§4）・host の面の置き場と導き方・選定の純関数とその入力の型・rules 行（閾値の行を足すのは第 2 段の便）・event log の schema・host の根（第 3 段まで記録を置かない）。
- 歯（接頭辞 `host_group_`・crate の統合 test の target に置く。`host_group_` は crate に 1 件も無い〔実測〕ので、verify の filter が当たる file は下の 3 つに閉じる）:
  - `crates/scribe2-boundary/tests/e2e/rules.rs`: 約束 1 / 2 / 3（母集団 = 正常 1 + 不在 1 + tracked の面 1 + 欠陥 6 種 = 9 本。欠陥の周は行番号と欠陥の字面を全件見る）
  - `crates/scribe2-boundary/tests/e2e/fleet.rs`: 約束 4 / 5 / 6（母集団 = 便用で全候補が外れる 1 + 群に属さない口座は残る 1 + 群 0 の host は今の候補のまま 1 + session 用では候補に残る 1 + `fleet select` の口でも同じ除外 1 = 5 本）
  - `crates/scribe2-boundary/tests/e2e/seat/account.rs`: 約束 7 / 8（母集団 = 群 2 つの host が宣言順に 2 行 1 + 席の登録 row の在る群に label が出る 1 + 無い群は無しの語 1 + 群 0 の host は行 0 本 1 = 4 本）
- verify の当たる file（事前検査の実測・どれも write-set の中）: `rules_host_` は 3 file（`crates/scribe2-boundary/tests/e2e/rules.rs`・`crates/scribe2-boundary/tests/e2e/fleet.rs`・`crates/scribe2-boundary/tests/e2e/seat/rules.rs`）に当たる。`seat/rules.rs` は**中身を変えない**が、歯の置き場の門のため write-set に載せる。`fleet_select_` は `crates/scribe2-boundary/tests/e2e/fleet.rs` の 1 file、`doctor_accounts_` は `crates/scribe2-boundary/tests/e2e/seat/account.rs` の 1 file だけに当たる。`host_group_` の当たる歯は 0 本（この行が起こす歯が最初の 1 群）。
- 大きさの余地（事前検査の実測）: 歯を足す `crates/scribe2-boundary/tests/e2e/{rules.rs,fleet.rs}` は 1 file の行数の上限に対する余地が 0 で、本体側の 8 file はどれも 1 段の見積より余地が大きい。歯を足す側は上限の外（歯の file は 1 file の上限を数えない）なので受付は通るが、`fleet.rs` は既に 4000 行を超えているので、歯の群をこの file の末尾へ足すか、同じ target の子 module へ割るかは実装の便が決める（割る周は write-set に新しい file を `+` で足す）。
- 変更する既存の歯（名で数える）: 表の数と受理する表の名を数える `rules_host_` の歯（受理する表が 1 つ増える）と、面の合わせを測る歯。doctor の外形 snapshot は約束 8 のとおり**動かない**。
- 却下: 群の宣言を新しい file に置く（ADR-0036 の形・宣言の置き場が 2 つになる・読み手も 2 本になる）／群を置き場ごとの面（各 state dir）に置く（同じ群の定義が host に N 個できて食い違う）／群の宣言を持たず席が起きるたびに選定へ落とす（同じ群の席が散る・席が起きていない周は便へ取られる・ADR-0049 の OPT4）／便用の除外を群の今の口座 1 つだけにして候補の残りを便に開ける（便用の選定の上限は 100 % の定数で便は口座を上限まで使う側なので、移り先の候補を先に使い切って逃げ道が残らない・除外が実効の記録の読みに依存し読めない周に typed に決まらない・ADR-0049 の OPT7）／除外を `select_for_run` の中だけに足す（`fleet select` の口から漏れる・約束 5）／doctor の行を群 0 の host でも 1 行出す（既存の外形 snapshot が動き、無変更の約束が崩れる）。
- 依存: 無し（本 § は宣言と読みと除外と点検だけで、契機も記録も持たない）。
- 後続: 第 2 段 = 閾値越えの通知（契機は別の設計 doc の拍・閾値の rules 行 3 本は裁定 id 付きで足す）／第 3 段 = 群の今の口座の記録と自動の移動（host の根・lock・承認 event・機械の復帰）／[seat-roles.md](./seat-roles.md) §15〜§18 の置き換えの後始末。

## 18. 席の起動の短い形が会話を運ぶ — `-c` で直前の会話を、`-r <id>` で名指した会話を引き継いで起動し直す（契約表の行 g・§14 の続き）

やさしく言うと: 口座を移すときに「会話ごと起動し直す」口が無く、手書きの script で resume していた。短い形に `-c` / `-r <id>` を足して `seat <口座> -c` の 1 行で済ませる。役割の flag は今も省略できる（0 個は orchestrator）。

- 出所: user 直命 2026-09-21（逐語は台帳 `s2-07l.491` の notes）。口座の移動（席の口座の 7 日窓が逼迫）で 4 席を手書きの script（置き場の `seat/orchestrator.launch` に `--resume <id>` を足して exec する 1 本）で起動し直した。短い形（§14）は登録 row を書き直して起動行を注入するが、会話を引き継ぐ flag が無い＝script は row を書き換えず、SessionStart が row と実口座の食い違いを出し続ける。
- 現物（verified・main）: 短い形の parser は `crates/scribe2/src/seat/cli.rs` の `short_of`（役割の flag は 0 個で既定の `Role::Orchestrator`・`short_role_of`）。起動行は `crates/scribe2/src/seat/cycle/launch.rs` の `derive_launch` が組み、`prepare` が登録 row（`launch` = 導出した行・穴を埋める前・旗無し）を書き、`boot` が穴を口座 dir で埋めて pane へ注入する。**呼び手の pane が target そのものの周**（同じ窓）は前面と入力欄の判定を飛ばす＝「席の窓で Claude を抜けて同じ shell から撃つ」流れは既に通る。会話の置き場は claude の `projects/<cwd>/<id>.jsonl` で、口座 dir の `projects` が host で共有されていれば別口座からも `--continue` / `--resume` で引ける（本 host は共有・器は確かめない）。
- 形（短い形だけ・長い形 `seat launch` は触らない）:
  1. **flag 2 つ**: `-c`（別名 `--continue`）は直前の会話を、`-r <id>`（別名 `--resume <id>`）は名指した会話を引き継ぐ。どちらも**注入する起動行の末尾**に claude の同名の flag（`--continue` / `--resume <id>`）を足すだけ。**登録 row の `launch` と置き場の `.launch` は旗無しのまま**（row は雛形・会話の id は 1 回きりの値・§14 の「row の `launch` = 導出した行」を変えない）。
  2. **使い方の誤り**（rc 1・key を 1 つも送らず row も書かない・§14 と同じ極性）: `-c` と `-r` の両方／`-r` に値が無い・空／同じ flag の重複／**`-r` の値が会話 id の形でない**。会話 id の形は claude の session id（UUID・16 進小文字と `-` だけ・36 字）で、それ以外の字（空白・`$`・引用符・`;`・`-` 始まり等）を 1 字でも含む値は使い方の誤りとして断る。注入する起動行は pane の shell が読むので、値を引用符で包むのでなく**値の字の集合を絞る**（2026-09-21 の便 1 本目の gate で lens が指摘: 空と `-` 始まりだけを断り、`-r 'a b'` が `--resume a b`、`-r '$(x)'` が pane で実行される形だった）。
  3. **`-c` の先は器が確かめない**: 直前の会話は claude が口座 dir の `projects/<cwd>` から選ぶ。別口座へ移る周に `projects` が共有されていなければ別の会話（か新規）が開く＝人が SessionStart の brief で会話の id を見る（§16 の照合の 1 行と同じ面・器に会話の一覧を読ませない）。
  4. **役割の flag は今のまま省略可**（0 個 = orchestrator・`Role` の variant は 1 つ）。役割が増えても既定は変えない（増えた役割は flag で名指す）。
  5. `--restore CMD` との併用は可（起動後に 1 回送る手順は不変）。同じ窓の周の `--restore` の断り（§14 の約束 8）も不変。
- 触らない: `derive_launch` の雛形・登録 row の schema・`seat launch`（長い形）・`account shell`・`.launch` file の中身・置き場の解き方。
- 歯（`seat_launch_short_` 接頭辞・`crates/scribe2-boundary/tests/e2e/seat/launch.rs`・偽 tmux と偽 claude で撃つ既存の型・新設の歯は `.config/nextest.toml` の tmux の群に名を足す）: `-c` の周は inject.jsonl の注入行の末尾が `--continue` で row の `launch` に `--continue` が無い／`-r <id>` の周は注入行の末尾が `--resume <id>` で row の `launch` に無い／`-c -r x`・値の無い `-r`・`-c -c`・**会話 id の形でない `-r` の値（空白を含む・`$(` を含む・`-` 始まり）**は使い方 rc 1 で key 0・row 0（負例は 1 本の歯で 3 値とも撃ち、正例の UUID が通ることも同じ歯で見る）。外形 snapshot（`seat_usage_external_form`・`crates/scribe2-boundary/tests/e2e/seat.rs`）は usage の 1 行に `[-c|-r ID]` が増える。
- 却下: `.launch` file に旗ごと書く（雛形に 1 回きりの値が混ざり、次の起動で古い会話へ戻る）／`-c` を既定にする（会話の無い口座で claude が新規を開くだけだが、人が「引き継いだつもり」になる・明示の flag が安全側）／器が会話の一覧を読んで id を選ぶ（claude の内部形式に依存・N3 の匂い・`-c` は claude 自身が選ぶ）／長い形にも足す（人が打つのは短い形だけ・§14 の却下と同じ）。
- 後続: 第 3 段（自動の移動・§17 の後続）は本行の `-c` を機械が撃つ形で組める（席の restart = `seat <次の口座> -c` の 1 行）。

## 19. 群の逼迫の通知（第 2 段）— 閾値の rules 行 3 本を足し、dispatch の 1 周が群ごとに使用率を読んで席の pane へ 1 行を注入し、席自身の hook が自席の口座を読んで指示文で告げる（契約表の行 h・§17 の第 2 段・[ADR-0055](../../design-intent/decisions/ADR-0055-group-pressure-is-measured-at-run-ends-and-seat-turns-without-a-timer.html)・`s2-07l.491`）

やさしく言うと: 「このかたまりの口座、もう残りが少ない」を器が自分で気づいて席に知らせる段。時計は持たない。作業（便）が 1 つ終わった直後に器が回す 1 周と、席が話す番のたびに動く仕組み（hook）の 2 つで測る。移る動きは次の §20。

- 出所: user の要求 2026-09-19（5 時間窓 85 / 7 日窓 95 / モデル別窓 95 を境に群ごと別口座へ・逐語は台帳 `s2-07l.491` の notes）。決定は ADR-0049 §2（3 段の第 2 段・閾値は rules 行）と ADR-0055（契機 = 便の終端の周 + 席自身の hook・timer は持たない）。SRS は FR38 の改稿の後にしか本 § を許さない（ADR-0055 CTX4）。
- 現物（verified・main）:
  - 1 周は `crates/scribe2/src/pipe/dispatch.rs` の `fire`（起こす側）と `turn`（見る側・`dispatch ls`）。終端の周の通知は `crates/scribe2/src/pipe/cli.rs` の `notices` が集め、`crates/scribe2/src/pipe/notify.rs` の `send` が repo を anchor に持つ orchestrator の登録 row の席へ `deliver_within` を 1 回撃つ（§5 の契機・[dispatcher.md](./dispatcher.md) §19）。
  - 使用率の計測は `crates/scribe2/src/fleet/usage.rs` の `measure` の 1 本（usage API・実測は event `AllowanceMeasured`・field は `used_pct`）。event の種類は `crates/scribe2/src/fleet/mod.rs` の閉じた列挙 `EventKind`（全 variant を並べる const `KINDS`・enum-slices の門）が持ち、網羅 match は `crates/scribe2/src/fleet/event.rs` と `crates/scribe2/src/fleet/replay.rs` に在る＝通知の event の variant を足す便はこの 3 file を触る。`KINDS` の本数と末尾の順は `crates/scribe2-boundary/tests/e2e/fleet.rs` の歯 2 本（`fleet_kinds_follow_declaration_order`・`account_cmd_kinds_are_fifteen_with_retire_and_restore_last`）が pin し（本数 19・末尾 3 種）、`crates/scribe2-boundary/tests/e2e/pipe/gate.rs` の歯 2 本も `STAGES` と対で `KINDS` の本数 19 を pin する（tests 全体の走査で pin はこの 2 file の 4 本だけ・verified）＝variant を足す便は同じ diff でこれらの pin を新しい本数と順へ動かす（2 file とも行 h の write-set に在る）。鮮度は `run_fresh`（rules 行 `fleet.usage_fresh_s`・[account-autonomy.md](./account-autonomy.md) §13）。窓は `WindowKind` の 3 値（`FiveHour` / `SevenDay` / `SevenDayModel`・`crates/scribe2/src/fleet/mod.rs`）。計測の CLI の口は `fleet usage`（`crates/scribe2/src/fleet/cli.rs`・方針は鮮度なし）。
  - 群の宣言は `AccountGroup`（`crates/scribe2/src/rules/manifest.rs`・`name` / `anchors` / `accounts`・`Manifest::groups`）。群の置き場の席の登録 row の口座を導く読みは doctor の `render_group`（`crates/scribe2/src/account/mod.rs`）が既に持つ。
  - rules 行の形は `rules/manifest.toml`（id / kind / value / enabled / ruling / ruled_at）で、kind は `RuleKind`（`crates/scribe2/src/rules/mod.rs`・閉じた列・56 variant）。群の閾値の行は無い。既存の `R-C9-1`（`AccountSelection`）は session 用の選定の閾値 1 値で窓を区別しない＝群の判定には使わず、触らない。
  - hook は `crates/scribe2/src/hook/mod.rs` の `dispatch` の match: SessionStart は `session_start`（§16 の `account_record` が席の実口座を測って記録する）・UserPromptSubmit は `stamped`（打刻だけ）。hook の timeout は rules 行 `hook.timeout_s`（10 秒）を写して生成される（`crates/xtask/src/genmanifest.rs`）。計測の上限 `fleet.usage_timeout_s` は 30 秒＝hook の中で同期に撃つと timeout を越えうる。
  - 器は自分を子として起こす口を持つ（1 周の `spawn_self`・待たない）。`fleet usage` の口は鮮度なし（`Freshness::Always`・宣言の全口座・旗は `--show` / `--table` / `--model` と `--rules` / `--curl` / `--claude`）で、鮮度つき（`Freshness::Within`・秒は rules 行 `fleet.usage_fresh_s`）は `fleet select` の前計測 `run_fresh` だけが持ち、口座を 1 つに絞る旗は無い。既存の歯 `fleet_select_fresh_usage_mouth_measures_every_account_regardless_of_freshness` が旗の無い `fleet usage` の鮮度なしを pin する（動かさない）。
- 形（1 つずつ歯が測る・行 h の done と 1:1）:
  1. **閾値の rules 行 3 本**: id は fleet.group_pressure_5h_pct / fleet.group_pressure_7d_pct / fleet.group_pressure_model_pct、kind は GroupPressure5hPct / GroupPressure7dPct / GroupPressureModelPct（Int・百分率）。値と裁定は user の要求そのもの（裁定 id = `user 2026-09-19T12:12Z`・裁定日 = 2026-09-19・C5・逐語は台帳）。規範の値を持つのは manifest だけで、ADR-0055 に写した 85 / 95 / 95 は裁定の写しである。行と variant は対で足す（`fleet.usage_fresh_s` と同型・§13 (2)）。読み手は約束 2 と約束 5 の 2 つ（`rules_wired` の門を通る）。
  2. **1 周の群の段**: `fire` は宣言された群ごとに、測る集合 = 群の今の口座（本便では記録が無いので種 = 候補の先頭・ADR-0049）∪ その群の置き場の席の登録 row の口座（`render_group` と同じ導き・重複は畳む）を取り、口座ごとに 3 窓の最新の実測を **1 周の置き場の event log** から読む（鮮度の規則は `run_fresh` と同じ 1 本・鮮度の外の口座は計測を 1 回撃って追記する・置き場を跨いで読まない = 鮮度は置き場ごと・C3）。どれかの窓の `used_pct` が対応する行の値以上なら**逼迫**。群 0 の host は段が 1 語も出ない。道具（`--runner`）の無い周も群の段は走る（群の段は台帳を読まない＝§5 の「起こせない周は台帳を読まない」と両立する）。
  3. **通知は群の置き場ごとに 1 行**: 逼迫の（群, 口座）ごとに 1 行を、その群の置き場すべての orchestrator の登録 row の席へ `send` と同じ口で注入する。形は `<NAME> group: pressure group=<名> account=<label> window=<5h|7d|model> used=<n> cap=<n>`（窓は越えた中で使用率が最大の 1 つ）。1 周の置き場の event log に通知の event を 1 件記す（`EventKind` に variant を 1 つ足す・群・口座・窓・値・送り先の数）。
  4. **同じ実測に 2 度通知しない**: 同じ群・口座・窓で、前回の通知の event より新しい実測が無い周は送らず記さない（event log の順序で判じ、値の比較で判じない）。
  5. **席自身の hook**: SessionStart と UserPromptSubmit で、自席の登録 row の口座と anchor の属する群（無ければ黙る・1 語も出ない）を読み、約束 2 と同じ読み手で自席の口座 1 つの 3 窓を読む。逼迫なら SessionStart は brief に、UserPromptSubmit は追加文脈に 1 行 `group=<名> account=<label> window=<w> used=<n> cap=<n> — 移動は次の 1 周（第 3 段まで手で）` を出す。**鮮度の外は hook の中で撃たず**、器自身を子として起こして待たない＝値は次の話す番で読める（hook の timeout を上げない・常駐にしない）。子の argv は `fleet usage --state-dir <D> --account <label> --fresh` の 1 形で、2 旗は本便が `fleet usage` に足す: `--account <label>` は宣言に在る 1 口座だけを測る（無い label は typed に断る・呼出 0）・`--fresh` は `fleet select` の前計測と同じ鮮度つき（新しい実測を持つ口座は測り直さない・同じ `Freshness::Within`）。旗の無い `fleet usage` は鮮度なし・全口座のまま。子を起こした周は 1 行 `usage: measuring account=<label>` を出す。
  6. **群を宣言しない host と群に属さない置き場は 1 語も変わらない**: 既存の hook の brief の外形 snapshot と dispatch の通知の歯は動かない。
  7. **`dispatch ls` は群を測らない**（観測は起こさない・§6）: 逼迫の群が在っても見る側の周は行 0・event 0・計測 0。移動は本便に無い（第 3 段・§20）。
- 触らない: 選定の純関数 `select` と `Input`・`R-C9-1`・便用の除外（§17）・doctor の群の行・群の今の口座の記録（§20）・`seat launch`・hooks.json の timeout・event の既存 variant の形。
- 歯（接頭辞ごとに 1 file・母集団は本文の件数）:
  - `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（pipe_dispatch_group_ 接頭辞・偽 usage client）: 群 0 は行 0・event 0 ／ 5 時間窓 90 の口座を種に持つ群は 2 つの置き場の席へ各 1 行 + event 1 ／ 鮮度の内側は偽 client の呼出 0・外は 1 ／ 同じ実測の 2 周目は通知 0 ／ 通知の後に同じ値の新しい実測が event log に入った周は再び通知（値の比較でなく event log の順序で判じる＝「通知済みなら永久に送らない」と「値が同じなら送らない」の 2 変異を捕まえる・便 033854Z の審査 vacuous-assert の根）／ 種でなく席の登録 row の口座が逼迫でも通知 ／ 窓ごとに閾値が別（7 日窓 90 は通知せず 96 は通知）／ 同じ fixture で dispatch ls の周は行 0・event 0・偽 client の呼出 0（約束 7）／ 席が種と違う閾値未満の口座に居て種だけが逼迫する周も（群, 種）の 1 行が出る（種の半分の pin・他の歯は席を種と同じ口座に置くので、測る集合から種を外す変異が生き残る＝便 022725Z の審査 teeth-nonvacuous の根）／ モデル別窓（fleet.group_pressure_model_pct・実測行の種類は SevenDayModel）だけが閾値以上で 5 時間窓と 7 日窓は未満の口座でも通知し、行の window は model（5 時間窓と 7 日窓の歯は model の窓を判定から落とす変異を捕まえない＝便 032913Z の審査 vacuous-assert の根・3 窓それぞれを別々の歯が pin する）／ --runner の無い周（台帳を読まない周）でも群の段は走り、逼迫の種に 1 行 + event 1（他の歯は --runner つきの終端の周で撃つ＝道具の有無の両側を別々の歯が pin する・便 034702Z の審査 vacuous-assert の根）＝ 11 本。
  - `crates/scribe2-boundary/tests/e2e/hook.rs`（hook_group_ 接頭辞）: SessionStart の brief に 1 行 ／ UserPromptSubmit の追加文脈に 1 行 ／ 群に属さない anchor は 0 行 ／ 閾値未満は 0 行 ／ 鮮度の外は子を 1 本起こして待たず measuring の 1 行 ／ 席の登録 row の口座が種と違い、席の口座だけが逼迫する周も 1 行（自席の口座を読む＝種を読む変異を捕まえる）／ モデル別窓だけが閾値以上の口座でも 1 行で window は model（5 時間窓と 7 日窓の歯は model を落とす変異を捕まえない・便 035919Z の審査 vacuous-assert の根・dispatch 側と同じ両側の pin）／ 窓ごとに閾値の行が別（7 日窓 90 は 0 行・96 は 1 行＝5 時間窓の行を他の窓に当てる変異を捕まえる・便 040716Z の審査 vacuous-assert の根）＝ 8 本。
  - `crates/scribe2-boundary/tests/e2e/rules.rs`（rules_embedded_manifest_declares_group_pressure_ 接頭辞・3 本 + 外形 `rules_external_form` の snapshot 更新）。
  - `crates/scribe2-boundary/tests/e2e/fleet.rs`（fleet_usage_narrowed_ 接頭辞・`fleet usage` の 2 旗）: `--account` は名指した 1 口座だけ測り他の口座は呼出 0 ／ 宣言に無い label は typed な断りで呼出 0 ／ `--fresh` は新しい実測を持つ口座を測り直さず鮮度の外だけ測る（旗の無い口は既存の歯が全口座を pin・便 041416Z の審査 section-material-missing の根）＝ 3 本。
- 却下: hook の中で同期に計測する（`hook.timeout_s` 10 秒 < `fleet.usage_timeout_s` 30 秒・timeout の行を上げるには裁定が要り、話す番が API の待ちぶん延びる）／群の今の口座だけを測る（記録が無い本便では種しか見えず、席が実際に居る口座を見落とす）／閾値を 1 行にする（窓ごとに裁定の値が違う）／通知を dispatch ls にも出す（見る口が起こす口になる・§6）／使用率を置き場を跨いで集める（event log の写しが増える・C3）。
- 後続: 第 3 段（§20）= 群の今の口座の記録・移動を頼む記録・自動の移動。

## 20. 群の自動の移動（第 3 段）— 群の今の口座を host の根の記録で持ち、逼迫した群は 1 周が lock の内側で移り先を 1 回決めて記録と承認 event を書き、退避の合図の後に同じ target へ新しい口座の席を起こす（契約表の行 i・§19 の続き・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html) §2・[ADR-0055](../../design-intent/decisions/ADR-0055-group-pressure-is-measured-at-run-ends-and-seat-turns-without-a-timer.html)）

やさしく言うと: 知らせるだけ（§19）から、器が自分で移す段へ。かたまりの「今の口座」を machine 共通の置き場に 1 件の記録で持ち、残りが線を越えたら、鍵をかけて移り先を 1 回だけ決め、古い席に「まとめて終えて」と伝え、同じ窓に新しい口座で席を起こし直す。対話の記録は運ばず、続きは器が台帳と git から組み直す。

- 出所: §19 と同じ（ADR-0049 §2 の第 3 段・ADR-0055 の契機）。
- 現物（verified・main）:
  - host の根は `host_slots_dir`（`crates/scribe2/src/seat/mod.rs`・state dir の親の下の host 用 dir・env を読まない）。lock の実装は 1 本（`create_new`・`crates/scribe2/src/pipe/admission.rs` の受付の lock と同じ）。
  - 群の今の口座を置く型も reader も無い（`AccountGroup` は宣言値だけ・`render_group` は 1 件も書かない・読まない）。
  - 席の起動は `crates/scribe2/src/seat/cycle/launch.rs` の `launch`（`pick_account` = `--account` か session 用の選定 `choose`〔`crates/scribe2/src/seat/cycle/relaunch.rs`〕→ `prepare` が登録 row を書く → 窓が無ければ作る）。短い形は `crates/scribe2/src/seat/cli.rs`（§14 / §18）。席を終える口は seat に無い（`/exit` は席の側が打つ・§18 の現物）。
  - 承認の event は `crates/scribe2/src/pipe/approve.rs` の `record_words`（`ApprovalReceived`・逐語を持つ）。移動・断り・保留の event の variant は §19 と同じく `crates/scribe2/src/fleet/mod.rs` の `EventKind` に足し、網羅 match（`crates/scribe2/src/fleet/event.rs`・`crates/scribe2/src/fleet/replay.rs`）を揃え、§19 の現物と同じ `KINDS` の pin の歯 4 本（`crates/scribe2-boundary/tests/e2e/fleet.rs`・`crates/scribe2-boundary/tests/e2e/pipe/gate.rs`）を新しい本数と順へ動かす。
- 形（1 つずつ歯が測る・行 i の done と 1:1）:
  1. **群の今の口座の記録**: host の根の下の群用 dir に群ごとに高々 1 file（口座 label・ts・理由 = move・前の口座）。書くのは 1 周の群の段だけで、lock（群用 dir の 1 file・`create_new`）の内側で一時 file → rename。書き換える周は前の記録を消さず履歴の側へ move する（N1.2・ADR-0041 の形）。
  2. **群の今の口座の解決は 1 関数**: 記録が在ればその label・無ければ種（宣言の候補の先頭）・在るのに読めなければ typed に止まる（ADR-0049）。§19 の測る集合の「今の口座」はこの解決値に替わる（種の読みはこの関数の中に残る）。doctor の群の行に `current=<label|seed>` を 1 項目足す（群 0 の host の外形は不変）。
  3. **群の置き場の席は群の今の口座で起きる**: `seat launch` と短い形は、anchor が群に属する周は session 用の選定を撃たず解決値で起き、`--account` / `<label>` が解決値と違う周は使い方の誤りで断る（rc 1・row 0・群の外に席を置かない）。群に属さない anchor は今のまま。
  4. **移動を頼む記録**: §19 の hook が逼迫を読んだ周に、群用 dir に群ごとに高々 1 file（ts・口座・窓）を置く（在れば上書きしない）。1 周の群の段はこれが在れば鮮度に依らず計測を撃ち、判定の後に履歴の側へ move する。
  5. **移動の判定は lock の内側で 1 回**: 群の今の口座が逼迫（§19 の判定）なら、移り先 = 宣言の候補の順で、他の群の今の口座でない ∧ 1 周の置き場の live 便が使っていない ∧ 3 窓とも閾値未満の実測（鮮度の内側）を持つ、最初の label。無ければ移らず、断りの event を 1 件記して席へ 1 行 `<NAME> group: move-refused group=<名> reason=no-candidate`（妥協の移動を作らない・ADR-0020 §2.4）。
  6. **移動の執行**（同じ lock の内側・この順）: 記録を書く（約束 1）→ 承認 event を 1 件（`record_words` と同じ形・逐語は群の宣言の行 = 常設の承認・A1）→ 群の置き場ごとに orchestrator の登録 row が在る席へ退避の合図 1 行 `<NAME> group: evacuate group=<名> to=<label> — 作業記憶を台帳と git に残して /exit` を注入 → 席の pane が shell に戻るのを起動と同じ窓（`seat.cycle_settle_s`）で待ち、戻った置き場から順に `launch` の 1 本で新しい口座の席を**同じ target** に起こす（登録 row は起動が書き直す・会話は運ばない・復帰は SessionStart の brief と §16）。窓の内に戻らない席は保留の event を記し、次の 1 周が記録（新しい口座）と登録 row（古い口座）の食い違いから同じ手を続ける（冪等・判定はやり直さない）。移動した周は §19 の通知を送らない（退避の合図が代わる）。
  7. **周の中の順序**: 群の段（通知 → 移動）は便の列の前に走り、群の段の失敗は便の列の rc を変えない（§5 の終端の 1 周と同型）。
  8. **群 0 の host は 1 語も変わらない**。
- 触らない: 宣言の表 `[[account-group]]`（人が書く宣言と器が書く現状を分ける・ADR-0049）・便用の除外（§17）・選定の純関数・`R-C9-1`・hooks.json・§18 の `-c` / `-r`（機械の復帰は会話を運ばない・ADR-0049）。
- 歯: `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（pipe_dispatch_group_move_ 接頭辞・偽 tmux と偽 usage）: 逼迫 + 候補ありで記録 1・承認 event 1・退避の 1 行・新しい席の起動行 1 ／ 候補なしは記録 0・断りの event 1・1 行 ／ 他の群の今の口座は候補から外れる ／ 2 周目は判定を繰り返さず保留の席だけ起こす ／ 移動を頼む記録が在る周は鮮度の内側でも計測 1 ＝ 5 本。`crates/scribe2-boundary/tests/e2e/seat/launch.rs`（seat_launch_group_ 接頭辞）: 群の anchor は解決値で起き選定を撃たない ／ 違う label は rc 1・row 0 ／ 群の外の anchor は今のまま ＝ 3 本。`crates/scribe2-boundary/tests/e2e/seat/account.rs`（host_group_record_ 接頭辞）: doctor の current= が記録 / seed を映す ／ 読めない記録は typed に止まる ＝ 2 本。
- 却下: 席が自分を起こし直す（ADR-0055 OPT3・殺到と権能の柵）／別の window に新しい席を起こす（target が 2 つになり登録 row の鍵が割れる・§14 の短い形と食い違う）／古い席の process を kill する（作業記憶が退避されない・N1）／記録を宣言 file に書き戻す（宣言と現状を分ける・ADR-0049）／移動の判定を hook で行う（群単位の 1 回にならない）／古い席が戻らない周に別口座へ 2 度目の移動をする（判定は 1 回・冪等の続きだけ）。
- write-set の注: 行 h が `+` で足す 2 file は、行 h の着地前は base に無いので本行も `+` で宣言する。行 h の着地後に素の path へ直す（受付は着地済みの file の `+` を断る）。
- 後続: 席の登録 row を退役する口（古い置き場の row が便用の除外に残る・`s2-07l.494` の notes）／群の宣言の A1 承認の 1 回目を doctor に見せる形は別便。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "役割なしの起動 — account shell が derive_launch を再利用し登録 row を書かずに起動行を注入する"
req = ["FR60", "FR59", "FR58"]
section = "13"
write-set = ["crates/scribe2/src/account/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/seat/cycle.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail account_cmd_shell_", "cargo nextest run -p scribe2 --no-tests=fail seat_launch_", "cargo nextest run -p scribe2 --lib --no-tests=fail account_cmd_errors_"]
size = "S"
done = "偽 tmux と偽 claude で account shell が登録 row 0 のまま起動行を 1 回だけ差し込み、resume と拒否 3 種が typed に出る"

[[contract]]
id = "b"
title = "席の起動の短い形 seat <label> --planner|--admin — 置き場は git 設定、target と model は登録 row から導き、長い形と同じ 1 経路を通る"
req = ["FR59", "FR40"]
section = "14"
write-set = ["crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_launch_short_", "cargo nextest run -p scribe2 --no-tests=fail seat_usage_external_form"]
size = "S"
done = "登録 row の在る anchor で短い形が長い形と同じ row と起動行を作り、row も flag も無い周は defaults-unresolved で 1 key も送らず、既知の verb は従来どおり通る"

[[contract]]
id = "c"
title = "口座の OAuth 墓標を器が名指す — probe の credential= を present / missing / dead の 3 値にし、tick は墓標の周を account-dead に弁別して連続 N 周で planner の席へ NEEDS-USER の 1 行を注入する"
req = ["FR33", "FR38"]
section = "15"
write-set = ["crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/usage.rs", "rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "docs/design/rules-manifest.md", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail account_credential_dead_", "cargo nextest run -p scribe2 --no-tests=fail seat_tick_account_dead_"]
size = "M"
done = "墓標の credential の口座が account ls / doctor で credential=dead と出て、その口座の席の tick が account-dead を記録し N 周目に planner の席へ NEEDS-USER の 1 行を注入し、429 の unmeasured は従来どおり鳴らず、rules 行が裁定 id 付きで 1 本増える"

[[contract]]
id = "d"
title = "席の実口座の記録 — SessionStart hook が入力 JSON の transcript_path から実口座（当たらない周は unknown）を測って席の打刻 dir に 1 file 1 行で毎回上書きし、row も state.jsonl も書かない"
req = ["FR42", "FR40"]
section = "16"
write-set = ["crates/scribe2/src/hook/mod.rs", "+crates/scribe2/src/seat/session_account.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_account_mismatch_record_"]
size = "S"
done = "偽の transcript_path で SessionStart が実口座（置き場の accounts の外・. 始まり・key 無しは unknown）を席の打刻 dir に上書きで記録して前の値を残さず、pane を解けない周は書かず、登録 row と state.jsonl と指示文は 1 byte も変わらず、記録を読んで合図を撃つ・席を止める経路は無い"

[[contract]]
id = "e"
title = "席の実口座の見える化 — SessionStart の指示文に row の口座・実測の口座・照合の 1 行を出す"
req = ["FR42", "FR40"]
section = "16"
write-set = ["crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/seat/brief/mod.rs", "crates/scribe2/src/seat/brief/orchestrator.txt", "crates/xtask/src/seat_brief.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__hook__hook_brief_orchestrator.snap", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_account_mismatch_shown_", "cargo nextest run -p scribe2 --no-tests=fail hook_brief_", "cargo nextest run -p scribe2 --lib --no-tests=fail seat_brief_holes_", "cargo nextest run -p xtask --no-tests=fail seat_brief_"]
size = "S"
done = "登録 row の在る席の SessionStart の指示文に row の口座・実測の口座・照合（match / mismatch / unknown）の 1 行が直し方の pointer 付きで出て、雛形の穴は core と xtask で同じ数に揃う"

[[contract]]
id = "f"
title = "席の口座を持つ単位は project の群（第 1 段）— host の面に群の宣言の表を 1 つ足して既存の読み手と拒否の形で読み、便用の選定の 2 つの口が群の候補の口座を host 全体で外し、doctor が群ごとに 1 行を出す（群を宣言しない host は無変更・移動と記録は作らない）"
req = ["FR57", "FR36", "NFR4"]
section = "17"
write-set = ["crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "crates/scribe2-boundary/tests/e2e/seat/rules.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail host_group_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail rules_host_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail fleet_select_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail doctor_accounts_"]
size = "M"
done = "(1) host の面に群の表が 1 つ増え、名・置き場の列・候補の口座 label の列の 3 key を既存の読み手 1 本が読み、file の無い host は 0 群で続き、読めない host は typed に止まる (2) tracked の面に群の表が在る周は未知の表として行番号付きで断る (3) 同じ名が 2 行・同じ置き場が 2 つの群・宣言に無い label・置き場の列が空・候補の列が空・未知の key の 6 種を行番号付きで全件断り、面の中で止まった周は合わせの検査へ進まない (4) 便用の選定の候補から宣言のどの群の候補 label も外れ、便の置き場の席の登録 row の除外はそのまま残り、除外は次の選定から効いて走行中の便は止まらない (5) 同じ除外が便用の候補を作る 2 つの口の両方で効く (6) session 用の選定の候補には群の口座が残り、便用の並べ順は変わらず、群の今の口座の記録は 1 件も書かれない (7) doctor が宣言された群 1 つにつき 1 行を宣言順で出し、名・候補の label の列・置き場の数・その群の置き場の席の登録 row の口座 label の列（無ければ無しの語）を載せて判定しない (8) 群を 1 つも宣言しない host は群の行が 0 本で便用の候補も今のままで、doctor の既存の外形 snapshot が 1 行も動かない"

[[contract]]
id = "g"
title = "席の起動の短い形が会話を運ぶ — seat <label> [-c|-r ID] で注入する起動行の末尾に --continue / --resume ID を足す（登録 row と .launch は旗無しのまま・両方や値無しは使い方 rc 1・役割の flag は省略可のまま）"
req = ["FR59", "FR40"]
section = "18"
write-set = ["crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_launch_short_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail seat_usage_external_form"]
size = "S"
done = "(1) seat <label> -c（--continue）で inject.jsonl の注入行の末尾が --continue になり、登録 row の launch と置き場の .launch に --continue が無い (2) seat <label> -r ID（--resume ID）で注入行の末尾が --resume ID になり row の launch に無い (3) -c と -r の両方・値の無い -r・同じ flag の重複・会話 id の形（16 進小文字と - だけの UUID）でない -r の値（空白・$( を含む・- 始まり）は使い方 rc 1 で key 0・row 0 (4) 役割の flag 0 個は今までどおり orchestrator で通り、既存の seat_launch_short_ の歯 4 本は 1 字も変わらず緑 (5) usage の 1 行に [-c|-r ID] が増えて外形 snapshot が更新され、tests/e2e/seat.rs の diff は 0 行"
[[contract]]
id = "h"
title = "群の逼迫の通知（第 2 段）— 閾値の rules 行 3 本（5 時間窓 / 7 日窓 / モデル別窓・裁定 id つき）を足し、dispatch の 1 周が群ごとに今の口座と席の口座の使用率を鮮度つきで読んで逼迫の群の置き場の席へ 1 行を注入し event に 1 件記し、席自身の hook が自席の口座を同じ読み手で読んで指示文で告げる（鮮度の外は子を起こして待たない・群 0 の host は無変更・移動は作らない）"
req = ["FR36", "FR38", "NFR4"]
section = "19"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/notify.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_group_pressure_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_usage_narrowed_"]
size = "M"
done = "(1) 埋め込みの manifest が群の閾値の rules 行 3 本を値・裁定 id・裁定日つきで持ち、kind の variant と対で足され、外形 snapshot の rows= が 3 増える (2) 1 周が宣言された群ごとに今の口座（種）と群の置き場の席の登録 row の口座の 3 窓の最新の実測を 1 周の置き場の event log から鮮度つきで読み（席が種と違う口座に居る周も種を読む＝種だけが逼迫する周に 1 行・種と席の口座の両側を別々の歯が pin する）、鮮度の外の口座だけ計測を 1 回撃ち、どれかの窓が行の値以上なら逼迫と判じ（3 窓それぞれを別々の歯が pin し、モデル別窓だけが越える口座も通知して window は model）、群 0 の host は 1 語も出さず、runner の無い周も群の段は走る（runner つきの終端の周と runner の無い周の両側を別々の歯が pin する） (3) 逼迫の（群, 口座）ごとに 1 行を群の置き場すべての orchestrator の席へ注入し、EventKind に足した通知の variant で event を 1 件記す (4) 同じ群・口座・窓で前回の通知より新しい実測が無い周は送らず記さず、通知の後に同じ値でも新しい実測が入った周は再び通知する（送らない側と再び送る側を別々の歯が pin する） (5) SessionStart と UserPromptSubmit の hook が自席の登録 row の口座（種と違う周は席の口座・別々の歯が pin）と anchor の群を読み、3 窓それぞれを別々の歯が pin し、窓ごとに別の行の値を当て（7 日窓 90 は 0 行・96 は 1 行）、逼迫なら brief / 追加文脈に 1 行を出し、群に属さない anchor と閾値未満は 0 行で、鮮度の外は器を子として起こして待たず measuring の 1 行を出し、子の argv は fleet usage --state-dir D --account <label> --fresh の 1 形（本便が足す 2 旗＝--account は宣言の 1 口座だけ・--fresh は select の前計測と同じ鮮度つき・旗の無い口は鮮度なしのまま）で、2 旗の意味は fleet.rs の歯 3 本が pin する (6) 群を宣言しない host は hook の brief の外形 snapshot と dispatch の通知の既存の歯が動かない (7) 逼迫の群が在る fixture で dispatch ls の周は行 0・event 0・偽 client の呼出 0 で、群の段は起こす側の 1 周だけが持つ"

[[contract]]
id = "i"
title = "群の自動の移動（第 3 段）— 群の今の口座を host の根の記録（群ごとに高々 1 件・履歴へ move）で持ち、解決は記録 > 種の 1 関数、群の置き場の席は解決値で起き、hook は移動を頼む記録を置き、逼迫した群は 1 周が lock の内側で移り先（他の群の今の口座でない ∧ live 便が使っていない ∧ 3 窓とも閾値未満）を 1 回決めて記録と承認 event を書き、退避の合図の後に同じ target へ新しい口座の席を起こす（候補なしは断りの event・群 0 の host は無変更）"
req = ["FR36", "FR38", "FR59", "NFR4"]
section = "20"
write-set = ["crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/cycle/relaunch.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_move_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_record_"]
size = "L"
growth = ["crates/scribe2/src/pipe/dispatch.rs:20", "crates/scribe2/src/fleet/mod.rs:40", "crates/scribe2/src/account/mod.rs:60"]
done = "(1) 群の今の口座の記録が host の根の群用 dir に群ごとに高々 1 file で在り、1 周の群の段だけが lock の内側で一時 file → rename で書き、書き換える周は前の記録を履歴へ move する (2) 解決の 1 関数が記録 > 種の順で返し、読めない記録は typed に止まり、doctor の群の行に current= が増えて群 0 の host の外形は不変 (3) 群の anchor の seat launch と短い形は選定を撃たず解決値で起き、違う label は rc 1・row 0 で断り、群の外の anchor は今のまま (4) hook が逼迫の周に移動を頼む記録を高々 1 file 置き、1 周はそれが在れば鮮度に依らず計測を撃って判定の後に履歴へ move する (5) 逼迫した群の移り先を宣言の候補の順で他の群の今の口座でない ∧ live 便が使っていない ∧ 3 窓とも閾値未満の最初の label に 1 回だけ決め、無ければ記録 0・断りの event 1・席へ 1 行 (6) 移動の周は記録 → 承認 event（逐語 = 宣言の行）→ 退避の合図 → settle の窓で shell に戻った置き場から同じ target へ新しい口座の席を launch の 1 本で起こし、戻らない席は保留の event を記して次の 1 周が判定を繰り返さず続きだけ行い、移動した周は通知を送らない (7) 群の段は便の列の前に走り、失敗は便の列の rc を変えない (8) 群 0 の host は 1 語も変わらない"
<!-- contracts:end -->
