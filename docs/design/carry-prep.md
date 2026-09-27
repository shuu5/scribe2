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
  - 名を迂回する字面: build 元 commit の compile 時の env の名 `SCRIBE2_BUILD_COMMIT` が core と境界 crate と e2e の 12 site に literal で在る（憲法 C2.2 は env の接頭辞を NAME から導くと定める・`crates/scribe2/build.rs` の doc が literal を限界として認めている）。

## 2. 形（契約表の行 a〜c・1 つずつ歯が測る・done と 1:1）

1. **行 a — 消えた機構の残りを消し、seat の案内を今の形に直す**: 上の 13 個と、それらからだけ参照される item を消す。module の doc は今在る子 module と口だけを書き、消えた item への link を持たない。help の seat の頁の examples は orchestrator の役割と `s2:orchestrator` の target で書く。消すのは参照 0 を git grep で確かめた item だけで、生きている定数（起動・立て直し・停止が共有する `REASON_` の残り・`WHO_LAUNCH`・`HOLE`）と rules 行 `seat.cycle_*` は残す。
2. **行 b — lens の prompt から消えた役割の名を外す**: 裁定の節の見出しを「便の質問への回答」の語に、本文の「回答で planner が認めた形」を「回答で認めた形」に替える。節の位置・中身（裁定の対の逐語）・無いときの「（裁定なし）」は不変。doc comment の同じ語も揃える。
3. **行 c — build 元 commit を名の無い形で焼く**: build script は値を compile 時の env ではなく build の出力 dir の 1 file に書き、core が `include_str!` で 1 つの pub const に読む。core・境界 crate・e2e の全 site はその const を読み、env の名の literal は repo から消える。値の形（`<sha12>` / `<sha12>+dirty` / `unknown`）と測り方（HEAD と作業木の汚れ・再走の母集団）は不変。境界 crate は build script を持たなくなる（値は core の const を読む）。

## 3. 触らない

- on-disk の形と鍵（state dir の名・git config の鍵・marker・commit の trailer・systemd の unit 名）: 跨版の契約で、v3 がどれを旧値で読むかを決める。
- 器の名と CLI の名の定数の分離: 憲法 C2.2 の解釈に触れる。v3 の判断の記録（CLI の名は器の名と別の定数）が根拠を持つので v3 の側で行う。
- 実測の口座の記録（`crates/scribe2/src/seat/session_account.rs`）と SRS FR71 の食い違い: 消すか要件を改めるかは持ち主の裁定が先（台帳 `s2-07l.669`）。
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
- 行 c（`crates/scribe2/src/name.rs` の歯の module・`build_commit_` 接頭辞）: core の const が `<sha12>` / `<sha12>+dirty` / `unknown` のどれかの形である（base に const は無い＝RED）。既存の e2e（version の行・binary の世代の記録・consumer の drift）は同じ const を読んで GREEN のまま。

## 6. 後続（行は本設計の着地の後に同じ doc へ足す）

- 境界の整理: JSON の道具（`crates/scribe2/src/fleet/json_lite.rs`・`crates/scribe2/src/fleet/json_tree.rs`）を fleet から leaf module へ純移動する・`StateDir` と vessel の marker の置き場を leaf へ出して top-level module の輪を切る・`seat::cycle` を中身どおりの名へ改める。file を移す便は、その path を素の項目で名指す過去の契約表の行を同じ PR で新しい path へ直す（境界 crate の新設の便と同じ扱い）。
- e2e の大きさ: 2500 行を越える e2e の 9 file を族ごとの子 module へ割る。前提は e2e の file 数を literal で pin する歯（`crates/scribe2-boundary/tests/e2e/main.rs` の `e2e_fixture_clock_dated_reset_lines_are_pinned`）を宣言から導く形へ直すこと。
- 契約の型の leaf module 化は v3 の面の契約の設計（serde を持つ crate と憲法 C13.2 の derive の置き場）が決まってから。

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
done = "(1) seat/mod.rs から作業記憶の数え（scan_wm と WmScan とそれらからだけ参照される定数・関数）と statusline の探索（search_region・tail_nonempty・TAIL_LINES）が消え、seat/cycle.rs から DEFAULT_RESTORE・REASON_LOCK_HELD・REASON_WM_MISSING・REASON_WM_UNREADABLE・REASON_STATE_MISSING・REASON_STATE_UNREADABLE・REASON_STATE_STALE・REASON_STAMP・REASON_CLEAR が消え、seat::role::relabel と headless::INCONCLUSIVE_HEAD が消える (2) seat/mod.rs と seat/cycle.rs の module の doc は今在る子 module と口だけを書き、seat/mod.rs・seat/cycle.rs・seat/cycle/relaunch.rs の doc に消えた item（meter・externalize・consume・DEFAULT_RESTORE）への link が無い (3) help の seat の頁の examples は --role orchestrator と --target s2:orchestrator の形で、help の全頁の examples の --role の語が全部 seat::role::Role::parse で解けることを歯 help_table_role_ が測る (4) 生きている定数（起動・立て直し・停止が共有する REASON_ の残り・WHO_LAUNCH・HOLE）・rules 行・使い方の 1 行・doctor・極性一覧・外形 snapshot は不変"

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
done = "(1) build script は値（<sha12> / <sha12>+dirty / unknown・測り方と再走の母集団は不変）を cargo の出力 dir の 1 file に書き、compile 時の env を出さない (2) core の name.rs に pub const BUILD_COMMIT が在り include_str! でその file を読み、歯 build_commit_ が値の 3 つの形のどれかであることを測る (3) core・境界 crate の src・e2e の全 site が BUILD_COMMIT を読み、SCRIBE2_BUILD_COMMIT の字面が crates の下に 0 件 (4) 境界 crate の Cargo.toml は build script を持たない (5) version の行・binary の世代の記録・consumer の drift の既存の歯は同じ値で GREEN のまま"
<!-- contracts:end -->
