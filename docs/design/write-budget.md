# 設計: 書き込みの検出線 — host の面が名指す装置の 1 日の書き込みを管理 tick の周で測って host の根に記し、7 日平均・1 日・続いた日数の線と比べて doctor の行と席の 1 行で示す（作業は止めない）

- 要件: [FR113](../../design-intent/spec/srs.html#FR113) 書き込みの検出線 / [AC90](../../design-intent/spec/srs.html#AC90) その受け入れの基準 / [FR27](../../design-intent/spec/srs.html#FR27) 管理 tick（周が書き込みの記録の契機を兼ねる）/ [FR57](../../design-intent/spec/srs.html#FR57) host の面（表の読みと断り）/ [NFR4](../../design-intent/spec/srs.html#NFR4) 読めない記録は測れないと言う / [NFR5](../../design-intent/spec/srs.html#NFR5) hook の予算 / [FR44](../../design-intent/spec/srs.html#FR44) 入力欄への差し込みは使わない。
- 憲法: [C1](../../design-intent/spec/constitution.html#c1) / [C5](../../design-intent/spec/constitution.html#c5) 線と窓と段の値は rules 行・裁定 id 付き / [C2.2](../../design-intent/spec/constitution.html#c2) env を読まない・置き場は state dir の親から導く / [C3](../../design-intent/spec/constitution.html#c3) host の根に state の真実を置かない（記録は装置の累計から取った出所つきの実測）/ [C10](../../design-intent/spec/constitution.html#c10) 測れない日と欠けた日を 0 や完全な値と書かない / [C15](../../design-intent/spec/constitution.html#c15) 台帳は task と裁定だけ（実測は器の記録）/ [C17](../../design-intent/spec/constitution.html#c17) 新しい timer・verb・口を足さない / [N1](../../design-intent/spec/constitution.html#n1) 記録を消さない / [N3](../../design-intent/spec/constitution.html#n3) host ごとの値は面にだけ置き code は分岐しない。
- 決定: [ADR-0112](../../design-intent/decisions/ADR-0112-the-vessel-measures-the-daily-host-writes-against-the-write-detection-line.html)（本 doc の決定の正本）。
- 土台: [seat-heartbeat.md](./seat-heartbeat.md) §2（管理 tick の判定の列）/ §12（tick-last の打刻と「2 × 周期」の健全）・[rules-manifest.md](./rules-manifest.md) §4（rules 行と §4.1 の表）・[account-lifecycle.md](./account-lifecycle.md) §2 / §7（host の面 `host.toml` と読み手）/ §19 形 5（席の追加文脈の群の逼迫の 1 行）/ §20（host の根の群用 dir）・[host-init.md](./host-init.md) §15（host の面の表 `[[device]]` の前例）・[vessel-hook.md](./vessel-hook.md) §19（host の面の表 `[[publish-exclusion]]` の前例）。
- この設計から出る契約: 行 a（§2〜§4・面の表と標本と記録）→ 行 b（§5・§6・rules 行 4 本と判定と doctor の行）→ 行 c（§7・席の 1 行）。値の裁定は持ち主 2026-10-03T02:42Z（器の rules 行の裁定の字は `user 2026-10-03T02:42Z 項 <語>`〔行ごとに項の語で分ける〕・裁定日 2026-10-03・逐語は本 repo の外の台帳）。

## 1. 何を解くか

やさしく言うと: host の記憶装置（NVMe）は書いた量で寿命が減る。今の器はその量を測らず、持ち主と席が装置の数を手で読んで割り算している。器が管理 tick の周のついでに装置ごとの 1 日（UTC）の書き込みを測って host の根に記し、持ち主の決めた線（7 日の平均で 1 日 1.5 TB・1 日 3 TB・3 日続けて）と比べて、doctor の 1 行と、越えた周だけ席への 1 行で示す。作業は止めない。測れなかった日と一部が欠けた日は 0 と書かず、別の語で残す。

- 出所: 持ち主の値の裁定（上の裁定 id）。要旨は 4 点 — 7 日の平均で 1 日 1.5 TB 以下・1 日に 3 TB を越えた日も知らせる・持ち主へは 3 日続けて越えた時だけ・作業は止めない。器の仕組みを置くこと・測り方・示す口・「3 日続けて」の数え方・装置が複数ある host の扱いは、持ち主の常設の裁定（決めてほしいことは推奨で進める・user 2026-09-28T00:54Z）の下で orchestrator が推奨を採った（ADR-0112）。
- 何が起きているか（2026-10-03 の実測）: 書き込みの多い装置 1 台は起動からの累計で約 8.2 日に約 39.7 TB（1 日の平均で約 4.9 TB）、もう 1 台は約 0.2 TB だった。[gate-cost.md](./gate-cost.md) §50 は検出線を 1 日 1 回に間引くが、間引いた効きを日ごとに測る物差しは器に無い。
- 3 つの決め（ADR-0112 の推奨どおり）:
  1. 「席への memo」は席の hook の追加文脈の 1 行（§7）。器が台帳の memo を自分で起こす口は足さない（C15・新しい口を増やさない）。
  2. 「3 日続けて越えた」は、閉じた日ごとに「1 日の線」か「その日で終わる 7 日平均の線」のどちらかを越えた日が 3 日続くこと（§5）。
  3. 装置が複数ある host は、表の行ごとに独立に判じ、和は取らない（§3・§5）。
- 消すもの（C17.2）: 持ち主と席が手で撃つ使い捨ての読み（装置の stat file を読んで割り算する）を doctor の 1 行に置き換える。
- 作業を止めない: 管理 tick・doctor・hook の rc と既存の行の字は変えない。器の門ではないので極性の行は足さない。

## 2. 測る周 — 管理 tick の判定の後に、表の行ごとに host の根の記録を lock の内で 1 回だけ進める（契約表の行 a）

やさしく言うと: 席ごとの管理 tick は 15 秒ごとに回る。その判定の後に「装置の累計を読んで host の根の記録を進める」1 本を足す。同じ host の席が何本 tick を回しても、記録は lock の内で 1 回だけ進み、周期より短い間隔では読み直さない。読む値は装置の累計なので、席が増えても水増しにならない。

- 現物（main f2d14990・verified）:
  - `crates/scribe2/src/seat/tick.rs` の run は判定の後に stamp_last（登録 row の在る target だけ・row の無い target は dir も作らない）・局面の出力の部分の書き直し・full_rewrite（event log と manifest を読めた周の枝だけ）を撃ち、返りを捨てる。判定行・rc・stderr はそれらに依らない。run が受ける manifest は tracked の面で、host の面は判定の中で `crates/scribe2/src/rules/mod.rs` の with_state_dir が合わせる。
  - 周期の rules 行の id は同じ tick.rs の pub な const ROW_INTERVAL（`seat.tick_interval_s`・値 15）。tick の判定（同じ file の Rows::of）はこの行を必須に読み、読めない周は判定が no-rule（rc 1・tick-last は `decision=error reason=no-rule` で終わる）になる。その周も stamp_last・部分の書き直し・full_rewrite（event log と manifest を読めた周の枝）は撃つ（既存の歯 seat_tick_status_no_rule_round_still_stamps）。
  - host の根は `crates/scribe2/src/seat/mod.rs` の私有の host_root（state dir の親の下の `<NAME>-host`・env を読まない）で、外へ開く口は host_slots_dir と host_groups_dir の 2 つ。
  - lock は `crates/scribe2/src/fleet/store.rs` の acquire（pub(crate)・取れた lock に自分の pid と起動時刻を書き、所有者の死んだ lock と古い lock を回収する・外すのは呼び手が lock file を消すこと）。待ち方 LockPolicy の 2 欄は from_rules が rules 行 `fleet.lock_retry_ms`（5000）と `fleet.lock_stale_ms`（30000）から組む。群の段の lock（`crates/scribe2/src/hook/group.rs` の Lock）は回収を持たない。
  - 登録 row の引き手は `crates/scribe2/src/seat/role.rs` の registration_of_target と fleet の replay。時刻の字は `crates/scribe2/src/fleet/cli.rs` の format_utc（UNIX 秒 → `YYYY-MM-DDTHH:MM:SSZ`）の 1 本、今の秒は `crates/scribe2/src/seat/state.rs` の now_secs。
- 形（番号は行 a の done と 1:1）:
  1. **host の面の表**: §3 の表 `[[write-budget]]` を host の面の読み手が組み、合わせた manifest が宣言順の行（name・stat・見出しの行番号）を返す。
  2. **呼ぶ所**: run は full_rewrite の後（event log と manifest を読めた周の同じ枝）に、置き場・target・tracked の manifest・読んだ event の列を行 a の write-set の `+` の file（fleet の子 module）の標本の 1 本へ渡し、返りを捨てる。標本の 1 本は stdout と stderr に 1 byte も書かず、判定行・rc・stderr・tick-last の字は表の無い置き場の同じ周と同じである。
  3. **測らない周**: target に登録 row が無い周（stamp_last と同じ門）・合わせた manifest を読めない周・表が 0 行の周は何もしない（host の根の下に dir を作らない）。周期の行（ROW_INTERVAL）か lock の 2 行のどれかを読めない周も記録を書かない（既定の値で埋めない・C5）。周期の行を読めない周は判定が no-rule でも標本の 1 本は full_rewrite の枝で呼ばれるので、標本の 1 本が自分で行を読んで止まる。
  4. **置き場と独立**: 表の行ごとに host の根の `write-budget/<name>/` の dir（`crates/scribe2/src/seat/mod.rs` の host_groups_dir の隣に足す pub の関数 1 つが返す dir の下）を持ち、§4 の 2 file と lock file `open.lock` を置く。行ごとに独立に進め、和は取らない。
  5. **最初の読みと同じ日の足し**: 記録の無い周は今日を partial・written 0 で開く（0 時からを測っていない）。同じ UTC 日の読みが前の読み以上なら、差 × 512 byte を written に足す。
  6. **同じ日の再起動**: 同じ日の読みが前の読みより小さい周は再起動と読み、読み × 512 を足し、reboots を 1 増やし、state を partial にする（停止の前の最後の読みの後の書き込みが欠ける＝下限）。
  7. **早抜け**: 読めた記録が今と同じ UTC 日で probed + 周期 > 今（加算は飽和）の周は何も書かない。判定は pure な関数 1 つで、lock の外（安い早抜け）と lock の内（読み直し）の 2 か所が同じ関数を呼ぶ。
  8. **日を閉じる（measured）**: 日付が進んだ周は、読みが前の読み以上 ∧ 今日が記録の日の次の日 ∧ 今 − at ≤ 2 × 周期（tick の健全と同じ係数）の 3 条件がそろうときだけ、前の日を written + 差 × 512・前の state・前の reboots・tail `-` で閉じて days.log へ足し、今日を measured・written 0・reboots 0 で開く（境目をまたぐ 2 標本の間の書き込みは前の日に数える）。
  9. **日を閉じる（partial と unmeasured）**: 形 8 の 3 条件のどれかを欠く周は、前の日を written のまま partial・前の reboots・tail = 差 × 512（読みが減った周は読み × 512）で閉じ、間の日（記録の日の次の日から昨日まで）を 1 日 1 行の `written=- state=unmeasured reboots=0 tail=-` で足し、今日を partial・written 0 で開く（今日の reboots は読みが減った周だけ 1・ほかは 0）。
  10. **読めない・形でない・装置の違い**（判定の順は装置の違い → 読み）: 記録の stat が表の stat と違う周は probe=stat-mismatch、stat file を読めない周（無い・権限・dir）は probe=unreadable、1 行目を空白で割った欄が 7 つ未満か 7 つ目が 10 進でない周は probe=malformed とし、どれも記録の数（date・sectors・at・written・state・reboots）を変えずに probed と probe だけを書き換える（fail-closed・NFR4）。記録の無い周に読めない・形でないときは何も書かない。
  11. **書かない周**: lock を取れない周と、記録の `open` が在るのに読めない周（dir・§4 形 1 の形でない）は記録を 1 byte も変えない。測りは止まり、doctor が unreadable を出す（§6）。直しは人が dir を退避する（N1）。
  12. **閉包**: 行 a の write-set の `+` の file 2 つは、ほかの行の touches に在る型（`crate::rules::RuleKind`・`crate::fleet::EventKind`・`crate::seat::tick::Judged` など）を literal・match の arm・variant の構築・const slice の形で持たない（登録 row は registration_of_target と replay で引き、rules 行は id の字で int_row から読む）。行の検証行の最後で、便の木の契約表の検査（`contracts check`）を撃つ。
- 判定の順（変異の A/B の順）: 登録 row → 面と表 → 周期と lock の行 → 早抜け → lock → 記録の読み → 装置の違い → stat の読み → 進め（記録が無い → 同じ日〔以上・減り〕→ 日付が進んだ〔3 条件〕）。
- 後の行が呼ぶ口（行 b）: 行 b は自分の `+` の file（fleet の兄弟 module）から次の 3 つを呼び、行 a の `+` の file は書き換えない。
  - 合わせた manifest の表の行の読み手（rules の Manifest の pub な口・宣言順の name と stat）。
  - 記録の dir の口（`crates/scribe2/src/seat/mod.rs` の pub の関数・形 4）。
  - open の読み手: 引数は記録の dir、返りは閉じた 3 値（無い・読めない・読めた 10 欄の値）。行 a の進め（形 7 の早抜けと形 11）も記録をこの 1 本で読む。
  open の読み手と、返りの型とその欄は fleet の中から見える可視性（pub(super)）で置く。fleet の子 module の宣言は pub mod にする（seat の tick.rs が標本の 1 本を呼ぶ）。days.log の読み手は行 a に置かない: 行 a は days.log を追記するだけで読まず、呼び手の無い pub(super) の口は common-verify の clippy の dead_code で落ちる（allow の属性は lint が断る）。days.log は行 b の `+` の file が §4 形 2 の読みで読む（§5 形 2）。
- 歯（置き場は既存の e2e の file・新設の test file は作らない・helper は歯と同じ file に置く・lib の新しい口を歯から呼ばない＝base で compile が通り機能不在で落ちる）:
  - e2e（`crates/scribe2-boundary/tests/e2e/seat/tick.rs`・接頭辞 seat_tick_write_budget_・crates と docs で 0 件〔2026-10-03 の grep〕）。fixture: 親の `crates/scribe2-boundary/tests/e2e/seat.rs` の tick_place の置き場（呼ぶたびに新しい tmp の根の下の state・host の根は同じ tmp の根の下の `<NAME>-host`・子の file から呼べる）の `host.toml` に表を書き、stat は tmp の根の下の fixture の file（17 欄の 1 行・7 欄目が書いた区の数）を指す。sysfs は読まない。記録の字は §4 の形を歯が組んで書き・読む。日付は撃つ前の今から組み、撃った後の open の at が撃つ前後の間であることを assert する。`--rules` の写しは tick の判定が読む行と lock の 2 行を持つ。
    - (a) seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors: 表 2 行（nvme-a の 7 欄目 1000・nvme-b の 7 欄目 70）の置き場と、表の無い同じ fixture の置き場を同じ周（黙り 100 秒の noop）で撃つ → 2 つの rc・stdout・stderr が等しく、2 つの tick-last が `ts=` の語を除いて等しく、表の無い置き場の host の根に `write-budget` の dir が無い。表の置き場の nvme-a の open がちょうど `schema=1 stat=<a の path> date=<今日> sectors=1000 at=<t> written=0 state=partial reboots=0 probed=<t> probe=ok` と改行（t は撃つ前後の間）・nvme-b は sectors=70・days.log は無い。nvme-a の 7 欄目を 3000 にし、2 つの open の probed を 0 に書き換えて（書き換えた字を assert）撃つ → nvme-a は written=1024000・sectors=3000・state=partial、nvme-b は written=0・sectors=70（和を取らない）。〔形 1・2・4・5〕
    - (b) seat_tick_write_budget_skips_a_round_inside_the_period: 埋め込みの rules（周期 15）で 1 回撃った直後に 7 欄目を 5000 にして撃つ → open の bytes が撃つ前と等しい。続けて probed を 0 に書き換えて撃つ → written=2048000（対照）。〔形 7〕
    - (c) seat_tick_write_budget_counts_a_reboot_inside_the_day: open を `… date=<今日> sectors=5000 at=<今日の 0 時> written=100 state=measured reboots=0 probed=0 probe=ok` に書き、7 欄目 1200 で撃つ → written=614500・sectors=1200・state=partial・reboots=1。〔形 6〕
    - (d) seat_tick_write_budget_closes_the_day_measured_within_two_periods: 周期の行を 86400 にした写しで、open を `date=<昨日> sectors=1000 at=<今日の 0 時 − 5> written=7000 state=measured reboots=0 probed=<同じ値>` にし、7 欄目 3000 で撃つ → days.log がちょうど `schema=1 date=<昨日> written=1031000 state=measured reboots=0 tail=-` と改行・open が今日・sectors=3000・written=0・state=measured・reboots=0。〔形 8〕
    - (e) seat_tick_write_budget_closes_the_day_partial_after_a_reboot: (d) と同じ写しと open（sectors=1000）で 7 欄目 600（減り）→ days.log がちょうど `… date=<昨日> written=7000 state=partial reboots=0 tail=307200`・open が今日・sectors=600・state=partial・written=0・reboots=1。〔形 9 の読みの減り〕
    - (f) seat_tick_write_budget_closes_the_day_partial_after_a_late_sample: 埋め込みの rules（周期 15）で open を `date=<昨日> at=<昨日の 12 時> sectors=1000 written=7000 state=measured` にし 7 欄目 3000 → days.log がちょうど `… date=<昨日> written=7000 state=partial reboots=0 tail=1024000`・open が今日・state=partial・reboots=0。〔形 9 の 2 × 周期〕
    - (g) seat_tick_write_budget_closes_a_gap_with_unmeasured_days: 周期の行を 345600 にした写し（2 × 周期が 8 日＝遅れの条件に掛からない）で open を `date=<3 日前> at=<3 日前の 12 時> sectors=1000 written=500 state=measured` にし 7 欄目 4000 → days.log がちょうど 3 行（3 日前の `written=500 state=partial reboots=0 tail=1536000`・2 日前と昨日の `written=- state=unmeasured reboots=0 tail=-`）・open が今日・state=partial。〔形 9 の日の跳び〕
    - (h) seat_tick_write_budget_names_an_unreadable_or_malformed_stat: 1 回撃った後、probed を 0 にして stat file を消して撃つ → probe=unreadable・probed が撃った時刻・ほかの 8 語が前と等しい。6 欄の stat と 7 欄目が `x` の stat → どちらも probe=malformed で数は前と等しい。記録の無い別の置き場で stat が無い → open も days.log も無い。stat を置いて撃つ → open が在る（対照）。〔形 10 の読み〕
    - (i) seat_tick_write_budget_refuses_a_changed_stat_path: 記録の stat が表の stat と違う open（ほかの語は (c) の形）で撃つ → probe=stat-mismatch・数は前と等しい。〔形 10 の装置の違い〕
    - (j) seat_tick_write_budget_leaves_the_record_while_the_lock_is_held_or_the_record_is_unreadable: `fleet.lock_retry_ms` を 200 にした写しで、`open.lock` に歯の process の pid の 1 語を書き（字を assert）、probed を 0 にして撃つ → open の bytes が等しい。lock file を消して撃つ → written が増える（対照）。open を dir にした置き場で撃つ → dir の中身が空のまま・days.log が無い。〔形 11〕
    - (k) seat_tick_write_budget_writes_nothing_outside_a_registered_seat_with_a_table: 登録 row の無い target（tick_place の未登録の形）の置き場に表を書いて撃つ → host の根に `write-budget` の dir が無い。登録 row の在る置き場を、周期の行を欠く写し・`fleet.lock_retry_ms` を欠く写し・`fleet.lock_stale_ms` を欠く写し（どれも欠く行のほかは同じ写し）でこの順に撃つ → どの周も tick-last の `ts=` が撃つ前後の間（tick が撃たれ full_rewrite の枝に届いた前提）で open が無い。rc と tick-last の終わりは、周期の行を欠く周が rc 1 と `decision=error reason=no-rule`（判定も周期の行を読む）、lock の行を欠く 2 周が rc 0 と `reason=no-rule` で終わらない打刻。同じ置き場を埋め込みの rules で撃つ → open が在る（対照）。〔形 3〕
  - e2e（`crates/scribe2-boundary/tests/e2e/rules/host.rs`・接頭辞 rules_host_write_budget_・0 件）: `rules validate --state-dir` の rc・stdout・stderr だけを測る。
    - (l) rules_host_write_budget_two_rows_validate_like_a_tableless_face: 表 2 行の面で validate が rc 0・stdout が表の無い面の 1 行と同じ字。〔形 1〕
    - (m) rules_host_write_budget_refuses_each_defect_once_with_its_line: 1 行の面を崩した 7 形（stat の欠け・未知の key・name `a/b`・name が空・相対の stat・空白を含む stat・name の重複）がそれぞれ rc 1・stdout 0 行・stderr がちょうど §3 の字の `rules: host.toml: <字> line=<n>` の 1 行で、崩す前の 1 行は rc 0。〔形 1〕
    - (n) rules_host_write_budget_table_on_the_tracked_face_is_refused_once_per_table: tracked の写し（`--rules`）に置いた 2 表が、見出しの行番号つきの 2 件 `[[write-budget]] は tracked の manifest に置けない（書き込みの測りの装置は host の面だけ）`。〔形 1〕
  - 変わらない既存の歯（行の検証行に載せる）: seat_tick_status_tick_last_mirrors_the_judgement_line（tick-last の字）。
  - 変異の A/B（判定の順・条件 1 つに歯 1 本）: 登録 row を見ないと (k) の 1 段目が落ちる。周期の行を既定値で埋めると (k) の 2 段目が、lock の 2 行をそれぞれ既定値で埋めると (k) の 3 段目と 4 段目が落ちる。早抜けを外すと (b) が落ちる。lock を取らないと (j) の 1 段目が落ちる。読めない記録を「無い」と読むと (j) の 2 段目が落ちる。装置の違いを見ないと (i) が落ちる。読めない stat で数を動かすと (h) が落ちる。同じ日の減りを足しにすると (c) が落ちる。3 条件の読みの減り・2 × 周期・次の日をそれぞれ外すと (e)・(f)・(g) が落ちる。measured の閉じで差を足さないと (d) が落ちる。行ごとの和を取ると (a) が落ちる。
- base で RED の理由（機能不在）: base は `[[write-budget]]` を未知の section として断る＝(l)(m)(n) の rc と字が違い、表を持つ置き場の tick は記録を書かない＝(a)〜(k) の各歯の対照の assert（open が在る・字が変わる）が落ちる。

## 3. host の面の表 `[[write-budget]]`（行 a）

- 置き場: host の面（`<state_dir>/host.toml`）にだけ置ける。0〜n 行。tracked の面（埋め込みと `--rules`）に置いた表は中身を検査せず 1 表 1 件で `[[write-budget]] は tracked の manifest に置けない（書き込みの測りの装置は host の面だけ）` と断る（`[[device]]` と `[[publish-exclusion]]` と同じ置き方・装置の path は host 固有で PUBLIC repo に載せない・CON2）。
- key は 2 つで両方必須（欠けと未知の key は既存の字「必須 key <k> が無い」「未知の key <k>」）:
  - `name`: 記録の dir の名。英数字と `-` と `_` の 1 字以上（外れは `name "<値>" は英数字と - と _ の 1 字以上でない`）。面の中で一意（2 度目の行の見出しの行番号で `書き込みの測りの名 <name> が重複する`）。
  - `stat`: block 装置の stat file の絶対 path（形は `/sys/block/<device>/stat`）。相対は `stat "<値>" が絶対 path でない`、空白を含むものは `stat が空白を含む: "<値>"`。
- 断りは行番号つきで 1 件ずつ・`host.toml:` の接頭辞・`rules validate` は rc 1。`rules validate --state-dir` の 1 行は表の無い面と同じ字（表を数えない）。
- 組み立ては rules の兄弟 module（行 a の write-set の `+` の file・見出しと key の列と組み立てと名の重複の検査を持つ）で、`crates/scribe2/src/rules/manifest.rs` の section の閉じた列に見出しを 1 つ足し、host の面の読みと合わせの口（joined）がその module を呼ぶ。`[[device]]` は端末の表が使う見出しなので分ける。
- device の名でなく file の path を書く理由: code に「`/sys/block/` + 名」を焼くと歯が sysfs を直に読むことになり、差し替えの口（env・root の seam）は C2.2 で作れない。path なら歯は tmp の fixture file を指せ、host ごとの値は面だけに在る（N3）。
- 面の注記（code は判じない）: 物理の装置だけを書く（dm や partition の装置を足すと同じ書き込みを 2 度数える）。同じ host の state dir の面は同じ行を持つ（雛形から写す・食い違いは §9 の限界）。

## 4. on-disk の記録（schema=1・行 a）

- 置き場: host の根の `write-budget/<name>/` に 2 file と lock file。host の根は state の真実を置かない（C3）。記録は装置の累計（真実は kernel の数）から取った出所つきの実測で、host・口座・席・lease・退役の状態を持たず、消えても判定は測っていない側（unmeasured）に倒れるだけである（群の今の口座を同じ根に実効の記録として置いた ADR-0049 の前例・ADR-0112）。
- 形（番号は §2 形 4 の 2 file）:
  1. **`open`**（今日の 1 行）: `schema=1 stat=<path> date=<YYYY-MM-DD> sectors=<n> at=<UNIX 秒> written=<byte> state=<measured|partial> reboots=<n> probed=<UNIX 秒> probe=<ok|unreadable|malformed|stat-mismatch>` と改行。sectors は最後に読めた書いた区の数、at はその読みの時刻、probed は最後に読みを試みた時刻。読めるのは、空白で割った 10 語がこの順で各 key= を持ち、schema が 1・stat が空でない・date が at の UTC の日付（format_utc の頭 10 字）と等しい・sectors / at / written / reboots / probed が 10 進・state と probe が閉じた語のときだけで、ほか（dir・読めない file・語の数と順と値の字の違い）は「読めない記録」の 1 値である。書きは同じ dir の `open.tmp` に書いて rename で置き換える。
  2. **`days.log`**（閉じた日の追記）: 1 行 `schema=1 date=<YYYY-MM-DD> written=<byte|-> state=<measured|partial|unmeasured> reboots=<n> tail=<byte|->`。written が `-` なのは state が unmeasured のときだけ（逆も）。tail は閉じた周に日へ割り振れなかった書き込み（partial の閉じだけ値を持つ）。追記は lock の内で、days.log への追記 → open の rename の順（途中で落ちた周は同じ date の行が重なる＝欠けにしない）。読み手は同じ date の最後の行を採り、形でない行が 1 行でも在る file は全体を「読めない」とする（無い file は空と読む）。
  3. **進め**: §2 形 5〜9 の規則は、入力（読めた記録か無い・読み・今・周期・表の stat）から出力（新しい open・days.log へ足す行の列）を返す pure な関数 1 つが持つ。日の番号は UNIX 秒 ÷ 86400（UTC）で、字は format_utc の頭 10 字（日付の字を読み戻さない）。
  4. **単位**: 書いた区の数は 512 byte 単位（装置の論理 block の大きさに依らない kernel の約束）。written と tail は byte の 10 進。
- event log へは書かない（置き場ごとに割れ、15 秒ごとの標本で肥大する・§8）。

## 5. 判定と rules 行 — 線と窓と段を rules 行 4 本に持ち、判定の 1 本が doctor と席の hook に同じ値を渡す（契約表の行 b）

やさしく言うと: 持ち主の決めた 4 つの数（平均の線・1 日の線・窓の日数・持ち主へ知らせる連続日数）を裁定つきの規則の行にする。行 a の記録から「今日・昨日・7 日平均・続いた越えの日数」を求める判定を 1 本だけ書き、doctor の行（§6）と席の 1 行（§7）はその値を写すだけにする。測れない日は下限として数え、下限だけで線を越えた日も越えと数える。

- 現物（main f2d14990・verified）:
  - kind の閉じた列 ALL（`crates/scribe2/src/rules/mod.rs`）は HostBlockedPerCore の直後が PipeLandWaitS。閉じた match は同じ file の as_str・shape・has_permit_reader の 3 つ（wildcard 無し）。行の読み手は同じ file の int_row（無い・不発効・整数でない周は理由つきの Err）。
  - 埋め込み manifest（`rules/manifest.toml`）は `host.blocked_per_core` の行の直後が `gate.tmux_test_threads` の行。
  - 数を pin する既存の歯: 行数 rules_embedded_manifest_is_valid_and_covers_all_kinds・kind 数 rules_embedded_manifest_declares_one_capability_row_per_role（どちらも `crates/scribe2-boundary/tests/e2e/rules/embedded.rs`）・外形 snapshot rules_external_form（`crates/scribe2-boundary/tests/e2e/rules.rs`・`rows=` と `kinds=` の 2 行）。並びを pin する歯は、末尾からの窓（`crates/scribe2-boundary/tests/e2e/rules.rs` の `.rev()` の窓と `crates/scribe2-boundary/tests/e2e/rules/embedded.rs` の LedgerDeniedWrites から末尾までの列）と前から数える窓（FollowRetries・GateLensCount・PipeCiWaitS などの直後）で、HostBlockedPerCore と PipeLandWaitS の間・`host.blocked_per_core` と `gate.tmux_test_threads` の間に跨る窓は無い。`host.runnable_per_core` / `host.blocked_per_core` の歯は自分の裁定の字の行が 2 本であることを数える（別の字の行を足しても変わらない）。
  - 字 `2026-10-03T02:42Z` を裁定に持つ行は main に 0 本。
- 形（番号は行 b の done と 1:1）:
  1. **rules 行 4 本**: 埋め込み manifest の `host.blocked_per_core` の行の直後（`gate.tmux_test_threads` の前）に、意味と本節を指す注を置いて次の順で足す。どれも形 Int・enabled・裁定日 2026-10-03（裁定の字は表の下）。

     | id | kind | 値 | 意味 |
     |---|---|---|---|
     | `host.write_avg_gb` | HostWriteAvgGb | 1500 | 平均の線（10^9 byte・TBW と同じ十進）。窓の 1 日平均がこれを越えた日は越え |
     | `host.write_day_gb` | HostWriteDayGb | 3000 | 1 日の線（10^9 byte）。1 日の書き込みがこれを越えた日は越え |
     | `host.write_avg_days` | HostWriteAvgDays | 7 | 平均の窓の日数（その日で終わる日数） |
     | `host.write_owner_days` | HostWriteOwnerDays | 3 | 持ち主の段の連続日数（越えた閉じた日がこの数だけ続くと owner=yes） |

     裁定の字は行ごとに項の語で分ける（新しい行は同じ裁定の字を使い回せない・rules-diff の new-row-reuses-ruling）: 上から `user 2026-10-03T02:42Z 項 avg`・`user 2026-10-03T02:42Z 項 day`・`user 2026-10-03T02:42Z 項 window`・`user 2026-10-03T02:42Z 項 owner`。kind は ALL の HostBlockedPerCore の直後（PipeLandWaitS の前）に同じ順で置き、`crates/scribe2/src/rules/mod.rs` の閉じた match に 1 つずつ足す（as_str は字のまま・shape は Int・has_permit_reader は false）。行の id は本行の write-set の `+` の file（fleet の兄弟 module）の const 4 つで持ち、int_row で読む。[rules-manifest.md](./rules-manifest.md) §4.1 の表に 4 行を 1 行で足す。
  2. **判定の 1 本と doctor の行**: 本行の write-set の `+` の file（fleet の兄弟 module・fleet の pub な子 module として宣言する）に判定の 1 本（crate の中から呼べる pub）を置き、§2 の「後の行が呼ぶ口」の 3 つで表と記録の dir と open を読む（行 a の `+` の file は書き換えない）。days.log の読み手は同じ `+` の file に 1 つ置く（引数は記録の dir、返りは閉じた 2 値〔読めない・date ごとの最後の行の値の列〔date の字・written の値か無し・state〕〕・無い file は空の列・§4 形 2 の読み）。引数は置き場・rules の path（Option の字・doctor と hook の `--rules` と同じ）・今（UNIX 秒）で、返りは表の行ごとの判定の値の列（宣言順・欄は name と §6 の 10 key の値の字）。合わせた manifest を読めない周と表が 0 行の周は空の列。doctor の行（§6・同じ `+` の file の pub の関数 1 つ）と席の 1 行（§7・行 c）は、この値だけを読む。
  3. **today・yesterday・sampled・probe**: today は、open が読めて date が今日なら written（state が partial なら `<byte>:partial`）、open が無いか date が今日でないなら unmeasured、open が読めないなら unreadable。yesterday は days.log の昨日の行が measured なら値・partial なら `<byte>:partial`・unmeasured か行が無いなら unmeasured、days.log が読めないなら unreadable。sampled は open が読めれば format_utc(at)、ほかは `-`。probe は open が読めれば probe の語、ほかは `-`。
  4. **avg と days**: 昨日で終わる W 日（W = `host.write_avg_days`）の各日の値（measured と partial の written・unmeasured と行の無い日は値なし）のうち、値の在る日の数を k、値の和を W で割った整数（切り捨て）を平均とする。k = W で全部 measured なら `<平均>`、k ≥ 1 でそれ以外なら `<平均>:partial`（下限）、k = 0 なら unmeasured。days は `<k>/<W>`。days.log が読めない周は avg と days が unreadable。
  5. **over**: 今日か昨日の値（partial の下限を含む）が 1 日の線（10^9 × `host.write_day_gb` byte・乗算は飽和）を越えた（`>`）なら `day`、昨日で終わる平均の値（下限を含む）が平均の線（10^9 × `host.write_avg_gb`）を越えたなら `avg`、両方なら `day,avg`、どちらも無ければ `-`。days.log が読めない周は today だけで判じる。
  6. **streak と owner**: 閉じた日 d の越えは「d の値が在り、d の値が 1 日の線を越えるか、d で終わる W 日の平均（形 4 の数え方）が平均の線を越える」こと。streak は昨日から 1 日ずつ遡り、越えの日が続く数（値の無い日〔行が無い日と unmeasured の日〕・越えない日で止まる。partial の日は下限の値で判じ、下限で越えれば越え）。owner は streak ≥ `host.write_owner_days` なら yes、ほかは no。days.log が読めない周は streak と owner が unreadable。
  7. **線を読めない周**: 4 行のどれかを読めない周（manifest を読めない・行が無い・不発効・整数でない）と W が 0 の周は、avg・days・over・streak・owner・cap を no-rule にし（today・yesterday・sampled・probe は形 3 のまま）、既定の値で埋めない（C5）。どの周も doctor の rc と doctor のほかの行は変わらない。
  8. **閉包**: 本行の `+` の file は、ほかの行の touches に在る型（`crate::fleet::EventKind`・`crate::seat::tick::Judged` など）と RuleKind の variant を literal・match の arm・variant の構築・const slice の形で持たない（RuleKind の variant を足すのは `crates/scribe2/src/rules/mod.rs` だけ）。行の検証行の最後で `contracts check` を撃つ。
- 判定の順（変異の A/B の順）: 面と表 → 線の 4 行 → 記録の読み（open・days.log）→ today / yesterday → 窓の和 → 越え（day → avg）→ 遡りの streak → owner。
- 後の行が呼ぶ口（行 c）: 形 2 の判定の 1 本（本行の `+` の file の pub の関数・引数は置き場と rules の path と今・返りは表の行ごとの値の列）。行 c は over が `-` でない表の行の値を席の 1 行に写す。行 c は本行の `+` の file を書き換えない。
- 歯（置き場は既存の e2e の file・helper は歯と同じ file・lib の新しい口と新しい kind の variant の path を歯から書かない〔kind は RuleKind の parse で字面から引く〕＝base で compile が通る）:
  - e2e（`crates/scribe2-boundary/tests/e2e/seat.rs`・接頭辞 seat_doctor_write_budget_・0 件）。fixture: 歯ごとの tmp の根の下の state（`seat_doctor_external_form` と同じ置き場の形）の `host.toml` に表を書き、host の根の `write-budget/<name>/` に §4 の形の open と days.log を歯が組んで置き（日付は撃つ前の今から組む）、`doctor --state-dir` を撃って `write-budget: ` で始まる行を取る。値は 10^12 byte の単位（1 TB = 1000000000000）。
    - (a) seat_doctor_write_budget_under_the_lines_is_quiet: 表 2 行。nvme-a は昨日まで 7 日が 1 TB の measured・open が今日の measured で written 0.5 TB → 1 行目がちょうど `write-budget: name=nvme-a today=500000000000 yesterday=1000000000000 avg=1000000000000 days=7/7 over=- streak=0 owner=no cap=1500000000000/3000000000000 sampled=<format_utc(at)> probe=ok`。記録の無い nvme-b の 2 行目がちょうど `write-budget: name=nvme-b today=unmeasured yesterday=unmeasured avg=unmeasured days=0/7 over=- streak=0 owner=no cap=1500000000000/3000000000000 sampled=- probe=-`。席の根（seats_root）に drafts-cap の記録（`no-rule` の 1 行）を置き、`--repo` を渡さずに撃つ → doctor の最後の 3 行が `drafts-cap=no-rule`・nvme-a の行・nvme-b の行の順。〔形 2・3・6〕
    - (b) seat_doctor_write_budget_names_the_day_line: 昨日 3.5 TB・ほかの 6 日 1 TB → `avg=1357142857142 days=7/7 over=day streak=1 owner=no`。別の fixture で 7 日とも 1 TB・今日の open が 3.1 TB → `over=day streak=0`。〔形 5 の day〕
    - (c) seat_doctor_write_budget_names_the_average_line: 7 日とも 1.6 TB → `avg=1600000000000 days=7/7 over=avg streak=1 owner=no`（2 日前で終わる窓は 6 日で下限 1371428571428）。〔形 5 の avg・形 6〕
    - (d) seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds: 7 日とも 1 TB で 3 日前だけ partial → `yesterday=1000000000000 avg=1000000000000:partial days=7/7 over=-`。2 日前の行が無く 5 日前が `written=- state=unmeasured`・ほかは 1 TB → `avg=714285714285:partial days=5/7`。昨日から 5 日前まで 2.2 TB・6 日前と 7 日前の行が無い → `avg=1571428571428:partial days=5/7 over=avg streak=1`（下限で越える）。昨日が partial の 1 TB → `yesterday=1000000000000:partial`。open の date が昨日（at は昨日の 12 時・written 0.5 TB）→ `today=unmeasured` で sampled は昨日の 12 時の字。〔形 3・4・5〕
    - (e) seat_doctor_write_budget_counts_the_streak_to_the_owner: 昨日から 3 日前まで 3.5 TB・4 日前から 7 日前まで 1 TB → `over=day,avg streak=3 owner=yes`。昨日と 2 日前だけ 3.5 TB → `streak=2 owner=no`。昨日・3 日前・4 日前が 3.5 TB で 2 日前の行が無い → `streak=1 owner=no`。〔形 6〕
    - (f) seat_doctor_write_budget_says_unreadable_and_no_rule_without_changing_rc: open を dir にした置き場 → `today=unreadable … sampled=- probe=-`。形でない行（5 語）を持つ days.log と今日 3.1 TB の open → `yesterday=unreadable avg=unreadable days=unreadable over=day streak=unreadable owner=unreadable`。`host.write_owner_days` の行を欠く `--rules` の写しと、`host.write_avg_days` を 0 にした写し → どちらも `avg=no-rule days=no-rule over=no-rule streak=no-rule owner=no-rule cap=no-rule`（today と yesterday は値のまま）。どの周も doctor の rc が 0 で、`write-budget: ` の行を除いた stdout が、`host.toml` から表だけを除いた同じ置き場（`host-manifest=present` のまま）の doctor と等しい。〔形 3・7〕
  - e2e（`crates/scribe2-boundary/tests/e2e/rules.rs`・接頭辞 rules_write_detection_・0 件）:
    - (g) rules_write_detection_rows_follow_the_host_health_rows: 形 1 の 4 行の id・kind（字面から parse で引ける）・形 Int・値・enabled・裁定の字（形 1 の項の語の 4 つ・互いに違い base のどの行の字とも違う）と裁定日・int_row の値・kind ごとの行が 1 本・ALL の HostBlockedPerCore の直後に 4 kind がこの順で続き PipeLandWaitS が次・manifest の `host.blocked_per_core` の直後に 4 行がこの順で続き `gate.tmux_test_threads` が次・has_permit_reader が false・文字列の値の写しが形で断られる。〔形 1〕
  - 直す既存の歯: 上の現物の数の pin 3 本（行数と kind 数と snapshot の `rows=` / `kinds=` を本行を撃つ時の main の値 +4 にする・数の字は書かない）。ほかの rules の歯は本文を変えずに緑。
  - 変わらない既存の歯（行の検証行に載せる）: seat_doctor_external_form（表の無い置き場は 0 行で snapshot が動かない）。
  - 変異の A/B（判定の順・条件 1 つに歯 1 本）: 表の行ごとに読まずに 1 つ目の記録を使い回すと (a) の 2 行目が落ちる。today で open の date を見ないと (d) の 5 つ目が落ちる。partial を素の値にすると (d) の 1 つ目が落ちる。値の無い日を 0 として k に数えると (d) の 2 つ目の days が落ちる。下限の越えを越えと数えないと (d) の 3 つ目が落ちる。today を over に入れないと (b) の 2 つ目が落ちる。avg を越えに入れないと (c) が落ちる。streak を値の無い日で止めないと (e) の 3 つ目が落ちる。owner の比較を `>` にすると (e) の 1 つ目が落ちる。線の行の欠けを既定値で埋めると (f) の no-rule が落ちる。W = 0 を割り算に渡すと (f) の 2 つ目の写しが panic で落ちる。
- base で RED の理由（機能不在）: base の doctor は `write-budget: ` の行を出さない＝(a)〜(f) が落ちる。base の埋め込み manifest は 4 行も 4 kind も持たない＝(g) の parse と行の引きが落ちる。直す 3 本は base の行数と kind 数と違うので base で落ちる（retroactive の札は要らない）。

## 6. doctor の行（行 b）

- 置き場: `crates/scribe2-boundary/src/main.rs` の render_doctor_with の `--state-dir` を持つ枝で、drafts_cap_doctor_line の行の後ろに `lines.extend(…)` の 1 行を足し、行 b の `+` の file の doctor の関数（引数は置き場と rules の path）が §5 形 2 の判定の値から表の行ごとに 1 行（宣言順）を返す。表の無い置き場と面を読めない置き場は 0 行で、`seat_doctor_external_form` の snapshot は 1 byte も動かない。新しい verb は作らない（C17）。
- 形（key の順は固定）: `write-budget: name=<name> today=<…> yesterday=<…> avg=<…> days=<…> over=<…> streak=<…> owner=<…> cap=<…> sampled=<…> probe=<…>`

  | key | 値（閉じた形） | 意味 |
  |---|---|---|
  | `name` | 表の name | 表の行 |
  | `today` | `<byte>` / `<byte>:partial` / `unmeasured` / `unreadable` | 今日（UTC）の書き込み |
  | `yesterday` | `<byte>` / `<byte>:partial` / `unmeasured` / `unreadable` | 昨日の閉じた値 |
  | `avg` | `<byte>` / `<byte>:partial` / `unmeasured` / `unreadable` / `no-rule` | 昨日で終わる窓の 1 日平均（`:partial` は下限） |
  | `days` | `<k>/<W>` / `unreadable` / `no-rule` | 窓のうち値の在る日 |
  | `over` | `-` / `day` / `avg` / `day,avg` / `no-rule` | 越えた線 |
  | `streak` | 整数 / `unreadable` / `no-rule` | 昨日から続いた越えの閉じた日の数 |
  | `owner` | `yes` / `no` / `unreadable` / `no-rule` | 持ち主の段（streak ≥ 持ち主の段の連続日数） |
  | `cap` | `<平均の線 byte>/<1 日の線 byte>` / `no-rule` | 線の値 |
  | `sampled` | `YYYY-MM-DDTHH:MM:SSZ` / `-` | 最後に読めた標本の時刻（open の at） |
  | `probe` | `ok` / `unreadable` / `malformed` / `stat-mismatch` / `-` | 最後の読みの結果 |

- 持ち主の段に届いた行は `owner=yes` を持つ。器は持ち主へ直に届ける口を持たず、持ち主向けの札は消費側の板（器の外の表示面）が key の名で読んで出す（tick status の key を読む前例と同じ・text の行は跨版の約束の外）。着地の後に key の表を消費側の席へ渡す。

## 7. 席への 1 行 — 越えた周と線を読めない周だけ、席の UserPromptSubmit の追加文脈に表の行ごとに 1 行を足す（契約表の行 c）

やさしく言うと: 書き込みが線を越えている間（と線の規則の行を読めない間）は、席が prompt を受けるたびに書き込みの検出線の知らせの 1 行を席の文脈に添える。入力欄へは何も送らない。黙った席にも、次の heartbeat の合図の turn で届く。越えていない周・表の無い host・記録を読めず越えの値が無い周は 1 byte も足さない。

- 現物（main f2d14990・verified）:
  - `crates/scribe2/src/hook/mod.rs` の dispatch の UserPromptSubmit の枝は、打刻 → 発話の記帳（utterance）→ 群の段の行（`crates/scribe2/src/hook/group.rs` の lines）の順に stdout の行を足す。群の段は出した行ごとに注入の記録 1 行（同じ mod.rs の私有の record と record_lines・Emit の who / what / when）を残し、読めない面・群に属さない anchor・pane の無い呼び出しは 0 行（席は止めない・rc は変えない）。Hooked は置き場（dir）・`--rules`（rules）・`--pane`（pane）を持つ。
  - 入力欄への差し込みは FR44 が閉じた列（打刻の合図ほか 3 つ）で、追加文脈の行は差し込みではない。hook の予算は NFR5（rules 行 `hook.budget_ms`）。
- 形（番号は行 c の done と 1:1）:
  1. **出す行**: UserPromptSubmit の枝の群の段の行の後ろで、`--pane` が空でない呼び出し（席）だけ、§5 形 2 の判定の 1 本を置き場・`--rules`・今で撃ち、over が `-` でない表の行（`day`・`avg`・`day,avg`・`no-rule`）ごとに 1 行（宣言順）`write-budget: name=<name> over=<…> today=<…> yesterday=<…> avg=<…> streak=<…> owner=<…> — 書き込みの検出線の知らせ（器は作業を止めない・持ち主への札は owner=yes の周に消費側の板が出す）` を stdout へ足し、出した行ごとに注入の記録 1 行（who `hook:user-prompt-submit`・what `write-budget`・when `UserPromptSubmit`）を残す。行の字は `crates/scribe2/src/hook/mod.rs` の私有の関数 1 つが判定の値から組み、同じ mod.rs の record と record_lines で記録する（行 b の `+` の file は書き換えない）。
  2. **出さない周**: over が `-` の表の行・表の無い置き場・`--pane` の無い呼び出しは 0 byte。SessionStart の brief には足さない。
  3. **持ち主の段**: streak が持ち主の段の連続日数に届いた周の行は `owner=yes` を持つ（値は判定の値の写し）。
  4. **読めない周**: 面・記録が読めない周（host の面が壊れている・open が dir で days.log が無い）は、越えの値が無ければ 0 行で、rc 0・stderr 0 byte（prompt を止めない）。
  5. **入力欄へ送らない**: 1 行は追加文脈だけで、tmux の send-keys を撃たない（FR44 の列を増やさない）。
  6. **既存の行**: 表の無い置き場の UserPromptSubmit の出力（発話の行と群の行）と記録は 1 字も変わらない（記録の行数は (b) が表と記録の無い置き場との一致で測る）。
  7. **閉包**: 行 c で足す code は、ほかの行の touches に在る型（`crate::hook::group::Current`・`crate::fleet::EventKind` など）を literal・match の arm・variant の構築・const slice の形で新しく持たない。行の検証行の最後で `contracts check` を撃つ。
- 判定の順: pane → 判定の値（面・記録）→ over の語 → 行と記録。
- 歯（`crates/scribe2-boundary/tests/e2e/hook/group.rs`・接頭辞 hook_write_budget_・0 件・helper は同じ file・置き場は群の歯と同じ tmp の根の下の state〔host の根も歯ごと〕・記録は §4 の形を歯が組んで置く）:
  - (a) hook_write_budget_over_round_adds_one_line_after_the_group_line: 群に属さない anchor の席で、昨日 3.5 TB（ほか 6 日 1 TB）の記録 → UserPromptSubmit（prompt を持たない payload・`--pane` あり）の stdout がちょうど 1 行 `write-budget: name=nvme-a over=day today=… yesterday=3500000000000 avg=1357142857142 streak=1 owner=no — …`・記録の what が `write-budget` の行が 1 本増える・偽 tmux の呼び出しに send-keys が 0 回。逼迫の群の席で同じ記録 → 2 行で 1 行目が群の行・2 行目が書き込みの行。7 日とも 1 TB の記録を `host.write_owner_days` の行を欠く `--rules` の写しで撃つ → 1 行で `over=no-rule`。〔形 1・5〕
  - (b) hook_write_budget_quiet_tableless_or_paneless_rounds_print_nothing: 表を持ち 7 日とも 1 TB の記録で `--pane` あり・表が無く host の根に昨日 3.5 TB の記録だけが在る置き場で `--pane` あり・表を持ち昨日 3.5 TB の記録で `--pane` の無い呼び出しの 3 形がどれも stdout 0 byte で、どの形も注入の記録の file（`inject.jsonl`）に what が `write-budget` の行が 0 本で、記録の行数が、同じ payload と同じ `--pane` を表と記録の無い同じ形の置き場へ撃った周の行数と等しい（空の行も黙った記録も足さない）。表を持ち昨日 3.5 TB の置き場の SessionStart の出力に `write-budget` の字が無い。表を持ち昨日 3.5 TB の記録で `--pane` ありで撃つ → 1 行（対照）。〔形 2〕
  - (c) hook_write_budget_owner_round_carries_owner_yes: 昨日から 3 日前まで 3.5 TB → 行が `streak=3 owner=yes` を持つ。〔形 3〕
  - (d) hook_write_budget_unreadable_round_prints_nothing_and_keeps_rc: open を dir にし days.log の無い置き場 → rc 0・stdout 0 byte・stderr 0 byte。壊れた host の面 → 同じ。同じ置き場で days.log に越えの記録を置く → 1 行（対照）。〔形 4〕
  - 変わらない既存の歯（行の検証行に載せる）: hook_group_user_prompt_submit_adds_one_context_line・hook_utterance_record_line_comes_before_the_group_line。〔形 6〕
  - 変異の A/B: pane を見ないと (b) の 3 形目が落ちる。over の `-` でも出すと (b) の 1 形目が落ちる。no-rule を出さないと (a) の 3 段目が落ちる。群の行の前に置くと (a) の 2 段目が落ちる。記録を残さないと (a) の記録の数が落ちる。 越えない周にも空の行か黙った記録を残すと (b) の記録の行数が落ちる。
- base で RED の理由（機能不在）: base の hook は書き込みの行を出さない＝(a)(c) と (b)(d) の対照が落ちる。

## 8. 却下

- **drive の SMART の書いた量を読む**: root の権限と外部の道具が要り、依存 OSS の追加（A3）に当たる。道具と drive の種類で出力の形が違う。
- **boot_id で再起動を見分ける**: 読む path と記録の欄が 1 つずつ増え、得は 1 標本ぶんの精度だけ（止まる前の最後の読みの後の書き込みはどちらでも欠け、再起動の日はどちらでも partial）。
- **測るだけの専用の timer を足す**: unit の種類が増える（C17）。管理 tick の周で足りる。dispatcher の周に相乗りする案は、便の無い host で周が来ない。常駐の process は FR38 の向きの外。
- **記録を event log へ書く**: 置き場ごとに割れ、15 秒ごとの標本で 1 日 5760 行に肥大する（tick-last を event log に書かない理由と同じ）。
- **越えを席の入力欄へ差し込む**: FR44 の閉じた列を増やす改訂が要る。追加文脈の 1 行で足りる。
- **越えた日に器が台帳へ memo を起こす**: 器が台帳に issue を起こす口は初期化の 1 か所だけで、新しい口になる。台帳は task と裁定だけ（C15）で、実測の記録は器の記録が持つ。
- **`/proc/diskstats` を読む**: 同じ値を装置の名で引く parse が増えるだけ。
- **表に device の名を書き code が `/sys/block/` を組む**: 歯が sysfs を直に読む（§3）。
- **装置の和を取る**: 摩耗の線は drive ごとで、和は線と比べられない。
- **「3 日続けて」を平均の線だけで数える**: 1 日の線を越えた日も知らせる裁定の 2 点目と揃わない。
- **平均の窓に今日を入れる**: 今日は閉じていない（値が時刻で増える）ので、窓は昨日で終わる閉じた日だけにする。
- **1 席だけを測る席にする**: 席の生死に依存する。どの席の周期でも同じ 1 本が lock で排他される方が簡単（[seat-heartbeat.md](./seat-heartbeat.md) §9 の却下と同じ）。
- **lock に群の段の Lock を使う**: 回収を持たず、kill された tick が残した lock で測りが永久に止まる。

## 9. 限界と後続

- 限界:
  - 管理 tick の無い host と、全部の tick が止まっている間は測れない。その日は partial か unmeasured になる。
  - 再起動の後の最初の読みが前の boot の最後の読み以上だと、減りで見分けられず、前の boot の読みの分だけ少なく数える（前の boot がごく短いときだけ起きる）。
  - measured の閉じは境目をまたぐ 2 標本の間（2 × 周期以内）の書き込みを前の日に数える。
  - 時計が戻ると、早抜け（probed + 周期 > 今）が戻った分だけ長く効く。
  - 同じ host の state dir の面が同じ name に違う stat を書くと、置き場ごとの tick が probe を stat-mismatch と ok の間で揺らす（直しは人が面を揃える）。
  - 読み手が days.log の追記の途中を読んだ周は、その 1 周だけ unreadable になる。
  - 器から持ち主へ直に届ける口は無い。持ち主向けの札は消費側の板が `owner=yes` を読んで出すまで出ない（器から届けるには FR44 の改訂と ADR が要る）。
  - 越えが続く間は席の turn ごとに 1 行（約 40 token）を足す。
  - 書いた区の数の意味は Linux の kernel の文書に依る。1 日の境目は UTC。
- 後続:
  - SRS の改訂の round（新しい FR と AC）の後に、3 行の req を差し替えて起票する。ADR-0112 はその後に accepted にする。
  - 着地の後に PATH の binary を入れ替える（tick の unit と hook は PATH の binary を撃つ・入れ替えは swap-binary.sh）。
  - 行 b の着地の後に、§6 の key の表を消費側の席へ渡す。
  - host の面の雛形（[host-init.md](./host-init.md)）に表の例を足すかは別の周。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "書き込みの検出線の測り — host の面の表 [[write-budget]]（name と stat）が名指す装置の stat file の書いた区の数を、管理 tick の判定の後に表の行ごとに host の根の記録（open と days.log・schema=1）へ lock の内で 1 回だけ進め、測れない日は unmeasured・欠けた日は partial と書き 0 と書かない（§2・ADR-0112）"
req = ["FR113", "FR27", "FR57", "AC90", "NFR4"]
section = "2"
write-set = ["+crates/scribe2/src/rules/write_budget.rs", "crates/scribe2/src/rules/manifest.rs", "crates/scribe2/src/rules/mod.rs", "+crates/scribe2/src/fleet/write_budget.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/tick.rs", "crates/scribe2-boundary/tests/e2e/rules/host.rs", "crates/scribe2-boundary/tests/e2e/seat/tick.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_write_budget_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_host_write_budget_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_tick_status_tick_last_mirrors_the_judgement_line", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "L"
growth = ["crates/scribe2/src/rules/write_budget.rs:120", "crates/scribe2/src/rules/manifest.rs:25", "crates/scribe2/src/rules/mod.rs:2", "crates/scribe2/src/fleet/write_budget.rs:340", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/seat/mod.rs:6", "crates/scribe2/src/seat/tick.rs:3"]
done = "(1) host の面の表 [[write-budget]] は 0〜n 行で key は name と stat の 2 つ（両方必須）、tracked の面に置いた表は 1 表 1 件で §3 の字で断り、host の面の行は stat の欠け・未知の key・name が英数字と - と _ の 1 字以上でない・stat が絶対 path でない・stat が空白を含む・name の重複（2 度目の見出しの行番号）を §3 の字と行番号で 1 件ずつ断り、rules validate --state-dir の 1 行は表の無い面と同じ字で、合わせた manifest が宣言順の行を rules の Manifest の pub な口で返す〔rules_host_write_budget_ の 3 本と seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors の 2 行の表〕 (2) 管理 tick の run は full_rewrite の後の同じ枝で標本の 1 本を呼んで返りを捨て、標本の 1 本は stdout と stderr に書かず、判定行・rc・stderr・tick-last の字は表の無い置き場の同じ周と同じ〔seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors の対照・変わらない seat_tick_status_tick_last_mirrors_the_judgement_line〕 (3) 登録 row の無い target・合わせた manifest を読めない周・表が 0 行の周は host の根に write-budget の dir を作らず、seat.tick_interval_s と fleet.lock_retry_ms と fleet.lock_stale_ms のどれかを読めない周は記録を書かない〔seat_tick_write_budget_writes_nothing_outside_a_registered_seat_with_a_table〕 (4) 記録は表の行ごとに host の根の write-budget/<name>/（crates/scribe2/src/seat/mod.rs に足す pub の関数 1 つが返す dir）の open（§4 の 10 語の 1 行・open.tmp から rename）と days.log（§4 の 6 語の追記）と lock file open.lock（fleet の store の acquire）で、行ごとに独立に進めて和を取らない〔seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors〕 (5) 記録の無い周は今日を partial・written 0 で開き、同じ UTC 日の読みが前の読み以上なら差 × 512 を足す〔seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors〕 (6) 同じ日の読みが前の読みより小さい周は読み × 512 を足し reboots を 1 増やし state を partial にする〔seat_tick_write_budget_counts_a_reboot_inside_the_day〕 (7) 読めた記録が今と同じ UTC 日で probed + seat.tick_interval_s > 今 の周は lock の外と内の 2 か所が呼ぶ同じ pure な関数で早抜けして何も書かない〔seat_tick_write_budget_skips_a_round_inside_the_period〕 (8) 日付が進んだ周は、読みが前の読み以上 ∧ 次の日 ∧ 今 − at ≤ 2 × 周期 のときだけ前の日を written + 差 × 512・前の state・前の reboots・tail - で days.log へ閉じ、今日を measured・0・reboots 0 で開く〔seat_tick_write_budget_closes_the_day_measured_within_two_periods〕 (9) 3 条件のどれかを欠く周は前の日を written のまま partial・tail = 差 × 512（減った周は読み × 512）で閉じ、間の日を written=- state=unmeasured reboots=0 tail=- の行で足し、今日を partial・0 で開く（今日の reboots は減った周だけ 1）〔seat_tick_write_budget_closes_the_day_partial_after_a_reboot・seat_tick_write_budget_closes_the_day_partial_after_a_late_sample・seat_tick_write_budget_closes_a_gap_with_unmeasured_days〕 (10) 記録の stat が表の stat と違う周は probe=stat-mismatch、stat file を読めない周は probe=unreadable、7 欄に満たないか 7 欄目が 10 進でない周は probe=malformed で、どれも数を変えずに probed と probe だけを書き換え、記録の無い周に読めないときは何も書かない〔seat_tick_write_budget_names_an_unreadable_or_malformed_stat・seat_tick_write_budget_refuses_a_changed_stat_path〕 (11) 記録は open の読み手 1 本（引数は記録の dir・返りは閉じた 3 値〔無い・読めない・読めた 10 欄の値〕・fleet の中から見える pub(super)・fleet の子 module の宣言は pub mod）で読み、lock を取れない周と open が在るのに読めない周は記録を 1 byte も変えない（days.log の読み手は本行に置かない）〔seat_tick_write_budget_leaves_the_record_while_the_lock_is_held_or_the_record_is_unreadable〕 (12) 行 a の write-set の + の file 2 つはほかの行の touches に在る型を literal・match の arm・variant の構築・const slice の形で持たず、検証行の最後の contracts check が便の木で findings 0 歯は base で RED（機能不在: base は [[write-budget]] を未知の section として断り、表を持つ置き場の tick は記録を書かない・歯は lib の新しい口を呼ばないので base でも compile が通る）"
done-teeth = ["1:rules_host_write_budget_two_rows_validate_like_a_tableless_face", "1:rules_host_write_budget_refuses_each_defect_once_with_its_line", "1:rules_host_write_budget_table_on_the_tracked_face_is_refused_once_per_table", "1:seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors", "2:seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors", "2:=seat_tick_status_tick_last_mirrors_the_judgement_line", "3:seat_tick_write_budget_writes_nothing_outside_a_registered_seat_with_a_table", "4:seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors", "5:seat_tick_write_budget_opens_a_partial_day_then_adds_the_sectors", "6:seat_tick_write_budget_counts_a_reboot_inside_the_day", "7:seat_tick_write_budget_skips_a_round_inside_the_period", "8:seat_tick_write_budget_closes_the_day_measured_within_two_periods", "9:seat_tick_write_budget_closes_the_day_partial_after_a_reboot", "9:seat_tick_write_budget_closes_the_day_partial_after_a_late_sample", "9:seat_tick_write_budget_closes_a_gap_with_unmeasured_days", "10:seat_tick_write_budget_names_an_unreadable_or_malformed_stat", "10:seat_tick_write_budget_refuses_a_changed_stat_path", "11:seat_tick_write_budget_leaves_the_record_while_the_lock_is_held_or_the_record_is_unreadable", "12:!closure"]

[[contract]]
id = "b"
title = "書き込みの検出線の判定と doctor の行 — rules 行 4 本（host.write_avg_gb 1500・host.write_day_gb 3000・host.write_avg_days 7・host.write_owner_days 3・裁定 user 2026-10-03T02:42Z を行ごとに項の語で分ける）を足し、行 a の記録から today・yesterday・昨日で終わる窓の平均（下限は :partial）・越え・続いた越えの日数・持ち主の段を判定の 1 本で求め、doctor に表の行ごとの write-budget: の 1 行を出す（§5・§6・ADR-0112）"
req = ["FR113", "AC90", "NFR4"]
section = "5"
depends = ["a"]
touches = ["crate::rules::RuleKind"]
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "+crates/scribe2/src/fleet/write_detection.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2-boundary/src/main.rs", "docs/design/rules-manifest.md", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2-boundary/tests/e2e/seat.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_doctor_write_budget_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_write_detection_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail seat_doctor_external_form", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/fleet/write_detection.rs:340", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/rules/mod.rs:20", "crates/scribe2-boundary/src/main.rs:1"]
done = "(1) 埋め込み manifest の host.blocked_per_core の行の直後（gate.tmux_test_threads の前）に host.write_avg_gb（HostWriteAvgGb・1500）・host.write_day_gb（HostWriteDayGb・3000）・host.write_avg_days（HostWriteAvgDays・7）・host.write_owner_days（HostWriteOwnerDays・3）がこの順で在り、どれも形 Int・enabled・裁定日 2026-10-03 で、裁定の字は上から user 2026-10-03T02:42Z 項 avg・項 day・項 window・項 owner（互いに違い base のどの行の字とも違う）、kind は ALL の HostBlockedPerCore の直後（PipeLandWaitS の前）に同じ順で字面から引け、has_permit_reader が false、int_row で値を引け、文字列の値の写しは形で断られ、行数と kind 数と rules_external_form の rows= と kinds= は本行を撃つ時の main の値 +4〔rules_write_detection_rows_follow_the_host_health_rows と直す rules_embedded_manifest_is_valid_and_covers_all_kinds・rules_embedded_manifest_declares_one_capability_row_per_role・rules_external_form〕 (2) 本行の + の file（fleet の兄弟 module・行 a の + の file の表と記録の dir と open の読み手を呼び、days.log の読み手は本行の + の file が持ち、行 a の file は書き換えない）の判定の 1 本（置き場・rules の path・今を受け、表の行ごとの値の列を宣言順に返し、面を読めない周と表 0 行の周は空）から、doctor --state-dir が drafts-cap の行の後ろ（--repo の無い周は doctor の最後）に表の行ごとに write-budget: name= today= yesterday= avg= days= over= streak= owner= cap= sampled= probe= の順の 1 行を出し、表の無い置き場は 0 行〔seat_doctor_write_budget_under_the_lines_is_quiet・変わらない seat_doctor_external_form〕 (3) today は open の date が今日なら written か <byte>:partial・無いか今日でないなら unmeasured・読めないなら unreadable、yesterday は昨日の行の measured の値か <byte>:partial か unmeasured（行が無い日を含む）か days.log を読めない周の unreadable、sampled は format_utc(at) か -、probe は open の語か -〔seat_doctor_write_budget_under_the_lines_is_quiet・seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds・seat_doctor_write_budget_says_unreadable_and_no_rule_without_changing_rc〕 (4) avg は昨日で終わる host.write_avg_days 日の値の在る日の和を日数で割った切り捨てで、全日 measured なら素の値・それ以外で値の在る日が 1 日以上なら :partial・0 日なら unmeasured、days は <値の在る日>/<窓>〔seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds〕 (5) over は今日か昨日の値（下限を含む）が 10^9 × host.write_day_gb を越えれば day、昨日で終わる平均（下限を含む）が 10^9 × host.write_avg_gb を越えれば avg、両方は day,avg、どちらも無ければ -〔seat_doctor_write_budget_names_the_day_line・seat_doctor_write_budget_names_the_average_line・seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds〕 (6) streak は昨日から遡り、値が在って 1 日の線かその日で終わる窓の平均の線を越えた閉じた日が続く数（値の無い日と越えない日で止まる）で、owner は streak ≥ host.write_owner_days なら yes・ほかは no〔seat_doctor_write_budget_counts_the_streak_to_the_owner〕 (7) 4 行のどれかを読めない周と窓が 0 の周は avg・days・over・streak・owner・cap が no-rule、days.log を読めない周は yesterday・avg・days・streak・owner が unreadable で over は今日だけで判じ、どの周も doctor の rc は 0 のままで write-budget: の行を除いた stdout は host.toml から表だけを除いた同じ置き場と等しい〔seat_doctor_write_budget_says_unreadable_and_no_rule_without_changing_rc〕 (8) 本行の + の file はほかの行の touches に在る型を literal・match の arm・variant の構築・const slice の形で持たず（RuleKind の variant は crates/scribe2/src/rules/mod.rs だけ）、検証行の最後の contracts check が便の木で findings 0 歯は base で RED（機能不在: base の doctor は write-budget: の行を出さず、埋め込み manifest は 4 行も 4 kind も持たない・歯は kind を字面から parse で引くので base でも compile が通る・直す 3 本は base の数と違うので retroactive の札は要らない）"
done-teeth = ["1:rules_write_detection_rows_follow_the_host_health_rows", "1:rules_embedded_manifest_is_valid_and_covers_all_kinds", "1:rules_embedded_manifest_declares_one_capability_row_per_role", "1:rules_external_form", "2:seat_doctor_write_budget_under_the_lines_is_quiet", "2:=seat_doctor_external_form", "3:seat_doctor_write_budget_under_the_lines_is_quiet", "3:seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds", "3:seat_doctor_write_budget_says_unreadable_and_no_rule_without_changing_rc", "4:seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds", "5:seat_doctor_write_budget_names_the_day_line", "5:seat_doctor_write_budget_names_the_average_line", "5:seat_doctor_write_budget_reads_partial_and_missing_days_as_lower_bounds", "6:seat_doctor_write_budget_counts_the_streak_to_the_owner", "7:seat_doctor_write_budget_says_unreadable_and_no_rule_without_changing_rc", "8:!closure"]

[[contract]]
id = "c"
title = "書き込みの検出線の席の 1 行 — UserPromptSubmit で --pane を持つ席の hook が、群の段の行の後ろに、判定の 1 本の over が - でない表の行ごとに write-budget: の 1 行を追加文脈へ足して注入の記録を残し、over が - の周・表の無い host・読めず越えの値が無い周・pane の無い呼び出しは 0 byte で、入力欄へは送らない（§7・ADR-0112）"
req = ["FR113", "AC90", "FR44", "NFR5"]
section = "7"
depends = ["b"]
write-set = ["crates/scribe2/src/hook/mod.rs", "crates/scribe2-boundary/tests/e2e/hook/group.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_write_budget_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_group_user_prompt_submit_adds_one_context_line", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail hook_utterance_record_line_comes_before_the_group_line", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
growth = ["crates/scribe2/src/hook/mod.rs:30"]
done = "(1) UserPromptSubmit の枝の群の段の行の後ろで、--pane が空でない呼び出しだけ行 b の判定の 1 本を置き場・--rules・今で撃ち（行 b の + の file は書き換えず、行の字は hook/mod.rs の私有の関数 1 つが組む）、over が - でない表の行（day・avg・day,avg・no-rule）ごとに write-budget: name= over= today= yesterday= avg= streak= owner= の順の値と §7 の定型の文を持つ 1 行を宣言順に stdout へ足し、出した行ごとに注入の記録 1 行（who hook:user-prompt-submit・what write-budget・when UserPromptSubmit）を残す〔hook_write_budget_over_round_adds_one_line_after_the_group_line〕 (2) over が - の表の行・表の無い置き場・--pane の無い呼び出しは stdout 0 byte で、注入の記録（inject.jsonl）に what が write-budget の行を足さず記録の行数が表と記録の無い同じ形の置き場の周と等しく、SessionStart には足さない〔hook_write_budget_quiet_tableless_or_paneless_rounds_print_nothing〕 (3) streak が host.write_owner_days に届いた周の行は owner=yes を持つ〔hook_write_budget_owner_round_carries_owner_yes〕 (4) 面や記録を読めず越えの値が無い周は 0 行で rc 0・stderr 0 byte〔hook_write_budget_unreadable_round_prints_nothing_and_keeps_rc〕 (5) 1 行は追加文脈だけで tmux の send-keys を撃たない〔hook_write_budget_over_round_adds_one_line_after_the_group_line〕 (6) 表の無い置き場の UserPromptSubmit の出力（発話の行と群の行）と記録は変わらない〔hook_write_budget_quiet_tableless_or_paneless_rounds_print_nothing の表の無い形の記録の行数・変わらない hook_group_user_prompt_submit_adds_one_context_line・hook_utterance_record_line_comes_before_the_group_line〕 (7) 本行が足す code はほかの行の touches に在る型を literal・match の arm・variant の構築・const slice の形で新しく持たず、検証行の最後の contracts check が便の木で findings 0 歯は base で RED（機能不在: base の hook は書き込みの行を出さない＝(1)(3) と (2)(4) の対照が落ちる）"
done-teeth = ["1:hook_write_budget_over_round_adds_one_line_after_the_group_line", "2:hook_write_budget_quiet_tableless_or_paneless_rounds_print_nothing", "3:hook_write_budget_owner_round_carries_owner_yes", "4:hook_write_budget_unreadable_round_prints_nothing_and_keeps_rc", "5:hook_write_budget_over_round_adds_one_line_after_the_group_line", "6:hook_write_budget_quiet_tableless_or_paneless_rounds_print_nothing", "6:=hook_group_user_prompt_submit_adds_one_context_line", "6:=hook_utterance_record_line_comes_before_the_group_line", "7:!closure"]
<!-- contracts:end -->
