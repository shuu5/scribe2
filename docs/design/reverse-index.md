# 設計: code の索引と逆引きの表 — 外の道具が作った索引を器が読み、行の審査・受付・起動の列が code の事実を測る

- 出所: epic s2-07l.736.33 の打ち手 3（逆引きの表を器が機械で組む）と打ち手 6（設計に書いた code の事実を、便を起こす時点で測り直す）→ [ADR-0105](../../design-intent/decisions/ADR-0105-code-facts-come-from-an-external-index-the-vessel-reads.html)。材料は 2026-09-28〜09-30 の便 165 本の FAIL / INCONCLUSIVE 76 件の分類と、同じ日の索引の道具の試し（問い 74・採点 53・正解の site 249・base 32 本）。どちらも host の file で tracked でない。推奨の採用は常設の裁定 user 2026-09-28T00:54Z（決めてほしいことは推奨で進める）の適用で、外の道具を採ること（憲法 A3）と rules 行の値 2 つ（§4 形 7）と索引の置き場の消し（A1）は、SRS の追加 round で裁定を取った（user 2026-09-30T22:13Z 項 index-tools・項 index-cap・項 index-timeout）。
- 要件（今の字）: [FR48](../../design-intent/spec/srs.html#FR48) 閉包 / [FR55](../../design-intent/spec/srs.html#FR55) CI の契約表の検査 / [FR47](../../design-intent/spec/srs.html#FR47) 契約の正本 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR68](../../design-intent/spec/srs.html#FR68) 起動の列 / [NFR3](../../design-intent/spec/srs.html#NFR3) 依存 / [NFR6](../../design-intent/spec/srs.html#NFR6) host の資源。FR48・FR55・FR47 の字を直し、要件を 3 つ足す SRS の追加 round が先に要る（§12）。
- 前提: 審査役が読みの道具を持つ設計（[pipeline.md](./pipeline.md) §64・行 bg）、行の審査（[row-review.md](./row-review.md)・[ADR-0103](../../design-intent/decisions/ADR-0103-contract-rows-pass-row-review-before-merge-and-failed-rows-keep-their-place.html)）、done の項目ごとの歯の欄（[contract-source.md](./contract-source.md) の done の欄の設計・同じ epic の別の PR・未着地）。表を材料に足す場所と受付の検査に足す場所は、この 3 つの設計の口に揃える。
- この設計から出る契約: §12 の 10 行。行 0 は [contract-source.md](./contract-source.md) の契約表の行 bu（§67）に束ね、行 f は SRS の round を待たずに本 doc の契約表に足した（§15）。行 a1・a2・b・c・d・e・g は SRS 0.33 の round と A3 の裁定（user 2026-09-30T22:13Z 項 index-tools）の後に本 doc の契約表に足した（行 a は NFR2 の見積の内に収めるため a1 と a2 に割った）。行 h は条件つき、行 i は行の審査の口の着地の後に足す。

## 1. 何を解くか

設計が code の事実を誤る・知らない型が、便の失敗の最大の根である。

- FAIL / INCONCLUSIVE 76 件のうち、設計が code の事実を誤った・知らなかった型（D2）は 32 件。分類の「機械で先に取れた」の印は、索引で取れる 27 件（D2 18・書き足りない 6・食い違い 3）と、compiler の試し撃ちで取れる 6 件の計 33 件。
- 設計者に渡っていなかった事実の型: 型を名指す file の集合（ほかの行の touches の閉包）・struct の literal の全 site・関数の呼び手・既存の歯が pin する物・item と親 module の可視性・path 形の名指しの site。どれも「名の全 site とその役」の問い。
- 設計の時点では正しく、便を起こすまでの間に別の着地で変わった事実が 2 件ある（行 aa の便・行 aj の便）。
- 試しの結論（§3）: 名の単語で `git grep` を撃つだけでも事実の 88% は手に入っていた。ただし余分な答えが多すぎて（precision 0.023・1 問に数百〜千数百の行）、設計者は「引けなかった」のではなく「埋もれて読めなかった」。

やさしく言うと: 設計を書く人が「この型はどこで組み立てられているか」「この関数を誰が呼んでいるか」を知らずに書いて、便が落ちる。器が code の索引（どの名がどこで・どの役で使われているかの表）を外の道具に作らせて自分で読み、行ごとに「触る物の使われ方」を件数つきの表にして設計者と審査役に見せ、受付で足りない file を機械で断り、設計に書いた数が便を起こす時点でまだ正しいかを測る。

## 2. 何が起きているか（main ecf841ca・verified）

- **字面の閉包**: `crates/scribe2/src/pipe/closure.rs` は touches の型を構造として持つ file を字面で集める（形 1 literal 構築・形 2 match の arm・形 3 件数 pin・形 4 const slice の宣言・形 6 variant 構築）。file の頭の doc が「下界である」と書き、別名（`use … as`）・`Self { … }` の構築・glob の取り込みを見ない。上界は構文木が要り A3 の依存になるので却下した（[contract-source.md](./contract-source.md) §11「閉包を構文木（syn 等）で求める」）。fn 形の touches は fn を宣言する file だけを数え、呼び手は数えない。
- **名指しの読み手**: `crates/scribe2/src/pipe/closure/names.rs` の `unresolved_names` は、§ の散文の backtick の中身のうち path 形・型の path 形・fn 形を名指しと読み、base に解けない名を返す。審査の材料の名の照合は同じ file の `mentioned_names`。
- **受付の 1 行の検査は起動の列でも撃たれる**: `crates/scribe2/src/pipe/cli/intake.rs` の `generated` は行を読んで表の検査を 1 行に撃ち、findings を断りにする。起動の列の候補（`crates/scribe2/src/pipe/dispatch/candidates.rs` の `entry_of`）も同じ `generated` を撃ち、断りは待ちの理由 admission になる。受付の判定の本体は同じ file の `judge`（交差・余地・同時本数）。
- **審査の材料**: `crates/scribe2/src/pipe/review.rs` の材料の file は design.txt・requirements.txt・promises.txt・base.txt・outside.txt・items.txt の 6 つ。outside.txt（`crates/scribe2/src/pipe/review/outside.rs`・1082 行）は「lens は shell も cargo も撃てない」ことを理由に、契約が名指す write-set の外の item の本文と要約を字面の照合で束ねる。
- **受付札**: `crates/scribe2/src/pipe/admission.rs` の `admit` が host の memory と core と生きている札から枠を配る。job 1 つの memory は rules 行 `gate.job_memory_mb`（3072）。
- **裏で起こす形**: 先撃ちの `fire`（`crates/scribe2/src/pipe/dispatch/prelens.rs`）は、起こす側の周が lens を process group を分けて起こし、終わりを待たない（`crates/scribe2/src/pipe/dispatch.rs` の `spawn_self` と同じ起こし方）。
- **vessel 宣言の任意 key**: `crates/scribe2/src/pipe/declaration/optional_keys.rs` の DECLARED_KEYS は 17 key の閉じた列で、未知の key を持つ宣言は読めない。
- **行の欄**: `crates/scribe2/src/pipe/table.rs` の FIELDS は 18 欄の閉じた列で、未知の欄を持つ行が在る doc は区間ごと読めない。
- **入れ子の JSON の読み手**（main 92a5ddc8 で測り直した）: `crates/scribe2/src/fleet/json_tree.rs` の pub な parse が 1 本在り、入れ子の object・配列を読み、数を 10 進の字のまま持つ。口座の面・床の検査・未反映の裁定の読みが同じ 1 本を呼ぶ（account の私有の `read_tree` もこれを包むだけ）。書き手は真偽の 1 形だけ。
- **床の検査の撃ち方**（main 92a5ddc8・verified）: `crates/scribe2/src/pipe/dispatch/floor.rs` は宣言の 1 行を空白で割り、program を shell の command -v で解き、commit を detach した木（Worktree の make・pub(in crate::pipe)）の上で封じ込めの箱に包んで撃ち、時間の上限で process group ごと止める。撃ち中の印は `<sha>.lock`（pid と起動時刻・死んだ持ち主の印は 1 回だけ外す）で、印・撃ち（run）・program の解き（resolve）は私有。撃てない形は語（path・row・confine・tree）で残す。
- **toolchain**: `rust-toolchain.toml` は channel 1.98.1 と component clippy・rustfmt。rust-analyzer は component に無い（host の toolchain には在った）。
- **依存**: 実行時の直接依存は 0 本（NFR3・`cargo xtask check` の deps-empty）。器の子 process は封じ込めの中で走る（NFR6）。
- **言語の集合**: 持ち主は 2026-09-27 に Rust 専用の索引の道具（rust-analyzer の LSIF / SCIP）の採用を断り、器が乗る言語を Rust・TypeScript・typed Python と React Native + Expo に決めた（裁定 user 2026-09-27T17:33Z・台帳 s2-07l.669 の notes・逐語は台帳にだけ在る）。
- **索引の候補の枠**: vocabulary は、seam だけを先に置き採否を測って決める外部依存の枠（day-1 optional adapter）に graphify（land ごとに知識の graph を作る候補）を置いている。
- 母集団: 契約表の行 434、touches を持つ行 76、touches の項目の異なり 50。

## 3. 試しの結果（host の測り・2026-09-30）

- 問いは FAIL の code の事実 44 件から作った 74 問（採点 53 問・正解の site 249）。型は、参照の全 site（a）・struct の literal（b）・呼び手（d）・値を pin する歯（e）・可視性と経路（f）・影響範囲（g）。道具では引けない 21 問（器の規則・外の CLI の仕様・実行時の実測）は数えの外。
- 採点は同じ規則（その問いの正解の site を全部出せたら合格）。

| 答え | 合格（53 問） | case の全合格（34 件） | site の recall | site の precision |
|---|---:|---:|---:|---:|
| 名の単語の grep（後処理なし） | 43 | 25 | 0.884 | 0.023 |
| SCIP（rust-analyzer）だけ | 42 | 26 | 0.884 | 0.525 |
| **SCIP + 構文の分類** | **49** | **31** | **0.968** | **0.780** |
| compiler の試し撃ち（改名・欄を足す・私有化） | 20 | 10 | 0.446 | 0.661 |
| compiler の試し撃ち（#[deprecated]・doc の link） | 39 | 21 | 0.751 | 0.652 |
| 既製の graph 3 つ（code-review-graph・graphify・codebase-memory-mcp・行で数える） | 10〜16 | 4〜5 | 0.36〜0.45 | 0.39〜0.84 |
| 同じ 3 つを関数単位で数える（正解の行を囲む関数が合えば当たり・recall と precision も関数で数える） | 14〜27 | 5〜13 | 0.47〜0.70 | 0.38〜0.50 |

- SCIP + 構文の分類は、型 a・b・d・e・f の 211 site で recall 0.995・precision 0.972。同名の別 symbol が在る 16 問で別物を出した数は 0。落ちは索引の外の file（toml の 3 site）と型 g の 1 問。
- SCIP（rust-analyzer 1.98.1）に出ない物: doc の link（1 base に 3551 個）・`format!` の文字列の暗黙の取り込み・`Self { … }` の literal（impl の symbol に付く）・test の役・use と呼び出しの区別・source の字の可視性（署名の文は正規化した実効の形で、`pub(in …)` の path が消える）。どれも構文の層で足した。
- 注意: 規則 3 つ（format の取り込み・doc の散文の名指し・文字列の中の数）は同じ bench の落ちを見てから足した。新しい問いでは割り引いて読む（新しく作った 10 問では recall を落とさなかった・下の監査のやり直し）。
- 既製の graph は、行で数えると literal（型 b）が 3 つとも 0/5、可視性と mod（型 f）が 0〜1/7。道具が名乗る参照と呼び手の 33 問（型 a・d）に絞っても、名の単語の grep と SCIP + 構文の分類の 32/33 に対し、graph は行で 8〜12・関数単位で 10〜20。
- codebase-memory-mcp は呼び手（型 d）を関数単位なら 14/16 当てる（行では 8/16）。呼び手の関数の下見には使えるが、行の網羅には使えない。呼び手は SCIP + 構文の分類が行で 16/16 答えるので、その下見も要らない。
- graphify は Rust で path と名を小文字にして node を潰す（fn と同名の struct が 1 つになる）。source と小さな見本の crate で確かめ、設定では避けられない。main の木では item の 1.5% が消える。
- 監査のやり直し（2026-10-01）: 元の採点の不公平を 2 つ見つけ、直した数字を上の表に書いた。
  - (1) 行が一致した site だけを数えたので、呼び手の関数を名指すが行を持たない辺が 0 点になった。関数単位で数え直すと codebase-memory-mcp で 11 問・graphify で 3 問・code-review-graph で 1 問ぶん。
  - (2) code-review-graph の `references_to` の問い合わせを使わなかった（1 問）。ほかに codebase-memory-mcp の辺の引数の欄を読んでいなかった（直しても合格は同じで、site が 2 つ増えるだけ）。graphify は同名の型の stub も辿る版を採った（+1 問）。
  - bench に無い名と場所で新しく作った 10 問（正解 55 site）では、SCIP + 構文の分類が 10/10（precision 0.90）で、bench を見て足した規則は recall を落とさなかった。graph は行で 1〜3/10、関数単位では codebase-memory-mcp だけが 8/10。
  - 3 つとも網羅の列挙を約束していない（codebase-memory-mcp は README と論文で、graphify は docs で自ら否定する）。測ったのは器の用途（行の網羅）で、道具の本来の売り（token の削減・全体像）ではない。
- 費用: SCIP の索引 1 本は wall 約 30 秒（host の load が高いと 80〜105 秒）・peak RSS 約 2.5 GB・`.scip` 35〜41 MB。増分は無く、1 commit ごとに全部作り直す。compiler の試し撃ち 1 回は増分で 5〜9 秒。
- 決定性と egress: 同じ base の 2 回の出力は byte で同一。network を切った名前空間の中で索引と問いが最後まで通り、答えは同一。rust-analyzer は build script と proc-macro を実行する（cargo build と同じ信頼）。

## 4. 索引の宣言と組み立て（契約表の行 a1・a2）

- 行の割り方（NFR2 の見積 550 行の内に収める）: 行 a1 は読み手と表（形 3・4・5、形 6 の鍵の digest、形 9 の表の問いと役の語の列）で、I/O を持たない pub の関数だけを置き、e2e が直に撃つ。行 a2 は組み立ての口（形 1・2、形 6 の置き場と記録と印、形 7・8、形 9 の状態の読みと組み立てと索引の宣言の読み、口 `pipe index build`、rules 行 2 本）で、a1 の関数を呼ぶ。a1 の module は pipe の子 module index（その下に子 2 つ）、a2 の module は床の検査と並ぶ起動の列の子 module に置く（a2 が a1 の file を書き換えないので、a2 の write-set は a1 の着地の前にも解ける）。
- 形:
  1. **宣言**（行 a2・vessel 宣言の任意 key 2 つ・どちらも command の字の列・行 bu が置いた読みに値を持たせる）:
     - `index-scip`: 1 行ごとに SCIP の file を 1 つ出す command。穴は `{tree}`（索引を作る commit の木）と `{out}`（出力の file の path・器が行ごとに別の名を渡す）。言語ごとに 1 行（Rust は rust-analyzer・TypeScript は scip-typescript・Python は scip-python か ty-scip）。
     - `index-roles`: 1 行ごとに構文の役（§5）の一致を stdout に 1 行 1 件の JSON で出す command。穴は `{tree}`。本 repo は ast-grep の scan に役の規則の file を渡す 1 行（§10・行 b）。
     - 索引の宣言の読み（vessel 宣言の parse とは別の 1 関数）は、2 key の片方だけの宣言と、穴の欠けた行（`index-scip` の行に `{tree}` か `{out}` が無い・`index-roles` の行に `{tree}` が無い）を、key の名と行番号を名指す不備にする（状態は half）。この読みを撃つのは索引を要する周（touches に型を持つ契約・欄 `code-facts` を持つ行・行の審査・表を出す口）だけで、vessel 宣言の parse・ほかの key の読み・索引を要しない契約の受付・contracts check・FR101 の merge の門の宣言の読みは、片方だけの宣言でも今のまま読む（SRS FR107）。
     - どちらも無い repo は索引を持たない（状態は undeclared・§7 の判定は欄 `code-facts` を持つ行のほかは撃たない）。
     - command の許しは床の検査の 1 行（`floor-check`）と同じく allowlist で見ない（宣言は tracked で行の審査と merge を通り、撃つのは封じ込めの箱の中）。
  2. **撃つ所**（行 a2）: 床の検査と同じ撃ち方（§2）を共用する。索引を作る commit を detach した木（床の検査の Worktree の make・make は sha を木の名と detach する commit の両方に使うので、置き場は索引の dir の下の `<commit の sha>.tree`）の上で、cwd をその木にして宣言の順に撃つ（行を空白で割り、穴を語ごとに埋め、program は床の検査の resolve で解く・rustup の proxy は cwd の toolchain の宣言で版を選ぶ）。撃つ子は封じ込めの箱の中で走り（NFR6）、受付札を 1 枚（jobs 1・job の memory は `gate.job_memory_mb`・`crates/scribe2/src/pipe/admission.rs` の admit と release・rules は gate の Limits から組む）取る。時間の上限は rules 行 `index.timeout_s`（600 秒・user 2026-09-30T22:13Z 項 index-timeout）で、越えた子は床の検査の run と同じく process group ごと止める。床の検査の印・run・resolve は同じ親の兄弟の module から呼べる可視性に開いて共用する（2 本目の撃ち方を作らない・C6）。
  3. **読む**（行 a1）: 器は外の道具の library を持たない（NFR3）。
     - SCIP は protobuf の wire の形のうち、document の相対 path と position_encoding・occurrence の範囲と symbol と役の印（定義か）と囲む範囲・symbol の情報の名と種類だけを std の読み手で読む。知らない欄は wire の型で読み飛ばす。外の crate の symbol（package が repo の外）と関数の中の local は落とす。壊れた wire（途中で切れた varint・長さが本体を越える欄）は読めない理由にする。
     - 役の一致は JSON の 1 行ずつを `crates/scribe2/src/fleet/json_tree.rs` の parse（2 本目の JSON の読み手を作らない）で読み、key は ast-grep の stream の JSON の 1 行の部分集合（ruleId・file・range の byteOffset の start と end・metaVariables の single の NAME の text）だけを読む。ruleId が §5 の 9 語の外の行は捨て、捨てた数を返す。JSON でない行は読めない理由にする。
  4. **結び**（行 a1）: occurrence と役の一致を、file の先頭からの byte の位置に直して比べる（SCIP の列は document の position_encoding の単位で読み、呼び手が渡す file の本文で byte に直す）。occurrence ごとに、範囲を含む最も内側の役を付ける。役の一致のうち SCIP の occurrence を持たない物（doc の link・文字列の取り込み・`Self` の literal）は、捕えた名の字と同じ字の occurrence を、囲む定義 → 同じ file の順に探して symbol を借りる。借りられない物は字だけの行として残し、印を付ける。module を宣言する occurrence が test の役の中に在る module の file は、全体を test にする。
  5. **平らな表**（行 a1・器が書き器が読む on-disk の形・schema 1）: 1 行 1 occurrence で、path・行・列・symbol・定義か・役の列・test か・囲む定義の symbol・source の可視性の字を tab で区切る。頭の行は `schema=1`。描きと読みは 1 対で、読みは描きの逆（往復で等しい）。schema の違う表は「無い」と読み、形の崩れた行は読めない理由にする。書きは一時 file → rename（行 a2）。形を変える版は schema を上げ、古い表を読まない（作り直せる）。
  6. **鍵と置き場**: 索引の鍵は、code の木の鍵（[row-review.md](./row-review.md) §5 と同じ定義・commit の tracked から契約表を持つ file を除いた全 file の path と blob の hash の列の digest・役の規則の file も tracked なのでここに入る・行の審査の行 a が置く 1 本を行 a2 が呼ぶ）と、宣言 2 key の字を並べた字の FNV-1a 64（16 桁・行 a1 の digest の関数・`crates/scribe2/src/hook/vessel/digest.rs` の FNV-1a 64 を呼ぶ・2 本目の hash を作らない）。置き場（行 a2）は state dir の pipe の下の index の dir で、鍵ごとに平らな表と記録（1 行 1 key の `key=value`・1 行目は `schema=1`・key は key・commit・宣言の digest・rows・files・dropped・secs・at・stderr の末尾）。撃ち中の印は床の検査の印（`<鍵>.lock`）。外の道具の出力（SCIP の file と一致の列）と木は平らにした後に外す。
  7. **量の上限と消し**（行 a2）: 置き場の合計が rules 行 `index.cap_mb`（2048 MiB・user 2026-09-30T22:13Z 項 index-cap）を越える周は、撃ち中の鍵と、anchor の HEAD の鍵を除いて、記録の at の古い順に上限まで消す（[ADR-0101](../../design-intent/decisions/ADR-0101-seat-draft-build-dirs-are-capped-per-state-dir-and-shed-oldest-first.html) と同じ形・消すのは器が作った導出値だけ・A1 は項 index-timeout の裁定で済み）。撃つのは組み立てが表を書いた直後の同じ印の中（2 つ目の掃除を足さない）。rules 行 2 本のどちらかを読めない周は組み立ても消しも撃たず、記録に `failed:no-rule` を書く（既定値に倒さない）。
  8. **無い・壊れた・古い**（行 a2）: 表が無い・schema が違う・読めない鍵は「無い」と読む。組み立てが rc 0 でない・時間切れ・出力を読めない・撃てない（床の検査の撃てない形と同じ語）周は、記録に失敗の語（`failed:<rc|timeout|unreadable|no-rule|path|confine|tree>`）を書き、表は置かない。索引は導出値で、真実の置き場にしない（憲法 C3・C10）。
  9. **後の行が呼ぶ口**:
     - 行 a1（pipe の子 module index・pub）: SCIP の読み・役の一致の読み・結び・表の描きと読み・鍵の digest の 5 関数と、表の問い（読めた表と項目 1 つから、項目の path を SCIP の descriptor の名の列の末尾一致で symbol に解き〔0・1・複数の 3 形〕、symbol ごとに役つきの site の列〔path・行・役・test か・囲む定義〕を返す）と、役の語の閉じた列（§5 の 9 語・宣言の順）。行 a2・c・d・e と e2e の歯が撃つ。
     - 行 a2（起動の列の子 module・pub(in crate::pipe)）:
       - 状態の読み（引数は state dir・repo・commit の sha・返りは閉じた 6 値・読むだけで撃たず待たない）: 宣言が 2 key を持たない → undeclared／片方だけか穴の欠け → half（不備の列を持つ）／表も印も失敗の記録も無い → absent／印の持ち主が生きている → building／記録が失敗の語を持つ → failed（語を持つ）／表を読める → ready（表を持つ）。受付と起動の列が撃つ。
       - 組み立て（引数は state dir・repo・commit の sha・rules・返りは built・cached・failed の 3 形と鍵、half と undeclared は撃たずにその値）: 同じ鍵の撃ち中の印の持ち主が生きている周は撃たずに、唯一の待機の実装（`crates/scribe2/src/fleet/wait.rs` に、pid の消えを待つ既存の値 RunnerExited を包む pub の 1 関数を足して撃つ・述語は /proc の有無・新しい値は足さない・deadline は rules 行 `index.timeout_s` の秒）で持ち主の終わりを待ってから状態を読み直す（ready → cached・failed → その語・deadline を越えた周は記録を書かずに failed:timeout と読む）。口 `pipe index build`・口 `pipe index show`（行 c）・行の審査（行 i）・行 d の裏の起こしの子がこの 1 本で索引を得る。受付と起動の列は組み立てを撃たず、状態の読みだけを撃つ（作り中に出会うのは受付と起動の列だけ）。
       - 索引の宣言の読み（pub・宣言の file の中の `floor_check_at` と同じ形・sha の木の宣言から 2 key の行の列の組を返し、2 key が無い周は無し、片方だけと穴の欠けは key の名と行番号の不備の列）: 状態の読みと行 b の歯が撃つ。
- 口（行 a2）: `<NAME> pipe index build --repo R --state-dir S [--ref <sha>]`（既定は HEAD）。同じ鍵の表が在れば撃たずに cached、撃ち中の印の持ち主が生きていれば撃たずに終わりを待って読み直し（形 9）、失敗の記録の在る鍵は撃ち直す。前面で最後まで走る。結果の 1 行は `[INDEX] key=<16 桁> rows=<n> files=<m> built|cached|failed:<語> secs=<s>`。rc は built・cached が 0、failed が 1、索引の宣言の不備（half）・宣言を読めない・repo でない・ref を解けない周が 2（half は key の名と行番号を名指す）。宣言が 2 key を持たない repo は `[INDEX] undeclared` の 1 行で rc 0（撃たない）。
- 閉包: 行 a1・a2 の module は、ほかの行が touches に持つ閉じた型（完了待ちの値・封じ込めの結果・契約表の行・表の検査の断り・受付の断り・rules の kind）を組まず match もしない。待ちは wait.rs の 1 関数を呼び、箱は封じ込めの結果の confined の値で読み、rules 行は id の字で読む（新しい file で組むと、その型を touches に持つ着地済みの行の閉包が広がり CI の契約表の検査が落ちる）。
- 変えないもの: 床の検査の撃ち方・語・置き場・歯の字、vessel 宣言の parse と既存の key の読み、受付・起動の列・審査の材料（行 c・d・e が変える）。
- rules 行 2 本の kind は ALL の末尾に足す（便の base の末尾の後ろ・同じ時期に別の行が ALL の末尾に kind を足すなら、後に着く便が末尾の pin を直す）。

## 5. 構文の役（閉じた語の列・言語は宣言の側）

- 器が知るのは SCIP の形と、役の語の閉じた列だけ。どの構文の node がどの役かは、宣言が名指す役の規則の file（言語ごと）が持つ。規則の id は役の語と同じ字にする。
- 役の語（9 つ）: literal（struct・record の literal の名・`Self` を含む）・pattern（match の pattern の位置の path）・call（呼び出しの callee）・use（import の宣言の中）・reexport（公開の再輸出の中）・test（test の関数・test の module・test の file の範囲）・doclink（doc の link の中の名）・capture（文字列の中の取り込みの名）・vis（定義の可視性の字を捕える）。
- Rust・TypeScript・typed Python は SCIP の indexer を持つ（rust-analyzer・scip-typescript・scip-python / ty-scip）。ast-grep は 3 言語の文法を 1 つの binary に持つ。言語を足すのは宣言の 2 key と規則の file を足すことで、器の code は変わらない。
- 今 Rust の形に閉じている所（言語を足すときに広げる所）: touches と名指しの path の形（`crate::` の頭と `::` の区切り）・字面の閉包（`.rs` の 6 形）・歯の名の読み手（nextest の形）。索引はこれらを置き換えず、touches の path を SCIP の descriptor の名の列の末尾一致で symbol に解く。

## 6. 逆引きの表（項目・列・母集団）

- 項目: 契約表の行ごとに、(i) touches の項目と、(ii) 行が実装する § の散文の名指しのうち型の path 形と fn 形（`unresolved_names` と同じ読み・下の行 c の形 4 の口の 1 本で読む）。項目ごとに symbol へ解き、解けない（0）・1 つ・複数（`ambiguous:<n>` と候補の定義の site）を名指す。`--item` は同じ解き方で名を 1 つ受ける。
- 列（7 つ・どれも件数と site の列）:
  1. refs: 参照の site（定義を除く）。本体と test に分け、file の数を添える。
  2. callers: 関数の項目の、call の役の site を囲む定義。本体と test に分ける。
  3. literals: literal の役の site（`Self` を解いた物を含む）。
  4. patterns: pattern の役の site（型と variant）。
  5. teeth: 項目を参照する test の関数の名と site。
  6. vis: 定義の source の可視性の字と、親 module を crate の根まで辿った各段の可視性と、reexport の役の site。
  7. rows: 項目を touches に持つほかの契約表の行（doc#行 id）と、その行の write-set の外に在る項目の site の file（その行の閉包を広げる file）。台帳は読まない。
- write-set の印: 行を名指した周は、site の file が行の write-set（導出値を含む）の外なら `外` を付け、列ごとに外の件数を出す。
- 母集団（件数と同時に出す）: 項目の最後の節の名が tracked file に語の境界で現れる件数（`text=`）・そのうち索引が解いた件数（`indexed=`）・索引の外の file（`.rs` 以外など）に現れる件数と path（`outside-index=`）。「0 件」を「測れていない」と区別する。
- 出す所: `pipe index show` の stdout（切り詰めない）と、審査の材料の file index.txt（§7 (a)）。表の字の組みは 1 か所（index の子 module）に置き、両方が同じ字を出す。
- 材料の大きさ: index.txt は審査の材料の既存の予算（`gate.token_cap` の残り）に項目ごとに収め、収まらない項目は件数だけの 1 行にし、落とした項目の数を最後の 1 行に数える（数えるのは項目だけで、見出しと告げる 1 文は数えない・outside.txt と同じ扱い・新しい閾値を作らない）。

- 行 c の形（番号は行 c の done と 1:1）:
  1. **口**: `<NAME> pipe index show --repo R --state-dir S [--ref <sha>] (--row <doc>#<行 id> | --item <path>)…`（既定は HEAD・--row と --item は 1 つ以上・混ぜてよい）。§4 形 9 の組み立ての 1 本で ref の commit の索引を得て（無ければ組み、撃ち中の持ち主が生きていれば終わりを待つ）、項目ごとに上の表を stdout に出す。組み立てが失敗した周は `index=unavailable:<失敗の語>`、宣言が 2 key を持たない repo は `index=unavailable:undeclared` の 1 行だけを出して rc 0。--row は ref の木の契約表（docs/design の直下の .md）から行を引き、touches と § の名指しを項目にする（行 f と同じく read_table の返りを field で読み、契約表の行の型を名で書かない）。引けない行・解けない ref・宣言を読めない周は rc 2。引数の許しは verb ごとに分け、--row と --item は show だけが受ける（build に渡すと今のまま未知の引数の使い方の誤りで rc 2）。
  2. **列の数え**（審査の新しい子 module の中〔外の材料の子 module と同じ置き方〕・pub(in crate::pipe)）: ready の表・repo と commit・項目 1 つ・行の write-set（--item では空）から、7 列の件数と site と write-set の外の印と母集団を返す 1 関数。表の字の組み（描き）も 1 関数。show・index.txt・行 e の測りが同じ 2 本を撃つ（2 本目の数えを作らない）。
  3. **材料**: §7 (a)。
  4. **§ の名指しの読み**（項目 (ii)）: 名指しの読み手（`crates/scribe2/src/pipe/closure/names.rs`）に 4 本目の口 section_symbols を置き、親（`crates/scribe2/src/pipe/closure.rs`）の pub use の 1 行に足して pipe::closure の path で引く（names の module は私有のまま）。署名は pub fn section_symbols(texts: &[&str], touches: &[String]) -> Vec<String>。本文の backtick の中身を `unresolved_names` と同じ私有の form_of（touches の各項目の末尾の節を touched に渡す）で読み、型の path 形は中身の先頭の token（`::` の節の列のまま・`crate::` の頭を問わない）を、fn 形は識別子を、本文の順と現れた順に重複を除いて返す。path 形と散文（予約語の呼び出しの形・`pub(crate)` のような可視性・大文字始まりの tuple variant の構築・touches の型の variant・末尾 `::` の module path）は返さない。審査の子 module は § の名指しを自前で読まない（form_of の写しを作らない）。
- 変えないもの（行 c）: 名指しの読み手の既存の 3 口（`unresolved_names`・`mentioned_names`・`named_items`）の字と返り、口 `pipe index build` の字と rc、審査の既存の 6 材料の字と置き方、lens の雛形の既存の字と穴の順（index.txt の穴は末尾に足す）。

## 7. 使う場所

索引を要する行は、touches に型の項目（fn 形でない項目）を持つ行と、欄 `code-facts` を持つ行だけである。どちらも持たない行は、索引のどの状態でも今のまま（待たない・断らない・出力の字も変えない）。状態の語は §4 形 9 の 6 値（undeclared・half・absent・building・failed・ready）で、absent と building を合わせて「作り中」、failed を「作れない周」と呼ぶ。half（片方の key だけか穴の欠け）は索引を要する周の索引の宣言の読みの断りで、索引を要しない周は half を読まない（§4 形 1）。行の審査（行 i）と `pipe index show` は §4 形 9 の組み立ての 1 本で索引を得て、撃ち中の持ち主の終わりを待つので、作り中に出会うのは受付と起動の列だけである。判定は受付の括弧（FR68）の「契約表の行の閉包と code の索引と code の事実の判定」の内に在る。

### (a) 審査の材料（行 c・行の審査の材料は行 i）
- 契約の審査（Reviewed）の材料の dir に index.txt を足す。中身は §6 の表（審査の木の commit〔Reviewed が材料を組む前に 1 回読む HEAD〕の索引・行を名指す形・行の write-set で外の印を付ける）。状態が ready でない周は `index=unavailable:<状態の語>` の 1 行だけを置く。undeclared の repo は file を置かない（今の材料のまま）。置くのは既存の材料を置いた後で、state dir を持つ Reviewed だけが置く。
- 先撃ちは index.txt を置かない（先撃ちは行の審査の行 e で退役する）。索引を名乗る repo では、先撃ちの判定の使い回し（材料の鍵の一致）が外れる。
- lens は index.txt を雛形の末尾の穴（outside の穴の後ろ）に、outside.txt と同じく `gate.token_cap` の残り（outside を足した後）に項目ごとに収めて埋める（収まらない項目は件数だけの 1 行・落とした項目の数を最後の 1 行）。収める 1 関数は行 c の審査の子 module に pub で置き、外の材料の `outside_block`（`crates/scribe2/src/pipe/review/outside.rs`）と同じく lens が撃つ。見出しの下の 1 文で「index.txt は器が外の道具の索引から組んだ事実で、件数の横の母集団を合わせて読む・site は読みの道具で開ける（[pipeline.md](./pipeline.md) §64）」と告げる。file が無い周は穴が空で、雛形の出力は 1 字も変わらない。
- 行の審査の材料（[row-review.md](./row-review.md) §3 形 5）の index.txt は、行 i が同じ 1 本で置く。index.txt は材料の dir の file なので、材料の鍵に自然に入る。

### (b) 受付の索引の閉包（行 d）
- 受付の判定（契約表の行の閉包と code の索引と code の事実の判定・FR48・FR107・FR109）の材料（intake の Materials）は索引の状態を 1 つ持つ。state dir を持つ呼び手（手の受付・`pipe preflight`・起動の列の候補）が、base の commit の状態を §4 形 9 の状態の読みで載せる。予想の base の写し（Materials の forecast）と、状態を載せない呼び手は「状態なし」。表の検査を直に撃つ CI の contracts check は状態を持たず、今の字面の閉包のまま。
- 索引の閉包（字面の閉包と同じ file の 1 関数・行 a1 の表の問いを呼ぶ）: touches の型の項目ごとに、字面の閉包の形 1（literal）・形 2（match の arm）・形 6（variant 構築）と同じ形を、字の代わりに索引の解いた symbol で数える（literal の役・pattern の役・variant の symbol の本体の参照）。形を足さず、別名・`Self`・glob の越しの site が加わるだけ（字面の閉包の上に足す）。形 3・形 4 と fn 形は字面のまま。
- 索引の閉包が名指し、字面の閉包が名指さない file が write-set に無ければ、字面の閉包と同じ write-set-incomplete の断りを、file に ` (索引)` を添えて出す（確定の断り・在り処は本文の読み手）。判定は `generated` の表の検査の後に撃ち、表の検査の findings に足す。
- 状態ごとの扱い（touches の型を持ち、欄 `code-facts` を持たない行）:
  - ready: 索引の閉包で判じる。
  - 作り中（absent・building）: 手の受付と `pipe preflight` は断り index-building（状態の語を名指す・在り処は置き場＝予想の base では測れない）で断り、便を作らない。起動の列はその候補を受付の理由 index-building で待たせる（`dispatch ls` の reason は admission:index-building）。
  - 作れない周（failed）と状態なし（予想の base）: 字面の閉包だけで判じ、受付の結果の行と `pipe preflight` の出力と `dispatch ls` の候補の行に ` index=unavailable:<語>`（failed の語か forecast）を足す（止めない、縮退する・[gate-cost.md](./gate-cost.md) §2 と同じ極性）。
  - undeclared と、状態を載せない呼び手: 今のまま（尾も足さない）。
- half の repo: 索引を要する行（touches の型を持つ行と欄 `code-facts` を持つ行のどちらも）の手の受付と `pipe preflight` は、今の宣言の不備と同じ断りの名 declaration（key の名と行番号を名指す・rc 1）で断り、起動の列はその候補を受付の理由 declaration で待たせる（`dispatch ls` の reason は admission:declaration）。索引を要しない行の受付・起動の列・contracts check は half の repo でも今のまま通る。断りの名は declaration の 1 つに決める（欄を持つ行も同じ名・code-facts-unmeasured にしない）。
- 欄 `code-facts` を持つ行は、touches の型を持っても index-building にせず、(c) の code-facts-unmeasured に寄せる（ready の周は索引の閉包も撃つ）。
- 裏の組み立て: 起動の列の周は、索引を要する候補が在り、HEAD の状態が absent のとき、`pipe index build --ref <HEAD>` を 1 本、裏で起こして待たない（`crates/scribe2/src/pipe/dispatch.rs` の `spawn_self` と同じ起こし方・撃ち中の印が 2 本目を止める）。failed の鍵は列が起こし直さない（毎周の失敗を避ける・code を変える着地で鍵が変わるか、手の `pipe index build` が撃ち直す）。
- 新しい `+` の file が将来名指す型は、受付の時点で file が無いので測れない。代わりに、runner の stdin に「ほかの行の touches の型」の節を足す: 行の write-set の外の行（open な行と着地済みの行）の touches の型の列と、その行 id（契約表から導く・索引は要らない）。runner がその型を字面の閉包の形で名指すと、終わりの門（[pipeline.md](./pipeline.md) §66）と gate の共通の検証の閉包の歯が落とす。節は、落ちる前に runner に知らせる形である（行 f・§15）。

### (c) code の事実の欄と、起動の列の測り直し（行 e・打ち手 6）
- 行の欄 `code-facts`（任意・文字列の列・行 bu が読みを置いた）: 要素 1 つが `<列>:<項目>=<値>`。
  - 列は §6 の refs・files（refs の file の数）・callers・literals・patterns・teeth・vis の閉じた 7 語。
  - 項目は touches と同じ path の形（`crate::` で始まり `::` で結んだ識別子の列）。
  - 値は 10 進の件数か、vis では可視性の字（pub・pub(crate)・pub(super)・pub(in <path>)・private の 5 形）。
  - この形の正本は SRS の FR47 に置き、契約表の検査（FR55）が照らす。設計者は `pipe index show` の値を写す。§ の散文の数は機械が読まない。散文は欄を名指す。
- 測る所:
  - 契約表の検査（CI の contracts check と受付の同じ 1 本）: 要素の形・列の語・項目の path の形・値の形だけを照らし、外れを要素ごとに行番号つきの 1 語 code-facts-form で名指す（growth-form と同じ扱い）。索引は読まない。
  - 受付と `pipe preflight`: base の commit の索引（ready）で要素ごとに §6 の列の数え（行 c の 1 関数）を撃ち、違えば断り code-facts で断る（便を作らない・在り処は本文の読み手）。
  - 起動の列: 候補ごとの `generated` が同じ判定を HEAD の索引で撃つ。違えば受付の理由 code-facts で待つ（FR68 の閉じた理由の「受付」の内・新しい待ちの理由を足さない）。
  - 行の審査: `--ref` の commit の索引を §4 形 9 の組み立ての 1 本で得て（作り中なら終わりを待つ）同じ判定を撃ち、違えば確定の断りとして FAIL（行 i）。
- 断りの 1 行は、要素・名乗りの値・実測の値・増えた site と消えた site（先頭 3 つと残りの件数）・母集団（`text=`）を名指す。
- 欄を持つ行が索引を測れない周（作り中・作れない周・状態なし）: 手の受付と `pipe preflight` は断り code-facts-unmeasured（名乗りの要素と測れない理由の語を名指す・在り処は置き場＝確からしさは測れない）で断る。起動の列は受付の理由 code-facts-unmeasured で待たせる。名乗った事実を測らずに起動しない（C10）。行の審査が出会うのは作れない周だけで、名乗りと測れない理由を記録に残し、確定の finding に数えない（行の判定は lens に委ねる・行 i）。
- vessel 宣言が索引の 2 key を揃えて名乗らない repo は、永久に測れない側である（2 形）。
  - 名乗らない repo（undeclared）で欄を持つ行: 受付と `pipe preflight` は同じ断り code-facts-unmeasured（理由の語 undeclared・在り処は vessel 宣言の file＝依存の行が宣言を変えない限り確定）で断り、起動の列は受付の理由 code-facts-unmeasured で待たせ、行の審査は確定の断りとして FAIL にする（merge の前に止める）。
  - 片方だけ名乗る repo（half）: 索引の宣言の読みが断る（§4 形 1・(b) の half の repo の扱い・行 d）ので、欄を持つ行の受付は断りの名 declaration で断り、起動の列は受付の理由 declaration で待たせ、行の審査は確定の finding として FAIL にする（行 i）。
- 欄の値は契約表の行の型に field を 1 つ足して読む（行 bu が捨てた値を持たせる）。生成する契約 file には写さない（受付と起動の列は行を読む）。

## 8. 読み手の先行と入れ替えの順

- 欄 `code-facts` と宣言の key 2 つは、今の binary には未知で、書いた doc の区間ごと・宣言ごと読めなくなる（§2）。done の項目ごとの歯の欄の設計と同じく、読むだけの行（§12 の行 0・[contract-source.md](./contract-source.md) §67 の行 bu）を先に着地させ、PATH の binary を入れ替えてから書く。
- 同じ時期に done の歯の欄の読むだけの行が未着地なら、2 つの欄を 1 本の読むだけの行で足す（入れ替えを 1 回で済ませる）。
- 行 a〜h の着地のあとも、受付・起動の列・審査の材料の振る舞いが変わる行は、そのつど入れ替える。

## 9. 費用と、足す物・消す物（C17.2）

- 費用（見積り）: 索引 1 本は code を変える着地 1 本ごとに 1 回（設計の PR は code の木の鍵が main と同じなので作り直さない）。1 回 30〜105 秒・peak RSS 約 2.5 GB・受付札 1 枚。平らな表の大きさは試しの base で自前の package の occurrence 111113 行で、数 MB と見込む（未実測）。
- 足す物と消す物:

| 足す | 消す |
|---|---|
| 口 `pipe index`（build・show） | 設計の § と台帳に書いてきた「起票の前に現 main で数え直す」の手の手順（欄 `code-facts` と起動の列の測り直しが置き換える） |
| on-disk の形 1 つ（平らな表と記録）・rules 行 2 本（`index.cap_mb`・`index.timeout_s`） | 外の材料（outside.txt）の `.rs` の item と要約の塊（行 g・表の site と読みの道具が置き換える） |
| vessel 宣言の任意 key 2 つ・行の欄 1 つ | vocabulary の day-1 optional adapter の graphify（試しで不採用・索引が枠を持つ） |
| 材料の file 1 つ・受付の断りの語 3 つ（index-building・code-facts・code-facts-unmeasured）・表の検査の語 1 つ（code-facts-form）・runner の stdin の節 1 つ | — |

- event の種類・待ちの理由の variant・新しい門は足さない（受付の断りの語と表の検査の finding に載せる・起動の列の待ちは既存の受付の理由 admission の値）。

## 10. A3 の論点（外の道具）

- 本 repo が採る道具（推奨）:
  - rust-analyzer: `rust-toolchain.toml` の component に足す。版は toolchain の channel が固定する（今の 1.98.1）。
  - ast-grep: 版を固定して `cargo install --locked` で入れる（試しの版 0.45.3・MIT）。host の `sg` は別物なので、宣言は `ast-grep` の名で撃つ。
- 採らない道具: scip の CLI（器が protobuf を読むので要らない）・tree-sitter の Python の binding（試しの道具）・bubblewrap（封じ込めは器の箱が持つ）・既製の graph 3 つ（§3）。
- 別の言語の道具（scip-typescript・scip-python / ty-scip）は、その言語の消費側の repo が宣言するときに、その repo の A3 で決める。scip-python は保守が 1 年止まっている。
- 採らない場合の代わり（却下案）: 字面の閉包を std の走査のまま広げる（別名の表・`impl` の範囲の追跡・glob の展開・呼び手の関数の範囲・可視性の連鎖・test の文脈）。見積りは 1500〜3000 行の Rust に閉じた構文の近似（未実測）で、method の呼び出しの受け手の型（`x.measure()` の `x` の型）は型推論が要り字面では解けず、下界のまま残る。言語を足すたびに同じ量を書き直す。
- 2026-09-27 の裁定との関係: 退けられたのは Rust 専用の道具。この形では器が読むのは SCIP（言語に依らない形式）と役の語だけで、Rust に依る物は本 repo の宣言と規則の file に閉じる。

- 裁定: rust-analyzer と ast-grep 0.45.3 の採用は user 2026-09-30T22:13Z 項 index-tools（台帳 s2-07l.736.33.4 の裁定・[ADR-0105](../../design-intent/decisions/ADR-0105-code-facts-come-from-an-external-index-the-vessel-reads.html)）。実行時の依存（Cargo の依存）は 0 本のまま（NFR3・deps-empty）で、足すのは外の道具の宣言と toolchain の component だけである。起票の時に行 b の bead の notes へこの裁定と ADR-0105 を名指し、依存（外の道具）を足す行であることを残す（orchestrator が書く・runner は notes を書かない）。
- 本 repo の宣言（契約表の行 b・番号は行 b の done と 1:1）:
  1. `.vessel.toml` に `index-scip` を 1 行（rust-analyzer の scip の subcommand に `{tree}` と出力の `{out}` を渡す）と、`index-roles` を 1 行（ast-grep の scan に役の規則の file と `{tree}` を渡し、stream の JSON を出す）足す。コメントで ADR-0105 と §4 を名指す。
  2. `rust-toolchain.toml` の components に rust-analyzer を足す（版は channel が固定する）。
  3. 役の規則の file（tracked・.config の dir の下の yaml 1 本）: 言語 Rust の rule を §5 の 9 語に 1 つずつ置き、rule の id を役の語と同じ字にし、名を捕える rule は捕えた名を NAME の meta 変数に置く。
  4. host の道具: ast-grep 0.45.3 は `cargo install --locked` で host に入れる（repo の外・CI には入れない・索引の閉包は CI に無い §14）。入れるのは行 b の着地の前に orchestrator が行う。
- 行 b の着地の後の実測: orchestrator が本 repo の HEAD で `pipe index build` を撃ち、built と rows・files・dropped の値を行 b の bead の notes に残す（歯ではない・CI は外の道具を持たない）。

## 11. 試し撃ち（補助・後段）

- compiler に仮の編集を当てて診断を読む形（item に `#[deprecated]` を付けて使う site を全部出させる・struct に欄を足して E0063 で literal の site を出させる・enum に variant を足して E0004 で網羅の match を出させる）。
- 試しでは、呼び手・literal・可視性で満点（16/16・5/5・7/7）だが、索引も同じ問いで満点で、試し撃ちだけが答えた問いは 0 だった。試し撃ちだけが正本になるのは、`..base` の構築（欄を足しても落ちない）と `_` で受ける match で、44 件の失敗には無かった。
- そのため行は起こさず、失敗の分類にこの型が 1 件出た時に行 h として起こす（§12）。編集の字と診断の読みは言語ごとに違い（tsc・pyright も同じ形の診断を持つ）、宣言の側に置く。

## 12. 行（粒度・順序・write-set・歯）

SRS 0.33 の round（FR107〜FR109 を足し、FR47・FR48・FR55 の字を直した）と A3 の裁定（user 2026-09-30T22:13Z 項 index-tools）の後に、行 a1・a2・b・c・d・e・g を本 doc の契約表に足した。write-set は起票の前に `pipe preflight` と行の審査で測り直す。行 0 と行 f は round を待たずに起こした（行 0 は contract-source.md §67 の行 bu・行 f は §15）。

| 行 | 中身 | 順（契約表の depends と台帳の blocks） | write-set |
|---|---|---|---|
| 0 | 欄 `code-facts` と宣言の key `index-scip`・`index-roles` を読むだけ（読んで捨てる・効かせない） | round を待たない。[contract-source.md](./contract-source.md) §67 の行 bu に起こした | 行 bu の write-set |
| a1 | 読み手と表（§4 形 3・4・5・形 6 の鍵の digest・形 9 の表の問いと役の 9 語の列・pub の関数だけ・口と置き場を持たない） | 待つ行なし（行 bu の欄と key を読まない） | pipe の新しい子 module index と子 2 つ（`+`）・pipe の mod 宣言・e2e の pipe の既存の歯の file |
| a2 | 組み立ての口（§4 形 1・2・形 6 の置き場と記録と印・形 7・8・形 9 の状態の読みと組み立てと索引の宣言の読み・`pipe index build`・rules 行 2 本） | depends a1。台帳の blocks で行 bu の着地と PATH の binary の入れ替えの後、行の審査の行 a1（code の木の鍵の口 (C)）の後。touches は rules の kind と pipe の subcommand の閉じた型 | 起動の列の新しい子 module（`+`・床の検査の兄弟）・`crates/scribe2/src/pipe/dispatch.rs`（mod の 1 行）・床の検査の file（3 つを開く）・`crates/scribe2/src/fleet/wait.rs`・`crates/scribe2/src/fleet/mod.rs`・宣言の 2 file・`crates/scribe2/src/pipe/cli.rs`・`crates/scribe2/src/pipe/cli/args.rs`・`crates/scribe2/src/help.rs`・`crates/scribe2/src/rules/mod.rs`・`rules/manifest.toml`・e2e の pipe と rules の既存の歯の file・rules と pipe の外形 snapshot |
| b | 本 repo の宣言（§10・`.vessel.toml` の 2 key・`rust-toolchain.toml` の component・役の規則の file） | depends a2。着地の前に host へ ast-grep 0.45.3 を入れる（orchestrator） | `.vessel.toml`・`rust-toolchain.toml`・役の規則の file（`+`）・e2e の pipe の既存の歯の file |
| c | 逆引きの表（§6・`pipe index show`・列の数えの 1 関数）と材料 index.txt（§7 (a)） | depends a1 と a2 | 審査の新しい子 module（`+`）・`crates/scribe2/src/pipe/closure/names.rs`（§ の名指しの口）・`crates/scribe2/src/pipe/closure.rs`（pub use の 1 行）・`crates/scribe2/src/pipe/cli.rs`・`crates/scribe2/src/pipe/cli/args.rs`・`crates/scribe2/src/pipe/review.rs`・`crates/scribe2/src/headless/lens.rs`・lens の雛形・e2e の審査の既存の歯の file |
| d | 受付の索引の閉包と状態の扱い（§7 (b)・断り index-building・` index=unavailable:<語>`・裏の組み立て・half の repo の断り） | depends a1 と a2。touches は受付の断りの閉じた型 | `crates/scribe2/src/pipe/closure.rs`・受付の新しい子 module（`+`）・`crates/scribe2/src/pipe/cli/intake.rs`・`crates/scribe2/src/pipe/refuse.rs`・`crates/scribe2/src/pipe/cli/preflight.rs`・`crates/scribe2/src/pipe/dispatch/candidates.rs`・`crates/scribe2/src/pipe/dispatch.rs`・e2e の受付と起動の列の既存の歯の file・断りの閉包の置き場（`=`） |
| e | 欄 `code-facts` の照らしと測り（§7 (c)・code-facts-form・code-facts・code-facts-unmeasured） | depends c と d。touches は受付の断り・表の検査の断り・契約表の行の 3 つの閉じた型 | 表の 3 file・`crates/scribe2/src/pipe/refuse.rs`・受付の新しい子 module（`+`）・`crates/scribe2/src/pipe/cli/intake.rs`・`crates/scribe2/src/pipe/contract.rs`・e2e の契約表と受付と起動の列の既存の歯の file・閉包の置き場（`=`） |
| f | runner の stdin の「ほかの行の touches の型」の節（§15） | 着地の列に在る（s2-07l.736.33.6） | §15 と契約表の行 f |
| g | 外の材料の `.rs` の item と要約の塊を外す（§16） | depends c。台帳の blocks で pipeline.md の行 bg（読みの道具）の着地の後 | `crates/scribe2/src/pipe/review/outside.rs`（`-`）・e2e の審査の既存の歯の file |
| h（条件つき） | 試し撃ち（§11） | 分類にその型が出た時だけ | 別の設計で決める |
| i（後で足す） | 行の審査の口（[row-review.md](./row-review.md) の行 a）に索引をつなぐ: 機械の検査の材料に `--ref` の commit の状態を載せ（行 d・e の判定がそのまま効く）、材料の dir に index.txt を置く（行 c の 1 本） | 行の審査の行 a と本 doc の行 c・d・e の着地の後に、その時の口の file を write-set にして足す（今は口の file が base に無く名指せない） | 行の審査の口の file と e2e の行の審査の歯の file |

- 行を分けた理由: a1 は pure な読み手と表だけ、a2 は組み立ての口と on-disk の置き場で、どちらも受付・列・材料の振る舞いを変えない（口が 1 つ増えるだけ）。a は src で約 840 行の見込みで NFR2 の見積 550 行を越えるので、I/O の有無で a1（約 450 行）と a2（約 420 行）に割った。a1 の歯は新しい file の中でなく既存の e2e の file（pipe.rs）から新しい pub の関数を直に撃ち、base では compile で落ちる。c は審査の材料、d は受付の閉包、e は欄の測りで、それぞれ別の歯の母集団を持つ。d と e は同じ受付の file を触るので depends で並べる（e は c の列の数えも呼ぶ）。
- 上限の余地（pipe preflight の headroom・2026-10-01 の base）: `crates/scribe2/src/pipe/cli/intake.rs` 149 行・`crates/scribe2/src/pipe/dispatch.rs` 23 行・`crates/scribe2/src/pipe/table/check.rs` 85 行。行 d・e は判定を受付の新しい子 module に置き、intake.rs と dispatch.rs には配線だけを足す（dispatch.rs は行 a2 の mod の 1 行と行 d の 10 行）。同じ時期に同じ file を育てる別の束の行が先に着けば、後の行の growth を起票の前に測り直す。
- 呼ぶ口の置き場: a1 の口（読み手・表・digest・表の問い）と a2 の口（状態の読み・組み立て・索引の宣言の読み）は §4 形 9、c の列の数えと描きは §6 の行 c の形 2 に書いた（後の行の審査が名と可視性を照らす）。
- 他の束との順: 行の審査の行 a と本 doc の行 a2 はどちらも pipe の subcommand の閉じた型に 1 語足し、`crates/scribe2-boundary/tests/e2e/pipe.rs` の数の pin（便の base の本数）を直す。後に着く便の base には先の語が在るので、pin は便の base の本数 +1 で書く。rules の kind の末尾の pin も、同じ時期に kind を足す別の行と同じ扱いにする。
- 歯（done の項目ごと・どれも base で RED の理由を書く・接頭辞は `crates/` の fn 名の substring に 0 件）:
  - 行 0: contract-source.md §67 の歯（接頭辞 contract_fields_read_only_）。
  - 行 a1: e2e（`crates/scribe2-boundary/tests/e2e/pipe.rs`・接頭辞 pipe_index_table_・SCIP の fixture は同じ file の中の小さな protobuf の書き手で作る〔binary の fixture と helper だけの file を置かない〕）。base は index の module が無く compile で落ちるので RED。
  - 行 a2: e2e（`crates/scribe2-boundary/tests/e2e/pipe.rs`・接頭辞 pipe_index_build_）と、lib（`crates/scribe2/src/pipe/declaration/optional_keys.rs` の既存の test 区間・接頭辞 declaration_index_）と、e2e（`crates/scribe2-boundary/tests/e2e/rules.rs`・接頭辞 rules_index_）。base は subcommand・索引の宣言の読み・kind が無いので RED。
  - 行 b: e2e（`crates/scribe2-boundary/tests/e2e/pipe.rs`・接頭辞 pipe_index_declared_）が本 repo の宣言を器の宣言の読みで読む。base は key が無いので RED。
  - 行 c: e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・接頭辞 pipe_index_show_）と lib（names.rs の既存の test 区間・接頭辞 closure_names_section_symbols_）。base は verb と材料と口が無いので RED。
  - 行 d: e2e（`crates/scribe2-boundary/tests/e2e/pipe/intake.rs` と `crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs`・接頭辞 pipe_intake_index_closure_）。base は断りが無いので RED。
  - 行 e: e2e（`crates/scribe2-boundary/tests/e2e/pipe/contracts.rs`・接頭辞 contract_code_facts_ と、intake.rs と dispatch.rs・接頭辞 pipe_dispatch_code_facts_）。base は語と断りが無いので RED。
  - 行 f: §15 の歯（接頭辞 runner_touches_section_）。
  - 行 g: e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`・接頭辞 pipe_review_outside_trimmed_）と lib（outside.rs の既存の test 区間の書き換え）。base は塊が在るので RED。
- 項目ごとの歯の対応は、各行の done の末尾に書いた（契約表）。

## 13. 限界

- 索引の外: `.rs` 以外の file（toml・nextest の設定・shell）と、文字列を経由する流れ（CLI の verb・rules の行 id・file の path）は辺にならない。表は母集団の `outside-index` で見せるだけで、閉包と事実の欄には入れない。影響範囲（型 g）は候補の上界しか出せない。
- 規則のいくつかは試しの bench の落ちを見てから足した（§3）。行 a1〜c の着地の後に新しい問いの bench で測り直し、ADR-0105 の見直しの材料にする。
- 増分が無い: code を変える着地 1 本ごとに全部作り直す。1 つの host の state dir ごとに別に作る。
- 索引の閉包は CI に無い。設計の PR の CI は通り、行の審査と受付で初めて断られる形が残る（行の審査が merge の前に止める）。
- 事実の欄は設計者が写した値で、写さなかった事実は測らない。設計の時点で知らなかった事実は表（(a)）が見せ、欄は知った事実が古びないことを守る。
- rust-analyzer は build script と proc-macro を実行する。gate の検証と同じ信頼で、封じ込めの箱の中で撃つ。
- TypeScript の object literal の key と interface の欄の結び、Python の indexer の実用度は未実測。
- 行の審査への接続（索引の閉包の確定の FAIL・code の事実の FAIL と名指し・材料の index.txt）は行 i まで無い。AC81・AC82・AC83 のうち行の審査の部分は行 i が満たし、行 c・d・e は契約の審査・受付・`pipe preflight`・起動の列の部分を満たす。
- 欄 `code-facts` は生成する契約 file に写さないので、着地の列の settled の鍵（契約 file の sha）は欄だけの変化を見ない。live な行の書き換えを止める guard は契約表の行の型の等しさで比べるので、行 e の後は欄の変化も見る。
- ast-grep の規則で 9 つの役（特に doc の link と文字列の取り込み）を書き切れるかは未実測（試しは tree-sitter の Python の binding で役を付けた）。書き切れない役は行 b の規則の file から外し、§3 の数字をその役の分だけ割り引いて読む。
- 列の揃え: SCIP の列の単位（position_encoding）と役の一致の byte の位置を木の本文で揃える。非 ASCII の字を持つ行（日本語の doc と文字列）の結びは行 a1 の fixture で測る。

## 14. 却下

- 既製の code の graph（code-review-graph・graphify・codebase-memory-mcp）: 53 問のうち行で 10〜16（関数単位で 14〜27）で、literal と可視性が引けず（行で型 b 0/5・型 f 0〜1/7）、名の一致で結ぶ作りが器の失敗の型そのもの。graphify は node を潰し、設定では避けられない。codebase-memory-mcp は呼び手の関数の下見（関数単位で型 d 14/16）には使えるが、行の網羅には使えず、呼び手は SCIP + 構文の分類が答える。
- 構文木の crate を器に足す（syn 等）: 実行時の依存（NFR3）で、Rust に閉じる（[contract-source.md](./contract-source.md) §11 の却下と同じ）。
- scip の CLI で JSON に直してから読む: 道具が 1 つ増え、1 本 71 MB の JSON を読む。器が protobuf の必要な欄だけを読む方が小さい。
- 素の grep の件数だけを表にする: 事実の 88% を持つが precision 0.023 で、同名の別物と use の行と歯の中の呼び出しが混ざり、設計者が読めない。
- 索引の閉包を CI に入れる: PUBLIC の CI に rust-analyzer と ast-grep を入れ、PR ごとに 30〜105 秒と 2.5 GB を足す。行の審査が merge の前に同じ判定を撃つので、CI は字面のまま置く。
- 索引が無い周に全部の候補を待たせる: 道具が壊れた周に列の全部が止まる。事実の欄を名乗った行だけを待たせ、ほかは字面の閉包に縮退する。
- 事実を欄でなく § の散文の数から読む: 散文を判定の入力にしない（憲法 C3.3）。
- 起動の列の測り直しに新しい待ちの理由を足す: 受付の理由の語で足り、閉じた型と FR68 の字を動かさない。
- 試し撃ちを今起こす: 試しで試し撃ちだけが答えた問いは 0 で、言語ごとの編集の字と診断の読みが要る（§11）。

## 15. runner の stdin にほかの行の touches の項目を並べる（§7 (b) の後半を索引なしで先に・契約表の行 f・FR4 / FR48）

やさしく言うと: 便の runner は、自分の行の write-set の中で新しい file や行を書く。そこで、ほかの行が touches に挙げた型を新しく名指す（組み立てる・match する・数を pin する）と、その行の閉包がその行の write-set の外へ広がり、gate の共通の検証の閉包の歯が落ちる。落ちる前に runner に知らせるため、stdin に「ほかの行の touches」節を足し、ほかの契約表の行の touches の項目と行の名を並べる。契約表を読むだけなので、索引は要らない。

- 出所: 行 al の 2 本目の便が gate で落ちた型（[contract-source.md](./contract-source.md) §66 の出所）と、§7 (b) の後半。
- 何が起きているか（main 36c34993・verified）:
  - runner の stdin は `crates/scribe2/src/pipe/spawn.rs` の prompt が組む。順は、契約 file の写し → 「## 共通 verify」節（common_section・[pipeline.md](./pipeline.md) の行 bh・着地済み）→ 回答 → 途中再開 → 追随。節の読み方は雛形 `crates/scribe2/src/headless/runner.txt` が持ち、外形の snapshot の歯 headless_runner_prompt_external_form が雛形の字を pin する。
  - 便の base は初回の spawn で repo の HEAD を記録し、再開の turn も同じ base を使う。gate は設計 doc を便の base から git show の形で読む（`crates/scribe2/src/pipe/gate/verify.rs` の約束の行の読み）。
  - 契約表の doc の母集団は `crates/scribe2/src/pipe/table/check.rs` の私有の fn design_docs（docs/design の直下の .md・tracked の順）で、contracts check と宣言済みの新規 file の母集団 declared_files が使う。`crates/scribe2/src/pipe/table.rs` の pub(crate) の再輸出の列は declared_files・read・tracked_files などを crate の中へ開くが、design_docs は開いていない。read_table と parse_pointer は pub の再輸出に在る。
  - 契約 file の design は設計 pointer（<doc>#<行 id>）である。
  - 母集団（main 36c34993 で数え直した）: 契約表の行 434、touches を持つ行 76、touches の項目の異なり 50（うち fn 形〔末尾が小文字始まり・contract-source.md §18〕6）。
- 形（番号は行 f の done と 1:1）:
  1. **節の置き場と中身**: prompt は「## 共通 verify」節の直後・回答の節の前に「## ほかの行の touches」節を足す。中身は便の base の木の契約表から組む。base の tracked な path の列（git ls-tree）を design_docs の母集団で絞り、doc ごとに base の本文（git show）を read_table で読み、行ごとに pointer（<doc>#<id>）と touches の列を集める。契約 file の design と同じ pointer の行（自分の行）は除く。並べ方は touches の項目ごとに 1 行「- <項目> ← <pointer>, <pointer>」で、項目は辞書順・pointer は doc の順と行の順・同じ pointer は 1 回。anchor の作業木は読まない（未 commit の行は載らない）。
  2. **読めない周**: base の木の列・doc の本文・doc の区間のどれかを読めない周は、節の本文を理由の 1 行「（ほかの行の touches を読めない: <理由>）」にし、runner を止めない（段は今までどおり進む）。理由は doc の path と最初の不備を持つ（declared_files の断りと同じ形）。
  3. **組み立ての関数**: 本文の組み立ては pure な pub の 1 関数（行の pointer と touches の列・自分の pointer を受けて本文を返す）で、e2e が直に撃てる（common_lines と同じ置き方）。項目が 0 のときは「なし」の 1 行。
  4. **母集団の読み**: design_docs を pub(crate) にし、table.rs の pub(crate) の再輸出の列に足す（2 つ目の doc の母集団の読みを作らない・C6）。
  5. **雛形**: runner.txt の守ることに 1 項目足す。「ほかの行の touches」節の項目は、ほかの契約表の行が閉包で守る名である。write-set の file のうち今それを名指していない file に新しく名指す（struct の literal・match の arm・件数の pin・const slice の宣言・variant の構築・同じ module での同名の fn の宣言）と、その行の閉包がその行の write-set の外へ広がり、gate の共通の検証の閉包の歯が落ちる。名指さずに作れないときは、契約の不足として質問で止まる。
  6. **変えないもの**: 共通 verify・回答・途中再開・追随の節の字と順、段と rc、契約 file の字。
  7. **閉包**: spawn.rs と e2e の歯の file は、read_table の返りを field で読むだけにし、ContractRow（契約表の行 h の touches・[contract-source.md](./contract-source.md) §66 の出所の型）を名で書かない。組み立ての関数の入力は pointer と touches の列の組にする。便の木で契約表の検査を撃つ検証行を最後に置いて測る（contract-source.md §66 形 0 の closure）。
- 歯（接頭辞 runner_touches_section_・e2e の `crates/scribe2-boundary/tests/e2e/pipe/spawn.rs`・どれも base で RED）:
  - (a) 自分の行（touches に型 1 つ）と、別の doc の 2 行（1 行は型と fn の 2 項目・もう 1 行は同じ型）と、docs/design の下の子の dir の doc の行（touches に型 1 つ）を commit し、anchor の作業木だけに別の doc の行を 1 本足した repo の便の初回の stdin は、「## 共通 verify」節の後に「## ほかの行の touches」節を持つ。本文は 2 項目の 2 行（型の行は 2 つの pointer・fn の行は 1 つ）だけで、自分の行の型・子の dir の doc の型・未 commit の行の型を持たない。base は節が無いので RED。
  - (b) 受付の後・spawn の前に別の doc の区間を壊して commit した便の節の本文は、doc の path を持つ理由の 1 行で、段は Implemented まで進む。base は節が無いので RED。
  - (c) 組み立ての関数を直に撃つ。項目 0 は「なし」の 1 行、自分の pointer の行を除く、同じ項目の pointer を 1 行に束ねる。base は関数が無く compile で落ちる。
  - (d) 外形の snapshot headless_runner_prompt_external_form が雛形の新しい項目を持つ（snapshot を作り直す）。base は雛形の字が違うので RED。
  - 変わらない既存の歯: runner_common_section_ の 3 本（節の本文は次の「## 」の手前で切るので、後ろに節が増えても本文は同じ）。
- 限界:
  - 読むのは行の欄 touches だけ。約束の行の symbols から導く touches（Promised の行）は載らない。
  - docs/design の直下の .md だけを読み、導出物の .toml は読まない（declared_files と同じ母集団）。
  - 名指してよいかは判じない。器は知らせるだけで、測るのは gate の共通の検証の閉包の歯のまま。
  - 項目の形（型か fn か）を読み分けず、節の長さに上限を置かない（今の母集団で 50 項目ほど）。

## 16. 外の材料の `.rs` の item と要約の塊を外す（契約表の行 g・§9 の消す物・FR49）

やさしく言うと: 審査役は今、shell も cargo も撃てないので、器が write-set の外の型の本文と file の要約を outside.txt に束ねて渡している。審査役が読みの道具（Read・Grep・Glob）で木を開けるようになり、index.txt が名の使われ方を件数と場所で渡すようになった後は、この 2 種類の塊は重複して材料の予算を食う。そこでこの 2 種類だけを外し、道具でも索引でも取りにくい塊（依存の表・親 module の宣言・data file の鍵・depends の相手の行）は残す。

- 何が起きているか（main 92a5ddc8・verified）: `crates/scribe2/src/pipe/review/outside.rs` の塊の並びは (f) depends の相手の行 → (d) crate の依存の表 → (e) 親 module の宣言 → (a) `.rs` の item → (b) 名指された `.rs` の要約 → (c) data file の鍵の行で、その後ろに子 module の bodies（write-set の中の item の本文）と linked（別の設計の § の本文）が続く。見出しの下の説明の 1 文（PREAMBLE）が並びを名指す。(a) を pin する歯は lib の pipe_review_outside_names_an_outside_struct_with_its_fields と e2e の pipe_review_outside_material_carries_the_named_outside_struct。
- 形（番号は行 g の done と 1:1）:
  1. **外す塊**: (a) `.rs` の item と (b) 名指された `.rs` の要約を束ねない。(a)(b) だけが使う私有の関数は消す（使い手の残る関数は残す）。
  2. **残す塊**: (f)(d)(e)(c) と、bodies と linked の塊の字と順は変えない。
  3. **説明の 1 文**: PREAMBLE から (a)(b) の句を外し、`.rs` の item の本文は読みの道具で開け、名の使われ方は index.txt が渡す、の 1 句に替える。
  4. **名指しだけの契約**: 本文が write-set の外の `.rs` の item だけを名指す契約は、outside.txt を置かない（本文が空なら置かない今の扱い）。
  5. **変えないもの**: 材料の file の名と置き方、`{outside}` の穴と cap の収め方、名指しの読み手、base の要約（base.txt）。
- 歯（接頭辞 pipe_review_outside_trimmed_・どれも base で RED）:
  - e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs`）: § が backtick の外で write-set の外の struct を名指す契約の審査の材料の dir に outside.txt が無く（名指しがその struct だけ）、同じ § に data file の名指しを足した契約では outside.txt が data file の鍵の塊を持ち struct の塊を持たない。base は struct の塊を置くので RED。既存の歯 pipe_review_outside_material_carries_the_named_outside_struct はこの歯へ置き換える（名を替え、塊が無いことを pin する）。
  - lib（outside.rs の既存の test 区間）: 既存の歯 pipe_review_outside_names_an_outside_struct_with_its_fields を、同じ fixture で struct の塊と要約の塊が無く、depends・依存の表・親 module・data file の塊の見出しが同じ順で在ることを pin する歯へ置き換える（名を pipe_review_outside_trimmed_ の接頭辞へ替える）。base は塊が在るので RED。
  - 変わらない既存の歯: pipe_review_outside_ の残りの lib の歯（data file・依存の表・親 module・depends・切り詰め・名指しなし）と、bodies・linked の歯。
- 限界: lens が読みの道具で木を開けない周（道具を渡さない古い lens の cmd）は、外の型の本文を材料から失う。行 g は pipeline.md の行 bg の着地の後に起こす。

<!-- contracts:begin -->
schema = 1

[[contract]]
id = "f"
title = "runner の stdin の共通 verify の節の後に「ほかの行の touches」節を足し、便の base の契約表から自分の行を除いた行の touches の項目を pointer と並べ、雛形はそれを新しく名指すとその行の閉包が広がると読む（§15）"
req = ["FR4", "FR48"]
section = "15"
write-set = ["crates/scribe2/src/pipe/spawn.rs", "crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/headless/runner.txt", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__headless__headless_runner_prompt_external_form.snap", "=crates/scribe2-boundary/tests/e2e/headless.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_touches_section_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail headless_runner_prompt_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail runner_common_section_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/spawn.rs:60", "crates/scribe2/src/pipe/table.rs:1", "crates/scribe2/src/pipe/table/check.rs:1", "crates/scribe2-boundary/tests/e2e/pipe/spawn.rs:170"]
done = "(1) runner の stdin は「## 共通 verify」節の後・回答の節の前に「## ほかの行の touches」節を持ち、本文は便の base の木の契約表（docs/design の直下の .md の区間）の行のうち契約 file の design と同じ pointer の行を除いた行の touches の項目を、項目ごとに 1 行「- <項目> ← <pointer>, …」で辞書順に並べ、anchor の作業木だけに在る行は載せない〔runner_touches_section_ の (a)〕 (2) base の木の列・doc の本文・doc の区間のどれかを読めない周の本文は（ほかの行の touches を読めない: <理由>）の 1 行で doc の path を持ち、runner は止まらず段は Implemented まで進む〔(b)〕 (3) 組み立ては pure な pub の 1 関数で、項目 0 は なし の 1 行・自分の pointer の行を除く・同じ項目の pointer を 1 行に束ねる〔(c)〕 (4) doc の母集団は check.rs の design_docs を pub(crate) にして table.rs の再輸出の列から読み、docs/design の下の子の dir の doc の行を載せない〔(a)〕 (5) runner.txt の守ることに、節の項目を今それを名指していない file で新しく名指すとその行の閉包がその行の write-set の外へ広がり gate の共通の検証の閉包の歯が落ちる・名指さずに作れないときは契約の不足として質問で止まる、の項目が在る〔外形の snapshot headless_runner_prompt_external_form〕 (6) 共通 verify の節の本文と順は変わらない〔変わらない既存の歯 runner_common_section_〕 (7) spawn.rs と e2e の歯の file は ContractRow を名で書かず、契約表の行 h の閉包を広げない〔verify の 4 行目の契約表の検査〕 歯: e2e の runner_touches_section_（spawn.rs・(a) 自分の行と別の doc の 2 行と子の dir の doc の行を commit し作業木だけに 1 行を足した repo の初回の stdin (b) 受付の後に別の doc の区間を壊して commit した便 (c) 組み立ての関数を直に撃つ）・headless_runner_prompt_external_form（snapshot を作り直す）・変わらない既存の歯 runner_common_section_、base は節と関数が無いので (a)(b) が RED・(c) は compile で落ち・snapshot は雛形の字が違うので RED"
[[contract]]
id = "a1"
title = "code の索引の読み手と表 — SCIP の wire と役の一致の JSON を std で読み、byte の位置で最も内側の役を結び、平らな表を描いて読み、鍵の digest と表の問いと役の 9 語の列を I/O の無い pub の関数で置く（§4 形 3・4・5・形 6 の digest・形 9 の a1 の口）"
req = ["FR107", "NFR3"]
section = "4"
write-set = ["+crates/scribe2/src/pipe/index.rs", "+crates/scribe2/src/pipe/index/scip.rs", "+crates/scribe2/src/pipe/index/flat.rs", "crates/scribe2/src/pipe/mod.rs", "crates/scribe2-boundary/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_table_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_index_read_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/index.rs:90", "crates/scribe2/src/pipe/index/scip.rs:170", "crates/scribe2/src/pipe/index/flat.rs:190", "crates/scribe2/src/pipe/mod.rs:1"]
done = "(1) SCIP の読みは document の相対 path と position_encoding・occurrence の範囲と symbol と定義の印と囲む範囲・symbol の情報の名と種類だけを読み、知らない欄を wire の型で飛ばし、外の crate の symbol と関数の中の local を落とし、途中で切れた varint と長さが本体を越える欄を読めない理由にする〔pipe_index_table_ の (a)〕 (2) 役の一致の読みは json_tree の parse で 1 行ずつ ruleId・file・byteOffset の start と end・NAME の text だけを読み、9 語の外の ruleId の行を捨てて捨てた数を返し、JSON でない行を読めない理由にする〔(b)〕 (3) 結びは両方を呼び手が渡した file の本文で file の先頭からの byte に直して最も内側の役を付け、occurrence の無い doclink・capture・Self の literal は囲む定義 → 同じ file の順に同じ字の occurrence から symbol を借り、借りられない物は字だけの行に印を付け、test の役の中で宣言された module の file の行は全部 test にする〔(c)〕 (4) 表の描きは頭の行 schema=1 と tab 区切りの 9 列で、読みは描きの逆で往復が等しく、schema の違う表は無いと読み、形の崩れた行は読めない理由にする〔(d)〕 (5) 鍵の digest は code の木の鍵と宣言 2 key の字を hook の vessel の digest の FNV-1a 64 に渡した 16 桁で、同じ入力は同じ字、code の木の鍵か宣言の字の 1 字の違いは別の字になる〔(e)〕 (6) 表の問いは項目の path を descriptor の名の列の末尾一致で 0・1・複数の symbol に解き、symbol ごとに役つきの site の列を返し、別 module の同名の型の site を混ぜず、役の語の列は §5 の 9 語の宣言の順〔(f)〕 (7) 関数は pub で I/O を持たず（file の本文は呼び手が渡す）、口・置き場・印・rules 行は足さない（行 a2）〔(a)〜(f) が e2e から直に撃つ〕 (8) index の module はほかの行が touches に持つ閉じた型を組まず match もしない〔verify の最終行の contracts check〕 歯: e2e の pipe_index_table_（既存の file の pipe.rs・SCIP の fixture は同じ file の中の小さな protobuf の書き手で作り、その書き手は同じ file の歯が撃つ〔helper だけの file を作らない〕・(a) 知らない欄と外の crate の symbol と local を持つ document の読みと、切れた varint と長さの越えの 2 形の読めない理由 (b) 9 語の行と 9 語の外の行 1 つと JSON でない行 (c) 別名の取り込みの越しの literal・Self の literal・doc の link・同じ行の日本語の字の後ろの文字列の取り込み・test の module の file の行の役と借りた symbol と字だけの行の印 (d) 描きの頭と列と往復・schema=2 の表・列の足りない行 (e) digest の同じ入力と 1 字違いの入力 (f) 末尾一致の 0・1・2 の 3 形と別 module の同名の型）、lib の pipe_index_read_（新しい file の test 区間・flip の外の補い・wire の型ごとの読み飛ばし）、base は index の module が無く (a)〜(f) が compile で落ちるので RED"

[[contract]]
id = "a2"
title = "code の索引の組み立ての口 — pipe index build が索引の宣言の 2 key の command を commit を detach した木で封じ込めと受付札の中で撃ち、行 a1 の読み手で平らな表にして state dir の鍵ごとに置き、撃ち中の印の持ち主の終わりを待ち、rules 行 index.cap_mb と index.timeout_s で量と時間を絞り、片方だけの宣言は索引を要する周の索引の宣言の読みでだけ断る（§4 形 1・2・形 6 の置き場・形 7〜9 の a2 の口）"
req = ["FR107", "NFR6"]
section = "4"
depends = ["a1"]
touches = ["crate::pipe::cli::PipeCommand", "crate::rules::RuleKind"]
write-set = ["+crates/scribe2/src/pipe/dispatch/index_build.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2/src/pipe/dispatch/floor.rs", "crates/scribe2/src/fleet/wait.rs", "crates/scribe2/src/fleet/mod.rs", "crates/scribe2/src/pipe/declaration.rs", "crates/scribe2/src/pipe/declaration/optional_keys.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/help.rs", "crates/scribe2/src/rules/mod.rs", "rules/manifest.toml", "crates/scribe2-boundary/tests/e2e/pipe.rs", "crates/scribe2-boundary/tests/e2e/rules.rs", "crates/scribe2-boundary/tests/e2e/rules/embedded.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__rules__rules_external_form.snap", "=crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_build_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_index_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_index_status_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_index_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_drafts_cap_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_floor_timeout_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_is_valid_and_covers_all_kinds", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_embedded_manifest_declares_one_capability_row_per_role", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none", "cargo nextest run -p scribe2 --lib --no-tests=fail help_table_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_floor_", "cargo nextest run -p scribe2 --lib --no-tests=fail declaration_floor_check_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_table_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail rules_memo_rows_are_the_last_three_kinds_and_rows", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/dispatch/index_build.rs:300", "crates/scribe2/src/pipe/dispatch.rs:1", "crates/scribe2/src/pipe/dispatch/floor.rs:4", "crates/scribe2/src/fleet/wait.rs:8", "crates/scribe2/src/fleet/mod.rs:1", "crates/scribe2/src/pipe/declaration.rs:8", "crates/scribe2/src/pipe/declaration/optional_keys.rs:50", "crates/scribe2/src/pipe/cli.rs:30", "crates/scribe2/src/pipe/cli/args.rs:6", "crates/scribe2/src/help.rs:1", "crates/scribe2/src/rules/mod.rs:10"]
done = "(1) 索引の宣言の読み（vessel 宣言の parse とは別の pub の 1 関数・floor_check_at と同じ形）は 2 key の行の列を返し、片方だけの宣言と穴の欠けた行（index-scip に {tree} か {out} が無い・index-roles に {tree} が無い）を key の名と行番号の不備にし、2 key が無ければ無しを返す〔declaration_index_ の (j)〕 (2) 片方だけの宣言を断るのは索引を要する周の索引の宣言の読みだけで、vessel 宣言の parse（Declared は 2 key の値を持つだけ）・ほかの key の読み・索引を要しない契約の受付・contracts check・FR101 の merge の門の宣言の読みは片方だけの宣言でも今のまま読み、pipe index build は half の repo で rc 2 で key の名と行番号を名指す〔pipe_index_build_ の (h)〕 (3) build は ref（既定 HEAD）の commit を床の検査の Worktree で置き場の下の <commit の sha>.tree に detach し、cwd をその木にして宣言の行を順に、行を空白で割り穴を語ごとに埋め床の検査の resolve で解いた program で、封じ込めの箱の中で受付札 1 枚（jobs 1）を取って撃ち、行 a1 の読み手と結びで平らな表にし、撃ち終えた後に木と外の道具の出力を片付ける〔(a)(e)〕 (4) 鍵は行の審査の行 a の code の木の鍵と宣言の字を行 a1 の digest に渡した 16 桁で、置き場は state dir の pipe/index の下の鍵ごとの表と記録（schema=1・key・commit・宣言の digest・rows・files・dropped・secs・at・stderr の末尾）、同じ鍵の 2 回目は撃たずに cached、code の file を変えた commit は別の鍵で built、契約表の doc だけを変えた commit は cached〔(a)(b)〕 (5) 撃ち中の印は床の検査の印の <鍵>.lock で、生きた持ち主の周は撃たずに fleet の wait.rs に足す pid の消えを待つ 1 関数（RunnerExited を包む・deadline は index.timeout_s）で終わりを待ってから状態を読み直し（ready は cached・失敗の記録はその語・deadline 越えは記録を書かずに failed:timeout）、死んだ持ち主の印は外して built〔(f)〕 (6) 置き場の合計が index.cap_mb を越える周は撃ち中の鍵と anchor の HEAD の鍵を除いて記録の at の古い順に上限まで消し、2 本の rules 行のどちらかを読めない周は撃たず消さずに記録へ failed:no-rule を書く〔(c)(g)〕 (7) 壊れた SCIP・rc 1 の command・index.timeout_s を越えた command（process group ごと止める）・program の無い行のそれぞれで、1 行と記録が failed:unreadable・failed:rc・failed:timeout・failed:path で rc 1 になり表を置かない〔(d)〕 (8) 状態の読みは宣言なし・片方だけか穴の欠け・表も印も記録も無い・生きた印・失敗の記録・読める表を undeclared・half・absent・building・failed・ready の 6 値に分け、撃たず待たない〔pipe_index_status_ の (k)・e2e の (a)(f)(h) の口の振る舞い〕 (9) 口の 1 行は [INDEX] key=<16 桁> rows=<n> files=<m> <built|cached|failed:<語>> secs=<s> で、rc は built・cached が 0・failed が 1・half・宣言を読めない・repo でない・ref を解けない周が 2、2 key の無い repo は [INDEX] undeclared で rc 0 で撃たない〔(a)(d)(f)(h)〕 (10) pipe の subcommand の閉じた列の末尾に index が 1 語足され（便の base の本数 +1）、usage と help の表に載る〔pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none・外形の snapshot pipe_external_form・help_table_〕 (11) 埋め込みの manifest の末尾に行 index.cap_mb（kind IndexCapMb・Int・2048・enabled・裁定 id user 2026-09-30T22:13Z 項 index-cap・裁定日 2026-09-30）と行 index.timeout_s（kind IndexTimeoutS・Int・600・user 2026-09-30T22:13Z 項 index-timeout・2026-09-30）がこの順で在り、kind は ALL の末尾に同じ順で字面から引け、組み立ての module が const の id を int_row で読む〔rules_index_ の (i)・rules_external_form の snapshot〕 (12) 床の検査の印・run・resolve は兄弟の module から呼べる可視性に開くだけで、床の検査の撃ち方・語・置き場・歯の字は変わらず、行 a1 の関数と歯も変わらない〔変わらない既存の歯 pipe_dispatch_floor_・declaration_floor_check_・pipe_index_table_〕 (13) 組み立ての module は ContractRow・TableError・Refuse・RuleKind・Completion・Confinement を組まず match もせず（待ちは wait.rs の 1 関数・箱は confined の値・rules 行は id の字で読む）、ほかの行の閉包を広げない〔verify の最終行の contracts check〕 歯: e2e の pipe_index_build_（pipe.rs・偽の宣言は行 a1 の歯の SCIP の書き手で作った fixture と一致の JSON を {out} と stdout へ写し呼ばれた回数と cwd を file に残す sh の script）の (a) 1 回目 built で rows・files・dropped=1 と表の頭、2 回目 cached で回数 1 のまま (b) code の file を変えた commit は built で回数 2、契約表の doc だけを変えた commit は cached で回数 2 (c) 上限 1 MiB の rules の写しで 3 つの鍵の後に HEAD の鍵と生きた印の鍵が残り残りが at の古い順に消える (d) 壊れた SCIP・rc 1・上限 1 秒の写しで sleep する command・program の無い行の 4 形の語と rc 1 と表の不在 (e) 箱の記録と受付札の record が残り、記録した cwd が置き場の下の <commit の sha>.tree で、撃ち終えた後に木と外の道具の出力が無い (f) 歯が起こした sleep 1 の子の pid と起動時刻の印を置いた周の build が command を撃たず（回数 0）に子の終わりまで待ち（経過が子の sleep 以上）、子が終わる前に歯が置いた失敗の記録を読み直して failed:rc で rc 1、死んだ持ち主の印は外して built (g) 行 2 本のそれぞれを持たない rules の写しで回数 0 の failed:no-rule (h) 片方の key だけ・{out} の無い index-scip・{tree} の無い index-roles の 3 形の repo で build が rc 2 で key の名と行番号を持ち、同じ 3 つの repo で touches も欄も持たない契約の受付と contracts check は通り、2 key の無い repo は [INDEX] undeclared rc 0、lib の declaration_index_（optional_keys.rs の既存の test 区間・(j) 2 key の行の列・片方・穴の欠け・文字列・空の配列の不備と、同じ片方だけの宣言を vessel 宣言の parse が断らないこと）、e2e の rules_index_（rules.rs・(i) 2 行の id・kind・形 Int・値・enabled・裁定 id と裁定日・int_row の値・ALL と manifest の末尾 2 つの順・字面から引ける・文字列の値の写しは形と合わないで断られる）、lib の pipe_index_status_（新しい file の test 区間・flip の外の補い・(k) 6 値）、直す既存の歯 pipe_command_all_subcommands_round_trip_and_unknown_tokens_are_none（本数と語の列）・rules_drafts_cap_rows_are_the_last_two_kinds_and_rows（名は変えない）・rules_floor_timeout_row_precedes_the_drafts_cap_rows・rules_lifecycle_rows_carry_the_ruled_values_and_the_lifecycle_ruling（局面の出力の 2 kind・14 行を、memo の 3 kind・3 行と索引の 2 kind・2 行の前で読む）・rules_memo_rows_are_the_last_three_kinds_and_rows（memo の 3 kind・3 行を、ALL と manifest の末尾の索引の 2 kind・2 行の前で読む）・class_derive_embedded_row_carries_the_ruled_three_elements_and_ruling_id・rules_embedded_manifest_declares_host_guard_kinds_at_the_tail_of_all・rules_embedded_manifest_is_valid_and_covers_all_kinds・rules_embedded_manifest_declares_one_capability_row_per_role（どれも便の base で数え直し、末尾の読み飛ばしと数に索引の 2 kind・2 行を足す）と外形の snapshot 2 本は base で RED なので retroactive の札は要らない、base は subcommand・索引の宣言の読み・kind が無いので (a)〜(j) が RED"

[[contract]]
id = "b"
title = "本 repo の索引の宣言 — .vessel.toml に rust-analyzer の scip と ast-grep の scan の 2 key を足し、rust-toolchain.toml の components に rust-analyzer を足し、9 つの役の rule を持つ規則の file を置く（外の道具の採用は裁定 user 2026-09-30T22:13Z 項 index-tools・ADR-0105・§10）"
req = ["FR107", "NFR3"]
section = "10"
depends = ["a2"]
write-set = [".vessel.toml", "rust-toolchain.toml", "+.config/index-roles.yml", "crates/scribe2-boundary/tests/e2e/pipe.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_declared_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_build_", "cargo xtask check", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
done = "(1) 本 repo の HEAD の vessel 宣言を行 a2 の索引の宣言の読みで読むと、index-scip は rust-analyzer の scip の subcommand に {tree} と {out} を渡す 1 行、index-roles は ast-grep の scan に役の規則の file と {tree} を渡し stream の JSON を出す 1 行で、不備が 0〔pipe_index_declared_ の (a)〕 (2) rust-toolchain.toml の components は clippy・rustfmt・rust-analyzer で、channel は変わらない〔(b)〕 (3) 役の規則の file（.config の下の yaml 1 本）の rule の id の集合が器の役の 9 語の列と等しく、どの rule も言語 Rust で、名を捕える rule（literal・pattern・call・use・reexport・doclink・capture）は NAME の meta 変数を持つ〔(c)〕 (4) 宣言のコメントは ADR-0105 と本 doc の §4 を名指し、ほかの key の字と順は変わらない〔(a) の本文の ADR-0105 の字と既存の key の値の照合〕 (5) Cargo の依存は増えない〔cargo xtask check の deps-empty〕 歯: e2e の pipe_index_declared_（pipe.rs・repo の根は CARGO_MANIFEST_DIR から辿る・(a) 宣言の読み (b) toolchain の components (c) 規則の file の rule の id と言語と meta 変数を行で読む）、変わらない既存の歯 pipe_index_build_、base は key と component と file が無いので (a)(b)(c) が RED。着地の後に orchestrator が host で pipe index build を撃ち built を bead の notes に残す（歯ではない）"

[[contract]]
id = "c"
title = "逆引きの表と審査の材料 index.txt — 口 pipe index show が ref の commit の索引から行か名の項目ごとに 7 列の件数と site と write-set の外の印と母集団を出し、契約の審査の材料の dir に同じ字の index.txt を置き、lens が雛形の末尾の穴に token の残りで収めて埋める（§6・§7 (a)）"
req = ["FR108", "FR49"]
section = "6"
depends = ["a1", "a2"]
write-set = ["+crates/scribe2/src/pipe/review/index.rs", "crates/scribe2/src/pipe/cli.rs", "crates/scribe2/src/pipe/cli/args.rs", "crates/scribe2/src/pipe/review.rs", "crates/scribe2/src/pipe/closure/names.rs", "crates/scribe2/src/pipe/closure.rs", "crates/scribe2/src/headless/lens.rs", "crates/scribe2/src/headless/lens-contract.txt", "crates/scribe2-boundary/tests/e2e/pipe/review.rs", "crates/scribe2-boundary/tests/e2e/snapshots/e2e__pipe__pipe_external_form.snap", "=crates/scribe2-boundary/tests/e2e/pipe.rs", "=crates/scribe2-boundary/tests/e2e/headless.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_show_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_index_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_outside_", "cargo nextest run -p scribe2 --lib --no-tests=fail headless_lens_base_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail lens_contract_prompt_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_external_form", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_base_summary_file_names_every_write_set_item", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_index_build_", "cargo nextest run -p scribe2 --lib --no-tests=fail closure_names_section_symbols_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_name_form_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/review/index.rs:260", "crates/scribe2/src/pipe/cli.rs:15", "crates/scribe2/src/pipe/cli/args.rs:8", "crates/scribe2/src/pipe/review.rs:20", "crates/scribe2/src/pipe/closure/names.rs:45", "crates/scribe2/src/pipe/closure.rs:1", "crates/scribe2/src/headless/lens.rs:25"]
done = "(1) pipe index show --item <path> は ref（既定 HEAD）の commit の状態が ready なら、項目を symbol に解いて 7 列（refs・callers・literals・patterns・teeth・vis・rows）の件数と site（本体と test の別・file の数）と母集団（text=・indexed=・outside-index= と path）を出し、別名・Self の literal・glob・再輸出の越しの site を含み、別 module の同名の型の site を数えず、toml に現れる名を outside-index に数える〔pipe_index_show_ の (a)〕 (2) --row <doc>#<行 id> は ref の木の契約表から行を引き、touches と行の節の名指しを項目にし、site の file が行の write-set の外なら 外 を付けて列ごとに外の件数を出し、解けない項目は 0 件・複数の symbol は ambiguous:<n> と候補の定義の site を名指す。行の節の名指しは名指しの読み手の口 section_symbols（closure/names.rs・pipe::closure の再輸出）の 1 本で読み、unresolved_names と同じく予約語の呼び出しの形・pub(crate) のような可視性・大文字始まりの tuple variant の構築・touches の型の variant を項目にせず、crate:: の頭の無い型の path 形（型::項目）も項目にする〔(b) と lib の closure_names_section_symbols_〕 (3) show は行 a2 の組み立ての 1 本で索引を得て、無ければ組み、撃ち中の持ち主が生きていれば command を撃たずに終わりを待ち、組み立てが失敗した周は index=unavailable:<失敗の語>、2 key の無い repo は index=unavailable:undeclared の 1 行だけで rc 0、片方の key だけの repo（half）は索引の宣言の不備の key の名と行番号で rc 2、引けない行・解けない ref は rc 2、build に --row か --item を渡すと今のまま未知の引数の使い方の誤りで rc 2〔(c)〕 (4) 列の数えと描きはそれぞれ審査の新しい子 module の 1 関数（pub(in crate::pipe)・収める関数は lens が撃つので pub）で、show と index.txt が同じ字を出す〔(d) の show と index.txt の byte の一致〕 (5) 契約の審査（Reviewed）は既存の材料を置いた後に、審査の木の commit の索引から行を名指す表を index.txt として材料の dir に置き、状態が ready でない周は index=unavailable:<語> の 1 行、undeclared の repo は file を置かない（材料の dir の file の列は今のまま）〔(d)〕 (6) lens は index.txt が在れば雛形の末尾の outside の後ろの穴に、見出しと、器が外の道具の索引から組んだ事実で件数の横の母集団を合わせて読み site は読みの道具で開ける、の 1 文と表を、outside を足した後の token の残りに項目ごとに収め、収まらない項目は件数だけの 1 行・落とした項目の数（項目だけを数え、見出しと 1 文は数えない）を最後の 1 行にし、file が無い周は雛形の出力が 1 字も変わらない〔headless_lens_index_ の (e)・変わらない既存の歯 lens_contract_prompt_external_form〕 (7) 先撃ちの材料・既存の 6 材料の字と置き方・pipe index build の字と rc・名指しの読み手の既存の 3 口（unresolved_names・mentioned_names・named_items）の字と返りは変わらない〔変わらない既存の歯 pipe_review_base_summary_file_names_every_write_set_item と pipe_index_build_ と contract_name_form_ と verify の最終行の contracts check〕 (8) 審査の新しい子 module と歯の file は契約表の行の型と審査の材料の型（Material）を組まず match もせず、ほかの行の閉包を広げない〔verify の最終行の contracts check〕 歯: e2e の pipe_index_show_（review.rs・fixture の小さな crate は別名の取り込み・Self の literal・glob の取り込み・pub use の再輸出・test の module の file・doc の link・文字列の取り込み・別 module の同名の型・toml の名指しを持ち、SCIP と一致は行 a1 の歯の file〔pipe.rs〕の書き手で作る）の (a) --item の 7 列の site の集合と母集団の 3 値 (b) --row の外の印と列ごとの外の件数と ambiguous、行の節の散文が名指す pub(crate)・予約語の呼び出しの形・大文字始まりの tuple variant の構築・touches の型の variant は項目に出ず、crate:: の頭の無い型の path 形と fn 形は項目に出る (c) 索引の無い周の show が偽の宣言を 1 回撃って表を出し、歯が起こした sleep 1 の子の pid の印を置いた周の show は command を撃たず（回数 0）に子の終わりまで待ち（経過が子の sleep 以上）、rc 1 の偽の宣言の周は index=unavailable:rc、2 key の無い repo は index=unavailable:undeclared でどちらも rc 0、片方の key だけの repo と引けない行は rc 2、build に --item か --row を渡すと stderr が未知の引数を名乗り rc 2 (d) 偽の lens で受付から審査まで通した便の材料の dir の index.txt が同じ ref の show --row の出力と byte で等しく、索引を作らない周は index=unavailable:absent の 1 行、宣言の無い repo は file の列が今のまま、lib の headless_lens_index_（lens.rs の既存の test 区間・(e) 穴の埋めと収めと落とした数〔項目 2 つの写しのうち 1 つが収まらない周は落とした数 1〕・file が無い周の不変）、lib の closure_names_section_symbols_（names.rs の既存の test 区間・(f) 型の path 形〔crate:: の有無を問わない〕と fn 形を本文の順に重複なしで返し、予約語の呼び出しの形・pub(crate)・大文字始まりの tuple variant の構築・touches の型の variant・path 形・末尾 :: の module path を返さない）、直す既存の歯は雛形の末尾の穴の並びを pin する lens.rs の lib の歯 headless_lens_base_fills_the_hole_only_when_the_copy_exists_in_one_pass と headless_lens_outside_fills_the_last_hole_only_when_the_copy_exists（index の穴を outside の後ろに足す・直した本文が base で緑のままの歯は retroactive の札を便の bead で付ける）、外形の snapshot pipe_external_form（index の verb に show を足す）、base は verb と材料と穴と口が無いので (a)〜(f) が RED"

[[contract]]
id = "d"
title = "受付の索引の閉包と索引の状態の扱い — touches の型を持つ行の受付・preflight・起動の列が base の索引の literal と pattern と variant の site で閉包を数えて字面の閉包が名指さない file の不足を (索引) つきの write-set-incomplete で断り、作り中は index-building で断るか待たせ、作れない周は字面に縮退して index=unavailable を足し、列は索引の無い HEAD に組み立てを裏で起こす（§7 (b)）"
req = ["FR107", "FR48", "FR68"]
section = "7"
depends = ["a1", "a2"]
touches = ["crate::pipe::refuse::Refuse"]
write-set = ["crates/scribe2/src/pipe/closure.rs", "+crates/scribe2/src/pipe/cli/intake/index.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/refuse.rs", "crates/scribe2/src/pipe/cli/preflight.rs", "crates/scribe2/src/pipe/dispatch/candidates.rs", "crates/scribe2/src/pipe/dispatch.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "=crates/scribe2-boundary/tests/e2e/pipe.rs", "=crates/scribe2/src/pipe/cli/intake/refusal.rs", "=crates/scribe2/src/pipe/closure/names.rs", "=crates/scribe2/src/pipe/table.rs", "=crates/scribe2/src/pipe/table/check.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_index_closure_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_index_building_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_names_are_pinned_in_declaration_order", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_refuse_evidence_is_decided_once_for_each_of_the_23_words", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_refuse_evidence_discern_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_preflight_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/closure.rs:70", "crates/scribe2/src/pipe/cli/intake/index.rs:80", "crates/scribe2/src/pipe/cli/intake.rs:25", "crates/scribe2/src/pipe/refuse.rs:25", "crates/scribe2/src/pipe/cli/preflight.rs:4", "crates/scribe2/src/pipe/dispatch/candidates.rs:40", "crates/scribe2/src/pipe/dispatch.rs:10"]
done = "(1) 受付の材料は索引の状態を 1 つ持ち、手の受付・pipe preflight・起動の列の候補は base（列は HEAD）の commit の状態を行 a2 の状態の読みで載せ、予想の base の写しは状態なし、contracts check は今の字面の閉包のまま〔pipe_intake_index_closure_ の (a) の後半〕 (2) touches の型の項目ごとに、索引の literal の役・pattern の役・variant の symbol の本体の参照の site の file を数え、字面の閉包が名指さず write-set に無い file を、file に (索引) を添えた write-set-incomplete で断り（rc 1・便を作らない）、同じ file を write-set に足した行は通る〔(a)〕 (3) 索引を要する行は touches に型の項目を持つ行と欄 code-facts を持つ行だけで、どちらも持たない行はどの状態でも出力の字が変わらない〔(d) の A/B〕 (4) touches の型を持ち欄を持たない行は、absent と building の周に手の受付と preflight が断り index-building（状態の語を名指す・在り処は置き場）で断り、起動の列が受付の理由 index-building で待たせる〔(d)(e)〕 (5) failed の周と予想の base では字面の閉包だけで判じ、受付の結果の行と preflight の出力と dispatch ls の候補の行に index=unavailable:<語> を足して通し、undeclared の repo は尾を足さない〔(a) の後半・(c)(e)〕 (6) 欄 code-facts を持つ行は index-building にしない（行 e の断りに寄せる・本行では欄を持つ行の作り中の周を今のまま通す）〔(d) の欄を持つ行〕 (7) 起動の列の周は、索引を要する候補が在り HEAD の状態が absent の周に、候補を組む file（candidates.rs）が pipe index build --ref <HEAD> を dispatch.rs の spawn_self で裏に 1 本起こして待たず（dispatch.rs の余地は 23 行なので足すのは配線だけ）、生きた印の周と failed の鍵には起こさない〔(e) の偽の宣言の呼ばれた回数〕 (8) REFUSALS の末尾が index-building で、Evidence は Place、rc は RC_REFUSED で、断りの 1 行は状態の語を名指し、既存の歯 refuse_names_are_pinned_in_declaration_order と pipe_refuse_evidence_is_decided_once_for_each_of_the_23_words の本文を retroactive の札（便の bead）つきで直す（名は変えない）〔refuse_index_building_ の (f)〕 (9) 判定は generated の表の検査の後に撃ち、索引の閉包の断りは表の検査の findings に足し、ほかの判定の順と字は変わらない〔変わらない既存の歯 pipe_intake_ と pipe_preflight_〕 (10) 索引の閉包の関数は字面の閉包の file（closure.rs）に置いて行 a1 の表の問いを呼び、状態の扱いと判定は受付の新しい子 module（intake の下・ruling.rs と同じ置き方）に置いて閉じた結果を返し、どちらも Refuse・ContractRow・TableError・Denial・WaitReason を組まず match もしない（断りを組むのは intake.rs・待ちの理由を組むのは candidates.rs・intake.rs の余地は 149 行で行 e と分け合うので intake.rs に足すのは配線だけ）〔verify の最終行の contracts check〕 (11) 索引を要する行（touches の型を持つ行と欄を持つ行）は、状態が half の repo で手の受付と preflight が今の宣言の不備と同じ断りの名 declaration（索引の宣言の読みの key の名と行番号・rc 1）で断り、起動の列が受付の理由 declaration で待たせ、索引を要しない行の受付と起動の列と contracts check は同じ repo で今のまま通る〔(g)〕 歯: e2e の pipe_intake_index_closure_（intake.rs と dispatch.rs・偽の宣言と SCIP の書き手は行 a1・a2 の歯の file〔pipe.rs〕の物を使う）の (a) touches の型を別名の取り込みで組む file X が write-set に無い行を、base の索引が ready の受付が X (索引) の write-set-incomplete で rc 1 で断り run を作らず、X を write-set に足した同じ行は通り、後半で同じ repo から宣言の 2 key を外した commit の受付と contracts check は X の無い行を通し受付の結果の行に index= が無い (c) 失敗の記録を持つ鍵の周の受付が rc 0 で結果の行に index=unavailable:rc、preflight の出力にも同じ尾、同じ doc の未着地の行を depends に持つ行の preflight（予想の base）は index=unavailable:forecast を足して通る (d) absent の周と生きた印の周の受付が rc 1 の index-building で状態の語 absent と building を名指し、touches も欄も持たない行と欄を持つ行は同じ 2 周で通る (e) dispatch ls の候補が absent の周に reason=admission:index-building で待ち、1 周が偽の宣言を 1 回だけ裏で撃ち、生きた印の周の 2 周目は撃たず、失敗の記録の周は候補に index=unavailable:rc が付いて待たず組み立ても起こさない、(g) 片方の key だけ宣言した repo で touches の型を持つ行と欄を持つ行の受付が rc 1 の declaration で key の名と行番号を名指し、dispatch ls の同じ 2 候補が reason=admission:declaration で待ち、touches も欄も持たない行の受付と候補と contracts check は通る、lib の refuse_index_building_（refuse.rs の既存の test 区間・(f) 語と Evidence と rc と 1 行）、base は断りと尾と裏の起こしと half の断りが無いので (a)(c)(d)(e)(f)(g) が RED（(a) は前半の断りで落ちる・(g) は前半の断りで落ちる）"

[[contract]]
id = "e"
title = "code の事実の欄の照らしと測り — 欄 code-facts の要素の形を契約表の検査が code-facts-form で名指し、受付・preflight・起動の列が base か HEAD の索引で列の数えを撃って違えば code-facts で断るか待たせ、測れない周は code-facts-unmeasured で断るか待たせ、索引の key を名乗らない repo では確定の断りにする（§7 (c)）"
req = ["FR109", "FR47", "FR55", "FR68"]
section = "7"
depends = ["c", "d"]
touches = ["crate::pipe::refuse::Refuse", "crate::pipe::table::TableError", "crate::pipe::table::ContractRow"]
write-set = ["crates/scribe2/src/pipe/table.rs", "crates/scribe2/src/pipe/table/parse.rs", "crates/scribe2/src/pipe/table/check.rs", "crates/scribe2/src/pipe/refuse.rs", "+crates/scribe2/src/pipe/cli/intake/code_facts.rs", "crates/scribe2/src/pipe/cli/intake.rs", "crates/scribe2/src/pipe/contract.rs", "crates/scribe2-boundary/tests/e2e/pipe/contracts.rs", "crates/scribe2-boundary/tests/e2e/pipe/intake.rs", "crates/scribe2-boundary/tests/e2e/pipe/dispatch.rs", "=crates/scribe2/src/hook/live_row.rs", "=crates/scribe2/src/pipe/cli/intake/refusal.rs", "=crates/scribe2/src/pipe/closure/names.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_code_facts_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_code_facts_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_dispatch_code_facts_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_code_facts_", "cargo nextest run -p scribe2 --lib --no-tests=fail contract_check_code_facts_", "cargo nextest run -p scribe2 --lib --no-tests=fail refuse_names_are_pinned_in_declaration_order", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_refuse_evidence_is_decided_once_for_each_of_the_23_words", "cargo nextest run -p scribe2 --lib --no-tests=fail table_error_names_are_pinned_in_declaration_order_and_carry_their_line", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_table_evidence_is_decided_once_for_each_of_the_17_variants", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_intake_index_closure_", "cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail contract_fields_read_only_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "M"
growth = ["crates/scribe2/src/pipe/table.rs:25", "crates/scribe2/src/pipe/table/parse.rs:20", "crates/scribe2/src/pipe/table/check.rs:45", "crates/scribe2/src/pipe/refuse.rs:45", "crates/scribe2/src/pipe/cli/intake/code_facts.rs:110", "crates/scribe2/src/pipe/cli/intake.rs:20", "crates/scribe2/src/pipe/contract.rs:0"]
done = "(1) 契約表の行の型は欄 code-facts の値を持ち（行 bu が捨てた値・構築点は parse の 1 か所と test の 2 か所）、生成する契約 file には写さない〔pipe_intake_code_facts_ の (b)(c)〕 (2) 契約表の検査（contracts check と受付の同じ 1 本）は、要素が <列>:<項目>=<値> の形でない・列が refs・files・callers・literals・patterns・teeth・vis の 7 語の外・項目が crate:: で始まり :: で結んだ識別子の列でない・値が 10 進でない（vis では pub・pub(crate)・pub(super)・pub(in <path>)・private の 5 形の外）の要素ごとに TableError の code-facts-form を行番号つきで名指して非 0 で止まり、適合の要素だけなら 0 件で、索引を読まない〔(a)・contract_check_code_facts_ の (g)〕 (3) 欄を持つ行の受付と preflight は base の索引が ready の周に要素ごとに行 c の列の数えを撃ち、名乗りと等しければ通り、違えば code-facts（在り処は本文の読み手）で rc 1 で断り、1 行は要素・名乗りの値・実測の値・増えた site と消えた site（先頭 3 つと残りの件数）・母集団 text= を名指す〔pipe_intake_code_facts_ の (b)(c)〕 (4) 起動の列は同じ判定を HEAD の索引で撃ち、違えば受付の理由 code-facts で待たせる〔pipe_dispatch_code_facts_ の (e)〕 (5) 欄を持つ行が absent・building・failed・予想の base の周は、受付と preflight が code-facts-unmeasured（要素と状態の語を名指す・在り処は置き場）で rc 1 で断り、起動の列が受付の理由 code-facts-unmeasured で待たせ、touches の型も持つ行でも index-building にしない〔(d)(e)〕 (6) 宣言が索引の 2 key を名乗らない repo で欄を持つ行は、受付と preflight が code-facts-unmeasured（語 undeclared・在り処は vessel 宣言の file で確からしさは firm）で断り、起動の列が code-facts-unmeasured で待たせ、片方だけ名乗る repo では行 d の索引の宣言の断り（名 declaration）で受付が断り起動の列が admission:declaration で待つ〔(d)(e)・refuse_code_facts_ の (f)〕 (7) 欄を持たない行はどの状態でもこの判定の外で、行 d の扱いのまま〔(d)(e) の欄の無い行〕 (8) REFUSALS の末尾が code-facts・code-facts-unmeasured の順で、Evidence は Name と、undeclared なら Files（宣言の file）・ほかは Place、rc は RC_REFUSED、TableError の末尾が code-facts-form で Evidence は Row、既存の歯 refuse_names_are_pinned_in_declaration_order・pipe_refuse_evidence_is_decided_once_for_each_of_the_23_words・table_error_names_are_pinned_in_declaration_order_and_carry_their_line・pipe_table_evidence_is_decided_once_for_each_of_the_17_variants の本文を retroactive の札（便の bead）つきで直す（名は変えない）〔refuse_code_facts_ の (f)・contract_check_code_facts_ の (g)〕 (9) 行 d の索引の閉包と、欄 done-teeth と code-facts の読むだけの歯は変わらない〔変わらない既存の歯 pipe_intake_index_closure_ と contract_fields_read_only_〕 (10) 要素の読みは契約表の検査の file の 1 関数で、表の検査と受付の測りが同じ 1 本を撃ち、測りは受付の新しい子 module（intake の下）に置いて閉じた結果を返し、その子 module は Refuse・TableError・ContractRow・Denial を組まず match もせず（断りを組むのは intake.rs で、足すのは配線だけ）、足す code はほかの行の touches の型を今それを名指していない file で新しく名指さない〔verify の最終行の contracts check〕 歯: e2e の contract_code_facts_（contracts.rs・(a) 列の語の外・値の形の外・vis の字の外・項目の path の形の外・= の無い要素の 5 行が code-facts-form の 5 件で各行の見出しの行番号を持ち非 0、適合の 7 列の要素を持つ行は 0 件）、e2e の pipe_intake_code_facts_（intake.rs・偽の宣言と SCIP の書き手は行 a1・a2 の物・(b) literals と refs と vis の名乗りが実測と等しい行が受付と preflight を通り、受付が作った便の契約 file に code-facts の key が無い (c) fixture の code に組み立ての site を 1 つ足した commit で literals の要素が名乗り 2・実測 3 と増えた site の path:行と text= を名指す code-facts で rc 1 (d) absent・生きた印・失敗の記録・宣言の無い repo の 4 周で欄を持つ行が code-facts-unmeasured と語 absent・building・failed:rc・undeclared を名指し、片方の key だけの repo の受付が宣言の不備で断り、touches の型も持つ同じ行も index-building にならず、欄の無い行は同じ 4 周で行 d の扱いのまま）、e2e の pipe_dispatch_code_facts_（dispatch.rs・(e) dispatch ls の候補が違いの周に reason=admission:code-facts、測れない 4 周に reason=admission:code-facts-unmeasured、片方の key だけの repo で reason=admission:declaration で待ち、欄も touches の型も無い候補は失敗の記録の周と宣言の無い repo の 2 周で待たない）、lib の refuse_code_facts_（refuse.rs の既存の test 区間・(f) 2 語の 1 行と rc と Evidence と、undeclared と absent の確からしさが firm と unmeasured）と contract_check_code_facts_（check.rs の既存の test 区間・(g) 5 形の code-facts-form と適合の 0 件）、base は欄の値を読まず語と断りが無いので (a)〜(g) が RED"

[[contract]]
id = "g"
title = "外の材料の .rs の item と要約の塊を外す — outside.txt は write-set の外の .rs の item の本文と名指された .rs の要約を束ねず、depends の相手の行・依存の表・親 module の宣言・data file の鍵と write-set の中の item の本文・別の設計の § の塊だけを残し、説明の 1 文を読みの道具と index.txt へ向ける（§16）"
req = ["FR49", "FR108"]
section = "16"
depends = ["c"]
write-set = ["-crates/scribe2/src/pipe/review/outside.rs", "crates/scribe2-boundary/tests/e2e/pipe/review.rs"]
verify = ["cargo nextest run -p scribe2-boundary --test e2e --no-tests=fail pipe_review_outside_", "cargo nextest run -p scribe2 --lib --no-tests=fail pipe_review_outside_", "cargo nextest run -p scribe2 --lib --no-tests=fail named_item_body_", "cargo nextest run -p scribe2 --lib --no-tests=fail linked_section_material_", "cargo run -q -p scribe2-boundary --bin scribe2 -- contracts check --repo ."]
size = "S"
done = "(1) outside.txt は write-set の外の .rs の item の塊と名指された .rs の要約の塊を持たず、それだけを使う私有の関数は消え、本文が write-set の外の .rs の item だけを名指す契約の材料の dir に outside.txt が無い〔pipe_review_outside_trimmed_ の e2e (a)〕 (2) depends の相手の行・crate の依存の表・親 module の宣言・data file の鍵の塊は同じ順と字で残り、その後ろの write-set の中の item の本文と別の設計の § の塊も変わらない〔lib (b)・変わらない既存の歯 pipe_review_outside_ の残りと named_item_body_ と linked_section_material_〕 (3) 見出しの下の説明の 1 文から .rs の item と要約の句が外れ、.rs の item の本文は読みの道具で開け、名の使われ方は index.txt が渡す、の 1 句が在る〔lib (b) の説明の字〕 (4) 材料の file の名と置き方・outside の穴と cap の収め方・名指しの読み手・base.txt は変わらない〔lens.rs は変えない・変わらない既存の歯 pipe_review_outside_ の cap の歯〕 歯: e2e の pipe_review_outside_trimmed_（review.rs・(a) § が backtick の外で write-set の外の struct だけを名指す契約の材料の dir の file の列に outside.txt が無く、同じ § に data file の名指しを足した契約の outside.txt が data file の鍵の塊を持ち struct の塊を持たない・既存の歯 pipe_review_outside_material_carries_the_named_outside_struct をこの歯へ置き換える）、lib の pipe_review_outside_trimmed_（outside.rs の既存の test 区間・(b) 既存の歯 pipe_review_outside_names_an_outside_struct_with_its_fields を置き換え、同じ fixture で struct と要約の塊が無く depends・依存の表・親 module・data file の塊の見出しがこの順で在り、説明の 1 文が新しい句を持つ）、base は塊と句が在るので (a)(b) が RED"
<!-- contracts:end -->
