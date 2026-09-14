# 設計: 口座の生涯 — 口座の宣言は host の manifest が持ち、登録・一覧・退役・席の起動を器の口が担う（外の wrapper に頼らない）

- 決定: [ADR-0026](../../design-intent/decisions/ADR-0026-account-lifecycle-host-manifest-and-seat-launch.html)（§2.1 host の manifest / §2.2 口座の口 / §2.3 席の起動 / §2.4 supersede）
- 要件: SRS FR33（口座残量の計測・credential の場所）/ FR36（口座の選定）/ FR38（席の退避と立て直し）/ FR40（席の登録）/ AC13（別口座での立て直し）が正本。口座の登録・退役と席の起動そのものを名指す要件は SRS の次の改訂で足す（材料は planner の置き場・改訂は user の folio-architect）。改訂までは本設計の契約は上の id を指す。
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

- **`account add <label> [--anchor DIR] [--target T]`**: (1) label を manifest の規則で検査し、宣言済み（退役中を含む）なら typed に断る（`exists`）。(2) `<state_dir>/accounts/<label>/` を dir として作る（既に dir か link が在れば `dir-exists`・中身は読まない）。(3) 直下に `settings.json` を書く（内容は器が起こす席の前提だけ = agent view を切る 1 項目・[account-autonomy.md](./account-autonomy.md) §5 (4)。user が置いた既存 dir には書かない＝(2) で断るので到達しない）。(4) `host.toml` に `[[account]]` 行を 1 つ足す（読み → 検査 → 一時 file → rename・部分書きを残さない）。(5) login は **user の手番**（器は credential に触れない・代筆しない・ADR-0017 §2.3）: `--target T` が在れば、起動行 `CLAUDE_CONFIG_DIR=<dir> <claude>`（`--anchor` を cwd として・trust の dialog を同じ session で通すため）を T の shell へ §5 の門を通して注入し、無ければ同じ行を stdout に出す（`account: prepared <label> next=<行>`）。login の完了は器が待たない（`account ls` / doctor の `credential=` が示す）。
- **`account ls`**: doctor の口座行（`s2-07l.233` の形・label ごとに 1 行・`dir` / `credential` / `config` / `agentview` / `trust`）と**同じ関数**で行を作り、`retired=<yes|no>` と最新の実測行の要約（`five_hour=<pct|unmeasured> seven_day=<pct|unmeasured>`・event log を読むだけ・計測は撃たない）を足す。judgement を持たない（C10.2）。
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

## 5. 注入の門（共通・値を持たない）

[account-autonomy.md](./account-autonomy.md) §5 の「shell への注入の門」をそのまま使う（前面 process が shell・prompt 末尾の閉じた列・特定できない周は送らない）。`account add --target` と `seat launch` と立て直しの 3 つが同じ関数を呼ぶ（新しい判定を足さない）。

## 6. 極性（[polarity.md](./polarity.md)）

- host の manifest が読めない（在るが壊れている）: **FailClosed**（`Guard::Rules` の既存の断りに host の面が加わる・manifest の拒否と同じ極性・NFR4）。無い周は縮退（0 宣言・止めない）。
- `account add` / `retire` / `restore` の前提違反（`exists` / `in-use` / `unknown` …）: 断って何も書かない（file も event も・部分書きなし）。guard ではない（行為を止めうる判定ではなく入力の拒否・ADR-0014 §2.1）。
- `seat launch` の門: `Guard::Inject` の既存の極性（送らない側）。新しい Guard variant は足さない＝極性一覧の行数は不変。

## 7. 失敗の型

- `HostManifest`: `Absent`（縮退）/ `Unreadable`（FailClosed）/ 検査の error は manifest の `RuleError`（行番号付き）をそのまま使う。
- `AccountError`（closed enum・`as_str`）: `exists` / `dir-exists` / `unknown` / `already-retired` / `not-retired` / `in-use` / `label-invalid` / `write-failed`。
- `LaunchError`（closed enum・`as_str`）: `no-account` / `account-unknown` / `account-retired` / `session-missing` / `input-busy` / `input-unknown` / `register-failed`。
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

- **(a) host の manifest**（M）: §2。write-set = `rules/manifest.rs`（`Section::Plugin` / `Section::LaunchArg`・host の面の読み・面をまたぐ重複）・`rules/mod.rs`（loader の入口に state dir を渡す）・`rules/cli.rs`（`validate --state-dir`）・`fleet/usage.rs` / `fleet/select.rs` / `seat/tick.rs` / `main.rs`（doctor の行）の呼び手・`rules/manifest.toml`（口座行を外す）・`tests/e2e/rules.rs` / `fleet.rs`・外形 snapshot（`src/snapshots/`・doctor と usage）。依存: `.233`（doctor の口座行）の Landed 後（doctor の行の隣に足す・`src/snapshots/` の交差）。base で RED = `rules validate --state-dir` が host の面を数える歯（機能不在）。
- **(b) 席の起動**（M）: §4 / §5。write-set = `seat/cli.rs`（`launch` の flag と usage）・`seat/cycle.rs`（`derive_launch`・launch と relaunch の共通経路・`new-window`）・`seat/inject.rs`（`InjectKind::Launch`）・`fleet/mod.rs`（`Registration.sid: Option<String>`・literal 構築点・KINDS は不変）・`seat/role.rs`（sid の読み手）・`tests/e2e/seat.rs`・seat の外形 snapshot。依存: (a)。base で RED = `seat launch` の偽 tmux の歯 + `derive_launch` の in-file の歯（機能不在）。
- **(c) 口座の口**（M）: §3。write-set = 新 module `account/`（`mod.rs` / `cli.rs`・歯は in-file `#[cfg(test)]` + `tests/e2e/account.rs` は足さない〔統合 test target の上限・`fleet.rs` に置く〕）・`main.rs`（dispatch と usage）・`fleet/mod.rs`（`EventKind::AccountRetired` / `AccountRestored`・replay の退役集合・`KINDS` +2）・`fleet/select.rs` / `fleet/usage.rs`（有効な口座の集合を読む）・`main.rs` の doctor の行（`retired=`）・`tests/e2e/fleet.rs`・外形 snapshot。依存: (a)・(b) と `fleet/mod.rs` で交差するので直列（(b) の後）。base で RED = `KINDS.len()` の pin を 15 にする歯 + `account add` の歯（機能不在）。

順序の理由: (a) は `.220` の穴を塞ぐ土台で他の 2 便が読む。(b) が user 裁定 2026-09-14 の「席の起動を `.222` の後・`.208` run 5 の前」の便。(c) は (b) と `fleet/mod.rs` で交差するので後。

## 11. 却下案（ADR-0026 §5 の写しは持たない・設計固有のもの）

- `account add` が login の session を器の子 process として起こす。却下: 器の子 process は封じ込めの箱の中（NFR6）で、対話の login を箱に入れる理由が無い。席と同じく shell へ注入する（子にしない）。
- `<state_dir>/accounts/` を走査して口座を数える。却下: ADR-0017 §2.3（真実は宣言・C3）。dir が在っても宣言の無い口座は「無い」。
- 退役を `host.toml` の行の削除で表す。却下: 退役は状態（C3「退役は DB」）で、宣言 file の書き換えは可逆 move にならない（行を失う）。event + mv。
- 起動行の雛形 file を `seat launch` の引数に残す（`--launch FILE`）。却下: 雛形が手書きに戻り host 固有の値が file に散る（C10.2）。host 固有の値は `[[plugin]]` / `[[launch-arg]]` の行で持ち、行は器が組む。
- 席の起動に cgroup の防壁を持ち込む。却下: 席は shell の子で器の子ではない（立て直しと同型）。防壁が要るのは便の runner で、それは [gate-cost.md](./gate-cost.md) の封じ込めが持つ。
- 前の版の launcher と別 repo の口座切替 wrapper を器から呼ぶ。却下: 器の視野の外の script に依存する形（ADR-0009 §1 の根因と同型）。

## 12. 後続

user-scope MCP 設定の同期・別口座への `--resume` の混線 fence・primary 口座の写像（user 裁定 2026-09-14「持ち込まない」）／再 login（credential の失効・user の手番・doctor が示す）／退役した口座の dir の削除（A1・削除の口は持たない）／launch 直後に止まった席の起こし直し（[account-autonomy.md](./account-autonomy.md) §11 の crash と同じ）／tracked の manifest に口座行を戻す形（A1）。
