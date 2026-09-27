# carry-prep — 次の世代の器へ履歴つきで持ち込む前の整理（消えた機構の残り・名への依存・境界・e2e の大きさ）

## 1. 何を解くか（裁定と現物）

やさしく言うと: scribe2 の code は次の世代の器（v3）へ履歴つきで持ち込まれ、書き直されない。持ち込む前に、もう使われない code・今は断られる案内・器の名を迂回する字面を片付けておくと、v3 は「名の定数を 1 つ替える」だけで済み、読み手は消えた機構の説明に迷わない。

- 出所: 持ち主の裁定 2026-09-27（リファクタリングの目的は v3 へ持ち込む前の整理・v2 の中だけの磨きは採らない。UTC の分と逐語は台帳 `s2-07l.669` の notes）。
- 棚卸し（census・main bed08a9・数えは git grep と wc・台帳 `s2-07l.669` の notes に要旨）:
  - 要件の突合: SRS v0.26 の FR 79 本のうち生きている 66 本は 65 本が着地・未着地は FR69 の 1 本（seat-roles.md §18 の据え置き）。本設計は FR69 を扱わない。
  - 消えた機構の残り（ADR-0045 §2 (2)・廃止の FR23 / FR25 / FR28 / FR63 / FR66 / FR70）: `crates/scribe2/src/seat/mod.rs` に作業記憶の数え（`scan_wm` と `WmScan` の塊）と statusline の探索（`search_region` の塊）、`crates/scribe2/src/seat/cycle.rs` に参照 0 の定数 9 つ（`DEFAULT_RESTORE` と `REASON_` の 8 つ）、`crates/scribe2/src/seat/role.rs` の `relabel`、`crates/scribe2/src/headless/mod.rs` の `INCONCLUSIVE_HEAD`。core の pub item 1,833 個のうち定義の外から参照 0 がこの 13 個、それらからだけ参照される item が 9 個。
  - 行き先の無い doc link が `crates/scribe2/src/seat/mod.rs` に 6 本（消えた `meter` / `externalize` / `consume` を指す）。
  - 今は断られる案内: `crates/scribe2/src/help.rs` の seat の頁の examples が `--role planner`（役割は orchestrator 1 つだけ・`crates/scribe2/src/seat/role.rs`）。
  - 消えた役割の名が lens の prompt に残る: `crates/scribe2/src/headless/lens.txt` の裁定の節の見出しと本文が「planner の回答」（外形の snapshot と e2e の定数が同じ字面を持つ）。
  - 名を迂回する字面: build 元 commit の compile 時の env の名 `SCRIBE2_BUILD_COMMIT` が core と境界 crate と e2e の `env!` 13 site に literal で在る（憲法 C2.2 は env の接頭辞を NAME から導くと定める・`crates/scribe2/build.rs` の doc が literal を限界として認めている）。

## 2. 形（契約表の行 a〜c・1 つずつ歯が測る・done と 1:1）

1. **行 a — 消えた機構の残りを消し、seat の案内を今の形に直す**: 消す item は次の 4 塊で全部である（どれも定義の外の code から参照 0・下の census）。
   - `crates/scribe2/src/seat/mod.rs` の作業記憶の数え: `WM_PREFIX`・`WM_SUFFIX`・`WM_CONSUMED`・`FRONTMATTER`・`FRONTMATTER_CAP`・`SEAT_KEY`・`WmScan`・`scan_wm`・`is_unconsumed_name`・`seat_of`（この file の `pub fn seat_of`。`crates/scribe2/src/hook/mod.rs` の同名の private fn は別物で残す）。
   - 同じ file の statusline の探索: `search_region`・`tail_nonempty`・`TAIL_LINES`。
   - `crates/scribe2/src/seat/cycle.rs` の定数 9 つ: `DEFAULT_RESTORE`・`REASON_LOCK_HELD`・`REASON_WM_MISSING`・`REASON_WM_UNREADABLE`・`REASON_STATE_MISSING`・`REASON_STATE_UNREADABLE`・`REASON_STATE_STALE`・`REASON_STAMP`・`REASON_CLEAR`。
   - `crates/scribe2/src/seat/role.rs` の `relabel` と `crates/scribe2/src/headless/mod.rs` の `INCONCLUSIVE_HEAD`。
   - census（verified・main 2d1a996・`git grep -w` を crates の全 file に comment 込みで撃った）: 上の名の出現は定義の塊の中と、次の doc の行にしか無い＝`crates/scribe2/src/seat/cycle/relaunch.rs` の立て直しの doc の `DEFAULT_RESTORE` への link 1 本・`crates/scribe2/src/seat/role.rs` の `register` の doc の `relabel` への link 1 本。`crates/scribe2/src/seat/cycle/launch.rs` ほか write-set の外の file と外形 snapshot には 0 件。行き先の無い doc link（消えた `meter`・`externalize`・`consume` を指す）は `crates/scribe2/src/seat/mod.rs` の module の doc・塊の doc・`REASON_NO_RULE` の説明の中の 6 本だけ。
   - module の doc（`crates/scribe2/src/seat/mod.rs` と `crates/scribe2/src/seat/cycle.rs` の頭）は今在る子 module と口だけを書き、`/clear` の作り直し・context の計測・statusline の説明を持たない。`register` と立て直しの doc は消した item を名指さない。
   - help の seat の頁の examples は orchestrator の役割と `s2:orchestrator` の target で書く。
   - 残すもの: 起動・立て直し・停止が共有する `REASON_` の残り・`WHO_LAUNCH`・`HOLE`・rules 行 `seat.cycle_*`。
2. **行 b — lens の prompt から消えた役割の名を外す**: 裁定の節の見出しを「便の質問への回答」の語に、本文の「回答で planner が認めた形」を「回答で認めた形」に替える。節の位置・中身（裁定の対の逐語）・無いときの「（裁定なし）」は不変。doc comment の同じ語も揃える。
3. **行 c — build 元 commit を名の無い形で焼く**: build script は値を compile 時の env ではなく build の出力 dir の 1 file に書き、core が `include_str!` で 1 つの pub const に読む。core・境界 crate・e2e の全 site はその const を読み、env の名の literal は repo から消える。値の形（`<sha12>` / `<sha12>+dirty` / `unknown`）と測り方（HEAD と作業木の汚れ・再走の母集団）は不変。境界 crate は build script を持たなくなる（値は core の const を読む）。
   - census（verified・main 4f60266・`git grep -w SCRIBE2_BUILD_COMMIT` を crates の全 file に comment 込みで撃った・17 行）: `env!` で読む site は 13 で、core の src に 3（`crates/scribe2/src/account/consumers.rs` 1・`crates/scribe2/src/hook/mod.rs` 1・`crates/scribe2/src/pipe/land/finish.rs` 1）、境界 crate の src に 4（`crates/scribe2-boundary/src/main.rs` の version の行 1 と歯の置換 3）、e2e に 6（`crates/scribe2-boundary/tests/e2e/main.rs` 4・`crates/scribe2-boundary/tests/e2e/seat.rs` 1・`crates/scribe2-boundary/tests/e2e/hook.rs` 1）。残る 4 行は `crates/scribe2/build.rs` の名の定数 `ENV_NAME` と module の doc の 1 行、`crates/scribe2-boundary/src/main.rs` と `crates/scribe2-boundary/tests/e2e/seat.rs` の doc の各 1 行。字面を持つ file はこの 8 つで、const の置き場 `crates/scribe2/src/name.rs` と build script の宣言の `crates/scribe2-boundary/Cargo.toml` を足した 10 file が行 c の write-set の 10 項目（docs を除く）と一致する。字面は write-set の外に 0 件。
   - 境界 crate の build script の宣言の今の形: `crates/scribe2-boundary/Cargo.toml` の `[package]` の `build = "../scribe2/build.rs"` が core の build script を共有で指す（境界 crate の下に自前の build script は無い）。同じ file の頭の comment もこの共有を説明する。行 c はこの `build =` の行と comment の共有の説明を消し、境界 crate の下に自前の build script も作らない。e2e（境界 crate の integration test）は依存の core の `pub const` を読む。
   - 再走の母集団が 1 本の build script で不変な理由: 列挙の `crates/scribe2/build/rerun.rs` は repo root（`rev-parse --show-toplevel`）で `git ls-files` を撃つので、core の build script だけで境界 crate の tracked file も母集団に入る。汚れの測り方（`git status --porcelain --untracked-files=no`）も作業木の全体を見る。build script が再走すると core が作り直され、依存する境界 crate も作り直される。

## 3. 触らない

- on-disk の形と鍵（state dir の名・git config の鍵・marker・commit の trailer・systemd の unit 名）: 跨版の契約で、v3 がどれを旧値で読むかを決める。
- 器の名と CLI の名の定数の分離: 憲法 C2.2 の解釈に触れる。v3 の判断の記録（CLI の名は器の名と別の定数）が根拠を持つので v3 の側で行う。
- 実測の口座の記録と SRS FR71 の食い違い: 行 a〜c では触らない。第 2 段の census で読み手が 0 と分かったので、要件の側へ揃える形を §7（行 d）に書く。
- 上限に近い src の file の分割・契約の型の leaf module 化: v2 の中の磨き、または v3 の面の契約の設計を待つもの（§6）。
- 使い方の 1 行・doctor の行・極性一覧・rules 行・既存の外形 snapshot（行 b が名指す 1 本を除く）。
- 消えた口の不在を測る歯（`seat_inject_subcommand_is_gone_from_the_usage` ほか）: 死んだ code ではない。

## 4. 却下

- 参照 0 の item を 1 便で全部消す（core 全体の走査）: 同じ名の別 item との衝突で偽陰性が残る数え方なので、根拠の ADR と廃止の FR を持つ塊だけを消す。
- 行 c を env の名を NAME から導く形で行う: `env!` は literal しか受けず、build script は core の const を import できない。名の無い file 1 つに置けば導く必要が無い。
- 行 c で env の名だけを器の名に依らない綴りへ替える: C2.2 の「env の接頭辞を NAME から導く」に新しい例外を足す読みになる。
- 墓標の歯を消す: 消えた口が戻らないことを測る歯で、v3 へ持ち込むかは v3 の方針。

## 5. 歯

- 行 a（`crates/scribe2/src/help.rs` の歯の module・`help_table_role_` 接頭辞）: help の全頁の examples に現れる `--role <語>` の語が全部、席の役割の解き手で解ける（base の seat の頁は planner で解けない＝RED）。消した item の不在は lens が diff で確かめる（字面の pin は書かない）。
- 行 b（e2e `crates/scribe2-boundary/tests/e2e/headless.rs` の `lens_rulings_` と外形 snapshot `lens_prompt_external_form`）: 裁定の節の見出しが新しい語で 1 回だけ在り、裁定の対の逐語がその直後に在り、裁定が無い周は「（裁定なし）」が続く（base の見出しは planner の語＝RED）。
- 行 c（`crates/scribe2/src/name.rs` の歯の module・`build_commit_` 接頭辞）: core の const が `<sha12>` / `<sha12>+dirty` / `unknown` のどれかの形である（base に const は無い＝RED）。既存の e2e（version の行・binary の世代の記録・consumer の drift）は同じ const を読んで GREEN のまま。env の名の字面が crates の下に残らないことと境界 crate の `build =` の行の不在は、lens が diff と §2 の census の 13 site で確かめる（`build_commit_` は値の形だけを測り、旧い `env!` の形でも GREEN になりうるので、字面の pin の代わりに census を材料にする）。

## 6. 後続（行は本設計の着地の後に同じ doc へ足す）

- 境界の整理: JSON の道具（`crates/scribe2/src/fleet/json_lite.rs`・`crates/scribe2/src/fleet/json_tree.rs`）を fleet から leaf module へ純移動する・`StateDir` と vessel の marker の置き場を leaf へ出して top-level module の輪を切る・`seat::cycle` を中身どおりの名へ改める。file を移す便は、その path を素の項目で名指す過去の契約表の行を同じ PR で新しい path へ直す（境界 crate の新設の便と同じ扱い）。
- e2e の大きさ: 2500 行を越える e2e の 9 file を族ごとの子 module へ割る。前提は e2e の file 数を literal で pin する歯（`crates/scribe2-boundary/tests/e2e/main.rs` の `e2e_fixture_clock_dated_reset_lines_are_pinned`）を宣言から導く形へ直すこと。
- 契約の型の leaf module 化は v3 の面の契約の設計（serde を持つ crate と憲法 C13.2 の derive の置き場）が決まってから。
- 第 2 段の census（verified・main d2d82a7・2026-09-27・数えと出所は台帳 `s2-07l.669` の notes）で、上の見立てと違う事実が 4 つ出た。どこまでを v2 で行うかは持ち主の裁定を待つ。
  - top-level module の輪: core の 16 module のうち 9 つ（account・fleet・headless・hook・ledger・pipe・polarity・rules・seat）が 1 つの強連結成分を成す。`StateDir` と vessel の marker を leaf へ出しても、辺は 1 本も消えない（動く参照は 27）。輪を切るのに要る最小の切断は、item の参照で 151 以上。
  - JSON の道具の移動: 極性の登録簿（`crates/scribe2/src/polarity.rs` の `NOT_A_GUARD`）が site の path を file の path から導くので、同じ PR で書き換えが要る。その結果、純移動の証明が通らない。path を名指す行は 46 file の 97 行と、契約表の 3 行（host-init e・rules-manifest e・vessel-hook d）。
  - `seat::cycle` の改名: 外からの参照は 12 file の 44 行、契約表では 15 行の 21 項目。`seat::launch` への改名は子の `launch` と重なり、clippy の `module_inception` に当たる。rules 行の id `seat.cycle_*` と記録の字面 `seat-cycle` は残す約束なので、名は揃いきらない。
  - e2e の分割: 2500 行を越える 9 file は計 38,731 行。file の並びを固定するものは 4 つある。main.rs の数の pin・pipe の子を兄弟に限る歯・`.config/nextest.toml` の tmux の群の名の列（53 本）・`crates/scribe2-boundary/tests/e2e/pipe/intake.rs` の親 file の埋め込みで、ほかに snapshot 15 本がある。

## 7. 行 d — 読み手の無い実口座の記録を消す（第 2 段・SRS FR71）

やさしく言うと: 席の session が始まるたびに、器は「実際に使っている口座」を 1 行の file に書いている。しかしその file を読む所はどこにも無く、要件（FR71）は「記録しない」と言っている。書く側を消して、要件どおりにする。

- 出所: 持ち主の裁定 2026-09-27（リファクタリングの目的は、v3 へ持ち込む前の整理で、死んだ code を含む。UTC の分と逐語は台帳 `s2-07l.669`）。§3 で保留していた食い違いは、下の census で読み手が 0 と分かったので、要件の側へ揃える。
- census（verified・main d2d82a7）:
  - 要件: SRS FR71（恒常・必須）は「器は session の開始で実測した口座を記録せず、登録 row の口座との食い違いを判定しない（ADR-0045 §2 (2) で廃止）。席の口座は、席の起動（FR59）が登録 row に書く値だけが持つ。不在は不在を測る歯が確かめる」と書く。
  - 書き手: `crates/scribe2/src/hook/mod.rs` の SessionStart の枝が、名乗り → 打刻 → 読み込み元の記録の後に、実口座の記録の fn を呼ぶ。この fn は `--pane` から target を解けた周だけ、payload の transcript の path から口座の label を導く（導けなければ unknown）。そして `crates/scribe2/src/seat/session_account.rs` の書き手で、`<state_dir>/seat/<target>/account` に 1 行（schema・sid・account・ts）を上書きする。
  - 読み手: 0。記録の型と module の名を `git grep` で crates・xtask・scripts・plugin・rules の全 file に撃つと、当たるのは次のものだけである。
    - 書き手の fn
    - `crates/scribe2/src/seat/mod.rs` の module の宣言と、module の doc の 1 行
    - module 自身の歯 2 本
    - e2e の歯

    記録を読む fn は、歯の外に呼び手を持たない。
  - 経緯: 記録は account-lifecycle.md §16 の行 d で入り、2026-09-22 に着地した。これは FR71 の廃止（SRS v0.18・2026-09-19）の後である。読み手の行 e は着地しなかった。
  - 歯: e2e `crates/scribe2-boundary/tests/e2e/hook.rs` に、記録が在ることを測る歯が 4 本ある（名の接頭辞 seat_account_mismatch_record_）。4 本とも `.config/nextest.toml` の tmux の群の名の列に在る（`cargo xtask check` の nextest-tmux-group が両向きで照合する）。
- 形:
  1. `crates/scribe2/src/seat/session_account.rs` を消す。`crates/scribe2/src/seat/mod.rs` からは、その module の宣言と、module の doc の記録の行を消す。
  2. `crates/scribe2/src/hook/mod.rs` の SessionStart は実口座の記録の fn を呼ばず、その fn も消す。名乗り・打刻・読み込み元の記録の順と中身は不変。
  3. e2e の 4 本と、それらだけが使う helper を消し、不在の歯 1 本（下の歯）に替える。`.config/nextest.toml` の tmux の群の名の列から 4 本の名を外し、新しい歯の名を入れる。
  4. この節と行 d の title / done では、消す型と fn の名を backtick で書かない。backtick の中の型の path と fn の形は、着地の後に base で解けなくなり、契約表の名指しの検査が赤くなるからである。path は write-set の `~` の項目なので解ける。
- 触らない:
  - `crates/scribe2/src/seat/cycle.rs` の `ACCOUNTS_DIR`（`crates/scribe2/src/seat/cycle/relaunch.rs` が使う）
  - 登録 row・打刻・読み込み元の記録・席の指示文
  - account-lifecycle.md（行 d の write-set の `+` の項目は、base に無い file として解け続ける）
  - 既存の置き場に残る account の file（読み手が無いので害は無い。消すのは A1 の操作なので、この便ではしない）
- 歯（e2e `crates/scribe2-boundary/tests/e2e/hook.rs`・名 `session_start_leaves_no_account_record`・tmux の群）: pane を解ける周に、置き場の accounts の下の transcript を持つ SessionStart を撃つ。そして 2 つを測る。
  - 同じ席の dir に、読み込み元の記録（file 名 plugin）が在ること。target を解いたことの対照になる。
  - その dir に account の file が無いこと。

  base は label を書くので RED になる。
- 却下:
  - 記録を残して FR71 を改める: 読み手が無い。SRS の改訂は、持ち主が /folio-architect を起こす手番を要するうえ、得る物が無い。
  - 読み手（account-lifecycle §16 の行 e）を作る: ADR-0045 が食い違いの判定ごと廃止した。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "消えた機構の残りを消し seat の案内を今の形に直す — 作業記憶の数え・statusline の探索・seat/cycle.rs の参照 0 の定数 9 つ・relabel・INCONCLUSIVE_HEAD と行き先の無い doc link を消し、help の seat の examples を orchestrator の形に（§2 の 1）"
req = ["FR23", "FR25", "FR28", "FR63", "FR66", "FR70", "FR59"]
section = "2"
write-set = ["crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/cycle.rs", "crates/scribe2/src/seat/cycle/relaunch.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/headless/mod.rs", "crates/scribe2/src/help.rs", "docs/design/carry-prep.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail help_table_role_"]
size = "M"
done = "(1) seat/mod.rs から作業記憶の数え（WM_PREFIX・WM_SUFFIX・WM_CONSUMED・FRONTMATTER・FRONTMATTER_CAP・SEAT_KEY・WmScan・scan_wm・is_unconsumed_name・seat_of）と statusline の探索（search_region・tail_nonempty・TAIL_LINES）が消え、seat/cycle.rs から DEFAULT_RESTORE・REASON_LOCK_HELD・REASON_WM_MISSING・REASON_WM_UNREADABLE・REASON_STATE_MISSING・REASON_STATE_UNREADABLE・REASON_STATE_STALE・REASON_STAMP・REASON_CLEAR が消え、seat::role::relabel と headless::INCONCLUSIVE_HEAD が消える (2) seat/mod.rs と seat/cycle.rs の module の doc は今在る子 module と口だけを書き、seat/mod.rs・seat/cycle.rs・seat/cycle/relaunch.rs・seat/role.rs の doc に消えた item（meter・externalize・consume・DEFAULT_RESTORE・relabel）への link が無い (3) help の seat の頁の examples は --role orchestrator と --target s2:orchestrator の形で、help の全頁の examples の --role の語が全部 seat::role::Role::parse で解けることを歯 help_table_role_ が測る (4) 生きている定数（起動・立て直し・停止が共有する REASON_ の残り・WHO_LAUNCH・HOLE）・rules 行・使い方の 1 行・doctor・極性一覧・外形 snapshot は不変"

[[contract]]
id = "b"
title = "lens の prompt から消えた役割の名を外す — 裁定の節の見出しを「便の質問への回答」の語に、本文の「回答で planner が認めた形」を「回答で認めた形」に替え、doc comment の同じ語を揃える（§2 の 2）"
req = ["FR9", "FR32"]
section = "2"
write-set = ["crates/scribe2/src/headless/lens.txt", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "docs/design/carry-prep.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_rulings_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_prompt_external_form"]
size = "S"
done = "(1) lens の prompt の裁定の節の見出しが「## 契約への裁定（便の質問への回答・逐語）」で 1 回だけ在り、裁定の対の逐語がその直後に在り、裁定が無い周は「（裁定なし）」が続く (2) 審査の材料の節の文が「裁定の節に在る逸脱（回答で認めた形）は契約の一部として読む」になる (3) headless/lens.rs・pipe/gate.rs・pipe/move_proof.rs の doc comment の「planner の回答」が「回答」の語に揃う (4) 節の位置・材料の読み方・外形 snapshot の他の行は不変で、歯 lens_rulings_ と lens_prompt_external_form が新しい語で GREEN"

[[contract]]
id = "c"
title = "build 元 commit を名の無い形で焼く — build script は値を build の出力 dir の 1 file に書き、core の pub const BUILD_COMMIT が include_str! で読み、core・境界 crate・e2e の全 site がその const を読んで env の名の literal を repo から消す（§2 の 3）"
req = ["FR61", "FR11"]
section = "2"
write-set = ["crates/scribe2/build.rs", "crates/scribe2/src/name.rs", "crates/scribe2/src/account/consumers.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/pipe/land/finish.rs", "crates/scribe2-boundary/Cargo.toml", "crates/scribe2-boundary/src/main.rs", "crates/scribe2-boundary/tests/e2e/main.rs", "crates/scribe2-boundary/tests/e2e/seat.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", "docs/design/carry-prep.md"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail build_commit_"]
size = "S"
done = "(1) build script は値（<sha12> / <sha12>+dirty / unknown・測り方と再走の母集団は不変）を cargo の出力 dir の 1 file に書き、compile 時の env を出さない (2) core の name.rs に pub const BUILD_COMMIT が在り include_str! でその file を読み、歯 build_commit_ が値の 3 つの形のどれかであることを測る (3) core の src 3 site（account/consumers.rs・hook/mod.rs・pipe/land/finish.rs）・境界 crate の src/main.rs 4 site・e2e 6 site（main.rs 4・seat.rs 1・hook.rs 1）の計 13 site が BUILD_COMMIT を読み、build.rs の名の定数と doc を含めて SCRIBE2_BUILD_COMMIT の字面が crates の下に 0 件 (4) 境界 crate の Cargo.toml は core の build script を共有で指す build = の行と、その共有を説明する comment を持たず、境界 crate の下に自前の build script も無い (5) version の行・binary の世代の記録・consumer の drift の既存の歯は同じ値で GREEN のまま"

[[contract]]
id = "d"
title = "session の開始で実口座を記録しない — 読み手の無い seat/session_account.rs と SessionStart の実口座の記録を消し、記録を測る 4 本の歯を不在の歯 1 本に替えて SRS FR71 に揃える（§7）"
req = ["FR71"]
section = "7"
write-set = ["~crates/scribe2/src/seat/session_account.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2-boundary/tests/e2e/hook.rs", ".config/nextest.toml", "docs/design/carry-prep.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail session_start_leaves_no_account_record"]
size = "S"
done = "(1) crates/scribe2/src/seat/session_account.rs が無く、seat/mod.rs にその module の宣言と記録を説明する doc の行が無い (2) hook/mod.rs の SessionStart は実口座の記録を書かず、その fn が無い。名乗り・打刻・読み込み元の記録の順と中身は不変 (3) 歯 session_start_leaves_no_account_record が、pane を解ける周に accounts の下の transcript を持つ SessionStart を撃ち、同じ席の dir に読み込み元の記録が在り account の file が無いことを測る（base は label を書く＝RED） (4) e2e hook.rs の seat_account_mismatch_record_ の 4 本とそれらだけが使う helper が消え、.config/nextest.toml の tmux の群の名の列から 4 本の名が外れて新しい歯の名が入り、cargo xtask check が GREEN (5) ACCOUNTS_DIR・登録 row・打刻・読み込み元の記録・席の指示文・account-lifecycle.md・既存の置き場の account の file は不変"
<!-- contracts:end -->
