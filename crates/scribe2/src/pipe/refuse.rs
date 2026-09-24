//! 契約単位の拒否理由と、write-set の交差判定（設計 docs/design/pipeline-conflict.md §2・
//! ADR-0019 §2.1・FR4 / FR11・NFR4）。
//!
//! **主語は「契約 file が読めた後の契約単位の判定」**である。引数の不足（rc 1）と
//! [`super::contract::Contract::load`] の error（rc 2）は従来の口のまま外に在る——入力の型が
//! 違うものを 1 つの enum に集めると、極性一覧の 1 行が 2 種の判定を背負う。
//!
//! [`Unfit`](super::declaration) は **verify 行 1 本**の理由で境界が違うので触らない。
//!
//! 交差の判定は**契約の字面と base の tracked file の一覧**で閉じる: 正規化して項目ごとに突き合わせ、
//! dir 項目は base の file 一覧に**展開してから**数える（設計 contract-source.md §3・新規 file〔`+`〕と
//! dir は交差しない＝dir で書いた snapshot の置き場が配下 1 file の別便と偽の交差を起こさない）。symlink は
//! 解かない。実体が同じ file を別名で持つ 2 契約は入口で見逃す（偽陰性）が、編集時の guard が実体名で塞ぐ
//! （ADR-0009 §2.1 の既知の穴はそのまま）。

use super::closure::ClosureError;
use super::review::{FindingKind, ROW_SAME_KIND_STOP};
use super::table::TableError;
use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
use crate::polarity::{OnFailure, Polarity, Timing};

/// この境界の極性: intake（便を起こす前）で止め、live な便の write-set を読めない周は
/// 断る側へ倒す（読めない store は rc 2・NFR4）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// [`Refuse`] の全 variant の名（**宣言順**・判別子順の pin が読む）。
///
/// payload 付きの enum は `as` で判別子へ写せないので、`&[Refuse]` の const slice
/// （`enum-slices` が測る形）は置けない。名前の slice + [`Refuse::as_str`] の網羅 match を
/// 対にして宣言順を pin する（ADR-0013 §2.1 の形・限界は「末尾の入れ忘れ」を機械が
/// 捕まえないことで、それは `Unfit` / `Guard` と同じ）。
///
/// 読むのは pin の歯だけで、runtime に消費する口は作らない（外形を増やさない）。それでも
/// src に置くのは、base に test 区間だけを写した木で compile error＝flip-check の RED を
/// 構造で作るためである（`xtask::check::shape` と同じ形）。
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "判別子順 pin の歯だけが読む（src 配置で flip-check の RED を作る）")
)]
pub(crate) const REFUSALS: &[&str] = &[
    "not-a-repo",
    "duplicate-run",
    "write-set-overlap",
    "write-set-unreadable",
    "write-set-incomplete",
    "write-set-dir-without-slash",
    "contract-table",
    "write-set-item-unresolved",
    "cap-headroom",
    "name-unresolved",
    "write-set-drift",
    "teeth-place-unresolved",
    "also-names-rust",
    "tests-not-a-teeth-file",
    "fn-undeclared",
    "teeth-outside-write-set",
    "hand-written-contract",
    "same-kind-repeated",
    "finding-unaddressed",
    "promised-field-written",
    "promise-symbol-unresolved",
    "max-live",
    "entrance-not-red",
];

/// 契約 file が読めた後の、契約単位の拒否理由。**新しい理由は variant を 1 つ足す**（憲法 C2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refuse {
    /// 対象が git repo でない。
    NotARepo {
        /// 対象 repo の path の字面。
        repo: String,
    },
    /// 同じ秒に同じ bead を再 intake した（run id が衝突する）。
    DuplicateRun {
        /// 衝突した run id。
        run: String,
    },
    /// live な便の write-set と交差する（**先頭の 1 組**を持つ・全組は stderr に並ぶ）。
    WriteSetOverlap {
        /// 交差した相手の run id。
        run: String,
        /// 交差した契約側の path（字面は契約が書いたまま）。
        path: String,
    },
    /// live な便の write-set を読めない（契約の写しが無い / 壊れている / 判定を読めない）。
    WriteSetUnreadable {
        /// 読めなかった run id。
        run: String,
    },
    /// 契約表の行の `touches` の閉包（[`super::closure`]）が write-set に含まれない（設計 contract-source.md §3・
    /// FR48）。**足りない file を全部**持つ。
    WriteSetIncomplete {
        /// write-set に無い閉包の file（repo 相対・辞書順）。
        missing: Vec<String>,
    },
    /// write-set の項目が末尾 `/` 無しで既存の dir を指す（guard は末尾 `/` 無しを字面一致でしか通さず、runner が
    /// 配下を書けない・設計 contract-source.md §9）。
    WriteSetDirWithoutSlash {
        /// 契約が書いた項目の字面。
        path: String,
    },
    /// 契約表そのものの欠陥（区間・parse・id・section・req・verify・depends・設計 contract-source.md §2）。
    ContractTable(TableError),
    /// write-set の項目が base に解けない（実在する file でも・末尾 `/` の dir でも・`+` 接頭辞の新規 file〔base に
    /// 無い〕でも・`-` 接頭辞の縮む file〔base に在る〕でもない・設計 contract-source.md §3「項目の実在と展開」）。
    WriteSetItemUnresolved {
        /// 契約が書いた項目の字面。
        item: String,
    },
    /// 上限の余地（R-C4-2 / R-C4-1 の値と base の行数の差）を file の見込み（行の `growth` に在ればその値・無ければ
    /// `size` の見積・§46）が超える（設計 contract-source.md §3「上限の余地」・受付だけが撃つ）。core の合計の周は
    /// `file` = `core`（見込みは core に属する file の見込みの和）。
    CapHeadroom {
        /// 余地の足りない file（repo 相対・core の合計は `core`）。
        file: String,
        /// 残っている行数。
        headroom: u64,
        /// 契約の `size` の字面。
        size: String,
        /// その file の見込み（行）。
        estimate: u64,
    },
    /// `title` / `done` / 節の本文の名指し（backtick の中身・path 形 / 型の path 形 / fn 形）が base に解けない
    /// （設計 contract-source.md §3「名指しの実在」・FR54 と同型）。
    NameUnresolved {
        /// 名指しの字面。
        name: String,
        /// どこに書かれていたか（`title` / `done` / `section <N> line <L>`）。
        at: String,
    },
    /// 契約表の行の手書きの write-set が導出値（[`super::closure::derive_write_set`]）と集合として一致しない
    /// （設計 contract-source.md §3「手書きの write-set の扱い」・受付だけが撃つ）。**不足も余分も全部**持つ。
    WriteSetDrift {
        /// 導出値に在って手書きに無い項目。
        missing: Vec<String>,
        /// 手書きに在って導出値に無い項目。
        extra: Vec<String>,
    },
    /// verify の filter 語を含む `#[test]` の fn が base に無く、行の `tests` 欄も無い（歯の置き場を解けない・§3
    /// 「write-set の導出」(ii)）。
    TeethPlaceUnresolved {
        /// 解けなかった filter 語。
        filter: String,
    },
    /// 行の `also` に `.rs` が書かれた（Rust の面は `touches` と `tests` から導く・§3 (v)）。
    AlsoNamesRust {
        /// 書かれていた項目。
        item: String,
    },
    /// 行の `tests` の項目が歯の file（`tests/` 配下か test 区間を持つ `.rs`）でない（§3「限界」）。
    TestsNotATeethFile {
        /// 書かれていた項目。
        item: String,
    },
    /// 行の fn 形の `touches`（`crate::<module>::<snake_ident>`）の fn を宣言する file が base に無い（§18・行 r・
    /// 導出値を空集合に潰さない）。
    FnUndeclared {
        /// module の段の字面（`pipe::cli`・`crate` 直下は `crate`）。
        module: String,
        /// fn の名。
        name: String,
    },
    /// Declared 行（新欄を持たず `write-set` を持つ行）の verify の歯の file（base の `#[test]` の fn 名が filter 語を
    /// 含む file）が行の write-set に無い（§20・行 t・受付だけが撃つ）。**足りない file を全部**持つ。
    TeethOutsideWriteSet {
        /// write-set に無い歯の file（repo 相対・辞書順）と、その file を解いた verify 行の filter 語の対（§41）。
        files: Vec<(String, String)>,
    },
    /// 手書きの契約 file を渡された（`--contract`・契約 (b)・FR54）。契約の正本は設計 doc の行だけで、
    /// 器が base の行から写しを作る＝手で書いた file は**使い方の誤りでなく typed な断り**である。
    HandWrittenContract {
        /// 渡された path の字面。
        path: String,
    },
    /// 同じ bead の直前の便から同じ理由の型（[`FindingKind`]）の審査 FAIL が rules 行 `review.same_kind_stop` の
    /// 本数続き、契約 file と節の本文がともに不変のまま run N+1 を求めた（設計 contract-source.md §23 (2)・受付だけが
    /// 撃つ）。**焼き直しは書き直す**＝契約か節のどちらかが変わっていれば通る。
    SameKindRepeated {
        /// 続いた理由の型。
        kind: FindingKind,
        /// 数えた便 id の列（新しい順）。
        runs: Vec<String>,
        /// rules 行の値（本）。
        stop: u64,
    },
    /// 同じ bead の直前の便の指摘（`review.json` の `kind` と `at`）に対応する差分が今回の材料に無い（§23 (3)・受付だけが
    /// 撃つ）。**対応の無かった項目だけ**（辞書順）を持つ。測れない型（goal-done-contradiction / vacuous-assert / other /
    /// unparsed）と `at` の空な周はこの断りに届かない。
    FindingUnaddressed {
        /// 直前の便の理由の型。
        kind: FindingKind,
        /// 対応の無かった `at` の項目（辞書順）。
        at: Vec<String>,
    },
    /// 約束の行を持つ行（Promised・設計 contract-source.md §33）が、約束の行から器が導く欄（`write-set` / `touches` /
    /// `surfaces` / `tests` / `also` / `creates` / `done`）を手で書いた（受付だけが撃つ）。**書かれた欄を全部**持つ。
    PromisedFieldWritten {
        /// 行 id。
        row: String,
        /// 書かれていた欄の名（欄の宣言順）。
        fields: Vec<String>,
    },
    /// 約束の行の `symbols` の名が base と合わない（§33 項 5・受付だけが撃つ）: `+` 無しの名が base に解けない、か
    /// `+` 付きの名（新設の宣言）が base に既に在る。名は書かれた字面のまま（`+` の有無が極性を名乗る）。
    PromiseSymbolUnresolved {
        /// 親の行 id。
        of: String,
        /// 約束の行の番号。
        n: u64,
        /// 名の字面。
        name: String,
    },
    /// 置き場の live な便の本数が rules 行 `pipe.max_live` の値以上（設計 gate-cost.md §24・受付だけが撃つ・走行中の便は
    /// 止めない）。数え方は交差と同じ live の判定。
    MaxLive {
        /// 数えた live な便の本数。
        live: u64,
        /// rules 行の値（本）。
        cap: u64,
    },
    /// 宣言が `entrance-flip = "deny"` を名乗り、契約の nextest の検証行を base の木で撃った結果に base で緑か測れない行が
    /// 1 本以上在る（設計 pipeline.md §56 形 6・ADR-0059・受付だけが撃つ）。
    EntranceNotRed {
        /// base で緑か測れない行の本数。
        count: u64,
        /// 契約の検証行ごとの 4 値の語（検証行の順）。
        values: Vec<String>,
    },
}

impl Refuse {
    /// 一覧と pin が読む名（kebab・宣言順は [`REFUSALS`]）。
    pub(crate) fn as_str(&self) -> &'static str {
        match *self {
            Self::NotARepo { .. } => "not-a-repo",
            Self::DuplicateRun { .. } => "duplicate-run",
            Self::WriteSetOverlap { .. } => "write-set-overlap",
            Self::WriteSetUnreadable { .. } => "write-set-unreadable",
            Self::WriteSetIncomplete { .. } => "write-set-incomplete",
            Self::WriteSetDirWithoutSlash { .. } => "write-set-dir-without-slash",
            Self::ContractTable(_) => "contract-table",
            Self::WriteSetItemUnresolved { .. } => "write-set-item-unresolved",
            Self::CapHeadroom { .. } => "cap-headroom",
            Self::NameUnresolved { .. } => "name-unresolved",
            Self::WriteSetDrift { .. } => "write-set-drift",
            Self::TeethPlaceUnresolved { .. } => "teeth-place-unresolved",
            Self::AlsoNamesRust { .. } => "also-names-rust",
            Self::TestsNotATeethFile { .. } => "tests-not-a-teeth-file",
            Self::FnUndeclared { .. } => "fn-undeclared",
            Self::TeethOutsideWriteSet { .. } => "teeth-outside-write-set",
            Self::HandWrittenContract { .. } => "hand-written-contract",
            Self::SameKindRepeated { .. } => "same-kind-repeated",
            Self::FindingUnaddressed { .. } => "finding-unaddressed",
            Self::PromisedFieldWritten { .. } => "promised-field-written",
            Self::PromiseSymbolUnresolved { .. } => "promise-symbol-unresolved",
            Self::MaxLive { .. } => "max-live",
            Self::EntranceNotRed { .. } => "entrance-not-red",
        }
    }

    /// findings の行の見出し（契約表の欠陥は `contract-table:<理由の名>`・他は [`Self::as_str`]）。
    pub(crate) fn label(&self) -> String {
        match *self {
            Self::ContractTable(ref found) => format!("{}:{}", self.as_str(), found.as_str()),
            _ => self.as_str().to_owned(),
        }
    }

    /// 断る理由の 1 行（run id と path を名乗る）。
    pub(crate) fn reason(&self) -> String {
        match *self {
            Self::NotARepo { ref repo } => format!("{repo} は git repo でない"),
            Self::DuplicateRun { ref run } => format!("run {run} は既に在る（同じ秒の再 intake）"),
            Self::WriteSetOverlap { ref run, ref path } => {
                format!("write-set が live な run {run} と交差する（{path}）")
            }
            Self::WriteSetUnreadable { ref run } => {
                format!("live な run {run} の write-set を読めない")
            }
            Self::WriteSetIncomplete { ref missing } => {
                format!("touches の閉包の file が write-set に無い（{}）", missing.join(", "))
            }
            Self::WriteSetDirWithoutSlash { ref path } => {
                format!("write-set の {path} は既存の dir を末尾 / 無しで指す（配下を書くなら {path}/）")
            }
            Self::ContractTable(ref found) => found.reason(),
            Self::WriteSetItemUnresolved { ref item } => {
                format!("write-set の {item} は base に解けない（実在する file・末尾 / の dir・+ 接頭辞の新規 file・- 接頭辞の縮む file のどれでもない）")
            }
            Self::CapHeadroom { ref file, headroom, ref size, estimate } => {
                format!("{file} の上限の余地が {headroom} 行で見込み {estimate} 行（size {size}・growth で上書き可）に足りない")
            }
            Self::NameUnresolved { ref name, ref at } => format!("名指し {name} が base に無い（{at}）"),
            // 8 理由の字面は導出の側（`ClosureError`）と同じ 1 本（受付が写すだけ・2 面に書かない）。
            Self::WriteSetDrift { ref missing, ref extra } => {
                ClosureError::WriteSetDrift { missing: missing.clone(), extra: extra.clone() }.reason()
            }
            Self::TeethPlaceUnresolved { ref filter } => ClosureError::TeethPlaceUnresolved { filter: filter.clone() }.reason(),
            Self::AlsoNamesRust { ref item } => ClosureError::AlsoNamesRust { item: item.clone() }.reason(),
            Self::TestsNotATeethFile { ref item } => ClosureError::TestsNotATeethFile { item: item.clone() }.reason(),
            Self::FnUndeclared { ref module, ref name } => {
                ClosureError::FnUndeclared { module: module.clone(), name: name.clone() }.reason()
            }
            Self::TeethOutsideWriteSet { ref files } => ClosureError::TeethOutsideWriteSet { files: files.clone() }.reason(),
            Self::HandWrittenContract { ref path } => {
                format!("手書きの契約 file は受け付けない（{path}）＝契約の正本は設計 doc の行で、--design <doc>#<id> を渡す")
            }
            Self::SameKindRepeated { kind, ref runs, stop } => format!(
                "審査 FAIL の型 {} が {} 便続き {ROW_SAME_KIND_STOP} の {stop} に達した（run {}）のに契約 file と節の本文がともに不変＝焼き直しは書き直す",
                kind.as_str(),
                runs.len(),
                runs.join(", ")
            ),
            Self::FindingUnaddressed { kind, ref at } => {
                format!("直前の便の審査の指摘（{}）に対応する差分が無い（{}）", kind.as_str(), at.join(", "))
            }
            Self::PromisedFieldWritten { ref row, ref fields } => format!(
                "行 {row} は約束の行を持つ（Promised）ので {} を書けない（write-set / touches / surfaces / tests / also / creates / done は約束の行から導く）",
                fields.join(", ")
            ),
            Self::PromiseSymbolUnresolved { ref of, n, ref name } => match name.strip_prefix(NEW_FILE) {
                Some(fresh) => format!("約束 {of} の n {n} の symbols の {name} は base に既に在る（+ は base に無い新設の名・{fresh} は + を外す）"),
                None => format!("約束 {of} の n {n} の symbols の {name} が base に無い（新設の名なら + を前置する）"),
            },
            // stderr の 1 行は `pipe: max-live live=<n> cap=<c>`（設計 gate-cost.md §24・名と 2 値だけ）。
            Self::MaxLive { live, cap } => format!("{} live={live} cap={cap}", self.as_str()),
            Self::EntranceNotRed { count, ref values } => {
                format!("{} base で緑か測れない契約の検証行が {count} 本在る（deny の名乗り・行ごと {}）", self.as_str(), values.join(","))
            }
        }
    }

    /// **rc は variant が持つ**。読めない周だけが「壊れた store」の rc 2 で、
    /// 残りは前提違反の rc 1 である（NFR4）。契約表の欠陥は理由の側が持つ（読めない表だけ rc 2）。
    pub(crate) fn rc(&self) -> u8 {
        match *self {
            Self::NotARepo { .. }
            | Self::DuplicateRun { .. }
            | Self::WriteSetOverlap { .. }
            | Self::WriteSetIncomplete { .. }
            | Self::WriteSetDirWithoutSlash { .. }
            | Self::WriteSetItemUnresolved { .. }
            | Self::CapHeadroom { .. }
            | Self::NameUnresolved { .. }
            | Self::WriteSetDrift { .. }
            | Self::TeethPlaceUnresolved { .. }
            | Self::AlsoNamesRust { .. }
            | Self::TestsNotATeethFile { .. }
            | Self::FnUndeclared { .. }
            | Self::TeethOutsideWriteSet { .. }
            | Self::HandWrittenContract { .. }
            | Self::SameKindRepeated { .. }
            | Self::FindingUnaddressed { .. }
            | Self::PromisedFieldWritten { .. }
            | Self::PromiseSymbolUnresolved { .. }
            | Self::MaxLive { .. }
            | Self::EntranceNotRed { .. } => RC_REFUSED,
            Self::WriteSetUnreadable { .. } => RC_BROKEN,
            Self::ContractTable(ref found) => found.rc(),
        }
    }
}

/// `path` が write-set のどれかに含まれるか（dir は配下全部・file は字面の一致・照合は正規化した形）。
///
/// 契約表の閉包 ⊆ write-set の判定（設計 contract-source.md §3）が使う。畳み方は [`overlaps`] と同じ 1 本である。
pub(crate) fn covered(write_set: &[String], path: &str) -> bool {
    let target = normalize(path);
    write_set.iter().any(|item| covers(&normalize(item), &target))
}

/// 2 つの write-set が交差した**全組**（`(左の字面, 右の字面)`・先頭が理由の 1 行に載る）。
///
/// **照合は正規化した形で、返すのは契約が書いた字面のまま**である（読み手が自分の契約の
/// どの行を直せばよいかは、器が畳んだ形ではなく書いた字面でしか分からない）。
///
/// dir 項目は `tracked`（base の tracked file の repo 相対 path）に**展開してから**数える（設計
/// contract-source.md §3）: dir × dir は段の境目の prefix、dir × file はその file が base に在って配下の周だけ、
/// file × file は字面の一致（`+` 接頭辞の新規 file は剥がして比べる）。base に無い file と dir は交差しない
/// （dir で書いた snapshot の置き場が、配下に新規 file 1 つを持つ別便と偽の交差を起こした `s2-07l.243 × .248` の型）。
///
/// `pub(crate)` なのは、回答で write-set を広げる周（`.133`）が**同じ 1 本**を呼ぶためである
/// （本便は口だけを置き、answer への配線は `.133`）。
pub(crate) fn overlaps(left: &[String], right: &[String], tracked: &[String]) -> Vec<(String, String)> {
    let base: Vec<String> = tracked.iter().map(|path| normalize(path)).collect();
    let mut found = Vec::new();
    for mine in left {
        for theirs in right {
            if touches(&normalize(mine), &normalize(theirs), &base) {
                found.push((mine.clone(), theirs.clone()));
            }
        }
    }
    found
}

/// 新規 file の項目の接頭辞（設計 contract-source.md §3「項目の実在と展開」）。
pub(crate) const NEW_FILE: char = '+';

/// 縮む面の項目の接頭辞（設計 contract-source.md §3「項目の実在と展開」・base に在る file を減らす便が宣言する）。
/// 受付の宣言だけの文法で、guard と交差の照合は素の path で持つ。
pub(crate) const SHRINK_FILE: char = '-';

/// 着地で消える file の項目の接頭辞（設計 contract-source.md §24・受付は base に**実在する** file を要し、契約表の
/// 検査は tracked に無ければ着地で消えたと読む）。[`SHRINK_FILE`] と同じく受付の宣言だけの文法で、guard と交差の
/// 照合と gate の write-set 照合は素の path で持つ（消す file は触る file）。
pub(crate) const DELETE_FILE: char = '~';

/// 中身を変えない・verify の置き場として載せただけの項目の接頭辞（設計 contract-source.md §43 (1)・行 ar・base に
/// **実在する** file を要する）。[`SHRINK_FILE`] と同じく受付の宣言だけの文法で、上限の余地も core の見積の本数も
/// 求めない。guard と交差の照合と gate の write-set 照合は素の path で持つ。
pub(crate) const PLACE_ONLY_FILE: char = '=';

/// path 1 本を字面で畳む（write-set guard の `relative_to` と同じ規則）。
///
/// 先頭の `./` を落とす・連続する `/` を 1 つにする・`..` を畳む・**末尾の `/` は dir の印
/// として残す**・接頭辞（新規 file の `+`・縮む面の `-`・消える file の `~`・置き場だけの `=`）は剥がす。root の外へ出る `..`
/// （畳めない分）はそのまま残す
/// ＝字面が違うものを同じ path に化けさせない。**存在は見ない**ので、まだ無い file を書く契約も同じ規則で測れる。
///
/// `pub(crate)` なのは、spawn が guard へ写す policy（`spawn::write_policy`）が**同じ 1 本**で接頭辞を剥がすため
/// である（剥がす規則を 2 か所に持たない）。
pub(crate) fn normalize(raw: &str) -> String {
    let raw = raw.strip_prefix([NEW_FILE, SHRINK_FILE, DELETE_FILE, PLACE_ONLY_FILE]).unwrap_or(raw);
    let is_dir = raw.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.truncate(parts.len().saturating_sub(1));
            }
            name => parts.push(name),
        }
    }
    let joined = parts.join("/");
    if is_dir && !joined.is_empty() {
        format!("{joined}/")
    } else {
        joined
    }
}

/// 正規化した 2 本が交差するか（**対称**）。dir × dir は段の境目の prefix・dir × file は base に在る配下の file
/// だけ・file × file は字面の一致（`base` は正規化済みの tracked path の列）。
fn touches(left: &str, right: &str, base: &[String]) -> bool {
    match (left.ends_with('/'), right.ends_with('/')) {
        (true, true) => covers(left, right) || covers(right, left),
        (true, false) => in_base(right, base) && covers(left, right),
        (false, true) => in_base(left, base) && covers(right, left),
        (false, false) => left == right,
    }
}

/// 正規化した file が base の tracked file に在るか。
fn in_base(file: &str, base: &[String]) -> bool {
    base.iter().any(|path| path == file)
}

/// `left` が `right` を含むか。dir（末尾 `/`）は配下を全部含み、file は字面の一致だけ。
///
/// `a/` は `a/b.rs` と `a` を含み、`ab/` は含まない（prefix の比較を段の境目で切る）。
fn covers(left: &str, right: &str) -> bool {
    match left.strip_suffix('/') {
        Some(dir) => {
            right == dir || right.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
        }
        None => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::{covered, normalize, overlaps, ClosureError, FindingKind, Refuse, REFUSALS};
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
    use crate::pipe::table::TableError;
    use proptest::prelude::*;
    use proptest::test_runner::Config;

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// 宣言順に 1 つずつ組んだ全 variant（payload は測らないので固定値）。
    fn samples() -> Vec<Refuse> {
        vec![
            Refuse::NotARepo { repo: "/tmp/x".to_owned() },
            Refuse::DuplicateRun { run: "r-1".to_owned() },
            Refuse::WriteSetOverlap { run: "r-1".to_owned(), path: "src/lib.rs".to_owned() },
            Refuse::WriteSetUnreadable { run: "r-1".to_owned() },
            Refuse::WriteSetIncomplete { missing: vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()] },
            Refuse::WriteSetDirWithoutSlash { path: "src".to_owned() },
            Refuse::ContractTable(TableError::SectionMissing { line: 3, section: "9".to_owned() }),
            Refuse::WriteSetItemUnresolved { item: "src/none.rs".to_owned() },
            Refuse::CapHeadroom { file: "src/big.rs".to_owned(), headroom: 7, size: "M".to_owned(), estimate: 30 },
            Refuse::NameUnresolved { name: "Guard::Rules".to_owned(), at: "done".to_owned() },
            Refuse::WriteSetDrift { missing: vec!["src/a.rs".to_owned()], extra: vec!["docs/x.md".to_owned()] },
            Refuse::TeethPlaceUnresolved { filter: "fresh_".to_owned() },
            Refuse::AlsoNamesRust { item: "src/a.rs".to_owned() },
            Refuse::TestsNotATeethFile { item: "src/a.rs".to_owned() },
            Refuse::FnUndeclared { module: "pipe::cli".to_owned(), name: "missing".to_owned() },
            Refuse::TeethOutsideWriteSet {
                files: vec![("src/a.rs".to_owned(), "alpha_".to_owned()), ("tests/b.rs".to_owned(), "beta_".to_owned())],
            },
            Refuse::HandWrittenContract { path: "contract.toml".to_owned() },
            Refuse::SameKindRepeated {
                kind: FindingKind::LiteralMismatch,
                runs: vec!["b-2".to_owned(), "b-1".to_owned()],
                stop: 2,
            },
            Refuse::FindingUnaddressed { kind: FindingKind::TeethOutsideWriteSet, at: vec!["src/a.rs".to_owned()] },
            Refuse::PromisedFieldWritten { row: "ag".to_owned(), fields: vec!["write-set".to_owned(), "done".to_owned()] },
            Refuse::PromiseSymbolUnresolved { of: "ag".to_owned(), n: 2, name: "+Refuse::Fresh".to_owned() },
            Refuse::MaxLive { live: 3, cap: 2 },
            Refuse::EntranceNotRed { count: 1, values: vec!["green-on-base".to_owned(), "absent".to_owned()] },
        ]
    }

    /// 受付の 2 門（設計 contract-source.md §23・`s2-07l.396`）: 宣言順の 18〜19 番目（約束の行の 2 理由の手前）・rc 1 で、
    /// 同型の停止は型と本数と行の値と便 id の列（新しい順）を、焼き直しは型と対応の無かった項目を名乗る。
    #[test]
    fn refuse_repeat_reasons_are_last_and_name_kind_runs_and_value() {
        let found = samples();
        let pair: Vec<&str> = found.iter().skip(17).take(2).map(Refuse::as_str).collect();
        assert_eq!(pair, ["same-kind-repeated", "finding-unaddressed"], "宣言順の 18〜19 番目");
        let repeated = found.get(17).map(Refuse::reason).unwrap_or_default();
        for want in ["literal-mismatch", " 2 便", "review.same_kind_stop の 2", "b-2, b-1"] {
            assert!(repeated.contains(want), "{want}: {repeated}");
        }
        let unaddressed = found.get(18).map(Refuse::reason).unwrap_or_default();
        assert!(unaddressed.contains("teeth-outside-write-set") && unaddressed.contains("src/a.rs"), "{unaddressed}");
        assert!(found.iter().skip(17).take(2).all(|refuse| refuse.rc() == RC_REFUSED && !refuse.reason().contains('\n')), "rc 1・1 行");
    }

    /// write-set の導出の 6 理由（契約 (h)・設計 contract-source.md §3「write-set の導出」・§18 の fn 形・§20 の Declared
    /// 行の門）: 宣言順の末尾に並び、rc 1 で、理由は不足と余分 / filter 語 / 項目 / module の段と fn の名 /
    /// write-set に無い歯の file の全部を名乗る（字面は導出の側と同じ 1 本）。
    #[test]
    fn refuse_derive_reasons_are_last_and_name_their_payload() {
        let found = samples();
        // 契約 (b) の `hand-written-contract` は導出の理由ではないので、末尾の 6 つはその手前に並ぶ。
        let tail: Vec<&str> = found.iter().skip(10).take(6).map(Refuse::as_str).collect();
        assert_eq!(
            tail,
            [
                "write-set-drift",
                "teeth-place-unresolved",
                "also-names-rust",
                "tests-not-a-teeth-file",
                "fn-undeclared",
                "teeth-outside-write-set"
            ],
            "宣言順の 11〜16 番目"
        );
        let reasons: Vec<String> = found.iter().skip(10).take(6).map(Refuse::reason).collect();
        assert!(reasons.first().is_some_and(|line| line.contains("missing: src/a.rs") && line.contains("extra: docs/x.md")), "{reasons:?}");
        assert!(reasons.get(1).is_some_and(|line| line.contains("fresh_") && line.contains("tests")), "{reasons:?}");
        assert!(reasons.get(2).is_some_and(|line| line.contains("also の src/a.rs")), "{reasons:?}");
        assert!(reasons.get(3).is_some_and(|line| line.contains("tests の src/a.rs")), "{reasons:?}");
        assert!(
            reasons.get(4).is_some_and(|line| line.contains("pipe::cli::missing を宣言する file が base に無い")),
            "{reasons:?}"
        );
        assert!(
            reasons.get(5).is_some_and(|line| line.contains("歯の file が write-set に無い")
                && line.contains("src/a.rs ← filter 語 alpha_, tests/b.rs ← filter 語 beta_")),
            "{reasons:?}"
        );
        assert!(found.iter().skip(10).all(|refuse| refuse.rc() == RC_REFUSED && !refuse.reason().contains('\n')), "rc 1・1 行");
    }

    /// §41: 受付の側の `teeth-outside-write-set` の理由の 1 行が file と filter 語の両方を名乗り、導出の側の 1 本と同じ字面・
    /// 語と rc は不変。
    #[test]
    fn contract_teeth_origin_refuse_reason_names_file_and_filter() {
        let files = vec![("tests/b.rs".to_owned(), "beta_".to_owned())];
        let refuse = Refuse::TeethOutsideWriteSet { files: files.clone() };
        let reason = refuse.reason();
        assert!(reason.contains("tests/b.rs") && reason.contains("beta_"), "{reason}");
        assert_eq!(reason, ClosureError::TeethOutsideWriteSet { files }.reason(), "導出の側の 1 本を写す");
        assert_eq!((refuse.as_str(), refuse.rc()), ("teeth-outside-write-set", RC_REFUSED), "語と rc は不変");
    }

    /// 契約表の 3 理由（`s2-07l.208`・設計 contract-source.md §2 / §3）: 見出しは契約表の欠陥だけ理由の名を足し、
    /// 閉包の不足は**足りない file を全部**名乗る。読めない表だけ rc 2（NFR4）。
    #[test]
    fn refuse_contract_table_reasons_name_every_missing_file_and_keep_the_rc_of_the_table_error() {
        let found = samples();
        let labels: Vec<String> = found.iter().skip(4).take(3).map(Refuse::label).collect();
        assert_eq!(labels, ["write-set-incomplete", "write-set-dir-without-slash", "contract-table:section-missing"]);
        let incomplete = found.get(4).map(Refuse::reason).unwrap_or_default();
        assert!(incomplete.contains("src/a.rs, src/b.rs"), "足りない file を全部名乗る: {incomplete}");
        let unreadable = Refuse::ContractTable(TableError::Unreadable { line: 0, reason: "x を読めない".to_owned() });
        assert_eq!(unreadable.rc(), RC_BROKEN, "読めない表は rc 2");
        assert_eq!(unreadable.reason(), "x を読めない", "理由は表の欠陥の字面のまま");
    }

    /// 閉包の拡張の 3 理由（契約 (g)・設計 contract-source.md §3）: 導出の 4 理由の前に並び、rc 1 で、理由は項目 / file
    /// と余地と size / 名指しと在り処を名乗る。
    #[test]
    fn refuse_closure_ext_reasons_are_last_and_name_their_payload() {
        let found = samples();
        let tail: Vec<&str> = found.iter().skip(7).take(3).map(Refuse::as_str).collect();
        assert_eq!(tail, ["write-set-item-unresolved", "cap-headroom", "name-unresolved"], "(g) の 3 つ");
        let reasons: Vec<String> = found.iter().skip(7).take(3).map(Refuse::reason).collect();
        assert!(reasons.first().is_some_and(|line| line.contains("src/none.rs")), "項目を名乗る: {reasons:?}");
        let headroom = reasons.get(1).cloned().unwrap_or_default();
        assert!(headroom.contains("src/big.rs") && headroom.contains(" 7 ") && headroom.contains("size M"), "{headroom}");
        assert!(headroom.contains("見込み 30 行"), "file の見込みの値も名乗る（§46）: {headroom}");
        assert!(reasons.get(2).is_some_and(|line| line.contains("Guard::Rules") && line.contains("done")), "{reasons:?}");
        assert!(found.iter().skip(7).take(3).all(|refuse| refuse.rc() == RC_REFUSED), "前提違反は rc 1");
    }

    /// 閉包の file が write-set に含まれるか: dir（末尾 `/`）は配下全部・file は字面の一致・正規化してから比べる。
    #[test]
    fn refuse_covered_follows_the_overlap_folding() {
        let set = vec!["src/".to_owned(), "docs/a.md".to_owned()];
        for (path, want) in [("src/x.rs", true), ("src/deep/y.rs", true), ("./docs/a.md", true), ("docs/b.md", false), ("srcx/z.rs", false)] {
            assert_eq!(covered(&set, path), want, "{path}");
        }
        assert!(!covered(&["src".to_owned()], "src/x.rs"), "末尾 / 無しは dir として配下を含まない");
    }

    /// 名前の slice は **宣言順**で、`as_str` の網羅 match と 1 対 1 である（ADR-0013 §2.1）。
    #[test]
    fn refuse_names_are_pinned_in_declaration_order() {
        let names: Vec<&str> = samples().iter().map(Refuse::as_str).collect();
        assert_eq!(names, REFUSALS, "名前の slice は宣言順（母集団 {} 値）", REFUSALS.len());
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "名は一意: {names:?}");
        // 末尾 2 つ（本便が足した理由）は交差と読めなさである。
        assert_eq!(names.get(2).copied(), Some("write-set-overlap"), "{names:?}");
        assert_eq!(names.get(3).copied(), Some("write-set-unreadable"), "{names:?}");
        // 約束の行の 2 理由（設計 contract-source.md §33・`s2-07l.512`）が同時本数の上限（gate-cost.md §24・`s2-07l.398`）の
        // 手前に並び、base の木で撃った入口の断り（pipeline.md §56・`s2-07l.557`）が末尾で、母集団は 23 値。
        assert_eq!(REFUSALS.len(), 23, "母集団 23 値");
        assert_eq!(
            names.iter().rev().take(4).copied().collect::<Vec<&str>>(),
            ["entrance-not-red", "max-live", "promise-symbol-unresolved", "promised-field-written"]
        );
        let entrance = samples().get(22).map(|found| (found.reason(), found.rc()));
        let want = "entrance-not-red base で緑か測れない契約の検証行が 1 本在る（deny の名乗り・行ごと green-on-base,absent）".to_owned();
        assert_eq!(entrance, Some((want, RC_REFUSED)), "本数と行ごとの 4 値を名乗る 1 行・rc 1");
        let cap = samples().get(21).map(|found| (found.reason(), found.rc()));
        assert_eq!(cap, Some(("max-live live=3 cap=2".to_owned(), RC_REFUSED)), "名と live / cap の 2 値だけの 1 行・rc 1");
        let found = samples();
        let written = found.get(19).map(Refuse::reason).unwrap_or_default();
        assert!(written.contains("行 ag") && written.contains("write-set, done"), "行 id と欄を全部名乗る: {written}");
        let fresh = found.get(20).map(Refuse::reason).unwrap_or_default();
        assert!(fresh.contains("約束 ag の n 2") && fresh.contains("+Refuse::Fresh は base に既に在る"), "{fresh}");
        let bare = Refuse::PromiseSymbolUnresolved { of: "ag".to_owned(), n: 1, name: "Nope".to_owned() }.reason();
        assert!(bare.contains("Nope が base に無い"), "+ 無しは不在を名乗る: {bare}");
        assert!(found.iter().skip(19).all(|refuse| refuse.rc() == RC_REFUSED && !refuse.reason().contains('\n')), "rc 1・1 行");
    }

    /// **rc は variant が持つ**: 読めない周だけ rc 2 で、残りは rc 1。理由は run / path を名乗る。
    #[test]
    fn refuse_carries_its_own_rc_and_names_the_run() {
        for found in samples() {
            let rc = found.rc();
            let expected = match found {
                Refuse::WriteSetUnreadable { .. } => RC_BROKEN,
                _ => RC_REFUSED,
            };
            assert_eq!(rc, expected, "{} の rc", found.as_str());
            assert!(!found.reason().is_empty(), "{} は理由を 1 行で名乗る", found.as_str());
        }
        let overlap = Refuse::WriteSetOverlap { run: "r-1".to_owned(), path: "src/lib.rs".to_owned() };
        let line = overlap.reason();
        assert!(line.contains("r-1"), "相手の run id を名乗る: {line}");
        assert!(line.contains("src/lib.rs"), "交差した path を名乗る: {line}");
        assert!(!line.contains('\n'), "理由は 1 行: {line}");
    }

    /// base の tracked file（交差の表の fixture）。
    fn tracked() -> Vec<String> {
        ["a/b.rs", "src/x.rs", "src/a.rs", "src/b.rs"].iter().map(|path| (*path).to_owned()).collect()
    }

    /// 設計 §2 の表（dir と file・正規化の 3 形）と dir の展開（contract-source.md §3・新規 file と dir は交差しない）
    /// を字面で測る。
    #[test]
    fn refuse_overlap_table_follows_the_design() {
        let set = |item: &str| vec![item.to_owned()];
        let base = tracked();
        for (left, right, want) in [
            ("a/", "a/b.rs", true),
            ("a/", "ab/", false),
            ("a/", "a/c/", true),
            ("./a/b.rs", "a/b.rs", true),
            ("a//b.rs", "a/b.rs", true),
            ("src/../src/x.rs", "src/x.rs", true),
            ("src/a.rs", "src/b.rs", false),
            // dir は base の file 一覧に展開して数える＝新規 file（`+`）と base に無い file は配下でも交差しない。
            ("a/", "+a/new.rs", false),
            ("a/", "a/new.rs", false),
            ("+a/b.rs", "a/b.rs", true),
            ("+a/new.rs", "a/new.rs", true),
            // 縮む面（`-`）も素の path で照合する＝同じ file を減らす便と書く便は交差する。
            ("-a/b.rs", "a/b.rs", true),
            ("a/", "-a/b.rs", true),
        ] {
            let found = !overlaps(&set(left), &set(right), &base).is_empty();
            assert_eq!(found, want, "{left} × {right}");
            // 返るのは**契約が書いた字面のまま**（器が畳んだ形ではない）。
            if want {
                assert_eq!(
                    overlaps(&set(left), &set(right), &base).first().cloned(),
                    Some((left.to_owned(), right.to_owned())),
                    "{left} × {right} の組は字面のまま"
                );
            }
        }
        assert_eq!(normalize("./src//../src/x.rs"), "src/x.rs", "正規化の 3 形を畳む");
        assert_eq!(normalize("src/"), "src/", "末尾の / は dir の印として残る");
        assert_eq!(normalize("+src/new.rs"), "src/new.rs", "新規 file の接頭辞は剥がす");
        assert_eq!(normalize("-src/big.rs"), "src/big.rs", "縮む面の接頭辞も剥がす（交差の照合は素の path）");
    }

    /// 置き場だけの `=`（§43 (1)・行 ar）も `normalize` の 1 本が既存 3 形の隣で剥がす: 4 形とも同じ素の path に畳まれ、
    /// 交差と guard の照合（[`covered`]）は素の path の項目と同じ面を触ると読む。剥がすのは先頭の 1 字だけ。
    #[test]
    fn contract_place_only_normalize_strips_the_mark_next_to_the_other_three() {
        let marked = ["+src/a.rs", "-src/a.rs", "~src/a.rs", "=src/a.rs"];
        let stripped: Vec<String> = marked.iter().map(|item| normalize(item)).collect();
        assert_eq!(stripped, vec!["src/a.rs".to_owned(); 4], "4 形とも素の path（母集団 {} 形）", marked.len());
        assert_eq!(normalize("=src/"), "src/", "dir の印は残る");
        assert_eq!(normalize("==src/a.rs"), "=src/a.rs", "剥がすのは 1 字だけ");
        let place = vec!["=src/a.rs".to_owned()];
        assert!(covered(&place, "src/a.rs"), "= の項目は素の path を覆う（guard の照合は不変）");
        let crossed = overlaps(&place, &["src/a.rs".to_owned()], &tracked());
        assert_eq!(crossed, vec![("=src/a.rs".to_owned(), "src/a.rs".to_owned())], "交差は素の path で数え、字面のまま返る");
    }

    /// 交差の全組が返る（1 組で止めない＝stderr に全組を並べる材料）。
    #[test]
    fn refuse_overlap_returns_every_pair() {
        let mine = vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()];
        let theirs = vec!["src/".to_owned(), "docs/x.md".to_owned()];
        let found = overlaps(&mine, &theirs, &tracked());
        assert_eq!(found.len(), 2, "2 組とも返る: {found:?}");
        assert_eq!(found.first().cloned(), Some(("src/a.rs".to_owned(), "src/".to_owned())));
    }

    /// path の 1 本。**段に `..` を含む形も空間に入れる**（畳む規則を性質で測るため）。
    fn path() -> impl Strategy<Value = String> {
        segments(vec!["a", "b", "ab", "src", ".."])
    }

    /// `..` を含まない path の 1 本（前置した dir が畳まれない＝prefix が効く形）。
    fn plain_path() -> impl Strategy<Value = String> {
        segments(vec!["a", "b", "ab", "src"])
    }

    /// 段の候補から path を 1 本組む（末尾の `/` の有無も振る）。
    fn segments(choices: Vec<&'static str>) -> impl Strategy<Value = String> {
        (
            prop::collection::vec(prop::sample::select(choices), 1..4),
            any::<bool>(),
        )
            .prop_map(|(parts, dir)| {
                let joined = parts.join("/");
                if dir {
                    format!("{joined}/")
                } else {
                    joined
                }
            })
    }

    /// write-set 1 つ分（1〜3 本）。
    fn write_set() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(path(), 1..4)
    }

    /// `..` を持たない write-set 1 つ分。
    fn plain_write_set() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec(plain_path(), 1..4)
    }

    proptest! {
        #![proptest_config(config())]

        /// 判定は**対称**である（どちらを新しい契約として撃っても同じ答え）。base は右の file 項目を全部持つ形で振る。
        #[test]
        fn prop_refuse_overlap_is_symmetric(left in write_set(), right in write_set()) {
            let base: Vec<String> = right.iter().filter(|item| !item.ends_with('/')).cloned().collect();
            let forward = overlaps(&left, &right, &base).is_empty();
            let backward = overlaps(&right, &left, &base).is_empty();
            prop_assert_eq!(forward, backward);
        }

        /// 非空の集合は**自分自身と交差する**（同じ契約の 2 本目は必ず掛かる・base が空でも）。
        #[test]
        fn prop_refuse_nonempty_set_overlaps_itself(set in write_set()) {
            prop_assert!(!overlaps(&set, &set, &[]).is_empty());
        }

        /// 正規化で結果が変わらない（`./` 前置・`//` 重複・`x/../` の挿入）。
        #[test]
        fn prop_refuse_normalization_does_not_change_the_answer(left in write_set(), right in write_set()) {
            let base: Vec<String> = right.iter().filter(|item| !item.ends_with('/')).cloned().collect();
            let want = overlaps(&left, &right, &base).is_empty();
            let dotted: Vec<String> = left.iter().map(|item| format!("./{item}")).collect();
            let doubled: Vec<String> = left.iter().map(|item| item.replace('/', "//")).collect();
            let hopped: Vec<String> = left.iter().map(|item| format!("q/../{item}")).collect();
            for decorated in [dotted, doubled, hopped] {
                prop_assert_eq!(overlaps(&decorated, &right, &base).is_empty(), want);
            }
        }

        /// 共通 prefix を持たない 2 集合は交差しない（`..` で外へ出る形は前置の外）。
        #[test]
        fn prop_refuse_disjoint_prefixes_never_overlap(left in plain_write_set(), right in plain_write_set()) {
            let mine: Vec<String> = left.iter().map(|item| format!("left/{item}")).collect();
            let theirs: Vec<String> = right.iter().map(|item| format!("right/{item}")).collect();
            let base: Vec<String> = mine.iter().chain(&theirs).cloned().collect();
            prop_assert!(overlaps(&mine, &theirs, &base).is_empty());
        }
    }
}
