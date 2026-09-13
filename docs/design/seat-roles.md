# 設計: 席の役割と権能 — 役割は閉じた enum、登録は fleet の event、権能は rules 行、執行は PreToolUse の guard、注入は SessionStart の生成文

- 要件: [FR40](../../design-intent/spec/srs.html#FR40) 席の登録 / [FR41](../../design-intent/spec/srs.html#FR41) 権能の所在 / [FR45](../../design-intent/spec/srs.html#FR45) 権能の執行 / [FR42](../../design-intent/spec/srs.html#FR42) 席の指示文の注入 / [FR30](../../design-intent/spec/srs.html#FR30) 配送構造 / [AC15](../../design-intent/spec/srs.html#AC15) [AC16](../../design-intent/spec/srs.html#AC16) [AC17](../../design-intent/spec/srs.html#AC17)・既存 [FR17](../../design-intent/spec/srs.html#FR17) / [FR18](../../design-intent/spec/srs.html#FR18) / [FR20](../../design-intent/spec/srs.html#FR20) / [FR22](../../design-intent/spec/srs.html#FR22) / [NFR5](../../design-intent/spec/srs.html#NFR5)
- 決定: [ADR-0022](../../design-intent/decisions/ADR-0022-seat-roles-are-typed-and-enforced-by-hooks.html)（本 doc の決定の正本・§2.1〜§2.8）/ [ADR-0015](../../design-intent/decisions/ADR-0015-seat-state-is-stamped-by-hooks-not-read-from-pane.html) §2.2（pane → target）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.1（列挙は core・文書は pointer）/ [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html) §2.1（guard の定義）/ [ADR-0018](../../design-intent/decisions/ADR-0018-working-memory-subcommands-and-pointer-required-directives.html) §2.2（出所 pointer）
- 土台: [seat-state.md](./seat-state.md)（打刻と pane → target）・[vessel-hook.md](./vessel-hook.md)（hooks.json の生成と guard の束）・[working-memory.md](./working-memory.md) §4（PointerKind）・[rules-manifest.md](./rules-manifest.md)・[polarity.md](./polarity.md)・[fleet-event-log.md](./fleet-event-log.md)
- 位置づけ: 役割の規律を憲法 C14 の 2 面（文書 = SRS / ADR・manifest = 権能の行）に収め、執行・注入・生成文書・drift 検査が**同じ行**を読む。散文の役割節（user の設定 file・共有 skill・退避物の命令行）は規則の置き場ではない（ADR-0022 §2.7・撤去は A1 の「消す」として別途 user に確かめる）。

## 1. 何を解くか

役割（planner / 管理席）の規律は文書（SRS FR30〜32・ADR-0016）と散文 7 面にしか無く、器は役割の型を持たない。散文でしか止まっていない事故型は 6 つ（ADR-0022 §1 (a)〜(f)）。本設計は (a)(b)(c)(e) を guard で止め、(d) を記帳の deny で効かなくし、(f) は [working-memory.md](./working-memory.md) の内蔵に委ねる。consumer の repo は席の登録だけで同じ執行と注入を得る（CLAUDE.md に役割の文を書かない）。

## 2. 役割と登録（ADR-0022 §2.1）

- **`Role`**（closed enum・core）: variant の列挙は core が持ち文書は写さない。記録時点の値は 2 つ。`as_str` / `ALL` / 判別子順の pin は既存の enum（`RuleKind` / `Guard`）と同じ形。
- **登録の subcommand**: `<NAME> seat register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR]`。`--anchor` の既定は cwd の repo root（`git rev-parse --show-toplevel`・env を読まない）。`--launch FILE` は起動の雛形（穴は口座の credential dir 1 つ・[account-autonomy.md](./account-autonomy.md) §5 が使う）で、内容を event に載せる（tracked file に置かない・CON2）。
- **event**: `EventKind::SeatRegistered`（末尾・宣言順）1 件。項目 = `role` / `anchor` / `target` / `sid`（登録を撃った session の id・SessionStart の打刻から解く）/ `account` / `launch`（雛形の本文）。項目は `Event` の typed な束 1 つ（`allowance` と同型の `Option<Registration>`・kind ではなく束の有無が本体を決める）で持つ＝`Event` を literal で組む既存の構築点（core・歯・property の生成器）と `KINDS` の件数の pin がすべて変わる（write-set は §9 (a)）。schema 1 のまま（値の追加）。
- **鍵と置き換え**: 鍵 = (role, anchor)。同じ鍵の再登録は前の row を置き換える（append のみ・replay の最新が効く・N1）。1 つの anchor に役割ごとに 1 席（FR40）。`target` / `sid` / `account` は項目で鍵ではない。pane id は鍵にも項目にも置かない。**書き手は 2 つ**: `seat register`（席の session が撃つ・打刻の条件付き）と tick の口座更新（[account-autonomy.md](./account-autonomy.md) §5・器の内部の同じ 1 関数・`target` / `sid` / `launch` は既存 row から写す）。
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
- **identity**: 生成 hooks.json の shell 行が渡す `--pane` だけ（[vessel-hook.md](./vessel-hook.md)・生成器は同じ gen-manifest）。PreToolUse の shell 行に `--pane "$TMUX_PANE"` を足し、matcher を `Edit|Write|MultiEdit|NotebookEdit` から **Bash を含む形**へ改める（Bash は PermissionRequest の matcher でもある・2 面の判定は別 hook event）。
- **解く順**: pane → target → 登録 row → role → 行 → 権能。pane が空（tmux の外・runner / lens）は席ではなく本 guard の対象外（ADR-0009 の write-set guard と allowlist がそのまま担う）。pane が在って登録 row が無い・target が解けない周は権能なし（FailClosed）。止めるのは権能付きの操作だけで、それ以外の Bash / Edit は通す。
- **deny 文**: 権能を持つ役割の名を含む（例の形: `<NAME>: この操作は <役割名> 席の権能（rules 行 <id>）`・字面は現物が正本）。**記録行**: allow の周も target と command の種別を 1 行（hook の消費記録と同じ置き場 `<state_dir>/inject.jsonl`・[vessel-hook.md](./vessel-hook.md)。打刻の `state.jsonl` には書かない）。
- **subcommand は役割を検査しない**（引数の identity は偽装できる）。発話は監視しない。

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
| `Role` | in-loop（PreToolUse・Bash / Edit 系） | FailClosed | 権能の無い役割の席からの権能付き subcommand と、権能の無い path 種別の編集（登録 row が無い・target が解けない周も deny） |

§2 の登録の拒否は行為（登録）を止める判定を返すので guard（ADR-0014 §2.1）＝`Guard::Register`（variant 1 つ・宣言順は `Cap` の直後・極性の定数は `seat/role.rs`）。§5 の注入は guard ではない（行為を止めうる判定を返さない）。

## 7. 歯（`crates/<NAME>/tests/e2e/seat.rs` に `seat_role_` 接頭辞・hook は `tests/e2e/hook.rs` に `hook_role_`・名前の列は現物が SSOT）

- 登録: `seat register` が `SeatRegistered` を 1 件追記し replay の最新が効く（同じ鍵の再登録で前の row が残ったまま最新だけが解決される）・打刻の無い session は `NoStamp` で rc 1・event なし・`--anchor` 無しは cwd の repo root・pane id は event に現れない（fixture の pane 文字列が events.jsonl に 0 回）。
- 解決: 役割の解決は登録 row だけを入力にする（pane id を差し替えた fixture でも同じ target なら同じ役割・env を置いても変わらない・同じ鍵で別 target に再登録すると旧 target では解けない・window を rename した fixture は解けない＝登録し直す）。
- guard（hook.rs・偽 tmux で pane → target を返す stub）: 管理席の target から `pipe answer` を含む Bash → deny・deny 文に権能を持つ役割の名と rules 行 id・記録行 1 件／planner の target から同じ command → allow・記録行 1 件／登録の無い pane → deny／pane 無し → 通す（記録なし）／Edit: 管理席の code path → deny・planner の design-intent → allow・契約の印で開いた便の write-set の内側 → allow・外 → deny（AC16）／権能付きでない Bash / Edit は通す。
- rules: 役割ごとの行の kind 件数 +1・値が列であること・列に無い名は `RuleError`・R-C7-1 の値の型（Str → Role の名）・rules 外形 snapshot。
- 注入（hook.rs）: 登録済みの target の SessionStart で生成文が出て権能の名がすべて含まれる・登録の無い target で 0 byte・雛形に pointer の無い行を置いた fixture で xtask check が落ちる（AC17）・行に在って文に無い権能を作った fixture で落ちる・生成文の外形 snapshot。
- 極性一覧 snapshot に `Register`（(a)）と `Role`（(b)）の 2 行（件数 +2・N = K + M の pin）・doctor の項目 1 行（`--state-dir` 付きの外形 snapshot）。
- property（`prop_role_`・in-file）: `Role` / `Capability` / `PathKind` の `as_str` ↔ parse が往復し、列に無い名は必ず Err。

## 8. 憲法・制約との整合

C1 / C5（権能の値は行・裁定 id）・C1.2（生成文に手書きの規範文 0・xtask が検査）・C2（Role / Capability / PathKind は closed enum・宣言順）・C2.2（env を読まない・identity は `--pane` の引数）・C3 / C3.3（登録は event log の replay・typed）・C7 / C7.2（対話面は R-C7-1 の値・承認は planner の席から）・C11.2（Guard は 1 極性）・C12.5（生成文と rules 表は snapshot）・C14 / C14.2（2 面と drift 検査）・C16 / C16.2（編集時に止める in-loop guard・極性一覧）・N2 / N3（散文と host の慣習を入力にしない）。

## 9. 契約（4 便・この順・実装は pipeline）

- **(a) 役割と登録**（M）: `Role`・`seat register`・`EventKind::SeatRegistered`（`Event` の束 `Registration`）・打刻の条件・`Guard::Register`・役割の解決 1 本・doctor の口と項目・歯 §7 の登録 / 解決。write-set = seat/（新 module `seat/role.rs`・極性の定数）・fleet/mod.rs（variant・`Event` の束・`KINDS`）・fleet/usage.rs・fleet/cli.rs・pipe/mod.rs（`Event` の literal 構築点）・main.rs（doctor の `--state-dir` と項目）・polarity.rs（`Guard::Register`）・tests/e2e/{seat,fleet,prop,polarity}.rs（構築点・`KINDS` の件数の pin・N = K + M）・snapshot（doctor・極性一覧）。依存: [working-memory.md](./working-memory.md) 契約 (a)（s2-07l.139・打刻の sid の読み手）の land 後。
- **(b) 権能と執行**（M）: `Capability`・`RuleKind::RoleCapabilities`（値は既存の `List`）・役割ごとの rules 行（**値と裁定 id は user 裁定**）・R-C7-1 の値の変更（裁定 id）・`PathKind`・`Guard::Role`・PreToolUse の 2 面・hooks.json の matcher と `--pane`・契約の印（field 名）・deny 文・記録行・歯 §7 の guard / rules。write-set = hook/（新 module `hook/role_guard.rs`）・polarity.rs・rules/mod.rs・rules/manifest.toml・xtask/genmanifest.rs・hooks/hooks.json（tracked な生成物・gen-manifest の出力・歯が読む）・pipe/declaration.rs（印の field）・tests/e2e/{hook,rules,polarity}.rs・snapshot。依存: (a)。
- **(c) 注入と検査**（M）: 雛形 2 枚・SessionStart の生成文・xtask の検査 3 つ・外形 snapshot・歯 §7 の注入。write-set = seat/brief/・hook/mod.rs・xtask/check.rs・tests/e2e/hook.rs・snapshot。依存: (b)・PointerKind（s2-07l.139）。
- **(d) 器の外の散文の撤去**（運用・便ではない）: (b)(c) の land 後、user の設定 file の役割行・共有 skill の役割節・退避物の役割の命令行の同文を A1 の「消す」として user に確かめてから外す（ADR-0022 §2.7）。

## 10. 却下案（ADR-0022 §5 の写しは持たない・設計固有のもの）

- 登録 row を席の state dir の file に置く。却下: 状態の置き場が 2 つになる（C3）・anchor をまたぐ突合ができない。event log の replay 1 本。
- 権能の行の値を Bool の行の束（`role.planner.answer = true` …）で持つ。却下: 権能の種類ごとに行が増え、列挙が manifest に散る。値は名の列 1 行。
- PreToolUse の Bash 面を PermissionRequest に寄せる。却下: PermissionRequest は許可の問い合わせであって編集時の deny ではない（C16）。
- 雛形を markdown の skill として同梱する。却下: ADR-0022 §5 (A)。

## 11. 後続

席の起動と登録の自動化（s2-07l.38）・plugin を積まない session の guard（s2-07l.149）・席の model の割当（別の裁定）・役割ごとの権能の値の改訂（裁定 id 付きの行の変更）。
