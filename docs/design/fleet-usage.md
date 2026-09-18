# 設計: 口座残量の計測 — host の HTTP client を子 process で呼び、出所付きの実測行を event log に追記する

- 要件: [FR33](../../design-intent/spec/srs.html#FR33) 口座残量の計測 / [AC11](../../design-intent/spec/srs.html#AC11) / [FR22](../../design-intent/spec/srs.html#FR22) 人由来 0 件（不変）/ [NFR3](../../design-intent/spec/srs.html#NFR3) 依存 0 本 / [NFR4](../../design-intent/spec/srs.html#NFR4) fail-closed。制約: CON2（PUBLIC）
- 憲法: [C3](../../design-intent/spec/constitution.html#c3) fleet の状態は 1 file / [C9](../../design-intent/spec/constitution.html#c9) C9.2 口座は窓の終わりまで使う / [C10](../../design-intent/spec/constitution.html#c10) Measured + Provenance・C10.2 host 固有の値は manifest だけ / [C11](../../design-intent/spec/constitution.html#c11) C11.2 失敗は極性付きの enum / [C13](../../design-intent/spec/constitution.html#c13) 依存予算
- 決定: [ADR-0017](../../design-intent/decisions/ADR-0017-account-allowance-measured-into-event-log.html)（記録先 = event log・手段 = host の HTTP client・口座は不透明 label・上限 record は合図のまま）/ [ADR-0004](../../design-intent/decisions/ADR-0004-mvp-persistence-and-cross-version-formats.html) §2.1 / §2.4 / §2.5 / [ADR-0012](../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html)（不変）
- 土台: [fleet-event-log.md](./fleet-event-log.md)（store・replay・CLI の既存形）。crate の形は [rules-manifest.md §2](./rules-manifest.md)。
- この設計から出る契約: §8（3 便・順序あり）。

## 1. 何を解くか

席が口座を選ぶとき、口座の残り（5 時間窓・7 日窓・モデル別 7 日窓の使用率と reset 時刻）を器の中の記録から読めるようにする。器は口座の endpoint を **host の HTTP client を子 process として**呼んで読み、**出所付きの実測行**として fleet の event log に追記し、1 コマンドで口座ごとに 1 行を見せる。読めない回は「測れなかった + 理由」の行にする（0 に読み替えない）。

やさしく言うと: 「あとどれだけ使えるか」を器が自分で聞きに行って、聞いた時刻と口座と聞き先を添えて台帳に 1 行ずつ残す。聞けなかったときは「聞けなかった・なぜ」を残す。

本設計は **見る + 記録** まで。止める判断は上限 record（ADR-0012）の経路のまま、選ぶ判断は v3（R-C9-1 は `enabled = false` のまま）。

## 2. 口座の列挙（manifest）と credential の場所

- manifest（`rules/manifest.toml`・schema 1 のまま）に **`[[account]]` 行**を足す。field は `label`（文字列・必須・不透明）だけ。同じ label の重複・未知 key は loader が拒む。`[[rule]]` 行の形と検査（裁定 id 必須）は変えない（account 行は規則の値ではなく宣言値・C10。ADR-0004 D-3 の受理する表の列挙を `[[account]]` へ広げる = ADR-0017 §2.3・C14.2 の参照要件は `[[rule]]` 行のまま）。
- **label は不透明**（本当の口座の識別子・host 名・path のどれでもない・CON2）。label 行を足す変更は公開面の情報（口座の数）を増やすので、A1 の対話面で user に確認してから行う。本設計は行を足さない。
- credential の場所 = `<state_dir>/accounts/<label>/`（dir または link・**user が host ごとに置く**〔置く口 = `account add`・宣言の置き場 = host の manifest・[account-lifecycle.md](./account-lifecycle.md) §2 / §3・ADR-0026〕）。中の `.credentials.json`（Claude Code の私有形式）から `claudeAiOauth.accessToken` と `claudeAiOauth.expiresAt` だけを読む。他の field（refresh token 等）は読まない・保持しない・出さない。
- state dir は ADR-0004 §2.4 の経路（`--state-dir`・無ければ `seat` と同じ 1 関数で git 設定から解く・§11）。HOME も env も読まない。
- 器は `<state_dir>/accounts/` を**走査しない**（真実は manifest の宣言・C3）。label に dir が無い host ではその口座を「測れなかった（credential 不在）」として記録し、コマンドは続く。

## 3. 計測（子 process と応答）

- HTTP client = `curl`（host の道具・git / claude / tmux と同種）。器の外の OSS の採用なので A3 の対象で、承認 = user 裁定 2026-09-12（ADR-0017 §2.2）。C13 の crate 手続きは対象なし。実行 file は `--curl PATH` で差し替え可（既定は `curl`・PATH 解決は子 process 起動側）。
- 起動形: `curl -sS -K - --max-time <rules 行の秒> -o - -w '\n%{http_code}' <URL>`。**token は stdin の設定（`-K -`）で渡す**: `header = "Authorization: Bearer <token>"` の行（と endpoint が要する固定 header）を stdin へ書き、閉じる。argv に token を載せない（`ps` に見えるため）。
- URL は code の定数 1 つ（host 固有の値ではない・manifest に置かない）。endpoint の名（`endpoint` field に書く出所）は同じ定数から導く短い識別子。
- 応答: stdout の末尾 1 行が HTTP status、その前が本文。status が 200 以外・rc 非 0・timeout は Unmeasured。本文は入れ子の JSON。実物の形の要旨（field 名と型・s2-07l.187 の実測）:
  - `five_hour` / `seven_day` = `{utilization: 数（**すでに % の値**・`2.0` = 2%）, resets_at: 文字列}`。
  - `limits[]` の要素 = `{kind: 文字列, group, percent: 整数（% の値）, severity, resets_at: 文字列, scope: {model: {id, display_name: 文字列}}, is_active: 真偽}`——**`utilization` を持たない**。
  - `resets_at` は `+00:00` 形（小数秒つき）と `Z` 形の両方が現れる。
  - **消費の無い窓は `{utilization: 0.0, resets_at: null}`**（admin 実測 2026-09-13・3 口座で同形・使い始めた周から文字列になる）。この形は「測れた 0%・reset 未定」の `AllowanceMeasured`（`resets_at` 無し）に写す（ADR-0024 §2.1）。**`utilization` が 0 でなく `resets_at` が null の応答は ShapeMismatch のまま**（0 以外を reset 無しで記録しない）。`limits[]` の要素の `resets_at` には掛けない。
- JSON reader: `json_lite` を **入れ子 object・配列・数（小数含む）** へ広げる（std のみ）。event log の flat な行の書き手 / 読み手（`Value` の 4 値）は**変えない**（別の型 `Tree` を足す。flat 行の受理は狭いまま＝綴り違いの key を拒む性質を保つ）。
- 窓の対応: `five_hour` → `window = "five_hour"`、`seven_day` → `"seven_day"`、`limits[]` のうち `kind == "weekly_scoped"` の要素 → `window = "seven_day_model"` + `model = <scope.model.display_name>`（**display_name でしか結べない**・`id` は null の実測）。要素が 0 件なら model 行は出さない（Unmeasured ではない・窓が無いだけ）。要素が `display_name` を持たない・型が違うなら**その要素だけ** Unmeasured（理由 = 形が違う）。
- 使用率は窓の `utilization`・`limits[]` の要素の `percent`（どちらも % の値・×100 しない・要素の `utilization` は読まない）を **整数 %（切り捨て・100 で cap しない**＝超過をそのまま残す・負数と数でない値は形が違う）に、reset は `resets_at` を UTC `YYYY-MM-DDTHH:MM:SSZ` に正規化。parse 不能なら Unmeasured（理由 = 形が違う）。
- 待ち時間の上限 = rules 行 `fleet.usage_timeout_s`（新 kind `UsageTimeoutS`・値は user 裁定の id 付き・C5）。契約 (b) で足す。
- **token の refresh（s2-07l.229・契約 (d)）**: credential の `expiresAt` が過ぎた口座（`TokenExpired`）は、そのままでは永久に測れず選ばれない——測れない口座は選ばれず（[account-autonomy.md](./account-autonomy.md) §3）、選ばれない口座では Claude Code が起動されないので refresh されない（使っていない口座ほど切れる・実測 2026-09-13: 4 便の枠が 2 便に縮んだ）。器は credential file を書かない（ADR-0017 §2.5 の fence は不変）が、**その口座の設定 dir で claude を 1 回起こして refresh を Claude Code にさせ、直後に credential を読み直して測る**（書き手は Claude Code のまま・器は起動と読み直しだけ）。
  - 形: headless の唯一の構築点 `headless::build`（`Call`・`--setting-sources ""`・`--strict-mcp-config`・permission mode 明示）で起こす。`account_dir` = `<state_dir>/accounts/<label>`・cwd = state dir（repo ではない・`-p` は trust dialog を出さない）・prompt は code の定数 1 語（stdin から）・`--max-turns 1`（`Call` に項目を 1 つ足す・runner / lens は渡さない）・streaming なし・plugin なし。実行 file は `--claude PATH`（headless と同じ seam・歯は偽 claude）。待ち時間の上限は既存の rules 行 `fleet.usage_timeout_s` を共用する（新しい rules 行を足さない・C5 非該当）。
  - **1 口座 1 command につき 1 回だけ**（loop しない）。読み直して `expiresAt` がなお過ぎている・子の rc 非 0・timeout・起こせない、の周は従来どおり `token_expired` の Unmeasured（`UnmeasuredReason` の語彙は増やさない）で、その口座の stdout の行の末尾に `refresh=<ok|rc:<n>|timeout|unlaunchable>` を足す（refresh を試みた周だけ・event log の行には載せない＝schema 1 不変・外形 snapshot は fixture で pin）。墓標（`expiresAt == 0`）と token 不在には掛けない（再 login は user の手番・ADR-0017 §2.5 の `Tombstone` のまま）。
  - なぜ `-p` の起動か: Claude Code に refresh だけを撃たせる口は無い（`claude auth` は `login` / `logout` / `status` のみ・実測 2026-09-13・CLI 2.1.270）。refresh の実体は token endpoint への交換で model 呼出ではないが、それに届く command として実測で確かめられているのは `-p` の起動だけ（planner 2026-09-13・3 口座で measured に戻った）。model 呼出の無い command で届くことが版で確かめられたら差し替える（§10）。
  - A1（使う）の判定: 定額の口座で 1 語 1 回・token の期限ごと（数時間に 1 回・口座あたり）は、便が枠を使い切る弾である設計（ADR-0020）の下で新しい消費の種類を増やさない＝**A1 非該当**（planner 判定 2026-09-13・記帳は bead s2-07l.229 notes）。従量の口座を manifest に足す周は前提が変わるので、その周に判定し直す。

## 4. event の追加（schema 1 のまま）

`EventKind` に 2 variant を**末尾**に足す（C2 宣言順・ADR-0013 §2.2 の判別子順 pin の内側）。

| variant | 意味 | 必須 field | 任意 field |
|---|---|---|---|
| `AllowanceMeasured` | 1 口座 1 窓の実測 | `account` `window` `endpoint` `used_pct`(u64) | `resets_at`（無し = 消費の無い窓・reset 未定・`used_pct` = 0 の周に限る・ADR-0024 §2.1 / §2.3）`model`（`seven_day_model` のとき必須） |
| `AllowanceUnmeasured` | 読めなかった | `account` `endpoint` `reason` | `window` `model`（要素単位の失敗のとき） |

- 共通 field（`schema` `ts` `kind` `host` `actor`）は既存どおり。`actor` は `machine`。
- **`run` / `bead` を任意 field に緩める**（既存の行はすべて読める＝D-5 の「同じ schema 番号」・ADR-0016 §2.2 と同じ論法）。既存 8 + 2 kind の行では引き続き必須（kind ごとの必須 field を `EventKind` の網羅 match で検査する）。
- `KNOWN_KEYS` に `account` `window` `model` `endpoint` `used_pct` `resets_at` `reason` を足す。`Unmeasured` 行に `used_pct` が在れば malformed（0 の捏造を構造で拒む）。
- `reason` = `pub enum UnmeasuredReason { NoCredentials, NoToken, Tombstone, TokenExpired, ClientMissing, ClientFailed, HttpStatus, Timeout, BodyUnreadable, ShapeMismatch }`（閉じた enum・字面変換は wildcard 無しの match・**極性 FailOpen**・§6・極性一覧には載せない）。
- replay: `State` に `allowance: BTreeMap<(account, window), AllowanceLatest>` を足す（口座 × 窓ごとの物理順で最後の行・Measured / Unmeasured のどちらでも最新が勝つ＝古い実測値を新しい「測れなかった」が覆う）。runs / seats は触らない（allowance 行は run を作らない）。
- `fleet export`（跨版 面 2）は**変えない**（header の件数・run 行・seat 行のまま）。allowance の外形は §5 の 1 行表示。

## 5. CLI（`<NAME> fleet …`・置き場は `--state-dir D` か git 設定〔§11〕・出力は emit 経由）

- `fleet usage [--curl PATH]` → manifest の全 `[[account]]` を順に読み、口座 × 窓ごとに event を append し、**口座ごとに 1 行**を stdout へ:
  - 全窓 Measured: `usage: account=<label> five_hour=<pct>% resets=<ts> seven_day=<pct>% resets=<ts> [model=<name>:<pct>% resets=<ts> …]`
  - 口座単位で Unmeasured: `usage: account=<label> unmeasured reason=<Reason>`
  - rc 0 = 全口座を処理した（Unmeasured を含む・「測れなかった」は失敗ではない）。rc 1 = 引数・manifest（account 行なし・重複）の誤り。rc 2 = store が書けない。
- `fleet usage --show` → append せず replay の `allowance` を同じ 1 行形で出す（read-only・lock を取らない）。
- token・credential の中身は stdout / stderr / event に**出さない**。

## 6. 失敗の型と極性

- 境界ごとの enum は 2 つ（C11.2）: `UsageError`（引数 / manifest / store の誤り・command を止める・rc 1 / 2）= **FailClosed**。`UnmeasuredReason`（口座 × 窓の読みの失敗・行として記録して**続行**・rc に出ない）= **FailOpen**（器の定義「測れない周は通す側へ倒し記録を残す」・cap guard と同型）。各 enum の隣に `pub const POLARITY` を置く。
- **Guard は足さない**: 計測は行為（編集・起動・merge・書込）を止めうる判定ではないので polarity.rs の guard に当たらず、極性一覧（C16.2 の母集団）は変えない。「0 に読み替えない」は極性でなく行の構造（Unmeasured 行に `used_pct` があれば malformed）で守る。

## 7. 歯（契約ごとの「base で RED」・`crates/<NAME>/tests/e2e/fleet.rs` に `fleet_usage_` 接頭辞・名前の列は現物が SSOT）

何を測るか（契約 (a) json reader）: 入れ子 object / 配列 / 小数 / 負数 / 深さ 1 を超える path の取り出しが round-trip する／flat 行の reader は入れ子を**拒み続ける**（既存の歯が緑のまま + 入れ子を与えて Err）／制御文字と escape。

何を測るか（契約 (b) event + manifest + rules 行）: `[[account]]` 行の読み（0 行・重複・label 以外の key を拒む）／`AllowanceMeasured` / `Unmeasured` の append と replay（口座 × 窓の最新が勝つ・runs / seats に影響 0）／`Unmeasured` 行に `used_pct` が在れば malformed／`run` / `bead` 無しの既存 kind は malformed のまま／`KINDS` の判別子順 pin が 12 variant で通る／極性一覧 snapshot は不変（guard を足さない）・`UnmeasuredReason::POLARITY` が FailOpen・`UsageError::POLARITY` が FailClosed／rules 行 `fleet.usage_timeout_s` を manifest から読む（kind 件数の歯 + 外形 snapshot）。

何を測るか（契約 (c) `fleet usage`）: 偽の HTTP client（Rust の test helper binary・`--curl` で差す・stdin の設定に token が来ること・argv に token が無いことを検査し、fixture の本文と status を返す）で、live 2 口座相当の fixture から口座ごと 1 行 + 窓ごとの event が増える／status 500・timeout・本文 parse 不能・`display_name` 欠落が**それぞれ別の理由**の Unmeasured になる／credential 不在の label が `NoCredentials` の 1 行になり他の口座の読みは続く／`expiresAt` 0 が `Tombstone`／`--show` は append しない（file の size・mtime 同一）／stdout に token の字面が 0 回。AC11（live 2 口座・器の外の独立した手段との一致）は host での実演（D）で bead notes に記録する（歯にしない・CON2）。

## 8. 契約（3 便・この順）

- **(a)** `json_lite` の入れ子 reader（`Tree`）— core の中だけ・依存 0・event log の flat reader は不変。base で RED = 入れ子の parse の歯（機能不在）。
- **(b)**（前提: rules 行 `fleet.usage_timeout_s` の値の user 裁定が先に在ること・C5。裁定前に起票しない）manifest `[[account]]` 行 + `EventKind` 2 variant + `UnmeasuredReason` + `run` / `bead` 任意化 + replay の `allowance` + rules 行 `fleet.usage_timeout_s`（kind `UsageTimeoutS`・裁定 id）+ 外形 snapshot の更新（極性一覧は不変・§6）。write-set は構造の連鎖（manifest + `rules/mod.rs` + rules の外形 snapshot + kind 件数の歯 + `fleet/mod.rs`・`polarity` は触らない）。
- **(c)** `fleet usage` / `--show` + 子 process 起動 + credential 読み + 偽 client の test helper。(a) (b) に依存。
- AC11 の実演は (c) の land 後に planner が host で行い、値の一致を bead notes に逐語で記帳する。
- **(d) token の refresh**（S・s2-07l.229・§3 末尾）: `read_account` の `TokenExpired` の分岐で refresh の子 process を 1 回起こし、credential を読み直して測る。`Call` の `max_turns` 項目・stdout の `refresh=` 部・偽 claude（credential を書き換える stub と書き換えない stub）。write-set = `fleet/usage.rs`・`headless/mod.rs`（`Call` の項目 1 つと `build` の argv・runner / lens の argv は不変）・`tests/e2e/fleet.rs`・fleet usage の外形 snapshot（`refresh=` が足される周の fixture）。依存: なし。base で RED = 期限切れの credential + credential を書き換える偽 claude で行が measured になる歯（機能不在）・書き換えない偽 claude（rc 0）の周は `token_expired` + `refresh=ok`・rc 非 0 の周は `refresh=rc:<n>`・fresh な credential の周は偽 claude が起こされない（argv の写しが無い）歯。

## 9. 却下案（ADR-0017 §5 の写しは持たない・設計固有のもの）

- 応答を `jq` で切り出す（host の道具を 2 つに増やす・型の無い経路が 1 つ増える）。
- token を `-H` の argv で渡す（`ps` に見える）。`--netrc` / file 経由（disk に token の写しを作る）。
- 1 口座 1 行に 3 窓を詰める（flat な JSON に配列を持ち込む・KNOWN_KEYS が窓数に依存する）。
- `used_pct` を小数で持つ（`Value` に浮動小数を足すと flat 行の値域が広がる・整数 % で足りる）。
- `run = "-"` の番兵（replay に幽霊の run が生まれる）。
- Unmeasured を Measured の `used_pct = 0` + flag で表す（FR33「0 に読み替えない」に構造で反する）。
- token の refresh（§3・s2-07l.229）の却下案: (B) tick が `token_expired` の口座を「refresh 待ち」として planner の判定行に出す（人手を挟む・C9「人手なしで継ぐ」に反する）／(C) 選定で `token_expired` を候補に残す（測れていない口座を選ぶ・C10）／器が refresh token で token endpoint を自分で叩く（器が credential file を書くことになり ADR-0017 §2.5 の fence を破る・OAuth client の秘密を器が持つ）／refresh を model 呼出の無い command（`doctor` / `mcp list` 等）で誘発する（どの command が refresh に届くかが版の内部で未検証・実測で届いたのは `-p` だけ＝確かめられたら差し替える・§10）。

## 10. 後続

- doctor の面: manifest の label と event log の実測（host 列）を全 host で突き合わせ、dir の無い label・実測の無い host を 1 行ずつ出す（C3.2・v3）。
- tick からの定期計測（seat-autonomy の領分）。選ぶ規則（R-C9-1・A2）。
- refresh を model 呼出の無い command で誘発する形（§3・§9）: Claude Code の版で「refresh に届く非 model の command」が確かめられたら、`Call` の prompt を落としてその command に差し替える（外形は `refresh=` の値のまま・歯は偽 claude のまま）。再 login（墓標）は user の手番のまま。

## 11. `fleet` の置き場の既定と人が読む表（契約表の行 a・`s2-07l.403`）

- 何が起きているか: `fleet` の口は `--state-dir` を必須で受け（`fleet/cli.rs` の dispatch）、無い周は使い方だけを出す。`seat` の口は `seat/mod.rs` の state_dir_of（`--state-dir` > git 設定の 2 経路・出所付き・C10）で解く＝口ごとに解き方が違い、人が手で `fleet usage --show` を撃てない（user 直命 2026-09-16 08:1xZ・逐語は台帳 `s2-07l.403`）。1 行形（`usage: account=…`）は機械の読み手（tick・選定・歯）の面で、人が口座の状況を一目で見る形が無い。
- 形: (1) `fleet` の dispatch は `--state-dir` を任意にし、無い周は `seat` と**同じ 1 関数**（state_dir_of）で解く（第 2 の解決を書かない・env は読まない・C2.2）。解けない周は `fleet: refused reason=state-dir` の 1 行 + 使い方で rc 1（store を作らない）。全 verb（record / show / export / usage / select）が同じ入口を通る。**1 行形の字面は不変**（出所は表の見出し行に載せ、1 行形には足さない＝機械の読み手を動かさない）。(2) `fleet usage --table`: **出力の形**の指定で、計測か表示か（`--show`）とは直交（`--show --table` = read-only の表・`--table` だけ = 計測してから表）。表は pure 関数 1 本が組む: 1 行目 = `state_dir=<path> source=<flag|git-config>`、2 行目 = 見出し（account / 5h / 7d / model / seat / resets）、以下は口座ごとに 1 行。値は 1 行形と同じ replay の `allowance` から取り、Unmeasured の窓は `unmeasured:<reason>`、model 窓は名と % を `Fable:75%` の形、seat 列は登録 row が持つ口座ならその役割の名・無ければ `-`、resets は 5 時間窓の reset 時刻。列幅は値の最大幅で揃える（数を code に書かない）。
- 触らない: 1 行形の字面・event の形・`select` の判定・state_dir_of の中身・`--curl` / `--claude` の経路・極性一覧（Guard を足さない・§6）。
- 歯（`fleet_usage_statedir_` / `fleet_usage_table_` 接頭辞・`crates/scribe2/tests/e2e/fleet.rs` と `fleet/usage.rs` の in-file）: tmp repo の git 設定から解いた周は flag 無しで計測し store がその dir に出来る／git の無い tmp cwd で flag 無しは typed に断り store を作らない（**cwd は tmp**＝repo の cwd で撃つと本物の置き場を解く）／`--show --table` が見出し 2 行 + 口座行を出し seat 列が登録 row の役割を映す／pure な表の歯（Unmeasured 混在・列幅・口座 0 件）。既存の flag 必須の歯（`--state-dir` 無し = rc 1）は前者 2 本に置き換える。
- 却下: 1 行形に `source=` を足す（tick と選定の歯が字面を読む・機械面を動かす）／`--table` を既定にする（機械の読み手が表を parse する）／fleet に第 2 の解決関数を書く（seat と食い違う）／表の列幅を定数で持つ（数を code に焼く）。

## 12. fleet の e2e fixture の reset を壁時計から組む — 固定日付の時限を撤去する（契約表の行 b・`s2-07l.468`）

- 何が起きているか（planner の実測 2026-09-18・main 12e64cc・verified）: `crates/scribe2/tests/e2e/fleet.rs` の fixture `LIVE_BODY`（偽 curl が返す応答の本文）は `seven_day` / `limits[]` の `resets_at` を固定の `2026-09-18T00:00:00` で持つ。選定（`fleet select`・`fleet/select.rs` の `not_stale` = `resets_at >= now`）は壁時計の now と比べるので、その時刻を跨いだ瞬間から窓が「測れていない」に倒れて候補なしになり、`account_cmd_retired_account_leaves_select_and_usage` が赤（workspace 1621 本中この 1 本・nextest 単体でも同じ）。main の CI と gate の nextest・検出線の baseline がすべて赤＝便が 1 本も通らない（憲法 C12.6）。
- 形（行 b・S・test だけの差分）: reset 2 つを **process で 1 回**壁時計から組む（`LazyLock` の static 1 つ・five_hour = 翌日 05:00Z・seven_day = 7 日後 00:00Z・字面は器の pub な `format_utc`（`fleet/cli.rs`）と同じ `YYYY-MM-DDThh:mm:ssZ`）。`LIVE_BODY` はその値から組む `LazyLock` の static に替える（本文の形 = `+00:00` 形・小数付きの形・`Z` 形の混在は不変・値は末尾の `Z` を外して差す）。期待の側（`live_line`・`fleet_usage_` の歯の期待 tuple 3 つ・`shape_mismatch` の case の期待 1 本）も同じ static から組み、固定日付の literal を fixture と期待の両方から消す。呼び手は `&LIVE_BODY`（`fake_curl` の引数）。flip-check は test だけの差分で base が緑（base に新しい fixture を当てれば通る）なので、行頭の札 `// flip-check: retroactive s2-07l.468` を test 区間に 1 行置く（判定行 `retroactive=1`・notes に変異 proof）。
- 触らない: 選定の規則（`resets_at >= now`・過去の窓を stale と読むのは C10 の意図どおり）・器の src・`fleet_json_tree_reads_the_usage_shape` の fixture（構文の歯・壁時計と比べない）・ts を注入する歯（`ALLOWANCE_TS` / `RESETS_AT` の定数・now を渡す経路は時限ではない）・snapshot。
- 歯: 検証行 1 = `account_cmd_retired_account_leaves_select_and_usage`（base = 時限で赤・head = 緑＝flip の RED は「環境（壁時計）」で機能不在ではない）／検証行 2 = `fleet_usage_` 接頭辞（期待を static から組み直した歯が緑のまま）。新しい歯は足さない。
- 却下: 日付だけ先へずらす（同じ穴が再発）／選定の `now` を歯から注入できる口を器に足す（src を触る便になり main-red の回復が遅れる・C2.2 の env 縫い目にもなりうる＝別 bead）／固定日付を 2099 にする（時限のまま）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "fleet の置き場を seat と同じ 1 関数で解き、fleet usage --table が人の読む表を出す — 1 行形と event は不変"
req = ["FR33", "FR57"]
section = "11"
write-set = ["crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2/tests/e2e/fleet.rs", "crates/scribe2/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_usage_statedir_", "cargo nextest run -p scribe2 --no-tests=fail fleet_usage_table_"]
size = "S"
done = "flag 無しの fleet usage --show --table が git 設定の置き場から見出し 2 行と口座行を出し、git の無い cwd では typed に断って store を作らず、1 行形の字面と event の形は不変"
[[contract]]
id = "b"
title = "fleet の e2e fixture の reset を壁時計から組む — 固定日付 2026-09-18T00:00Z の時限を撤去し、選定の歯が常に未来の窓を見る（test だけの差分・札 retroactive）"
req = ["FR33", "AC11"]
section = "12"
tests = ["crates/scribe2/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail account_cmd_retired_account_leaves_select_and_usage", "cargo nextest run -p scribe2 --no-tests=fail fleet_usage_"]
size = "S"
done = "fleet.rs の fixture の resets_at が今より未来の値で組まれ、固定日付の literal が fixture と期待の両方から消え、fleet:: の歯が全部緑で main の nextest --workspace が緑に戻る"

<!-- contracts:end -->
