# 設計: code の索引と逆引きの表 — 外の道具が作った索引を器が読み、行の審査・受付・起動の列が code の事実を測る

- 出所: epic s2-07l.736.33 の打ち手 3（逆引きの表を器が機械で組む）と打ち手 6（設計に書いた code の事実を、便を起こす時点で測り直す）→ [ADR-0105](../../design-intent/decisions/ADR-0105-code-facts-come-from-an-external-index-the-vessel-reads.html)。材料は 2026-09-28〜09-30 の便 165 本の FAIL / INCONCLUSIVE 76 件の分類と、同じ日の索引の道具の試し（問い 74・採点 53・正解の site 249・base 32 本）。どちらも host の file で tracked でない。推奨の採用は常設の裁定 user 2026-09-28T00:54Z（決めてほしいことは推奨で進める）の適用で、外の道具を採ること（憲法 A3）と rules 行の値 2 つ（§4 形 7）と索引の置き場の消し（A1）は、SRS の追加 round で裁定を取った（user 2026-09-30T22:13Z 項 index-tools・項 index-cap・項 index-timeout）。
- 要件（今の字）: [FR48](../../design-intent/spec/srs.html#FR48) 閉包 / [FR55](../../design-intent/spec/srs.html#FR55) CI の契約表の検査 / [FR47](../../design-intent/spec/srs.html#FR47) 契約の正本 / [FR49](../../design-intent/spec/srs.html#FR49) 契約の審査 / [FR68](../../design-intent/spec/srs.html#FR68) 起動の列 / [NFR3](../../design-intent/spec/srs.html#NFR3) 依存 / [NFR6](../../design-intent/spec/srs.html#NFR6) host の資源。FR48・FR55・FR47 の字を直し、要件を 3 つ足す SRS の追加 round が先に要る（§12）。
- 前提: 審査役が読みの道具を持つ設計（[pipeline.md](./pipeline.md) §64・行 bg）、行の審査（[row-review.md](./row-review.md)・[ADR-0103](../../design-intent/decisions/ADR-0103-contract-rows-pass-row-review-before-merge-and-failed-rows-keep-their-place.html)）、done の項目ごとの歯の欄（[contract-source.md](./contract-source.md) の done の欄の設計・同じ epic の別の PR・未着地）。表を材料に足す場所と受付の検査に足す場所は、この 3 つの設計の口に揃える。
- この設計から出る契約: §12 の 8 行。行 0 は [contract-source.md](./contract-source.md) の契約表の行 bu（§67）に束ね、行 f は SRS の round を待たずに本 doc の契約表に足した（§15）。ほかの行は SRS の round と A3 の裁定の後に足す。

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
- **入れ子の JSON の読み手**: `crates/scribe2/src/account/mod.rs` の私有の `read_tree` が 1 本在る。書き手は無い。
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

## 4. 索引の宣言と組み立て

- 形（番号は §12 の行 a・b の done と 1:1 にする予定）:
  1. **宣言**（vessel 宣言の任意 key 2 つ・どちらも command の字の列）:
     - `index-scip`: 1 行ごとに SCIP の file を 1 つ出す command。穴は `{tree}`（索引を作る commit の木）と `{out}`（出力の file の path・器が行ごとに別の名を渡す）。言語ごとに 1 行（Rust は rust-analyzer・TypeScript は scip-typescript・Python は scip-python か ty-scip）。
     - `index-roles`: 1 行ごとに構文の役（§5）の一致を stdout に 1 行 1 件の JSON で出す command。穴は `{tree}`。本 repo は ast-grep の scan に役の規則の file を渡す 1 行。
     - どちらも無い repo は索引を持たない（今の振る舞いのまま・§7 の検査は全部撃たない）。片方だけの宣言は読めない宣言として断る。
  2. **撃つ所**: 索引を作る commit を detach した木（行の審査の行 a が共用にする木の実体化と片付け・[row-review.md](./row-review.md) §3 形 5）の上で、cwd をその木にして宣言の順に撃つ（rustup の proxy は cwd の toolchain の宣言で版を選ぶ）。撃つ子は封じ込めの箱の中で走り（NFR6）、受付札を 1 枚（jobs 1・job の memory は `gate.job_memory_mb`）取る。時間の上限は rules 行 `index.timeout_s`。
  3. **読む**: 器は外の道具の library を持たない（NFR3）。
     - SCIP は protobuf の wire の形のうち、document の path・occurrence の範囲と symbol と役の印と囲む範囲・symbol の情報の名と種類だけを std の読み手で読む。外の crate の symbol と関数の中の local は落とす。
     - 役の一致は JSON の 1 行ずつを、入れ子の JSON の読み手（account の私有の `read_tree` を crate の中へ開いて共用する・2 本目の JSON の読み手を作らない）で読む。
  4. **結び**: occurrence ごとに、範囲を含む最も内側の役を付ける。役の一致のうち SCIP の occurrence を持たない物（doc の link・文字列の取り込み・`Self` の literal）は、捕えた名の字と同じ字の occurrence を、囲む定義 → 同じ file の順に探して symbol を借りる。借りられない物は字だけの行として残し、印を付ける。module を宣言する occurrence が test の役の中に在る module の file は、全体を test にする。
  5. **平らな表**（器が書き器が読む on-disk の形・schema 1）: 1 行 1 occurrence で、path・行・列・symbol・定義か・役の列・test か・囲む定義の symbol・source の可視性の字。頭の行は `schema=1`。書きは一時 file → rename。形を変える版は schema を上げ、古い表を読まない（作り直せる）。
  6. **鍵と置き場**: 索引の鍵は、code の木の鍵（[row-review.md](./row-review.md) §5・契約表を持つ file を除いた全 file の path と blob の hash の列の digest・役の規則の file も tracked なのでここに入る）と、宣言 2 key の字を並べた字の FNV-1a 64（16 桁）。置き場は state dir の pipe の下の index の dir で、鍵ごとに平らな表と記録（1 行 1 key の `key=value`・1 行目は `schema=1`・key は key・commit・宣言の digest・rows・files・secs・at・stderr の末尾）。撃ち中の印は `<鍵>.pid`（`<pid> <起動時刻>`・`lock_owner` で生死を判じる）。外の道具の出力（SCIP の file と一致の列）は平らにした後に外す。
  7. **量の上限と消し**: 置き場の合計が rules 行 `index.cap_mb` を越える周は、撃ち中の鍵と、anchor の HEAD の鍵を除いて、記録の at の古い順に上限まで消す（[ADR-0101](../../design-intent/decisions/ADR-0101-seat-draft-build-dirs-are-capped-per-state-dir-and-shed-oldest-first.html) と同じ形・消すのは器が作った導出値だけ）。撃つのは組み立てが表を書いた直後の同じ印の中（2 つ目の掃除を足さない）。rules 行 2 本を読めない周は組み立てを撃たず、索引は無いと読む（既定値に倒さない）。
  8. **無い・壊れた・古い**: 表が無い・schema が違う・読めない鍵は「無い」と読む。組み立てが rc 0 でない・時間切れ・出力を読めない周は、記録に失敗の語（`failed:<rc|timeout|unreadable>`）を書き、表は置かない。索引は導出値で、真実の置き場にしない（憲法 C3・C10）。
- 口: `<NAME> pipe index build --repo R --state-dir S [--ref <sha>]`（既定は HEAD・同じ鍵の表が在れば撃たない・撃ち中の印の持ち主が生きていれば、唯一の待機の実装の pid の終わりの完了の値で待つ〔C3.4・今の値で名が合わなければ値を 1 つ足すかを行 a で決める〕）と `<NAME> pipe index show --repo R --state-dir S [--ref <sha>] (--row <doc>#<行 id> | --item <path>)…`（§6 の表を出す）。どちらも前面で最後まで走る。結果の 1 行は `[INDEX] key=<16 桁> rows=<n> files=<m> built|cached|failed:<語> secs=<s>`。

## 5. 構文の役（閉じた語の列・言語は宣言の側）

- 器が知るのは SCIP の形と、役の語の閉じた列だけ。どの構文の node がどの役かは、宣言が名指す役の規則の file（言語ごと）が持つ。規則の id は役の語と同じ字にする。
- 役の語（9 つ）: literal（struct・record の literal の名・`Self` を含む）・pattern（match の pattern の位置の path）・call（呼び出しの callee）・use（import の宣言の中）・reexport（公開の再輸出の中）・test（test の関数・test の module・test の file の範囲）・doclink（doc の link の中の名）・capture（文字列の中の取り込みの名）・vis（定義の可視性の字を捕える）。
- Rust・TypeScript・typed Python は SCIP の indexer を持つ（rust-analyzer・scip-typescript・scip-python / ty-scip）。ast-grep は 3 言語の文法を 1 つの binary に持つ。言語を足すのは宣言の 2 key と規則の file を足すことで、器の code は変わらない。
- 今 Rust の形に閉じている所（言語を足すときに広げる所）: touches と名指しの path の形（`crate::` の頭と `::` の区切り）・字面の閉包（`.rs` の 6 形）・歯の名の読み手（nextest の形）。索引はこれらを置き換えず、touches の path を SCIP の descriptor の名の列の末尾一致で symbol に解く。

## 6. 逆引きの表（項目・列・母集団）

- 項目: 契約表の行ごとに、(i) touches の項目と、(ii) 行が実装する § の散文の名指しのうち型の path 形と fn 形（`unresolved_names` と同じ読み）。項目ごとに symbol へ解き、解けない（0）・1 つ・複数（`ambiguous:<n>` と候補の定義の site）を名指す。`--item` は同じ解き方で名を 1 つ受ける。
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
- 材料の大きさ: index.txt は審査の材料の既存の予算（`gate.token_cap` の残り）に項目ごとに収め、収まらない項目は件数だけの 1 行にし、落とした項目の数を最後の 1 行に数える（outside.txt と同じ扱い・新しい閾値を作らない）。

## 7. 使う場所

### (a) 審査の材料
- 行の審査（[row-review.md](./row-review.md) §3 形 5）と契約の審査（Reviewed）の材料の dir に index.txt を足す。中身は §6 の表（読む木の commit の索引・行を名指す形）。索引が無い周は `index=unavailable:<語>` の 1 行だけを置く（黙って落とさない）。
- index.txt は材料の dir の file なので、行の審査の材料の鍵（[row-review.md](./row-review.md) §9）に自然に入る。索引の中身は code の木の鍵で決まり、判定の鍵にはすでに入っている。
- lens の雛形は「index.txt は器が外の道具の索引から組んだ事実で、件数の横の母集団を合わせて読む」と告げる（読みの道具で site を開ける・§64）。

### (b) 受付の索引の閉包
- 表の検査の 1 行の判定（`generated` → 表の検査・行の審査の機械の検査〔[row-review.md](./row-review.md) §3 形 4〕・`pipe preflight`・受付）に、索引を持つ周だけ効く判定を 1 つ足す。CI（索引を持たない）は今の字面の閉包のまま。
- 索引の閉包: touches の型の項目ごとに、字面の閉包の形 1（literal）・形 2（match の arm）・形 6（variant 構築）と同じ形を、字の代わりに索引の解いた symbol で数える（literal の役・pattern の役・variant の symbol の本体の参照）。形を足さず、別名・`Self`・glob の越しの site が加わるだけ（字面の閉包の上に足す）。形 3・形 4 と fn 形は字面のまま。
- 索引の閉包が名指し、字面の閉包が名指さない file が write-set に無ければ、字面の閉包と同じ write-set-incomplete の finding を、在り処に `(索引)` を添えて出す（確定の finding・行の審査では lens を撃たずに FAIL）。
- 索引の状態ごとの扱い（索引を要するのは touches の型か欄 `code-facts` を持つ行だけで、どちらも持たない行はどの状態でも今のまま）: 撃ち中 → 起動の列はその候補を受付の理由 `index-building` で待たせ、手の受付は同じ名で断る（便を作らない）。失敗・宣言が無い → 字面の閉包だけで判じ、受付の 1 行と `dispatch ls` の行に ` index=unavailable` を足す（止めない、縮退する・[gate-cost.md](./gate-cost.md) §2 と同じ極性）。
- 新しい `+` の file が将来名指す型は、受付の時点で file が無いので測れない。代わりに、runner の stdin に「ほかの行の touches の型」の節を足す: 行の write-set の外の行（open な行と着地済みの行）の touches の型の列と、その行 id（契約表から導く・索引は要らない）。runner がその型を字面の閉包の形で名指すと、終わりの門（[pipeline.md](./pipeline.md) §66）と gate の共通の検証の閉包の歯が落とす。節は、落ちる前に runner に知らせる形である。

### (c) code の事実の欄と、起動の列の測り直し（打ち手 6）
- 行の欄 `code-facts`（任意・文字列の列）: 要素 1 つが `<列>:<項目>=<値>`。列は §6 の refs・files（refs の file の数）・callers・literals・patterns・teeth・vis の 7 語、項目は touches と同じ path の形、値は件数（10 進）か、vis では可視性の字。設計者は `pipe index show` の値を写す。§ の散文の数は機械が読まない。散文は欄を名指す。
- 測る所:
  - 表の検査（CI を含む）: 要素の形・列の語・項目の path の形・値の形だけを照らす。
  - 行の審査の機械の検査: `--ref` の commit の索引で測り、違えば確定の finding（行は FAIL）。
  - 受付と `pipe preflight`: base の索引で測り、違えば断る（便を作らない）。
  - 起動の列: 候補ごとの `generated` が同じ判定を HEAD の索引で撃つ。違えば受付の理由 `code-facts` で待つ（FR68 の閉じた理由の「受付」の内・新しい待ちの理由を足さない）。
- 断りの 1 行は、要素・宣言の値・実測の値・増えた site と消えた site（先頭 3 つと残りの件数）・母集団（`text=`）を名指す。
- 索引を測れない周（撃ち中・失敗・宣言が無い）に欄を持つ行は、受付の理由 `code-facts-unmeasured` で待つ（名乗った事実を測らずに通さない・C10）。欄の無い行は (b) の扱いのまま。
- 起動の列の周は、索引を要する候補が在り、HEAD の鍵の表が無く撃ち中の印も無いとき、`pipe index build` を 1 本、裏で起こして待たない（`spawn_self` と同じ起こし方・撃ち中の印が 2 本目を止める）。

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
| 材料の file 1 つ・受付の理由 3 語・runner の stdin の節 1 つ | — |

- event の種類・待ちの理由の variant・新しい門は足さない（受付の理由と表の検査の finding に載せる）。

## 10. A3 の論点（外の道具）

- 本 repo が採る道具（推奨）:
  - rust-analyzer: `rust-toolchain.toml` の component に足す。版は toolchain の channel が固定する（今の 1.98.1）。
  - ast-grep: 版を固定して `cargo install --locked` で入れる（試しの版 0.45.3・MIT）。host の `sg` は別物なので、宣言は `ast-grep` の名で撃つ。
- 採らない道具: scip の CLI（器が protobuf を読むので要らない）・tree-sitter の Python の binding（試しの道具）・bubblewrap（封じ込めは器の箱が持つ）・既製の graph 3 つ（§3）。
- 別の言語の道具（scip-typescript・scip-python / ty-scip）は、その言語の消費側の repo が宣言するときに、その repo の A3 で決める。scip-python は保守が 1 年止まっている。
- 採らない場合の代わり（却下案）: 字面の閉包を std の走査のまま広げる（別名の表・`impl` の範囲の追跡・glob の展開・呼び手の関数の範囲・可視性の連鎖・test の文脈）。見積りは 1500〜3000 行の Rust に閉じた構文の近似（未実測）で、method の呼び出しの受け手の型（`x.measure()` の `x` の型）は型推論が要り字面では解けず、下界のまま残る。言語を足すたびに同じ量を書き直す。
- 2026-09-27 の裁定との関係: 退けられたのは Rust 専用の道具。この形では器が読むのは SCIP（言語に依らない形式）と役の語だけで、Rust に依る物は本 repo の宣言と規則の file に閉じる。

## 11. 試し撃ち（補助・後段）

- compiler に仮の編集を当てて診断を読む形（item に `#[deprecated]` を付けて使う site を全部出させる・struct に欄を足して E0063 で literal の site を出させる・enum に variant を足して E0004 で網羅の match を出させる）。
- 試しでは、呼び手・literal・可視性で満点（16/16・5/5・7/7）だが、索引も同じ問いで満点で、試し撃ちだけが答えた問いは 0 だった。試し撃ちだけが正本になるのは、`..base` の構築（欄を足しても落ちない）と `_` で受ける match で、44 件の失敗には無かった。
- そのため行は起こさず、失敗の分類にこの型が 1 件出た時に行 h として起こす（§12）。編集の字と診断の読みは言語ごとに違い（tsc・pyright も同じ形の診断を持つ）、宣言の側に置く。

## 12. SRS の round の後の行（粒度・順序・write-set の見込み・歯）

SRS の追加 round（FR48・FR55・FR47 の字の直しと新しい要件 3 つ・直しの一覧は epic s2-07l.736.33 の notes が名指す）と A3 の裁定の後に、次の行をこの doc の契約表に足す。write-set は見込みで、起票の前に `pipe preflight` と行の審査で測り直す。行 0 と行 f は round を待たないので先に起こした（行 0 は contract-source.md §67 の行 bu・行 f は §15）。

| 行 | 中身 | 順（台帳の blocks） | write-set の見込み |
|---|---|---|---|
| 0 | 欄 `code-facts` と宣言の key `index-scip`・`index-roles` を読むだけ（読んで捨てる・効かせない） | round を待たない（FR47 の「少なくとも」の内）。done の歯の欄の読むだけの行と束ね、[contract-source.md](./contract-source.md) §67 の行 bu に起こした | `crates/scribe2/src/pipe/table.rs`・`contracts/schema.toml`・`crates/scribe2/src/pipe/declaration/optional_keys.rs`・`crates/scribe2/src/pipe/declaration.rs`・e2e の既存の歯の file |
| a | 索引の組み立て（§4 形 1〜8・`pipe index build`・SCIP と役の一致の読み手・結び・平らな表の書き手と読み手・撃ち中の印・量の上限・rules 行 2 本） | 0 と、行の審査の行 a（共用の木）の後。touches は rules の kind の閉じた型（行 2 本の kind） | pipe の新しい子 module（`+`）・`crates/scribe2/src/pipe/cli.rs`・`crates/scribe2/src/pipe/cli/args.rs`・`crates/scribe2/src/help.rs`・`crates/scribe2/src/account/mod.rs`（JSON の読み手を開く）・`crates/scribe2/src/rules/mod.rs`・`rules/manifest.toml`・e2e の新しい歯の file と SCIP と一致の fixture・rules と pipe の外形 snapshot |
| b | 本 repo の宣言（`.vessel.toml` の 2 key・`rust-toolchain.toml` の component・役の規則の file） | a の後・A3 の裁定の後 | `.vessel.toml`・`rust-toolchain.toml`・役の規則の file（`+`） |
| c | 逆引きの表（§6・`pipe index show`）と材料 index.txt（§7 (a)） | a の後 | 行 a の子 module・`crates/scribe2/src/pipe/review.rs`・`crates/scribe2/src/headless/lens.rs`（雛形の 1 文）・e2e の審査の歯の file・headless の外形 snapshot |
| d | 受付の索引の閉包（§7 (b)）と索引の状態の扱い | a の後（c と同じ file を触るなら受付の交差が順を決める）。断りの名を足す閉じた型を touches に持ち、その閉包の file を起票の前に数える | `crates/scribe2/src/pipe/closure.rs`・`crates/scribe2/src/pipe/cli/intake.rs`・表の検査の file・`crates/scribe2/src/pipe/dispatch/candidates.rs`・e2e の受付と起動の列の歯の file |
| e | 欄 `code-facts` の照らしと測り（§7 (c)・表の検査・行の審査・受付・起動の列・裏の組み立ての起こし） | d の後。表の検査の断りの閉じた型を touches に持ち、その閉包の file を起票の前に数える | 表の検査の file・`crates/scribe2/src/pipe/cli/intake.rs`・`crates/scribe2/src/pipe/dispatch/candidates.rs`・`crates/scribe2/src/pipe/dispatch.rs`（裏の起こし）・e2e の歯の file |
| f | runner の stdin の「ほかの行の touches の型」の節（§7 (b) の後半） | pipeline.md の行 bh（共通 verify の節）の後。bh は着地済みで、§15 と本 doc の契約表の行 f に起こした | `crates/scribe2/src/pipe/spawn.rs`・`crates/scribe2/src/headless/runner.rs`・e2e の spawn の歯の file・runner の外形 snapshot |
| g | 外の材料の `.rs` の item と要約の塊を外す（§9） | c と pipeline.md の行 bg（読みの道具）の後 | `crates/scribe2/src/pipe/review/outside.rs`・その子 module・e2e の審査の歯の file |
| h（条件つき） | 試し撃ち（§11） | 分類にその型が出た時だけ | 別の設計で決める |

- 歯（done の項目ごとに 1 本以上・どれも base で RED の理由を書く）:
  - 行 0: contract-source.md §67 の歯（接頭辞 contract_fields_read_only_・欄と key を持つ fixture を読んで断らない・形の違いを名指す）。書き換える pin の歯は base の列に対して RED になり flip する。
  - 行 a（接頭辞 pipe_index_build_・e2e）: 偽の宣言（fixture の SCIP と一致の file を `{out}` と stdout へ写すだけの command）で (1) 1 回目は組み、2 回目は撃たない (2) code の file を 1 つ変えると組み直し、契約表の doc だけを変えると組み直さない (3) 上限を越えると HEAD の鍵と撃ち中の鍵を残して古い順に消す (4) 壊れた SCIP・rc 1・時間切れで失敗の語を記録し表を置かない (5) 箱と受付札の record が残る。base は subcommand が無いので RED。
  - 行 c（接頭辞 pipe_index_show_・e2e）: fixture の小さな crate（別名・`Self` の literal・glob・再輸出・test の module の file・doc の link・文字列の取り込み・別 module の同名の型）から作った SCIP と一致で、7 列の site が期待と一致し、同名の別物を数えず、toml に現れる名を `outside-index` に数える。材料に index.txt が在り、索引の無い周は unavailable の 1 行。base は RED。
  - 行 d（接頭辞 pipe_intake_index_closure_・e2e）: 別名で型を組む file が write-set に無い行を、索引の在る受付が `(索引)` 付きで断り、索引の無い受付と CI の表の検査は通す。撃ち中は index-building で待ち、失敗は index=unavailable で通る。base は RED。
  - 行 e（接頭辞 contract_code_facts_ と pipe_dispatch_code_facts_・e2e）: 形の外れを表の検査が行番号つきで名指し、値の違いを受付が断り、起動の列が code-facts で待ち、索引の無い周は code-facts-unmeasured で待ち、欄の無い行は待たない。base は RED。
  - 行 f: §15 の歯（接頭辞 runner_touches_section_・e2e）。runner の stdin にほかの行の touches の項目と行の pointer の節が在り、自分の行の touches は載らない。base は RED。
  - 行 g（接頭辞 pipe_review_outside_trimmed_・e2e）: outside.txt に item の本文と要約の塊が無く、依存の表・親 module の宣言・data file の鍵の塊は残る。base は塊が在るので RED。

## 13. 限界

- 索引の外: `.rs` 以外の file（toml・nextest の設定・shell）と、文字列を経由する流れ（CLI の verb・rules の行 id・file の path）は辺にならない。表は母集団の `outside-index` で見せるだけで、閉包と事実の欄には入れない。影響範囲（型 g）は候補の上界しか出せない。
- 規則のいくつかは試しの bench の落ちを見てから足した（§3）。行 a〜c の着地の後に新しい問いの bench で測り直し、ADR-0105 の見直しの材料にする。
- 増分が無い: code を変える着地 1 本ごとに全部作り直す。1 つの host の state dir ごとに別に作る。
- 索引の閉包は CI に無い。設計の PR の CI は通り、行の審査と受付で初めて断られる形が残る（行の審査が merge の前に止める）。
- 事実の欄は設計者が写した値で、写さなかった事実は測らない。設計の時点で知らなかった事実は表（(a)）が見せ、欄は知った事実が古びないことを守る。
- rust-analyzer は build script と proc-macro を実行する。gate の検証と同じ信頼で、封じ込めの箱の中で撃つ。
- TypeScript の object literal の key と interface の欄の結び、Python の indexer の実用度は未実測。

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
<!-- contracts:end -->
