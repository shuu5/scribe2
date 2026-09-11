//! C16.2 の門（polarity）: core が描いた極性一覧の **tracked snapshot** の集計行を読み、in-loop の
//! guard の件数と全数を判定行に出す（設計 docs/design/polarity.md §5・ADR-0014 §2.3）。
//!
//! xtask は core に依存しない（ADR-0006 / ADR-0013）ので、読むのは tracked file だけである。
//! snapshot と実装のずれは insta の歯（nextest）が落とし、集計と門はここが落とす＝2 つの CI job が
//! 別の面を受ける。
//!
//! **読めない周は違反に倒す**（fail-closed）: snapshot が無い・集計行が無い・数が読めない・集計が
//! 自己矛盾（N ≠ K + M）のどれも「在る」に化けさせない。違反 = (a) 読めない (b) guard が 0 件
//! (c) in-loop が 0 件（全件 post-hoc）。閾値は無く manifest 行も持たない（構造不変条件・
//! name-literal / enum-slices と同じ型）。

use crate::check::{failed, read_text, Layout, Measured};

/// 判定行の tag。
const TAG: &str = "polarity";

/// 極性一覧の snapshot（core crate の dir からの相対）。
pub(crate) const SNAPSHOT_REL: &str = "tests/e2e/snapshots/e2e__polarity__polarity_external_form.snap";

/// 集計行の前置き（設計 §4）。
const SUMMARY_PREFIX: &str = "polarity: ";

/// 集計行の 4 つの数。
struct Summary {
    guards: usize,
    in_loop: usize,
    post_hoc: usize,
    fail_open: usize,
}

/// snapshot の集計行を読み、`polarity=<K>/<N>` を出す。
pub(crate) fn measure(layout: &Layout) -> Measured {
    let path = layout.core_dir.join(SNAPSHOT_REL);
    let text = match read_text(&path) {
        Ok(found) => found,
        Err(reason) => return failed(TAG, &format!("極性一覧を読めない: {reason}")),
    };
    let summary = match parse_summary(&text) {
        Ok(found) => found,
        Err(reason) => return failed(TAG, &format!("{}: {reason}", path.display())),
    };
    let mut violations = Vec::new();
    if summary.guards != summary.in_loop.saturating_add(summary.post_hoc) {
        violations.push(format!(
            "{TAG}: 集計が自己矛盾（guards={} ≠ in-loop={} + post-hoc={}）",
            summary.guards, summary.in_loop, summary.post_hoc
        ));
    }
    if summary.fail_open > summary.guards {
        violations.push(format!(
            "{TAG}: 集計が自己矛盾（fail-open={} > guards={}）",
            summary.fail_open, summary.guards
        ));
    }
    if summary.guards == 0 {
        violations.push(format!("{TAG}: guard が 0 件（C16.2）"));
    }
    if summary.in_loop == 0 {
        violations.push(format!("{TAG}: in-loop の guard が 0 件＝全件 post-hoc（C16.2）"));
    }
    Measured {
        fact: format!("{TAG}={}/{}", summary.in_loop, summary.guards),
        violations,
    }
}

/// `polarity: guards=<N> in-loop=<K> post-hoc=<M> fail-open=<F>` の行を 1 本だけ読む。
fn parse_summary(text: &str) -> Result<Summary, String> {
    let mut lines = text.lines().filter(|line| line.starts_with(SUMMARY_PREFIX));
    let Some(line) = lines.next() else {
        return Err("集計行（polarity: guards=…）が無い".to_owned());
    };
    if lines.next().is_some() {
        return Err("集計行が 2 本以上在る".to_owned());
    }
    let rest = line.strip_prefix(SUMMARY_PREFIX).unwrap_or_default();
    let count = |key: &str| -> Result<usize, String> {
        rest.split(' ')
            .find_map(|token| token.strip_prefix(&format!("{key}=")))
            .ok_or_else(|| format!("集計行に {key}= が無い: {line}"))?
            .parse()
            .map_err(|_| format!("集計行の {key}= が数でない: {line}"))
    };
    Ok(Summary {
        guards: count("guards")?,
        in_loop: count("in-loop")?,
        post_hoc: count("post-hoc")?,
        fail_open: count("fail-open")?,
    })
}

#[cfg(test)]
mod tests {
    // 歯は module と同居させる（`.101` の裁定 A′）: 新 module の別 file `*_tests.rs` は base に
    // 宣言元が無く compile されず flip-check が測れない。fixture は tmp dir に置いた snapshot 1 本。
    use super::{measure, SNAPSHOT_REL};
    use crate::check::Layout;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 同一 process 内での dir 名衝突を避ける連番。
    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// repo の外に一意な tmp dir を作る（作れなければ `None`）。
    fn make_tmp_dir() -> Option<PathBuf> {
        let base = std::env::temp_dir();
        for _ in 0..8 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("xtask-polarity-{}-{nanos}-{seq}", std::process::id()));
            if std::fs::create_dir(&dir).is_ok() {
                return Some(dir);
            }
        }
        None
    }

    /// core dir 相当の tmp dir を作り、snapshot の本文（`None` なら file 無し）を置いて測る。
    /// 後始末は assert より前。
    fn measured(snapshot: Option<&str>) -> (String, Vec<String>) {
        let Some(dir) = make_tmp_dir() else {
            return ("tmp dir を作れない".to_owned(), vec!["tmp dir を作れない".to_owned()]);
        };
        let written = match snapshot {
            None => true,
            Some(body) => write_snapshot(&dir, body),
        };
        let layout = Layout {
            root: dir.clone(),
            core_dir: dir.clone(),
            member_dirs: Vec::new(),
            name: "demo".to_owned(),
        };
        let got = if written {
            measure(&layout)
        } else {
            crate::check::failed("fixture", "snapshot を書けない")
        };
        let _ = std::fs::remove_dir_all(&dir);
        (got.fact, got.violations)
    }

    /// snapshot を書く（親 dir は作る）。
    fn write_snapshot(dir: &Path, body: &str) -> bool {
        let path = dir.join(SNAPSHOT_REL);
        path.parent()
            .is_some_and(|parent| std::fs::create_dir_all(parent).is_ok())
            && std::fs::write(&path, body).is_ok()
    }

    /// insta の header 付きの snapshot 本文。
    fn snap(lines: &[&str]) -> String {
        format!("---\nsource: x\nexpression: form\n---\n{}\n", lines.join("\n"))
    }

    /// 違反が `tag` の 1 件で `needle` を含むことを表明する。
    fn assert_single(violations: &[String], needle: &str) {
        assert_eq!(violations.len(), 1, "違反は 1 件のはず: {violations:?}");
        let head = violations.first().map(String::as_str).unwrap_or_default();
        assert!(head.starts_with("polarity: "), "tag は polarity のはず: {head}");
        assert!(head.contains(needle), "{needle} を名指すはず: {head}");
    }

    /// 正常形は `polarity=K/N` を出し違反 0（guard 行の数ではなく集計行を読む）。
    #[test]
    fn polarity_reads_the_summary_line_of_the_snapshot() {
        let (fact, violations) = measured(Some(&snap(&[
            "guard=a timing=in-loop on-failure=fail-closed boundary=m::A",
            "guard=b timing=post-hoc on-failure=fail-closed boundary=m::B",
            "guard=c timing=in-loop on-failure=fail-open boundary=m::C",
            "polarity: guards=3 in-loop=2 post-hoc=1 fail-open=1",
        ])));
        assert!(violations.is_empty(), "正常形は通る: {violations:?}");
        assert_eq!(fact, "polarity=2/3", "in-loop / 全数");
    }

    /// **読めない形は違反**（fail-closed）: snapshot 不在・集計行欠落・数でない・集計行 2 本。
    #[test]
    fn polarity_fails_closed_when_the_snapshot_cannot_be_read() {
        let (fact, missing) = measured(None);
        assert_single(&missing, "極性一覧を読めない");
        assert_eq!(fact, "polarity=?", "読めない周の fact は ? （数に化けない）");
        let (_, no_summary) = measured(Some(&snap(&[
            "guard=a timing=in-loop on-failure=fail-closed boundary=m::A",
        ])));
        assert_single(&no_summary, "集計行");
        let (_, not_a_number) = measured(Some(&snap(&["polarity: guards=x in-loop=1 post-hoc=0 fail-open=0"])));
        assert_single(&not_a_number, "数でない");
        let (_, twice) = measured(Some(&snap(&[
            "polarity: guards=1 in-loop=1 post-hoc=0 fail-open=0",
            "polarity: guards=1 in-loop=1 post-hoc=0 fail-open=0",
        ])));
        assert_single(&twice, "2 本以上");
    }

    /// C16.2 の門: guard が 0 件・in-loop が 0 件（全件 post-hoc）は違反。集計の自己矛盾も違反。
    #[test]
    fn polarity_rejects_zero_guards_and_zero_in_loop() {
        let (fact, zero) = measured(Some(&snap(&["polarity: guards=0 in-loop=0 post-hoc=0 fail-open=0"])));
        assert_eq!(zero.len(), 2, "guard 0 件と in-loop 0 件の両方が違反: {zero:?}");
        assert!(zero.iter().any(|line| line.contains("guard が 0 件")), "{zero:?}");
        assert!(zero.iter().any(|line| line.contains("in-loop の guard が 0 件")), "{zero:?}");
        assert_eq!(fact, "polarity=0/0", "0 を 0 と出す（伏せない）");
        let (_, all_post_hoc) = measured(Some(&snap(&[
            "guard=a timing=post-hoc on-failure=fail-closed boundary=m::A",
            "polarity: guards=1 in-loop=0 post-hoc=1 fail-open=0",
        ])));
        assert_single(&all_post_hoc, "in-loop の guard が 0 件");
        let (_, inconsistent) = measured(Some(&snap(&["polarity: guards=3 in-loop=1 post-hoc=1 fail-open=0"])));
        assert_single(&inconsistent, "自己矛盾");
        let (_, too_many_open) = measured(Some(&snap(&["polarity: guards=1 in-loop=1 post-hoc=0 fail-open=2"])));
        assert_single(&too_many_open, "fail-open=2 > guards=1");
    }
}
