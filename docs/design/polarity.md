# 設計: 極性一覧 — 全 guard の極性を型で持ち、外形 snapshot として生成し、CI が in-loop 0 件を落とす

- 要件: [FR20](../../design-intent/spec/srs.html#FR20) write-set guard / [FR26](../../design-intent/spec/srs.html#FR26) cap guard / [AC3](../../design-intent/spec/srs.html#AC3) 偽の PASS を作らない
- 憲法: [C11.2](../../design-intent/spec/constitution.html#c11) 境界ごとの failure enum が極性型を運ぶ・極性一覧を build 時に生成する / [C16.2](../../design-intent/spec/constitution.html#c16) in-loop guard を一覧に載せ、0 件・全 PostHoc を CI が落とす / [C12.5](../../design-intent/spec/constitution.html#c12) 極性一覧は外形 snapshot / [C2](../../design-intent/spec/constitution.html#c2) 閉じた enum・宣言順
- 決定: [ADR-0014](../../design-intent/decisions/ADR-0014-polarity-list-is-a-snapshot-rendered-by-core.html)（一覧は core が描く外形 snapshot・CI の門は xtask がその tracked 生成物を読む）/ [ADR-0013](../../design-intent/decisions/ADR-0013-machine-holds-enumerations-docs-hold-pointers.html) §2.1（列挙は機械が持つ）
- この設計から出る契約: `s2-07l.25`（極性の型・`Guard` 列挙・外形 snapshot・xtask の門）。

## 1. 何を解くか

器は「止める」判断をいくつもの境界で行う（hook の write-set guard・承認の一律 deny・cap guard・intake の断り・spawn の予算・gate・land の main 確認・store の lock・cycle の断り）。各境界は**いつ止めるか**（編集の時点か、後から測るか）と**測れない周にどちらへ倒れるか**（止める側か、通す側か）を持つが、記録時点までその 2 軸は各 module の doc comment（散文）にしか無く、**全数を並べた面が無い**。C11.2 は極性を型で運び一覧を生成せよと言い、C16.2 は「in-loop の guard が 0 件・全部が後追い」の構成を CI が落とせと言う。どちらも受け皿が無かった（`.24` の lens K-7・2026-09-09）。

やさしく言うと: 「止める仕組みが何個あって、それぞれ『その場で止める』のか『後で見つける』のか、『迷ったら止める』のか『迷ったら通す』のか」を、機械が数えて 1 枚の表に出し、その表を test が固定し、「その場で止める仕組みが 1 つも無い」状態を CI が拒む。

## 2. 極性の型（core・`crates/<NAME>/src/polarity.rs`）

- `Timing`（閉じた enum・2 値）: `InLoop`（行為の時点で止める＝C16「edit time」）/ `PostHoc`（行為の後に測って落とす）。
- `OnFailure`（閉じた enum・2 値）: `FailClosed`（測れない・読めない周は**止める側**へ倒す）/ `FailOpen`（測れない周は**通す側**へ倒し、記録を残す）。
- `Polarity { timing, on_failure }`（2 軸の対・`Copy`）。
- **値は境界が持つ**（C11.2「境界ごとの enum が極性型を運ぶ」）: 各 guard の module は自分の判定 enum の隣に `pub const POLARITY: Polarity` を置く。中央の一覧はそれを**集めるだけ**で、値を持たない（2 面化しない）。
- 「guard」の定義: **行為（編集・起動・merge・書込・/clear）を止めうる判定を返す境界**。状態を選ぶだけの判定（tick の inject / noop・Served）は guard ではない。

## 3. `Guard` 列挙（閉じた enum・const slice・網羅 match＝ADR-0013 §2.2 の 4 つ組）

- `pub enum Guard`（宣言順は「行為の流れ」＝hook → intake → spawn → gate → land → store → cycle。順序に意味は無いが C2 の形に合わせて const slice `ALL` と判別子順 pin を持つ・`enum-slices` の measure が集合完全性を測る）。
- 各 variant は `fn polarity(self) -> Polarity`（網羅 match・各境界の `POLARITY` を返す）と `fn boundary(self) -> &'static str`（境界の module path・pointer）を持つ。
- **記録時点の母集団**は本 doc に列挙しない（ADR-0013 §2.1）。現物は `<NAME> polarity` の出力と `polarity.rs`。目安として、記録時点で hook 3・pipe 5・fleet 1・seat 1 の境界を数えた（件数は契約の A/B が実測する）。
- **FailOpen の境界を隠さない**: cap guard は「測れない周は deny しない」（FR26・[seat-autonomy.md §3](./seat-autonomy.md)）と設計で決めた FailOpen である。一覧はそれを FailOpen として**そのまま**出す（極性一覧の目的は全数を可視にすることで、全部を FailClosed に見せることではない）。

## 4. 外形（`<NAME> polarity`・C12.5 の snapshot）

- subcommand `<NAME> polarity`（引数なし・stdin を読まない・env を読まない）。stdout に **1 guard 1 行**、末尾に集計 1 行。
- 行の形: `guard=<kebab> timing=<in-loop|post-hoc> on-failure=<fail-closed|fail-open> boundary=<module::Type>`。token の名前・順序・書式は本節が正で、変える便は ADR-0014 を supersede する。
- 集計行: `polarity: guards=<N> in-loop=<K> post-hoc=<M> fail-open=<F>`（N = K + M）。
- **snapshot が一覧の生成物である**: `tests/e2e/polarity.rs` の外形の歯が `<NAME> polarity` の全出力を insta の snapshot（`e2e__polarity__polarity_external_form.snap`・tracked）に pin する。C11.2「build 時に生成」の充足形は **test 時に core が描き、tracked snapshot として残る**形である（build.rs は持たない・ADR-0014 §4）。参照されない snapshot は C12.5 が拒む（既存の insta 運用）。

## 5. CI の門（C16.2・`cargo xtask check` の measure）

- measure `polarity` は **tracked snapshot file を読む**（xtask は core に依存しない＝ADR-0006 / ADR-0013 の形）。集計行を parse し、`in-loop=<K>/<N>` を判定行に出す。
- 違反: (a) snapshot が無い・集計行を読めない（**fail-closed**＝「読めなかった」を「在る」に化けさせない）(b) `N == 0`（guard 0 件）(c) `K == 0`（全部 PostHoc）。閾値は無い（構造不変条件・`name-literal` / `enum-slices` と同じ型・manifest 行を持たない）。
- snapshot と実装のずれは insta の歯（nextest）が落とし、集計と門は xtask が落とす。2 つの CI job が別の面を受ける。

## 6. 却下案

- **build.rs で一覧を生成**: 第 2 の build 経路と compile 時間（憲法の順位「compile speed」）。core は build script を持たない。
- **xtask が core の source を字面で走査して極性を推定**: 字面から極性は読めない（doc comment は規則ではない・N2）。
- **手書きの表を docs に置く**: N2 / C1.2 / ADR-0013 §2.1。
- **rules manifest に guard ごとの行を置く**: 極性は規則値（閾値・裁定）ではなく型の事実。manifest に写すと 2 面になり drift する。

## 7. 歯（契約 `s2-07l.25`・接頭辞 `polarity_`）

- 外形の歯（snapshot）／`Guard::ALL` の判別子順 pin（既存の述語）／`polarity` の集計が行数と一致する歯／xtask の measure の歯（fixture: snapshot 不在・集計行欠落・`in-loop=0`・`guards=0` の 4 形で違反、正常形で `polarity=K/N`）。
- 各境界の `POLARITY` が実装と一致することは、**既存の境界の歯**（policy 不読 → deny・INCONCLUSIVE ≠ PASS・Unmeasurable ≠ green・cap guard の測れない周 → allow + 記録）が測る。本便は一覧の面を足すのであって、境界の挙動を変えない。

## 8. 後続

- 境界の追加は `Guard` に variant を足し `ALL` に並べ、境界に `POLARITY` を置く（`enum-slices` と網羅 match が入れ忘れを落とす）。
- lens（gate の審査役）の「呼べない → INCONCLUSIVE」も gate の境界に含める（別 variant にするかは契約で決める）。
