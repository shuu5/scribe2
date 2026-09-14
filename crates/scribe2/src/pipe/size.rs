//! 便の規模（設計 docs/design/gate-cost.md §5.1・`s2-07l.189`・FR10 / FR11）。
//!
//! land の成立した周に、面 5（`verdicts.jsonl`）の行へ `order` の後ろの任意 field として
//! `size` / `files` / `lines` / `pub_symbols` を足す材料を作る。**閾値は持たない**（knob 0・
//! 分布が溜まった後の裁定の材料・憲法 C10 / C12.4）。
//!
//! 数えるのは pure 関数 [`measure_size`] の 1 本で、git を撃つ口は land が渡す（[`fields`] の `read`）。
//! 読めない周は field を**欠く**——0 と書くと「測った 0」と「測れなかった」が混ざる。

use crate::fleet::json_lite::Value;

/// 便の diff（`<base>..<new>`）から数えた 4 値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// 触った file の数（`git diff --name-only -z` の件数・write-set 照合と同じ面）。
    pub files: u64,
    /// 足した行数（`git diff --numstat` の合計・binary の `-` は数えない・test 区間を除かない）。
    pub added: u64,
    /// 消した行数（同上）。
    pub removed: u64,
    /// 追加行のうち `pub ` で始まる行の数（字面走査の下界）。
    pub pub_symbols: u64,
}

/// 3 つの git の出力から 4 値を数える（**pure**）。
///
/// - `name_only`: `git diff --name-only -z` の出力（NUL 区切り・空の要素は数えない）。
/// - `numstat`: `git diff --numstat` の出力（`<added>\t<removed>\t<path>`・binary の `-\t-` は数えない）。
/// - `diff`: `git diff` の text。`+` で始まる行の中身が（行頭の空白を除いて）`pub ` で始まるものを数える
///   ——impl の中の `pub fn` と struct の `pub` field も数え、`pub(crate)` は `pub ` で始まらないので数えない。
///   削除行と文脈行は数えない。見出しの `+++ b/…` は `+` を 1 つ外しても `pub ` で始まらない。
pub fn measure_size(name_only: &str, numstat: &str, diff: &str) -> Size {
    let files = count(name_only.split('\0').filter(|path| !path.is_empty()).count());
    let (mut added, mut removed) = (0_u64, 0_u64);
    for line in numstat.lines() {
        let mut columns = line.split('\t');
        added = added.saturating_add(columns.next().and_then(|found| found.parse().ok()).unwrap_or(0));
        removed = removed.saturating_add(columns.next().and_then(|found| found.parse().ok()).unwrap_or(0));
    }
    let pub_symbols = count(
        diff.lines()
            .filter_map(|line| line.strip_prefix('+'))
            .filter(|body| body.trim_start().starts_with("pub "))
            .count(),
    );
    Size { files, added, removed, pub_symbols }
}

/// 面 5 の行へ足す 4 field（`order` の後ろ・この順）。
///
/// `read` は git の引数列を撃って stdout を返す口（land が渡す・読めない周は `None`）。base が無い周・
/// 3 本のどれかを読めない周は**空**＝4 field を欠く（0 と書かない）。`size` は契約の字面のまま載せる。
pub fn fields(
    declared: &str,
    base: Option<&str>,
    new: &str,
    read: impl Fn(&[&str]) -> Option<Vec<u8>>,
) -> Vec<(&'static str, Value)> {
    let measured = base.map(|base| format!("{base}..{new}")).and_then(|range| {
        let name_only = read_text(&read, &["diff", "--name-only", "-z", &range])?;
        let numstat = read_text(&read, &["diff", "--numstat", &range])?;
        let diff = read_text(&read, &["diff", "--no-color", "--no-ext-diff", &range])?;
        Some(measure_size(&name_only, &numstat, &diff))
    });
    let Some(size) = measured else {
        return Vec::new();
    };
    vec![
        ("size", Value::Str(declared.to_owned())),
        ("files", Value::Num(size.files)),
        ("lines", Value::Str(format!("{}/{}", size.added, size.removed))),
        ("pub_symbols", Value::Num(size.pub_symbols)),
    ]
}

/// git を 1 回撃って stdout を text で得る（読めない周は `None`）。
fn read_text(read: &impl Fn(&[&str]) -> Option<Vec<u8>>, args: &[&str]) -> Option<String> {
    read(args).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// 件数を u64 へ（usize が溢れる形は無いが、`as` で黙って切らない）。
fn count(found: usize) -> u64 {
    u64::try_from(found).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{fields, measure_size, Size};
    use crate::fleet::json_lite;

    /// `src/lib.rs` の 3 行のうち 2 行を消して 4 行を足し、binary を 1 本足した diff の fixture。
    const NAME_ONLY: &str = "src/blob.bin\0src/lib.rs\0";
    /// 同じ diff の numstat（binary は `-\t-`）。
    const NUMSTAT: &str = "-\t-\tsrc/blob.bin\n4\t2\tsrc/lib.rs\n";
    /// 同じ diff の text。数える `pub ` の追加行は `a`（行頭）と `b`（字下げ）の 2 本だけで、削除行の `gone`・
    /// 文脈行の `kept`・`pub(crate)` の `c`・comment の `d` は数えない。
    const DIFF: &str = "diff --git a/src/blob.bin b/src/blob.bin\nnew file mode 100644\n\
                        Binary files /dev/null and b/src/blob.bin differ\n\
                        diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,5 @@\n\
                        -// seed\n-pub fn gone() {}\n pub fn kept() {}\n+pub fn a() {}\n+    pub fn b() {}\n\
                        +pub(crate) fn c() {}\n+// pub fn d() {}\n";

    /// 偽の git（range が `b0..n1` で `fail` を含まない呼出しにだけ fixture を返す）。
    fn fake_git(args: &[&str], fail: &str) -> Option<Vec<u8>> {
        if args.last() != Some(&"b0..n1") || args.contains(&fail) {
            return None;
        }
        let text = if args.contains(&"--name-only") {
            NAME_ONLY
        } else if args.contains(&"--numstat") {
            NUMSTAT
        } else {
            DIFF
        };
        Some(text.as_bytes().to_vec())
    }

    /// 空の diff は**読めた 0**（`files=0 lines=0/0 pub_symbols=0`）で、field を欠く周（読めない）とは別の形。
    #[test]
    fn size_empty_diff_is_a_measured_zero() {
        assert_eq!(measure_size("", "", ""), Size { files: 0, added: 0, removed: 0, pub_symbols: 0 });
        let zero = fields("S", Some("b0"), "n1", |_| Some(Vec::new()));
        assert_eq!(
            json_lite::write_object(&zero),
            r#"{"size":"S","files":0,"lines":"0/0","pub_symbols":0}"#,
            "読めた 0 は 4 field を 0 で持つ"
        );
    }

    /// numstat の binary（`-\t-`）は行数に数えないが、file 数には name-only の件数として数える。
    #[test]
    fn size_numstat_does_not_count_binary() {
        assert_eq!(measure_size(NAME_ONLY, NUMSTAT, ""), Size { files: 2, added: 4, removed: 2, pub_symbols: 0 });
        assert_eq!(measure_size("-\t-\tx\n", "-\t-\tx\n", "").added, 0, "binary だけの numstat は 0 行");
        assert_eq!(measure_size("a\0\0b", "", "").files, 2, "空の要素は file に数えない（末尾 NUL の有無に依らない）");
    }

    /// `pub ` の追加行だけを数える（削除行・文脈行・`pub(crate)`・comment は数えない・字下げは数える）。
    #[test]
    fn size_counts_added_pub_lines_only() {
        assert_eq!(measure_size("", "", DIFF).pub_symbols, 2, "a と b の 2 本");
        assert_eq!(measure_size("", "", "-pub fn gone() {}\n pub fn kept() {}\n").pub_symbols, 0, "削除行と文脈行");
    }

    /// 4 field は `<base>..<new>` の 3 本を読めた周だけ・この順で載る。どれか 1 本・base を読めない周は**全部欠く**。
    #[test]
    fn size_fields_follow_the_range_and_drop_all_when_unreadable() {
        let read = fields("M", Some("b0"), "n1", |args| fake_git(args, "none"));
        assert_eq!(
            json_lite::write_object(&read),
            r#"{"size":"M","files":2,"lines":"4/2","pub_symbols":2}"#,
            "size は契約の字面・値は fixture の diff"
        );
        for fail in ["--name-only", "--numstat", "--no-color"] {
            assert!(fields("M", Some("b0"), "n1", |args| fake_git(args, fail)).is_empty(), "{fail} を読めない周");
        }
        assert!(fields("M", None, "n1", |args| fake_git(args, "none")).is_empty(), "base が無い周");
        assert!(fields("M", Some("other"), "n1", |args| fake_git(args, "none")).is_empty(), "range は base..new");
    }
}
