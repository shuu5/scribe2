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
- credential の場所 = `<state_dir>/accounts/<label>/`（dir または link・**user が host ごとに置く**）。中の `.credentials.json`（Claude Code の私有形式）から `claudeAiOauth.accessToken` と `claudeAiOauth.expiresAt` だけを読む。他の field（refresh token 等）は読まない・保持しない・出さない。
- state dir は ADR-0004 §2.4 の経路（`--state-dir` 必須・fleet の subcommand の既存形）。HOME も env も読まない。
- 器は `<state_dir>/accounts/` を**走査しない**（真実は manifest の宣言・C3）。label に dir が無い host ではその口座を「測れなかった（credential 不在）」として記録し、コマンドは続く。

## 3. 計測（子 process と応答）

- HTTP client = `curl`（host の道具・git / claude / tmux と同種）。器の外の OSS の採用なので A3 の対象で、承認 = user 裁定 2026-09-12（ADR-0017 §2.2）。C13 の crate 手続きは対象なし。実行 file は `--curl PATH` で差し替え可（既定は `curl`・PATH 解決は子 process 起動側）。
- 起動形: `curl -sS -K - --max-time <rules 行の秒> -o - -w '\n%{http_code}' <URL>`。**token は stdin の設定（`-K -`）で渡す**: `header = "Authorization: Bearer <token>"` の行（と endpoint が要する固定 header）を stdin へ書き、閉じる。argv に token を載せない（`ps` に見えるため）。
- URL は code の定数 1 つ（host 固有の値ではない・manifest に置かない）。endpoint の名（`endpoint` field に書く出所）は同じ定数から導く短い識別子。
- 応答: stdout の末尾 1 行が HTTP status、その前が本文。status が 200 以外・rc 非 0・timeout は Unmeasured。本文は入れ子の JSON（`five_hour` / `seven_day` = `{utilization: 小数, resets_at: 文字列}`・`limits[]` = `{kind, scope: {model: {display_name}}, utilization, resets_at}`）。
- JSON reader: `json_lite` を **入れ子 object・配列・数（小数含む）** へ広げる（std のみ）。event log の flat な行の書き手 / 読み手（`Value` の 4 値）は**変えない**（別の型 `Tree` を足す。flat 行の受理は狭いまま＝綴り違いの key を拒む性質を保つ）。
- 窓の対応: `five_hour` → `window = "five_hour"`、`seven_day` → `"seven_day"`、`limits[]` のうち `kind == "weekly_scoped"` の要素 → `window = "seven_day_model"` + `model = <scope.model.display_name>`（**display_name でしか結べない**・`id` は null の実測）。要素が 0 件なら model 行は出さない（Unmeasured ではない・窓が無いだけ）。要素が `display_name` を持たない・型が違うなら**その要素だけ** Unmeasured（理由 = 形が違う）。
- 使用率は `utilization`（0〜1 の小数・稀に 1 超）を **整数 %（切り捨て・100 で cap しない**＝超過をそのまま残す）に、reset は `resets_at` を UTC `YYYY-MM-DDTHH:MM:SSZ` に正規化。parse 不能なら Unmeasured（理由 = 形が違う）。
- 待ち時間の上限 = rules 行 `fleet.usage_timeout_s`（新 kind `UsageTimeoutS`・値は user 裁定の id 付き・C5）。契約 (b) で足す。

## 4. event の追加（schema 1 のまま）

`EventKind` に 2 variant を**末尾**に足す（C2 宣言順・ADR-0013 §2.2 の判別子順 pin の内側）。

| variant | 意味 | 必須 field | 任意 field |
|---|---|---|---|
| `AllowanceMeasured` | 1 口座 1 窓の実測 | `account` `window` `endpoint` `used_pct`(u64) `resets_at` | `model`（`seven_day_model` のとき必須） |
| `AllowanceUnmeasured` | 読めなかった | `account` `endpoint` `reason` | `window` `model`（要素単位の失敗のとき） |

- 共通 field（`schema` `ts` `kind` `host` `actor`）は既存どおり。`actor` は `machine`。
- **`run` / `bead` を任意 field に緩める**（既存の行はすべて読める＝D-5 の「同じ schema 番号」・ADR-0016 §2.2 と同じ論法）。既存 8 + 2 kind の行では引き続き必須（kind ごとの必須 field を `EventKind` の網羅 match で検査する）。
- `KNOWN_KEYS` に `account` `window` `model` `endpoint` `used_pct` `resets_at` `reason` を足す。`Unmeasured` 行に `used_pct` が在れば malformed（0 の捏造を構造で拒む）。
- `reason` = `pub enum UnmeasuredReason { NoCredentials, NoToken, Tombstone, TokenExpired, ClientMissing, ClientFailed, HttpStatus, Timeout, BodyUnreadable, ShapeMismatch }`（閉じた enum・字面変換は wildcard 無しの match・**極性 FailOpen**・§6・極性一覧には載せない）。
- replay: `State` に `allowance: BTreeMap<(account, window), AllowanceLatest>` を足す（口座 × 窓ごとの物理順で最後の行・Measured / Unmeasured のどちらでも最新が勝つ＝古い実測値を新しい「測れなかった」が覆う）。runs / seats は触らない（allowance 行は run を作らない）。
- `fleet export`（跨版 面 2）は**変えない**（header の件数・run 行・seat 行のまま）。allowance の外形は §5 の 1 行表示。

## 5. CLI（`<NAME> fleet …`・`--state-dir D` 必須・出力は emit 経由）

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
- **(b)**（前提: rules 行 `fleet.usage_timeout_s` の値の user 裁定が先に在ること・C5。裁定前に起票しない）manifest `[[account]]` 行 + `EventKind` 2 variant + `UnmeasuredReason` + `run` / `bead` 任意化 + replay の `allowance` + rules 行 `fleet.usage_timeout_s`（kind `UsageTimeoutS`・裁定 id）+ 極性一覧 / 外形 snapshot の更新。write-set は構造の連鎖（manifest + `rules/mod.rs` + snapshot + kind 件数の歯 + `fleet/mod.rs` + `polarity`）。
- **(c)** `fleet usage` / `--show` + 子 process 起動 + credential 読み + 偽 client の test helper。(a) (b) に依存。
- AC11 の実演は (c) の land 後に planner が host で行い、値の一致を bead notes に逐語で記帳する。

## 9. 却下案（ADR-0017 §5 の写しは持たない・設計固有のもの）

- 応答を `jq` で切り出す（host の道具を 2 つに増やす・型の無い経路が 1 つ増える）。
- token を `-H` の argv で渡す（`ps` に見える）。`--netrc` / file 経由（disk に token の写しを作る）。
- 1 口座 1 行に 3 窓を詰める（flat な JSON に配列を持ち込む・KNOWN_KEYS が窓数に依存する）。
- `used_pct` を小数で持つ（`Value` に浮動小数を足すと flat 行の値域が広がる・整数 % で足りる）。
- `run = "-"` の番兵（replay に幽霊の run が生まれる）。
- Unmeasured を Measured の `used_pct = 0` + flag で表す（FR33「0 に読み替えない」に構造で反する）。

## 10. 後続

- doctor の面: manifest の label と event log の実測（host 列）を全 host で突き合わせ、dir の無い label・実測の無い host を 1 行ずつ出す（C3.2・v3）。
- tick からの定期計測（seat-autonomy の領分）。選ぶ規則（R-C9-1・A2）。
