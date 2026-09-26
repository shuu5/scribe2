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
  4. **便用の選定が群の候補の口座を host 全体で外す**: 便用の候補から、宣言のどの群の候補の label も外す。除外は**次の選定から**効き、走行中の便は止めない。便の置き場の席の登録 row の除外（[account-autonomy.md](./account-autonomy.md) §14）はそのまま残り、群の除外がその上に重なる。**§23（行 l）で改めた**: 外すのは群ごとの今の口座（記録 > 種）だけで、候補の残りは便用の候補に残る。
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
  - 承認の逐語の持ち方は `crates/scribe2/src/pipe/approve.rs` の `record_words` に倣う（逐語の field）が、**event の kind は便の承認 `ApprovalReceived` ではなく本行が足す移動の variant**（群の移動は run / bead を持たず、`ApprovalReceived` は run と bead が必須で replay が run を承認済みにする＝使わない・`approve.rs` と `admission.rs` は触らない〔便 215412Z の審査 section-material-missing の根〕）。移動・断り・保留の event の variant は §19 と同じく `crates/scribe2/src/fleet/mod.rs` の `EventKind` に足し、網羅 match（`crates/scribe2/src/fleet/event.rs`・`crates/scribe2/src/fleet/replay.rs`）を揃え、§19 の現物と同じ `KINDS` の pin の歯 4 本（`crates/scribe2-boundary/tests/e2e/fleet.rs`・`crates/scribe2-boundary/tests/e2e/pipe/gate.rs`）を新しい本数と順へ動かす。
- 形（1 つずつ歯が測る・行 i の done と 1:1）:
  1. **群の今の口座の記録**: host の根の下の群用 dir に群ごとに高々 1 file（口座 label・ts・理由 = move・前の口座）。書くのは 1 周の群の段だけで、lock（群用 dir の 1 file・`create_new`）の内側で一時 file → rename。書き換える周は前の記録を消さず履歴の側へ move する（N1.2・ADR-0041 の形）。
  2. **群の今の口座の解決は 1 関数**: 記録が在ればその label・無ければ種（宣言の候補の先頭）・在るのに読めなければ typed に止まる（ADR-0049）。§19 の測る集合の「今の口座」はこの解決値に替わる（種の読みはこの関数の中に残る）。doctor の群の行に `current=<label|seed>` を 1 項目足す（群 0 の host の外形は不変）。
  3. **群の置き場の席は群の今の口座で起きる**: `seat launch` と短い形は、anchor が群に属する周は session 用の選定を撃たず解決値で起き、`--account` / `<label>` が解決値と違う周は使い方の誤りで断る（rc 1・row 0・群の外に席を置かない）。群に属さない anchor は今のまま。
  4. **移動を頼む記録**: §19 の hook が逼迫を読んだ周に、群用 dir に群ごとに高々 1 file（ts・口座・窓）を置く（在れば上書きしない）。1 周の群の段はこれが在れば鮮度に依らず計測を撃ち、判定の後に履歴の側へ move する。
  5. **移動の判定は lock の内側で 1 回**: 群の今の口座が逼迫（§19 の判定）なら、移り先 = 宣言の候補の順で、今の口座でなく ∧ 他の群の今の口座でなく ∧ 退役中でなく ∧ 3 窓とも閾値未満の実測（鮮度の内側）を持つ、最初の label（§27 形 1 の改めの後の形: 稼働中の便が使う口座は候補から外さず、新規の便は §23 の記録で止まる）。「他の群の今の口座」は**同じ周で先に移った群の移り先を含む**（周の中で更新する＝周の頭の記録だけで判じない・2 群が同じ周に同じ label へ移らない）。無ければ移らず、断りの event を 1 件記して席へ 1 行 `<NAME> group: move-refused group=<名> reason=no-candidate`（妥協の移動を作らない・ADR-0020 §2.4）。**断った周は §19 の通知を送らない**（断りの 1 行が代わる・席への行は群の置き場ごとに高々 1 行）。
  6. **移動の執行**（同じ lock の内側・この順）: 記録を書く（約束 1）→ 承認 event を 1 件（kind は本行が足す移動の variant・account = 移り先・detail = 群の宣言の行の逐語〔host の面の行番号つき〕= 常設の承認・A1・actor は machine で run / bead を持たない）→ 群の置き場ごとに orchestrator の登録 row が在る席へ退避の合図 1 行 `<NAME> group: evacuate group=<名> to=<label> — 作業記憶を台帳と git に残して /exit` を注入 → 席の pane が shell に戻るのを起動と同じ窓（`seat.cycle_settle_s`）で待ち、戻った置き場から順に `launch` の 1 本で新しい口座の席を**同じ target** に起こす（登録 row は起動が書き直す・会話は運ばない・復帰は SessionStart の brief と §16）。窓の内に戻らない席は保留の event を記し、次の 1 周が記録（新しい口座）と登録 row（古い口座）の食い違いから同じ手を続ける（冪等・判定はやり直さない）。移動した周は §19 の通知を送らない（退避の合図が代わる）。
  7. **周の中の順序**: 群の段（通知 → 移動）は便の列の前に走り、群の段の失敗は便の列の rc を変えない（§5 の終端の 1 周と同型）。
  8. **群 0 の host は 1 語も変わらない**。
- 触らない: 宣言の表 `[[account-group]]`（人が書く宣言と器が書く現状を分ける・ADR-0049）・便用の除外（§17）・選定の純関数・`R-C9-1`・hooks.json・§18 の `-c` / `-r`（機械の復帰は会話を運ばない・ADR-0049）。
- 歯: `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（pipe_dispatch_group_move_ 接頭辞・偽 tmux と偽 usage）: 逼迫 + 候補ありで記録 1・承認 event 1・退避の 1 行・新しい席の起動行 1 ／ 候補なしは記録 0・断りの event 1・席の pane への行は群の置き場ごとに 1 行だけ（§19 の通知の行は 0・全 send の行数で数える）／ 他の群の今の口座は候補から外れる ／ 同じ周に 2 群が逼迫し候補を共有する周は後の群が先の群の移り先を飛ばす（記録 2 の label が異なる・周の頭の記録だけを読む変異を捕まえる）／ 2 周目は判定を繰り返さず保留の席だけ起こす ／ 移動を頼む記録が在る周は鮮度の内側でも計測 1・判定の後にその記録が群用 dir から履歴の側へ move され（群用 dir の頼みの file 0・履歴 1）、同じ fixture の 2 周目は頼みの記録が無いので鮮度の内側の計測 0（move せず残す変異を捕まえる）／ 移動の周の 4 手の順（記録の書き → 承認 event → 退避の合図 → 起動）を偽 tmux と event log と launched の記録の時刻の並びで pin し、承認 event が記録より先・合図が承認より先・起動が合図より先のどれも起きない（順を入れ替える変異を捕まえる）／ 群を宣言しない host の fixture で起こす側の周を撃つと群用 dir が作られず記録 0・event 0・送りの行 0・起動行 0 で便の列の rc と dispatch ls の外形は不変（群 0 の host は 1 語も変わらない・done (8)）／ 2 周目の書き換えで前の記録が履歴の側へ move し群用 dir の記録は高々 1 file（形 1 の履歴）／ 候補の順で先の label を置き場の live 便が使っている周（fleet の inflight）はその label を飛ばして次へ移り、live 便が無ければ先の label へ移る（両側）／ 候補の順で先の label の 3 窓のどれか 1 つ（5 時間窓・7 日窓・モデル別窓を別々の歯で）が閾値以上ならその label を飛ばし、3 窓とも未満の次の label へ移る／ lock の file が残る周は群の段が typed で止まり記録 0・event 0 で便の列の rc は変わらない（形 1 の lock と形 7）＝ 14 本（便 220418Z の審査の根: live 便と 3 窓の条件を測る歯が無く、条件を外す変異が全行を通る／便 222835Z の審査の根: done (4) の履歴への move・done (6) の 4 手の順・done (8) の群 0 の不変を測る歯が無かった）。`crates/scribe2-boundary/tests/e2e/seat/launch.rs`（seat_launch_group_ 接頭辞）: 群の anchor は解決値で起き選定を撃たない ／ 違う label は rc 1・row 0 ／ 群の外の anchor は今のまま ＝ 3 本。`crates/scribe2-boundary/tests/e2e/seat/account.rs`（host_group_record_ 接頭辞）: doctor の current= が記録 / seed を映す ／ 読めない記録は typed に止まる ＝ 2 本。`crates/scribe2-boundary/tests/e2e/hook.rs`（hook_group_move_ 接頭辞・§19 の hook_group_ の歯と同じ fixture）: 逼迫の周に移動を頼む記録が群用 dir に 1 file（ts・口座・窓）置かれ、2 度目の hook は上書きしない（1 file・ts 不変）／ 閾値未満の周と群の外の anchor は 0 file ＝ 2 本（形 4 の hook の側）。
- 変更する既存の歯（名で数える・実装の便の実測）: §19 の dispatch の通知の歯のうち群の今の口座（種）を逼迫にしていた 8 本は、種を閾値未満の口座（候補の先頭・鮮度の内側の実測）に替えて席の登録 row の口座を逼迫にする（形 5 / 6 のとおり今の口座の逼迫は移動の周になり通知を送らない）。種だけが逼迫する歯（pipe_dispatch_group_seed_pressure_is_notified_while_the_seats_sit_elsewhere）は移動の周の形（記録は候補の次・通知 0・席は既に移り先に居るので送り 0）を測る。§19 の hook の歯 8 本は置き場を tmp の 1 段下に置く群の歯用の fixture へ載せ替える（host の根は置き場の親の下なので、tmp の根の直下の置き場では頼みの記録が歯どうしで共有され tmp の根に残る・assert は不変）。doctor の群の行の歯 3 本（host_group_doctor_ 接頭辞）は行の末尾に current=seed が増える（形 2）。KINDS の pin の歯 4 本は 20 → 23・末尾に移動の 3 種。seat_launch_group_ の 3 本は本物の tmux を使わず偽 tmux の script だけで撃つ（nextest の tmux の群に名を足さない）。
- 却下: 席が自分を起こし直す（ADR-0055 OPT3・殺到と権能の柵）／別の window に新しい席を起こす（target が 2 つになり登録 row の鍵が割れる・§14 の短い形と食い違う）／古い席の process を kill する（作業記憶が退避されない・N1）／記録を宣言 file に書き戻す（宣言と現状を分ける・ADR-0049）／移動の判定を hook で行う（群単位の 1 回にならない）／古い席が戻らない周に別口座へ 2 度目の移動をする（判定は 1 回・冪等の続きだけ）。
- write-set の注: 行 h が `+` で足す 2 file は、行 h の着地前は base に無いので本行も `+` で宣言する。行 h の着地後に素の path へ直す（受付は着地済みの file の `+` を断る）。
- 後続: 席の登録 row を退役する口（古い置き場の row が便用の除外に残る・`s2-07l.494` の notes）／群の宣言の A1 承認の 1 回目を doctor に見せる形は別便。

## 21. 退避を器が完結させる（第 3 段の続き）— 保留の席へ器が /exit を送り、席の hook は記録の口座と登録 row の食い違いを告げ、群の置き場の席の断りは次の 1 手を持つ（契約表の行 j・§20 の続き・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html) §2・[ADR-0055](../../design-intent/decisions/ADR-0055-group-pressure-is-measured-at-run-ends-and-seat-turns-without-a-timer.html)・`s2-07l.604`）

やさしく言うと: §20 の移動は「古い席に /exit を頼む」ところで止まった。席（AI）は自分の process を終えられない。器が代わりに /exit を送り、席には「いま移動中・作業記憶を残して待て」と正しい 1 行を出し、人が別の口座で席を撃ったときは「どの口座なら通るか」を断りの行に添える。

- 出所: 台帳 `s2-07l.604`（memo・2026-09-24 の実測: 05:17:30Z に `GroupMoved` の後、席の pane が shell に戻らず `GroupMovePending reason=input-unknown` が 5 件・席の hook は登録 row の口座で「第 3 段まで手で」を出し続け・人の短い形は `group-account` で断られた）。
- 現物（verified・main cbf1d7d）:
  - 移動の周は `crates/scribe2/src/pipe/dispatch/group.rs` の `execute` が記録 → `GroupMoved` → 退避の 1 行（`notify::send`・`crates/scribe2/src/pipe/notify.rs` の §19 と同じ口）→ `relaunch`（`Wait::Settle`）の順で撃ち、続きの周は `relaunch`（`Wait::Once`）が `pane_is_shell`（`crates/scribe2/src/seat/mod.rs`）の席だけ起こす。pane が shell でない席には何も送らない（保留の event を重ねない）＝席が /exit を打たない限り永久に保留。
  - 席の hook の群の段は `crates/scribe2/src/hook/group.rs` の `line_of` が**登録 row の口座**だけを読み（`registration_of_target`）、逼迫なら `seat_line` の固定の字面「移動は次の 1 周（第 3 段まで手で）」を出す。群の今の口座（`current_of`・記録 > 種）は読まない＝移動済みの席に古い口座の逼迫を告げ続ける。
  - 群の置き場の席の起動は `crates/scribe2/src/seat/cycle/launch.rs` の `pick_account` が `current_of` と違う label を `REASON_GROUP_ACCOUNT` で断り、行は `render_launched` の `Launched::Refused` の腕（`next=` を持つのは `not-a-shell` だけ）。`seat launch` の短い形の呼び手は `crates/scribe2/src/seat/cli.rs`（`render_launched` を 4 か所で呼ぶ）。
  - 偽 tmux の fixture（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`）は `send-keys -l` の payload を入力欄の file に写し、`list-panes` の `pane_current_command` で shell / claude を作り分ける（§20 の歯が使う）。
- 形（1 つずつ歯が測る・行 j の done と 1:1）:
  1. **器が /exit を送る**: 続きの周（記録の口座 ≠ 登録 row の口座の群・`Wait::Once`）で pane が shell でない席には、退避の合図と同じ口（`notify::send`）で `/exit` の 1 行を 1 回だけ送り、その周は起こさない（送りは inject の記録に残る・保留の event は重ねない）。次の周に shell に戻っていれば §20 形 6 のとおり同じ target へ起こす。移動の周（`Wait::Settle`）は §20 のまま（退避の合図の直後には送らない＝席が作業記憶を残す番を 1 周ぶん持つ）。/exit で dialog が出て止まる席（描画は読まない）は次の周も shell でないので同じ 1 行をもう 1 回送る（周ごとに 1 回・上限は置かない＝人が窓を見れば分かる・ADR-0055 の柵の内側）。
  2. **hook の 1 行を記録で組む**: `line_of` は登録 row の口座に加えて群の今の口座（`current_of`）を読み、(a) 記録の口座 ≠ 登録 row の口座の周は逼迫を測らず `group=<名> row=<row の label> current=<記録の label> — 器が移動中: 作業記憶を台帳と git に残して待つ（/exit は器が送る）` の 1 行（移動を頼む記録は置かない）、(b) 一致して逼迫の周は `group=<名> account=<label> window=<w> used=<n> cap=<n> — 次の 1 周が移り先を決める` の 1 行（§19 形 5 の字面の後半を置き換える・「第 3 段まで手で」の語は消える）、(c) それ以外は §19 のまま（0 行か measuring）。記録が読めない周は 0 行（席は止めない・§19 の極性）。
  3. **断りに次の 1 手**: `reason=group-account` の断りの行は `next=seat <記録の label> -c` を置き場の 2 語の前に足す（`not-a-shell` の `next=` と同じ位置・label は `current_of` の解決値・短い形と長い形の両方）。他の断りの行は 1 字も変わらない。
  4. **群 0 の host と群に属さない anchor は 1 語も変わらない**。
- 触らない: §20 の判定（移り先の 3 条件・lock・記録の形）・`GroupMoved` / `GroupMovePending` / `GroupMoveRefused` の kind と `EventKind` の列（event の種類は足さない・detail の語だけ）・`Launched` の variant・`seat.cycle_settle_s`・§18 の `-c` / `-r`・§19 の通知の判定と `fleet usage` の 2 旗・便の列。
- 歯（置き場は既存の file・接頭辞ごとに 1 file）:
  - `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（`pipe_dispatch_group_exit_` 接頭辞・偽 tmux と偽 usage・§20 の fixture）: 記録 ≠ row の群の続きの周で pane が claude の席は `/exit` の payload の送りが 1 行・起動行 0・保留の event は増えない ／ 同じ席は次の周も shell でなければもう 1 行（周ごとに 1 回） ／ shell に戻った周は送り 0・起動行 1 ／ 移動の周（記録を書いた同じ周）は退避の 1 行だけで `/exit` は 0（base では続きの周に送り 0 ＝ RED）。
  - `crates/scribe2-boundary/tests/e2e/hook.rs`（`hook_group_current_` 接頭辞・§19 の hook の fixture）: 記録の口座 ≠ 登録 row の口座の席は UserPromptSubmit と SessionStart に `row=` と `current=` を持つ 1 行で `window=` を持たず、移動を頼む記録が置かれない ／ 一致して逼迫の席は `次の 1 周が移り先を決める` を持ち「第 3 段」の語を持たない ／ 記録が読めない席は 0 行（base では「第 3 段まで手で」を出す ＝ RED）。
  - `crates/scribe2-boundary/tests/e2e/seat/launch.rs`（`seat_launch_group_next_` 接頭辞・§20 の `seat_launch_group_` の fixture）: 群の置き場で記録と違う label の短い形は rc 1 で行に `next=seat <記録の label> -c` が置き場の 2 語の前に在り row 0 ／ 長い形も同じ ／ `not-a-shell` の行と群の外の断りは 1 字も変わらない（base では `next=` 無し ＝ RED）。
- 却下: 席が自分を終える（Claude の /exit は人か pane の入力でしか撃てない・ADR-0055 OPT3 の柵と同じ根）／process を kill する（作業記憶が退避されない・N1・§20 の却下のまま）／退避の合図と同じ周に /exit を送る（席が作業記憶を残す番が無い）／hook が記録の口座を移動の周に書き換える（記録を書くのは 1 周の群の段だけ・§20 形 1）／断りの行に候補の一覧を出す（次の 1 手は 1 つ・対話面の作法）。
- 後続: /exit の送りの上限（周の回数）を置くかは実測を見て決める（rules 行は足さない）。

## 22. /exit の dialog を器が確定する（第 3 段の続き）— 続きの周で入力欄の門が /exit の dialog の既定の行を返す席には Enter を 1 回送り、/exit の送りは未確認でも inject の記録に残す（契約表の行 k・§21 の続き・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html) §2・[ADR-0055](../../design-intent/decisions/ADR-0055-group-pressure-is-measured-at-run-ends-and-seat-turns-without-a-timer.html)・`s2-07l.604`）

やさしく言うと: §21 で器は /exit を送るようになったが、席に subagent や background の shell が残っていると Claude Code は「Background work is running」の確認 dialog を出して止まる。器は次の周に同じ /exit を送るつもりだったが、入力欄の門が dialog の画面を「人の打ちかけ」と読んで何も送らず、席は永久に保留になる。dialog の既定は「Exit and stop tasks」なので、門が**その既定の行**を返した周だけ Enter を 1 回送れば席は終わる。

- 出所: 台帳 `s2-07l.604` の notes（2026-09-24 の実地試験・toy の置き場で本物の claude を subagent つきで退避させた）: 続きの周の /exit は届き dialog が出る・その次の周からは `notify::send` が `Blocked::Foreign` で断り記録も event も残らない・手で Enter を 1 回送ると既定が確定して shell に戻り、次の周で同じ target に新しい口座の席が立った。`CLAUDE_CODE_DISABLE_AGENT_VIEW=1` は dialog を消さない（消えるのは「Move to background and exit」の選択肢だけ・公式 doc の記述はその意味）＝ `crates/scribe2/src/seat/cycle/launch.rs` の `with_agent_view_off` の注釈と account-autonomy.md の前提は誤りで、本 § が正す（env の前置は残す: 選択肢が 2 つになり既定が「Exit and stop tasks」で固定される）。
- 現物（verified・main 75829d9）:
  - 続きの周は `crates/scribe2/src/pipe/dispatch/group.rs` の `relaunch`（`Wait::Once`）が `notify::send`（`crates/scribe2/src/pipe/notify.rs`）で `/exit` を送り、結果を捨てる。`send` は `deliver_within`（`crates/scribe2/src/seat/inject.rs`）で、送る前に `pass_input` の門を通し、送った後は目印の出現で settle する。dialog の周は目印が現れないので `Delivery::Unconfirmed` ＝ `record` は呼ばれず inject.jsonl に残らない。
  - 門の読み手は `input_tail`（`crates/scribe2/src/seat/mod.rs`）: 可視域で最後に `❯` を含む行の右側を返す。dialog の画面では既定の選択肢の行 `❯ 1. Exit and stop tasks` が最後の `❯` 行なので tail は `1. Exit and stop tasks`（非空・記録と一致しない）＝ `Blocked::Foreign`。
  - Enter だけを送る口は `inject.rs` に既に在る（`OwnQueued` の周の 1 回・`send-keys Enter`）。
- 形（1 つずつ歯が測る・行 k の done と 1:1）:
  1. **/exit の送りを記録に残す**: 続きの周の /exit は `Delivered` でも `Unconfirmed` でも inject の記録に 1 行残す（`who` は群の段の値・`what` は `/exit`）。`Refused` は残さない（送っていない）。
  2. **既定の行への Enter**: 続きの周で pane が shell でない席に対し、門が `Foreign` で断り、かつその tail（畳んだ字面）が dialog の既定の行の literal `1. Exit and stop tasks` に等しい周は、/exit を送らず Enter を 1 回だけ送る（同じ周に /exit と Enter の両方は送らない）。送りは inject の記録に 1 行（`what` は `enter:exit-dialog` の固定の字面）。tail がそれ以外（人の打ちかけ・別の dialog・prompt 行なし）の周は今のまま 1 key も送らない（fail-closed・N1）。
  3. **Enter の後**: 次の周に shell に戻っていれば §20 形 6 のとおり同じ target へ起こす。戻っていなければ同じ判定を繰り返す（dialog が残れば Enter・prompt 行が戻れば /exit・周ごとに 1 key 列）。上限は置かない（§21 の後続のまま）。
  4. **移動の周（`Wait::Settle`）と群 0 の host と群に属さない anchor は 1 語も変わらない。** 席の hook の行・`Launched` の variant・event の kind の列も変わらない。
- 触らない: `input_tail` と `pass_input` の判定（門は緩めない＝dialog の弁別は群の段の側で tail の等値で行う）・`seat.cycle_settle_s`・§20 の判定・`with_agent_view_off`（env は残す）。
- 歯: `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（`pipe_dispatch_group_exit_dialog_` 接頭辞・§21 の偽 tmux の fixture に「pane の可視域の字面」を作り分ける口を足す）: 続きの周で pane が claude で入力欄が空の席は /exit の送り 1 行と inject の記録 1 行（base では記録 0 ＝ RED） ／ 同じ席の次の周で pane の最後の `❯` 行が `❯ 1. Exit and stop tasks` なら Enter の送り 1 回・/exit の送り 0・記録 1 行（`enter:exit-dialog`）（base では送り 0 ＝ RED） ／ tail が別の字面（例 `1. Exit and stop task`・`foo`）なら送り 0・記録 0 ／ 移動の周は Enter 0。
- 却下: 門を通さず tmux へ直に /exit を送る（co-submit の門を外す・N1）／Enter を条件なしに送る（人の打ちかけを submit する）／dialog の 3 行全部を読む（読む字面は門が既に返す tail の 1 行で足りる・C3.3 の柵）／`CLAUDE_CODE_DISABLE_AGENT_VIEW` を外す（3 択になり「Move to background and exit」が残る）／席の process を kill（§20 の却下のまま）。
- 後続: 移り先の口座が anchor を一度も trust していない周は起動の trust dialog（既定 No, exit）で席が立たず `launch-unconfirmed` の保留になる（同じ実地試験で実測）。公式 doc は口座の `.claude.json` の `projects[<anchor>].hasTrustDialogAccepted` を書く方法だけを案内する。本 repo は JSON の入れ子を書く道具を持たないので、依存の追加（A3）か画面の読みかの裁定を待って別の行にする。

## 23. 便用の除外は群の今の口座だけ — 群の候補の全部を外す形（§17 約束 4）を改め、群ごとの今の口座（記録 > 種）だけを便用の候補から外す（契約表の行 l・§17 の改め・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html) §2・`s2-07l.618`）

やさしく言うと: 群の席は全部同じ口座（群の今の口座）で立つ。§17 は「群が候補に挙げた口座は全部、便に使わない」と決めたので、候補を 6 口座にした host では便に使える口座が群の外の 1 つだけになり、その 1 つの OAuth が死んだ周に全便が黙って止まった。便から外すのは席が実際に使っている口座＝群の今の口座だけでよい。残りの候補は、席が移ってくるまで便が自由に使える。

- 出所: 台帳 `s2-07l.618`（user 裁定 2026-09-25T00:37Z・逐語は台帳の notes・実測は同 bead の本文）。
- 現物（verified・main 8ff1ead）:
  - 群の除外の集合は `crates/scribe2/src/rules/manifest.rs` の `grouped_accounts` 2 本（`HostManifest` の側〔面の 3 値の入口〕と `Manifest` の側〔宣言の全群の候補の和・行 l の便が消しうる〕）と `crates/scribe2/src/rules/mod.rs` の `grouped_accounts(state_dir)`（置き場から面を読む 1 本）。
  - 呼び手は 2 つ: `crates/scribe2/src/pipe/ratelimit.rs`（`grouped` の欄・`select_for_run` へ渡す）と `crates/scribe2/src/fleet/cli.rs` の `select_account` の `Purpose::Run` の枝（`manifest.grouped_accounts()` を自分の除外に足す）。
  - 群の今の口座の解決は `crates/scribe2/src/hook/group.rs` の `current_of(state_dir, group)`（記録 > 種・読めない記録は `RecordError`）。読み手は dispatch の 1 周・席の起動・doctor の 3 つ（§20 形 2）。
  - §17 の歯（`crates/scribe2-boundary/tests/e2e/fleet.rs` と `crates/scribe2-boundary/tests/e2e/rules.rs` の `host_group_` 接頭辞）は「便用で全候補が外れる」を測っている。
- 形（1 つずつ歯が測る・行 l の done と 1:1）:
  1. **除外の集合は群ごとの今の口座**: 置き場から解く 1 本（`crates/scribe2/src/rules/mod.rs` の `grouped_accounts(state_dir)`）は、宣言の各群について `current_of` の label（記録 > 種）を集めて返す（2 群が同じ今の口座なら 1 つ）。記録が在るのに読めない群は typed の断り（fail-closed・候補の全部に読み替えない・候補を 1 つも返さない）。面が無い周は空・読めない面は欠陥の全件（今のまま）。
  2. **口は 1 本**: `fleet select` の `Purpose::Run` の枝も同じ 1 本を使い、宣言の候補の和を返す `Manifest` の側の `grouped_accounts` は便用の除外に使わない（残す用途が無ければ消す・C17.2）。
  3. **除外は次の選定から効き、走行中の便は止めない**（§17 約束 4 のまま）。群が移った周（§20 形 6）に前の今の口座で走っている便もそのまま走る。
  4. **席の登録 row の除外は今のまま重なる**（[account-autonomy.md](./account-autonomy.md) §14）。
  5. **doctor の群の行は 1 字も変わらない**（`accounts=` は宣言の候補・`current=` は今の口座・§20 形 2）。群 0 の host と session 用の選定も不変。
- 触らない: `select_for_run` の並べ順・`select` の判定・session 用の選定・記録の形と書き手（§20 形 1）・`[[account-group]]` の形・便の置き場の登録 row の除外。
- 却下: 群の候補の全部を外す（今の形・便用の口座が host に残らず 1 口座の死で全便が止まる）／群の口座を便にも使う（席と便が同じ口座を消費し、席の逼迫の判定が便の消費で偽陽性になる・§17 の決定のまま今の口座は外す）／便の側が記録を書く（記録の書き手は 1 周の群の段だけ・§20 形 1）／候補の全部を外したまま候補を減らす運用で凌ぐ（移り先の候補と便用の口座が競合し続ける・規則で解く）。
- 歯（`crates/scribe2-boundary/tests/e2e/fleet.rs` の `host_group_` 接頭辞・§17 の fixture〔host.toml に群・偽の実測行〕+ host の群用 dir の記録〔`host_group_record_` の fixture と同じ形〕）:
  - (a) 群 [A, B, C] で記録 = B → 便用の候補から外れるのは B だけ・A と C は候補に残る（base では A / B / C の全部が外れる ＝ RED）。
  - (b) 記録なし → 種 A だけが外れる／記録が読めない（dir）→ 選定は typed に断り候補を 1 つも返さない／群 0 の host → 除外は登録 row の口座だけ（既存の歯のまま）。
  - (c) `fleet select --purpose run` の口でも同じ（B だけが外れる・`--exclude` の重なりは今のまま）。
  - (d) 2 群が同じ今の口座 → 除外は 1 つ／2 群が別の今の口座 → 2 つ。
  - §17 の既存の歯（便用で全候補が外れる 1 本・`fleet select` の口の 1 本・`crates/scribe2-boundary/tests/e2e/rules.rs` の「便用の除外は全群の候補の和」の assert）は「今の口座だけ」の形へ書き換え、名も新しい形（`host_group_run_selection_drops_only_the_current_account` の形）に改める。
- 後続: 群の移動の周に、移り先の口座で走っている便を見る形（§20 形 5 の条件のまま・実測を見て決める）。断りの行の内訳と doctor の便用の口座数（`s2-07l.618` の候補 1 / 2）は別の行。

## 24. 席の登録 row を退役する口 — `seat retire` が退役の event を 1 件記し、replay が最新の退役より後の登録だけを row と読む（契約表の行 m・§20 の後続・[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html)・`s2-07l.618`）

やさしく言うと: 席の登録 row は append-only の event で、消す口が無い。置き場を移した project や役割を畳んだ席の古い row が残り、doctor は missing と数え続け、便用の除外は古い口座を外し続け、群の段は古い target へ退避を送り続ける（実測: 1 つの置き場に別の project の orchestrator の古い row が残り、群の行の `seat-accounts=` に古い口座が出た）。退役の event を 1 つ足し、replay がそれを読めば、row は可逆に消える（N1.2）。

- 出所: 台帳 `s2-07l.618`（実測）と `s2-07l.609` の notes の引き継ぎ (a)（`s2-07l.494` の後続）。
- 現物（verified・main 8ff1ead）:
  - 登録は `SeatRegistered` の event（`crates/scribe2/src/fleet/mod.rs` の `EventKind`・`Shape::Registration`・`registration` の欄）で、replay（`crates/scribe2/src/fleet/replay.rs` の `replay`）は同じ鍵（role, anchor）の最新の行を `registrations` に置く。退役の kind は無い（口座の退役 `AccountRetired` / `AccountRestored` は在る＝同型の先例）。
  - 読み手: `registration_of_target` / `registration_of_key`（`crates/scribe2/src/seat/role.rs`）・`registered_accounts`（便用の除外）・doctor の登録 row の行・群の段の `behind`・tick の `front`。全部 `State` 経由。
  - `seat` の口は `register` / `launch` / `ruling` / `tick` / 短い形（`crates/scribe2/src/seat/cli.rs`・登録の書き手は `crates/scribe2/src/seat/role.rs` の `register`）。
  - `seat` の口の列（`SeatCommand`）の件数と字面は `crates/scribe2-boundary/tests/e2e/seat.rs` の pin（`SEAT_COMMANDS`・4 語）が測り、fleet の record の拒否列は `crates/scribe2/src/fleet/cli.rs` が持つ（`retire` と `SeatRetired` を足す周に両方が動く・行 m の write-set）。
  - `KINDS` の pin の歯 4 本（`crates/scribe2-boundary/tests/e2e/fleet.rs` に 2 本・`crates/scribe2-boundary/tests/e2e/pipe/gate.rs` に 2 本）が event の種類の本数（23）と順を測る。
- 形（1 つずつ歯が測る・行 m の done と 1:1）:
  1. **口**: `seat retire --state-dir S --target S:W [--reason WORDS]`。target の登録 row が無い周は `no-row` で断る（rc 1・event 0）。在る周は `SeatRetired` の event を 1 件記す（role・anchor・target・account は row の写し・detail に reason・actor は human）。stdout 1 行 `seat retire: retired target=<S:W> role=<role> account=<label>`。
  2. **replay**: 同じ鍵（role, anchor）について、最新の `SeatRetired` より後に `SeatRegistered` が無ければ row は無い（`registrations` から外す）。後に `SeatRegistered` が在れば復活（起動が row を書き直す今の形のまま）。物理順で後の event が勝つ（§20 現物の replay と同じ規則）。
  3. **読み手は全部 replay 経由**なので、doctor の登録 row の行・便用の除外・群の段の `behind`・tick の `no-row`・席の起動の `pick_account` の群の除外は、退役した row を見なくなる（読み手の側は 1 字も変えない）。
  4. **event の種類**: `EventKind` に `SeatRetired` を足し（`Shape::Registration`）、網羅 match と `KINDS` の pin の歯 4 本を新しい本数（24）と順で書き換える（§19 / §20 と同じ形）。
  5. **権能**: 退役は 3 クラスに当たらない（消さない・可逆・出さない・使わない）ので orchestrator の席が撃てる（PreToolUse の guard は `launch` だけを止める）。
- 触らない: `SeatRegistered` の形・登録 row の鍵（role, anchor）・席の起動の `prepare`・`AccountRetired` / `AccountRestored`・doctor の行の形（row が消えるだけ）。
- 却下: event log から行を消す（append-only・N1）／`SeatRegistered` を account 空で書いて消したことにする（row の形を緩める・C10）／退役を host の面に書く（宣言と現状を分ける・ADR-0049）／target でなく (role, anchor) を引数にする（人が知っているのは窓の名・row の鍵は器が引く）。
- 歯:
  - `crates/scribe2-boundary/tests/e2e/seat/register.rs`（`seat_retire_` 接頭辞・既存の `role_log` / `role_state` の fixture）: 登録済みの target を retire → event 1 件（kind と role / anchor / target / account）・rc 0 の 1 行／row の無い target → `no-row`・event 0・rc 1／retire の後の doctor は `registered=` が 1 減り missing に数えない／retire の後に同じ target を `seat register` → row が戻る（base では `retire` が使い方の誤り ＝ RED）。
  - `crates/scribe2-boundary/tests/e2e/fleet.rs`（`fleet_replay_seat_retired_` 接頭辞）: `SeatRegistered` → `SeatRetired` の並びで `registered_accounts` が空・`SeatRegistered` → `SeatRetired` → `SeatRegistered` で最後の row（物理順）・`KINDS` の pin 4 本の本数と順。
  - `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（`pipe_dispatch_group_retired_` 接頭辞・§20 の fixture）: 退役した orchestrator の row の target には退避の合図も /exit も送られない（送りの行 0）。
  - 使い方: `seat_usage_external_form` の snapshot に `retire` が増える（同じ便で更新）。
- 後続: 退役した row の target の tick の unit（[seat-heartbeat.md](./seat-heartbeat.md) §3）を同じ口で撤去するかは、行 d（§5）の着地の後に決める。

## 25. 便用の候補なしの断りが内訳を出す（契約表の行 n・§17 / §23 の続き・`s2-07l.618`）

やさしく言うと: 便用の口座が 1 つも選べない周、器は「口座待ちである（候補なし: unmeasured・待つ reset が無い）」とだけ言う。どの口座が除外され、どれが測れず、どれが上限に当たっているかが無いので、席は model の窓を疑って時間を使った（2026-09-25 の実測: 便用の口座が host に 1 つしか残らず、その 1 つの OAuth が墓標で、便が 2 時間止まった）。断りの行に口座ごとの内訳を足す。

- 出所: 台帳 `s2-07l.618`（候補 1）。
- 現物（verified・main 67e74ff）:
  - 便用の選定は `crates/scribe2/src/fleet/select.rs` の `select_for_run`（結果 `Selection`・候補なしは `NoCandidate`〔`reason` = 宣言順で畳んだ 1 語・`earliest_reset`〕・理由の語は `NoCandidateReason` の `NO_CANDIDATE_REASONS`）。歯は `crates/scribe2/src/fleet/select_tests.rs`。
  - 断りの行は `crates/scribe2/src/pipe/ratelimit.rs` の `choose_or_wait`（stdout `run=<id> next=wait reset=<ts|->`・stderr `pipe: run <id> は口座待ちである（候補なし: <語>・待つ reset が無い）`）。歯は `crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs`（`pipe_ratelimit_` 接頭辞）。
- 構築点（verified・main aa0e8b2）: `NoCandidate` の struct literal は `select.rs` に 1・`select_tests.rs` に 1・`crates/scribe2-boundary/tests/e2e/prop.rs` に 2（性質の歯・`Selection` の等値比較）。field を足す便は 4 か所とも直すので `prop.rs` は行 n の write-set に入る（.618 run 1 の Questioned about:write-set・2026-09-25T07:42Z）。
- 形（1 つずつ歯が測る・行 n の done と 1:1）:
  1. **`NoCandidate` が内訳を持つ**: 口座 label の列 3 本 `excluded`（除外集合に在る）・`unmeasured`（測れない・実測行なし・Unmeasured・reset 過ぎ）・`limited`（上限に当たっている）を宣言順に並べて持つ（1 口座は 1 列にだけ・畳む前の値・`reason` の畳み方は今のまま）。
  2. **判定行に 3 欄を足す**: `run=<id> next=wait reset=<ts|-> excluded=<n> unmeasured=<n> limited=<n>`（件数・0 も出す・列は固定）。stderr の断りは今の 1 文の後ろに ` excluded=<label,…> unmeasured=<label,…> limited=<label,…>`（label は `,` 区切り・空は `-`）を足す（先頭の字面 `pipe: run <id> は口座待ちである（候補なし: <語>・待つ reset が無い）` は 1 字も変えない＝既存の歯の pin を動かさない）。
  3. **待つ周（reset が在る周）も同じ 3 欄**を判定行に足す（次の `resume` の判断材料）。
  4. **選ばれた周の行・待ちの観測・段の判定は 1 字も変わらない。**
- 触らない: 選定の順・除外の集合（§23）・`NoCandidateReason` の語と優先順・待ちの deadline・`AccountFree` の観測。
- 却下: 内訳を stderr だけに出す（判定行を読む道具が数えられない）／label を判定行に出す（行が長くなる・stderr に在れば足りる）／群の口座を便にも使う（§17 の決定に反する）／面の検査で「宣言の全部が群に入る面」を断る（§23 以後は除外が群の今の口座だけなので便用の口座が構造として 0 にならない・要らない）。
- 歯: lib（`crates/scribe2/src/fleet/select_tests.rs` に `select_breakdown_` 接頭辞）: 除外 1・測れない 1・上限 1・空き 0 の fixture で 3 列が宣言順の label（base では field が無い ＝ RED）／同じ口座は 1 列にだけ。e2e（`crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs` に `pipe_ratelimit_breakdown_` 接頭辞）: 候補なし（reset 無し）の周の stdout の行の末尾 3 欄と stderr の label の列／reset の在る周の判定行にも 3 欄。

## 26. doctor が便用の口座の数を出す（契約表の行 o・§25 の続き・`s2-07l.618`）

やさしく言うと: 便用に残る口座が 0 の host は、便が起きた瞬間に黙って止まる。doctor の host の行に「便用に使える口座の数」を 1 欄足し、0 を先に名指す。

- 出所: 台帳 `s2-07l.618`（候補 2）。
- 現物（verified・main 67e74ff）: doctor の host の行は `crates/scribe2/src/account/mod.rs` の `render_host_manifest`（`host-manifest=<present|absent|unreadable>[ tick=declared]`）。行の外形は insta の snapshot（`crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap`・`crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap`）と `crates/scribe2-boundary/tests/e2e/seat.rs` / `crates/scribe2-boundary/tests/e2e/seat/account.rs` / `crates/scribe2-boundary/tests/e2e/seat/rules.rs` の字面の歯が pin する。有効な口座の集合（退役を除く）と群の今の口座の解決（§23 の `grouped_accounts`）は既存の 1 本ずつ。
- 形（1 つずつ歯が測る・行 o の done と 1:1）:
  1. **欄 1 つ**: 面が `present` の周だけ行の末尾に `run-accounts=<n>` を足す（n = 有効な口座の数 − 群の今の口座の数・§23 の除外と同じ読み手・席の登録 row の除外は repo ごとなので数えない・計測の鮮度も読まない＝宣言と記録だけの静的な数）。面が無い / 読めない周は今のまま（欄を足さない）。面は読めて event log か群の記録が読めない周は `run-accounts=unreadable`（0 に潰さない・C11・群の行の `unreadable` と同じ語）。
  2. **判定しない**: 0 でも rc と他の行は 1 字も変わらない（C10.2・FR73）。
  3. **既存の pin を進める**: snapshot 2 本と字面の歯は `run-accounts=` の 1 欄分だけ更新する（write-set の外の .snap は触らない）。実測（着地の便）: 欄は `doctor_lines` が行の後ろに足すので、`render_host_manifest` を直に撃つ lib の snapshot と面が absent の e2e の snapshot は動かず、動いたのは面が present の字面の歯 4 本（tick の 3 本・rules の 1 本）だけ。
- 触らない: 群の行（`group=…`）・口座の行・`account ls`・選定。
- 却下: 測れる口座の数を出す（doctor が計測を撃つことになる・§3 の「計測は撃たない」）／群の行に出す（群 0 の host で出ない）。
- 歯（`crates/scribe2-boundary/tests/e2e/seat.rs` に `seat_doctor_run_accounts_` 接頭辞・§17 の host.toml の fixture）: 宣言 3・群 1（今の口座 = 種）→ `run-accounts=2`（base では欄が無い ＝ RED）／退役 1 を足す → 1／群 0 → 3／宣言 1・群 1（有効な口座の全部が群の今の口座）→ `run-accounts=0` で rc と他の行は 2 の周と 1 字も変わらない／面 absent → 欄なし。

## 27. 稼働中の便は群の移動を妨げない — 移り先の候補から「1 周の置き場の live 便が使っている口座」の除外を外し、新規の便だけが群の記録で止まる（契約表の行 p・§20 形 5 / §23 の改め・持ち主の裁定 2026-09-25T15:2xZ〔逐語は台帳〕）

やさしく言うと: 群が口座を移るとき、器は「他の群の今の口座」「閾値以上の口座」に加えて「この置き場で走っている便が使っている口座」も候補から外している。持ち主の整理は違う: 稼働中の便は移動を妨げなくてよく、移り先に選ばれた口座で**新しい便が起きないこと**だけが要る。後者は §23 が既に持つ（便の選定は群の記録の口座を外す・記録は移り先が決まった瞬間に lock の内側で書かれる）。しかも置き場は repo ごとなので他の repo の便はもともと見えず、同じ repo の便にだけ余計に厳しい非対称になっていた。5 口座 2 群で便が走る host では「候補なし」が増えるだけなので、除外を外して統一する。

- 出所: 持ち主の 2026-09-25T15:19Z の問い（群の排他の整理）と 15:2xZ の裁定「外す」（逐語は台帳・契約の bead の notes）・裁定の記録は ADR-0068（ADR-0049 / ADR-0055 の移り先の条件を部分 supersede・要件 FR38 / AC41・SRS v0.24）。
- 現物（verified・main 6fcd3d5）:
  - 候補の規則は §20 形 5（`crates/scribe2/src/pipe/dispatch/group.rs` の `target_of`: 今の口座でなく・他の群の今の口座でなく・退役中でなく・**1 周の置き場の live 便が使っていない**〔`crates/scribe2/src/fleet/replay.rs` の `inflight_by_account`〕・3 窓とも閾値未満）。seat-heartbeat.md 行 i の着地後はこの規則は `crates/scribe2/src/hook/group.rs` の判定の 1 本の中に在る（本行はその後に撃つ・write-set は両 file）。
  - 新規の便の除外は §23（`crates/scribe2/src/rules/mod.rs` の `grouped_accounts` が各群の今の口座〔記録 > 種〕を返し、`crates/scribe2/src/pipe/ratelimit.rs` と `crates/scribe2/src/fleet/cli.rs` の便用の選定が外す）。記録は §20 形 6 の執行が lock の内側で先に書く＝選定の後は新しい便がその口座で起きない。
  - 歯 `pipe_dispatch_group_move_skips_an_account_used_by_a_live_run`（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`）が「live 便の在る口座を飛ばす」を測っている。
- 形（行 p・1 つずつ歯が測る・done と 1:1）:
  1. **候補の規則から live 便の項を外す**: 移り先 = 宣言の候補の順で、今の口座でなく ∧ 他の群の今の口座でなく ∧ 退役中でなく ∧ 3 窓とも閾値未満の実測（鮮度の内側）を持つ、最初の label。live 便の読み（`inflight_by_account`）は候補の規則から消える（便用の選定と着地の列はそのまま読む・本行は触らない）。§20 形 5 の本文は本 § と同じ規則に書き直してある（docs で先に整えた・便は §20 を触らない＝便 235411Z の gate の根: 本文に live 便の項を残したまま「外した」と注記する自己矛盾）。
  2. **新規の便は記録で止まる（§23 のまま・本行は 1 字も変えない）**: 移り先の記録が書かれた後の便用の選定はその口座を外す。稼働中の便は終端まで走る（止めない・口座を替えない）。
  3. **他の置き場の便**: もともと見えない（置き場ごとの event log）。本行の後は同じ置き場の便と同じ扱い＝非対称が消える。
  4. **判定行・event・通知・記録の形は不変**。候補なしの内訳（§25 の `excluded` / `unmeasured` / `limited`）から live の理由が消える（§25 の内訳の語に live 便の項が在れば消す・無ければ不変）。
- 触らない: §23 の便用の除外・§20 形 6 の執行の順・lock・記録の形・承認 event・退避と起こし直し・§25 の内訳の 3 欄の形。
- 却下: 便の側で「群の今の口座で走っている便を止めて別口座で再開する」（便の途中再開は別の要件 FR37 の道・移動と結ばない）／移り先の記録を便の選定が lock の内側で読む（隙間は稼働中の便 1 本が新しい口座で走り出すだけ＝持ち主の整理では害が無い・lock の読み手を増やさない）／置き場を跨いで live 便を集める（置き場ごとの event log の原則を崩す・要らない）。
- 歯（`crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`・`pipe_dispatch_group_move_` 接頭辞の既存の fixture）: (a) `pipe_dispatch_group_move_skips_an_account_used_by_a_live_run` を「live 便の在る口座へも移る」の歯に書き換える（名も改める・live あり / なしの両方で同じ移り先＝base では live ありが次の候補へ飛ぶ ＝ RED）(b) 移った直後の便用の選定（`fleet select` の口・§23 の既存の歯の fixture・fleet.rs）がその口座を外す（不変・GREEN のまま・write-set の外なので verify では撃たず CI の全数で測る）(c) 既存の `pipe_dispatch_group_` の他の歯は 1 字も変えず GREEN（verify の filter は `pipe_dispatch_group_` の全体で (a) と (c) を撃つ）。

## 28. 群の種は宣言順に重ならない — 記録の無い群の今の口座（種）を「宣言順で前の群の種でない最初の候補」にし、両群が同じ候補の列を宣言しても初期状態で同じ口座に乗らない（契約表の行 q・§20 形 2 の改め・同じ裁定の周）

やさしく言うと: 群の今の口座は「記録 > 種」で、種は宣言の候補の先頭。Tier1 と Tier2 は同じ 5 口座を同じ順で宣言しているので、記録が無い初期状態では両群とも先頭の口座に乗る。群 ↔ 群の排他は移動の判定にしか無く、種の段では効かない。host を作り直した周・記録を消した周に必ず起きる。種を「宣言順で前の群が種にしていない最初の候補」にすれば、面を書き換えずに初期状態から重ならない。

- 出所: §27 の問いの周の実測（2026-09-25 時点の host.toml は両群が同じ列・両群とも記録あり＝実害なし）。
- 現物（verified・main c07b823）: 種は `crates/scribe2/src/hook/group.rs` の `current_of`（記録が無ければ `accounts().first()`）。群の宣言は `crates/scribe2/src/rules/manifest.rs` の `AccountGroup`（name / anchors / accounts / line）で、面の読みが宣言順の列を持つ。面の欠陥の列は同じ file の `Vec<RuleError>`（`RuleError::new(行番号, 文)`・欠陥の種は enum でなく文の literal・群の検査は `check_duplicate_groups` が名の重複と置き場の重複の 2 種を push する）で、欠陥の型の file は他に無い。読み手は dispatch の 1 周・席の起動・doctor・hook の 4 つ（§20 形 2・§21）。
- 形（行 q・1 つずつ歯が測る・done と 1:1）:
  1. **種は面の読みで 1 回決める**: `AccountGroup` に種の欄を足し、面を読む 1 本が宣言順に「前の群の種でない最初の候補」を種として埋める。全候補が前の群の種に使われている群は面の欠陥＝`check_duplicate_groups` と同じ file・同じ列（`Vec<RuleError>`）に `RuleError::new(群の行番号, 文)` の push を 1 つ足す（文の literal は「群 <名> の種を決める候補が無い」の形で語「種を決める候補が無い」を含む・enum も struct も新しい file も足さない・fail-closed）。
  2. **解決の 1 関数は種の欄を読む**（`accounts().first()` をやめる）。記録が在る周は今のまま記録が勝つ。
  3. **doctor の群の行の `current=` は種の周も同じ 1 関数から出る**（形は不変・値だけ変わりうる）。
  4. 候補が 1 口座しか無い群 2 つ（同じ口座）は欠陥＝面を直すまで両群の読みが止まる（読めない面は全群を typed に止める今の規則と同じ）。
- 触らない: 記録の形・移動の判定・§23 の除外・面の表の形（欄を足さない・種は導出）。
- 却下: 面の検査で「同じ種」を欠陥にする（今の host.toml がそのまま欠陥になり全群が止まる・持ち主に面の書き換えを強いる）／種を乱数や host 名で選ぶ（決定的でない・N3）／群ごとに別の候補の列を強いる（面の書き方の規則が増える・C1）。
- 歯（`crates/scribe2-boundary/tests/e2e/rules.rs` に `host_group_seed_` 接頭辞〔既存の `host_group_` の歯は fleet.rs / seat/account.rs にも在るので、行 q の verify はこの接頭辞で rules.rs の新しい歯だけを名指す〕と `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`）: (d) 同じ列を宣言した 2 群の記録なしの周の `current=` が宣言順に先頭と 2 番目（base では両方先頭 ＝ RED）(e) 候補 1 つを共有する 2 群は欠陥の行番号（2 番目の群の行）と語「種を決める候補が無い」（`RuleError` の文の literal・型は足さない） (f) 記録が在る群は種に依らず記録の口座（不変）(g) 群 1 つの host は先頭のまま（不変）(h) 既存の歯 2 本 `host_group_run_exclusion_folds_the_current_accounts_of_the_groups`（`crates/scribe2-boundary/tests/e2e/fleet.rs`）と `host_group_doctor_line_says_none_without_seat_rows`（`crates/scribe2-boundary/tests/e2e/seat/account.rs`）は 2 群が同じ先頭の候補を種に持つ旧い規則を fixture で pin しているので、名は変えずに新しい規則（宣言順に先頭と 2 番目）へ書き換えて GREEN に戻す（実装役の問い 2026-09-25T16:1xZ・write-set の閉包）。fleet.rs の歯は便用の除外の label を assert するので書き換えは base で RED になるが、seat/account.rs の歯は `current=seed` の語しか assert せず fixture（候補を 2 つに）だけが変わる＝base でも GREEN なので、その fn の中の行頭に `// flip-check: retroactive s2-07l.641` の札を 1 行付ける（効く条件は test 区間内 / 行頭 / bead id / base に無い札の 4 つ・契約 contract-source.md の逃がしと同じ形）。札は緑を免じる印でなく「後から動かした歯」の申告なので、done の変異 A/B（候補 1 つを共有する 2 群で doctor が typed に止まる変異の撃墜）を notes に残す（run 231539Z の gate FAIL green-on-base の根）。

## 29. 群の移り先は残量の鍵と群の予約で決まり、群の名は Tier と数字 — 候補を残量の鍵〔7 日窓と役割の model の 7 日窓の残量の小さい方 → 7 日窓の reset → 宣言順〕で並べ、群を宣言順に見て「どの群の今の口座でもなく先の群の予約でもない先頭」を群の予約とし、移る群は自分の予約へ移り、doctor の群の行に next= を出す（契約表の行 r・行 s・§20 形 5 / §27 形 1 の候補の順の改め・ADR-0069・FR38 / AC41・`s2-07l.645`）

やさしく言うと: 今の移り先は「宣言の候補の順で門を通る最初の口座」なので、残量が 1% しか無い口座でも列の先に在れば選ばれ、役割の model の窓だけが高い口座も 7 日窓が低ければ選ばれる。残量の大きい順に並べ替え、群が複数在る host では宣言順で先の群（Tier の数字が小さい群）が先に移り先を取り、後の群はそれを避ける。予約は file に書かず、周ごとに lock の内側で導き直す。便の起動は予約を避けない（便用の除外は §23 のまま群の今の口座だけ）。優先の低い群が移り先なしになるのは想定内（持ち主の是認 2026-09-26T01:3xZ・逐語は台帳）。

- 出所: 台帳 `s2-07l.645`（tsuzuri 席の relay と持ち主の裁定 2026-09-26T00:43Z / 00:48Z）・ADR-0069・SRS v0.25 FR38 / AC41。
- 現物（verified・main 31118b7）: 移り先は `crates/scribe2/src/hook/group.rs` の `target_of`（`Judge` の `others` を避け、候補の宣言順で門〔今の口座でなく・他の群の今の口座でなく・退役中でなく・鮮度の外は 1 回測り・`fresh_rows` の 3 窓が `pressed` でない〕を通る最初の label）。呼び手は 1 周の群の段（`crates/scribe2/src/pipe/dispatch/group.rs` の `round`・`others` = 他の群の今の口座〔同じ周で先に移った群の移り先で更新〕∪ 記録を読めない群の候補）と管理 tick の `judged`（`crates/scribe2/src/seat/tick.rs`・`others` = 他の群の今の口座〔記録を読めない群は候補の全部〕）の 2 つで、doctor の群の行は `crates/scribe2/src/account/mod.rs` の `render_group`（`current=` まで・移り先は出さない）。実測の行は `Measured`（`window` / `model` / `used_pct` / `resets_at`）で、モデル別窓の行は `model` に表示名（`Model` の `display`・例 Fable）を持つ。役割の model は rules 行 `seat.model.<役割>`（kind `RoleModel`）。群の名は `check_duplicate_groups` が重複だけを拒む（形は見ない）。e2e rules.rs の既存の歯 `host_group_defects_are_refused_with_line_numbers_and_the_face_prefix` は欠陥の列を**全件・件数ごと**に pin する（名の重複の本文は 1 件だけ・「同じ欠陥を 2 行にしない」・面の中の欠陥と合わせの欠陥を同時に持つ本文は面の中の 1 件だけ）ので、同じ行に欠陥を重ねる規則はその歯の件数を動かす。`crates/scribe2/src/rules/manifest.rs` は幅で正規化した行数が上限 R-C4-2（1500）まで**余地 5 行**（受付の `cap-headroom`・2026-09-26 の実測）なので、検査の本体はそこに置けない。`crates/scribe2/src/rules/mod.rs` は 724 行で `mod` 宣言の列（`cli` / `manifest`）を持つ。host の fixture の群の名は e2e 8 file（alpha / beta / g / g1 / g2）に散り、lib の test は群の名を持たない（母集団は行 s の write-set）。
- 形（行 r・1 つずつ歯が測る・done と 1:1）:
  1. **残量の鍵は pure な 1 関数**: 口座の鮮度の内側の実測の行から (a) 残量 = min(100 − 7 日窓の使用率, 役割の model ごとの 100 − モデル別 7 日窓の使用率) の大きい順 → (b) 7 日窓の `resets_at` の早い順（無い口座は後）→ (c) 宣言の候補の index の順、の辞書順。役割の model の集合 = 群の置き場の席の登録 row の役割（重複は畳む）ごとの rules 行 `seat.model.<役割>` の表示名。集合のどれか 1 つでもモデル別窓の行を持たない口座は最後（(a) の前に立つ bool）。集合が空（席の row が無い群）は 7 日窓だけで並べる。役割の行が無い周は typed に止まる（`Caps` の行が無い周と同じ極性・既定を出さない）。5 時間窓は門にだけ使う。
  2. **群の予約は 1 関数**（鍵の関数を呼ぶ）: 群を宣言順に見て、群 g の予約 = 門（今のまま: 今の口座でなく・どの群の今の口座でもなく・退役中でなく・鮮度の内側の実測を持ち 3 窓とも閾値未満・鮮度の外の候補は `measure` で 1 回測る）を通る候補を鍵で並べたものから、先の群の予約でない先頭（無ければ無し）。移る群の移り先 = 自分の予約。`target_of` はこの関数に置き換わる（宣言順で最初に門を通る label は返さない）。
  3. **予約は記録しない**: 1 周の群の段と tick の `judged` は lock の内側で、逼迫した群を判じる周にだけ、その群と宣言順で前の群の予約を導く（前の群の候補も鮮度の外は測る・口座ごとに 1 周 1 回）。逼迫でない周は候補を測らず予約も導かない。群用 dir に予約の file は 0。
  4. **便の起動は排他しない**: §23 の便用の除外（群の今の口座だけ）と便用の並べ鍵（ADR-0042）は 1 字も変えない（予約された口座を便は使ってよい・live 便は移動を妨げない〔§20 形 5 の改めのまま〕）。
  5. **doctor の群の行は `next=<label|none|unreadable>`** を `current=` の後ろに足す（同じ予約の関数・測らない〔鮮度の外の候補は門を通らない＝none になりうる〕・記録を読めない群は unreadable・群 0 の host は 1 語も変わらない）。
  6. **移動の契機・lock・記録・承認 event・退避・起こし直し・閾値の行・event の種類は不変**（§20 形 6・§27・seat-heartbeat.md §9）。
- 形（行 s・1 つずつ歯が測る・done と 1:1）:
  1. **群の名は Tier と数字**（`Tier` の後ろに 10 進の数字 1 桁以上・先頭の 0 は不可）に限る。外れる名は群の見出し行の欠陥（`check_duplicate_groups` と同じ列 `Vec<RuleError>` に `RuleError` の new（群の行番号・文）で push・文は語「Tier と数字の形でない」を含む・enum も struct も足さない）。**検査の本体は行 s の write-set の `+` の file（rules の兄弟 module・`crates/scribe2/src/rules/mod.rs` に `mod` 宣言 1 行）に置き、`crates/scribe2/src/rules/manifest.rs` に増えるのは `check_duplicate_groups` の呼び出しの隣の呼び出し 1 行だけ**（余地 5 行の内側・引数は群の列と欠陥の列・`AccountGroup` の `name` と `line` の accessor だけを読む）。
  2. **宣言順は数字の昇順**（狭義）: 前の群の数字以下の群は欠陥（文は語「前の群より大きくない」を含む）。ただし**名が前の群と同じ周は昇順の欠陥を重ねない**（名の重複の 1 件だけ・既存の歯の「同じ欠陥を 2 行にしない」を守る＝同じ数字で名が違う周〔Tier01 は形で落ちるので起きない〕だけが昇順の欠陥）。名の重複の検査は今のまま残す。数字は数値で比べる（辞書順ではない）。
  3. 読めない面は全群を typed に止める今の規則のまま（fail-closed）。優先は宣言順のまま（数字は宣言順の検査にだけ使い、並べ替えには使わない）。
  4. **fixture の群の名を Tier と数字に改める**（e2e 8 file）: 名だけを改め assert の他の語は変えない。`init --group` の周の fixture も同じ。母集団（`[[account-group]]` の字面を持つ helper / const から名の到達で数えた歯・2026-09-26 の census・main 060f804）: rules.rs 7 本（`host_group_defects_` / `host_group_seed_` / `host_group_table_` / `rules_review_same_`）・main.rs 7 本（`host_init_` / `init_repo_` / `init_seat_`）・pipe/dispatch.rs 40 本（`pipe_dispatch_group_`）・fleet.rs 8 本（`host_group_`）・seat/launch.rs 18 本（`seat_launch_`）・seat.rs 30 本（`seat_doctor_run_` / `seat_pane_shell_` / `seat_tick_`）・seat/account.rs 5 本（`host_group_`）・hook.rs 13 本（`hook_group_`）＝**128 本**。行 s の検証行はこの接頭辞を全部持ち、名の改め漏れ（旧い名の群を持つ面が読みで断られて歯が赤くなる）を検証行が測る。**改名だけの歯の file（rules.rs 以外の 7 file）は base でも緑**（名の改めは base の規則を 1 つも破らない）なので、gate の flip-check が `green-on-base file=…` で落とす（run 023931Z の再現・fleet.rs の overlay が全部 PASS）＝file ごとに札 `// flip-check: retroactive <行 s の bead>` を**1 行**置く（e2e の file は全体が歯の区間・効く条件は test 区間内 / 行頭 / bead id 必須 / base に無い札の 4 つ・[contract-source.md](./contract-source.md) の `.277` の逃がしと同じ形）。札は緑を免じる印でなく「後から動かした歯」の申告なので、done の変異 A/B（Tier でない名を通す変異・昇順を辞書順で比べる変異が `host_group_tier_` で落ちる本数と母集団）を notes に残す。rules.rs は新しい歯 `host_group_tier_` が base で赤なので札は要らない。
- 触らない: 便用の口座選定（§23・ADR-0042）・群の記録の形（§20 形 1）・種（§28）・hook の頼み（§20 形 4）・退避と起こし直し（§20 形 6・§21）・面の表の key（欄を足さない）。
- 却下: 予約を file に書く（周をまたぐ状態が増え、消し忘れが移り先を永久に塞ぐ・C1）／ 5 時間窓を鍵に入れる（5 時間で戻る値が 7 日の残量の順を乱す）／役割の model を無視して 7 日窓だけで並べる（役割の model の窓だけが 100% の口座へ移り、着いた席が最初の turn で逼迫する＝持ち主の pushback 2026-09-26T00:36Z）／後の群が先の群の予約を奪える（優先の意味が消える）／群を並列に判じる（lock 1 本の内側の宣言順が予約の定義そのもの）／数字で並べ替える（宣言順と数字が食い違う面を黙って通す・断る方が読み手に近い）。
- 歯: `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`（`pipe_dispatch_group_reserve_` 接頭辞・§20 の `pipe_dispatch_group_move_` と同じ偽 usage の fixture〔候補の窓の値を口座ごとに与える〕・行 s の後なので群の名は Tier1 / Tier2）: (a) 宣言順で先の候補の残量が小さく後の候補の残量が大きい周は後の候補へ移る（base では先の候補 ＝ RED）(b) 7 日窓の残量が同じで役割の model の窓の残量が違う 2 候補は model の残量の大きい方へ（base では宣言順 ＝ RED）(c) 残量の同点は 7 日窓の reset の早い方へ、reset も同じなら宣言順（2 本）(d) 役割の model のモデル別窓の行を持たない候補は残量に依らず最後へ（base では宣言順 ＝ RED）(e) 5 時間窓が閾値以上の候補は門で落ち、閾値未満の周は 5 時間窓の値が並びを変えない（2 本）(f) 2 群が同じ周に逼迫し候補を共有する周は Tier1 が鍵の先頭へ、Tier2 は次へ（記録 2 の label が異なる・鍵の先頭 ≠ 宣言の先頭で組み、base の宣言順の更新と同じ結果になる fixture を避ける）(g) Tier2 だけが逼迫し Tier1 は逼迫でない周も、Tier2 は Tier1 の予約（Tier1 の鍵の先頭）を飛ばして 2 番目へ移る（base では Tier1 の予約を取る ＝ RED・持ち主の裁定の核）(h) 予約の立つ口座（Tier1 の鍵の先頭・記録は動かない）を便用の選定が使い、群用 dir に予約の file は 0（base では便も使う＝GREEN なので、この歯は (g) の fixture の中で同じ周の便用の選定を測る 1 assert にする）(i) Tier2 だけが逼迫する周に Tier1 の候補（鮮度の外）が測られる（偽 usage の呼び出しに Tier1 の候補の label が在る・base では無い ＝ RED）。`crates/scribe2-boundary/tests/e2e/seat.rs`（`seat_tick_judge_reserve_` 接頭辞・§9 の `seat_tick_judge_` の fixture）: tick の判定も Tier1 の予約を飛ばす（base では RED）。`crates/scribe2-boundary/tests/e2e/seat/account.rs`（`host_group_next_` 接頭辞）: doctor の群の行の `next=` が鍵の先頭の label ／ 門を通る候補が無ければ none ／ 記録を読めない群は unreadable ／ 既存の `host_group_doctor_line_says_none_without_seat_rows` は行の末尾に `next=` が増えるので書き換えて GREEN（base では `next=` 無し ＝ RED）。lib（`crates/scribe2/src/hook/group.rs`・`group_key_` 接頭辞）: 鍵の関数の並びを行の値だけで測る（役割の集合が空・1 つ・2 つ・model の行を欠く口座）。行 s は `crates/scribe2-boundary/tests/e2e/rules.rs`（`host_group_tier_` 接頭辞・§28 の `host_group_seed_` の fixture）: (j) 名が alpha の群は欠陥（行番号は群の行・語「Tier と数字の形でない」・base では通る ＝ RED）(k) Tier2, Tier1 の順は 2 番目の群の行の欠陥（語「前の群より大きくない」）(l) Tier1, Tier1 は名の重複の 1 件だけ（昇順の欠陥を重ねない・既存の歯 `host_group_defects_are_refused_with_line_numbers_and_the_face_prefix` の名の重複の本文が名の改めだけで 1 件のまま GREEN） (m) Tier01 は形の欠陥 (n) Tier1, Tier2, Tier10 は通る（数値で比べる・辞書順で Tier10 < Tier2 と読む変異を捕まえる）。既存の群の歯（上の 128 本）は名の改めだけで GREEN＝検証行の接頭辞 12 本がその全部を撃つ。

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
write-set = ["crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/cycle/relaunch.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_move_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_record_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_group_move_"]
size = "L"
growth = ["crates/scribe2/src/pipe/dispatch.rs:20", "crates/scribe2/src/fleet/mod.rs:40", "crates/scribe2/src/account/mod.rs:60"]
done = "(1) 群の今の口座の記録が host の根の群用 dir に群ごとに高々 1 file で在り、1 周の群の段だけが lock の内側で一時 file → rename で書き、書き換える周は前の記録を履歴へ move する (2) 解決の 1 関数が記録 > 種の順で返し、読めない記録は typed に止まり、doctor の群の行に current= が増えて群 0 の host の外形は不変 (3) 群の anchor の seat launch と短い形は選定を撃たず解決値で起き、違う label は rc 1・row 0 で断り、群の外の anchor は今のまま (4) hook が逼迫の周に移動を頼む記録を高々 1 file 置き、1 周はそれが在れば鮮度に依らず計測を撃って判定の後に履歴へ move する（move せず残す変異は 2 周目の計測 0 を測る歯が捕まえる） (5) 逼迫した群の移り先を宣言の候補の順で他の群の今の口座（同じ周で先に移った群の移り先を含む＝2 群が同じ周に同じ label へ移らない）でない ∧ live 便が使っていない ∧ 3 窓とも閾値未満の最初の label に 1 回だけ決め（他の群・live 便・3 窓の各条件を外す変異は別々の歯が捕まえる＝候補を飛ばす側と移る側の両側）、無ければ記録 0・断りの event 1・席へ群の置き場ごとに 1 行だけ（断った周は §19 の通知を送らない） (6) 移動の周は記録 → 承認 event（kind は本行が足す移動の variant で run / bead を持たず・account = 移り先・逐語 = 宣言の行）→（4 手の順は時刻の並びを歯が pin する） 退避の合図 → settle の窓で shell に戻った置き場から同じ target へ新しい口座の席を launch の 1 本で起こし、戻らない席は保留の event を記して次の 1 周が判定を繰り返さず続きだけ行い、移動した周は通知を送らない (7) 群の段は便の列の前に走り、失敗は便の列の rc を変えない (8) 群 0 の host は 1 語も変わらない（起こす側の周で群用 dir が作られず記録 0・event 0・送り 0・起動 0・rc と dispatch ls の外形が不変・歯が pin する）"

[[contract]]
id = "j"
title = "退避を器が完結させる — 続きの周は pane が shell でない保留の席へ /exit の 1 行を送り、席の hook は記録の口座と登録 row の食い違い（row= / current=）を告げ、group-account の断りは next=seat <記録の label> -c を持つ（§21・ADR-0049 §2・s2-07l.604）"
req = ["FR38", "FR36", "NFR4"]
section = "21"
write-set = ["crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_exit_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_group_current_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_group_next_"]
size = "M"
depends = ["i"]
done = "(1) 記録の口座 ≠ 登録 row の口座の群の続きの周は、pane が shell でない席へ退避の合図と同じ口で /exit の 1 行を周ごとに 1 回送り、その周は起こさず保留の event を重ねず、shell に戻った周は §20 形 6 のとおり同じ target へ起こし、移動の周（記録を書いた周）は退避の 1 行だけで /exit を送らない (2) 席の hook は群の今の口座（current_of）を読み、記録 ≠ row の周は逼迫を測らず row= と current= を持つ移動中の 1 行を出して移動を頼む記録を置かず、一致して逼迫の周は「次の 1 周が移り先を決める」の 1 行で「第 3 段まで手で」の語を持たず、記録が読めない周は 0 行 (3) reason=group-account の断りの行は next=seat <記録の label> -c を置き場の 2 語の前に持ち（短い形と長い形）、他の断りの行は 1 字も変わらない (4) event の kind の列と Launched の variant と §20 の判定は変わらず、群 0 の host と群に属さない anchor は 1 語も変わらない 歯: pipe_dispatch_group_exit_ の歯が続きの周の /exit の送り 1 行・周ごとに 1 回・shell に戻った周は起動 1・移動の周は /exit 0 を測り、hook_group_current_ の歯が row= / current= の 1 行と頼みの記録 0 と「第 3 段」の語の不在を測り、seat_launch_group_next_ の歯が next= の位置と他の断りの不変を測る"

[[contract]]
id = "k"
title = "/exit の dialog を器が確定する — 続きの周で門が既定の行 1. Exit and stop tasks を返す席には Enter を 1 回送り、/exit の送りは Unconfirmed でも inject に記録する（§22・ADR-0049 §2・s2-07l.604）"
req = ["FR38", "FR36", "NFR4"]
section = "22"
write-set = ["crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/pipe/notify.rs", "crates/scribe2/src/seat/inject.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_exit_dialog_"]
size = "M"
depends = ["j"]
done = "(1) 続きの周の /exit の送りは Delivered でも Unconfirmed でも inject の記録に 1 行残り（what は /exit）、Refused の周は残らない (2) 続きの周で pane が shell でない席に対し、門が Foreign で断りその tail が literal 1. Exit and stop tasks に等しい周は /exit を送らず Enter を 1 回だけ送って inject の記録に what=enter:exit-dialog の 1 行を残し、tail がそれ以外の周は 1 key も送らず記録も残さない (3) Enter の後に shell に戻った周は §20 形 6 のとおり同じ target へ起こし、戻らない周は同じ判定を繰り返して上限を置かない (4) 移動の周と群 0 の host と群に属さない anchor と席の hook の行と Launched の variant と event の kind の列は 1 語も変わらない 歯: pipe_dispatch_group_exit_dialog_ の歯が /exit の記録 1 行・既定の行への Enter 1 回と記録 1 行・別の字面への送り 0・移動の周の Enter 0 を測る"
[[contract]]
id = "l"
title = "便用の除外は群の今の口座だけ — 置き場から解く 1 本が群ごとの current_of の label を集め、fleet select の便用の枝も同じ 1 本を使う（§23・§17 約束 4 の改め・ADR-0049 §2・s2-07l.618）"
req = ["FR57", "FR36", "NFR4"]
section = "23"
write-set = ["crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_"]
size = "M"
growth = ["crates/scribe2/src/rules/mod.rs:40", "crates/scribe2/src/rules/manifest.rs:0", "crates/scribe2/src/hook/group.rs:60", "crates/scribe2/src/fleet/cli.rs:20", "crates/scribe2/src/fleet/replay.rs:10", "crates/scribe2/src/pipe/ratelimit.rs:10"]
done = "(1) 置き場から解く 1 本は宣言の各群について current_of の label（記録 > 種）を集めて返し、2 群が同じ今の口座なら 1 つ、記録が在るのに読めない群は typed に断って候補を 1 つも返さず、面が無い周は空、読めない面は欠陥の全件のまま (2) fleet select の Purpose::Run の枝も同じ 1 本を使い、宣言の候補の和を返す Manifest の側の grouped_accounts は便用の除外に使わず残す用途が無ければ消す (3) 除外は次の選定から効き走行中の便は止めない (4) 席の登録 row の除外は今のまま重なる (5) doctor の群の行と群 0 の host と session 用の選定は 1 字も変わらない 歯: host_group_ の歯が、群 [A, B, C] で記録 = B なら B だけが外れ A と C は候補に残ること・記録なしなら種 A だけ・読めない記録は typed の断り・fleet select --purpose run の口でも同じ・2 群が同じ今の口座なら除外 1 つで別なら 2 つ・群 0 は登録 row の口座だけを測り、§17 の「全候補が外れる」の歯と rules.rs の「全群の候補の和」の assert は今の口座だけの形へ書き換える"

[[contract]]
id = "m"
title = "席の登録 row を退役する口 — seat retire が SeatRetired の event を 1 件記し、replay が最新の退役より後の登録だけを row と読む（§24・ADR-0049・s2-07l.618）"
req = ["FR36", "FR59", "NFR4"]
section = "24"
write-set = ["crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/register.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_retire_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_replay_seat_retired_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_retired_"]
size = "M"
growth = ["crates/scribe2/src/fleet/mod.rs:40", "crates/scribe2/src/fleet/event.rs:40", "crates/scribe2/src/fleet/replay.rs:40", "crates/scribe2/src/seat/cli.rs:60", "crates/scribe2/src/seat/role.rs:80", "crates/scribe2-boundary/src/main.rs:20"]
done = "(1) seat retire --state-dir S --target S:W [--reason WORDS] は登録 row の無い target を no-row で断り（rc 1・event 0）、在る周は SeatRetired の event を 1 件記して（role・anchor・target・account は row の写し・detail に reason・actor は human）stdout に seat retire: retired target=<S:W> role=<role> account=<label> の 1 行を出す (2) replay は同じ鍵（role, anchor）について最新の SeatRetired より後に SeatRegistered が無ければ row を registrations から外し、後に在れば復活させ、物理順で後の event が勝つ (3) doctor の登録 row の行・便用の除外・群の段の behind・tick の no-row・席の起動の群の除外は読み手を 1 字も変えずに退役した row を見なくなる (4) EventKind に SeatRetired（Shape::Registration）を足し、網羅 match と KINDS の pin の歯 4 本を 24 種と新しい順で書き換える (5) SeatRegistered の形・登録 row の鍵・prepare・AccountRetired / AccountRestored・doctor の行の形は変えない 歯: seat_retire_ の歯が登録済みの target の retire で event 1 件と rc 0 の 1 行・row の無い target の no-row と event 0 と rc 1・retire 後の doctor の registered= が 1 減ること・retire 後の seat register で row が戻ることを測り、fleet_replay_seat_retired_ の歯が SeatRegistered → SeatRetired で registered_accounts が空・SeatRegistered → SeatRetired → SeatRegistered で最後の row を測り、pipe_dispatch_group_retired_ の歯が退役した row の target に退避の合図も /exit も送られないことを測り、seat_usage_external_form の snapshot に retire が増える"
[[contract]]
id = "n"
title = "便用の候補なしの断りが内訳を出す — NoCandidate が excluded / unmeasured / limited の label の列を持ち、判定行に件数 3 欄・stderr に label を足す（§25・s2-07l.618 候補 1）"
req = ["FR36", "FR33", "NFR4"]
section = "25"
write-set = ["crates/scribe2/src/fleet/select.rs", "crates/scribe2/src/fleet/select_tests.rs", "crates/scribe2/src/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/pipe/ratelimit.rs", "crates/scribe2-boundary/tests/e2e/prop.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail select_breakdown_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_ratelimit_breakdown_"]
size = "S"
growth = ["crates/scribe2/src/fleet/select.rs:40", "crates/scribe2/src/pipe/ratelimit.rs:20", "crates/scribe2-boundary/tests/e2e/prop.rs:10"]
done = "(1) NoCandidate が excluded / unmeasured / limited の label の列 3 本を宣言順に持ち、1 口座は 1 列にだけ入り、reason の畳み方は今のまま (2) 候補なしの判定行は run=<id> next=wait reset=<ts|-> excluded=<n> unmeasured=<n> limited=<n>（0 も出す）、stderr は今の 1 文の後ろに excluded=<label,…> unmeasured=<label,…> limited=<label,…>（空は -）を足し先頭の字面は 1 字も変えない (3) reset の在る待ちの周も同じ 3 欄 (4) 選ばれた周の行・待ちの観測・段の判定は 1 字も変わらない (5) crates/scribe2-boundary/tests/e2e/prop.rs の性質の歯にある NoCandidate の構築点 2 つは 3 列つきの形に直すだけで期待値の意味は変えない 歯: select_breakdown_ の lib の歯が除外 1・測れない 1・上限 1 の fixture で 3 列の label（base では field が無い ＝ RED）と 1 口座 1 列を測り、pipe_ratelimit_breakdown_ の歯が候補なしの周の stdout の 3 欄と stderr の label と reset の在る周の 3 欄を測る"

[[contract]]
id = "o"
title = "doctor が便用の口座の数を出す — host の行の末尾に run-accounts=<n>（有効な口座 − 群の今の口座）を面が present の周だけ足し、snapshot と字面の歯を 1 欄分進める（§26・s2-07l.618 候補 2）"
req = ["FR73", "FR36"]
section = "26"
write-set = ["crates/scribe2/src/account/mod.rs", "crates/scribe2-boundary/src/snapshots/scribe2__tests__doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "crates/scribe2-boundary/tests/e2e/seat/rules.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_doctor_run_accounts_"]
size = "S"
growth = ["crates/scribe2/src/account/mod.rs:30"]
depends = ["n"]
done = "(1) 面が present の周だけ host の行の末尾に run-accounts=<n>（有効な口座の数 − 群の今の口座の数・§23 と同じ読み手・計測は撃たない）を足し、absent / unreadable の周は欄を足さない (2) 0 でも rc と他の行は 1 字も変わらない (3) snapshot 2 本と字面の歯を 1 欄分だけ更新する 歯: seat_doctor_run_accounts_ の歯が、宣言 3・群 1 で run-accounts=2（base では欄が無い ＝ RED）・退役 1 で 1・群 0 で 3・宣言 1・群 1 で run-accounts=0 と rc と他の行の不変・面 absent で欄なしを測る"
[[contract]]
id = "p"
title = "稼働中の便は群の移動を妨げない — 移り先の候補から 1 周の置き場の live 便が使っている口座の除外を外し、新規の便だけが群の記録（§23）で止まる（§27・裁定 2026-09-25T15:2xZ）"
req = ["FR38", "FR36", "NFR4"]
section = "27"
write-set = ["crates/scribe2/src/hook/group.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_"]
size = "S"
growth = ["crates/scribe2/src/hook/group.rs:0", "crates/scribe2/src/pipe/dispatch/group.rs:0"]
depends = ["l"]
done = "(1) 移り先の候補の規則は「今の口座でなく ∧ 他の群の今の口座でなく ∧ 退役中でなく ∧ 3 窓とも閾値未満の実測を持つ最初の label」で、live 便の読み（inflight_by_account）は候補の規則から消える (2) §23 の便用の除外・§20 形 6 の執行の順・lock・記録・承認 event・退避・起こし直しは 1 字も変わらない (3) 稼働中の便は止めず口座も替えない (4) 候補なしの内訳に live 便の理由が在れば消え、無ければ不変 (5) §20 形 5 の本文は live 便の項を持たず §27 形 1 と同じ規則で、便は §20 を触らない 歯: pipe_dispatch_group_move_ の live 便の歯を「live 便の在る口座へも移る」に書き換え（名も改める・live あり / なしで同じ移り先・base では live ありが次の候補へ飛ぶ ＝ RED）、移った直後の便用の選定がその口座を外す §23 の既存の歯は GREEN のまま、他の pipe_dispatch_group_ の歯は 1 字も変えず GREEN"

[[contract]]
id = "q"
title = "群の種は宣言順に重ならない — 面の読みが AccountGroup に種（前の群の種でない最初の候補）を埋め、解決の 1 関数が種の欄を読む（§28・§20 形 2 の改め）"
req = ["FR38", "FR57", "NFR4"]
section = "28"
write-set = ["crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/hook/group.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_seed_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_move_"]
size = "S"
growth = ["crates/scribe2/src/rules/manifest.rs:20", "crates/scribe2/src/hook/group.rs:5"]
depends = ["l"]
done = "(1) 面を読む 1 本が宣言順に各群の種（前の群の種でない最初の候補）を AccountGroup の欄に埋め、全候補が前の群の種に使われている群は面の欠陥（check_duplicate_groups と同じ file の Vec<RuleError> に RuleError::new(群の行番号, 文) の push を 1 つ足す・文は語「種を決める候補が無い」を含む・enum も struct も新しい file も足さない）で fail-closed (2) 群の今の口座の解決の 1 関数は記録 > 種の欄で、accounts の先頭を読まない (3) 記録の形・移動の判定・§23 の除外・面の表の形は不変 (4) 旧い種を pin する既存の歯 2 本（fleet.rs の host_group_run_exclusion_folds_the_current_accounts_of_the_groups と seat/account.rs の host_group_doctor_line_says_none_without_seat_rows）は名を変えずに新しい規則へ書き換えて GREEN、seat/account.rs の歯は fixture だけが動き base でも GREEN なので fn の中の行頭に flip-check: retroactive s2-07l.641 の札を 1 行付け、変異 A/B を notes に残す 歯: host_group_seed_ の歯が同じ列を宣言した 2 群の記録なしの周の current= を宣言順に先頭と 2 番目（base では両方先頭 ＝ RED）・候補 1 つを共有する 2 群の欠陥の行番号（2 番目の群の行）と語「種を決める候補が無い」・群 1 つの host は先頭のまま を測り、pipe_dispatch_group_move_ の記録ありの歯は 1 字も変えず GREEN"
[[contract]]
id = "r"
title = "群の移り先は残量の鍵と群の予約 — 候補を残量の鍵〔7 日窓と役割の model の 7 日窓の残量の小さい方 → reset → 宣言順〕で並べ、宣言順に導く群の予約（記録しない・便の起動は排他しない）へ移り、doctor の群の行に next= を出す（§29・ADR-0069）"
req = ["FR38", "FR36", "FR33", "AC41", "NFR4"]
section = "29"
write-set = ["crates/scribe2/src/hook/group.rs", "crates/scribe2/src/pipe/dispatch/group.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_reserve_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_judge_reserve_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_next_", "cargo nextest run -p scribe2 --lib --no-tests=fail group_key_"]
size = "M"
growth = ["crates/scribe2/src/hook/group.rs:140", "crates/scribe2/src/pipe/dispatch/group.rs:40", "crates/scribe2/src/seat/tick.rs:30", "crates/scribe2/src/account/mod.rs:30"]
depends = ["s"]
done = "(1) 残量の鍵は pure な 1 関数で、鮮度の内側の実測の行から min(100 − 7 日窓の使用率, 役割の model ごとの 100 − モデル別 7 日窓の使用率) の大きい順 → 7 日窓の resets_at の早い順（無い口座は後）→ 宣言の候補の index の辞書順に並べ、役割の model の集合（群の置き場の席の登録 row の役割ごとの rules 行 seat.model.<役割> の表示名・重複は畳む）のどれか 1 つでもモデル別窓の行を持たない口座は最後、集合が空の群は 7 日窓だけで並べ、役割の行が無い周は typed に止まり、5 時間窓は門にだけ使う (2) 群の予約は 1 関数で、群を宣言順に見て門（今の口座でなく・どの群の今の口座でもなく・退役中でなく・鮮度の内側の実測を持ち 3 窓とも閾値未満・鮮度の外は 1 回測る）を通る候補を鍵で並べたものから先の群の予約でない先頭を返し、移る群は自分の予約へ移り、宣言順で最初に門を通る label は返さない (3) 1 周の群の段と tick の判定は lock の内側で逼迫した群を判じる周にだけ、その群と前の群の予約を導き（前の群の候補も鮮度の外は測る・口座ごとに 1 周 1 回）、逼迫でない周は候補を測らず、群用 dir に予約の file は 0 (4) §23 の便用の除外と便用の並べ鍵は 1 字も変わらず、予約の立つ口座を便は使う (5) doctor の群の行に next=<label|none|unreadable> が current= の後ろに増え、同じ予約の関数を測らずに呼び、群 0 の host は 1 語も変わらない (6) 移動の契機・lock・記録・承認 event・退避・起こし直し・閾値の行・event の種類は不変 歯: pipe_dispatch_group_reserve_ が残量の逆順の 2 候補で後の候補へ（base では先 ＝ RED）・model の窓の残量で決まる 2 候補・同点は reset の早い方と宣言順・model の行を欠く候補は最後・5 時間窓は門だけ（2 本）・2 群同時の逼迫で Tier1 が鍵の先頭・Tier2 だけの逼迫でも Tier1 の予約を飛ばす（base では取る ＝ RED）・同じ周の便用の選定が予約の口座を使う・Tier2 だけの逼迫で Tier1 の候補が測られる を測り、seat_tick_judge_reserve_ が tick の判定も Tier1 の予約を飛ばすを、host_group_next_ が next= の label / none / unreadable を測り、既存の host_group_doctor_line_says_none_without_seat_rows は next= の増分で書き換えて GREEN、lib の group_key_ が並びを行の値だけで測る"

[[contract]]
id = "s"
title = "群の名は Tier と数字に限り宣言順は数字の昇順 — 面の読みが形と順を検査して外れる面を断り（fail-closed）、fixture の群の名を改める（§29・ADR-0069）"
req = ["FR38", "FR57", "AC41", "NFR4"]
section = "29"
write-set = ["+crates/scribe2/src/rules/groups.rs", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/seat/launch.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/seat/account.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "docs/design/account-lifecycle.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_tier_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_review_same_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_doctor_run_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_pane_shell_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_launch_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail init_repo_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail init_seat_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_init_"]
size = "S"
growth = ["crates/scribe2/src/rules/manifest.rs:3", "crates/scribe2/src/rules/mod.rs:3", "crates/scribe2/src/rules/groups.rs:80"]
done = "(1) 群の名は Tier の後ろに 10 進の数字 1 桁以上（先頭の 0 は不可）に限り、外れる名は群の見出し行の欠陥（check_duplicate_groups と同じ列 Vec<RuleError> に RuleError の new で 1 件・文は語「Tier と数字の形でない」を含む・enum も struct も足さない・検査の本体は write-set の + の file〔rules の兄弟 module・mod.rs に mod 宣言 1 行〕に在り manifest.rs に増えるのは呼び出し 1 行だけ） (2) 宣言順は数字の狭義の昇順で、前の群の数字以下の群は欠陥（文は語「前の群より大きくない」を含む・数値で比べる）、名が前の群と同じ周は昇順の欠陥を重ねず名の重複の 1 件だけ（既存の歯の件数の pin は名の改めだけで GREEN）、名の重複の検査は残る (3) 読めない面は全群を typed に止める今の規則のまま、優先は宣言順のままで数字は並べ替えに使わない (4) e2e 8 file の fixture の群の名を Tier と数字に改め、assert の他の語は変えず、群の fixture に到達する既存の歯 128 本（§29 の census・検証行の接頭辞 12 本が全部を撃つ）は GREEN で、改名だけの 7 file には file ごとに札 flip-check: retroactive <本 bead> を 1 行置き（4 条件・rules.rs は新しい歯が base で赤なので札なし）、変異 A/B（Tier でない名を通す・数字を辞書順で比べる）の撃墜と母集団を notes に残す 歯: host_group_tier_ が alpha の群の欠陥（行番号は群の行・base では通る ＝ RED）・Tier2, Tier1 の順の 2 番目の行の欠陥・Tier1, Tier1 の名の重複の 1 件だけ・Tier01 の形の欠陥・Tier1, Tier2, Tier10 は通る を測る"
<!-- contracts:end -->
