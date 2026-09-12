//! 契約単位の拒否理由と、write-set の交差判定（設計 docs/design/pipeline-conflict.md §2・
//! ADR-0019 §2.1・FR4 / FR11・NFR4）。
//!
//! **主語は「契約 file が読めた後の契約単位の判定」**である。引数の不足（rc 1）と
//! [`super::contract::Contract::load`] の error（rc 2）は従来の口のまま外に在る——入力の型が
//! 違うものを 1 つの enum に集めると、極性一覧の 1 行が 2 種の判定を背負う。
//!
//! [`Unfit`](super::declaration) は **verify 行 1 本**の理由で境界が違うので触らない。
//!
//! 交差の判定は**契約の字面だけ**で閉じる: 正規化して集合の共通部分を見るだけで、symlink は
//! 解かず file の存在も見ない。実体が同じ file を別名で持つ 2 契約は入口で見逃す（偽陰性）が、
//! 編集時の guard が実体名で塞ぐ（ADR-0009 §2.1 の既知の穴はそのまま）。

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
}

impl Refuse {
    /// 一覧と pin が読む名（kebab・宣言順は [`REFUSALS`]）。
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "判別子順 pin の歯だけが読む（[`REFUSALS`] と対）")
    )]
    pub(crate) fn as_str(&self) -> &'static str {
        match *self {
            Self::NotARepo { .. } => "not-a-repo",
            Self::DuplicateRun { .. } => "duplicate-run",
            Self::WriteSetOverlap { .. } => "write-set-overlap",
            Self::WriteSetUnreadable { .. } => "write-set-unreadable",
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
        }
    }

    /// **rc は variant が持つ**。読めない周だけが「壊れた store」の rc 2 で、
    /// 残りは前提違反の rc 1 である（NFR4）。
    pub(crate) fn rc(&self) -> u8 {
        match *self {
            Self::NotARepo { .. } | Self::DuplicateRun { .. } | Self::WriteSetOverlap { .. } => {
                RC_REFUSED
            }
            Self::WriteSetUnreadable { .. } => RC_BROKEN,
        }
    }
}

/// 2 つの write-set が交差した**全組**（`(左の字面, 右の字面)`・先頭が理由の 1 行に載る）。
///
/// **照合は正規化した形で、返すのは契約が書いた字面のまま**である（読み手が自分の契約の
/// どの行を直せばよいかは、器が畳んだ形ではなく書いた字面でしか分からない）。
///
/// `pub(crate)` なのは、回答で write-set を広げる周（`.133`）が**同じ 1 本**を呼ぶためである
/// （本便は口だけを置き、answer への配線は `.133`）。
pub(crate) fn overlaps(left: &[String], right: &[String]) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for mine in left {
        for theirs in right {
            if touches(&normalize(mine), &normalize(theirs)) {
                found.push((mine.clone(), theirs.clone()));
            }
        }
    }
    found
}

/// path 1 本を字面で畳む（write-set guard の `relative_to` と同じ規則）。
///
/// 先頭の `./` を落とす・連続する `/` を 1 つにする・`..` を畳む・**末尾の `/` は dir の印
/// として残す**。root の外へ出る `..`（畳めない分）はそのまま残す＝字面が違うものを同じ
/// path に化けさせない。**存在は見ない**ので、まだ無い file を書く契約も同じ規則で測れる。
fn normalize(raw: &str) -> String {
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

/// 正規化した 2 本が交差するか（**対称**）。
fn touches(left: &str, right: &str) -> bool {
    covers(left, right) || covers(right, left)
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
    use super::{normalize, overlaps, Refuse, REFUSALS};
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
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
        ]
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

    /// 設計 §2 の表（dir と file・正規化の 3 形）を字面で測る。
    #[test]
    fn refuse_overlap_table_follows_the_design() {
        let set = |item: &str| vec![item.to_owned()];
        for (left, right, want) in [
            ("a/", "a/b.rs", true),
            ("a/", "ab/", false),
            ("a", "a/", true),
            ("./a/b.rs", "a/b.rs", true),
            ("a//b.rs", "a/b.rs", true),
            ("src/../src/x.rs", "src/x.rs", true),
            ("src/a.rs", "src/b.rs", false),
        ] {
            let found = !overlaps(&set(left), &set(right)).is_empty();
            assert_eq!(found, want, "{left} × {right}");
            // 返るのは**契約が書いた字面のまま**（器が畳んだ形ではない）。
            if want {
                assert_eq!(
                    overlaps(&set(left), &set(right)).first().cloned(),
                    Some((left.to_owned(), right.to_owned())),
                    "{left} × {right} の組は字面のまま"
                );
            }
        }
        assert_eq!(normalize("./src//../src/x.rs"), "src/x.rs", "正規化の 3 形を畳む");
        assert_eq!(normalize("src/"), "src/", "末尾の / は dir の印として残る");
    }

    /// 交差の全組が返る（1 組で止めない＝stderr に全組を並べる材料）。
    #[test]
    fn refuse_overlap_returns_every_pair() {
        let mine = vec!["src/a.rs".to_owned(), "src/b.rs".to_owned()];
        let theirs = vec!["src/".to_owned(), "docs/x.md".to_owned()];
        let found = overlaps(&mine, &theirs);
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

        /// 判定は**対称**である（どちらを新しい契約として撃っても同じ答え）。
        #[test]
        fn prop_refuse_overlap_is_symmetric(left in write_set(), right in write_set()) {
            let forward = overlaps(&left, &right).is_empty();
            let backward = overlaps(&right, &left).is_empty();
            prop_assert_eq!(forward, backward);
        }

        /// 非空の集合は**自分自身と交差する**（同じ契約の 2 本目は必ず掛かる）。
        #[test]
        fn prop_refuse_nonempty_set_overlaps_itself(set in write_set()) {
            prop_assert!(!overlaps(&set, &set).is_empty());
        }

        /// 正規化で結果が変わらない（`./` 前置・`//` 重複・`x/../` の挿入）。
        #[test]
        fn prop_refuse_normalization_does_not_change_the_answer(left in write_set(), right in write_set()) {
            let want = overlaps(&left, &right).is_empty();
            let dotted: Vec<String> = left.iter().map(|item| format!("./{item}")).collect();
            let doubled: Vec<String> = left.iter().map(|item| item.replace('/', "//")).collect();
            let hopped: Vec<String> = left.iter().map(|item| format!("q/../{item}")).collect();
            for decorated in [dotted, doubled, hopped] {
                prop_assert_eq!(overlaps(&decorated, &right).is_empty(), want);
            }
        }

        /// 共通 prefix を持たない 2 集合は交差しない（`..` で外へ出る形は前置の外）。
        #[test]
        fn prop_refuse_disjoint_prefixes_never_overlap(left in plain_write_set(), right in plain_write_set()) {
            let mine: Vec<String> = left.iter().map(|item| format!("left/{item}")).collect();
            let theirs: Vec<String> = right.iter().map(|item| format!("right/{item}")).collect();
            prop_assert!(overlaps(&mine, &theirs).is_empty());
        }
    }
}
