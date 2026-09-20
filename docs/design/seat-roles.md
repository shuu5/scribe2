# 設計: 席の役割と権能 — 役割は閉じた enum、登録は fleet の event、権能は rules 行、執行は PreToolUse の guard、注入は SessionStart の生成文

- 要件: [FR40](../../design-intent/spec/srs.html#FR40) 席の登録 / [FR41](../../design-intent/spec/srs.html#FR41) 権能の所在 / [FR45](../../design-intent/spec/srs.html#FR45) 権能の執行 / [FR42](../../design-intent/spec/srs.html#FR42) 席の指示文の注入 / [FR30](../../design-intent/spec/srs.html#FR30) 配送構造 / [AC15](../../design-intent/spec/srs.html#AC15) [AC16](../../design-intent/spec/srs.html#AC16) [AC17](../../design-intent/spec/srs.html#AC17)・既存 [FR17](../../design-intent/spec/srs.html#FR17) / [FR18](../../design-intent/spec/srs.html#FR18) / [FR20](../../design-intent/spec/srs.html#FR20) / [FR22](../../design-intent/spec/srs.html#FR22) / [NFR5](../../design-intent/spec/srs.html#NFR5)
- 決定: [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html)（本 doc の決定の正本・§2.1〜§2.8）/ [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html) §2.2（pane → target）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.1（列挙は core・文書は pointer）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1（guard の定義）/ [ADR-0018](../../design-intent/decisions/ADR-0018-working-memory-subcommands-and-pointer-required-directives.html) §2.2（出所 pointer）
- 土台: [seat-state.md](./seat-state.md)（打刻と pane → target）・[vessel-hook.md](./vessel-hook.md)（hooks.json の生成と guard の束）・[working-memory.md](./working-memory.md) §4（PointerKind）・[rules-manifest.md](./rules-manifest.md)・[polarity.md](./polarity.md)・[fleet-event-log.md](./fleet-event-log.md)
- 位置づけ: 役割の規律を憲法 C14 の 2 面（文書 = SRS / ADR・manifest = 権能の行）に収め、執行・注入・生成文書・drift 検査が**同じ行**を読む。散文の役割節（user の設定 file・共有 skill・退避物の命令行）は規則の置き場ではない（ADR-0022 §2.7・撤去は A1 の「消す」として別途 user に確かめる）。

## 1. 何を解くか

役割の規律は文書（SRS FR30〜32・ADR-0016）と散文 7 面にしか無く、器は役割の型を持たない。散文でしか止まっていない事故型は 6 つ（ADR-0022 §1 (a)〜(f)）。本設計は (a)(b)(c)(e) を guard で止め、(d) を記帳の deny で効かなくし、(f) は [working-memory.md](./working-memory.md) の内蔵に委ねる。consumer の repo は席の登録だけで同じ執行と注入を得る（CLAUDE.md に役割の文を書かない）。

## 2. 役割と登録（ADR-0022 §2.1）

- **`Role`**（closed enum・core）: variant の列挙は core が持ち文書は写さない。記録時点の値は 1 つ（orchestrator・[ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (1)）。`as_str` / `ALL` / 判別子順の pin は既存の enum（`RuleKind` / `Guard`）と同じ形。
- **登録の subcommand**: `<NAME> seat register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR] [--model M]`。`--model M` は席が使う model の display name（任意・[account-autonomy.md](./account-autonomy.md) §3 の session 用の入力・無い周は None＝保守側・契約 (e)）。`--anchor` の既定は cwd の repo root（`git rev-parse --show-toplevel`・env を読まない）。`--launch FILE` は起動の雛形（穴は口座の credential dir 1 つ・[account-autonomy.md](./account-autonomy.md) §5 が使う）で、内容を event に載せる（tracked file に置かない・CON2）。
- **event**: `EventKind::SeatRegistered`（末尾・宣言順）1 件。項目 = `role` / `anchor` / `target` / `sid`（登録を撃った session の id・SessionStart の打刻から解く）/ `account` / `launch`（雛形の本文）/ `model`（任意・席が使う model の display name・契約 (e)・無い row は None）。項目は `Event` の typed な束 1 つ（`allowance` と同型の `Option<Registration>`・kind ではなく束の有無が本体を決める）で持つ＝`Event` を literal で組む既存の構築点（core・歯・property の生成器）と `KINDS` の件数の pin がすべて変わる（write-set は §9 (a)）。schema 1 のまま（値の追加）。
- **鍵と置き換え**: 鍵 = (role, anchor)。同じ鍵の再登録は前の row を置き換える（append のみ・replay の最新が効く・N1）。1 つの anchor に役割ごとに 1 席（FR40・役割は 1 つなので anchor が鍵を分ける）。`target` / `sid` / `account` は項目で鍵ではない。pane id は鍵にも項目にも置かない。**書き手は 2 つ**: `seat register`（席の session が撃つ・打刻の条件付き）と tick の口座更新（[account-autonomy.md](./account-autonomy.md) §5・器の内部の同じ 1 関数・`target` / `sid` / `launch` は既存 row から写す）。3 つ目 = `seat launch`（器が起動行を導出して row を先に書く・`sid` は無し・[account-lifecycle.md](./account-lifecycle.md) §4・ADR-0026 §2.3）。
- **登録を受ける条件**: 登録を撃った session に SessionStart の打刻（[seat-state.md](./seat-state.md) §2）が在ること。打刻の無い session（plugin を積まない・tmux の外）からの登録は typed な理由（`RegisterRefusal::NoStamp`）で断る＝guard の無い席が権能を持てない。登録そのものは権能を要しない（登録が先・ADR-0022 §2.5）。
- **役割の解決**（1 本・読み手は guard / 注入 / doctor / tick）: `pane → target（ADR-0015 §2.2）→ replay を鍵 (role, anchor) ごとに最新 row へ畳んでから `target` が一致する row を引く（同じ鍵の旧 row は旧 target では解けない・複数の鍵が同じ target を持てば replay の最新）→ role`。`anchor` は row の項目として読むだけで、hook の cwd と突合しない（席が worktree へ cd した周も同じ row が解ける）。window 名は target の一部（`session:window`）としてだけ効き、名前の慣習で役割を決めない。env・作業木の path・pane の字面は入力にしない（C2.2 / C3.3 / N3）。window を rename した席は別の target＝登録し直す（doctor の突合が `missing` で出す）。replay の cache は持たない（C3・hook の予算 NFR5 の内側で実測済み）。
- **doctor**: 登録 row と実在の target（tmux の `list-panes`）の突合を項目に持つ（C3.2・値は生成物）。`doctor` の現物は bin crate の `render_doctor`（記録時点は name / version の 2 行・state dir の引数なし）なので、`--state-dir S` の口を足し項目列に 1 行足す（外形 snapshot `doctor_external_form` が変わる）。

## 3. 権能と rules 行（ADR-0022 §2.2 / §2.5）

- **`Capability`**（closed enum・core）: 操作の種別。記録時点の variant の**種類**は次のとおりで、名は core が持つ: 回答の記帳（`pipe answer`）・承認の記帳（approval event）・go の記帳（merge の許可）・便の起動（`pipe intake` / `run` / `resume` / `retire`・名指しでない `stop`）・go 後の merge・便 1 本を名指す停止（`pipe stop --run <id>`・§25）・path 種別ごとの編集（design-intent / 設計 doc / 歯〔`crates/<crate>/tests/`〕/ code / 対象 repo の外）。中継の variant は持たない（[ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (1)・機械の面が無く席が 1 つになって意味を失う）。便の起動と merge は器の dispatcher の口ゆえ席の行には並ばない（variant は guard が deny を出すために残る）。
- **停止の権能**: 便を止める口（`pipe stop`）を起動の権能（launch）から外して新しい権能 `stop` に結び、rules 行 `role.orchestrator` の値に `stop` を足す形は [ADR-0048](../../design-intent/decisions/ADR-0048-stopping-a-run-is-a-separate-capability-of-the-orchestrator.html) が決めた（席が撃てるのは便 1 本を名指す形だけで `--all` と名指しの無い形は launch のまま）。実装の形は §25（契約表の行 s・`s2-07l.495`）＝権能の列の 1 語・行の値の 1 語（裁定 id `user 2026-09-20`）・guard の表の 1 行と名指しの窓の照合。
- **権能の名の対応**: `edit-tests` は [ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (1) の「`edit-code`〔`tests/` の path 種別に限る〕」である。名を分けるのは §4 の guard が path 種別と権能を 1:1 で照合するからで（値に修飾子を持たせると読み手を 1 本足すことになる）、意味は ADR のとおり＝src は行に無く歯だけが開く。
- **rules 行**: `RuleKind::RoleCapabilities`（variant 1 つ・値は権能の名の**列**〔既存の `RuleValue::List`・manifest に list の行が既に在る〕・裁定 id 付き）を役割ごとに 1 行（id は `role.<役割名>`）。値（どの役割がどの権能を持つか）は本 doc が決めず、契約 (b) が裁定 id 付きで書く（§9）。列に無い名は manifest の読み込みで拒む（閉じた enum の parse・既存の `RuleError`）。
- **読み手は 4 つ・行は 1 つ**: §4 の guard・§5 の注入文の生成・C1.2 の生成文書（rules 表）・xtask の drift 検査（C14.2）。
- **R-C7-1**: kind は既存の `Dialogue`（対話面）のまま、値は「役割 orchestrator の登録 row を持つ席」を表す typed な値（`Role` の名）である（行の値・裁定 id）。承認 event / 回答 event / go の記帳はその席からだけ受理（§4 の Bash guard が権能の行で止める・AC15）。
- **契約が開く例外**: 席が自分の手で code を編集してよい便は、契約 file の typed な印（既存の `classes` と同じ形の field・`opens`）で表す。§4 の Edit guard はその便の write-set の内側だけ通す（AC16）。散文に置かない。

## 4. 執行（ADR-0022 §2.3）

- **`Guard::Role`**（variant 1 つ・宣言順は `Register` の直後〔`Cap` → `Register` → `Role`＝登録が先で執行が後の行為の流れ〕・InLoop・FailClosed・極性一覧に 1 行）。
- **2 面**: (1) **Bash** — command 行が権能付き subcommand（core の const slice `CAPABILITY_COMMANDS`: subcommand の名 → `Capability`）を含む周に、席の役割の行がその権能を持たなければ deny。(2) **Edit 系** — path の種別（`PathKind`: design-intent / 設計 doc / 歯〔`crates/<crate>/tests/`〕/ code / 対象 repo の外・closed enum・分類は repo root からの相対 path の prefix）ごとの権能を照合し、持たなければ deny。契約が印で開いた便の write-set の内側は通す。
- **種別に属する path の出所**: 種別の集合（closed enum）と種別ごとの権能の 1:1 は本 § のままで、種別に属する path の集合を対象 repo の vessel 宣言の任意 key で名乗る形は [ADR-0047](../../design-intent/decisions/ADR-0047-path-kinds-are-declared-in-the-vessel-declaration.html) が決めた（proposed・発効は宣言の key 3 本と guard の読み口が land した版。発効までの分類は本 § の固定 prefix のまま）。
- **repo の写し**: `.worktrees/` 直下の worktree は便の worktree（`.worktrees/<NAME>/<run>/`）に限らず repo の写しで、`PathKind` はその worktree からの相対 path で分類する（席が docs PR 用に切る `.worktrees/<name>/` の `design-intent/` も DesignIntent）。契約の印が開くのは便の worktree だけ・`.worktrees/<name>` そのものは code（s2-07l.227）。
- **identity**: 生成 hooks.json の shell 行が渡す `--pane` だけ（[vessel-hook.md](./vessel-hook.md)・生成器は同じ gen-manifest）。PreToolUse の shell 行に `--pane "$TMUX_PANE"` を足し、matcher を `Edit|Write|MultiEdit|NotebookEdit` から **Bash を含む形**へ改める（Bash は PermissionRequest の matcher でもある・2 面の判定は別 hook event）。
- **解く順**: anchor → pane → target → 登録 row → role → 行 → 権能。**anchor（repo root・state dir）は payload の `cwd` でなく、生成 hooks.json の shell 行が渡す `--project`（session の起動 dir・Claude Code が hook の command に与える project dir・席が `cd` しても変わらない・`--pane` と同型で binary は env を読まない〔C2.2〕）から解く**。`--project` が無い周（旧 hooks.json）は `cwd` で解く（互換・生成物の更新で消える）。pane が空（tmux の外・runner / lens）は席ではなく本 guard の対象外（ADR-0009 の write-set guard と allowlist がそのまま担う）。**pane が在るのに anchor が解けない（root が無い・`served` が `ByMe` でない・state dir が無い）周と、登録 row が無い・target が解けない周は権能なし＝権能付きの操作を deny（FailClosed・理由を stderr に 1 行・記録 1 行）**。[vessel-hook.md](./vessel-hook.md) の「仕えない周は黙る」（FR24）は pane が無い周にだけ当たる（席が repo の外へ `cd` しても guard は外れない）。止めるのは権能付きの操作だけで、それ以外の Bash / Edit は通す。
- **deny 文**: 欠けた権能と rules 行の id を含む（例の形: `<NAME>: この操作（<権能>）は席の権能でない（rules 行 <id>）`・字面は現物が正本・役割が 1 つなので「他の役割が持つ」形は持たない）。**記録行**: allow の周も target と command の種別を 1 行（hook の消費記録と同じ置き場 `<state_dir>/inject.jsonl`・[vessel-hook.md](./vessel-hook.md)。打刻の `state.jsonl` には書かない）。
- **subcommand は役割を検査しない**（引数の identity は偽装できる）。発話は監視しない。
- **runner / lens は pane を持たない（起動の包みの 2 口で同じ）**: 便の runner / lens / verify 行は行の包み（`confine::wrap_line`）が起動側の `TMUX_PANE` を外す（s2-07l.216）。`headless/mod.rs` の `build`（runner と lens の唯一の構築点・pipe の外から `<NAME> runner` / `<NAME> lens` を単体起動した周もここを通る）は command の包み（`confine::wrap_command`）を通り、こちらは `TMUX_PANE` を外していなかった＝席の pane の中から単体起動した runner の hook が `--pane` で**その席の打刻と読み込み元の記録**に混入する（2026-09-15 15:23Z・別 repo の管理席が run の plugin 写しで runner を単体起動し、席の `plugin` 記録が run dir を指した）。**2 つの包みは同じ 1 点で `TMUX_PANE` を外す**（`wrap_command` に置き `wrap_line` はそれを通る・外す名は 1 つの const・足す env は無い・C2.2）。runner は席ではない（FR40）ので hook は `--pane` 空で黙る（既存）。

## 5. 注入（ADR-0022 §2.4）

- SessionStart の hook（[vessel-hook.md](./vessel-hook.md)）が pane → target → 登録 row で役割を解き、役割ごとの **tracked な雛形 1 枚**（`headless/runner.txt` と同じ形・binary に埋め込む・`seat/brief/<役割名>.txt`）から生成した指示文を stdout で注入する。登録の無い席は 0 byte（断りも出さない）。
- **雛形の行の規律**: 行は「穴」か「出所 pointer を持つ行」に限る。穴 = `{capabilities}`（権能の行の値の列）/ `{target}` / `{anchor}` / `{role}` / `{ledger}`（台帳の現在値・読めない周は `unknown`＝数に化けさせない・C10。読みは `bd --readonly list --json` の子 process 1 回で、待ち上限は rules 行 `seat.ledger_timeout_s`。置き場は席の子 module 1 枚＝`s2-07l.479.2` の純移動で復元の DATA と共用していた `seat/rebrief.rs` から移した）。pointer の形は `PointerKind`（憲法の id・ADR の節・SRS の要件 id・rules 行の id）で、分類と anchor での解決は指示文の子 module が持つ（`s2-07l.479.2` の純移動: 退避物の命令行と共用していた `seat/wm.rs` から、読み手の残る側だけを指示文の隣へ移した）。**規範文の定義 = pointer を持たない行**（typed・字面の語彙で判定しない）。
- **雛形が持つもの**（[ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (3)・全部で 11 行）: 席の同一性 3 行（役割 / 権能の行の値 / 台帳の現在値）・憲法の効く部分 5 行（順位・A1・A4.2・A2 と A3・N1〜N3）・役割の特性 3 行（対話面の作法と信頼度・実装を自分で行わない・決定はしご）。**C 条文は注入しない**（CI の門と PreToolUse の guard が執行する・[ADR-0046](../../design-intent/decisions/ADR-0046-constitution-is-enforced-by-gates-and-only-ask-first-and-never-are-injected.html) §2）。§4 で塞ぐ事項（回答・承認・go・merge・code の Edit）は書かない（二重化しない）。
- **席間の連絡の行**（FR44・AC19）: 席が 1 つになったので雛形は席間の連絡の行を持たない（[ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (3)）。器はこの経路を持たない（FR44 の経路設計は ADR-0022 §2.8 の射程外＝道具の機能をそのまま使う）。
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
- guard（hook.rs・偽 tmux で pane → target を返す stub）: 行に無い権能（`pipe run` の起動）を含む Bash → deny・deny 文に欠けた権能と rules 行 id・記録行 1 件／行が持つ `pipe answer` → allow・記録行 1 件／登録の無い pane → deny／pane 無し → 通す（記録なし）／Edit: 管理席の code path → deny・planner の design-intent → allow・契約の印で開いた便の write-set の内側 → allow・外 → deny（AC16）／権能付きでない Bash / Edit は通す。
- rules: 役割ごとの行の kind 件数 +1・値が列であること・列に無い名は `RuleError`・R-C7-1 の値の型（Str → Role の名）・rules 外形 snapshot。
- 注入（hook.rs）: 登録済みの target の SessionStart で生成文が出て権能の名がすべて含まれる・登録の無い target で 0 byte・雛形に pointer の無い行を置いた fixture で xtask check が落ちる（AC17）・行に在って文に無い権能を作った fixture で落ちる・生成文の外形 snapshot。
- 極性一覧 snapshot に `Register`（(a)）と `Role`（(b)）の 2 行（件数 +2・N = K + M の pin）・doctor の項目 1 行（`--state-dir` 付きの外形 snapshot）。
- property（`prop_role_`・in-file）: `Role` / `Capability` / `PathKind` の `as_str` ↔ parse が往復し、列に無い名は必ず Err。
- **外形 snapshot と歯の file の置き場**（`s2-07l.327`）: seat の外形 snapshot は面ごとに 1 file（usage / doctor の末尾＝`seat_usage_external_form` / `seat_doctor_external_form`・旧 `seat_external_form` は消す・復元の DATA の面は `s2-07l.479.2` で DATA ごと消えた）、`tests/e2e/seat/` の歯の file は接頭辞（責務）ごとに 1 file（module `inject` = `seat_inject_`・module `account` = `seat_account_` + `doctor_accounts_`〔doctor が口座を照合する歯・口座の面〕・module `launch` = `seat_launch_` + `seat_restore_` + `seat_attrib_`・module `register` = `seat_register_` + `seat_role_` + `seat_state_`・module `rules` = `rules_host_` + `seat_rules_`・分割は `s2-07l.361`・契約表の行 b。母集団は移す前の `seat::account::` の本数を `cargo nextest list` の module 名義で数え、移した後は **5 module**（`s2-07l.479.1` で `tick` / `cycle` が消え `inject` が出来た）の合計がそれと一致する＝接頭辞で数えない）。共有 helper は `seat.rs` の `pub(super)` に置き複製しない。pipe が外形を面ごとに分けている形と同じ。

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

## 12. 壁時計依存の inject の歯（契約表の行 f・`s2-07l.342`）— **超過**（ADR-0045 §2 (2)）

- 本節が形を揃えた歯（`tests/e2e/seat/inject.rs` の `seat_inject_` の族）は、測っていた `seat inject` の口ごと
  `s2-07l.479.3` で消えた。行 f は着地済みで、write-set の file が無くなったので表から落とした。本文は git の履歴に在る。
- **残る規律**（file を跨ぐ）: 壁時計の等号を pin する fixture は flaky（C12.6）＝合図を待つ形にする。この規律は
  [gate-cost.md](./gate-cost.md) の検出線と憲法 C12.6 が持ち、本 doc は持たない。

## 13. role guard の断りの理由を閉じた enum に・代替ルートを添える（契約表の行 g・`s2-07l.308`）

- 何が起きているか: 未登録の席の Write が reason=unregistered で deny され、deny 文が seat register を名指さないので source を読まないと解けなかった（folio2 planner の観測 2026-09-15）。止めるのは設計どおり（fail-closed・ADR-0022 §2.1）で、代替ルートを持たないのが穴。現物: `crates/scribe2/src/hook/role_guard.rs` の decide の断りの理由は素の文字列 6 種（target-unresolved / registry-unreadable / unregistered / rules-unreadable / no-row / no-anchor）・deny 文は 1 形。
- 形: 断りの理由を閉じた enum RefuseReason（TargetUnresolved / RegistryUnreadable / Unregistered / RulesUnreadable / NoRow / NoAnchor・宣言順の const slice・as_str = 現行の字面）にし decide は variant を返す。各 variant が代替ルートの 1 行 route を持ち（Unregistered = seat register の形・NoAnchor = anchor の解決の口・RegistryUnreadable / RulesUnreadable = doctor の口）、deny 文の末尾に route= の 1 句を足す。
- 触らない: 判定の順序と極性（fail-closed）・登録の口・deny 文の前半（理由の字面は不変）。
- 却下: deny 文に散文で手順を書く（理由ごとに違う route を 1 形の文に押し込むと散文の規則になる・N2）／未登録を allow に倒す（fail-closed を崩す）。

## 14. 相談席 consult — 相談・調査・実験の席（`s2-07l.430`）— **超過**（ADR-0045 §2 (2)・役割は orchestrator の 1 つ）

> 本節は [ADR-0045](../../design-intent/decisions/ADR-0045-seat-role-is-one-orchestrator-and-dispatcher-lands-runs.html) §2 (1)（席の役割は orchestrator 1 つ）が超過した。起票されていない提案として残す（中継 `relay` の権能も `s2-07l.478` で消えている）。

- 何が起きているか（user の要望 2026-09-17・逐語は台帳 `s2-07l.430`・裁定 id user 2026-09-17T01:45Z / 01:48Z）: 相談・調査・OSS の試用（例: 依存の候補を実際に動かして測る）を planner に兼ねさせると、planner が契約の焼き直しで詰まった日に相談が止まる。第 3 の役割を置き、**開発の本線と pipeline を汚さない**ことを権能の集合（§3・rules 行）で機械に守らせる。
- 形（§2 の役割の形に席を 1 つ足すだけ・ADR-0022 §2.1〜§2.5 は不変）:
  1. **`Role` の variant 1 つ** `Consult`（宣言順の末尾・`parse` / `as_str` / 網羅 match の消費側）。登録・起動（`seat register` / `seat launch --role consult`）・tick・rebrief・SessionStart の役割の解決は §2 の 1 本のまま。
  2. **rules 行 `role.consult`**（kind `RoleCapabilities`・値 = `["relay", "edit-outside", "edit-research"]`・裁定 id user 2026-09-17T01:48Z・C5）。中継（planner / 管理席へ結論を送る）・repo の外の編集（実験の作業場）・research 文書の編集の 3 つだけ。回答・承認・go・便の起動・merge・契約の編集（台帳の write）・code / 設計 doc / design-intent（research 以外）の編集は持たない＝§4 の guard が Edit / Write と `pipe` の口を止める。裁定の持ち込み先（R-C7-1）は planner の席のまま。
  3. **`Capability` の variant 1 つ** `EditResearch` と `PathKind` の variant 1 つ `Research`（`design-intent/research/` の段・`DesignIntent` より先に判定する＝1 関数の中の宣言順で決め、prose の順序注記を持たない・C2）。planner の行は `edit-design-intent` を持つので research も従来どおり書ける（`EditDesignIntent` は `Research` の段も通す＝上位の権能）。
  4. **brief の雛形 1 枚** `seat/brief/consult.txt`（§5 の規律・穴と pointer 付きの行だけ）: 第一手の復元・相談と調査の作法（repo と台帳は読むだけ・実験は repo の外の作業場・結論と実測は planner へ relay・research 文書は docs PR で出す・依存の候補を器に入れる話は A3）・席間の連絡の経路（FR44）・3 クラスの発火の pointer。xtask の検査（§5・穴 ⊆ 定義済み・pointer 無しの行 0・権能の名が全部現れる）と外形 snapshot はそのまま 3 枚目に掛かる。
  5. **触らない**: planner / admin の行と値・R-C7-1・`pipe` の口・§13 の断りの理由（`RefuseReason` は増やさない・consult が止められる周も既存の variant で足りる）。
- 却下: planner に相談を兼ねさせる（今日の詰まりの再発）／consult に `edit-contract` を渡す（台帳の書き手が 2 席になり契約の字面の事故の口が増える）／repo 内に `lab/` を切る（PUBLIC・CON2・実験物が tracked に漏れる）／`edit-design-intent` を渡す（spec / decisions まで書ける・広すぎる）。
- 歯（`seat_role_consult_` 接頭辞・`tests/e2e/seat.rs` と `tests/e2e/hook.rs`）: (a) `role.consult` の行が manifest に在り `RuleKind` の `ALL` と `rules validate` の外形に載る／(b) consult の登録 row を持つ席の Edit が `design-intent/research/x.html` を通し `design-intent/spec/x.html` と `docs/design/x.md` と crates 配下の Rust file を権能の名を告げて断る（planner の席は research も spec も通る）／(c) consult の席の `pipe answer` / `pipe run` が権能で断られる／(d) SessionStart の brief が consult の雛形から生成され外形 snapshot に載る。

## 15. 役割の口座 — host の根の宣言 1 か所を席の起動・立て直し・便用の除外・doctor が読む（`s2-07l.418`）— **置き換え**（[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html)・後継は [account-lifecycle.md](./account-lifecycle.md) §17）

- 何が起きているか（user 直命 2026-09-16 13:5xZ・folio2 planner の relay・逐語は台帳 `s2-07l.418`・決定は [ADR-0036](../../design-intent/decisions/ADR-0036-role-accounts-are-declared-once-per-host.html)）: planner と admin の口座を全 project で同じ口座に揃えたい（口座名と逐語は台帳が原本・CON2）が、役割 → 口座の対応は project ごとの state dir（host の manifest・[account-lifecycle.md](./account-lifecycle.md) §2）に閉じ、host 全体で 1 か所に宣言する口が無い。席の口座は `seat launch --account` か session 用の選定（同 §4）で決まる。便用の除外は便の repo の登録 row の口座だけ（[account-autonomy.md](./account-autonomy.md) §14・行 k）なので、他 project の席の口座が便に使われて席が逼迫する。
- 形: (1) **置き場** = host の根（受付札と同じ `<state_dir の親>/<NAME>-host/`・`seat/mod.rs` の `host_slots_dir` と同じ導き方・env を読まない）の file 1 つ `roles.toml`（`schema = 1` + `[[role-account]] role = "<Role の名>" account = "<label>"`・役割ごとに高々 1 行・role は閉じた `Role` の名で解け・account は**合わせた面**（tracked + host）の `[[account]]` に宣言済みで退役でない label＝`--account` の検査と同じ集合）。読み手は manifest の loader（`rules/manifest.rs`・TOML subset の同じ parser・同じ拒否形〔未知 key・型違い・役割の重複・未知の role・未宣言の label は行番号付きで全件・rc 1。段は host の面と同じ 2 つ＝面の中の欠陥で止まった周は合わせの検査に進まず、先に落ちた段の全件を出す〕）で、file が**無い**周は 0 行として続き、**在るのに読めない**周は typed に止める（FailClosed・`HostManifest` と同じ 3 値の型）。`<state_dir>/host.toml` に `[[role-account]]` が在れば未知の表として断る（1 か所）。(2) **席の起動と立て直し**（`seat/cycle/launch.rs`・`seat/cycle/relaunch.rs` の `choose`）: その役割の宣言が在り口座が**使える**（宣言済み・退役でない・最新の実測が在り・session 用の閾値 R-C9-1 未満・model は登録 row / `--model`）周は選定の純関数を撃たずにその label を返す。使えない周だけ従来の session 用の選定に落ち、理由（`declared-over-threshold` / `declared-unmeasured` / `declared-retired` の閉じた 3 値）を `inject.jsonl` の launch / relaunch の行に載せる。立て直しの結果の型は変えない（理由は結果が既に運ぶ口座の項目に添える＝管理 tick 側の網羅 match は不変）。`--account L` が宣言と違う周は `seat launch` の断り（起動の結果の閉じた型 `Launched`〔`seat/cycle/launch.rs`〕に宣言の label を運ぶ variant 1 つ・理由の字面は `seat/cycle.rs` の `REASON_` の列に `role-account-conflict` の定数 1 つ・描画は既存の `render_launched` が判定行に `reason=role-account-conflict declared=<label>` を載せ、`seat/cli.rs` の `Launched` の網羅 match はこの variant を既存の断りと同じ rc 1 に倒す）で起こさない。(3) **便用の除外**（`fleet/replay.rs` の `select_for_run`）: 除外集合 = 便の repo の登録 row の口座（行 k）∪ 宣言の全役割の口座（host 全体・席の生死を問わない）。純関数 `select` と `Input` は不変（除外集合の作り方が変わるだけ）。(4) **doctor** の 1 行 `role-accounts=<present|absent|unreadable> planner=<label|none> admin=<label|none>`（読むだけ・判定しない・C10.2・外形 snapshot `seat_doctor_external_form` が変わる）。(5) 登録 row の `account` は宣言から導いた実効値の写し（C10・row の鍵と書き手 3 つは不変）。
- 触らない: 純関数 `select`・R-C9-1・`Registration` の項目・退役の口・`host.toml` の 3 表・行 k の `--anchor`。
- 歯（`seat_role_account_` 接頭辞・`tests/e2e/seat/launch.rs` と `tests/e2e/seat/rules.rs` と `tests/e2e/seat/account.rs` と `tests/e2e/fleet.rs`・fixture は tmp の state dir の親に `<NAME>-host/roles.toml` を置く）: 宣言が在り使える周の `seat launch --role planner` が row の account に宣言の label を書く／宣言の口座が閾値以上の周は選定に落ちて理由 `declared-over-threshold` が記録に載る／`--account` が宣言と違う周は `role-account-conflict` で row も key も書かない／宣言が使える周の立て直しが宣言の口座で row を書き、使えない周は同じ理由が立て直しの記録の行に載る／`fleet select --purpose run` が宣言の口座を候補から外す（登録 row の無い host でも）／doctor の 1 行が 3 値（無い / 読める / 在るが読めない）で出て rc を変えない／`roles.toml` が壊れている周（未知 key・役割の重複・未知の role・未宣言の label）は launch も select も host の面と同じ拒否形（rc 1・欠陥を行番号付きで全部名指す）で止まる／無い周は従来どおり。
- 却下: [ADR-0036](../../design-intent/decisions/ADR-0036-role-accounts-are-declared-once-per-host.html) §3（写しは持たない）。

## 16. 役割の口座への移し替え — 宣言の書き換えの口 1 つと管理 tick の軸 1 つ（`s2-07l.418`）— **置き換え**（[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html)・軸の管理 tick は ADR-0045 §2 (2) で削除済み）

- 何が起きているか: 逼迫時に全 project の席を一括で別口座へ移す口が無い（席ごとの `seat launch --account` の手作業）。hook 集合の食い違い（FR62・[consumer-sync.md](./consumer-sync.md) §6・管理 tick の plugin の面〔削除済み〕）は「退避の合図 → 同じ target に立て直し」の経路を既に持つ。
- 形: (1) **口** `account reassign --state-dir S --role <Role の名> --to <label>`（`account/cli.rs` の verb 1 つ・`--to` は host の面に宣言済みで退役でない label・同じ label への書き換えは `unchanged` で何もしない）: §15 の `roles.toml` を読み → 検査 → 一時 file → rename で書き換える（部分書きを残さない・`account add` の host.toml の書き方と同じ）。A1 の対象外（宣言の書き換えで消費は席の起動と同じ・裁定の逐語は台帳）。(2) **tick の軸**（管理 tick の口座の軸〔削除済み〕 の `account_turn` の隣・inject / noop の判定で guard ではない）: 登録済みの席ごとに、登録 row の口座 ≠ その役割の宣言の口座 ∧ 宣言の口座が §15 (2) の意味で使える周は、FR29 と同じ除外（退避物が在る周・cycle が走っている周は注入しない）の下で退避の合図を注入する（`SignalOrigin` に variant 1 つ `Role`・`kind=externalize origin=role`）。使えない周は注入せず `NoopReason` に理由 1 つ。(3) **立て直し**は既存の入口（管理 tick の終了の手〔削除済み〕 の 立て直しの入口〔削除済み〕・origin が `Account` / `Hook` の周と同じ 3 条件）を通り、口座は §15 (2) の優先（宣言が使えればそれ）で決まる＝移し替えに新しい経路を持たない。(4) 常駐 process を持たない（各 project の tick が次の周に移す・ADR-0034 の契機の型）。
- 触らない: 退避の合図の形・立て直しの 3 条件・`Entry` の順序・復元の第 2 手。
- 歯（`seat_role_reassign_` 接頭辞・`tests/e2e/seat/account.rs` と 管理 tick の歯の file〔削除済み〕）: `account reassign` が `roles.toml` の 1 行を書き換え他の行を保つ／未宣言・退役中の label と未知の role を typed に断る／登録 row の口座が宣言と違い宣言の口座が使える席に tick が `origin=role` の退避の合図を注入する／宣言の口座が閾値以上の周は注入しない／合図の後の立て直しが宣言の口座で row を書く。
- 却下: ADR-0036 §3（写しは持たない）。

## 17. 役割の実効の口座 — host の根の記録 1 件が持ち、逼迫した周にだけ余裕が最大の口座へ役割ごと移る（`s2-07l.436`）— **置き換え**（[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html)・記録の単位は役割でなく群）

- 何が起きているか（user 裁定 id user 2026-09-17T05:50Z〔逐語は台帳 `s2-07l.418`〕と user 2026-09-17T06:10Z〔逐語は台帳 `s2-07l.436`〕・決定は [ADR-0041](../../design-intent/decisions/ADR-0041-role-effective-account-is-one-host-record-and-moves-only-under-pressure.html)）: §15 (2) は宣言の口座が使えない周に**席ごとに** session 用の選定へ落ち、その選定（`seat/cycle/relaunch.rs` の `choose`）は除外集合を「自席以外の全登録 row の口座」で作る。結果 (a) 同じ役割の他 project の席が先に移った口座が後続の席の候補から外れ、同じ役割の席が別々の口座へ散る (b) 候補が尽きて `no-account` で席が立たず、対話面が消える（実測 2026-09-17・planner の席の立て直しが連続で断られた・口座名は台帳が原本・CON2）。裁定の要旨: 同じ役割の席は project をまたいで同じ口座を使い、逼迫した周にだけ揃って余裕が最大の口座へ移り、移った先に居続ける（宣言へは戻らない）。走行中の便は止めない。consult は planner の口座を使う。本 § は §15 (2) / (3) と §16 (2) の「宣言の口座」を「実効の口座」に読み替える（ADR-0036 の DR3 は前半も後半も supersede）。候補が無い周の妥協の起動は §18。
- 形:
  1. **実効の口座の解決は 1 関数**（`seat/` の下の新しい module 1 つ・読むだけで選定も書きもしない）。結果は閉じた型で、宣言順がそのまま適用順（C2）: 記録（host の根の**実効の記録**が在り、その口座が §15 (2) の意味で使える＝居続ける・宣言の口座に余裕が戻っても戻らない）／種（記録が無く、宣言〔`roles.toml`〕の口座が使える＝宣言は初期値の種）／未決（それ以外・§15 (2) の理由の閉じた 3 値を運ぶ）。**consult は planner に写す**: この関数の入口で `Role` の consult を planner に読み替える 1 か所だけを持ち、consult 用の宣言の行も記録も持たない（`roles.toml` に consult の行が在れば §15 (1) の loader が同じ拒否形で断る＝`account reassign --role consult` も同じ検査で断られる）。以下「同じ役割」は写した後の役割で数える（planner と consult の席は同じ役割）。
  2. **実効の記録の置き場と形**: host の根（§15 (1) と同じ `<state_dir の親>/<NAME>-host/`・`seat/mod.rs` の `host_slots_dir` と同じ導き方・env を読まない）の下の dir 1 つに役割ごとの file 1 つ（`schema = 1` + role・account・ts・compromise の 4 項目・compromise は none か §18 の閉じた 2 値の字面で、行 k が書くのは none だけ・reader は `rules/manifest.rs` の TOML subset の同じ parser と同じ拒否形）。宣言（人が書く宣言値）とは file も型も別（器が書く実効値・C10）。読みは無い / 読めた / 在るが読めない の 3 値で、在るが読めない周は typed に止める（FailClosed・`HostManifest` と同じ形）。state dir が project ごとに分かれていても同じ 1 件を読む。
  3. **記録を書くのは席を起こす口だけ**（`seat/cycle/launch.rs` と `seat/cycle/relaunch.rs` の `choose`）: (1) の結果が記録の周は選定を撃たずにその label を返す。それ以外の周は host の根の lock（cycle の lock と同じ `create_new` の取り方・ttl は同じ rules 行）の下で (1) を解き直し → 記録になっていればそれを読むだけ（先に書いた席が勝ち、後続の席は読むだけ＝同じ役割の席が同じ口座へ揃う）→ 種の周は宣言の label を記録に書く → 未決の周は (4) で**役割単位で 1 回**選び直して記録を書き換える。書きは一時 file → rename。前の記録は上書きせず、先に履歴の側（同じ dir の下の退役の置き場・名に退役の ts・口座の退役の dir と同じ rename の形）へ move してから書く（N1 / N1.2）。lock を取れない周は 1 key も送らず既存の断り（lock-held）に倒す。選び直しが候補なしの周は既存の断りのまま（記録も書き換えない・§18 が妥協の段を足す）。
  4. **選び直しの順序と除外**（純関数 `select` と `Input` は不変・変わるのは除外集合の作り方だけ・留まる口座 `prefer` の渡し方も不変）: **席用と便用は順序が別**＝席用は session 用の既存の順序（逼迫度が最小＝余裕が最大・`fleet/select.rs` の `pick` の session 側）、便用は reset の早い順に使い切る（ADR-0027）のままで、用途の分岐で分かれている。除外集合 = 他の役割の実効の記録の口座（記録が無い役割は宣言の口座）∪ 他の役割の登録 row の口座。**同じ役割の席の登録 row の口座は除外しない**。
  5. **tick の役割の軸**（§16 (2) の 1 本のまま・tick は記録を書かない）: 比較の相手を宣言の口座から (1) の label に替える＝登録 row の口座 ≠ (1) の label ∧ 結果が記録か種の周に退避の合図（`origin=role`・§16 の除外と brake はそのまま）。未決の周は注入せず §16 の noop の理由のまま。常駐 process を持たない（ADR-0034）。
  6. **便用の除外**（§15 (3)・`fleet/replay.rs` の `select_for_run` と `fleet/cli.rs` の便用の枝）= 便の repo の登録 row の口座（[account-autonomy.md](./account-autonomy.md) §14）∪ 全役割の宣言の口座 ∪ 全役割の実効の記録の口座。**次の選定から効く**＝走行中の便は止めない（席が移った先の口座で走っている便はそのまま走り切り、再開と次の便の選定からその口座が外れる）。記録が在るが読めない周は §15 と同じ拒否形で止まる。
  7. **`--account` と宣言の書き換えの口**: `seat launch --account L` の食い違いの断り（§15 (2)・`role-account-conflict`）は比較の相手を (1) の label（記録か種）に替える（断りの型は不変）。`account reassign`（§16 (1)）は宣言を書き換えた周に、その役割の実効の記録を (3) と同じ lock の下で履歴の側へ move する（人の 1 手で全席が移る口を保つ＝次に席を起こす周が新しい宣言を種にし、他の席は (5) で揃う・`unchanged` の周は move しない）。
  8. **記録の行と doctor**: `inject.jsonl` の launch / relaunch の行に口座の出所（記録 / 種 / 選び直しの 3 値の字面）を 1 項目足す（§15 (2) の理由の項目の隣・立て直しの結果の型は変えない・**tick の判定行の `relaunch=` の値は不変**＝出所は行の末尾の別の項目に載り、作り直しの歯の file〔削除済み〕 が完全一致で測る `relaunch=` の token は動かない）。**立て直しの行の運び方**: 立て直しの行は tick の判定行（管理 tick の終了の手〔削除済み〕 が立て直しの結果を受けて 管理 tick の判定行〔削除済み〕 の本文に描く 1 行）なので、結果の型を変えずに、管理 tick の終了の手〔削除済み〕 が立て直しを撃つ**前**に (1) を読むだけで解き、その結果（記録 / 種 / 未決）を立て直しが届いた周の出所（記録 / 種 / 選び直し）として判定行の末尾の項目に 管理 tick の判定行〔削除済み〕 が描く（実効の記録が原本・行は写し・(1) が止まる周は立て直しも同じ拒否形で止まるので項目は無い・実効の記録の 4 項目は増やさない）。起動の行は `seat/cycle/launch.rs` の起動の記録が同じ項目を載せる。壊れた記録の断りは `seat/cli.rs` の壊れた manifest の断りと同じ 1 本（欠陥を 1 件 1 行・rc 1）を通す。doctor は §15 (4) の 1 行の末尾に、その行が並べる役割ごとの実効の口座（label か none）と compromise（none でない周だけ）を足し、記録が在るが読めない周は unreadable と出す（読むだけ・判定しない・rc を変えない・外形 snapshot `seat_doctor_external_form` が変わる）。A1 非該当（宣言の書き換えと同じ扱い・ADR-0036 の読み）。
- 触らない: `roles.toml` の形と置き場・純関数 `select` と `Input`・R-C9-1・`Registration` の項目・`SignalOrigin` と `NoopReason` の variant（§16 が足したものを使う）・立て直しの結果の型と 3 条件・`Entry` の順序・rules 行（足さない）・役割の権能（consult の権能は §14 のまま）・走行中の便・口座の逼迫の軸（§18）・席の指示文（§18）。
- 歯（`seat_role_move_` 接頭辞・`tests/e2e/seat/launch.rs` と `tests/e2e/seat/account.rs` と 管理 tick の歯の file〔削除済み〕 と `tests/e2e/fleet.rs`・fixture は tmp の state dir の親に host の根を置き、**state dir を 2 つ**並べて同じ根を読ませる）: 記録が無く宣言が使える周の起動が宣言の label を記録に書く（種）／別の state dir の同じ役割の席が選定を撃たずに同じ記録の label で row を書く（2 つ目の state dir の実測は別の口座が最良になる形に置き、記録が勝つことを測る）／記録の口座が使え宣言の口座にも余裕が在る周の立て直しが**記録の口座に留まる**（宣言へ戻らない）／記録の口座が閾値以上の周の立て直しが逼迫度の最小の口座を選び（reset が最も早い口座が別に在る形に置き、便用の順序でないことを測る）、前の記録が履歴の側に残って新しい記録が 1 件になる／同じ役割の他の席の登録 row の口座は候補から外れず、他の役割の実効の口座と登録 row の口座は外れる／consult の席の起動が planner の記録を読んで同じ label で row を書き、`roles.toml` の consult の行は rc 1 で断られる／tick が登録 row の口座 ≠ 記録の口座の席に `origin=role` の合図を注入し、未決の周は注入も記録の書きもしない／`fleet select --purpose run` が実効の記録の口座を候補から外し（登録 row の無い state dir でも）、走行中の便の event は増えも変わりもしない／`--account` が記録の口座と違う周は `role-account-conflict` で断る／`account reassign` が宣言を書き換えた周に記録が履歴の側へ移る／記録が壊れている周（未知 key・未知の role・未宣言の label）は launch も立て直しも select も rc 1 で欠陥を行番号付きで名指して止まる／doctor の 1 行が実効の口座を 3 値（無い / label / 読めない）で出して rc を変えない／launch / relaunch の記録の行に出所の字面が載る。
- 却下: [ADR-0041](../../design-intent/decisions/ADR-0041-role-effective-account-is-one-host-record-and-moves-only-under-pressure.html) §3（写しは持たない）。

## 18. 妥協の起動 — 候補が無い周も席を立て、理由を記録と席の指示文に出して user の裁定を待つ（`s2-07l.436` / `s2-07l.440`）— **置き換え**（[ADR-0049](../../design-intent/decisions/ADR-0049-seat-accounts-are-owned-by-project-groups.html) は群の移動に妥協を作らない・席の起動の妥協〔SRS FR69〕は決め直さず据え置き）

- 何が起きているか（user 裁定 id user 2026-09-17T06:10Z・逐語は台帳 `s2-07l.436`・決定は ADR-0041）: 選び直し（§17 (4)）が候補なしを返す周は席が立たず（ADR-0020 §2.4「候補なしの周は立て直さず次の tick で選び直す」・[account-autonomy.md](./account-autonomy.md) §5）、planner の席が立たない間は対話面が無く user が進め方を裁定できない。裁定の要旨: 閾値未満の候補が無い周も一番ましな口座で席を立て、席は作業を一時停止して user の裁定を待つ。全口座が当たっている周だけは従来の断り。通常の周の同居は不可のまま。
- 形:
  1. **妥協の 2 段**（`seat/cycle/relaunch.rs` の `choose`・純関数 `select` と `Input` は不変＝閾値と除外集合の渡し方だけで表す）: 段は 3 つで宣言順に試す（C2）: 通常（§17 (4)・閾値 = R-C9-1）／妥協 over-threshold（同じ除外のまま閾値に窓の全量 `LIMIT_PCT` を渡す＝当たっていない中で逼迫度が最小）／妥協 shared-with-role（他の役割の口座の除外を外し閾値は窓の全量＝同居も候補に入れて逼迫度が最小）。3 段とも候補なしの周（当たっていない測れた口座が 1 つも無い）だけ従来の断り（`Relaunched` の `None`・launch の既存の断り・1 key も送らない）。妥協でない周の同居は不可のまま（通常の段は他の役割の口座を必ず除外する）。
  2. **理由の記録**: 妥協の段で立てた周は、その段の字面（over-threshold / shared-with-role の閉じた 2 値）を実効の記録の compromise（§17 (2) の項目）と `inject.jsonl` の launch / relaunch の行（§17 (8) の出所の隣に 1 項目）に載せる。立て直しの行は、管理 tick の終了の手〔削除済み〕 が立て直しが届いた直後に実効の記録の compromise を読み、none でない周だけ判定行の末尾の項目に 管理 tick の判定行〔削除済み〕 が描く（実効の記録が原本・立て直しの結果の型は変えない）。解決の結果の型（§17 (1)）に variant 1 つ 妥協（label と理由を運ぶ・宣言順は記録と種の後、未決の前）を足す: compromise を持つ記録は「居続ける」に当たらず、席を起こす周ごとに (1) の 3 段を解き直し、通常の段が候補を返した周に compromise 無しの記録へ書き換わる（同じ label・同じ compromise の周は書き換えない）。`--account` の比較（§17 (7)）と tick の役割の軸（§17 (5)）は妥協の周を未決と同じに扱う。
  3. **席の指示文**（§5）: **雛形の穴 1 つ** `{compromise}` を持つ行を全役割の雛形に 1 行足す（「妥協の口座で立った（理由 = 穴）＝作業を止めて対話面で user の進め方の裁定を待つ」・pointer は ADR-0041）。`hook/mod.rs` の SessionStart が §17 (1) を読み、自席の登録 row の口座 = 妥協の記録の label の周だけ穴を埋めて出し、それ以外の周はその行を出さない（`seat/brief/mod.rs` の `Hole` に variant 1 つ・`HOLES` は 5 つ・`render` が穴の値を受ける・xtask の雛形検査の穴の列も同じ 5 つに・consult の雛形は行 h が作るので、行 l の write-set は雛形の dir を名指す）。
  4. **tick の口座の逼迫の軸**（FR38・管理 tick の口座の軸〔削除済み〕 の `account_turn`・tick は記録を書かない）: 登録 row の口座が閾値以上の席と、解決の結果が妥協の席は、通常の段の選定（§17 (4) の除外・読むだけ）が候補を返す周にだけ退避の合図を注入する（走っている席を妥協の口座へ動かす cycle を作らない＝妥協の起動は席が止まっている周にだけ起こる）。候補を返さない周は退避の合図の代わりに**妥協の通知を 1 回だけ**注入する（`InjectKind` に variant 1 つ・文は (3) の行と同じ趣旨＝作業を止めて対話面で user の裁定を求める・FR29 と同じ除外の下）。1 回の弁別は記録で行う: `inject.jsonl` の同じ席の直近の妥協の通知が同じ口座で、その ts の後にその口座の数える窓の reset が来ていない周は再送せず、`NoopReason` に足す理由 1 つの noop（`signal_brake` と同じ読み手の隣・口座が変わった周と窓が開き直った後の周は新しい 1 回）。**妥協の通知は立て直しの入口の合図ではない**（管理 tick の終了の手〔削除済み〕 の直近の合図の読みは退避と終了の 2 種のままで、通知を数えない）＝通知を受けた席は走ったまま作業を止めて裁定を待ち、通知から立て直しへ進む経路は持たない。妥協の起動（(1) の 2 段）に入るのは既存の入口だけ: 退避の合図（context 起点・hook 起点・候補が在った周の口座起点）の後に席が止まった周と `seat launch` で、その時点の選び直しが候補なしの周である。歯の fixture も、合図の周には通常の候補を置き、立て直しの周の前に候補を消す形（または context 起点の合図）で組む。妥協の通知は管理 tick の合図の 1 種で、FR44 の「入力欄への注入は tick の合図と復元にだけ」の内側（FR44 の句が並べる合図の名の改訂は ADR-0041 と同じ user の周）。解決の module は行 k が作る未 land の file なので、行 l の write-set は `+` の接頭辞で名指す。
- 触らない: §17 の記録の形と lock・通常の段の除外・純関数 `select` と `Input`・R-C9-1・`SignalOrigin`・立て直しの結果の型と 3 条件・`Entry` の順序・退避の合図の形・既存の雛形の行と既存の外形 snapshot（妥協でない周の指示文は不変）・rules 行（足さない）。
- 歯（`seat_role_compromise_` 接頭辞・`tests/e2e/seat/launch.rs` と `tests/e2e/seat/account.rs` と 管理 tick の歯の file〔削除済み〕 と `tests/e2e/hook.rs`）: 閾値未満の候補が 0 の周の立て直しと起動が over-threshold で席を立て、理由が実効の記録と launch / relaunch の記録の行に載る／他の役割の口座しか当たっていない口座が無い周だけ shared-with-role で立ち、当たっていない自前の候補が在る周は同居しない／全口座が当たっている周は従来の断りで 1 key も送らず記録も変わらない／妥協の記録の席を起こし直す周に通常の段が候補を返せば compromise 無しの記録へ書き換わり前の記録は履歴の側に残る／SessionStart の指示文が妥協の周だけその行を理由付きで出す（外形 snapshot 1 枚）・妥協でない周の既存の外形 snapshot は不変／xtask の雛形検査が未定義の穴を断り `{compromise}` を通す（in-file の歯・`seat_brief_compromise_` 接頭辞・`-p xtask` で撃つ）／既存の「候補なしで立て直さない」歯（作り直しの歯の file〔削除済み〕 と `tests/e2e/seat.rs` の helper が閾値以上の予備の口座を置く形）は、当たっていない予備が在る周は妥協で立つ側へ、全口座が当たっている形は従来の断りの側へ書き分ける／tick が閾値以上の席に、候補が在る周は退避の合図を、無い周は妥協の通知を注入する／同じ口座・同じ窓の 2 周目は通知を注入せず足した理由の noop になり、口座の reset の後の周はもう 1 回注入する／`InjectKind` と `NoopReason` の全 variant の列が宣言順で字面が重複しない。
- 却下: ADR-0041 §3 の OPT6（写しは持たない）。

## 19. 役割ごとの既定 model と effort を rules 行が持つ（契約表の行 m・`s2-07l.433`）

- 何が起きているか（実測 2026-09-17・planner 席）: 役割 → model を決める rules 行が無い。宣言は席の登録 row の `model` 1 か所だけで、`seat register` も `seat launch` も閉じた表（`Model::parse`）に在る字面なら何でも受ける。席（AI）が手で起こした session で「いま動いている model」を row に書き直すと、実測値が宣言値を上書きし（C10 の逆流）、以後の立て直しは `crates/scribe2/src/seat/cycle/relaunch.rs` がその row をそのまま運ぶ。当日の事故はこの経路で起き、当座は row を登録し直して戻した。effort には宣言が無い: 席の起動行（`crates/scribe2/src/seat/cycle/launch.rs` の `derive_launch`）は model だけを運び、深さは口座の設定 dir の設定 file 任せで、同じ役割の席が口座ごとに違う深さで走る（runner / lens が `runner.effort` の行で塞いだのと同じ穴が、席の面に残っている）。
- 値の裁定（裁定 id `user 2026-09-17T04:23Z`・逐語は台帳 `s2-07l.433`）: 役割ごとの既定は model と effort の**対**で持つ。値は planner が `fable` / `high`・管理席が `opus` / `xhigh`・相談席が `fable` / `high`（相談席の役割は §14 の行 h が足す＝行 m は行 h の後）。
- 形（行と読み手だけ・導出と運びは次の節）:
  1. **`RuleKind` の variant 2 つ** `RoleModel` と `RoleEffort`（宣言順の末尾・`as_str` と `shape` の網羅 match の消費側・`ALL` にも足す）。どちらも値の形は既存の `Str` で、`ValueShape` と `RuleValue` は増やさない（対を 1 行の list で持つ形・新しい値の形を作る形は却下側）。
  2. **rules 行**（id は `role.<役割名>.model` と `role.<役割名>.effort`・役割ごとに 2 行・裁定 id は上の 1 つ・C5）。既存の権能の行 `role.<役割名>` と id が重ならない（行の引きは完全一致）。
  3. **閉じた表への照合**を manifest の読み込みに足す（`crates/scribe2/src/rules/mod.rs` の `RuleRow` の名の検査・権能の名と対話面の役割を検査している同じ 1 関数の arm を 2 つ足す）: model の値は `Model::parse` で、effort の値は `Effort::parse` で引けること。綴り違いを黙って「既定なし」に倒さない（NFR4）。
  4. **読み手 1 本**を `crates/scribe2/src/seat/role.rs` に置く（役割の隣）: 役割ごとの既定を型で運ぶ小さな struct（model と effort の 2 field・どちらも閉じた型）と、渡された manifest から引く pure 関数 `defaults_of`、埋め込み manifest から引く薄い口 `defaults`（`crates/scribe2/src/seat/mod.rs` の `int_rule` / `int_rule_of` と同じ 2 段）。
  5. **読めない周の理由**は既存の閉じた列 `RuleRead` に variant を 2 つ足して名指す（値が文字列でない周と、字面が閉じた表に無い周）。`as_str` / `no_rule` の網羅 match と宣言順の const slice の消費側が動き、`RuleRead` を網羅 match で record の語彙へ写す `crates/scribe2/src/pipe/confine.rs` の 1 関数も同じ周に arm を埋める（足した 2 つは既存の「行を読めない」側の 1 語に倒す・包みの挙動は不変）。読み手は判定しない側のままで、極性は **fail-closed**（行が無い・不発効・表に無い周は既定に倒さず呼び手が断る・C1「行の無さを既定に倒さない」）。
  6. **値の正本は manifest**（C1 / C14）: 本節が持つのは行 id の形と裁定 id の在り処で、上の値は裁定の要旨の写しに留まり、器が読むのは manifest の行だけ。
- 触らない: 権能の行 `role.<役割名>` とその値・`Model` と `Effort` の表そのもの（`xhigh` は既に在る）・`ValueShape` と `RuleValue`・R-C7-1・起動行の導出と登録の口（次の節）・極性一覧（guard は増えない）。
- 歯（`rules_role_defaults_` 接頭辞を `crates/scribe2/tests/e2e/rules.rs` に・読み手の in-file の歯は `seat_role_defaults_` 接頭辞で `crates/scribe2/src/seat/role.rs` に）: (a) 役割ごとに 2 行が在り、行の kind と値の形と発効と裁定 id が一致し、**行の数は役割の閉じた列の 2 倍**（母集団を同時に出す・権能の行の歯と同じ形）／(b) 値が閉じた表に無い manifest は読み込みで拒まれる（model 側・effort 側の 2 例）／(c) 読み手が行から対を型で返し、行が無い・不発効・値が文字列でない・表に無い の 4 周をそれぞれ別の理由で名指す（4 例・fail-closed）／(d) 母集団の数え（行の総数・kind の総数・理由の列の長さ）と `rules` の外形 snapshot を更新する。
- 却下案: 役割ごとに 1 行で対を list で持つ（要素の順序が散文の規則になる・C2）／値の形に「対」を足す（形の網羅 match と表示と parser が全部動き、対を持つ行は 1 種類しか無い）／model と effort を 1 つの文字列に区切りで詰める（区切りの規則が散文になる）／既定を code に焼く（C1・N2）／effort の閉じた表を席の側へ写す（表は 1 つ・置き場は現状のまま）。

## 20. 席の起動と立て直しが既定の行から model と effort を導く（契約表の行 n・`s2-07l.433`・行 m の後）

- 何が起きているか: 上の節の行が在っても、起動の経路が row の `model` を運び続ける限り事故は再発する。現物では起動行を組む 1 本（`derive_launch` → `with_model` → `launch_line`）が `--model` だけを挟み、立て直しは登録 row の `model` を、初回の起動は `--model` の flag を、それぞれ唯一の宣言として読む。effort はどこにも運ばれない。
- 形（導出は 1 本・宣言は行の 1 か所）:
  1. **起動行は行から導く**: 初回の起動も立て直しも、役割の既定の行から引いた対を `claude` の語の直後へ **`--model <別名>` `--effort <値>` の順**で挟む（挟む位置と順序は導出の関数の宣言順で決め、散文の順序注記を持たない・C2）。挟む関数は今の model 専用の 1 本を**旗と値の対の列を受ける 1 本**へ広げ、雛形の中に同じ旗が既に在る周を断る検査は旗ごとに 1 つの理由を持つ（`--model` の二重は既存の字面のまま・`--effort` の二重は理由 1 つを足す）。雛形（row の `launch`）は従来どおり書き換えない。
  2. **`--model` は照合であって宣言ではない**: `seat launch --model M` は行と一致する周だけ通し、食い違う周は typed に断って 1 key も送らず row も書かない（理由 1 つ・既存の「表に無い値」の断りの隣）。`--effort` の flag は作らない（席の effort の宣言は行の 1 か所・row も持たない）。
  3. **`seat register` は食い違いを断る**: `--model` を省いた周は器が行から導いた値を row に書き、`--model` が行と食い違う周は登録の断りの閉じた列に variant を 1 つ足して断る（event を書かない・受付の極性は in-loop / fail-closed のまま）。
  4. **row の `model` は導出値**: 口座選定（モデル別 7 日窓）と tick の逼迫度が row の `model` を読む経路は変えず、器が書く値を**実測行と同じ語彙（表示名）**に揃える。立て直しが登録 row を更新する既存の 1 本（口座 label の更新）が同じ周に `model` も導出値で書き直す＝land より前に書かれた row は次の立て直しで自動的に直る（移行の口を別に作らない）。
  5. **行を読めない周は起こさない**（fail-closed）: 初回の起動は断り、立て直しは注入せず理由を判定行に残す（黙って口座の設定の既定で起こさない・C10）。
  6. **doctor に宣言と row の突合を出す**（C3.2・C10）: 登録 row の 1 行に行の既定を 1 語添える。席の doctor の行を描く関数（`seat/role.rs`）は今は manifest を受けないので、`--rules` の値を口座の doctor の行と同じ形で受ける引数を 1 つ足し、呼び手（`crates/scribe2/src/main.rs` の doctor の口・1 か所）が渡す（行を読めない周は既定の語を出さず理由の字面を出す・rc を変えない）。突合の面は doctor の 1 つである（席は宣言を直す権能を持たず、同じ事実を 2 面に描かない。復元の DATA の `[SEAT]` 行は `s2-07l.479.2` で DATA ごと消えた）。
- 行 i〜l との交差: §15〜§18 の行 i / j / k / l も `crates/scribe2/src/seat/cycle/launch.rs`・`crates/scribe2/src/seat/cycle/relaunch.rs`・`crates/scribe2/src/seat/cli.rs`・管理 tick の終了の手の module〔`s2-07l.479.1` で削除〕 と e2e の同じ file を触る＝交差する便は直列に流す（口座の決め方と model / effort の導き方は別の軸で、互いの型と断りを変えない）。
- 触らない: 口座選定の規則と入力・注入の門と極性・復元の経路・`Registration` の項目（effort の field を足さない）・`seat` の使い方の 1 行（`--effort` の flag を作らないので動かない）・極性一覧（guard は増えない）。
- 歯（`seat_launch_` / `seat_account_relaunch_` / `seat_register_model_` / `seat_role_doctor_` の既存の接頭辞に足す）: (a) 起動行が `claude` の直後に `--model` と `--effort` をこの順で 1 つずつ運び、雛形には旗が残らない／(b) 行と食い違う `--model` の起動は typed に断り、注入 0・登録 row 0／(c) 行と食い違う `--model` の登録は typed に断り event 0、省いた登録は導出値が row に載る／(d) 行と食い違う古い row を持つ席の立て直しは行の値で起こし、更新後の row の `model` が導出値に直る／(e) 行を読めない manifest では起動も立て直しも起こさず理由を名指す／(f) 雛形に旗が二重に在る周の断りが旗ごとに違う理由を名乗る／(g) doctor の登録 row の行が行の既定を添える。
- 却下案: `--model` の flag を廃す（未知の旗は今の読み方では黙って無視され、宣言の食い違いが静かに通る＝loud でない）／`--model` の上書きを裁定付きの別経路で通す（裁定は行の値を変える側にあり、起動ごとの上書きは行を回避する口になる）／row に effort の field を足す（宣言が 2 面になり、今回の事故と同じ形を effort で作る）／effort を口座の設定 file へ書いて揃える（器が設定の層に依る・C2.2）／席の側にも突合を出す（同じ事実の 2 面・席に処置の権能が無い）／立て直しで row を直さず移行の subcommand を作る（口が 1 つ増え、直すまで逼迫度が別の窓を読む）。

## 21. 復帰の DATA — SessionStart が台帳と git から「直前の流れ」を出す（契約表の行 o・`s2-07l.489`）

- 何を解くか: 圧縮（`/compact`・自動圧縮）と起動し直しの後、席が持つのは道具の要約と §5 の指示文の件数 1 行だけで、仕掛かり中の便と直近の裁定の在処を自分で引き直している。作業記憶（散文）は ADR-0045 §2 (2) で消したので、復帰の材料は**台帳と git から機械で導く**（C15・散文の持ち越しを作らない）。
- 形: SessionStart の hook は §5 の指示文（11 行・**変えない**）の後ろに、事実の行だけの区間を 1 つ出す。指示文の行ではない（規範を持たない＝N2 に当たらない・穴も pointer も持たない typed な行）ので、§5 の雛形・xtask の検査・ADR-0045 §2 (3) の行数は動かない。登録の無い席・仕えない周（FR24）は今と同じく 0 byte。
- 行の種類（行頭の marker で弁別・この順）:
  1. `[RECENT-WIP] <id> <更新時刻> <題>` — status が in_progress の bead の全件。
  2. `[RECENT-BEAD] <id> <status> <更新時刻> <題>` — 直近 24 時間に更新された bead を更新の新しい順に上位 N 本（1. に出た id は除く）。
  3. `[RECENT-GIT] head=<短い sha> branch=<名> ahead=<n> behind=<n>` の 1 行と、`[RECENT-COMMIT] <短い sha> <subject>` を直近 N 本。
  4. `[RECENT-DIRTY] <worktree の repo 相対 path>` — 未 commit の変更を持つ worktree（anchor を含む）。
  5. 各種類の末尾に `[RECENT-CUT] kind=<種類> shown=<n> total=<m>`（上限で切った周だけ）。0 件は `[RECENT-NONE] kind=<種類>`、測れなかった周は `[RECENT-UNMEASURED] kind=<種類> reason=<閉じた enum の字面>`（0 件と測れないを分ける・C10）。
- N と 24 時間と題の切り詰め幅は module の定数（rules 行を足さない・閾値ではなく表示の幅）。時刻の窓は呼び手が渡す現在時刻で測る（壁時計を module の中で読まない＝歯が時刻を固定できる）。
- 読み: 台帳は §5 と同じ `bd --readonly list --json` の子 process（**同じ 1 回の出力を件数の 1 行と共用**・待ち上限も同じ rules 行 `seat.ledger_timeout_s`）。`Issue`（列の順序が読む型）は広げない——構築 site が列の歯に多数在るので、新 module が同じ JSON から id / title / status / updated_at だけを読む型を別に持つ。git は anchor で子 process（`git` の読みの口だけ・network に出ない＝fetch しない。origin との差は手元の remote 追跡 ref で測る）。開いている PR は載せない（forge の口が要る・`gh` の 1 行で足りる＝C17）。
- 題は台帳の自由文なので 1 行に畳み（改行と制御文字を空白へ）幅で切る。行頭の marker を題が偽装しても行の種類は行頭の 1 語で決まる（題は 3 語目以降にしか現れない）。
- 極性: 台帳が読めない・git が無い・anchor が repo でない周は、その種類だけ `[RECENT-UNMEASURED]` を出して他の種類と §5 の指示文は出す（fail-open・読みの失敗で注入全体を黙らせない）。理由は閉じた enum（`ledger-unreadable` / `ledger-timeout` / `git-unavailable` / `not-a-repo`）。極性の宣言 site は `polarity.rs` の `NOT_A_GUARD` に 1 行（**guard ではない**＝行為を止めうる判定を返さない・[polarity.md §2](./polarity.md) の定義・`fleet::UnmeasuredReason` と同型の計測の境界。fleet の歯が「`Unmeasured` を名指す Guard は 0」を pin しているので、`Guard` の variant にはしない＝`<NAME> polarity` の一覧と snapshot は不変）。
- 置き場: `seat/` の子 module 1 枚（行 o の write-set の `+` の file）。hook の SessionStart の口が §5 の指示文の直後に呼ぶ。source（`startup` / `resume` / `clear` / `compact`）で出し分けない（どの入口でも同じ事実）。
- 歯（`tests/e2e/hook.rs`・接頭辞 `hook_session_recent_`）: 偽の `bd`（JSON を返す script）と toy repo で、(a) in_progress の bead が `[RECENT-WIP]` に全件出る／(b) 24 時間の窓の内と外が分かれ、上限で切った周に `[RECENT-CUT]` が shown と total を持つ／(c) 台帳が読めない周は `[RECENT-UNMEASURED] kind=wip reason=ledger-unreadable` で、§5 の 11 行と git の行は出る／(d) git の行が head・branch・ahead / behind を持ち、`[RECENT-COMMIT]` が直近の commit の短い sha と subject を新しい順に持ち（上限を超える周は `[RECENT-CUT] kind=commit`）、dirty な worktree が `[RECENT-DIRTY]` に出る／(e) 登録の無い席は今と同じく 0 byte／(f) 改行入りの題が 1 行に畳まれ、行数が増えない／(g) 読めた上で 0 件の種類（in_progress が 0・窓の内の更新が 0・dirty が 0）は `[RECENT-NONE] kind=<種類>` を出し、同じ種類の `[RECENT-UNMEASURED]` は出ない（(c) と対＝0 件と測れないの両側を測る）。席は §7 の guard の歯と同じ**偽 tmux**（pane → target を返す stub＝PATH の先頭の script）で解く: tmux を立てないので nextest の tmux group（`.config/nextest.toml`・行 o の write-set の外）を動かさない。
- 着地形（`s2-07l.489.1`）: 台帳の子 process は `seat/ledger.rs` の読みの口（stdout の本文を返す 1 本）を件数の 1 行と DATA が分けて読む＝1 回。`LedgerError` は待ち上限超過を別 variant（`Timeout`）に分け、DATA の理由 `ledger-timeout` の出所にする（列の読み手は `Ok` / `Err` だけを見るので不変）。時刻を読めない bead と上流の無い branch の値は `-`（0 に化けさせない・C10）。DATA の区間は記録 1 行（`what` = `session-start-recent`・bytes は出した行数分）を残す＝FR21 の記録の母集団に載る。**dirty の走査は worktree の上位 N 本**（module の定数・anchor が先頭・残りは HEAD の commit が新しい順＝1 回の `rev-list --no-walk` で全 HEAD の時刻を引く）に限り、worktree が N より多い周は末尾に `[RECENT-CUT] kind=dirty shown=<測った本数> total=<worktree の本数>`（0 件の `[RECENT-NONE]` の後ろにも付く）——便ごとの worktree が数百本溜まった anchor（実測 2026-09-20: 195 本）で `status` を全数に撃つと hook の時間予算（rules 行 `hook.timeout_s`）を食い潰し、注入全体が黙る。
- 触らない: §5 の雛形と穴・`Issue` の field・rules 行・列（dispatch）の読み。
- 却下案: `{ledger}` の穴の値を複数行に広げる（雛形の行の規律と xtask の検査が「1 穴 1 値」を前提にしている・ADR-0045 §2 (3) の 11 行が動く）／席が notes に書く習慣の行を雛形に足す（規範文の追加・N2）／直近の会話を要約して持ち越す（作業記憶の再導入）。

## 22. 圧縮の直前の 1 枠 — PreCompact が直前の発言を逐語で残し、圧縮後の SessionStart が 1 回だけ出す（契約表の行 p・`s2-07l.489`・行 o の後）

- 何を解くか: 自動圧縮は席の手番の途中でも走る。§21 の DATA は台帳と git に**書かれた後**の事実しか持たないので、「いま何をしている途中だったか」は落ちる。道具の hook は LLM に書かせられない（shell の command）ので、できるのは機械の記録だけである。
- 形: 生成 hooks.json に PreCompact の 1 行を足す（生成器は同じ gen-manifest・`--pane` と `--project` は他の行と同じ）。hook は payload の `trigger`（`manual` / `auto`）・`transcript_path` を読み、transcript の**末尾から**直近の assistant の text block を逐語で抜いて、席の置き場（§4 と同じ解き方の `seat/<target>/`）の **1 枠**（file 1 つ・上書き）に書く。枠の中身 = 時刻・trigger・抜いた文（幅で切る・切ったら切った事実を持つ）。
- 消費: SessionStart が `source = compact` の周だけ、§21 の区間の前に `[PRECOMPACT] trigger=<字面> ts=<時刻>` の 1 行と抜いた文を出し、**出した後に枠を消す**（持ち越さない＝古い枠が次の圧縮で化けない・drift 源にしない・C15）。`compact` 以外の source は枠を読まず触らない。
- 読みの上限: transcript は末尾の定数 byte だけを読む（全読しない）。JSON として読めない行は読み飛ばし、assistant の text が 1 つも取れない周は枠を書かない（空の枠を作らない）。
- 極性: PreCompact は**何が起きても圧縮を止めない**（rc 0・stdout 0 byte・失敗は stderr 1 行と記録 1 行）。登録の無い席・仕えない周は何も書かない。SessionStart の側は枠が無い・読めない周に `[PRECOMPACT]` を出さないだけで他は出す。極性一覧に 2 行（書く側・読む側）。
- 出さないもの: 枠は置き場（repo の外）にだけ在り、repo には 1 byte も書かない。抜いた文は席の自分の発言だけ（tool の出力・user の発言は抜かない＝機微の混入の面を狭める）。
- 歯（`tests/e2e/hook.rs`・接頭辞 `hook_precompact_`）: 偽の transcript と toy repo で、(a) PreCompact が枠を書き、続く `source = compact` の SessionStart が `[PRECOMPACT]` と逐語の文を出し、枠が消える／(b) 同じ SessionStart をもう 1 回撃つと `[PRECOMPACT]` は出ない／(c) `source = startup` は枠を消さず出さない／(d) transcript が読めない・assistant の text が無い周は枠を書かず rc 0・stdout 0 byte／(e) 幅を超える文は切られ、切った事実が行に出る／(f) 登録の無い席は枠を書かない。生成 hooks.json の歯（xtask）は PreCompact の行が `--pane` と `--project` を運ぶことを測る。
- 行 o との交差: hook の SessionStart の口と `tests/e2e/hook.rs` を共に触る＝直列に流す（行 o が先）。
- 着地形（`s2-07l.489.2`）: 置き場は `hook/` の子 module 1 枚（行 p の write-set の `+` の file）。枠は `seat/<target>/precompact` の 1 file（1 行目 `schema=1 trigger=<字面> ts=<秒> total=<切る前の文字数>`・2 行目以降が逐語）。SessionStart の 1 行は `[PRECOMPACT] trigger=<字面> ts=<UTC の時刻> lines=<逐語の行数>` で、切った周だけ ` cut=<出した文字数>/<切る前の文字数>` が続く（切った事実は header が持つ＝逐語の側に印を混ぜない）。末尾の読みと幅は module の定数（読みは末尾 256 KiB・幅は 2000 文字・表示の幅であって閾値ではない＝rules 行を足さない）。抜くのは transcript の行の `type == assistant` の `message.content` の配列の**最後の** `text` block（空白だけは無いと見る・content が文字列の行と JSON でない行は飛ばす）。書かない理由は閉じた enum（`no-transcript` / `transcript-unreadable` / `no-text`）で、**読めないだけを失敗**として stderr 1 行に出し、無い・text 無しは黙る（どれも記録 1 行 `what = precompact-skip <理由>`・書いた周は `precompact-slot`・bytes 0）。読む側は記録 1 行（`what = session-start-precompact`・bytes は出した行数分）を残し、読めない枠も消す（古い枠が次の圧縮で化けない）。登録の判定は §5 と同じ登録 row の有無（pane → target → row）。trigger の字面は空白と制御文字を `_` に畳み、無い周は `-`（header の key を偽装しない）。
- 却下案: 枠を bead の notes に書く（bead は task と裁定だけ・C15。hook が台帳へ write する経路も作らない）／枠を複数持って履歴にする（作業記憶の再導入）／transcript の全文を要約する（hook は LLM を呼べない・呼ぶ経路は課金と依存を足す）。

## 23. 復帰の 2 便の歯の補強 — 変異検査をすり抜けた面に歯を足す（契約表の行 q・`s2-07l.489`・歯だけ）

- 何を解くか: 行 o と行 p の gate の変異検査（記録であって門ではない）で、173 本中 23 本が生存した（実測 2026-09-20・内訳は台帳 `s2-07l.489` の notes）。同値の変異は 1 本（待ち上限の境界の瞬間の `<` と `<=`）だけで、残りは歯の無い面である。挙動の誤りは見つかっていない＝src は触らず、歯だけを足す。
- 歯の無かった面と足す歯（`tests/e2e/hook.rs`・接頭辞 `hook_recovery_edge_`・偽の `bd` と toy repo・SessionStart の口から測る）:
  1. **台帳の子 process の終わり方**: (a) JSON を出した後 rc 非 0 で終わる偽の `bd` の周は `[RECENT-UNMEASURED] kind=wip reason=ledger-unreadable`（出力が読めても rc を見る）／(b) stdout を閉じた後も待ち上限を越えて生き続ける偽の `bd` の周は `reason=ledger-timeout`／(c) stdout を閉じた後、上限の内側で少し遅れて rc 0 で終わる偽の `bd` の周は測れた側（`[RECENT-WIP]` か `[RECENT-NONE]`）に出る。
  2. **worktree を測る順**: 列挙の順と HEAD の commit の新しい順が**食い違う** dirty な worktree 2 本を作り、`[RECENT-DIRTY]` の行が commit の新しい順に並ぶ。commit を 1 つも持たない（未生の HEAD の）worktree を混ぜても順は変わらず、その worktree は末尾側に来る。
  3. **上限とちょうど同じ件数**: commit の本数が表示の上限とちょうど同じ repo は `[RECENT-CUT] kind=commit` を出さない。worktree の本数が走査の上限とちょうど同じ repo は `[RECENT-CUT] kind=dirty` を出さない。
  4. **時刻の字**: 時差の字が 2 桁でない `updated_at`（例 `+9:00`）を持つ in_progress の bead は `[RECENT-WIP]` に更新時刻の値 `-` で出て（時刻を読めない側）、同じ字の open の bead は 24 時間の窓に入らない。
  5. **枠が「無い」以外の理由で読めない・消せない周**: 席の置き場の枠の名前が dir になっている周の `source = compact` の SessionStart は、`[PRECOMPACT]` を出さず、stderr に読めない理由の 1 行と消せない理由の 1 行を出し、§5 の指示文と §21 の区間は出す（rc 0）。枠が無い普通の周は stderr にどちらの行も出さない。
  6. **socket を渡した周の席の解決**: PreCompact の口に tmux の socket を渡した周も登録済みの席として枠を書く（socket を渡さない形でしか測っていなかった）。
- 足す歯は既に着地した挙動を測るので base でも緑である＝**各歯の fn の中の行頭に `// flip-check: retroactive s2-07l.489.3` の札を付ける**（札の無い歯は gate の flip-check が `green-on-base` で落とす・1 周目の実測 2026-09-20）。
- 各歯は、対応する生存した変異を src に当てると落ちることを実装の周に実測し、結果を便の報告に載せる（flip-check の後から足す歯の札の前提）。当てて落ちなかった変異は同値か歯の不足かを報告で分ける。
- 触らない: `src/` の全部・§21 / §22 の行の形・既存の歯。

## 24. path の種別を対象 repo の vessel 宣言が名乗る（契約表の行 r・[ADR-0047](../../design-intent/decisions/ADR-0047-path-kinds-are-declared-in-the-vessel-declaration.html)・`s2-07l.491`）

- 何を解くか: §4 の分類は本 repo の配置（3 つの固定 prefix）を器の中に持つ。配置の違う consumer repo では全 file が code の種別に落ち、src の編集の権能を持たない席は 1 file も編集できない（実測 2026-09-19）。決定は ADR-0047（種別に属する path の集合を対象 repo の宣言が名乗る）で、本 § はその実装の形である。
- やさしく言うと: 「どこが仕様で、どこが設計 doc で、どこが test か」を、相手の repo が自分の宣言 file に書けるようにする。書かない repo は今までどおり。
- 宣言の形: vessel 宣言の任意 key 3 本（ADR-0047 §04 の名）。値は repo 相対の prefix の配列で、末尾が `/` の項目はその dir の下の全 file、`/` で終わらない項目はその path と完全一致の 1 file。既存の任意 key と同じ読み口（宣言の parser の閉じた key 列に足す・配列の層は既に在る）で読み、書いた周の空配列は既存の key と同じく不備である。
- key ごとに独立に効く: 書かれた key はその種別の固定値を**置き換える**（足し合わせない）。書かれていない key の種別は今の固定の判定のまま＝3 本とも無い宣言と、宣言 file を持たない repo は今と 1 行も変わらない。本 repo の歯の置き場（crate ごとの tests の dir）は prefix 1 本では書けない形なので、固定の判定は消さずに既定として残す。
- 読む場所: guard は分類の直前に、席の登録 row の anchor の **HEAD の tree** の宣言を読む（宣言の既存の読み手と同じ 1 本＝作業ツリーは読まない・commit されていない宣言は無いのと同じ）。便の worktree と docs 用の worktree の中の file も、anchor の宣言で分類する（§4 の「repo の写し」の扱いは変えない）。宣言 file 自身は宣言に何が書いてあっても code の種別である（席は自分の柵を広げられない）。
- 不正な宣言: 項目が `..` の段を含む・絶対 path・空文字・同じ項目か一方が他方の prefix になる項目が 2 つの種別にまたがる、のどれかが 1 件でも在る周、および宣言 file が在るのに読めない（parser の不備が 1 件でも在る）周は、**repo 内の全 file を code の種別として扱う**（fail-closed・黙って固定値へ戻さない・NFR4）。理由は閉じた enum（`parent-segment` / `absolute` / `empty` / `overlap` / `unreadable`）。repo の外（`Outside`）の判定は宣言に依らない。
- 観測: doctor は行を増やさず、既存の席の行（`seat: role=… anchor=… target=… account=… model=…` の 1 行・登録 row ごと＝anchor ごと）の末尾に欄を 1 つ足す: `paths=default`（3 本とも書かれていない・宣言 file を持たない repo と存在しない anchor も同じ）／`paths=declared:<書かれた key の数>`／`paths=invalid:<上の理由の字面>`。総行数は変わらない（行数を pin する既存の歯は不変）。guard が断った deny の 1 行（§4）は、`invalid` の周に同じ `paths=invalid:<理由>` の字面を末尾に持つ（不正でない周は `paths=` を持たない）。
- 極性: 新しい guard は足さない（既存の編集面の guard の分類の入力が変わるだけ）。宣言が読めない周に倒れる先は「全部 code」＝権能なしの側で、既存の fail-closed の向きと同じである。
- hook の時間: 宣言の読みは git の子 process 1 回（HEAD の 1 file）で、Edit 系の tool の周にだけ撃つ（Bash の面は path を分類しないので読まない）。
- 歯（`tests/e2e/hook.rs`・接頭辞 `hook_role_paths_`・toy repo に宣言を commit して PreToolUse の口から測る）: (a) 3 本の key を書いた repo で、宣言した仕様の dir・設計 doc の dir・test の dir の下の編集が orchestrator の席で通り、それ以外の file は断られる／(b) `/` で終わらない項目は完全一致の 1 file だけが通り、同じ名で始まる別の file は断られる／(c) 1 本だけ書いた repo は、その種別だけが宣言で決まり、残りは固定の判定のまま／(d) 宣言 file を持たない repo と key を 1 本も書かない repo は今の分類と同じ／(e) 宣言 file 自身の編集は、宣言がそれを名指していても断られる／(f) 不正な宣言（5 つの理由のそれぞれ）の repo は、固定値なら通る path も含めて repo 内の全編集が断られ、deny の 1 行が理由の字面を持つ／(g) commit していない作業ツリーの宣言は効かない／(h) 便の worktree の中の file も anchor の宣言で分類される。doctor の歯（`tests/e2e/seat/register.rs`・接頭辞 `seat_role_doctor_paths_`）: 3 つの state のそれぞれの行と、`invalid` の理由の字面。
- 触らない: 種別の集合（closed enum）と種別ごとの権能の 1:1・rules 行・宣言の schema の版（1 のまま）・便の印で開く write-set の扱い（§4）。
- 後続: consumer の移行の手順（[consumer-sync.md](./consumer-sync.md) §16）に「宣言へ 3 本の key を書く」段を足すのは、本行の着地の後の docs の便。新しい consumer の最初の宣言 file を書く口は導入の口の設計（`s2-07l.491`）。

## 25. 便を止める権能 `stop` — 起動の権能から分け、席は便 1 本の名指しの形だけ撃てる（契約表の行 s・[ADR-0048](../../design-intent/decisions/ADR-0048-stopping-a-run-is-a-separate-capability-of-the-orchestrator.html)・`s2-07l.495`）

- 何を解くか: ADR-0045 §2 (1) の後、便を止める口は起動の権能に結ばれていて、どの席の行にも無い＝居座る便を席から外せない（実測 2026-09-20・[dispatcher.md](./dispatcher.md) §11）。決定は ADR-0048 で、本 § はその実装の形である。
- やさしく言うと: 「この 1 本を止める」だけを席に許す。「全部止める」と、始める・再開する・片付けるは、今までどおり席からは撃てない。
- 約束（1 つずつ歯が測る・行 s の done と 1:1）:
  1. **権能の列に `stop` が 1 つ増える**: 権能の閉じた enum と全 variant の列に `stop` を足す（字面は `stop`・宣言順は `merge` の後ろ）。rules 行の loader は `stop` を知っている名として受け、知らない名は今までどおり拒む。
  2. **rules 行 `role.orchestrator` の値に `stop` が載る**（裁定 id と日付を今回の裁定に更新）。席の指示文（§5 の権能の行）にも `stop` が出る（外形 snapshot が更新される）。
  3. **便 1 本を名指す停止は `stop` の権能で通る**（許す形を列挙する・allowlist）: guard の表は停止の口を `stop` に結ぶ（表は「口 → 権能」のまま）。その上で、停止の呼び出しの**窓**＝停止の 2 語の直後から command 行の末尾までの token が、**値つきの flag `--run` / `--state-dir` / `--repo` / `--rules` とその値だけ**で出来ていて、`--run` がちょうど 1 回在り、どの値も `-` で始まらず shell が意味を変える字（区切り・pipe・括弧・`$`・backtick・引用符・redirect）を 1 つも含まない呼び出しだけが、`stop` の権能を要る＝orchestrator の席で通る。
  4. **それ以外の停止は起動の権能へ降ろす**（＝どの席でも断られる・fail-closed）: 窓に上の 4 つ以外の token が 1 つでも在る形は全部こちらである——`--all` を持つ形・`--run` の無い形・`--run` の直後に値の無い形・`--run` と `--all` の両方を持つ形・**`--run=<id>` の 1 語の形**（止める口は flag を完全一致で読むので、この形は名指しにならず一括の停止へ落ちる）・列を撃つ道具の flag を持つ形・値や窓に区切りや pipe や `$(` を含む形（窓の後ろに別の command が続く行は、席は停止を単独の 1 行で撃つ）。
  5. **1 行に権能付きの呼び出しが複数在る周は全部の権能を要る**（今の規則のまま）: 名指しの停止の窓は行の末尾までなので、後ろに別の呼び出しが続く行は約束 4 で起動の権能へ降りる。停止の前に別の口（例えば回答）が在る行は、両方の権能を持つ席でだけ通る。
  6. **他の口の権能は変わらない**: 受付・起動・再開・退役は起動の権能のまま、着地は merge のまま、回答と承認は今のまま。測るのは既存の歯（`role_guard_capability_commands_match_the_three_word_sequence`〔停止の 1 件だけ期待値が変わる〕・`hook_role_bash_face_allows_answer_and_denies_launch`・権能の表の in-file の prop 3 本）で、行の verify がそれぞれを撃つ。prop は module の中の歯で名が `prop_role_` で始まり、接頭辞 `role_guard_` では当たらない（module の path は filter の字面に入らない）ので、verify に **prop の名の全体を 3 行**で書く（裸の `prop_role_` は write-set の外の `tests/e2e/prop.rs` の歯にも当たる）。
  7. **`stop` を持たない行の席では名指しの停止も断られる**（権能は行から来る・行に無ければ通らない）。
- 止める口そのものの挙動（終端 `Stopped` の記帳・worktree と branch を残す・止め切れない周の断り）は 1 つも変えない。止めた便は終端の列外に入り、契約の字を直すか `release` の印で列に戻る（[dispatcher.md](./dispatcher.md) §12）。
- 歯: guard の照合は pure な fn の in-file の歯（接頭辞 `role_guard_stop_`・約束 3 / 4 / 5・**母集団 = 停止の呼び出しの形 9 つ**: 通る形 1〔`--run` と値、置き場と repo と rules の flag を足した形も通る〕／`--all`／`--run` 無し／`--run` の値無し／`--run` と `--all`／`--run=<id>`／列の道具の flag つき／値に pipe を含む形〔置き場の値の途中に pipe と別の `--run`〕／`$(` を含む形）と、PreToolUse の口からの e2e（`tests/e2e/hook.rs`・接頭辞 `hook_role_stop_`・約束 3 / 4 / 7 を登録済みの席で測る）。rules 行と loader は `tests/e2e/rules.rs`（接頭辞 `rules_role_stop_`・約束 1 / 2）。
- 変更する既存の歯（名で数える・どれも write-set の中）: `role_guard_capability_commands_match_the_three_word_sequence`（停止の期待値が起動から `stop` へ）・埋め込みの rules 行の値と裁定 id を pin する `rules_embedded_manifest_` の歯 3 本・指示文の外形 snapshot（`hook_brief_` の歯・更新だけ）。約束 6 の「変わらない」は上に名指した歯がそのまま測る。verify の既存の接頭辞 `hook_role_` は行 r が足した in-file の歯（`pipe/declaration/path_kinds.rs`）にも当たるので、その file を write-set に載せる（歯の置き場の門のため・**中身は変えない**）。同じ理由で、接頭辞 `role_guard_` が名の途中に当たる極性一覧の歯の file（`tests/e2e/polarity.rs`）も write-set に載せるが、**中身は変えない**（下の「触らない」のとおり極性一覧は動かない・その歯は `role_guard_` の verify 行で緑のまま撃たれる）。
- 触らない: 止める口の本体・他の権能付きの口の結び・編集面の guard・極性一覧（新しい guard は足さない＝既存の Bash 面の guard の表の 1 行が変わるだけ）。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "a"
title = "seat の外形 snapshot を面ごとに割る（旧 snapshot は消し、他 doc の行の名指しを面ごとの file 名へ）"
req = ["FR23", "FR59"]
section = "7"
write-set = ["crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_usage_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "docs/design/seat-roles.md", "docs/design/seat-autonomy.md", "docs/design/dispatcher.md", "docs/design/working-memory.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_usage_external_form seat_doctor_external_form"]
size = "S"
done = "seat の外形 snapshot が面ごとに割れ・旧 snapshot は消えて未参照 0・他 doc の行が面ごとの file 名を名指す（rebrief の面は `s2-07l.479.2` で DATA ごと消えた）"

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
id = "g"
title = "role guard の断りの理由を閉じた enum RefuseReason にし、各 variant が代替ルートの 1 行を持って deny 文の末尾に route= を添える"
req = ["FR45", "FR40"]
section = "13"
write-set = ["crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/tests/e2e/hook.rs", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_role_guard_route_"]
size = "S"
done = "role guard の deny 文が理由ごとの代替ルートを 1 行で名指し、理由の字面と判定の順序と極性は不変、6 variant の宣言順が pin され route が全部非空"

[[contract]]
id = "m"
title = "役割ごとの既定 model と effort を rules 行が持つ — 規則の種類を 2 つ・役割ごとに 2 行・値は閉じた表に照合・読み手 1 本と読めない理由 2 つ（裁定 user 2026-09-17T04:23Z）"
req = ["FR41", "FR40"]
section = "19"
write-set = ["rules/manifest.toml", "crates/scribe2/src/rules/mod.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/pipe/confine.rs", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/snapshots/e2e__rules__rules_external_form.snap"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail rules_role_defaults_", "cargo nextest run -p scribe2 --no-tests=fail rules_embedded_manifest_", "cargo nextest run -p scribe2 --no-tests=fail rules_manifest_carries_", "cargo nextest run -p scribe2 --no-tests=fail rules_external_form", "cargo nextest run -p scribe2 --lib --no-tests=fail seat_role_defaults_", "cargo nextest run -p scribe2 --lib --no-tests=fail rule_read_"]
size = "S"
done = "役割の閉じた列のどの役割にも model と effort の 2 行が同じ裁定 id で在り、値が閉じた表に無い manifest は読み込みで拒まれ、読み手が対を型で返して行なし・不発効・文字列でない・表に無いの 4 周を別の理由で名指し、行と種類と理由の列の母集団の数えと rules の外形 snapshot が更新されている"

[[contract]]
id = "n"
title = "席の起動と立て直しが既定の行から model と effort を導く — 起動行は claude の直後に 2 つの旗を運び、行と食い違う登録と起動は typed に断り、row の model は導出値として直る"
req = ["FR59", "FR38", "FR40"]
section = "20"
depends = ["m"]
write-set = ["crates/scribe2/src/seat/cycle.rs", "crates/scribe2/src/seat/cycle/launch.rs", "crates/scribe2/src/seat/cycle/relaunch.rs", "crates/scribe2/src/seat/cli.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/src/main.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/seat/launch.rs", "crates/scribe2/tests/e2e/seat/account.rs", "crates/scribe2/tests/e2e/seat/register.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail seat_launch_", "cargo nextest run -p scribe2 --no-tests=fail seat_account_relaunch_", "cargo nextest run -p scribe2 --no-tests=fail seat_register_model_", "cargo nextest run -p scribe2 --no-tests=fail seat_role_doctor_", "cargo nextest run -p scribe2 --lib --no-tests=fail seat_launch_"]
size = "M"
done = "初回の起動も立て直しも既定の行から導いた model と effort を claude の直後にこの順で 1 つずつ運び、行と食い違う登録と起動は 1 key も送らず event も書かずに断り、古い row を持つ席は立て直しで行の値に直り、行を読めない周はどちらも起こさず理由を名指し、doctor の登録 row が行の既定を添える"

[[contract]]
id = "o"
title = "復帰の DATA — SessionStart が §5 の指示文の後ろに、台帳の仕掛かり中と直近更新の bead・anchor の git の直近・dirty な worktree を typed な行で出す（0 件と測れないを分ける・読めない種類だけ UNMEASURED）"
req = ["FR42", "FR19"]
section = "21"
write-set = ["+crates/scribe2/src/seat/recent.rs", "crates/scribe2/src/seat/mod.rs", "crates/scribe2/src/seat/ledger.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/polarity.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "docs/design/polarity.md", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_session_recent_"]
size = "S"
done = "偽の台帳と toy repo で、SessionStart が 11 行の指示文を変えずにその後ろへ仕掛かり中の bead の全件・24 時間の窓の直近更新（上限で切った周は shown と total）・git の head と branch と ahead / behind・直近の commit の短い sha と subject・dirty な worktree を出し、読めた上で 0 件の種類は NONE・台帳が読めない周はその種類だけ UNMEASURED で他は出て（0 件と測れないの両側）、改行入りの題は 1 行に畳まれ、登録の無い席は 0 byte のまま"

[[contract]]
id = "p"
title = "圧縮の直前の 1 枠 — 生成 hooks.json に PreCompact の行を足し、hook が transcript の末尾から席の直近の発言を逐語で 1 枠に書き、source = compact の SessionStart が 1 回だけ出して枠を消す（圧縮は止めない）"
req = ["FR42", "FR19"]
section = "22"
write-set = ["+crates/scribe2/src/hook/precompact.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/hook/stamp.rs", "crates/scribe2/src/main.rs", "crates/scribe2/src/polarity.rs", "crates/xtask/src/genmanifest.rs", "hooks/hooks.json", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap", "crates/scribe2/tests/e2e/snapshots/e2e__hook__vessel_external_form.snap", "docs/design/polarity.md", "docs/design/vessel-hook.md", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_precompact_", "cargo nextest run -p xtask --no-tests=fail gen_manifest_hooks_json_precompact_"]
size = "S"
done = "偽の transcript と toy repo で、PreCompact が rc 0・stdout 0 byte のまま席の直近の発言を 1 枠に書き、続く source = compact の SessionStart が [PRECOMPACT] と逐語の文を 1 回だけ出して枠を消し、startup では出さず消さず、transcript が読めない周と登録の無い席は枠を書かず、幅を超える文は切られて切った事実が行に出て、生成 hooks.json の PreCompact の行が --pane と --project を運ぶ"

[[contract]]
id = "q"
title = "復帰の 2 便の歯の補強 — 変異検査をすり抜けた面（台帳の子 process の終わり方・worktree を測る順・上限とちょうど同じ件数・時差の字・枠が無い以外の理由で読めない周・socket を渡した席の解決）に歯を足す（歯だけ・src/ は触らない）"
req = ["FR42", "FR19", "NFR4"]
section = "23"
write-set = ["crates/scribe2/tests/e2e/hook.rs"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_recovery_edge_"]
size = "S"
done = "偽の bd と toy repo で、JSON を出して rc 非 0 で終わる台帳は ledger-unreadable・stdout を閉じて上限を越えて生きる台帳は ledger-timeout・上限の内側で遅れて rc 0 で終わる台帳は測れた側に出て、列挙の順と食い違う dirty な worktree 2 本が commit の新しい順に並び未生の HEAD の worktree を混ぜても順が変わらずその worktree は末尾側に来て、commit と worktree の本数が上限とちょうど同じ repo は CUT を出さず、時差の字が 2 桁でない in_progress の bead は更新時刻 - で出て open の bead は窓に入らず、枠の名前が dir の周の compact の SessionStart は PRECOMPACT を出さず stderr に読めない理由と消せない理由の 2 行を出して指示文と DATA は出して rc 0 で終わり（枠が無い周は stderr にどちらの行も出さない）、socket を渡した PreCompact が登録済みの席の枠を書き、各歯が fn の中の行頭に後から足す歯の札（flip-check: retroactive s2-07l.489.3）を持って gate の flip-check が通り、各歯が対応する変異を src に当てると落ちる実測が便の報告に載り（当てて落ちなかった変異は同値か歯の不足かを報告で分ける）、src/ と既存の歯は 1 行も変わらない"

[[contract]]
id = "r"
title = "path の種別を対象 repo の vessel 宣言が名乗る — 宣言の任意 key 3 本（prefix の配列）を編集面の guard が anchor の HEAD から読んで分類し、書かれない key は固定の判定のまま・宣言 file 自身は常に code・不正な宣言は全 file を code に倒して doctor と deny の行が理由を名乗る"
req = ["FR45", "FR41", "NFR4"]
section = "24"
write-set = ["crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/src/hook/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "+crates/scribe2/src/pipe/declaration/path_kinds.rs", "crates/scribe2/src/seat/role.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/seat/register.rs", "crates/scribe2/tests/e2e/seat.rs", "crates/scribe2/tests/e2e/snapshots/e2e__seat__seat_doctor_external_form.snap", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail hook_role_paths_", "cargo nextest run -p scribe2 --no-tests=fail seat_role_doctor_paths_"]
size = "M"
done = "toy repo に宣言を commit して PreToolUse の口から測り、3 本の key を書いた repo で宣言した仕様と設計 doc と test の dir の下の編集が orchestrator の席で通ってそれ以外は断られ、/ で終わらない項目は完全一致の 1 file だけが通り、1 本だけ書いた repo は残りの種別が固定の判定のままで、宣言 file の無い repo と key を書かない repo は今の分類と同じで、宣言 file 自身の編集は宣言が名指していても断られ、不正な宣言（parent-segment / absolute / empty / overlap / unreadable のそれぞれ）の repo は固定値なら通る path も含めて repo 内の全編集が断られて deny の行が理由の字面を持ち、commit していない作業ツリーの宣言は効かず、便の worktree の中の file も anchor の宣言で分類され、doctor が anchor ごとに paths の 1 行を default / declared / invalid の 3 つの state と invalid の理由の字面で出し、種別の集合と権能の 1:1 と宣言の schema の版は変わらない"

[[contract]]
id = "s"
title = "便を止める権能 stop — 権能の列に stop を足して停止の口を起動の権能から外し、rules 行 role.orchestrator の値に stop を足し、guard は便 1 本を名指す停止だけを stop で通す（--all と名指しの無い形は起動の権能のまま・止める口の本体は不変）"
req = ["FR41", "FR45", "NFR4"]
section = "25"
write-set = ["crates/scribe2/src/seat/role.rs", "crates/scribe2/src/hook/role_guard.rs", "crates/scribe2/src/pipe/declaration/path_kinds.rs", "rules/manifest.toml", "crates/scribe2/tests/e2e/rules.rs", "crates/scribe2/tests/e2e/hook.rs", "crates/scribe2/tests/e2e/polarity.rs", "crates/scribe2/tests/e2e/snapshots/e2e__hook__hook_brief_orchestrator.snap", "docs/design/seat-roles.md"]
verify = ["cargo nextest run -p scribe2 --no-tests=fail role_guard_stop_", "cargo nextest run -p scribe2 --no-tests=fail hook_role_stop_", "cargo nextest run -p scribe2 --no-tests=fail rules_role_stop_", "cargo nextest run -p scribe2 --no-tests=fail role_guard_", "cargo nextest run -p scribe2 --no-tests=fail hook_role_", "cargo nextest run -p scribe2 --no-tests=fail rules_embedded_manifest_", "cargo nextest run -p scribe2 --no-tests=fail hook_brief_", "cargo nextest run -p scribe2 --no-tests=fail prop_role_names_round_trip_and_reject_unknown", "cargo nextest run -p scribe2 --no-tests=fail prop_role_bash_face_allows_iff_matched_capabilities_are_held", "cargo nextest run -p scribe2 --no-tests=fail prop_role_capabilities_are_a_subset_of_the_table"]
size = "M"
done = "(1) 権能の列と全 variant の列に stop が在り loader が stop を受けて知らない名は拒む (2) 埋め込みの rules 行 role.orchestrator の値が stop を持ち裁定 id が今回の裁定で、席の指示文の権能の行に stop が出て外形 snapshot が更新される (3) guard の表が停止の口を stop に結び、窓が --run / --state-dir / --repo / --rules の flag とその値だけで --run がちょうど 1 回の停止の呼び出しが orchestrator の席で通る (4) --all を持つ形・--run の無い形・--run の値の無い形・--run と --all の両方の形・--run=<id> の 1 語の形・列の道具の flag を持つ形・値や窓に pipe や区切りや $( を含む形の停止は起動の権能へ降りて断られる (5) 停止の後ろに別の呼び出しが続く行は断られ、停止の前に別の口が在る行は両方の権能を要り、deny の行が欠けた権能を名指す (6) 受付・起動・再開・退役は起動の権能のまま、着地は merge のまま、回答と承認は今のままで、既存の歯（3 語の並びの歯は停止の期待値だけが変わる・Bash 面の歯・権能の表の in-file の prop 3 本）が緑 (7) stop を持たない行の席では名指しの停止も断られる、の 7 つを in-file の歯（停止の呼び出しの形 9 つの母集団）と PreToolUse の口の歯と rules の歯が測り、止める口の本体は 1 行も変わらない"
<!-- contracts:end -->
