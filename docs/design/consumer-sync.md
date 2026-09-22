# 設計: 導入先の同期 — 器が版を測り、器が更新し、hook 集合の食い違いは席を作り直す（散文の手順を持たない）

- 決定: [ADR-0028](../../design-intent/decisions/ADR-0028-consumer-sync-is-measured-and-updated-by-the-vessel.html)（§2.1 build 元 commit / §2.2 読み込み元の記録 / §2.3 install の event と repo の宣言 / §2.4 hook 集合の食い違い / §2.5 役割なしの起動行）
- 要件: SRS [FR61](../../design-intent/spec/srs.html#FR61)（導入先の同期の測定と更新の口）/ [FR62](../../design-intent/spec/srs.html#FR62)（hook 集合の食い違いで席を作り直す）/ [AC31](../../design-intent/spec/srs.html#AC31) / [AC32](../../design-intent/spec/srs.html#AC32) が正本。役割なしの起動（FR60 / AC30）は [account-lifecycle.md](./account-lifecycle.md) §4.5 が持つ（同じ ADR の §2.5）。土台 = FR51（doctor の項目）/ FR19（SessionStart の名乗り）/ FR29 / FR38（退避と立て直しの既存経路）/ FR23（復元の DATA）/ FR57（host の面）。
- 土台: [vessel-hook.md](./vessel-hook.md) §4（`session-start`）・[seat-state.md](./seat-state.md) §2（席の打刻の置き場）・[seat-autonomy.md](./seat-autonomy.md) §3（tick の判定の軸〔機構ごと削除済み〕）・[account-autonomy.md](./account-autonomy.md) §5（退避後の終了の手と立て直し）・[account-lifecycle.md](./account-lifecycle.md) §2（host の manifest の表）・[fleet-event-log.md](./fleet-event-log.md) §3（event の schema）
- 語彙: `design-intent/vocabulary.yaml`（導入先・build 元 commit・読み込み元・hook 集合の食い違い）

## 1. 何を解くか

器は plugin（hooks.json・skill）と binary（PATH の `<NAME>`）の 2 つの面で導入先（consumer = 本 repo 以外の repo で器の hook を積む project と、その worktree）に入る。記録時点（2026-09-15・実測）の穴は 4 つで、どれも「黙って古くなる」型である:

- **binary の出所が測れない**: `<NAME> --version` は `CARGO_PKG_VERSION` だけ（`main.rs` `render_version`）で build 元 commit を持たない。install した binary が古いかは file の mtime の推測でしか分からず（auto-memory の事故: event schema の追加で PATH の binary が静かに古くなり hook が events を読めなくなった）、C10 の provenance が無い。
- **plugin の読み込み元が測れない**: Claude Code の directory source の plugin は cache（`installed_plugins.json` の `installPath`）でなく **repo の作業ツリーを直接読む**（実測 2 例・保証なし）。install の帳簿（`gitCommitSha`）と cache の写しは install 時刻のまま止まり、読み込み元が cache へ切り替わる版が来ると黙って古い hook 集合に戻る。cache は口座の設定 dir ごとに 1 つを全 consumer・全 worktree が共有し、最後の install が上書きする。
- **更新の手順が散文**: 「ff → `cargo install --path crates/<NAME> --locked` → consumer の planner へ 1 行通知」は planner の作業記憶の行で、規則ではない（N2）。誰が・いつ・どの sha を入れたかの記録も無い（C6.3 の append-only の外）。
- **hook 集合は session の起動時の snapshot**: `/clear` は session を作り直さないので、hooks.json に event / matcher が増えた版は走行中の席に載らない（実測）。席が古い配線のまま動く期間を器が測れず、直せない。

本設計は、(1) build 元 commit を binary に焼き、(2) SessionStart が読み込み元（hooks.json の path と digest）を席ごとに記録し、(3) doctor が consumer ごとに 1 行で食い違いを名指し、(4) 1 つの口 `vessel update` が ff → build → install を行って event を 1 件残し、(5) tick が hook 集合の食い違いを見つけたら既存の経路（退避 → 終了の手 → 立て直し）で席を作り直す。散文の手順は残さない。

## 2. build 元 commit（ADR-0028 §2.1・台帳 `s2-07l.302`）

- **焼く場所**: `crates/<NAME>/build.rs`（新規・依存なし）。`std::process::Command` で `git rev-parse HEAD` と `git status --porcelain`（tracked の変更の有無）を読み、`cargo:rustc-env=<ENV_PREFIX>_BUILD_COMMIT=<sha12>[+dirty]` を出す。git が無い・repo でない・失敗した周は `unknown`（失敗を成功に倒さない・C10）。`cargo:rerun-if-changed=.git/HEAD` と `.git/refs/heads/` を出して stale を避ける。
- **読む場所**: 実行時は `env!(…)`（compile time の値・実行時に env を読まない＝C2.2 の外・xtask の `env-reads` は `std::env` の参照だけを数えるので母集団に入らない〔契約化時に fact で実測する〕）。
- **外形**: `<NAME> --version` = `<NAME> <CARGO_PKG_VERSION> (<sha12>[+dirty])`（`unknown` は `(unknown)`）。doctor の 2 行目も同じ関数（`render_version`）。外形 snapshot の `[version]` の mask は sha と `+dirty` と `unknown` の 3 形を受ける。
- **意味**: この値は「どの source から build したか」の宣言値で、install 済みかどうか・最新かどうかは言わない（比べるのは §4 の doctor）。

## 3. 読み込み元の記録（ADR-0028 §2.2・台帳 `s2-07l.303` / `.304` の土台）

- **hook の引数**: hooks.json の全 command に `--plugin-root "$CLAUDE_PLUGIN_ROOT"` を足す（`$TMUX_PANE` / `$CLAUDE_PROJECT_DIR` と同じく shell が展開し器は引数で受ける＝器は env を読まない・C2.2。plugin.json / hooks.json は tracked の生成物ゆえ xtask check の drift が守る）。値が空の周（plugin の外から撃った hook・fixture）は記録しない（黙る・止めない）。
- **記録**: `session-start` は名乗りの後に、席の打刻 dir（[seat-state.md](./seat-state.md) §2 の `<state_dir>/seat/<target>/`）へ **`plugin` 1 file 1 行**を書く（毎 SessionStart に上書き＝最新 session の値・write-ahead は要らない〔読めない周は「未記録」〕）: `schema=1 sid=<sid> root=<plugin root> hooks=<digest> binary=<build 元 commit> ts=<秒>`。`hooks=` の digest は `<root>/hooks/hooks.json` の bytes の **FNV-1a 64**（16 hex・std だけ・release を跨いで不変・persist する値に `DefaultHasher` は使わない〔std の hasher は版で変わりうる〕）。file が無い・読めない周は `hooks=unreadable`（記録はする＝doctor が名指す）。
- **意味**: 「この session は、この場所の、この hooks.json で起動した」の実測値（provenance 付き・C10）。cache か作業ツリーかを器が推測しない（Claude Code の挙動に依存しない）。

## 4. doctor の導入先の行（ADR-0028 §2.3・FR61・台帳 `s2-07l.303` (a)）

- **導入の形（user 裁定 2026-09-15T03:52Z・台帳 `s2-07l.303` notes）**: 器が起こす席（`seat launch` / 立て直し / 役割なしの起動・[account-lifecycle.md](./account-lifecycle.md) §4）は、器の plugin を **checkout（`[[vessel]] repo` の dir・下の「vessel repo」）を host の manifest の `[[plugin]]` 行に書いて `--plugin-dir` で積む**。marketplace の add / `claude plugin install`（口座 × project の掛け算）は器の席には要らず、要件にしない。帳簿（`installed_plugins.json`）が要るのは **user が手で起こす session だけ**で、doctor は下の `source=` でその区別を名指す。同じ器を `--plugin-dir` と帳簿の両方で積んだ session は hook が二重に走るので `drift=dual` で名指す（器は帳簿を書かない・直すのは user の手番）。
- **母集団**（1 関数・和集合・順序 = 発見順を sort）: (i) 席の登録 row（[seat-roles.md](./seat-roles.md) §2）の anchor（`source=launch`・読み込み元は §3 の記録の `root`）(ii) 口座の設定 dir（`<state_dir>/accounts/<label>/`・[account-lifecycle.md](./account-lifecycle.md) §2 の有効な口座）ごとの `plugins/installed_plugins.json` の `plugins["<NAME>@<NAME>"][*]`（`projectPath` / `scope` / `installPath` / `gitCommitSha`・`source=install`・読むだけ・書かない・無い口座は飛ばす・壊れている周は `ledger=unreadable` の 1 行）。同じ path は 1 行に畳む（両方に在れば `source=launch+install`）。
- **行の形**（consumer 1 つに 1 行・`doctor --state-dir S` の口座行の後ろ・値は全部実測か `unknown` / `unrecorded`）:
  `consumer=<path> source=<launch|install|launch+install> scope=<project|local|user|-> binary=<記録の build 元 commit|unrecorded> plugin=<記録の root>:<hooks digest|unrecorded> ledger=<gitCommitSha|-> cache=<installPath の hooks.json の digest|absent> head=<vessel repo の HEAD|undeclared> drift=<none|binary|plugin|ledger|dual|binary+plugin|…>`
  - `binary` の食い違い = 記録の build 元 commit ≠ doctor 自身の build 元 commit（§2・doctor は PATH の binary そのもの）。
  - `plugin` の食い違い = 記録の digest ≠ 記録の root に今在る hooks.json の digest（同じ場所の file が変わった＝作業ツリーの前進か cache の上書き）。`source=launch` の行では root は checkout そのもの＝食い違いは「checkout が前に進んだのに席が古い snapshot のまま」を意味し、§6 の作り直しが直す。
  - `ledger` の食い違い = 帳簿の `gitCommitSha` ≠ vessel repo の HEAD（`source=install` の行だけ・帳簿が古い＝`claude plugin install` の打ち直しが要る周を名指す・器は帳簿を書かない）。`source=launch` だけの行は `ledger=-` で、この語は出ない。
  - `dual` = 記録の `root` が checkout で、かつ同じ path の帳簿にも器が在る（hook が二重に走る）。
  - `drift=` は該当する語を `+` で繋ぐ（閉じた列・宣言順）。記録が無い consumer は `unrecorded` で「none」に潰さない。
  - `statusline=<vessel|other|absent|unreadable>`（ADR-0029 §2.3・台帳 `s2-07l.320`）= consumer の project scope の `.claude/settings.json` の `statusLine.command` が器の行と一致するか。口座行の同名の語と同じ 1 関数で読む（`vessel` 以外は口座の設定を上書きしている印・判定はしない）。行の末尾に足す。
- **vessel repo**: `head=` と §5 の更新は器自身の checkout を要る。host 固有の path なので **host の manifest**（[account-lifecycle.md](./account-lifecycle.md) §2 の `host.toml`）に array-of-tables を 1 種足す: `[[vessel]] repo = "<dir>"`（最大 1 行・2 行目は重複として拒む・同じ loader・同じ拒否形）。無い周は `head=undeclared`（doctor は止めない）。
- **判定しない**: doctor は行を出すだけ（C10.2・verified の手書きは無い）。何をすべきかは `drift=` の語が名指し、更新は §5 の口が行う。

## 5. 1 つの口 `vessel update`（ADR-0028 §2.3・FR61・台帳 `s2-07l.303` (b)・user 裁定 2026-09-15「ff してから build + install」）

- **口**: `<NAME> vessel update --state-dir S [--remote R] [--branch B]`。`[[vessel]] repo` が無ければ `vessel-repo-undeclared` で断る。
- **順序固定**: (1) `git -C <repo> status --porcelain` が非空なら `dirty` で断る（作業ツリーを動かさない・N1）。(2) `git fetch <remote>` → `git merge --ff-only <remote>/<branch>`（既定 = `origin` / `main`・ff できない周は `not-fast-forward` で断る・rebase も reset もしない）。(3) `cargo install --path crates/<NAME> --locked`（PATH の binary を入れ替える・`--locked` は nextest と同じ前提）。(4) fleet の event log に **`InstallRecorded`**（`EventKind` の新 variant・宣言順の末尾・`KINDS` +1・schema 1 のまま）を 1 件: `{ sha: <install した HEAD の sha12>, host: <hostname>, path: <cargo が報告した binary の path> }`。(5) stdout に 1 行 `vessel: installed sha=<sha12> path=<path>`。
- **何を書かないか**: consumer の帳簿（`installed_plugins.json`）も cache も書かない（他人の帳簿・ADR-0028 §5）。consumer の席への通知も送らない（§6 の tick が食い違いを測って動く＝通知の散文を無くす）。
- **断りは閉じた型**（順序固定の各段の失敗を 1 つの enum で持ち、rc は既存の拒否と同じ・散文にしない）: 宣言が無い（`vessel-repo-undeclared`）／作業ツリーが汚れている（`dirty`）／ff できない（`not-fast-forward`）／build と install が落ちた。**どの断りでも (4) の event は 0 件**（成功した周だけ 1 件・C10＝「撃った」と「入った」を融合しない）。
- **口の側の連鎖（現物・verified 2026-09-22・main af56643＝pipeline.md §14 の行 f の着地後）**: `vessel` の面の引数の閉包は `crates/scribe2/src/hook/vessel.rs` の中で閉じる——verb ごとの allowed の const（`ALLOWED_INIT` / `ALLOWED_BARE`）と `dispatch` の verb の match が同 file に在り、共通 module `crates/scribe2/src/cli_args.rs` の `parse` を呼ぶだけで、verb や flag の一覧を共通 module は持たない。`update` を足す便は同 file に `--remote` / `--branch` を持つ allowed の const 1 つと match の腕 1 本と `usage` の 1 語を足す＝write-set は `hook/vessel.rs` で足り、`cli_args.rs` は触らない。usage の外形は `vessel_external_form` の snapshot（write-set と verify に在る）が受ける。
- **kind を足す連鎖（現物・verified 2026-09-20）**: `EventKind` の variant を 1 つ足す便は、宣言順の列（`crates/scribe2/src/fleet/mod.rs` の `KINDS`）・replay の網羅 match・`fleet record` が**手で書けない kind**として拒む側・`fleet` の外形を同じ PR で動かす（`fleet` の外形 = `crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap`・歯 `fleet_external_form`・write-set と verify に持つ＝2026-09-20 の審査 FAIL「fleet 側の外形の写しが無い」の再現）。記録時点で `InstallRecorded` は src に 0 件＝本行が新設する。**`crates/scribe2/src/fleet/usage.rs` は面に入らない**（`EventKind` を 2 つ構築するだけで網羅 match を持たない・同 file は上限の余地が 192 行で size M の見積に足りず、write-set に残すと受付が `cap-headroom` で断る＝2026-09-20 の preflight で実測）。
- **歯**（接頭辞 `vessel_update_`・置き場は `crates/scribe2/tests/e2e/hook.rs`〔口の側〕と `crates/scribe2/tests/e2e/fleet.rs`〔event の側〕・偽 `git` と偽 `cargo` は PATH の先頭に置く shim で argv を写す）:
  1. **順序**: 成功の周に写った argv が `status --porcelain` → `fetch` → `merge --ff-only` → `cargo install --path … --locked` の**この順**で、`InstallRecorded` が 1 件（sha12 と host と path を持つ）記され、stdout が 1 行（sha12 と path）である。
  2. **断り 4 形**（否定の枝・1 形 1 本）: 宣言が無い／`status --porcelain` が非空／`merge --ff-only` が rc ≠ 0／`cargo install` が rc ≠ 0 の各周で、**その段より後の argv が 1 本も写らず** event は 0 件で、断りの語が stdout か stderr に出る。
  3. **kind の連鎖**: `fleet record` の口にこの kind を手で渡すと拒まれる（run の kind と同じ扱いにしない）。
  4. **宣言順の pin**は既存の歯が受け、行の verify が完全名 `fleet_kinds_follow_declaration_order`（`crates/scribe2/tests/e2e/fleet.rs`・`KINDS` の並びと母集団の件数）で撃つ＝variant を足して列に足し忘れた周はここで赤になる。
  5. **口の外形**は既存の歯が受け、行の verify が完全名 `vessel_external_form`（`crates/scribe2/tests/e2e/hook.rs`・snapshot `e2e__hook__vessel_external_form.snap`）で撃つ＝`update` の usage 行の追加がそこに写る。
- **子 process**: `git` と `cargo` は器の子（`std::process::Command`・timeout は既存の唯一の wait・出力は `--color never` で読む〔auto-memory の CI 色の型〕）。失敗は typed（`fetch-failed` / `install-failed` に rc を添える）。(4) の追記に失敗した周（store が書けない）は `record-failed` で断る（install は済んでいる＝binary は新しく `InstallRecorded` は 0 件・§6 の tick が食い違いを測る側・rc は 2）。
- **A1**: 消す / 出す / 使う のどれでもない（local の build と install・push しない・課金しない）。

## 6. hook 集合の食い違いで席を作り直す — **超過**（ADR-0045 §2 (2)）

- 本節の設計（tick の軸を 1 つ足し、digest が違う席へ退避の合図を注入して終了の手 → 立て直しへ進ませる）は、
  軸を持つ管理 tick が `s2-07l.479.1` で、退避の合図と復元の DATA が `s2-07l.479.2` で、席の状態の最終行の読みが
  `s2-07l.479.3` で、それぞれ機構ごと消えたため**超過した**。本文は git の履歴に在る。
- **残るもの**: 導入先の食い違いは doctor の行が持つ（§4 の `drift=`・`[[vessel]]` の記録と今の hooks.json の digest の比較）。
  更新の判断と実行は user の手番で、器は測って見せるだけである。

## 7. 極性（[polarity.md](./polarity.md)）

- Guard は増やさない（極性一覧の行数は不変）。§6 の軸は judge（inject / noop）で、§4 の doctor は行を出すだけ、§5 は入力の拒否（typed な断り・書く前に断る）。
- `plugin` 記録が読めない周: 記録の**不在**として扱う（doctor は `unrecorded`・tick は noop の理由 1 つ）。「無い」と「読めない」は語で分ける（`unreadable`）。
- `vessel update` の断り（`dirty` / `not-fast-forward` / `fetch-failed` / `install-failed` / `vessel-repo-undeclared`）は何も動かさない（(1)(2) は作業ツリーを変えない・(3) が失敗した周は旧 binary が残る・event は (3) 成功の後だけ）。

## 8. 失敗の型

- `UpdateError`（closed enum・`as_str`）: `vessel-repo-undeclared` / `dirty` / `not-fast-forward` / `fetch-failed` / `install-failed` / `record-failed`。
- `PluginRecord`（読み）: `Recorded(…)` / `Absent` / `Unreadable`。
- `Drift`（doctor・closed・宣言順 = 出力順）: `Binary` / `Plugin` / `Ledger`。空 = `none`。
- すべて Result で呼び手に分岐を強いる（C11.3）。

## 9. 歯（`crates/<NAME>/tests/e2e/` に `vessel_version_` / `hook_plugin_record_` / `doctor_consumer_` / `vessel_update_` / `seat_tick_hook_drift_` 接頭辞・名前の列は現物が SSOT）

- §2: `--version` の行が `(<sha12>)` か `(<sha12>+dirty)` か `(unknown)` の 3 形のどれか（外形 snapshot の mask）／`build.rs` の関数は in-file の歯（git の無い tmp で `unknown`）。
- §3: fixture の hook を `--plugin-root <tmp>` 付きで撃つと `seat/<target>/plugin` に 1 行・digest は tmp の hooks.json の FNV-1a と一致／`--plugin-root` 無し・空は file を書かない／hooks.json が無い周は `hooks=unreadable`。
- §4（AC31）: tmp の state dir に口座 2 つ（片方の `plugins/installed_plugins.json` に consumer 2 つ・片方は worktree の path）と `[[vessel]] repo` を置き `doctor --state-dir` を撃つ → consumer ごとに 1 行・`drift=` が `none` / `binary` / `plugin` / `ledger` の 4 語をそれぞれ出す fixture 4 つ／記録の無い consumer は `unrecorded`／帳簿が壊れていれば `ledger=unreadable` の 1 行で他の行は出る／`[[vessel]]` 無しは `head=undeclared`。
- §5（AC31）: 偽 `git`・偽 `cargo`（argv を写す stub・PATH の先頭）で `vessel update` を撃つと順序 (1)→(4) の argv が写り `InstallRecorded` が 1 件・stdout 1 行／dirty な repo・ff できない repo・cargo が rc 101 の周はそれぞれ typed に断り event 0／`KINDS.len()` の pin が +1。
- §6（AC32）: 偽 tmux で席を立て `plugin` 記録に digest A を置いた後 root の hooks.json を B に変える → tick が `kind=externalize origin=hook` を注入／退避 → Stop の後に `/exit` → 立て直しが同じ target・同じ口座（閾値未満）で走る／hooks 同じで binary だけ違う周は noop（理由 1 つ）。
- 実地（done の一部・歯にしない）: 本 host で `vessel update` を 1 回撃ち、consumer（folio2 / ubuntu-note-system）の doctor 行が `drift=none` になり、hooks.json を変えた便の後に consumer の planner 席が器の手で作り直されること。

## 10. 憲法・制約との整合

C1（rules 行を足さない・閾値は無い）・C2 / C2.2（`EventKind` / `Drift` / `UpdateError` は variant で増やす・env は hook の引数と compile time だけ・NAME から crate と ENV_PREFIX を導く）・C3（真実は event log と席の記録・帳簿と cache は読むだけ）・C3.2（doctor が版を全 consumer で突合）・C6.3（install の記録は append-only の 1 store）・C9（止まった席を人手なしで作り直す）・C10 / C10.2（実測と `unknown` / `unrecorded` を型で分ける・host 固有の path は host の manifest にだけ）・C11.2 / C11.3（typed な断り・Guard は増減なし）・N1（ff-only・作業ツリーを動かさない）・N2（手順の散文を持たない＝本 doc の「暫定運用」は §12 の契約 Landed で消える）・A1 非該当（§5 は local の build・push しない）。

## 11. 却下案（ADR-0028 §5 の写しは持たない・設計固有のもの）

- 通知の散文（install ごとに planner が consumer の planner へ 1 行送る）。却下: 器の視野の外の作法（N2）。tick が測って動く形に置き換える。
- Claude Code の cache の写しを正とし、器が cache と作業ツリーを同期する。却下: 読み込み元が版で変わる挙動に器が依存する。器は読み込み元を**記録**して比べるだけ。
- `installed_plugins.json` を器が書く（帳簿の `gitCommitSha` を進める）。却下: 他人の帳簿（Claude Code の所有）で、形が版で変わる。食い違いを `ledger` の語で名指し、打ち直しは user / consumer の planner の手番。
- hook 集合の食い違いを `/reload-plugins` で解く。却下: 走行中の hook 登録を差し替えるかは未確認（uncertain）で、器は描画も結果も読めない（C3.3）。作り直しは既存の経路で確実に効く。
- digest に `DefaultHasher`。却下: std の hasher は release で変わりうる（persist する値に使うと偽の食い違い）。FNV-1a 64 は 10 行の pure 関数で依存なし。
- `vessel update` が `git pull`（merge を含む）や `reset --hard` を撃つ。却下: 作業ツリーを動かす（N1）。ff-only で揃わない周は人の手番として断る。

## 12. 契約（4 便・この順・実装は pipeline）

- **(a) build 元 commit**（S・`s2-07l.302`）: §2。write-set = `+crates/<NAME>/build.rs` / `crates/<NAME>/src/main.rs`（`render_version`）/ `crates/<NAME>/src/snapshots/`（doctor / version の外形）/ `crates/<NAME>/tests/e2e/`（version の歯の置き場は現物の module）。依存: なし。base で RED = `--version` の行に `(` が在る歯（機能不在）。
- **(b) 読み込み元の記録 + doctor の導入先の行 + `vessel update`**（M・`s2-07l.303`）: §3 / §4 / §5。write-set = `plugin/hooks/hooks.json`（`--plugin-root`・生成 dir の下は §17）/ `crates/<NAME>/src/hook/mod.rs`（引数・記録の書き手）/ `crates/<NAME>/src/hook/vessel.rs`（`update`）/ `crates/<NAME>/src/rules/manifest.rs`（`[[vessel]]`）/ `crates/<NAME>/src/fleet/mod.rs`（`InstallRecorded`・`KINDS` +1）/ `crates/<NAME>/src/main.rs`（doctor の行）/ `crates/<NAME>/src/vessel/`（消費者の母集団・`Drift`・digest の pure 関数の置き場は現物で決める）/ `tests/e2e/hook.rs` / `tests/e2e/fleet.rs`（`KINDS` の pin）/ `tests/e2e/rules.rs` / doctor と hook の外形 snapshot。依存: (a)（記録に build 元 commit を載せる）。base で RED = `--plugin-root` の記録の歯 + `doctor` の consumer 行の歯（機能不在）。
- **(c) 合図の出所と終了の手の弁別 + 立て直しの口座の優先**（M・`s2-07l.307`）: §6 の 2 点目・3 点目。write-set = 管理 tick の module〔削除済み〕（`.279` の分割後の file 名で焼く）/ `seat/inject.rs`（`origin`）/ `seat/cycle.rs`（relaunch の口座）/ `tests/e2e/seat.rs` / seat の外形 snapshot。依存: `.279` Landed。base で RED = context 由来の合図の後に `/exit` が出ない歯（現物は出る）。
- **(d) hook 集合の食い違いの軸**（M・`s2-07l.304`）は、軸を持つ管理 tick ごと削除済み（`s2-07l.479.1`）で、`[PLUGIN]` の DATA も削除済み（`s2-07l.479.2`）＝この便は超過した。

順序の理由: (a) は (b) の記録が載せる値。(b) は独立に Landed できる（tick を触らない＝`.279` と交差 0）。(c) は `.279` の分割を待つ tick の便で、(d) は (b)(c) の両方を読む。

## 13. 後続

`claude plugin install` の打ち直しを器が撃つ形（帳簿は他人のもの＝当面は doctor が名指すだけ）／consumer 側の state dir の一覧を host の manifest に宣言する形（今は口座の帳簿と登録 row から導く）／`vessel update` が消費者の席へ結果を報せる形（通知でなく tick が測る側に倒したので当面は無し）／binary の食い違いだけの席を軽く直す形（今は次の hook の起動に任せる）。

## 14. doctor の consumer 行に statusline= を足す（契約表の行 f・`s2-07l.325`）

- 何が起きているか: ADR-0029 §2.3（doctor の導入先の行に `statusline=<vessel|other|absent|unreadable>`）と本 doc §4 の行の末尾の語仕様を承け、`.320`（`StatusLine` の enum と口座行の語）から切り出した便。現物（verified）: consumer 行は `crates/scribe2/src/account/consumers.rs` の 2 形（記録あり・unrecorded）で組まれ in-file の歯が近くに在るが、`statusline=` の語は src に 0 件。
- 形: consumer 行（2 形とも）の末尾に `statusline=<vessel|other|absent|unreadable>` を足す。値は consumer の project scope の `.claude/settings.json` の `statusLine.command` を、口座行と**同じ 1 関数**（seat/statusline.rs の読み・`.320` が新設）で読む。`unrecorded` の行でも settings.json は読めるので語は出す。
- 触らない: `drift=` の語の列（`statusline` は drift の語ではない・判定しない）・口座行・`.320` の関数の中身・`docs/`。
- 依存: `.320` Landed が前提（seat/statusline.rs は本便の受付時点でまだ着地していない＝行の write-set では + で名指し、.320 Landed 後に素の path へ焼き直す）。
- 却下案: consumer 行で settings.json を独自に読む（口座行と 2 実装になる・C2）／`statusline` を `drift=` の語に入れる（上書きの有無は判定でなく事実の名指し・C10.2・設計 §4「判定しない」）。

## 15. vessel repo の checkout が上流の既定 branch から behind の周に、終端の周が §5 の口を撃つ — 更新の運用を列の 1 周の構造で強制し、doctor の consumer 行に `behind=` を足す（契約表の行 g・`s2-07l.408`）

- 何が起きているか: 別 host の planner 経由の user 直命 2026-09-16（要旨: 器を最新に更新する運用は plugin の hook が強制する形で持つ・逐語は別 repo の台帳）。**担い手が 1 度消えた**——本節の初版は管理 tick の軸として書いたが、tick は [ADR-0045](../../design-intent/decisions/ADR-0045-one-role-and-no-seat-autonomy.html) §2 (2)（`s2-07l.479.1`）で機構ごと消え、値の線 `seat.tick_stale_s` も `rules/manifest.toml` から消えた（実測 2026-09-22・grep 0 件）。user 裁定 2026-09-22T08:44Z: 担い手を **終端の周**（[dispatcher.md](./dispatcher.md) §5・便が live で無くなる瞬間と手動の 1 周と印の直後に撃つ列の 1 周）へ移して契約化する。
- 現物（verified・main 4aafb6f）: §5 の口は着地済み（行 e・`s2-07l.336`・b010cd4）で、`crates/scribe2/src/hook/vessel.rs` の `update`（引数 = 置き場・remote・branch）が「汚れの検査 → fetch → ff → install → 記帳」を 1 本で持ち、断りは同 file の閉じた型（語は `vessel-repo-undeclared` / `dirty` / `not-fast-forward` / `fetch-failed` / `install-failed` / `record-failed`）である。既定の remote と branch も同 file の const 2 つ（315 / 317 行）。列の 1 周は `crates/scribe2/src/pipe/dispatch.rs` の `fire`（570 行・**起こす側**）と `turn`（489 行・見る側）に分かれ、`crates/scribe2/src/pipe/cli.rs` が終端（224 行）と手動の 1 周（431 行）の両方で `fire` を撃つ。1 周の結果の 1 行は同 file の `line`（764 行）が組み、`dispatch=started:…,resumed:…,waiting:…` の書式である。doctor の consumer 行は `crates/scribe2/src/account/consumers.rs` の `head_of`（233 行）が読んだ vessel repo の HEAD を `render_consumer`（276 行）と `render_broken`（296 行）が `head=` で出すが、上流との差は持たない（§4 の `drift=` は「記録 vs 今の file」の比較である）。
- 形（番号は done と 1:1）:
  1. **behind を読む 1 本**（`crates/scribe2/src/hook/vessel.rs`・§5 の口と同じ file＝更新の口と読みが 1 か所に閉じる・C2）: host の manifest の `[[vessel]] repo` が無い周は宣言なしの値。在る周は起点を **vessel repo の checkout の HEAD**（doctor の `head=` と同じ読み・§5 の口が ff で進める当の ref）にして、`rev-list --count` で `HEAD` と `<remote>/<branch>` の差を数える（既定は §5 と同じ const 2 つ）。**この 1 本は fetch を撃たない**（読むだけ）。git が返らない・数が読めない周は理由付きの測れない値（`git-failed`）で、0 と融合しない（C10）。binary の build 元 commit（§2 の `env!` の 1 値）は起点にしない（歯の binary の出自で分岐が変わる seam を作らない・旧形の限界は却下案）。値は閉じた enum（宣言なし / 現行 / behind と数 / 更新と sha / 断りと語 / 測れないと理由）で持ち、bool にしない。
  2. **終端の周の軸**（`crates/scribe2/src/pipe/dispatch.rs` の `fire` の中・起こす側だけ）: **`dispatch ls` は撃たない**（§5「起こす口と見る口を分ける」＝見るだけで更新が走らない）。撃つのは **live な便が 0 の周だけ**である——走っている便の下で PATH の binary を入れ替えると、走行中の driver と次に起きる子で世代が割れる（`s2-07l.160` の座礁と同じ面）。live が残る周は behind の数を名乗るだけで口を撃たない（次の終端が拾う）。撃つ周は `git fetch <remote>` を **1 周に 1 回**だけ撃ってから 1 を読み、behind ≥ 1 なら §5 の口を **1 回**呼ぶ（rebuild と install と記帳は §5 のまま）。fetch が落ちた周は測れない値（`fetch-failed`）で口を撃たない。判定の型（`crates/scribe2/src/pipe/dispatch.rs` の `Turn`）に field を 1 つ足し、見る側の 1 周では常に空にする（構築点は同 file の 729 行と `crates/scribe2/src/pipe/dispatch/candidates.rs` の 100 行の 2 か所）。
  3. **1 行の記録**（`crates/scribe2/src/pipe/cli.rs`）: 軸を評価した周だけ、`fire` の結果から `vessel=<current|behind:<n>|updated:<sha12>|refused:<断りの語>|unmeasured:<理由>|undeclared>` の 1 行を outcome に足す（評価していない周は載らない）。**`line`（764 行）が組む `dispatch=` の行は 1 字も変えない**——その行は `--drive` の周にだけ出る約束（§5）で、書式を動かすと段を手で 1 つずつ進める既存の歯が動く。
  4. **doctor の consumer 行**（`crates/scribe2/src/account/consumers.rs`）: `head=` の直後に `behind=<n|unmeasured|->` を足す。値は 1 と**同じ 1 本**で読む（doctor は fetch を撃たない・§4「判定しない」）。`[[vessel]]` が無い行は `-`。`render_consumer` と `render_broken` の 2 形とも。行の外形は行 g の write-set に在る doctor の外形 snapshot が受ける。
  5. **触らない**: §5 の口の順序と断りの語・`drift=` の語・`dispatch=` の行の書式と `drive=` の token・列の候補の組み方と起こし直しの規則（dispatcher.md §5）・`dispatch ls` の面（§6）・rules 行（値も行数も増やさない）・導入先の帳簿と cache（§5 と同じく書かない）・別 host の席。
- 自動実行の判定（planner 裁定 2026-09-16・本節の改訂でも不変）: §5 の口は「消す / 出す / 使う」のいずれでもない（local の build と install・push しない・課金しない）ので器が撃ってよい。人間へ上げる形（NEEDS-USER の注入）は採らない（更新の運用を人の手に戻す＝直命の逆）。
- 歯（接頭辞 `pipe_dispatch_vessel_` と `doctor_consumer_behind_`・`crates/` 全体の fn 名の substring として base に 0 件・置き場は `crates/scribe2/tests/e2e/pipe/dispatch.rs`〔既存の `#[test]` 78 本の隣〕と `crates/scribe2/tests/e2e/main.rs`〔既存の `doctor_consumer_` の 4 本の隣〕・偽 `git` と偽 `cargo` は §5 の歯と同じ argv を写す stub を PATH の先頭に置く）: 偽 git が 2 を返し live 便 0 の周に口が 1 回撃たれて `updated:` と記帳 1 件が残る／0 の周は `current` で撃たない／live 便が在る周は `behind:<n>` のまま口の argv が 0 本（次の終端が拾う）／fetch が落ちた周は `unmeasured:fetch-failed` で撃たない／`[[vessel]]` 無しは `undeclared` で git の argv 0／汚れた repo は断りの語で記帳 0 件／見る側の 1 周（`dispatch ls`）では git も cargo も 1 本も撃たれず行に token が載らない／doctor の consumer 行が `behind=<n>` を持ち fetch の argv が写らない・`[[vessel]]` 無しは `-`。既存の `pipe_dispatch_` の歯と `doctor_consumer_` の 4 本は緑のままで、`dispatch=` の行を読む歯は 1 本も動かない。
- 却下案: 管理 tick の軸として持つ（tick が機構ごと無い・値の線も消えた）／時計の契機を足して撃つ（dispatcher.md §5「時計で撃つ契機を足さない・足すなら先に消すものを名指す」C17.2）／`dispatch ls` でも撃つ（見るだけで更新が走る・§5 の分離に反する）／live 便が在る周にも撃つ（走行中の便の下で binary を入れ替える）／fetch の間引きに新しい rules 行を足す（値の線と裁定 id が 1 つ増える・終端の周は便 1 本につき 1 度で tick の毎分とは頻度の桁が違う＝間引きの値が要らない）／`dispatch=` の行に token を混ぜる（その行は `--drive` の周にしか出ず、書式を動かすと既存の歯が動く）／behind を席へ 1 行 inject して人に撃たせる（散文の運用に戻る・直命の逆）／binary の build 元 commit を起点に数える（起点が歯の binary の出自で変わり、汚れた build では fetch も数えも呼ばれず歯が空虚になる＝測れない seam。限界として、手で pull だけした checkout は現行と読む＝binary の古さは §4 の食い違いが名指す側）／器の lock file で 2 周の同時実行を塞ぐ（git と cargo が既に直列化する・lock の TTL の設計が増える）。
- 外形の pin: doctor の consumer 行の字面は `crates/scribe2/tests/e2e/seat.rs` の `CONSUMER_REPO`（seat の既存の歯が完全一致で読む定数）が pin するので、形 (4) で欄を足す便は同じ file を write-set に持ち、定数を新しい字面に更新する（他の既存の歯の期待は変えない）。

## 16. consumer が orchestrator 1 役へ移る手順（`s2-07l.480`・運用・code の変更は無い）

- 何が起きているか: `s2-07l.478`（役割を orchestrator 1 つに畳む）と `s2-07l.479`（席の自律機能の削除）で **rules の語彙と役割の集合が変わった**。器の写しを自分の置き場に持つ consumer（自分の `rules.toml` / `host.toml` / 席の登録 / 管理 tick の unit を持つ repo）は、新しい binary をそのまま使うと**自分の写しの `rules.toml` が loader に断られる**。実測（consumer 1 つ・`rules validate` の断り 11 行・2026-09-19）: 役割を名指す裁定 id の行の value が 1 行・`.479` で消えた席の kind の行が 6 行・`role.*` の 2 行が持つ消えた権能が 2 行・台帳の memo の kind の行が 2 行。置き場に `rules.toml` を持たない consumer（埋め込みの manifest で動く側）はこの段を飛ばす。
- **手順**（この順・各段の後で次へ進む前に測る）:
  1. **写しの `rules.toml` の版上げ**: 断りの 4 群を直す——(a) 役割を名指す裁定 id の行の value を `orchestrator` にする (b) `.479` で消えた席の kind の行を落とす (c) `role.planner` / `role.admin` の 2 行を `role.orchestrator` の 1 行に畳む（消えた権能の語も落ちる） (d) 台帳の memo の kind の行を落とす。**測り方**: `<NAME> rules validate --rules <写し>` が rc 0 になるまで（断りは行番号つきで全件出る）。
  2. **席の登録の付け直し**: 旧 row（`role` が planner / admin のもの）は新しい binary の replay で**無視される**（実測: 旧 binary が `registered=2` と読む置き場を、新しい binary は `registered=0` と読む）。`seat register`（登録だけの口・起動しない）で `--role orchestrator` の row を書き直す。**1 anchor に 1 席**である（役割 × anchor が登録の鍵なので、同じ anchor の 2 席目は前の row を置き換える）。
  3. **管理席の退役**: 役割が 1 つになるので、repo ごとの 2 席目は退役させる（席を畳む手順は [account-lifecycle.md](./account-lifecycle.md) §4 のまま）。
  4. **管理 tick の unit の撤去**: 管理 tick は `.479` で subcommand ごと消えた。consumer が持つ timer / service の unit を止めて外す（残しても新しい binary は未知の subcommand として断るだけだが、失敗が毎周記録に積む）。
  5. **移る時期**: `s2-07l.382`（land の終端）の **Landed 後**。それまで consumer は旧 binary（build 元 commit `99f20d3`）に pin して運ぶ（§2 の build 元 commit の記録がそのまま pin の宣言である）。
- **検証**（consumer 側で 1 行ずつ）: `rules validate` が rc 0 ／ `doctor --state-dir <置き場>` の `seats: registered=<付け直した席の数>` が実測値と一致。
- **触らない**: consumer の帳簿（`installed_plugins.json`）と cache（§5 と同じく器は書かない）・§4 の `drift=` の語・§5 の口の順序と断り・器の repo の code（本段は運用であって実装の便ではない）。
- 却下案: 器が consumer の写しの `rules.toml` を自動で書き換える（他人の置き場を書く・§5「何を書かないか」と同じ線）／旧 role の row を新しい binary が読み替える（消した役割を replay に残す＝`.478` の削除が空洞化する）／移行を dispatcher の着地前に行う（consumer の席が止まっている間に列の設計が動く＝2 つの変化を同時に測ることになる）。

## 17. plugin の実体を生成 dir へ寄せ、marketplace の source をその dir にする（契約表の行 h・[ADR-0038](../../design-intent/decisions/ADR-0038-plugin-payload-lives-in-a-generated-dir.html)・`s2-07l.365`）

- 何が起きているか（別 project の席の照会 2026-09-16・逐語は台帳 `s2-07l.365` の notes）: plugin の install は marketplace の `source` が指す dir を cache の一時 dir へ丸ごと copy する実装で、本 repo の `source` は repo 全体（`.claude-plugin/marketplace.json` の値 `"./"`・`cargo xtask gen-manifest` の生成値）である。そのため git-ignored の worktree 群（実測 174 GB）と build 生成物を含んで copy が 120 秒で終わらず、その host の cache が 91 GB に膨らんだ。plugin の実体は 32 MB である。裁定 = user 2026-09-15（形 (a)「plugin 用の生成 dir を marketplace の source にする」の承認）・決定は ADR-0038（accepted・OPT1 採用）。A1 は非該当（plugin の置き場の移動と写しの向き＝消す / 出す / 使うのどれでもない）。A3 も非該当（依存を足さない）。
- 現物の実測（2026-09-20・本 repo の main・grep と `ls` で数えた）:
  - plugin の実体は（本節の着地の前は）repo root 直下の 2 file（manifest と hook の manifest・本節の後は `plugin/.claude-plugin/plugin.json` と `plugin/hooks/hooks.json`）で、`.claude-plugin/marketplace.json` と合わせた 3 file が `cargo xtask gen-manifest` の生成物である（置き場は `crates/xtask/src/genmanifest.rs` の 3 定数 `MANIFEST_REL` / `HOOKS_REL` / `MARKETPLACE_REL`）。
  - ADR-0038 の CTX2 が数えた 3 つ目の dir（復元と退避の skill 2 枚）は `s2-07l.479` の削除で機構ごと消え、現物に該当の dir は無い（`ls` が「そのような file や dir は無い」で返る）＝本節が動かすのは生成物の 2 file だけで、tracked の実体の純移動は 0 面である。
  - plugin root = checkout root を前提にする現物は 5 つ: (1) `crates/xtask/src/genmanifest.rs` の 3 定数（`MANIFEST_REL` / `HOOKS_REL` / `MARKETPLACE_REL`）と同じ `crates/xtask/src/genmanifest.rs` の `render_marketplace` が出す `source` の値 (2) `crates/scribe2/src/seat/cycle/launch.rs` の `derive_launch` が組む起動行の 1 つ目の `--plugin-dir` が anchor そのもの (3) `crates/scribe2/src/pipe/spawn.rs` の `EMBEDDED_PLUGIN_JSON` / `EMBEDDED_HOOKS_JSON` の `include_str!` の path と、consumer の plugin を写すときに読む dir の列 `PLUGIN_DIRS`（2 要素） (4) `crates/scribe2/src/main.rs` の in-file の歯 `name_is_single_source` が workspace root から plugin.json を読む (5) `crates/scribe2/src/account/consumers.rs` が §4 の母集団で席の登録 row の anchor 直下から hooks.json を読む。
  - **xtask の 2 面は「そのまま追随」しない（2026-09-20 に現物で測り直した・審査 FAIL の根）**: `crates/xtask/src/check_facts.rs` と `crates/xtask/src/check_tests.rs` はどちらも **workspace root と `genmanifest::MANIFEST_REL` を自分で結ぶ**——前者は plugin manifest の突合（manifest-name / manifest-version）で root と結んだ path を読み、後者は check の fixture の tree を組むときに同じ結び方で plugin.json を書く（2 か所・片方は名の食い違いの歯）。形 1 で生成 dir の名は tracked な `crates/scribe2/src/name.rs` からの**実行時の読み**になり定数の字面に埋められない（xtask は core に依存しない）ので、結び直しは呼び手の側でしか書けない＝**この 2 file は触る面**である。触らなければ、突合が root 直下の消えた旧 path を読んで `cargo xtask check` が落ち、fixture は生成 dir の外へ plugin.json を書いて check の歯が丸ごと赤くなる。
  - **この 2 面を足すと受付の余地に当たる（2026-09-20 の事前の検査で実測）**: `crates/xtask/src/check_tests.rs` は R-C4-2 の余地が **271 行**で、行 h の `size` の見積（M）に 29 行足りず受付が `cap-headroom` で断る。他の面は M で通る。行を通すには **`crates/xtask/src/check_tests.rs` を割る便が先に要る**（`size` の字面だけ下げるのは見積を嘘にする＝[contract-source.md](./contract-source.md) §29 の却下案と同じ型）。**その便は着地済み**（[rules-manifest.md](./rules-manifest.md) 行 f・orchestrator の実測 2026-09-22・main 62d064e: `crates/xtask/src/check_tests.rs` の正規化行数は 1033 で余地 **467**、`crates/xtask/src/check_facts.rs` は 906 で余地 594）＝本行は受付の余地に当たらず、先行便への `depends` は要らない。
  - hook の `--plugin-root`（`$CLAUDE_PLUGIN_ROOT`）と §3 の digest の path は root 相対なので不変、install 帳簿の比較（§4）は記録値どうしなので形は不変である。
- 形（ADR-0038 §3 の OPT1 の写し・番号は下の「約束 ↔ done ↔ 歯」と 1:1）:
  1. **置き場の定数は core に 1 つ**: `crates/scribe2/src/name.rs` の `NAME` の隣に、repo root 相対の dir 名を値に持つ定数を 1 つ置く（C2.2・NAME と同じ形）。xtask は core に依存しないので、`crates/xtask/src/workspace.rs` が `NAME` を tracked の `name.rs` から読んでいる既存の 1 本と同じ形でこの定数も読み、読んだ値を既存の配置の型（`Layout`）に 1 つ足す。**生成 dir を含む path を結ぶのは呼び手**で、`genmanifest::MANIFEST_REL` ほかの 3 定数は root 相対の**末尾**の字面のまま変えない（定数に実行時の値を埋められないため・上の「xtask の 2 面は『そのまま追随』しない」の実測）。結び直す呼び手は 3 つ: 生成の書き手（`crates/xtask/src/genmanifest.rs`）・plugin manifest の突合（`crates/xtask/src/check_facts.rs`）・check の fixture の組み手（`crates/xtask/src/check_tests.rs`・2 か所）。定数の無い core を読んだ周は typed に断る（(a) の否定の枝）。
  2. **生成物の置き場**: `plugin.json` と `hooks.json` は生成 dir の配下へ移り（行 h の write-set の `+` の 2 項目）、root 直下の 2 file は同じ便で消える（write-set の `~` の 2 項目）。`marketplace.json` は root の `.claude-plugin/` に残り、`source` の値が生成 dir を指す。生成の対象は 3 file のままで、`cargo xtask check` の drift（render == tracked）は置き場が変わるだけで型は不変である。
  3. **起動行**: (2) の `derive_launch` の 1 つ目の `--plugin-dir` は anchor の下の生成 dir になる（2 つ目以降の `[[plugin]]` の dir と `[[launch-arg]]` の順は不変）。
  4. **便の worktree への写し**: (3) の `include_str!` の path は生成 dir の配下を指し、consumer の plugin として写す dir の列も生成 dir の配下から読む。写し先（`<state_dir>/pipe/<run>/plugin/` の下の 1 dir = 1 plugin）と、`name` が `NAME` と同じ周は写さない判定は不変である。
  5. **読み手**: (4) の in-file の歯と (5) の導入先の読み込み元は同じ定数から解く。`--plugin-root` の記録（§3）と digest の path と install 帳簿の比較（§4）は変えない。
  6. **設計 doc の追随**: root 直下の 2 file の path を散文か行の write-set に持つ設計 doc（本 doc の §12 (b)・[pipeline.md](./pipeline.md) §5 の手順 5・[vessel-hook.md](./vessel-hook.md)・[seat-state.md](./seat-state.md)・[seat-roles.md](./seat-roles.md) の §9 (b) と契約表の 1 行の write-set）を同じ便で生成 dir 配下へ付け替える＝消えた path を名指す行と散文を残さない。
- 着地の形（`s2-07l.365`）: 形 1 の「配置の型に載せる」は `Layout` の field でなく読みの method（`plugin_dir`）である——`Layout` を literal で組む measure の歯が行の write-set の外に 10 本余り在り、field を足すとそれらが書き換わるため。形 5 の読み込み元は §4 の `dual` の判定の checkout 側で、記録の root を `[[vessel]] repo` の下の生成 dir と比べる（旧 root を記録した席は `dual` にならず、旧 root に hooks.json が無いので `plugin` の語が名指す）。形 3 の歯は `crates/scribe2/src/seat/cycle/launch.rs` の in-file（pure な導出行）で、席を立てる e2e の歯は既存の `seat_launch_` の期待値を生成 dir に付け替えた（tmux を立てる歯を足すと nextest の tmux group の宣言が行の write-set の外で動くため）。
- 触らない: 器の作業物の置き場（便の worktree・退役の退避先・台帳・build 生成物）・`drift=` の語の列・§5 の口の順序と断り・hook の `--plugin-root` と digest の path・install 帳簿と cache（器は書かない）・runner / lens が `--plugin-dir` に渡す root 配下の並べ方。
- 再 install は user 手番: 全 host の全口座 × project で入れ直しが要り、古い install は古い root を指し続ける。その食い違いは §4 の doctor の行（`plugin` / `ledger` の語）が名指す＝器は帳簿を書かない。
- 歯（`plugin_payload_` 接頭辞・`tests/e2e/hook.rs` と `tests/e2e/pipe/spawn.rs` と `tests/e2e/seat/launch.rs` と `tests/e2e/main.rs`・xtask 側は `crates/xtask/src/genmanifest.rs` の in-file）と**約束 ↔ done ↔ 歯 ↔ verify の 1:1**:
  - 形 1 → (a) xtask が tracked の `name.rs` から置き場の定数を読んで配置の型に載せ、定数の無い fixture の core を typed に断り（**否定の枝**）、plugin manifest の突合が root 直下の旧 path でなく生成 dir 配下から読む（旧 path にだけ plugin.json を置いた fixture は `manifest-name` の違反として名指される＝**否定の枝**・結び直しを忘れた実装では落ちる）／`-p xtask … plugin_payload_`
  - 形 2 → (b) `gen-manifest` が 3 file を生成し、2 file は生成 dir 配下・`marketplace.json` は root の `.claude-plugin/` にあって `source` が生成 dir を指し、root 直下の旧 path には何も書かれない（**否定の枝**＝旧 path が生成物として再び現れない）／`-p xtask … plugin_payload_`
  - 形 3 → (c) 偽 claude へ届く起動行の 1 つ目の `--plugin-dir` が anchor の下の生成 dir で、2 つ目以降の列と雛形は不変／`-p scribe2 … plugin_payload_`
  - 形 4 → (d) toy repo の便で run dir の写しに器の 2 file が在り、別名の plugin を持つ worktree の周だけ consumer の写しが生成 dir 配下から取られ、旧 path にしか plugin を持たない worktree は consumer とは見ない（**否定の枝**）／`-p scribe2 … plugin_payload_`
  - 形 5 → (e) `name_is_single_source` が生成 dir 配下の plugin.json から `name` を読んで 4 者一致を測り、doctor の consumer 行が生成 dir 配下の hooks.json を読み込み元として digest を出す（記録の無い consumer は従来どおり `unrecorded`＝**否定の枝**）／`-p scribe2 … plugin_payload_` と `-p scribe2 … name_is_single_source`
  - 形 6 → 設計 doc の追随は `cargo xtask check` の path の検査が測る（歯を足さない）
- 却下案: ADR-0038 §3 の OPT2〜OPT5（写しは持たない）。加えて設計固有: 生成 dir の名を xtask 側の定数で持つ（正本が 2 つになる・C2.2 の NAME と同じ形を崩す）／旧 path に symlink を残して両方から読めるようにする（cache の copy が link を辿るかが未確認で、写しの単位が閉じない）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "e"
title = "vessel update — ff → build → install を 1 つの口で行い InstallRecorded を event log に 1 件記す（§12 (b) からの切り出し）"
req = ["FR61"]
section = "5"
write-set = ["crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/fleet/event.rs", "crates/scribe2/src/fleet/replay.rs", "crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/account/mod.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2/src/pipe/queue.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/prop.rs", "crates/scribe2/tests/e2e/pipe/ratelimit.rs", "crates/scribe2/tests/e2e/pipe/stop.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/snapshots/e2e__hook__vessel_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap", ".config/nextest.toml"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail vessel_update_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail fleet_kinds_follow_declaration_order", "cargo nextest run -p scribe2 --test e2e --no-tests=fail vessel_external_form", "cargo nextest run -p scribe2 --test e2e --no-tests=fail fleet_external_form"]
size = "M"
done = "偽 git と偽 cargo で status → fetch → merge --ff-only → cargo install の順序の argv が写って InstallRecorded が 1 件（sha12 と host と path）記され stdout が 1 行出て、宣言なし / dirty / not-fast-forward / install の失敗の 4 形はその段より後の argv を 1 本も撃たず event 0 で典型の語を出し、fleet record はこの kind を手で渡されると拒み、KINDS の宣言順の pin と vessel の外形 snapshot と fleet の外形 snapshot が更新されて緑である"

[[contract]]
id = "f"
title = "doctor の consumer 行に statusline=<vessel|other|absent|unreadable> を足す — 口座行と同じ 1 関数で読む"
req = ["FR61", "FR63"]
section = "14"
write-set = ["crates/scribe2/src/account/consumers.rs", "+crates/scribe2/src/seat/statusline.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail doctor_consumer_statusline_"]
size = "S"
done = "doctor の consumer 行が statusline= を 4 値で出し、口座行と同じ関数を通る"

[[contract]]
id = "g"
title = "vessel repo の checkout が上流の既定 branch から behind の周に、終端の周（起こす側・live 便 0 の周）が vessel update を 1 回撃つ — behind を読む 1 本と閉じた値の列、1 行の記録、doctor の consumer 行の behind="
req = ["FR61"]
section = "15"
touches = ["crate::pipe::dispatch::Turn"]
write-set = ["crates/scribe2/src/hook/vessel.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/account/consumers.rs", "crates/scribe2/tests/e2e/pipe/dispatch.rs", "crates/scribe2/tests/e2e/main.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "docs/design/consumer-sync.md"]
verify = ["cargo nextest run -p scribe2 --test e2e --no-tests=fail pipe_dispatch_vessel_", "cargo nextest run -p scribe2 --test e2e --no-tests=fail doctor_consumer_behind_"]
size = "M"
done = "(1) behind を読む 1 本が hook の vessel の面に在り、起点を checkout の HEAD にして rev-list --count で上流との差を数え、fetch を撃たず、宣言なし / 現行 / behind と数 / 更新と sha / 断りと語 / 測れないと理由の閉じた値を返す (2) 終端の周の起こす側だけが軸を撃ち、live な便が 0 の周に fetch を 1 回撃ってから読み、behind が 1 以上なら §5 の口を 1 回呼び、live が残る周と測れない周と宣言の無い周は口の argv が 0 本で、見る側の 1 周では git も cargo も 1 本も撃たれない (3) 軸を評価した周だけ vessel= の 1 行が出力に足され、dispatch= の行の書式と drive= の token は 1 字も変わらない (4) doctor の consumer 行が head= の直後に behind= を持ち、値は (1) と同じ 1 本で読まれ、fetch の argv が写らず、[[vessel]] の無い行は - になり、doctor の外形 snapshot が consumer 行の 1 語だけ動く (5) §5 の口の順序と断りの語・drift= の語・rules 行の値と行数・列の候補の組み方と起こし直しの規則は 1 字も変わらない"
depends = ["e"]

[[contract]]
id = "h"
title = "plugin の実体を生成 dir へ寄せ marketplace の source をその dir にする — 置き場は core の定数 1 つ・install の写しの単位を plugin に閉じる（ADR-0038 OPT1）"
req = ["FR61", "FR59"]
section = "17"
write-set = ["crates/scribe2/src/name.rs", "crates/xtask/src/workspace.rs", "crates/xtask/src/genmanifest.rs", "crates/xtask/src/check_facts.rs", "crates/xtask/src/check_tests.rs", "~.claude-plugin/plugin.json", "~hooks/hooks.json", "+plugin/.claude-plugin/plugin.json", "+plugin/hooks/hooks.json", ".claude-plugin/marketplace.json", "crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/account/consumers.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/pipe/spawn.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/seat/launch.rs", "crates/scribe2/tests/e2e/main.rs", "docs/design/consumer-sync.md", "docs/design/pipeline.md", "docs/design/vessel-hook.md", "docs/design/seat-state.md", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p xtask --no-tests=fail plugin_payload_", "cargo nextest run -p scribe2 --no-tests=fail plugin_payload_", "cargo nextest run -p scribe2 --no-tests=fail name_is_single_source"]
size = "M"
done = "gen-manifest の 3 file のうち plugin.json と hooks.json が生成 dir の配下に在って root 直下の旧 path は消え、marketplace の source が生成 dir を指し、置き場は core の定数 1 つが正本で xtask は tracked の core からそれを読んで配置の型に載せ（定数の無い core は typed に断る）、生成 dir と結ぶ 3 つの呼び手（生成の書き手・plugin manifest の突合・check の fixture の組み手）が結び直されて旧 path にだけ plugin.json を置いた fixture が manifest-name の違反として名指され、席の起動行の 1 つ目の --plugin-dir と便の worktree への写しと binary の歯と導入先の読み込み元が同じ定数から解け、席の起動行の 2 つ目以降の列と起動行の雛形と、consumer の plugin の判定と写し先と hook の --plugin-root と digest の path と install 帳簿の比較は不変で、消えた path を名指す設計 doc の散文と行の write-set が残らない"
<!-- contracts:end -->
