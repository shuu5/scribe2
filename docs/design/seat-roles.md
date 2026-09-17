# 設計: 席の役割と権能 — 役割は閉じた enum、登録は fleet の event、権能は rules 行、執行は PreToolUse の guard、注入は SessionStart の生成文

- 要件: [FR40](../../design-intent/spec/srs.html#FR40) 席の登録 / [FR41](../../design-intent/spec/srs.html#FR41) 権能の所在 / [FR45](../../design-intent/spec/srs.html#FR45) 権能の執行 / [FR42](../../design-intent/spec/srs.html#FR42) 席の指示文の注入 / [FR30](../../design-intent/spec/srs.html#FR30) 配送構造 / [AC15](../../design-intent/spec/srs.html#AC15) [AC16](../../design-intent/spec/srs.html#AC16) [AC17](../../design-intent/spec/srs.html#AC17)・既存 [FR17](../../design-intent/spec/srs.html#FR17) / [FR18](../../design-intent/spec/srs.html#FR18) / [FR20](../../design-intent/spec/srs.html#FR20) / [FR22](../../design-intent/spec/srs.html#FR22) / [NFR5](../../design-intent/spec/srs.html#NFR5)
- 決定: [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html)（本 doc の決定の正本・§2.1〜§2.8）/ [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html) §2.2（pane → target）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.1（列挙は core・文書は pointer）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1（guard の定義）/ [ADR-0018](../../design-intent/decisions/ADR-0018-working-memory-subcommands-and-pointer-required-directives.html) §2.2（出所 pointer）
- 土台: [seat-state.md](./seat-state.md)（打刻と pane → target）・[vessel-hook.md](./vessel-hook.md)（hooks.json の生成と guard の束）・[working-memory.md](./working-memory.md) §4（PointerKind）・[rules-manifest.md](./rules-manifest.md)・[polarity.md](./polarity.md)・[fleet-event-log.md](./fleet-event-log.md)
- 位置づけ: 役割の規律を憲法 C14 の 2 面（文書 = SRS / ADR・manifest = 権能の行）に収め、執行・注入・生成文書・drift 検査が**同じ行**を読む。散文の役割節（user の設定 file・共有 skill・退避物の命令行）は規則の置き場ではない（ADR-0022 §2.7・撤去は A1 の「消す」として別途 user に確かめる）。

## 1. 何を解くか

役割（planner / 管理席）の規律は文書（SRS FR30〜32・ADR-0016）と散文 7 面にしか無く、器は役割の型を持たない。散文でしか止まっていない事故型は 6 つ（ADR-0022 §1 (a)〜(f)）。本設計は (a)(b)(c)(e) を guard で止め、(d) を記帳の deny で効かなくし、(f) は [working-memory.md](./working-memory.md) の内蔵に委ねる。consumer の repo は席の登録だけで同じ執行と注入を得る（CLAUDE.md に役割の文を書かない）。

## 2. 役割と登録（ADR-0022 §2.1）

- **`Role`**（closed enum・core）: variant の列挙は core が持ち文書は写さない。記録時点の値は 2 つ。`as_str` / `ALL` / 判別子順の pin は既存の enum（`RuleKind` / `Guard`）と同じ形。
- **登録の subcommand**: `<NAME> seat register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR] [--model M]`。`--model M` は席が使う model の display name（任意・[account-autonomy.md](./account-autonomy.md) §3 の session 用の入力・無い周は None＝保守側・契約 (e)）。`--anchor` の既定は cwd の repo root（`git rev-parse --show-toplevel`・env を読まない）。`--launch FILE` は起動の雛形（穴は口座の credential dir 1 つ・[account-autonomy.md](./account-autonomy.md) §5 が使う）で、内容を event に載せる（tracked file に置かない・CON2）。
- **event**: `EventKind::SeatRegistered`（末尾・宣言順）1 件。項目 = `role` / `anchor` / `target` / `sid`（登録を撃った session の id・SessionStart の打刻から解く）/ `account` / `launch`（雛形の本文）/ `model`（任意・席が使う model の display name・契約 (e)・無い row は None）。項目は `Event` の typed な束 1 つ（`allowance` と同型の `Option<Registration>`・kind ではなく束の有無が本体を決める）で持つ＝`Event` を literal で組む既存の構築点（core・歯・property の生成器）と `KINDS` の件数の pin がすべて変わる（write-set は §9 (a)）。schema 1 のまま（値の追加）。
- **鍵と置き換え**: 鍵 = (role, anchor)。同じ鍵の再登録は前の row を置き換える（append のみ・replay の最新が効く・N1）。1 つの anchor に役割ごとに 1 席（FR40）。`target` / `sid` / `account` は項目で鍵ではない。pane id は鍵にも項目にも置かない。**書き手は 2 つ**: `seat register`（席の session が撃つ・打刻の条件付き）と tick の口座更新（[account-autonomy.md](./account-autonomy.md) §5・器の内部の同じ 1 関数・`target` / `sid` / `launch` は既存 row から写す）。3 つ目 = `seat launch`（器が起動行を導出して row を先に書く・`sid` は無し・[account-lifecycle.md](./account-lifecycle.md) §4・ADR-0026 §2.3）。
- **登録を受ける条件**: 登録を撃った session に SessionStart の打刻（[seat-state.md](./seat-state.md) §2）が在ること。打刻の無い session（plugin を積まない・tmux の外）からの登録は typed な理由（`RegisterRefusal::NoStamp`）で断る＝guard の無い席が権能を持てない。登録そのものは権能を要しない（登録が先・ADR-0022 §2.5）。
- **役割の解決**（1 本・読み手は guard / 注入 / doctor / tick）: `pane → target（ADR-0015 §2.2）→ replay を鍵 (role, anchor) ごとに最新 row へ畳んでから `target` が一致する row を引く（同じ鍵の旧 row は旧 target では解けない・複数の鍵が同じ target を持てば replay の最新）→ role`。`anchor` は row の項目として読むだけで、hook の cwd と突合しない（席が worktree へ cd した周も同じ row が解ける）。window 名は target の一部（`session:window`）としてだけ効き、名前の慣習で役割を決めない。env・作業木の path・pane の字面は入力にしない（C2.2 / C3.3 / N3）。window を rename した席は別の target＝登録し直す（doctor の突合が `missing` で出す）。replay の cache は持たない（C3・hook の予算 NFR5 の内側で実測済み）。
- **doctor**: 登録 row と実在の target（tmux の `list-panes`）の突合を項目に持つ（C3.2・値は生成物）。`doctor` の現物は bin crate の `render_doctor`（記録時点は name / version の 2 行・state dir の引数なし）なので、`--state-dir S` の口を足し項目列に 1 行足す（外形 snapshot `doctor_external_form` が変わる）。

## 3. 権能と rules 行（ADR-0022 §2.2 / §2.5）

- **`Capability`**（closed enum・core）: 操作の種別。記録時点の variant の**種類**は次のとおりで、名は core が持つ: 回答の記帳（`pipe answer`）・承認の記帳（approval event）・go の記帳（merge の許可）・便の起動（`pipe intake` / `run` / `resume` / `stop` / `retire`）・中継（席間の連絡を送る）・go 後の merge・path 種別ごとの編集（design-intent / 設計 doc / code / 対象 repo の外）。
- **rules 行**: `RuleKind::RoleCapabilities`（variant 1 つ・値は権能の名の**列**〔既存の `RuleValue::List`・manifest に list の行が既に在る〕・裁定 id 付き）を役割ごとに 1 行（id は `role.<役割名>`）。値（どの役割がどの権能を持つか）は本 doc が決めず、契約 (b) が裁定 id 付きで書く（§9）。列に無い名は manifest の読み込みで拒む（閉じた enum の parse・既存の `RuleError`）。
- **読み手は 4 つ・行は 1 つ**: §4 の guard・§5 の注入文の生成・C1.2 の生成文書（rules 表）・xtask の drift 検査（C14.2）。
- **R-C7-1**: kind は既存の `Dialogue`（対話面）のまま、値を「役割 planner の登録 row を持つ席」を表す typed な値（`Role` の名）へ改める（行の値の変更・裁定 id・契約 (b)）。承認 event / 回答 event / go の記帳は planner の席からだけ受理（§4 の Bash guard が権能の行で止める・AC15）。
- **契約が開く例外**: 管理席が自分の手で code を編集してよい便は、契約 file の typed な印（既存の `classes` と同じ形の field・名は契約 (b) が決める）で表す。§4 の Edit guard はその便の write-set の内側だけ通す（AC16）。散文に置かない。

## 4. 執行（ADR-0022 §2.3）

- **`Guard::Role`**（variant 1 つ・宣言順は `Register` の直後〔`Cap` → `Register` → `Role`＝登録が先で執行が後の行為の流れ〕・InLoop・FailClosed・極性一覧に 1 行）。
- **2 面**: (1) **Bash** — command 行が権能付き subcommand（core の const slice `CAPABILITY_COMMANDS`: subcommand の名 → `Capability`）を含む周に、席の役割の行がその権能を持たなければ deny。(2) **Edit 系** — path の種別（`PathKind`: design-intent / 設計 doc / code / 対象 repo の外・closed enum・分類は repo root からの相対 path の prefix）ごとの権能を照合し、持たなければ deny。契約が印で開いた便の write-set の内側は通す。
- **repo の写し**: `.worktrees/` 直下の worktree は便の worktree（`.worktrees/<NAME>/<run>/`）に限らず repo の写しで、`PathKind` はその worktree からの相対 path で分類する（planner が docs PR 用に切る `.worktrees/<name>/` の `design-intent/` も DesignIntent）。契約の印が開くのは便の worktree だけ・`.worktrees/<name>` そのものは code（s2-07l.227）。
- **identity**: 生成 hooks.json の shell 行が渡す `--pane` だけ（[vessel-hook.md](./vessel-hook.md)・生成器は同じ gen-manifest）。PreToolUse の shell 行に `--pane "$TMUX_PANE"` を足し、matcher を `Edit|Write|MultiEdit|NotebookEdit` から **Bash を含む形**へ改める（Bash は PermissionRequest の matcher でもある・2 面の判定は別 hook event）。
- **解く順**: anchor → pane → target → 登録 row → role → 行 → 権能。**anchor（repo root・state dir）は payload の `cwd` でなく、生成 hooks.json の shell 行が渡す `--project`（session の起動 dir・Claude Code が hook の command に与える project dir・席が `cd` しても変わらない・`--pane` と同型で binary は env を読まない〔C2.2〕）から解く**。`--project` が無い周（旧 hooks.json）は `cwd` で解く（互換・生成物の更新で消える）。pane が空（tmux の外・runner / lens）は席ではなく本 guard の対象外（ADR-0009 の write-set guard と allowlist がそのまま担う）。**pane が在るのに anchor が解けない（root が無い・`served` が `ByMe` でない・state dir が無い）周と、登録 row が無い・target が解けない周は権能なし＝権能付きの操作を deny（FailClosed・理由を stderr に 1 行・記録 1 行）**。[vessel-hook.md](./vessel-hook.md) の「仕えない周は黙る」（FR24）は pane が無い周にだけ当たる（席が repo の外へ `cd` しても guard は外れない）。止めるのは権能付きの操作だけで、それ以外の Bash / Edit は通す。
- **deny 文**: 権能を持つ役割の名を含む（例の形: `<NAME>: この操作は <役割名> 席の権能（rules 行 <id>）`・字面は現物が正本）。**記録行**: allow の周も target と command の種別を 1 行（hook の消費記録と同じ置き場 `<state_dir>/inject.jsonl`・[vessel-hook.md](./vessel-hook.md)。打刻の `state.jsonl` には書かない）。
- **subcommand は役割を検査しない**（引数の identity は偽装できる）。発話は監視しない。
- **runner / lens は pane を持たない（起動の包みの 2 口で同じ）**: 便の runner / lens / verify 行は行の包み（`confine::wrap_line`）が起動側の `TMUX_PANE` を外す（s2-07l.216）。`headless/mod.rs` の `build`（runner と lens の唯一の構築点・pipe の外から `<NAME> runner` / `<NAME> lens` を単体起動した周もここを通る）は command の包み（`confine::wrap_command`）を通り、こちらは `TMUX_PANE` を外していなかった＝席の pane の中から単体起動した runner の hook が `--pane` で**その席の打刻と読み込み元の記録**に混入する（2026-09-15 15:23Z・別 repo の管理席が run の plugin 写しで runner を単体起動し、席の `plugin` 記録が run dir を指した）。**2 つの包みは同じ 1 点で `TMUX_PANE` を外す**（`wrap_command` に置き `wrap_line` はそれを通る・外す名は 1 つの const・足す env は無い・C2.2）。runner は席ではない（FR40）ので hook は `--pane` 空で黙る（既存）。

## 5. 注入（ADR-0022 §2.4）

- SessionStart の hook（[vessel-hook.md](./vessel-hook.md)）が pane → target → 登録 row で役割を解き、役割ごとの **tracked な雛形 1 枚**（`headless/runner.txt` と同じ形・binary に埋め込む・`seat/brief/<役割名>.txt`）から生成した指示文を stdout で注入する。登録の無い席は 0 byte（断りも出さない）。
- **雛形の行の規律**: 行は「穴」か「出所 pointer を持つ行」に限る。穴 = `{capabilities}`（権能の行の値の列）/ `{target}` / `{anchor}` / `{role}`。pointer の形は [working-memory.md](./working-memory.md) §4 の `PointerKind` を再利用（憲法の id・ADR の節・SRS の要件 id・rules 行の id）。**規範文の定義 = pointer を持たない行**（typed・字面の語彙で判定しない）。
- **雛形が持つもの**（§4 で塞げない振る舞いだけ・短く）: 中継の形・報告の形・裁定の持ち込み先（planner の席）・第一手の復元（`seat rebrief`）・命令行は pointer 付きだけ従う（ADR-0018 §2.2）・席間の連絡の経路（FR44）・3 クラスの発火の pointer。§4 で塞ぐ事項（回答・承認・go・merge・code の Edit）は書かない（二重化しない）。
- **席間の連絡の行**（FR44・AC19・雛形に置く pointer 付きの行の 1 つ）: planner と管理席の連絡は席の入力欄を経由しない経路＝開発 session の道具が持つ session 間の message（記録時点は Claude Code の `SendMessage` / `ListAgents`・宛先は毎回 `ListAgents` で取る）で送り、届いたことは経路自身の記録（送信結果の message id）で確かめる。器はこの経路を持たず（FR44 の経路設計は ADR-0022 §2.8 の射程外＝道具の機能をそのまま使う）、雛形は「経路の名・宛先の取り方・届いた確認の取り方」を pointer 付きの 3 行で持つだけ。入力欄への注入は器の管理 tick の合図（打刻・退避）と復元の command に限る（FR44）。
- **xtask の検査**（C14.2・AC17・`cargo xtask check` の 1 項目）: 雛形の穴 ⊆ 定義済みの穴・pointer を持たない行 0・行に在って文に無い権能 0（生成文に権能の名がすべて現れる）。生成文は外形 snapshot（C12.5）。
- 器は consumer の repo に file を書かない（CLAUDE.md の生成区間を持たない・1 経路）。

## 6. 極性一覧（[polarity.md](./polarity.md)）

| guard | 段 | 極性 | 何を止めるか |
|---|---|---|---|
| `Register` | in-loop（`seat register` の受付） | FailClosed | 打刻の無い session からの登録（`RegisterRefusal::NoStamp`・打刻が読めない周も断る）。`IntakeRefuse` と同型（受付で止める） |
| `Role` | in-loop（PreToolUse・Bash / Edit 系） | FailClosed | 権能の無い役割の席からの権能付き subcommand と、権能の無い path 種別の編集（登録 row が無い・target が解けない周も deny・権能を解けない断りは `RefuseReason` の variant ごとの代替ルート `route=` を deny 文の末尾に添える・§13） |

§2 の登録の拒否は行為（登録）を止める判定を返すので guard（ADR-0014 §2.1）＝`Guard::Register`（variant 1 つ・宣言順は `Cap` の直後・極性の定数は `seat/role.rs`）。§5 の注入は guard ではない（行為を止めうる判定を返さない）。

## 7. 歯（`crates/<NAME>/tests/e2e/seat.rs` に `seat_role_` 接頭辞・hook は `tests/e2e/hook.rs` に `hook_role_`・名前の列は現物が SSOT）

- 登録: `seat register` が `SeatRegistered` を 1 件追記し replay の最新が効く（同じ鍵の再登録で前の row が残ったまま最新だけが解決される）・打刻の無い session は `NoStamp` で rc 1・event なし・`--anchor` 無しは cwd の repo root・pane id は event に現れない（fixture の pane 文字列が events.jsonl に 0 回）。
- 解決: 役割の解決は登録 row だけを入力にする（pane id を差し替えた fixture でも同じ target なら同じ役割・env を置いても変わらない・同じ鍵で別 target に再登録すると旧 target では解けない・window を rename した fixture は解けない＝登録し直す）。
- guard（hook.rs・偽 tmux で pane → target を返す stub）: 管理席の target から `pipe answer` を含む Bash → deny・deny 文に権能を持つ役割の名と rules 行 id・記録行 1 件／planner の target から同じ command → allow・記録行 1 件／登録の無い pane → deny／pane 無し → 通す（記録なし）／Edit: 管理席の code path → deny・planner の design-intent → allow・契約の印で開いた便の write-set の内側 → allow・外 → deny（AC16）／権能付きでない Bash / Edit は通す。
- rules: 役割ごとの行の kind 件数 +1・値が列であること・列に無い名は `RuleError`・R-C7-1 の値の型（Str → Role の名）・rules 外形 snapshot。
- 注入（hook.rs）: 登録済みの target の SessionStart で生成文が出て権能の名がすべて含まれる・登録の無い target で 0 byte・雛形に pointer の無い行を置いた fixture で xtask check が落ちる（AC17）・行に在って文に無い権能を作った fixture で落ちる・生成文の外形 snapshot。
- 極性一覧 snapshot に `Register`（(a)）と `Role`（(b)）の 2 行（件数 +2・N = K + M の pin）・doctor の項目 1 行（`--state-dir` 付きの外形 snapshot）。
- property（`prop_role_`・in-file）: `Role` / `Capability` / `PathKind` の `as_str` ↔ parse が往復し、列に無い名は必ず Err。
- **外形 snapshot と歯の file の置き場**（`s2-07l.327`）: seat の外形 snapshot は面ごとに 1 file（usage / rebrief の DATA / doctor の末尾＝`seat_usage_external_form` / `seat_rebrief_external_form` / `seat_doctor_external_form`・旧 `seat_external_form` は消す）、`tests/e2e/seat/` の歯の file は接頭辞（責務）ごとに 1 file（module `account` = `seat_account_` + `seat_tick_` + `doctor_accounts_`〔doctor が口座を照合する歯・口座の面〕・module `launch` = `seat_launch_` + `seat_restore_` + `seat_attrib_`・module `register` = `seat_register_` + `seat_role_` + `seat_state_`・module `rules` = `seat_rules_` + `rules_host_` + nested module `rules_prop`・分割は `s2-07l.361`・契約表の行 b。母集団は移す前の `seat::account::` の本数を `cargo nextest list` の module 名義で数え、移した後は 4 module の合計がそれと一致する＝接頭辞で数えない）。共有 helper は `seat.rs` の `pub(super)` に置き複製しない。pipe が外形を面ごとに分けている形と同じ。

## 8. 憲法・制約との整合

C1 / C5（権能の値は行・裁定 id）・C1.2（生成文に手書きの規範文 0・xtask が検査）・C2（Role / Capability / PathKind は closed enum・宣言順）・C2.2（env を読まない・identity は `--pane` の引数）・C3 / C3.3（登録は event log の replay・typed）・C7 / C7.2（対話面は R-C7-1 の値・承認は planner の席から）・C11.2（Guard は 1 極性）・C12.5（生成文と rules 表は snapshot）・C14 / C14.2（2 面と drift 検査）・C16 / C16.2（編集時に止める in-loop guard・極性一覧）・N2 / N3（散文と host の慣習を入力にしない）。

## 9. 契約（4 便・この順・実装は pipeline）

- **(a) 役割と登録**（M）: `Role`・`seat register`・`EventKind::SeatRegistered`（`Event` の束 `Registration`）・打刻の条件・`Guard::Register`・役割の解決 1 本・doctor の口と項目・歯 §7 の登録 / 解決。write-set = seat/（新 module `seat/role.rs`・極性の定数）・fleet/mod.rs（variant・`Event` の束・`KINDS`）・fleet/usage.rs・fleet/cli.rs・pipe/mod.rs（`Event` の literal 構築点）・main.rs（doctor の `--state-dir` と項目）・polarity.rs（`Guard::Register`）・tests/e2e/{seat,fleet,prop,polarity}.rs（構築点・`KINDS` の件数の pin・N = K + M）・snapshot（doctor・極性一覧）。依存: [working-memory.md](./working-memory.md) 契約 (a)（s2-07l.139・打刻の sid の読み手）の land 後。
- **(b) 権能と執行**（M）: `Capability`・`RuleKind::RoleCapabilities`（値は既存の `List`）・役割ごとの rules 行（**値と裁定 id は user 裁定**）・R-C7-1 の値の変更（裁定 id）・`PathKind`・`Guard::Role`・PreToolUse の 2 面・hooks.json の matcher と `--pane`・契約の印（field 名）・deny 文・記録行・歯 §7 の guard / rules。write-set = hook/（新 module `hook/role_guard.rs`）・polarity.rs・rules/mod.rs・rules/manifest.toml・xtask/genmanifest.rs・hooks/hooks.json（tracked な生成物・gen-manifest の出力・歯が読む）・pipe/declaration.rs（印の field）・tests/e2e/{hook,rules,polarity}.rs・snapshot。依存: (a)。
- **(c) 注入と検査**（M）: 雛形 2 枚・SessionStart の生成文・xtask の検査 3 つ・外形 snapshot・歯 §7 の注入。write-set = seat/brief/・hook/mod.rs・xtask/check.rs・tests/e2e/hook.rs・snapshot。依存: (b)・PointerKind（s2-07l.139）。
- **(d) 器の外の散文の撤去**（運用・便ではない）: (b)(c) の land 後、user の設定 file の役割行・共有 skill の役割節・退避物の役割の命令行の同文を A1 の「消す」として user に確かめてから外す（ADR-0022 §2.7）。
- **(e) 登録 row の `model` 項目**（S）: `seat register --model M`（任意）・`Registration` の束に `model: Option<String>`（display name・schema 1 のまま値の追加・無い row は None）・replay の読み手・doctor の項目列に 1 欄。write-set = seat/cli.rs（口）・seat/role.rs（束と読み手）・fleet/mod.rs（`Event` の束の項目）・pipe/mod.rs と tests/e2e/{seat,fleet,prop}.rs の `Event` / `Registration` の literal 構築点・main.rs（doctor の欄）・snapshot（doctor が在れば）。依存: (a) の land 後。読み手 = [account-autonomy.md](./account-autonomy.md) §3（session 用の model）/ §5（tick と立て直し）。

## 10. 却下案（ADR-0022 §5 の写しは持たない・設計固有のもの）

- 登録 row を席の state dir の file に置く。却下: 状態の置き場が 2 つになる（C3）・anchor をまたぐ突合ができない。event log の replay 1 本。
- 権能の行の値を Bool の行の束（`role.planner.answer = true` …）で持つ。却下: 権能の種類ごとに行が増え、列挙が manifest に散る。値は名の列 1 行。
- PreToolUse の Bash 面を PermissionRequest に寄せる。却下: PermissionRequest は許可の問い合わせであって編集時の deny ではない（C16）。
- 雛形を markdown の skill として同梱する。却下: ADR-0022 §5 (A)。

## 11. 後続

席の起動と登録の自動化（s2-07l.38）・plugin を積まない session の guard（s2-07l.149）・席の model の割当（別の裁定）・役割ごとの権能の値の改訂（裁定 id 付きの行の変更）。

## 12. 壁時計依存の inject の歯（契約表の行 f・`s2-07l.342`）

- 何が起きているか: `crates/scribe2/tests/e2e/seat/cycle.rs` の inject の歯は、pane の script が `stty -echo` を終える前に inject が届くと tty の echo で「届いた」に化ける（gate 3 本同時の負荷で timing が動き、単独では緑・.304 run 1 の変異 baseline で 1 本落ちた・s2-07l.334 と同じ型）。壁時計の等号を pin する fixture は flaky（C12.6）。
- 形: pane の script は `stty -echo` の後に固定の合図（1 行の sentinel）を出し、歯は capture-pane の polling（上限付き・壁時計の等号を pin しない）で合図を見てから inject を撃つ。同じ file の壁時計依存の歯（sleep を pane の script に持ち送達の有無を時間で測るもの）を同じ形に揃える（母集団 = 該当した歯の本数を notes へ）。本体（`seat/inject.rs` 等）は触らない。
- 検証の形: 本体不変の歯だけの便＝base で RED を作れないので `// flip-check: retroactive s2-07l.342` の札で flip-check を通す（pipeline.md §5.3 の対の規則・account-autonomy.md §12 / gate-cost.md §20 と同じ型）。改名は RED の根拠にしない（名は `seat_inject_` の接頭辞を保つ）。負荷下の再現は `--test-threads 8` で 5 周回して赤 0（母集団 = 5 周 × 本数）を notes へ。

## 13. role guard の断りの理由を閉じた enum に・代替ルートを添える（契約表の行 g・`s2-07l.308`）

- 何が起きているか: 未登録の席の Write が reason=unregistered で deny され、deny 文が seat register を名指さないので source を読まないと解けなかった（folio2 planner の観測 2026-09-15）。止めるのは設計どおり（fail-closed・ADR-0022 §2.1）で、代替ルートを持たないのが穴。現物: `crates/scribe2/src/hook/role_guard.rs` の decide の断りの理由は素の文字列 6 種（target-unresolved / registry-unreadable / unregistered / rules-unreadable / no-row / no-anchor）・deny 文は 1 形。
- 形: 断りの理由を閉じた enum RefuseReason（TargetUnresolved / RegistryUnreadable / Unregistered / RulesUnreadable / NoRow / NoAnchor・宣言順の const slice・as_str = 現行の字面）にし decide は variant を返す。各 variant が代替ルートの 1 行 route を持ち（Unregistered = seat register の形・NoAnchor = anchor の解決の口・RegistryUnreadable / RulesUnreadable = doctor の口）、deny 文の末尾に route= の 1 句を足す。
- 触らない: 判定の順序と極性（fail-closed）・登録の口・deny 文の前半（理由の字面は不変）。
- 却下: deny 文に散文で手順を書く（理由ごとに違う route を 1 形の文に押し込むと散文の規則になる・N2）／未登録を allow に倒す（fail-closed を崩す）。

## 14. 相談席 consult — 相談・調査・実験の席（契約表の行 h・`s2-07l.430`）

- 何が起きているか（user の要望 2026-09-17・逐語は台帳 `s2-07l.430`・裁定 id user 2026-09-17T01:45Z / 01:48Z）: 相談・調査・OSS の試用（例: 依存の候補を実際に動かして測る）を planner に兼ねさせると、planner が契約の焼き直しで詰まった日に相談が止まる。第 3 の役割を置き、**開発の本線と pipeline を汚さない**ことを権能の集合（§3・rules 行）で機械に守らせる。
- 形（§2 の役割の形に席を 1 つ足すだけ・ADR-0022 §2.1〜§2.5 は不変）:
  1. **`Role` の variant 1 つ** `Consult`（宣言順の末尾・`parse` / `as_str` / 網羅 match の消費側）。登録・起動（`seat register` / `seat launch --role consult`）・tick・rebrief・SessionStart の役割の解決は §2 の 1 本のまま。
  2. **rules 行 `role.consult`**（kind `RoleCapabilities`・値 = `["relay", "edit-outside", "edit-research"]`・裁定 id user 2026-09-17T01:48Z・C5）。中継（planner / 管理席へ結論を送る）・repo の外の編集（実験の作業場）・research 文書の編集の 3 つだけ。回答・承認・go・便の起動・merge・契約の編集（台帳の write）・code / 設計 doc / design-intent（research 以外）の編集は持たない＝§4 の guard が Edit / Write と `pipe` の口を止める。裁定の持ち込み先（R-C7-1）は planner の席のまま。
  3. **`Capability` の variant 1 つ** `EditResearch` と `PathKind` の variant 1 つ `Research`（`design-intent/research/` の段・`DesignIntent` より先に判定する＝1 関数の中の宣言順で決め、prose の順序注記を持たない・C2）。planner の行は `edit-design-intent` を持つので research も従来どおり書ける（`EditDesignIntent` は `Research` の段も通す＝上位の権能）。
  4. **brief の雛形 1 枚** `seat/brief/consult.txt`（§5 の規律・穴と pointer 付きの行だけ）: 第一手の復元・相談と調査の作法（repo と台帳は読むだけ・実験は repo の外の作業場・結論と実測は planner へ relay・research 文書は docs PR で出す・依存の候補を器に入れる話は A3）・席間の連絡の経路（FR44）・3 クラスの発火の pointer。xtask の検査（§5・穴 ⊆ 定義済み・pointer 無しの行 0・権能の名が全部現れる）と外形 snapshot はそのまま 3 枚目に掛かる。
  5. **触らない**: planner / admin の行と値・R-C7-1・`pipe` の口・§13 の断りの理由（`RefuseReason` は増やさない・consult が止められる周も既存の variant で足りる）。
- 却下: planner に相談を兼ねさせる（今日の詰まりの再発）／consult に `edit-contract` を渡す（台帳の書き手が 2 席になり契約の字面の事故の口が増える）／repo 内に `lab/` を切る（PUBLIC・CON2・実験物が tracked に漏れる）／`edit-design-intent` を渡す（spec / decisions まで書ける・広すぎる）。
- 歯（`seat_role_consult_` 接頭辞・`tests/e2e/seat.rs` と `tests/e2e/hook.rs`）: (a) `role.consult` の行が manifest に在り `RuleKind` の `ALL` と `rules validate` の外形に載る／(b) consult の登録 row を持つ席の Edit が `design-intent/research/x.html` を通し `design-intent/spec/x.html` と `docs/design/x.md` と crates 配下の Rust file を権能の名を告げて断る（planner の席は research も spec も通る）／(c) consult の席の `pipe answer` / `pipe run` が権能で断られる／(d) SessionStart の brief が consult の雛形から生成され外形 snapshot に載る。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "seat の外形 snapshot を usage / rebrief / doctor の 3 面に割る（旧 snapshot は消し、他 doc の行の名指しを面ごとの file 名へ）"
req = ["FR23", "FR59"]
section = "7"
write-set = ["crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_rebrief_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "docs/design/seat-roles.md", "docs/design/seat-autonomy.md", "docs/design/dispatcher.md", "docs/design/working-memory.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_usage_external_form seat_rebrief_external_form seat_doctor_external_form"]
size = "S"
done = "seat の外形 snapshot が 3 file・歯の本数は移動前 + 2・旧 snapshot は消えて未参照 0・他 doc の行が面ごとの file 名を名指す"

[[contract]]
id = "b"
title = "e2e/seat/account.rs を接頭辞ごとの 3 module（launch / register / rules）に割る — 純移動・lens には move_proof の要約が渡る"
req = ["FR23", "FR59"]
section = "7"
write-set = ["crates/scribe2/tests/e2e/seat.rs", "-crates/scribe2/tests/e2e/seat/account.rs", "+crates/scribe2/tests/e2e/seat/launch.rs", "+crates/scribe2/tests/e2e/seat/register.rs", "+crates/scribe2/tests/e2e/seat/rules.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat::launch:: seat::register:: seat::rules::"]
size = "S"
done = "account.rs が seat_account_ と seat_tick_ と doctor_accounts_ だけになり、3 module に歯が移って 4 module の合計が移す前の seat::account:: の本数と一致し中身も不変、gate の lens 入力が diff でなく要約"

[[contract]]
id = "c"
title = "runner / lens の起動の包み 2 口で TMUX_PANE を外す — pipe の外の単体起動でも席の打刻と plugin 記録に混入しない"
req = ["FR40", "FR21"]
section = "4"
touches = ["crate::pipe::confine::Confinement"]
tests = ["crates/scribe2/tests/e2e/headless.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail headless_runner_drops_tmux_pane headless_lens_drops_tmux_pane"]
size = "S"
done = "席の pane の中から runner / lens を単体起動しても claude の env に TMUX_PANE が無く、PATH は継承され、wrap_command と wrap_line の両方が同じ 1 点で外す"

[[contract]]
id = "f"
title = "壁時計依存の inject の歯を pane の sentinel 待ちに — gate の同時走行で delivered に化ける flaky を fixture の競合の除去で塞ぐ（本体は触らない）"
req = ["FR44", "FR29"]
section = "12"
write-set = ["crates/scribe2/tests/e2e/seat/cycle.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_inject_"]
size = "S"
done = "pane の script が固定の合図を出してから inject を撃つ形に歯が揃い、同じ file の壁時計依存の歯が同じ形になり（母集団は notes・改名後の名は全部 seat_inject_ の接頭辞を保つ）、負荷下 5 周で赤 0"

[[contract]]
id = "g"
title = "role guard の断りの理由を閉じた enum RefuseReason にし、各 variant が代替ルートの 1 行を持って deny 文の末尾に route= を添える"
req = ["FR45", "FR40"]
section = "13"
write-set = ["crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/tests/e2e/hook.rs", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_role_guard_route_"]
size = "S"
done = "role guard の deny 文が理由ごとの代替ルートを 1 行で名指し、理由の字面と判定の順序と極性は不変、6 variant の宣言順が pin され route が全部非空"

[[contract]]
id = "h"
title = "相談席 consult — Role の variant 1 つ・rules 行 role.consult = [relay, edit-outside, edit-research]・Capability と PathKind に research の variant 1 つずつ・brief の雛形 1 枚（裁定 user 2026-09-17T01:48Z）"
req = ["FR40", "FR45", "FR44"]
section = "14"
write-set = ["rules/manifest.toml", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/src/seat/brief/mod.rs", "+crates/scribe2/src/seat/brief/consult.txt", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "crates/scribe2/tests/e2e/snapshots/"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_role_consult_"]
size = "S"
done = "consult の登録 row を持つ席が research の文書と repo の外だけ Edit でき、spec / 設計 doc / code と pipe の口は権能の名を告げて断られ、planner と admin の席の挙動と brief は不変で、consult の brief が外形 snapshot に載る"
<!-- contracts:end -->
