//! `cargo xtask deps-delta --base <sha>`: 1 便あたりの**直接依存の増分の門**と **compile 秒の検出線**
//! （設計 rules-manifest.md §4・憲法 C13 / C13.4 / C13.5・監査 2026-09-12 塊 2 = `s2-07l.161`）。
//!
//! `R-C13-1.per-pr`（1 便で足してよい本数）と `R-C13-1.check-delta-ms`（増分 compile 秒の検出線）は
//! manifest に在って `crates/*/src` に読み手が無かった（`rules-wired` が名指す側）。依存の門は
//! `cargo deny` と `ALLOWED_DEPS` の allow-list と check の `dep-budget`（総数）だけで、**1 便の増分**を
//! base と比べる面が無い。ここは `rules-diff` と同じ `--base` 経路で、base 側は `git show <base>:<path>`、
//! HEAD 側は tracked の現物を読み、直接依存を **(section, crate 名) の対**で数える。
//!
//! **2 面を分けて rc は deny の面だけで決める**: `added` > per-pr か `total` > budget なら rc 1（C13）。
//! `check-delta-ms` は C13.5 の検出線で、測れても超えても測れなくても rc に触れない（測れなかった
//! compile 秒で PR を止めない）。rc 2 は §4.3 と同じく**突合の鍵が壊れた周だけ**（`--base` 不在・
//! base の sha が解けない・HEAD の `Cargo.toml` / `Limits` が読めない）。
//!
//! 判定は純関数 [`judge`] が行い（歯はこれを撃つ・git も cargo も起動しない）、I/O は [`run`] に閉じる
//! （pipeline.md §7「歯は道具本体を起動しない」）。

use crate::check::{read_text, Layout};
use crate::check_facts::{dep_section, package_rename};
use crate::limits::Limits;
use crate::toml_lite::sections;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

/// 判定行と診断行の tag。
const TAG: &str = "deps-delta";

/// 使い方（rc 2 の 1 行・`rules-diff` と同形）。
const USAGE: &str = "usage: cargo xtask deps-delta --base <ref>";

/// 閾値の正本（HEAD 側の現物・cwd 相対）。
const RULES_REL: &str = "rules/manifest.toml";

/// root manifest の相対 path。
const ROOT_MANIFEST: &str = "Cargo.toml";

/// 作業 dir（`<root>/target/deps-delta`・base の木と `--target-dir`）。env は読まない（C2.2）。
const WORK_DIR: &str = "deps-delta";

/// 直接依存 1 本 = `(section の base 名, crate 名)`。version は見ない。
pub type Dep = (String, String);

/// 1 つの manifest（base か HEAD）の直接依存の集合。
pub type DepSet = BTreeSet<Dep>;

/// `Cargo.toml` の本文から直接依存の対を集める（節の分類は check の `deps-empty` と同じ 5 形）。
///
/// `[dependencies.<name>]` の入れ子は header の名を、それ以外は `key = value` の key を crate 名に
/// 取る。`package = "<other>"` で改名した entry は **改名先**を crate 名に取る（key を据え置いて
/// 中身を別 crate へ差し替える形を `added=0` に化けさせない）。
pub fn deps_of(manifest: &str) -> DepSet {
    let mut found = DepSet::new();
    for (header, pairs) in sections(manifest) {
        let Some((section, nested)) = dep_section(header) else {
            continue;
        };
        match nested {
            Some(dep) => {
                let renamed = pairs
                    .iter()
                    .find(|(key, _)| *key == "package")
                    .and_then(|(_, value)| crate::toml_lite::quoted(value));
                found.insert((section.to_owned(), renamed.unwrap_or_else(|| dep.to_owned())));
            }
            None => {
                for (key, value) in pairs {
                    let name = package_rename(value).unwrap_or_else(|| key.to_owned());
                    found.insert((section.to_owned(), name));
                }
            }
        }
    }
    found
}

/// base と HEAD の差（判定行の `added` / `ids` / `total`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    /// |HEAD の対 \ base の対|。
    pub added: usize,
    /// 増えた crate 名（名前順・重複なし）。
    pub ids: Vec<String>,
    /// |HEAD の対|。
    pub total: usize,
}

/// 集合の差を取る（純関数）。
pub fn delta(base: &DepSet, head: &DepSet) -> Delta {
    let fresh: Vec<&Dep> = head.difference(base).collect();
    let ids: BTreeSet<String> = fresh.iter().map(|(_, name)| name.clone()).collect();
    Delta {
        added: fresh.len(),
        ids: ids.into_iter().collect(),
        total: head.len(),
    }
}

/// deny の面の違反（閉じた enum・rc 1 の理由）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding {
    /// 1 便で足した本数が `R-C13-1.per-pr` を超えた。
    AddedOverPerPr,
    /// HEAD の直接依存の総数が `R-C13-1` を超えた。
    TotalOverBudget,
}

impl Finding {
    /// 違反行に写す名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AddedOverPerPr => "added-over-per-pr",
            Self::TotalOverBudget => "total-over-budget",
        }
    }

    /// stdout へ出す 1 行（判定行の前）。`limit=` の字面は判定行にだけ在る（grep の取り違えを避ける）。
    pub fn line(self, delta: &Delta, limits: &Limits) -> String {
        match self {
            Self::AddedOverPerPr => {
                format!("{TAG}: violation={} added={} per-pr={}", self.as_str(), delta.added, limits.dep_per_pr)
            }
            Self::TotalOverBudget => {
                format!("{TAG}: violation={} total={} budget={}", self.as_str(), delta.total, limits.dep_budget)
            }
        }
    }
}

/// `cargo check` を撃った側。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// base の木（`git archive` の展開）。
    Base,
    /// HEAD の木（cwd）。
    Head,
}

impl Side {
    /// 診断行に写す名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Head => "head",
        }
    }
}

/// `check-delta-ms` が測れなかった理由（閉じた enum・rc に触れない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckUnmeasured {
    /// cargo を起動できない（base の木を実体化できない周も含む）。
    Spawn,
    /// base か HEAD の `cargo check` が rc 非 0 で終えた。
    Failed(Side),
}

impl CheckUnmeasured {
    /// 診断行の `reason=` に写す字面。
    pub fn as_string(self) -> String {
        match self {
            Self::Spawn => "check-spawn".to_owned(),
            Self::Failed(side) => format!("check-failed side={}", side.as_str()),
        }
    }
}

/// 検出線の結果（閉じた enum・[`judge`] の引数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// `added` = 0 の便は測らない。
    Skipped,
    /// 両方の木の `cargo check` の壁時計（ms）。
    Measured {
        /// base の木。
        base_ms: u64,
        /// HEAD の木。
        head_ms: u64,
    },
    /// 測れなかった（判定行は `-`・stderr に診断 1 行）。
    Unmeasured(CheckUnmeasured),
}

impl CheckResult {
    /// 判定行の `check-delta-ms=` の 3 値（符号付き整数 / `skipped` / `-`）。
    pub fn as_string(self) -> String {
        match self {
            Self::Skipped => "skipped".to_owned(),
            Self::Measured { base_ms, head_ms } => {
                let delta = i128::from(head_ms).saturating_sub(i128::from(base_ms));
                delta.to_string()
            }
            Self::Unmeasured(_) => "-".to_owned(),
        }
    }
}

/// 判定の結果（stdout の行と rc）。
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// deny の面の違反（判定行の前に 1 件 1 行）。
    pub findings: Vec<Finding>,
    /// stdout の最終行。
    pub line: String,
    /// deny の面だけで決まる rc（0 / 1）。
    pub rc: ExitCode,
}

/// 判定行（stdout の最終行・key は常に全部出す・`limit=` は 1 度だけ）。
pub fn summary_line(base: &str, delta: &Delta, limits: &Limits, check: CheckResult) -> String {
    let ids = if delta.ids.is_empty() { "-".to_owned() } else { delta.ids.join(",") };
    format!(
        "{TAG}: base={base} added={} limit={} total={} budget={} ids={ids} check-delta-ms={} check-limit-ms={}",
        delta.added,
        limits.dep_per_pr,
        delta.total,
        limits.dep_budget,
        check.as_string(),
        limits.check_delta_ms
    )
}

/// 判定（純関数・I/O を持たない）。rc は deny の面（C13）だけで決まり、`check` は行に写るだけ（C13.5）。
pub fn judge(base: &DepSet, head: &DepSet, limits: &Limits, check: CheckResult, base_ref: &str) -> Verdict {
    let delta = delta(base, head);
    let mut findings = Vec::new();
    if u64::try_from(delta.added).ok().is_none_or(|added| added > limits.dep_per_pr) {
        findings.push(Finding::AddedOverPerPr);
    }
    if u64::try_from(delta.total).ok().is_none_or(|total| total > limits.dep_budget) {
        findings.push(Finding::TotalOverBudget);
    }
    let rc = if findings.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
    Verdict {
        findings,
        line: summary_line(base_ref, &delta, limits, check),
        rc,
    }
}

/// deny の面が測れなかった理由（閉じた enum・rc 2・`rules_diff` と同じ 2 variant）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasurable {
    /// base の sha が解けない・git を起動できない。
    BaseUnreadable,
    /// HEAD の `Layout` / `Cargo.toml` / `rules/manifest.toml` が読めない。
    HeadUnreadable,
}

impl Unmeasurable {
    /// stderr の 1 行に写す名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BaseUnreadable => "base-unreadable",
            Self::HeadUnreadable => "head-unreadable",
        }
    }
}

/// 「測れなかった」を表す rc（0 にも 1 にも化けさせない第 3 の値・判定行は出さない）。
fn unmeasurable(reason: Unmeasurable) -> ExitCode {
    crate::emit_err(&format!("{TAG}: unmeasurable reason={}", reason.as_str()));
    ExitCode::from(2)
}

/// flag の値を取る（値が無ければ `None`・`rules_diff` と同じ読み方）。
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at.saturating_add(1))
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
}

/// git を root で撃ち、rc 0 なら stdout を返す。起動できない周は `Err`、rc 非 0 は `Ok(None)`。
fn git_stdout(root: &Path, args: &[&str]) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|err| format!("git を起動できない: {err}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

/// `--base` を commit の sha に解く（解けない・起動できない周は `None`）。
fn resolve_base(root: &Path, base: &str) -> Option<String> {
    let spec = format!("{base}^{{commit}}");
    git_stdout(root, &["rev-parse", "--verify", &spec])
        .ok()
        .flatten()
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| !sha.is_empty())
}

/// HEAD の母集団（root の `Cargo.toml` + 各 member の `Cargo.toml`・root 相対）。
fn manifest_paths(layout: &Layout) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(ROOT_MANIFEST)];
    for dir in &layout.member_dirs {
        let rel = dir.strip_prefix(&layout.root).unwrap_or(dir);
        paths.push(rel.join(ROOT_MANIFEST));
    }
    paths
}

/// base 側の対の集合（`git show <sha>:<path>`・base に無い path は空集合）。
fn base_deps(root: &Path, sha: &str, paths: &[PathBuf]) -> Result<DepSet, String> {
    let mut found = DepSet::new();
    for path in paths {
        let spec = format!("{sha}:{}", path.display());
        if let Some(text) = git_stdout(root, &["show", &spec])? {
            found.extend(deps_of(&text));
        }
    }
    Ok(found)
}

/// HEAD 側の対の集合（tracked の現物・1 本でも読めなければ `Err`）。
fn head_deps(root: &Path, paths: &[PathBuf]) -> Result<DepSet, String> {
    let mut found = DepSet::new();
    for path in paths {
        found.extend(deps_of(&read_text(&root.join(path))?));
    }
    Ok(found)
}

/// `git archive <sha> | tar -x -C <dest>` で base の木を実体化する（pipeline.md §7 と同じ手）。
fn extract_base(root: &Path, sha: &str, dest: &Path) -> Result<(), String> {
    if let Err(err) = std::fs::remove_dir_all(dest) {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("{} を掃除できない: {err}", dest.display()));
        }
    }
    std::fs::create_dir_all(dest).map_err(|err| format!("{} を作れない: {err}", dest.display()))?;
    let mut archive = Command::new("git")
        .args(["archive", sha])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("git archive を起動できない: {err}"))?;
    let piped = archive.stdout.take().ok_or_else(|| "git archive の stdout を取れない".to_owned())?;
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dest)
        .stdin(Stdio::from(piped))
        .output()
        .map_err(|err| format!("tar を起動できない: {err}"))?;
    let archived = archive.wait_with_output().map_err(|err| format!("git archive を待てない: {err}"))?;
    if !archived.status.success() || !tar.status.success() {
        return Err("git archive | tar が rc≠0".to_owned());
    }
    if !dest.join(ROOT_MANIFEST).is_file() {
        return Err(format!("{} が無い（base の木が不完全）", dest.join(ROOT_MANIFEST).display()));
    }
    Ok(())
}

/// `cargo check --workspace --target-dir <target>` を `side` の木 `tree` で 1 回撃ち、壁時計（ms）を返す。
fn timed_check(tree: &Path, target: &Path, side: Side) -> Result<u64, CheckUnmeasured> {
    let started = Instant::now();
    let status = Command::new("cargo")
        .args(["check", "--workspace", "--target-dir"])
        .arg(target)
        .current_dir(tree)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|_| CheckUnmeasured::Spawn)?;
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if status.success() {
        Ok(elapsed)
    } else {
        Err(CheckUnmeasured::Failed(side))
    }
}

/// 検出線の測定（`added` ≥ 1 の便だけ）。base の木と HEAD の木で同じ `--target-dir` を使う。
fn measure_check(root: &Path, sha: &str) -> CheckResult {
    let work = root.join("target").join(WORK_DIR);
    let base_tree = work.join("base");
    let target = work.join("target");
    if let Err(reason) = extract_base(root, sha, &base_tree) {
        crate::emit_err(&format!("{TAG}: base の木を実体化できない: {reason}"));
        return CheckResult::Unmeasured(CheckUnmeasured::Spawn);
    }
    let measured = timed_check(&base_tree, &target, Side::Base)
        .and_then(|base_ms| timed_check(root, &target, Side::Head).map(|head_ms| (base_ms, head_ms)));
    match measured {
        Ok((base_ms, head_ms)) => CheckResult::Measured { base_ms, head_ms },
        Err(reason) => CheckResult::Unmeasured(reason),
    }
}

/// `deps-delta` の入口。
pub fn run(args: &[String]) -> ExitCode {
    let Some(base) = flag(args, "--base").filter(|value| !value.is_empty()) else {
        crate::emit_err(USAGE);
        return ExitCode::from(2);
    };
    let Ok(root) = std::env::current_dir() else {
        return unmeasurable(Unmeasurable::HeadUnreadable);
    };
    let Some(sha) = resolve_base(&root, base) else {
        return unmeasurable(Unmeasurable::BaseUnreadable);
    };
    let (layout, limits) = match (Layout::discover(&root), read_text(&root.join(RULES_REL)).and_then(|text| Limits::read(&text))) {
        (Ok(layout), Ok(limits)) => (layout, limits),
        (Err(reason), _) | (_, Err(reason)) => {
            crate::emit_err(&format!("{TAG}: HEAD: {reason}"));
            return unmeasurable(Unmeasurable::HeadUnreadable);
        }
    };
    let paths = manifest_paths(&layout);
    let base_set = match base_deps(&root, &sha, &paths) {
        Ok(set) => set,
        Err(reason) => {
            crate::emit_err(&format!("{TAG}: base: {reason}"));
            return unmeasurable(Unmeasurable::BaseUnreadable);
        }
    };
    let head_set = match head_deps(&root, &paths) {
        Ok(set) => set,
        Err(reason) => {
            crate::emit_err(&format!("{TAG}: HEAD: {reason}"));
            return unmeasurable(Unmeasurable::HeadUnreadable);
        }
    };
    let check = if delta(&base_set, &head_set).added == 0 { CheckResult::Skipped } else { measure_check(&root, &sha) };
    if let CheckResult::Unmeasured(reason) = check {
        crate::emit_err(&format!("{TAG}: check-unmeasured reason={}", reason.as_string()));
    }
    let verdict = judge(&base_set, &head_set, &limits, check, &sha);
    let delta = delta(&base_set, &head_set);
    for finding in &verdict.findings {
        crate::emit(&finding.line(&delta, &limits));
    }
    crate::emit(&verdict.line);
    verdict.rc
}

#[cfg(test)]
mod tests {
    use super::{delta, deps_of, judge, CheckResult, CheckUnmeasured, Finding, Side, Unmeasurable, Verdict};
    use crate::limits::Limits;
    use std::process::ExitCode;

    /// fixture の閾値（**実在しない**値＝判定行の数字が入力と衝突しない）: per-pr 1・budget 4・check 250。
    fn limits() -> Limits {
        Limits {
            core_lines: 1,
            file_lines: 1,
            test_src_ratio_pct: 1,
            fn_lines: 1,
            fn_complexity: 1,
            fn_args: 1,
            line_width: 1,
            dep_budget: 4,
            tmux_test_threads: 1,
            dep_per_pr: 1,
            check_delta_ms: 250,
        }
    }

    /// base の manifest（`[dependencies]` に 2 本）。
    const BASE: &str = "[package]\nname = \"probe\"\n\n[dependencies]\nprobe-alpha = \"1\"\nprobe-beta = { version = \"2\" }\n";

    /// `[dependencies]` へ crate を足した HEAD。
    fn head_with(names: &[&str]) -> String {
        let mut text = BASE.to_owned();
        for name in names {
            text.push_str(&format!("{name} = \"1\"\n"));
        }
        text
    }

    fn verdict_of(base: &str, head: &str, check: CheckResult) -> Verdict {
        judge(&deps_of(base), &deps_of(head), &limits(), check, "probe-sha-5a1")
    }

    #[test]
    fn deps_delta_counts_one_added_dep_within_per_pr() {
        let verdict = verdict_of(BASE, &head_with(&["probe-gamma"]), CheckResult::Skipped);
        assert_eq!(verdict.rc, ExitCode::SUCCESS, "{verdict:?}");
        assert_eq!(verdict.findings, Vec::new());
        assert!(verdict.line.starts_with("deps-delta: base=probe-sha-5a1 added=1 limit=1 total=3 budget=4 ids=probe-gamma "), "{}", verdict.line);
        assert_eq!(verdict.line.matches("limit=").count(), 1, "limit= は 1 度だけ: {}", verdict.line);
    }

    #[test]
    fn deps_delta_rejects_two_added_deps_over_per_pr() {
        let head = head_with(&["probe-gamma", "probe-delta"]);
        let verdict = verdict_of(BASE, &head, CheckResult::Skipped);
        assert_eq!(verdict.rc, ExitCode::FAILURE, "{verdict:?}");
        assert_eq!(verdict.findings, vec![Finding::AddedOverPerPr]);
        assert!(verdict.line.contains(" added=2 limit=1 "), "{}", verdict.line);
        assert!(verdict.line.contains(" ids=probe-delta,probe-gamma "), "名前順: {}", verdict.line);
        let d = delta(&deps_of(BASE), &deps_of(&head));
        assert_eq!(
            Finding::AddedOverPerPr.line(&d, &limits()),
            "deps-delta: violation=added-over-per-pr added=2 per-pr=1"
        );
    }

    #[test]
    fn deps_delta_rejects_total_over_budget() {
        // base に 4 本（budget いっぱい）・HEAD は +1（per-pr 以内）で total 5 > 4。
        let base = head_with(&["probe-gamma", "probe-delta"]);
        let head = head_with(&["probe-gamma", "probe-delta", "probe-epsilon"]);
        let verdict = verdict_of(&base, &head, CheckResult::Skipped);
        assert_eq!(verdict.rc, ExitCode::FAILURE, "{verdict:?}");
        assert_eq!(verdict.findings, vec![Finding::TotalOverBudget]);
        assert!(verdict.line.contains(" added=1 limit=1 total=5 budget=4 "), "{}", verdict.line);
        let d = delta(&deps_of(&base), &deps_of(&head));
        assert_eq!(
            Finding::TotalOverBudget.line(&d, &limits()),
            "deps-delta: violation=total-over-budget total=5 budget=4"
        );
    }

    #[test]
    fn deps_delta_counts_dev_build_and_workspace_sections() {
        let head = format!(
            "{BASE}\n[dev-dependencies]\nprobe-alpha = \"1\"\n\n[build-dependencies]\nprobe-build = \"1\"\n\n\
             [workspace.dependencies]\nprobe-shared = \"1\"\n\n[target.'cfg(unix)'.dependencies]\nprobe-unix = \"1\"\n\n\
             [dependencies.probe-nested]\nversion = \"1\"\n"
        );
        let d = delta(&deps_of(BASE), &deps_of(&head));
        // `[dependencies]` の `probe-alpha` と `[dev-dependencies]` の同名は別の対。
        assert_eq!(d.added, 5, "{d:?}");
        assert_eq!(d.total, 7, "{d:?}");
        assert_eq!(d.ids, ["probe-alpha", "probe-build", "probe-nested", "probe-shared", "probe-unix"]);
        let set = deps_of(&head);
        assert!(set.contains(&("dev-dependencies".to_owned(), "probe-alpha".to_owned())), "{set:?}");
        assert!(set.contains(&("dependencies".to_owned(), "probe-alpha".to_owned())), "{set:?}");
        assert!(set.contains(&("build-dependencies".to_owned(), "probe-build".to_owned())), "{set:?}");
        // `package =` の改名は改名先を crate 名に取る（key を据え置いた差し替えを 0 に化けさせない）。
        let renamed = BASE.replace("probe-alpha = \"1\"", "probe-alpha = { package = \"probe-other\", version = \"1\" }");
        let d = delta(&deps_of(BASE), &deps_of(&renamed));
        assert_eq!((d.added, d.ids.as_slice()), (1, ["probe-other".to_owned()].as_slice()), "{d:?}");
    }

    #[test]
    fn deps_delta_treats_manifest_absent_on_base_as_empty() {
        // base に無い member の `Cargo.toml` は空集合＝その依存は全部 `added`。
        let d = delta(&deps_of(""), &deps_of(BASE));
        assert_eq!(d.added, 2, "{d:?}");
        assert_eq!(d.ids, ["probe-alpha", "probe-beta"]);
        assert_eq!(d.total, 2);
        assert!(deps_of("").is_empty());
    }

    #[test]
    fn deps_delta_skips_check_when_nothing_added() {
        let verdict = verdict_of(BASE, BASE, CheckResult::Skipped);
        assert_eq!(verdict.rc, ExitCode::SUCCESS, "{verdict:?}");
        assert_eq!(
            verdict.line,
            "deps-delta: base=probe-sha-5a1 added=0 limit=1 total=2 budget=4 ids=- check-delta-ms=skipped check-limit-ms=250"
        );
        // 除去だけの便も `added=0`（base にだけ在る対は見ない）。
        let removed = BASE.replace("probe-beta = { version = \"2\" }\n", "");
        let d = delta(&deps_of(BASE), &deps_of(&removed));
        assert_eq!((d.added, d.total), (0, 1), "{d:?}");
    }

    #[test]
    fn deps_delta_records_measured_check_delta_over_limit_without_changing_rc() {
        let head = head_with(&["probe-gamma"]);
        let over = CheckResult::Measured { base_ms: 1_000, head_ms: 1_900 };
        let verdict = verdict_of(BASE, &head, over);
        assert_eq!(verdict.rc, ExitCode::SUCCESS, "検出線は rc に触れない: {verdict:?}");
        assert!(verdict.line.ends_with(" check-delta-ms=900 check-limit-ms=250"), "{}", verdict.line);
        // 負の差も符号付きで残る。
        let faster = CheckResult::Measured { base_ms: 1_900, head_ms: 1_000 };
        let verdict = verdict_of(BASE, &head, faster);
        assert!(verdict.line.ends_with(" check-delta-ms=-900 check-limit-ms=250"), "{}", verdict.line);
        assert_eq!(verdict.rc, ExitCode::SUCCESS);
    }

    #[test]
    fn deps_delta_keeps_deny_rc_when_check_is_unmeasured() {
        let spawn = CheckResult::Unmeasured(CheckUnmeasured::Spawn);
        let within = verdict_of(BASE, &head_with(&["probe-gamma"]), spawn);
        assert_eq!(within.rc, ExitCode::SUCCESS, "{within:?}");
        assert!(within.line.contains(" check-delta-ms=- check-limit-ms=250"), "{}", within.line);
        assert_eq!(CheckUnmeasured::Spawn.as_string(), "check-spawn");
        let failed = CheckResult::Unmeasured(CheckUnmeasured::Failed(Side::Base));
        let over = verdict_of(BASE, &head_with(&["probe-gamma", "probe-delta"]), failed);
        assert_eq!(over.rc, ExitCode::FAILURE, "rc は deny の面だけで決まる: {over:?}");
        assert_eq!(over.findings, vec![Finding::AddedOverPerPr]);
        assert!(over.line.contains(" check-delta-ms=- check-limit-ms=250"), "{}", over.line);
        assert_eq!(CheckUnmeasured::Failed(Side::Base).as_string(), "check-failed side=base");
        assert_eq!(CheckUnmeasured::Failed(Side::Head).as_string(), "check-failed side=head");
        // `-` は rc 2 に倒れない（rc 2 は deny の面が測れない周だけ）。
        assert_ne!(within.rc, ExitCode::from(2));
        assert_ne!(over.rc, ExitCode::from(2));
    }

    #[test]
    fn deps_delta_unmeasurable_base_yields_rc_two_without_a_line() {
        // deny の面が測れない周は判定行を持たない（`Verdict` を組まない）。rc 2 は 2 variant の名で出る。
        assert_eq!(super::unmeasurable(Unmeasurable::BaseUnreadable), ExitCode::from(2));
        assert_eq!(super::unmeasurable(Unmeasurable::HeadUnreadable), ExitCode::from(2));
        assert_eq!(Unmeasurable::BaseUnreadable.as_str(), "base-unreadable");
        assert_eq!(Unmeasurable::HeadUnreadable.as_str(), "head-unreadable");
        // `--base` が空の周も rc 2（USAGE の側）。
        assert_eq!(super::run(&["--base".to_owned(), String::new()]), ExitCode::from(2));
    }
}
