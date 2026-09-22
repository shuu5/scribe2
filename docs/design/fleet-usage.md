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
- **本便が seat 側に要る到達は全部「読むだけ」である**（verified・2026-09-20・main dce6aea。可視性を上げる編集が要らないので write-set は fleet 側の 4 面で閉じ、seat の src を触る実装になった周は gate の write-set 照合が outside-scope で止める＝その時は契約を書き直す）: (a) 置き場の解決 = `seat/mod.rs` の state_dir_of は `pub fn`（`lib.rs` は seat を `pub mod` で持つ）で、返す型も `pub`（解決した path と出所を持ち、出所は 2 値で字面は `Provenance::as_str`）。(b) 行の字面 = 既存の `StateDir::suffix`（出所を先・path を行末に置く既存の規約）をそのまま呼ぶ＝第 2 の書式を書かない。(c) 登録 row の読み手 = `fleet` 自身の replay が持つ `State` の `registrations`（鍵 = 役割 × anchor・値の `Registration` は `account` と `role` を `pub` で持つ）で、`fleet/usage.rs` は既に replay を呼んで `State` を読んでいる（`allowance`）＝読み手の module は増えない。役割の字面は `Role::as_str` で、ADR-0045 で役割は 1 値（Orchestrator）＝seat 列は実質「登録の有無」を映す。
- 形: (1) `fleet` の dispatch は `--state-dir` を任意にし、無い周は `seat` と**同じ 1 関数**（state_dir_of）で解く（第 2 の解決を書かない・env は読まない・C2.2。`fleet/cli.rs` の module doc の「置き場は flag で必ず外から受け取る」も同じ便で直す）。**verb を先に読み、既知の verb の周だけ置き場を解く**（verb の無い周・未知の verb の周は従来どおり使い方の 1 行＝外形の他の行を動かさない）。解けない周は `fleet: refused reason=state-dir` の 1 行 + 使い方で rc 1（store を作らない）。全 verb（record / show / export / usage / select）が同じ入口を通る。**1 行形の字面は不変**（出所は表の見出し行に載せ、1 行形には足さない＝機械の読み手を動かさない）。(2) `fleet usage --table`: **出力の形**の指定で、計測か表示か（`--show`）とは直交（`--show --table` = read-only の表・`--table` だけ = 計測してから表）。表は pure 関数 1 本が組む: 1 行目 = `StateDir::suffix` の字面から先頭の空白を落としたもの（出所が先・path が行末）、2 行目 = 見出し（account / 5h / 7d / model / seat / resets）、以下は口座ごとに 1 行。値は 1 行形と同じ replay の `allowance` から取り、Unmeasured の窓は `unmeasured:<reason>`、model 窓は名と % を `Fable:75%` の形、seat 列は登録 row が持つ口座ならその役割の名・無ければ `-`、resets は 5 時間窓の reset 時刻。列幅は値の最大幅で揃える（数を code に書かない）。
- 約束（行 a の done の (1)〜(6) と 1:1・番号は done の順）:
  1. flag 無しで git 設定から解いた置き場に計測が載る（flag が在る周は従来どおり flag が勝つ）。
  2. 全 verb（record / show / export / usage / select）が同じ入口を通る＝flag 無しで撃った 5 verb のどれも置き場の断りを出さない（各 verb 固有の断り〔引数の不足・無い便〕はそのまま）。
  3. 置き場を解けない cwd では 5 verb とも同じ 1 行で typed に断り、store を作らない（rc 1・stdout に表を出さない）。
  4. `--table` が見出し 2 行 + 口座ごとの行を出し、seat 列が登録 row の役割名（登録の無い口座は `-`）を映し、Unmeasured の窓と列幅が値から決まる。
  5. **1 行形の字面と event の形は不変**（`--table` を付けない周の出力と、口座 × 窓ごとの event の並び）。
  6. usage の外形は `--state-dir` が任意になった 1 行だけが変わり、他の行は不変。
- 歯（`fleet_usage_statedir_` / `fleet_usage_table_` 接頭辞・`crates/scribe2-boundary/tests/e2e/fleet.rs` と `fleet/usage.rs` の in-file）: 約束 1〜3 = tmp repo の git 設定から解いた周は flag 無しで計測し store がその dir に出来る（flag が在る周は flag の dir に出来る）／同じ tmp repo で 5 verb を flag 無しで撃つと置き場の断りが 1 つも出ない／git の無い tmp cwd では 5 verb とも同じ 1 行で断り store を作らない（**cwd は tmp**＝repo の cwd で撃つと本物の置き場を解く）。約束 4 = `--show --table` が見出し 2 行 + 口座行を出し seat 列が登録 row の役割を映す（登録の無い口座は `-`）／pure な表の歯（Unmeasured 混在・列幅・口座 0 件）。既存の flag 必須の歯 `fleet_state_dir_flag_is_required`（`--state-dir` 無し = rc 1）は約束 1〜3 の歯に置き換える。
- 検証行（4 本・歯の file はどれも行 a の write-set の中）: 約束 1〜3 = `fleet_usage_statedir_`／約束 4 = `fleet_usage_table_`（どちらも base で 0 本＝RED の理由は機能不在）／約束 5 = 既存の `fleet_usage_measures_two_accounts_into_lines_and_events`（1 行形と event の形を測る歯・緑のまま）／約束 6 = 既存の `fleet_external_form`（外形 snapshot・`--state-dir` の 1 語だけが変わる）。filter は歯の名の全体か十分に長い接頭辞で書く（裸の `fleet_usage_` は `fleet/usage.rs` の unit の歯と `tests/e2e/rules.rs` の歯にも当たり、受付の導出が write-set の外へ広がる・§12 の実測）。
- 触らない: 1 行形の字面・event の形・`select` の判定・state_dir_of の中身と seat 側の src（読むだけ・上の (a)〜(c)）・`State` に新しい method を足すこと（登録 row は `fleet/usage.rs` の中で読む・既存の `State::registered_accounts` は label の集合で役割を持たないので表の読み手にはしない）・`--curl` / `--claude` の経路・極性一覧（Guard を足さない・§6）。
- 却下: 1 行形に `source=` を足す（tick と選定の歯が字面を読む・機械面を動かす）／`--table` を既定にする（機械の読み手が表を parse する）／fleet に第 2 の解決関数を書く（seat と食い違う）／表の 1 行目に独自の書式を作る（`StateDir::suffix` と 2 本立てになり path を行末に置く規約が割れる）／表の列幅を定数で持つ（数を code に焼く）。

## 12. fleet の e2e fixture の reset を壁時計から組む — 固定日付の時限を撤去する（契約表の行 b・`s2-07l.468`）

- 何が起きているか（planner の実測 2026-09-18・main 12e64cc・verified）: `crates/scribe2-boundary/tests/e2e/fleet.rs` の fixture `LIVE_BODY`（偽 curl が返す応答の本文）は `seven_day` / `limits[]` の `resets_at` を固定の `2026-09-18T00:00:00` で持つ。選定（`fleet select`・`fleet/select.rs` の `not_stale` = `resets_at >= now`）は壁時計の now と比べるので、その時刻を跨いだ瞬間から窓が「測れていない」に倒れて候補なしになり、`account_cmd_retired_account_leaves_select_and_usage` が赤（workspace 1621 本中この 1 本・nextest 単体でも同じ）。main の CI と gate の nextest・検出線の baseline がすべて赤＝便が 1 本も通らない（憲法 C12.6）。
- 形（行 b・S・test だけの差分）: reset 2 つを **process で 1 回**壁時計から組む（`LazyLock` の static 1 つ・five_hour = 翌日 05:00Z・seven_day = 7 日後 00:00Z・字面は器の pub な `format_utc`（`fleet/cli.rs`）と同じ `YYYY-MM-DDThh:mm:ssZ`）。`LIVE_BODY` はその値から組む `LazyLock` の static に替える（本文の形 = `+00:00` 形・小数付きの形・`Z` 形の混在は不変・値は末尾の `Z` を外して差す）。期待の側（`live_line`・`fleet_usage_` の歯の期待 tuple 3 つ・`shape_mismatch` の case の期待 1 本）も同じ static から組み、固定日付の literal を fixture と期待の両方から消す。呼び手は `&LIVE_BODY`（`fake_curl` の引数）。flip-check は test だけの差分で base が緑（base に新しい fixture を当てれば通る）なので、行頭の札 `// flip-check: retroactive s2-07l.468` を test 区間に 1 行置く（判定行 `retroactive=1`・notes に変異 proof）。
- 触らない: 選定の規則（`resets_at >= now`・過去の窓を stale と読むのは C10 の意図どおり）・器の src・`fleet_json_tree_reads_the_usage_shape` の fixture（構文の歯・壁時計と比べない）・ts を注入する歯（`ALLOWANCE_TS` / `RESETS_AT` の定数・now を渡す経路は時限ではない）・snapshot。
- 歯: 検証行 1 = `account_cmd_retired_account_leaves_select_and_usage`（base = 時限で赤・head = 緑＝flip の RED は「環境（壁時計）」で機能不在ではない）／検証行 2 = `fleet_usage_measures_two_accounts_into_lines_and_events`（`want_for` の tuple 3 つと `live_line` の期待を static から組み直した歯が緑のまま）／検証行 3 = `fleet_usage_client_failures_name_their_reason`（`shape_mismatch` の case の期待・同上）。filter は歯の名の全体で書く: 裸の接頭辞 `fleet_usage_` は `src/fleet/usage.rs` の unit の歯 8 本と `tests/e2e/rules.rs` の歯 1 本の名にも含まれ、受付の導出（歯の置き場 = fn 名が filter 語を含む file）が write-set を 3 面に広げて審査が落ちる（run 003510Z の実測）。新しい歯は足さない。
- 却下: 日付だけ先へずらす（同じ穴が再発）／選定の `now` を歯から注入できる口を器に足す（src を触る便になり main-red の回復が遅れる・C2.2 の env 縫い目にもなりうる＝別 bead）／固定日付を 2099 にする（時限のまま）。

## 13. fleet/usage.rs の「credential と HTTP fetch と JSON の読み」の群を子 module へ割る（契約表の行 c・純移動・[contract-source.md](./contract-source.md) §37 と [pipeline.md](./pipeline.md) §45 の型）

やさしく言うと: 口座の残量を測る file が上限（1500 行）まで残り 52 行しか無く、この file を触る便が S でも受付で断られる。責務が閉じている「credential を読み、API を叩き、JSON を読む」群を、名前も本文も変えずに子の file へ移して余地を作る。

- 出所（orchestrator の実測 2026-09-22・`pipe dispatch ls` と `pipe preflight`）: `crates/scribe2/src/fleet/usage.rs` は幅 120 で正規化した行数が **1448**（上限 R-C4-2 = 1500・余地 **52**）で、[account-autonomy.md](./account-autonomy.md) 行 o（`s2-07l.359`・S）・[gate-cost.md](./gate-cost.md) 行 r（`s2-07l.462`・M）・[fleet-event-log.md](./fleet-event-log.md) 行 b（`s2-07l.535`・M）の 3 本が `cap-headroom` で受付を通らない（S の見積 100 > 52）。
- 現物（orchestrator が grep と正規化行数で実測・main 4dec9b8）: 責務は 7 群（const と型 37–188・入口 190–229・表 231–366・計測と鮮度 367–600・子 process 593–719・**credential と HTTP fetch と JSON の読み 721–979**・1 行形 981–1047）で、in-file の歯は 1049 行から（`#[cfg(test)]` の次の非空行が `mod tests {`＝札は `mod tests {` の直後に置ける）。**読みの群は閉じている**: item は 17 個（`credential_path` / `read_credential` / `now_ms` / `token_of` / `epoch_ms` / `client_args` / `config_of` / `fetch` / `body_of` / `windows_of` / `model_rows` / `value_key` / `window_row` / `reading` / `whole_pct` / `unmeasured` / `normalize_resets`・721–979 行・正規化 **260** 行）で、**全部が裸の private `fn`**（struct / enum / impl / 属性付きの item が 0 個＝field 由来の `items-differ` が原理的に起きない）、本文に `super::` / `crate::` の字面が 0 件。親の本体から裸で呼ばれるのは 7 名（`credential_path` 1 site・`read_credential` 2・`now_ms` 3・`token_of` 2・`fetch` 1・`windows_of` 1・`unmeasured` 1・全部 `read_account`〔602–623 行〕と `cutoff_of`〔436–441 行〕の中）、他 module からの参照は **0 site**（`crates/` 全数の `usage::` は `run` / `run_in` / `run_with` / `declared` / `fresh_of` / `Freshness` / `UsageError` / `POLARITY` だけ）。歯の `use super::{…}`（1051–1054 行・15 名）が名指す群の名は 4 つ（`body_of` / `client_args` / `config_of` / `normalize_resets`）。群が親から引くのは const 5 つ（`URL` / `BETA` / `RC_CLIENT_TIMEOUT` / `SCOPED_KIND` / `CREDENTIAL_FILE`）と `endpoint` の 1 fn、親の `use` 束縛 7 つ（`Allowance` / `Measured` / `Unmeasured` / `UnmeasuredReason` / `WindowKind` / `Tree` / `json_tree`・全部 `crate::fleet` の pub 面）で、field を歯が構築する型は群に無い。
- 名前解決の形（§45 と同じ・可視性は名前解決をしない）: 親に `use` を置く。解く名は 11（本体の 7 + 歯の 4）で、**歯だけが読む 4 名**を素の `use` に入れると通常 build で `unused_imports` → `-D warnings` で rc 101 になるので、`use` は**本体用（7 名・素）と歯用（4 名・`#[cfg(test)]` 付き）の 2 文**に割る（属性は別の行）。群の中だけで呼ばれる 6 名（`epoch_ms` / `model_rows` / `value_key` / `window_row` / `reading` / `whole_pct`）は可視性を 1 語も変えない。`dead_code` は起きない（4 名とも群内で `fetch` / `reading` が呼ぶ）。**親で孤立する import は削る**（compile が `unused_imports` で名指す分だけ・実測の見込みは `Tree` / `SystemTime` / `UNIX_EPOCH` / `io::Write` の 4 つで、`PathBuf` は doc の字面が親に残るので実測して決める・`use` 行の増減は `residual_allowed` の `use` の頭 / span で許される）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 17 item（721–979 行・正規化 260 行）を、行 c の write-set の `+` の file へ名・本文・順序・doc comment を変えずにそのまま移す（doc comment は item の一部＝1 字も書き換えない）。子の頭は module doc と、親の const / fn を引く `use super::{…}` 1 行と、`crate::fleet` の型と std（`io::Write` / `path::{Path, PathBuf}` / `process::{Command, Stdio}` / `time::{SystemTime, UNIX_EPOCH}`）の `use` だけ。子の module 名と親の `enum Read`（144 行）は別の識別子、子の中の fn `fetch` と module 名は名前空間が別（§45 の `nextest` と同型）。
  2. 親に増えるのは **4 行だけ**——`mod` 宣言 1 行（module doc〔1–13 行〕と最初の `use`〔15 行〕の間）、本体用の素の `use` 1 行（7 名・`pub` は付けない・`mod` 宣言の直後）、歯用の `#[cfg(test)]` だけの 1 行と `use` 1 行（4 名・**既存の行頭 `#[cfg(test)]`〔1049 行〕の直上**）。4 行とも 120 桁に収まる。孤立した `use` を削る行はこの数に含めない（残差の許容形）。
  3. 歯は 1 本も足さず 1 本も変えない: in-file の `mod tests` の本文と `use super::{…}` は 1 byte も変えない（その `use` は親の歯用の `use` が解く）。e2e（`crates/scribe2-boundary/tests/e2e/fleet.rs`）は binary 越しで名を引かず、write-set の外。
  4. 上げるのは**子側**の可視性だけで、語は `pub(super)` の 1 種類。上げる集合は名指しで **11**（本体の 7 名 + 歯の 4 名・全部 fn の頭の行）。親側の可視性は変えない。
  5. 純移動の札 `// flip-check: moved <行 c の bead>` を親の `mod tests {` の直後と子の module doc の直後に 1 行ずつ置く（説明 1 行 + 札 1 行の 2 行・[pipeline.md](./pipeline.md) §7 の `moved` の逃がし・入口の RED は札が担う）。
  6. 検証行が名指す歯は**既存の 4 本**（`fleet_usage_windows_of_maps_windows_and_isolates_the_broken_element` / `fleet_usage_token_of_names_each_credential_failure` / `fleet_usage_client_args_carry_timeout_and_never_the_token` / `fleet_usage_resets_accepts_z_and_utc_offset_and_rejects_the_rest`・全部 in-file・新設 0 本）で、着地後も名・本数・本文が不変。**接頭辞では書かない**（`fleet_usage_` は表の歯と `fleet_usage_error_polarity_` と `tests/e2e/rules.rs` の歯にも当たり、受付の導出が write-set を広げる・§12 の実測）。
- write-set の面（[contract-source.md](./contract-source.md) §3 の逐語: 縮む面は「`-` 接頭辞で宣言・base に実在する file・この便でその file の増分は 0 以下という見積の符号を項目が運ぶ」）: 親 `usage.rs` は **縮む面**（`-`・余地 52 の file を触る便なので、素の path で書くと受付が自分の見積で `cap-headroom` に倒れる＝本行が受付を通らない・実測 2026-09-22 の行 al と同型）、子は **新規 file**（`+`）。diff の面は「親から 17 item が消え、子に同じ 17 item が現れる」の 2 file だけで、`-` は削除の宣言ではない。
- 見積: 親 約 1188 行（余地 約 312＝size M を受けられる）・子 約 270 行。xtask の門の副作用 2 つを先に書く（[core-boundary.md](./core-boundary.md) の検出線）: (i) src / test の切れ目が最初の行頭 `#[cfg(test)]` に動くので `core-lines` が 2 行減るだけ（`std::env::` は群に 0 件＝`env_reads` は動かない）。(ii) `Command::new` の holder が `fetch` の移動で **1 増える**（site 34 / holder 20 → 34 / 21・親は `signal_group` の 1 site を残す）。違反は立たない（`core-spawn` の上限は検出線で ok は常に真）が件数は動く。`fetch` を親に残す回避は親 1214（余地 286）で M に届かないので採らない。
- 触らない: 移す群の外の 6 群（const と型・入口・表・計測と鮮度・子 process・1 行形）・`TableRow` の pub field・`Refresh` / `Freshness` / `UsageError` の公開面・e2e の歯・`fleet/mod.rs` の型。
- 却下: 表の群（231–366）を出す（親 1312 / 余地 188 で S 止まり・`table_row` の `state: &super::State` が親の `use` に無い `State` を指し、子へ移すと束縛を足す残差が要る）／計測と鮮度の群を出す（`run` / `declared` / `fresh_of` の公開面と入口が絡む）／`#[cfg(test)] use …;` を 1 行に畳む（`residual-line`・[pipeline.md](./pipeline.md) §45 の便 4 本目の再現）／検証行を接頭辞 `fleet_usage_` で書く（§12 の write-set 拡張の再現）。

## 14. fleet/usage.rs の「表」の群を子 module へ割る（契約表の行 d・純移動・§13 と同じ型・2 便目）

やさしく言うと: §13 で 260 行を出しても、その後の着地で親は上限まで残り 294 行に戻り、size M の便（見積 300）がまた受付で断られる。この file は設計 9 本・12 行が write-set に持つ hub なので、責務が閉じている「人の読む表を描く」群をもう 1 つ子へ移して余地を 400 行台にする。

- 出所（orchestrator の実測 2026-09-22・`pipe preflight`）: `crates/scribe2/src/fleet/usage.rs` は幅 120 で正規化した行数が **1206**（余地 **294**）で、[gate-cost.md](./gate-cost.md) 行 r（`s2-07l.462`・M）が `cap-headroom` で受付を通らない（M の見積 300 > 294）。
- 現物（orchestrator と census の実測・main a654561）: src 区間は 1–804・in-file の歯は 805 行（列 0 の最初の `#[cfg(test)]`＝§13 が置いた歯用の `use` の属性行・807 `mod tests {`・809 に §13 の札）から。src の群は 9 つ（use 頭 15–36・const 38–81・型 83–189・入口 191–245・**表 246–381**・endpoint 383–388・計測 390–516・表示と読み 518–616・refresh 618–735・event と render 737–803）。**表の群は閉じている**: item は 10 個（`CELL_NONE` / `CELL_GAP` / `TABLE_HEAD` / `pub struct TableRow`〔field 6 つ全部 `pub`〕/ `impl TableRow { fn cells }` / `pub fn table` / `aligned` / `table_row` / `pct_cell` / `model_cell`・246–381 行・正規化 **136** 行）。親の本体から裸で呼ばれるのは `tabled`（236–245 行）の 3 名（`TableRow` / `table_row` / `table`）だけ、他 module からの参照は **0 site**（`crates/` 全数の `fleet::usage::` は `UsageError` / `run` / `run_fresh` / `run_in` / `declared` だけ・`pipe/table/parse.rs` と `rules/manifest.rs` の `TableRow` と `seat/role.rs` の `table_row` は同名の別物）。歯の `use super::{…}`（811–813 行）が名指す群の名は同じ 3 名。群が親から引くのは private の `latest_rows` と `RESETS_NONE`、`crate::fleet` の `Allowance` / `WindowKind`、`crate::seat::StateDir`。field を歯が構築する型は群に無い（`TableRow` の field は既に `pub`＝可視性を触らない）。
- 名前解決の形（§13 と同じ）: 親に素の `use` を 1 行置いて 3 名を解く（`use table::{table, table_row, TableRow};`・3 名とも本体に site が在るので `#[cfg(test)]` 付きの `use` は要らない）。上げるのは `table_row` の可視性 1 語（`pub(super)`）だけで、`table` と `TableRow` は `pub` のまま逐語で運び、残り 7 名は子に閉じる。**`super::State` の束縛**: `table_row` の signature は `state: &super::State` の逐語で、子では `super` が `usage` に解けるので、親の `use super::{…}`（20–23 行・`crate::fleet` の名）に `State` を 1 語足す（use の span の中＝残差の許容形・子の `super::State` がこの束縛を使うので `unused_imports` は出ない）。親で孤立する import は **0**（`Allowance` / `WindowKind` / `StateDir` / `RESETS_NONE` は親に site が残る）。module 名 `table` と `pub fn table` は名前空間が別（§13 の `read` と `enum Read` と同型）。親の module doc 13 行目の intra-doc link `` [`table`] `` は先が private になるので素の字面にしてよい（`//!` は残差の許容形・rustdoc の門は CI に無い）。
- 約束（この行が作るもの・番号は done と 1:1）:
  1. 上の 10 item（246–381 行・正規化 136 行）を、行 d の write-set の `+` の file へ名・本文・順序・doc comment を変えずにそのまま移す。子の頭は module doc と札と `use super::{latest_rows, RESETS_NONE};` / `use crate::fleet::{Allowance, WindowKind};` / `use crate::seat::StateDir;` の 3 行だけ。
  2. 親に増えるのは **2 行だけ**——`mod table;` 1 行（15 行 `mod read;` の次）と素の `use table::{table, table_row, TableRow};` 1 行（16 行 `use read::{…};` の次）。親の `use super::{…}` に `State` を 1 語足す差と、孤立した use の削除（見込み 0）はこの数に含めない（残差の許容形）。`#[cfg(test)]` 付きの `use` は足さない。
  3. 歯は 1 本も足さず 1 本も変えない: in-file の `mod tests` の本文と `use super::{…}`（811–813 行）は 1 byte も変えない（3 名は親の素の `use` が解く）。e2e（`crates/scribe2-boundary/tests/e2e/fleet.rs`）は binary 越しで名を引かず、write-set の外。
  4. 上げるのは**子側**の `table_row` の 1 名だけ（語は `pub(super)`）。`table` / `TableRow` の `pub` と field の可視性、親側の可視性は変えない。
  5. 純移動の札 `// flip-check: moved <行 d の bead>` を親の `mod tests {` の直後（§13 の札 809 行の**次の行に足す**＝既存の札は置き換えない・base に無い札だけが効き、持ち越した札は §5.3 の対で残差から外れる・`closure.rs` / `land.rs` に札 2〜3 本の前例）と子の module doc の直後に 1 行ずつ置く。子は歯の区間を持たないので数に入るのは親の札だけ。
  6. 検証行が名指す歯は**既存の in-file 2 本**（`fleet_usage_table_rows_name_unmeasured_windows_and_seat_roles` / `fleet_usage_table_aligns_columns_by_the_widest_value_and_prints_headers_alone_for_no_accounts`・新設 0 本・repo 内で名は一意）で、着地後も名・本数・本文が不変。e2e の `fleet_usage_table_show_prints_two_headers_and_seat_roles_from_registration_rows`（`tests/e2e/fleet.rs`）は binary 越しに同じ表を測るが、file が write-set の外なので検証行には書かない（受付が `teeth-outside-write-set` で断る・2026-09-22 の実測）。接頭辞では書かない（`fleet_usage_table` は他の歯にも当たる）。
- write-set の面（§13 と同じ逐語）: 親 `usage.rs` は **縮む面**（`-`・素の path で書くと受付が自分の見積で `cap-headroom` に倒れる）、子は **新規 file**（`+`）。diff の面は「親から 10 item が消え、子に同じ 10 item が現れる」の 2 file だけで、`-` は削除の宣言ではない。
- 見積: 親 1206 → 約 1072（余地 約 428＝size M を受けられる）・子 約 149 行。xtask の門の副作用は無い（実測）: `Command::new` の site は `signal_group`（728 行）で群の外＝`core-spawn` の件数も holder も不変、`std::env::` は 0 件、src / test の切れ目は 805 → 約 671 へ平行移動するだけ、子に列 0 の `#[cfg(test)]` は置かない。
- 行 r（[gate-cost.md](./gate-cost.md) §26・`s2-07l.462`）との交差: 行 r が触る `Call` の構築（653 行・`refresh` の中）と `Event {` の literal（743 行・`event_of` の中）はどちらも群の外なので、行 r の write-set は `usage.rs` のままでよく、本行の着地で余地だけが増える。
- 触らない: 移す群の外の 8 群・`TableRow` の `pub` field・`Refresh` / `Freshness` / `UsageError` の公開面・e2e の歯・`fleet/mod.rs` の型・§13 が置いた子 `read.rs` と札。
- 却下: refresh の群（641–735・95 行）を出す（120 行に届かず、`Call` と `Command::new` を抱えて行 r と core-spawn の holder が動く）／event と render（737–803・67 行）を出す（小さく `Event {` を抱える）／計測の群（390–516）を出す（`measure` が入口 `tabled` の本体で閉包が広い）／§13 の札を置き換える（持ち越しの対が崩れ、§13 の便が何を免除したか辿れなくなる）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "fleet の置き場を seat と同じ 1 関数で解き、fleet usage --table が人の読む表を出す — 1 行形と event は不変"
req = ["FR33", "FR57"]
section = "11"
write-set = ["crates/scribe2/src/fleet/cli.rs", "crates/scribe2/src/fleet/usage.rs", "crates/scribe2-boundary/tests/e2e/fleet.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__fleet__fleet_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail fleet_usage_statedir_", "cargo nextest run -p scribe2 --no-tests=fail fleet_usage_table_", "cargo nextest run -p scribe2 --no-tests=fail fleet_usage_measures_two_accounts_into_lines_and_events", "cargo nextest run -p scribe2 --no-tests=fail fleet_external_form"]
size = "S"
done = "(1) flag 無しの fleet が git 設定の置き場を seat と同じ 1 関数で解き、(2) record / show / export / usage / select の 5 verb が同じ入口を通って flag 無しでも置き場の断りを出さず、(3) git の無い cwd では 5 verb とも同じ 1 行で断って store を作らず、(4) fleet usage --show --table が見出し 2 行と口座行を出して seat 列が登録 row の役割名（無い口座は -）を映し、(5) 1 行形の字面と event の形は不変で、(6) usage の外形は --state-dir が任意になった 1 行だけが変わる"
[[contract]]
id = "b"
title = "fleet の e2e fixture の reset を壁時計から組む — 固定日付 2026-09-18T00:00Z の時限を撤去し、選定の歯が常に未来の窓を見る（test だけの差分・札 retroactive）"
req = ["FR33", "AC11"]
section = "12"
tests = ["crates/scribe2-boundary/tests/e2e/fleet.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail account_cmd_retired_account_leaves_select_and_usage", "cargo nextest run -p scribe2 --no-tests=fail fleet_usage_measures_two_accounts_into_lines_and_events", "cargo nextest run -p scribe2 --no-tests=fail fleet_usage_client_failures_name_their_reason"]
size = "S"
done = "fleet.rs の fixture の resets_at が今より未来の値で組まれ、固定日付の literal が fixture と期待の両方から消え、fleet:: の歯が全部緑で main の nextest --workspace が緑に戻る"

[[contract]]
id = "c"
title = "fleet/usage.rs の「credential と HTTP fetch と JSON の読み」の群（17 fn・721–979 行・正規化 260 行）を子 module へ割る — 純移動（名・本文・順序・doc comment 不変・歯 0 本・親に mod 1 行と use 2 文の 4 行・子側の pub(super) 11 名・札 2 か所）"
req = ["FR33"]
section = "13"
write-set = ["-crates/scribe2/src/fleet/usage.rs", "+crates/scribe2/src/fleet/usage/read.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail fleet_usage_windows_of_maps_windows_and_isolates_the_broken_element", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_usage_token_of_names_each_credential_failure", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_usage_client_args_carry_timeout_and_never_the_token", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_usage_resets_accepts_z_and_utc_offset_and_rejects_the_rest"]
size = "S"
done = "(1) 17 fn が名・本文・順序・doc comment を変えずに + の file へ移る（move_proof が pure と判じる・items-differ / residual-line 0 件） (2) 親に増えるのは mod 宣言 1 行・素の use 1 行（本体の 7 名）・#[cfg(test)] だけの 1 行と use 1 行（歯の 4 名・既存の行頭 #[cfg(test)] の直上）の 4 行だけで、孤立した use は削る (3) in-file の歯の本文と use super::{…} が 1 byte も変わらず、e2e は触らない (4) 子側の pub(super) は名指しの 11 名だけで、群内の 6 名と親側の可視性は不変 (5) 札 moved が親の mod tests { の直後と子の module doc の直後に 1 行ずつ (6) 名指しの既存 4 本が名・本数・本文不変で緑・clippy -D warnings が通常 build と test build の両方で rc 0"

[[contract]]
id = "d"
title = "fleet/usage.rs の「表」の群（10 item・246–381 行・正規化 136 行）を子 module へ割る — 純移動 2 便目（名・本文・順序・doc comment 不変・歯 0 本・親に mod 1 行と素の use 1 行・子側の pub(super) は table_row の 1 名・親の use super に State を 1 語・札 2 か所）"
req = ["FR33"]
section = "14"
write-set = ["-crates/scribe2/src/fleet/usage.rs", "+crates/scribe2/src/fleet/usage/table.rs"]
verify = ["cargo nextest run -p scribe2 --lib --no-tests=fail fleet_usage_table_rows_name_unmeasured_windows_and_seat_roles", "cargo nextest run -p scribe2 --lib --no-tests=fail fleet_usage_table_aligns_columns_by_the_widest_value_and_prints_headers_alone_for_no_accounts"]
size = "S"
done = "(1) 10 item が名・本文・順序・doc comment を変えずに + の file へ移る（move_proof が pure と判じる・items-differ / residual-line 0 件） (2) 親に増えるのは mod 宣言 1 行と素の use 1 行（3 名）の 2 行だけで、親の use super::{…} に State を 1 語足し、#[cfg(test)] 付きの use は足さない (3) in-file の歯の本文と use super::{…} が 1 byte も変わらず、e2e は触らない (4) 子側の pub(super) は table_row の 1 名だけで、table / TableRow の pub と field の可視性と親側の可視性は不変 (5) 札 moved が親の mod tests { の直後（§13 の札の次の行・置き換えない）と子の module doc の直後に 1 行ずつ (6) 名指しの既存 in-file 2 本が名・本数・本文不変で緑・clippy -D warnings が通常 build と test build の両方で rc 0"

<!-- contracts:end -->
