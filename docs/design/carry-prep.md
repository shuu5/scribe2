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
- 第 2 段の census（verified・main d2d82a7・2026-09-27・数えと出所は台帳 `s2-07l.669` の notes）で、上の見立てと違う事実が 4 つ出た。持ち主の裁定 2026-09-27（UTC の分は台帳 `s2-07l.669`）で範囲を決めた。境界の整理の 3 つ（JSON の道具の移動・輪を切る・`seat::cycle` の改名）は v2 では行わず、v3 へ送る（v3 の引き継ぎ文書に census の数を写した）。e2e の分割だけを v2 で行う（§8）。
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

## 8. 行 e・f — 大きな e2e の file を族ごとの子 module へ割る（第 2 段・準備の 1 本と試しの 1 本）

やさしく言うと: 外から binary を動かして確かめる大きなテスト（e2e）の file が 9 本あり、どれも 2500 行を越えている（最大は 5307 行）。便はテストを 1 か所直すたびにこの大きな file を読み、審査役にも同じ大きさの材料が渡る。そこで、テストの中身を 1 byte も変えずに、族ごとの小さな file へ引っ越す。

行は 3 段で進める。

- 行 e（準備）: 引っ越しの邪魔になる「file の数を数字で固定した歯」を、宣言から数える形に直す。
- 行 f（試し）: 1 file（headless.rs）で割ってみる。
- 残る 8 file の行: 行 f の着地で分かったことを足して書く。`tests/e2e/` 直下の 4 file は §9（行 g〜j）、`pipe/` の下の 4 file は §10（行 k〜o）。

- 出所: 持ち主の裁定 2026-09-27（第 2 段の範囲は e2e の分割だけ・UTC の分は台帳 `s2-07l.669`）と、§6 の census。
- census（verified・main c01a456）:
  - 2500 行を越える e2e は 9 file（行数）:

    | file | 行数 |
    |---|---:|
    | pipe/land.rs | 5307 |
    | hook.rs | 5133 |
    | fleet.rs | 5100 |
    | pipe/gate.rs | 4987 |
    | pipe/dispatch.rs | 4604 |
    | seat.rs | 4111 |
    | headless.rs | 3709 |
    | rules.rs | 2834 |
    | pipe/spawn.rs | 2824 |

    e2e は file の大きさの門の外にある（R-C4-2 は `crates/*/src` だけを数える）。
  - file の並びを固定するものは 4 つある。それぞれ割り方に次のように効く。
    1. `crates/scribe2-boundary/tests/e2e/main.rs` の歯 `e2e_fixture_clock_dated_reset_lines_are_pinned` が、tracked な e2e の `.rs` の本数を literal の 29 で固定する（ほかに、日付の字面を持つ行の数 7 と 5 も固定する）。file を 1 つ足すたびにこの歯の中の行が動くので、分割の便が純移動でなくなる。→ 行 e で、宣言から導く形に直す。
    2. `crates/scribe2-boundary/tests/e2e/pipe.rs` の歯 `pipe_hermetic_sites_stay_one` は、`pipe/` の下の tracked な file の数を、同じ file の列 0 の `mod` 行の数 + 1 と比べる（既に導出の形）。pipe の 4 file の子は `pipe/` の下の兄弟に置き、`pipe.rs` で宣言する（入れ子の dir は作らない）。行 f には当たらない。（この案は §10 で改めた: 行 k でこの比べを外し、pipe の 4 file も入れ子の子へ割る。）
    3. `.config/nextest.toml` の tmux の群の名の列が、module path 付きの名を 53 本持つ（`cargo xtask check` の nextest-tmux-group が両向きで照合する）。tmux を起こす歯を子へ移すときは、同じ PR で名を直す。headless.rs の歯はこの列に 0 本なので、行 f には当たらない。
    4. `crates/scribe2-boundary/tests/e2e/pipe/intake.rs` の `SUBCOMMAND_FILES` が、`seat.rs` と `pipe.rs` の本文を埋め込む。動詞の数を固定する歯は、その親 file に残す。行 f には当たらない。
  - snapshot の名は module path を含み、既定の置き場は source の file の隣である。snapshot を撮る歯を子へ移すと、snapshot の file の名と置き場が変わる。→ snapshot の歯は親に残す。
- 行 e の形:
  1. 歯 `e2e_fixture_clock_dated_reset_lines_are_pinned` は、2 つの集合が等しいことを測る。
     - 左: tracked な e2e の `.rs` の集合
     - 右: `main.rs` と、各 `.rs` の列 0 の `mod <名>;` の行（file の module の宣言）が指す file の集合。宣言の指す file は、`main.rs` の宣言なら `tests/e2e/<名>.rs`、`<dir>/<stem>.rs` の宣言なら `<dir>/<stem>/<名>.rs` である。inline の module（`mod <名> {`）は数えない。

     食い違いは、両向きの差を message に名指す。日付の字面を持つ行の数（7 と 5）の pin は不変で、literal の 29 は消える。
  2. 歯は base でも GREEN である（今の 29 file は全部が宣言されている）。そこで、この便の bead id で新しい `// flip-check: retroactive` の札を歯の区間に足す。そして変異の証明を bead の notes に書く（宣言の無い tracked な e2e の `.rs` を 1 つ足すと RED になる・宣言の `mod` 行を 1 つ消すと RED になる）。歯に既に在る札（`s2-07l.469`）は base から持ち越した札なので、効かない。
- 行 f の形（headless.rs・試しの 1 本）:
  1. `crates/scribe2-boundary/tests/e2e/headless.rs` の歯 108 本のうち、次の族を 2 つの子の file へ移す。子は headless.rs の頭で `mod lens;` と `mod runner;` によって宣言し、子の頭は `use super::*;` にする（先例: `crates/scribe2-boundary/tests/e2e/pipe/gate.rs` ほか pipe の子）。
     - 子 runner.rs（headless.rs と同じ dir の `headless/` の下）: 名が `headless_runner_`・`runner_question_`・`runner_rate_`・`runner_prompt_` で始まる歯。snapshot の歯 `headless_runner_prompt_external_form` は除く。55 本・約 1,420 行。
     - 子 lens.rs（同じ `headless/` の下）: 名が `headless_lens_`・`lens_rulings_` で始まる歯。snapshot の歯 `headless_lens_prompt_external_form`・`headless_lens_contract_prompt_external_form`・`headless_lens_promise_prompt_external_form` は除く。28 本・約 720 行。
  2. 親に残すもの:
     - snapshot の歯 5 本（上の 4 本と `headless_external_form`）
     - ほかの族の歯 20 本（名は `run_cost_`・`headless_claude_`・`headless_plugin_`・`headless_flag_`・`headless_effort_`・`headless_args_`・`headless_agent_`・`pipe_unreachable_`・`e2e_toolbox_` で始まる）
     - helper と const の全部

     子は親の helper を `use super::*;` で読む（可視性は変えない）。
  3. 移す歯の本文（直前の doc と属性の行を含む）は 1 byte も変えない。親と子で増減してよい行は、`crates/scribe2/src/pipe/move_proof.rs` の残差の許容形（空行・`use` の行・`mod <名>;`・`//` の comment・純移動の札）だけである。親で使われなくなった import は、`use` の行だけで直す。
  4. 純移動の札: この便の bead id で `// flip-check: moved` の札を、各子の file の先頭と、親の `mod` の宣言の直後に置く（先例: host-init.md §16 の行 h）。
  5. write-set の headless.rs の項目に付けた `-` の接頭辞は、file を消す宣言ではない。縮む面（file は残り、約 2,130 行が減る）の宣言である。diff は親の M と子 2 つの A の 3 面で、この doc は触らない。doc の行が 1 行でも動くと、move_proof は純移動と判定しない（`.md` は item を持たないので、動いた行が全部残差になる）。すると審査役に diff の全部（約 30 万 byte）が渡り、token の上限を越えて審査が INCONCLUSIVE になる（1 周目の run 072459Z）。
  6. 見積: 親 約 1,580 行・子 runner.rs 約 1,430 行・子 lens.rs 約 730 行。
  7. 移す歯を verify に持つ過去の行が 2 つある。本 doc の行 b（`lens_rulings_`）と、gate-cost.md の行 am（`headless_runner_box_claude_is_one_job_and_the_prompt_names_the_detector`）である。歯の file が子へ移ると、この 2 行は `contracts check` の teeth-outside-write-set に当たる（実 repo の契約表を測る歯 2 本が赤になった・1 周目の run 072459Z）。そこで、本便の前の docs PR で 2 行の write-set に子の path を `+` で足す（行 b に `headless/lens.rs`・行 am に `headless/runner.rs`）。
     - 2 行の bead（`s2-07l.671`・`s2-07l.589`）は close 済みで、受付をもう通らない。`+` を読むのは CI の `contracts check` だけで、子の file の着地の前も後も通る。
     - 素の path は着地の前に解けないので使えない。
- 歯:
  - 行 e: 上の歯 1 本（retroactive）。
  - 行 f: 既存の歯が全部 GREEN のまま、期待を変えない。verify は、headless.rs にだけ在る名の接頭辞 3 語（`headless_runner_`・`runner_rate_`・`lens_rulings_`。`#[test]` の fn 名に substring として含む file は crates の中で headless.rs だけであることを実測）を 1 行ずつ撃つ。e2e の歯の本数が base = head（108 本が module path だけ変わって在る）であることは、move_proof の純移動の判定（消えた item と足された item の本文が同じ）が含む。本数は orchestrator が着地の確認で `cargo nextest list` で数え、bead の notes に写す。実装役の道具は cargo と git だけで、notes を書けない。この doc に書くと上の形 5 に当たる。
- 却下:
  - `include!` で子の本文を親の module へ貼る: module path と snapshot の名は変わらないが、`include!` の行は move_proof の残差の許容形に無い。加えて `pipe/` の下では `pipe_hermetic_sites_stay_one` の数えが合わない。
  - `#[path]` の子 module: module path はどのみち変わるので、得が無い。
  - 分割の便ごとに literal の 29 を上げる: 毎便が `main.rs` の歯の中の行を動かし、純移動の証明が通らなくなる。そうなると、審査役が引っ越しの diff の全部を読むことになる。

## 9. 行 g〜j — 入れ子の子へ割る 4 file（hook.rs・fleet.rs・seat.rs・rules.rs・行 f の形の本番）

やさしく言うと: 行 f（headless.rs）で確かめた引っ越しの形を、残る 8 file のうち `tests/e2e/` 直下の 4 file に当てる。どれも子の file を同じ名の dir の下に置く（入れ子）。`pipe/` の下の 4 file は、子を兄弟に置き親の私有の helper を見せる手当てが要るので、この節の着地の後に別の節で書く。

- 出所: 持ち主の裁定 2026-09-27（第 2 段の範囲は e2e の分割だけ・UTC の分は台帳 `s2-07l.669`）と、§8 の行 f の 2 周（1 周目の run 072459Z の Gated FAIL の 2 因・2 周目の run 074505Z の PASS）。
- 行 f から持ち込む形（どの行も同じ）:
  1. 子は親の file と同じ dir の、親と同じ名の dir の下に置き、親の頭で `mod <子>;` と宣言する。子の頭は純移動の札・`//!` の 1 行・`use super::*;` だけにする。
  2. 移す歯の本文（直前の doc と属性の行を含む）は 1 byte も変えない。親と子で増減してよい行は、move_proof の残差の許容形（空行・`use` の行・`mod <名>;`・`//` の comment・純移動の札）だけである。helper と const は全部親に残す（子は `use super::*;` で読む・可視性は変えない）。
  3. 純移動の札: 行の bead id で `// flip-check: moved` の札を、各子の file の先頭と、親の `mod` の宣言の直後に置く。
  4. 親に残す歯: snapshot の歯（名が module path を含むので動かさない）と、`.config/nextest.toml` の tmux の群の名の列に在る歯（名を直すと `.toml` の行が動き、純移動でなくなる）。
  5. 行の write-set は、親の `-`（縮む面・file は残る）と子の `+` だけにする。この doc は入れない（§8 の形 5）。本数の実測（base = head）は orchestrator が着地の確認で bead の notes に写す（§8 の歯の節）。
  6. 移す歯を verify に持つ過去の行には、この節と同じ docs PR で子の path を `+` で足す（§8 の形 7）。当たった行は 33 本で、どれも bead が close 済みである（下の表）。
- census（verified・main 754b958・歯の数え = `#[test]` の fn・行数は直前の doc と属性を含む）:

  | 行 | 親 | 行数（前 → 後） | 子（歯の本数・約の行数） | 親に残す歯 |
  |---|---|---|---|---|
  | g | hook.rs | 5134 → 約 2855 | guards（46・826）・session（28・776）・group（18・356）・vessel_cli（10・225） | 46 本（hook_role_ の族 22 本は 10 本が tmux の群に在るので族ごと残す・snapshot 2 本・tmux の群の他の 14 本・他 8 本） |
  | h | fleet.rs | 5101 → 約 3080 | usage（35・1022）・account（17・417）・json（27・508） | 65 本（fleet_select_ の族 19 本と host_group_ の族 8 本は本文に `super::` を持つ＝入れ子にすると指す先が変わるので残す・snapshot 1 本） |
  | i | seat.rs | 4112 → 約 3039 | tick（65・1010） | 51 本（snapshot 4 本・tmux の群 2 本・動詞の数を固定する歯 `seat_command_all_known_verbs_round_trip_and_unknown_tokens_are_none`〔§8 の census の 4〕） |
  | j | rules.rs | 2835 → 約 1243 | embedded（45・935）・host（27・589） | 44 本（snapshot 1 本） |

  - 子の族（名の接頭辞）:
    - 行 g: guards = `host_guard_`・`hook_guard_`・`hook_command_`・`hook_memo_`・`hook_ledger_`／session = `hook_session_`・`hook_recovery_`・`hook_precompact_`／group = `hook_group_`・`hook_permission_`／vessel_cli = `vessel_init_`・`vessel_update_`・`vessel_check_`・`vessel_args_`・`vessel_marker_`。
    - 行 h: usage = `fleet_usage_`・`fleet_allowance_`／account = `account_cmd_`／json = `fleet_json_`・`fleet_read_`・`fleet_record_`・`fleet_replay_`・`fleet_export_`。
    - 行 i: tick = `seat_tick_`。
    - 行 j: embedded = `rules_embedded_`・`rules_manifest_`／host = `rules_host_`・`host_group_`。
  - 子の名の制約: hook.rs は、use の行で読み込んだ guard の名と、core の crate の名 vessel を既に持つ。子を `guard` / `vessel` と名付けると名が衝突して compile が通らない。そこで guards / vessel_cli と名付ける。他の 3 file の子の名は、親の `use` の名と重ならない（実測）。
  - 移す歯の本文に `super::` は 0 site、macro_rules! は 4 file とも 0 個（子の深さで意味が変わる字面は無い）。
- 過去の行への `+` の先宣言（33 行・子の path ごとの行数は seat/tick 17・rules/embedded 10・rules/host 6・hook/group 5・hook/guards 4・fleet の 3 子が各 1）:

  | doc | 行 |
  |---|---|
  | account-lifecycle.md | h・i・j・l・m・q・r・s・t・u・w |
  | seat-heartbeat.md | a・c・d・f・h・i・j・k・m・o・p・q・r |
  | vessel-hook.md | b・c・d・e・f |
  | core-boundary.md | i |
  | gate-cost.md | aj |
  | host-init.md | g |
  | seat-roles.md | v |

- 事前の実測（使い捨ての detached worktree で 4 file を機械的に割った木・verified）:
  - `cargo clippy -p scribe2-boundary --all-targets -- -D warnings` が rc 0（親の `use` の行は 1 行も変えずに通る）。
  - `cargo nextest list` の e2e が base = 割った木 = 1556 本で、名の末尾の多重集合が一致。行 e の歯 `e2e_fixture_clock_dated_reset_lines_are_pinned` が GREEN（入れ子の宣言から導いた集合が一致）。
  - `cargo xtask check` が ok（nextest-tmux-group=ok）。
  - `scribe2 contracts check` は、先宣言の無い doc で findings=33（= 上の 33 行）、先宣言の有る doc で findings=0。
  - `cargo xtask flip-check --base 754b958` は hook.rs 単独・fleet.rs 単独の木で rc 0（moved=5・4）。4 file を 1 つの木にまとめると札が 29 で上限 16 を越える。札が多いのは、移す歯の本文が既存の `retroactive` の札の行を運ぶからである（hook.rs 7 行・fleet.rs 8 行）。行ごとの札は g 12・h 12・i 2・j 3 で、どれも 16 以下になる。1 行 = 1 PR を守れば上限に当たらない。
- 歯（どの行も既存の歯が全部 GREEN のまま・期待を変えない）: verify は、行の親にだけ在る名の接頭辞を子ごとに 1 語撃つ（`#[test]` の fn 名に substring として含む file が crates の中で行の親だけであることを実測）。
  - 行 g: `hook_guard_`（guards）・`hook_precompact_`（session）・`hook_permission_`（group）・`vessel_init_`（vessel_cli）。
  - 行 h: `fleet_allowance_`（usage）・`account_cmd_add_`（account）・`fleet_replay_`（json）。
  - 行 i: `seat_tick_judge_`・`seat_tick_grace_`（tick）。
  - 行 j: `rules_embedded_manifest_`（embedded）・`host_group_tier_`（host）。
- 行どうしの関係: 4 行の write-set は交わらない（親も子も別の file）。どの順に着地してもよく、depends は持たない。
- 却下:
  - 4 file を 1 行にまとめる: 札が 29 で flip-check の上限 16 を越える。審査の単位も大きくなる。
  - hook_role_ の族の tmux でない 12 本だけを子へ移す: 1 つの族が 2 file に割れ、`.config/nextest.toml` の名は動かさずに済むが、族を探す場所が 2 つになる。
  - fleet.rs の `super::` を持つ族を子へ移す: 入れ子の子では `super::` の指す先が変わり、本文を書き換えることになる（純移動でなくなる）。
  - seat.rs の isolated seat の fixture（非 test の約 2,000 行）を子へ移す: fixture は歯の族ではなく、他の module（hook.rs・seat の子）が seat の module の path で直に読む。移すには再 export の `use` と可視性の見直しが要り、「helper は親に残す」形（上の 2）から外れる。

## 10. 行 k〜o — pipe/ の下の 4 file（land.rs・gate.rs・dispatch.rs・spawn.rs）も入れ子の子へ割る（準備の 1 本と本番の 4 本）

やさしく言うと: §9 と同じ引っ越しを、`pipe/` の下の大きな 4 file に当てる。邪魔をしているのは、`pipe/` の下の file の数を宣言の数と比べる歯だけである。行 k でその比べを外してから（同じことは行 e の歯が e2e の全体で測っている）、4 file を §9 と同じ入れ子の形で割る。

- 出所: 持ち主の裁定 2026-09-27（第 2 段の範囲は e2e の分割だけ・UTC の分は台帳 `s2-07l.669`）と、§9 の着地（行 g〜j・4 本とも 1 周で PASS）。
- 行 k の形（準備・歯 1 本の手直し）:
  1. 歯 `pipe_hermetic_sites_stay_one`（`crates/scribe2-boundary/tests/e2e/pipe.rs`）は今 2 つを測る。(a) pipe.rs と `pipe/` の下の tracked な file の数が、pipe.rs の列 0 の `mod` 宣言の数 + 1 に等しい。(b) それらの file で binary を起こす字面 2 形の出現の合計が 1 である。(a) は入れ子の子（`pipe/<親>/<子>.rs`）を数えるのに、pipe.rs の宣言しか数えない。そのため、入れ子にすると必ず赤になる（使い捨ての木で実測: file 24 本・宣言 11 本）。
  2. (a) を歯から外し、(b) だけを残す。母集団（読んだ file の数・site の数・base の 41）は message に出し続ける。
  3. (a) が守っていた「tracked な file がどれも宣言されている」ことは、行 e の歯 `e2e_fixture_clock_dated_reset_lines_are_pinned`（`crates/scribe2-boundary/tests/e2e/main.rs`）が測る。この歯は e2e の全体を、入れ子の宣言まで辿って測る（§9 の入れ子の 4 行はこの歯が GREEN のまま着地した）。
  4. 歯の doc comment から (a) の説明を外し、宣言との一致は行 e の歯が測ると書く。
  5. 歯は base でも GREEN なので、この便の bead id で新しい `// flip-check: retroactive` の札を歯の区間に置く。base から持ち越した札（`s2-07l.547`）は効かないので、その行と置き換える。変異の証明は、着地の確認で orchestrator が bead の notes に写す（site を 1 つ足すと RED・宣言の無い入れ子の file を足すと行 e の歯が RED）。
- 行 l〜o の形: §9 の形 1〜6 と同じ（入れ子の子・`use super::*;` だけの頭・helper は親に残す・札・write-set は親の `-` と子の `+` だけ・過去の行へ `+` の先宣言）。加えて次の 2 つを守る。
  1. 本文に `super::` を持つ歯: 入れ子の子の `super::` は親の module を指す。親の file が頭で `use super::*;` を持つなら、`super::<名>` は親の glob を通って base と同じ item に解ける。ただし、親の file が同じ名の item を自前で持たないことが条件である。
     - land.rs（子 follow の 5 site・`super::gate::`）と gate.rs（子 detection の 2 site・`super::land`）は、親が `use super::*;` を持ち、同じ名の item を持たない（実測）。よって子へ移す。gate.rs の子 pure_move の 3 site は、fixture の文字列と doc comment の中の字面で、path ではない。
     - dispatch.rs は頭で `use super::{…}` の名指しの import しか持たない。そのため、`super::` を持つ歯 6 本は子へ移すと解けない（使い捨ての木で compile error を実測）。この 6 本は親に残す。
  2. 子の名: land.rs は use の行で core の land の名を持つので、子の名に land を使わない。他の子の名は、親の use の名と重ならない（compile で実測）。
- census（verified・main 8af5903・歯の数え = `#[test]` の fn・行数は直前の doc と属性を含む）:

  | 行 | 親 | 行数（前 → 後） | 子（歯の本数・約の行数） | 親に残す歯 |
  |---|---|---|---|---|
  | l | pipe/land.rs | 5307 → 約 2845 | follow（27・765）・retire（20・617）・order（20・533）・rebase（16・470） | 53 本（pipe_land_ の族の残り 44 本ほか） |
  | m | pipe/gate.rs | 4987 → 約 3604 | confine（23・642）・detection（15・328）・pure_move（25・355） | 65 本（snapshot 2 本・pipe_gate_ の族の残り 46 本ほか） |
  | n | pipe/dispatch.rs | 4604 → 約 2710 | group（60・884）・waiting（35・628）・terminal（7・285） | 44 本（`super::` を持つ歯 6 本・他 38 本） |
  | o | pipe/spawn.rs | 2824 → 約 1877 | question（18・504）・approval（14・415） | 37 本（pipe_spawn_ の族 25 本ほか） |

  - 子の族（名の接頭辞）:
    - 行 l: follow = `pipe_follow_`／retire = `pipe_retire_`・`pipe_train_`／order = `pipe_terminal_`・`pipe_order_`／rebase = `pipe_land_rebase_`・`pipe_land_onto_`。
    - 行 m: confine = `pipe_confine_`・`pipe_slots_`／detection = `pipe_detection_`・`pipe_landed_`／pure_move = `pipe_gate_move_`・`pipe_gate_elide_`（snapshot の歯 `pipe_gate_move_summary_external_form` は親に残す）。
    - 行 n: group = `pipe_dispatch_group_`／waiting = `pipe_dispatch_waiting_`・`pipe_dispatch_release_`・`pipe_dispatch_gated_`・`pipe_dispatch_regated_`・`pipe_dispatch_revive_`／terminal = `pipe_terminal_`（どの子も `super::` を持つ歯を除く）。
    - 行 o: question = `pipe_question_`・`pipe_resume_`／approval = `pipe_approval_`・`run_cost_`・`pipe_report_`。
  - 4 file とも、snapshot の歯のほかに tmux の群の歯と macro_rules! は 0 である。
- 過去の行への `+` の先宣言（25 行・どれも bead は close 済み・子の path ごとの行数は dispatch/group 18・gate/detection 4・dispatch/waiting 2・spawn/approval 1）:

  | doc | 行 |
  |---|---|
  | account-lifecycle.md | h・i・j・k・m・p・q・r・s・t・u・w |
  | gate-cost.md | aj・ak・am・an・ao |
  | seat-heartbeat.md | g・i・j・q・r |
  | dispatcher.md | t・v |
  | host-init.md | e |

- 事前の実測（使い捨ての detached worktree で 4 file を割り、行 k の手直しを足した木・verified）:
  - `cargo clippy -p scribe2-boundary --all-targets -- -D warnings` が rc 0。
  - `cargo nextest list` の e2e が main = 割った木 = 1556 本で、名の末尾の多重集合が一致。
  - 歯 `pipe_hermetic_sites_stay_one`（手直し後・site 1）と行 e の歯が GREEN。
  - `cargo xtask check` が ok。
  - `scribe2 contracts check` は、先宣言の無い doc で findings=25（= 上の 25 行）、先宣言の有る doc で findings=0。
  - `cargo xtask flip-check`: 行 k の木は main を base に rc 0（retroactive=1）。行 m（札が最も多い file）の木は行 k の木を base に rc 0（moved=4）。行ごとの札は l 5・m 9・n 5・o 3 で、どれも 16 以下である。
- 歯（行 l〜o は既存の歯が全部 GREEN のまま・期待を変えない）:
  - 行 k: 手直しした歯そのもの（retroactive）。
  - 行 l〜o の verify は、行の親にだけ在る名の接頭辞を子ごとに 1 語撃つ（`#[test]` の fn 名に substring として含む file が crates の中で行の親だけであることを実測）。
    - 行 l: `pipe_follow_main_`（follow）・`pipe_retire_`（retire）・`pipe_order_three_runs_`（order）・`pipe_land_rebase_`（rebase）。
    - 行 m: `pipe_slots_`（confine）・`pipe_landed_`（detection）・`pipe_gate_elide_`（pure_move）。
    - 行 n: `pipe_dispatch_group_`（group）・`pipe_dispatch_gated_`（waiting）・`pipe_terminal_dispatch_manual_turn_`（terminal）。
    - 行 o: `pipe_resume_`（question）・`pipe_approval_`（approval）。
- 行どうしの関係: 行 l〜o は行 k に depends を持つ（行 k の前に入れ子にすると、手直し前の歯が赤）。行 l〜o の write-set は互いに交わらず、行 k とも交わらない（行 k は pipe.rs だけを触る）。
- 却下:
  - 子を `pipe/` の下の兄弟に置く（行 k が要らない形）: 兄弟の子は親の私有の helper と親の use の名を見られない。見せるには、helper の頭を `pub(super)` に上げ、子ごとに use の行を足すことになる。純移動の判定は通るが、行ごとに形がばらつく。
  - 歯 (a) を、入れ子の宣言まで辿る形に書き直して残す: 行 e の歯と同じことを 2 本の歯で測ることになる（増殖）。
  - dispatch.rs の `super::` を持つ歯 6 本を子へ移し、本文の `super::` を書き換える: 純移動でなくなる。

## 11. 行 p — 器の木を別の repo の subdir に置いた写しで、xtask の check の 4 門が `n/a(not-a-repo-root)` で黙らずに、根の下の追跡 file を母集団にして測る

やさしく言うと: 器の木を別の repo の下の dir に置いて `cargo xtask check` を撃つと、公開の字面の 4 つの門（paths-clean・private-clean・non-rust-exec・prose-gate）が「repo の根でない」と名乗って何も測らずに rc 0 で通る。置いた先でも器の木の file は追跡されているので、根の下の追跡 file だけを母集団にして測る。根の下に追跡 file が 1 本も無い dir だけは、今どおり測らない（`n/a`）。

- 出所（隣の project の席の予行・2026-10-02T23:5xZ・verified）: 器の main 38cbb667 を外側の repo の subdir に置いた写しで、`cargo xtask check` は rc 0 だが 4 門の値が `n/a(not-a-repo-root)` だった。
- 現物（main 86cc4a25・verified）:
  - 4 門は `crates/xtask/src/paths_clean.rs` の `tracked_files` を共通の母集団の口にする（`crates/xtask/src/private_clean.rs`・`crates/xtask/src/non_rust_exec.rs`・`crates/xtask/src/prose_gate.rs` が同じ口を呼ぶ）。
  - `tracked_files` は `root_is_repo_root`（`git rev-parse --show-toplevel` と根を canonical で比べる）が偽の周に、`git ls-files` を撃たずに `NotRepoRoot` を返す。各門はそれを `n/a(not-a-repo-root)` の値にする。
  - flip-check の base の木は自前の git repo に作られる（`crates/xtask/src/flipcheck/git.rs` の `index_base`）ので、今この枝に当たるのは、別の repo の subdir に置いた木と、追跡 file の無い dir（歯の fixture）だけである。
  - `git -C <dir> ls-files -s -z` は、subdir の下の追跡 file だけを subdir からの相対 path で返す（verified）。
- 形（番号は done と 1:1）:
  1. `tracked_files` は根が toplevel でない周に `git -C <根> ls-files -s -z` を撃ち、1 本以上あれば根からの相対の列を母集団に返す（toplevel の周と同じ読み・形の読めない件が在れば全体を測れないへ倒す）。0 本の周だけ `NotRepoRoot` を返す。paths-clean は、根の下の追跡 file の本文の private path 形を違反と数え、値は走査した件数になる。
  2. private-clean も同じ口を通り、根の下の追跡 file の本文の needle（email の形ほか）を違反と数え、値は `n/a` でなくなる。
  3. 根の下に追跡 file の無い dir は、paths-clean と private-clean とも今どおり `n/a(not-a-repo-root)` で違反 0（既存の歯 2 本）。
- 設計の線（歯を持たない・審査が読む）: non-rust-exec と prose-gate は同じ口を通るので、同じ周から測る。門ごとの読み（needle・免除・閾値）は変えない。根が toplevel の周の読みと値は 1 字も変えない。
- 歯（xtask の lib・既存の `mod tests` の区間に 1 本ずつ）: fixture は一時 dir を `git init` し、その下の dir に file を書いて `git add` し（commit はしない）、下の dir を根にした `Layout` で門の `measure` を撃つ。違反の字は門の needle の定数から組み、字を doc と歯に字のまま書かない。
  - `paths_clean_measures_a_nested_root_by_its_own_tracked_files`（`paths_clean.rs`）: 下の dir の追跡 file 2 本のうち 1 本に private path 形を持たせると、値は `paths-clean=` の後ろが 2 で始まり（`n/a` でない）、違反が 1 件だけ在ってその file を名指す。
  - `private_clean_measures_a_nested_root_by_its_own_tracked_files`（`private_clean.rs`）: 同じ形で email の needle を持たせると、値は `n/a` でなく、違反が 1 件だけ在る。
  - base で RED: base は根が toplevel でない周を `n/a` にするので、2 本とも値の assert で落ちる（機能不在）。
- 触らない: 4 門の needle・免除・値の書式・根が toplevel の周・flip-check・`cargo xtask check` の判定行の並び。
- 限界: 置いた先の repo の CI がこの check を撃つかは、置いた先の repo の側の約束である。根の下に追跡されていない file は、どの周でも母集団に入らない（今と同じ）。
- ADR: 書かない（xtask の母集団の口の直しで、判定・rc の意味・on-disk の形・跨版の約束を変えない）。

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
write-set = ["crates/scribe2/src/headless/lens.txt", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/pipe/gate.rs", "crates/scribe2/src/pipe/move_proof.rs", "crates/scribe2-boundary/tests/e2e/headless.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__lens_prompt_external_form.snap", "docs/design/carry-prep.md", "+crates/scribe2-boundary/tests/e2e/headless/lens.rs"]
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

[[contract]]
id = "e"
title = "e2e の file 数の pin を宣言から導く — 歯 e2e_fixture_clock_dated_reset_lines_are_pinned が tracked な e2e の .rs の集合を main.rs と列 0 の mod 行が指す file の集合と照らし、literal の 29 を消す（§8 行 e）"
req = ["FR33", "FR36"]
section = "8"
write-set = ["crates/scribe2-boundary/tests/e2e/main.rs", "docs/design/carry-prep.md"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail e2e_fixture_clock_dated_reset_lines_are_pinned"]
size = "S"
done = "(1) 歯 e2e_fixture_clock_dated_reset_lines_are_pinned は、tracked な e2e の .rs の集合と、main.rs に各 .rs の列 0 の mod <名>; の行が指す file を足した集合が等しいことを測り、食い違いを両向きの差で名指す。宣言の指す file は、main.rs の宣言なら tests/e2e/<名>.rs、<dir>/<stem>.rs の宣言なら <dir>/<stem>/<名>.rs。inline の mod <名> { は数えない (2) 日付の字面を持つ行の数 7 と 5 の pin は不変で、literal の 29 は歯に無い (3) この便の bead id の flip-check: retroactive の札が歯の区間に在り、変異の証明（宣言の無い tracked な e2e の .rs を足すと RED・mod 行を 1 つ消すと RED）が bead の notes に在る (4) main.rs の他の歯と宣言、e2e の他の file は不変"

[[contract]]
id = "f"
title = "e2e の headless.rs を族ごとの子 module へ割る（試しの 1 本）— runner の族 55 本を子 headless/runner.rs へ、lens の族 28 本を子 headless/lens.rs へ純移動し、snapshot の歯 5 本と他の族の歯と helper は親に残す・札 moved（§8 行 f）"
req = ["NFR1", "FR7"]
section = "8"
write-set = ["-crates/scribe2-boundary/tests/e2e/headless.rs", "+crates/scribe2-boundary/tests/e2e/headless/runner.rs", "+crates/scribe2-boundary/tests/e2e/headless/lens.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_runner_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_rate_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_rulings_"]
size = "M"
done = "(1) 子 headless/runner.rs に、名が headless_runner_・runner_question_・runner_rate_・runner_prompt_ で始まる歯 55 本（snapshot の歯 headless_runner_prompt_external_form を除く）が在り、子 headless/lens.rs に、名が headless_lens_・lens_rulings_ で始まる歯 28 本（snapshot の歯 3 本を除く）が在る。2 つの子は headless.rs の mod lens; と mod runner; で宣言され、頭が use super::*; である (2) 親 headless.rs には snapshot の歯 5 本・他の族の歯 20 本・helper と const の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が、各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head（108 本が module path だけ変わって在る）で、snapshot と .config/nextest.toml と行 e の歯（宣言から導いた集合の一致）は GREEN のまま不変 (6) write-set の headless.rs の - は縮む面（file は残り、約 2,130 行が減る）で、diff は親の M と子 2 つの A の 3 面だけ（この doc を含まない）"
[[contract]]
id = "g"
title = "e2e の hook.rs を族ごとの子 module へ割る — guard の族 46 本を子 hook/guards.rs へ、session の族 28 本を子 hook/session.rs へ、group の族 18 本を子 hook/group.rs へ、vessel の口の族 10 本を子 hook/vessel_cli.rs へ純移動し、snapshot と tmux の群の歯と helper は親に残す・札 moved（§9 行 g）"
req = ["NFR1", "FR7"]
section = "9"
write-set = ["-crates/scribe2-boundary/tests/e2e/hook.rs", "+crates/scribe2-boundary/tests/e2e/hook/guards.rs", "+crates/scribe2-boundary/tests/e2e/hook/session.rs", "+crates/scribe2-boundary/tests/e2e/hook/group.rs", "+crates/scribe2-boundary/tests/e2e/hook/vessel_cli.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_guard_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_precompact_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_permission_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail vessel_init_"]
size = "M"
done = "(1) 子 hook/guards.rs に名が host_guard_・hook_guard_・hook_command_・hook_memo_・hook_ledger_ で始まる歯 46 本、子 hook/session.rs に hook_session_・hook_recovery_・hook_precompact_ で始まる歯 28 本、子 hook/group.rs に hook_group_・hook_permission_ で始まる歯 18 本、子 hook/vessel_cli.rs に vessel_init_・vessel_update_・vessel_check_・vessel_args_・vessel_marker_ で始まる歯 10 本が在る。4 つの子は hook.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 hook.rs には snapshot の歯 2 本・.config/nextest.toml の tmux の群の名の列に在る歯 24 本（hook_role_ の族 22 本は族ごと）・他の歯・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、snapshot と .config/nextest.toml と行 e の歯は GREEN のまま不変 (6) write-set の hook.rs の - は縮む面（file は残り、約 2,280 行が減る）で、diff は親の M と子 4 つの A だけ（この doc を含まない）"

[[contract]]
id = "h"
title = "e2e の fleet.rs を族ごとの子 module へ割る — usage と allowance の族 35 本を子 fleet/usage.rs へ、account_cmd の族 17 本を子 fleet/account.rs へ、json と記録の族 27 本を子 fleet/json.rs へ純移動し、super:: を持つ族と snapshot の歯と helper は親に残す・札 moved（§9 行 h）"
req = ["NFR1", "FR7"]
section = "9"
write-set = ["-crates/scribe2-boundary/tests/e2e/fleet.rs", "+crates/scribe2-boundary/tests/e2e/fleet/usage.rs", "+crates/scribe2-boundary/tests/e2e/fleet/account.rs", "+crates/scribe2-boundary/tests/e2e/fleet/json.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_allowance_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail account_cmd_add_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail fleet_replay_"]
size = "M"
done = "(1) 子 fleet/usage.rs に名が fleet_usage_・fleet_allowance_ で始まる歯 35 本、子 fleet/account.rs に account_cmd_ で始まる歯 17 本、子 fleet/json.rs に fleet_json_・fleet_read_・fleet_record_・fleet_replay_・fleet_export_ で始まる歯 27 本が在る。3 つの子は fleet.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 fleet.rs には fleet_select_ と host_group_ の族（本文に super:: を持つ）・snapshot の歯 1 本・他の歯・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、snapshot と .config/nextest.toml と行 e の歯は GREEN のまま不変 (6) write-set の fleet.rs の - は縮む面（file は残り、約 2,020 行が減る）で、diff は親の M と子 3 つの A だけ（この doc を含まない）"

[[contract]]
id = "i"
title = "e2e の seat.rs を族ごとの子 module へ割る — tick の族 65 本を子 seat/tick.rs へ純移動し、snapshot と tmux の群の歯と動詞の数の歯と fixture は親に残す・札 moved（§9 行 i）"
req = ["NFR1", "FR7"]
section = "9"
write-set = ["-crates/scribe2-boundary/tests/e2e/seat.rs", "+crates/scribe2-boundary/tests/e2e/seat/tick.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_judge_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_grace_"]
size = "S"
done = "(1) 子 seat/tick.rs に名が seat_tick_ で始まる歯 65 本が在り、seat.rs の mod 宣言の並びに mod tick; が足され、子の頭が use super::*; である (2) 親 seat.rs には snapshot の歯 4 本・tmux の群の歯 2 本・動詞の数を固定する歯・isolated seat の fixture・他の歯・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、snapshot と .config/nextest.toml と行 e の歯は GREEN のまま不変 (6) write-set の seat.rs の - は縮む面（file は残り、約 1,070 行が減る）で、diff は親の M と子 1 つの A だけ（この doc を含まない）"

[[contract]]
id = "j"
title = "e2e の rules.rs を族ごとの子 module へ割る — embedded と manifest の族 45 本を子 rules/embedded.rs へ、host と host_group の族 27 本を子 rules/host.rs へ純移動し、snapshot の歯と helper は親に残す・札 moved（§9 行 j）"
req = ["NFR1", "FR7"]
section = "9"
write-set = ["-crates/scribe2-boundary/tests/e2e/rules.rs", "+crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "+crates/scribe2-boundary/tests/e2e/rules/host.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail host_group_tier_"]
size = "S"
done = "(1) 子 rules/embedded.rs に名が rules_embedded_・rules_manifest_ で始まる歯 45 本、子 rules/host.rs に rules_host_・host_group_ で始まる歯 27 本が在る。2 つの子は rules.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 rules.rs には snapshot の歯 1 本・他の歯・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、snapshot と .config/nextest.toml と行 e の歯は GREEN のまま不変 (6) write-set の rules.rs の - は縮む面（file は残り、約 1,590 行が減る）で、diff は親の M と子 2 つの A だけ（この doc を含まない）"

[[contract]]
id = "k"
title = "pipe の置き場の歯から file 数と宣言の比べを外す — pipe_hermetic_sites_stay_one は binary を起こす字面の合計 1 だけを測り、tracked と宣言の一致は行 e の歯に任せる（入れ子の子の準備・札 retroactive・§10 行 k）"
req = ["FR33", "FR36"]
section = "10"
write-set = ["crates/scribe2-boundary/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_hermetic_sites_stay_one"]
size = "S"
done = "(1) 歯 pipe_hermetic_sites_stay_one は pipe.rs と pipe/ の下の tracked な file（入れ子を含む）で binary を起こす字面 2 形の出現の合計が 1 であることだけを測り、file の数を pipe.rs の mod 宣言の数と比べない。message は読んだ file の数・site の数・base の 41 を出す (2) 歯の doc comment は、tracked な file と宣言の一致を行 e の歯 e2e_fixture_clock_dated_reset_lines_are_pinned が e2e の全体で測ると書き、file 数と宣言の比べの説明を持たない (3) この便の bead id の flip-check: retroactive の札が歯の区間に在り、base から持ち越した s2-07l.547 の札の行は無い (4) pipe.rs の他の歯と宣言と helper、e2e の他の file は不変"

[[contract]]
id = "l"
title = "e2e の pipe/land.rs を族ごとの子 module へ割る — follow・retire と train・terminal と order・rebase と onto の族を子 pipe/land/follow.rs・retire.rs・order.rs・rebase.rs へ純移動し、他の族と helper は親に残す・札 moved（§10 行 l）"
req = ["NFR1", "FR7"]
section = "10"
depends = ["k"]
write-set = ["-crates/scribe2-boundary/tests/e2e/pipe/land.rs", "+crates/scribe2-boundary/tests/e2e/pipe/land/follow.rs", "+crates/scribe2-boundary/tests/e2e/pipe/land/retire.rs", "+crates/scribe2-boundary/tests/e2e/pipe/land/order.rs", "+crates/scribe2-boundary/tests/e2e/pipe/land/rebase.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_follow_main_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_retire_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_order_three_runs_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_land_rebase_"]
size = "M"
done = "(1) 子 pipe/land/follow.rs に名が pipe_follow_ で始まる歯 27 本、子 pipe/land/retire.rs に pipe_retire_・pipe_train_ で始まる歯 20 本、子 pipe/land/order.rs に pipe_terminal_・pipe_order_ で始まる歯 20 本、子 pipe/land/rebase.rs に pipe_land_rebase_・pipe_land_onto_ で始まる歯 16 本が在る。4 つの子は land.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 land.rs には他の歯 53 本・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、行 e の歯と pipe_hermetic_sites_stay_one は GREEN のまま不変 (6) write-set の land.rs の - は縮む面（file は残り、約 2,460 行が減る）で、diff は親の M と子 4 つの A だけ（この doc を含まない）"

[[contract]]
id = "m"
title = "e2e の pipe/gate.rs を族ごとの子 module へ割る — confine と slots・detection と landed・move と elide の族を子 pipe/gate/confine.rs・detection.rs・pure_move.rs へ純移動し、snapshot の歯と他の族と helper は親に残す・札 moved（§10 行 m）"
req = ["NFR1", "FR7"]
section = "10"
depends = ["k"]
write-set = ["-crates/scribe2-boundary/tests/e2e/pipe/gate.rs", "+crates/scribe2-boundary/tests/e2e/pipe/gate/confine.rs", "+crates/scribe2-boundary/tests/e2e/pipe/gate/detection.rs", "+crates/scribe2-boundary/tests/e2e/pipe/gate/pure_move.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_slots_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_landed_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_gate_elide_"]
size = "M"
done = "(1) 子 pipe/gate/confine.rs に名が pipe_confine_・pipe_slots_ で始まる歯 23 本、子 pipe/gate/detection.rs に pipe_detection_・pipe_landed_ で始まる歯 15 本、子 pipe/gate/pure_move.rs に pipe_gate_move_・pipe_gate_elide_ で始まる歯 25 本（snapshot の歯 pipe_gate_move_summary_external_form を除く）が在る。3 つの子は gate.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 gate.rs には snapshot の歯 2 本・他の歯・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、snapshot と行 e の歯と pipe_hermetic_sites_stay_one は GREEN のまま不変 (6) write-set の gate.rs の - は縮む面（file は残り、約 1,380 行が減る）で、diff は親の M と子 3 つの A だけ（この doc を含まない）"

[[contract]]
id = "n"
title = "e2e の pipe/dispatch.rs を族ごとの子 module へ割る — group・waiting と release と gated・terminal の族を子 pipe/dispatch/group.rs・waiting.rs・terminal.rs へ純移動し、super:: を持つ歯 6 本と他の族と helper は親に残す・札 moved（§10 行 n）"
req = ["NFR1", "FR7"]
section = "10"
depends = ["k"]
write-set = ["-crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch/group.rs", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch/waiting.rs", "+crates/scribe2-boundary/tests/e2e/pipe/dispatch/terminal.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_group_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_gated_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_terminal_dispatch_manual_turn_"]
size = "M"
done = "(1) 子 pipe/dispatch/group.rs に名が pipe_dispatch_group_ で始まる歯 60 本、子 pipe/dispatch/waiting.rs に pipe_dispatch_waiting_・pipe_dispatch_release_・pipe_dispatch_gated_・pipe_dispatch_regated_・pipe_dispatch_revive_ で始まる歯 35 本、子 pipe/dispatch/terminal.rs に pipe_terminal_ で始まる歯 7 本が在る（どの子も本文に super:: を持つ歯を含まない）。3 つの子は dispatch.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 dispatch.rs には本文に super:: を持つ歯 6 本・他の歯 38 本・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、行 e の歯と pipe_hermetic_sites_stay_one は GREEN のまま不変 (6) write-set の dispatch.rs の - は縮む面（file は残り、約 1,890 行が減る）で、diff は親の M と子 3 つの A だけ（この doc を含まない）"

[[contract]]
id = "o"
title = "e2e の pipe/spawn.rs を族ごとの子 module へ割る — question と resume・approval と run_cost と report の族を子 pipe/spawn/question.rs・approval.rs へ純移動し、他の族と helper は親に残す・札 moved（§10 行 o）"
req = ["NFR1", "FR7"]
section = "10"
depends = ["k"]
write-set = ["-crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "+crates/scribe2-boundary/tests/e2e/pipe/spawn/question.rs", "+crates/scribe2-boundary/tests/e2e/pipe/spawn/approval.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_resume_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_approval_"]
size = "S"
done = "(1) 子 pipe/spawn/question.rs に名が pipe_question_・pipe_resume_ で始まる歯 18 本、子 pipe/spawn/approval.rs に pipe_approval_・run_cost_・pipe_report_ で始まる歯 14 本が在る。2 つの子は spawn.rs の mod 宣言で宣言され、頭が use super::*; である (2) 親 spawn.rs には他の歯 37 本・helper と const と use の行の全部が残り、可視性は不変 (3) 移した歯の本文（直前の doc と属性の行を含む）は base と 1 byte も違わず、親と子で増減した行は空行・use の行・mod <名>;・comment・札だけで、move_proof が純移動と判定する (4) この便の bead id の flip-check: moved の札が各子の先頭と親の mod の宣言の直後に在り、flip-check が moved で通る (5) e2e の歯の本数は base = head で、行 e の歯と pipe_hermetic_sites_stay_one は GREEN のまま不変 (6) write-set の spawn.rs の - は縮む面（file は残り、約 950 行が減る）で、diff は親の M と子 2 つの A だけ（この doc を含まない）"

[[contract]]
id = "p"
title = "xtask の check の公開の字面の門の母集団の口 tracked_files は、根が git の toplevel でない周も根の下の追跡 file が 1 本以上あれば根からの相対の列を返し、paths-clean と private-clean が n/a で黙らずに測る（0 本の周だけ今どおり n/a・§11）"
req = ["FR52"]
section = "11"
write-set = ["crates/xtask/src/paths_clean.rs", "crates/xtask/src/private_clean.rs"]
verify = ["cargo nextest run -p xtask --no-tests=fail paths_clean_measures_a_nested_root_by_its_own_tracked_files", "cargo nextest run -p xtask --no-tests=fail private_clean_measures_a_nested_root_by_its_own_tracked_files", "cargo nextest run -p xtask --no-tests=fail paths_clean_is_na_outside_repo_root", "cargo nextest run -p xtask --no-tests=fail private_clean_is_na_outside_repo_root"]
size = "S"
done = "(1) tracked_files は根が toplevel でない周に git -C <根> ls-files -s -z を撃ち、1 本以上あれば根からの相対の列を母集団に返し（形の読めない件が在れば全体を測れないへ倒す・toplevel の周と同じ読み）、paths-clean は根の下の追跡 file の本文の private path 形を違反と数えて値は走査した件数になる (2) private-clean も同じ口を通り、根の下の追跡 file の本文の email の needle を違反と数えて値は n/a でない (3) 根の下に追跡 file の無い dir は paths-clean と private-clean とも今どおり n/a(not-a-repo-root) で違反 0"
done-teeth = ["1:paths_clean_measures_a_nested_root_by_its_own_tracked_files", "2:private_clean_measures_a_nested_root_by_its_own_tracked_files", "3:=paths_clean_is_na_outside_repo_root", "3:=private_clean_is_na_outside_repo_root"]

<!-- contracts:end -->
