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
//!
//! 本 module は**もう 1 面**を持つ（`polarity-sites`・`s2-07l.177`・設計 polarity.md §3）:
//! `crates/*/src` の `const <NAME>: Polarity` の宣言 site を path の形で全数集め、core の
//! `polarity.rs` の `Guard::polarity` の網羅 match が参照する path と**両方向**で突き合わせる。
//! guard でない境界の免除は doc コメント（散文は規則でない・N2）ではなく、同じ file の閉じた
//! const slice `NOT_A_GUARD` が持ち、ここはその要素の path を字面で読む。極性は snapshot の面と
//! 同じ fail-closed で、path を組めない const・`crate::` で始まらない参照・片側だけ読めない木は
//! 違反に倒す。site も arm も 0 の木だけが `0/0/0` で通る（enum-slices の母集団 0 と同じ）。

use crate::check::{failed, read_text, Layout, Measured, SourceFile};
use crate::enum_slices::{const_head, is_type_ident};
use std::path::Path;

/// 判定行の tag。
const TAG: &str = "polarity";

/// 宣言 site と網羅 match の突合（polarity-sites）の tag。
const SITES_TAG: &str = "polarity-sites";

/// 極性一覧の module（core crate の `src/` からの相対）。
const SITES_REL: &str = "polarity.rs";

/// 極性の型の名（`const <NAME>: Polarity` の型の字面）。
const POLARITY_TYPE: &str = "Polarity";

/// `Guard::polarity` の網羅 match を持つ関数の宣言行（字下げ込み・字面で 1 本だけ在る）。
const POLARITY_FN_HEAD: &str = "    pub fn polarity(self) -> Polarity {";

/// 網羅 match の閉じ（関数の `}`・4 空白）。
const POLARITY_FN_END: &str = "    }";

/// guard でない site の免除 slice の宣言行。
const NOT_A_GUARD_HEAD: &str = "pub const NOT_A_GUARD: &[Polarity] = &[";

/// 免除 slice の閉じ。
const NOT_A_GUARD_END: &str = "];";

/// crate 相対の参照の前置き。
const CRATE_PREFIX: &str = "crate::";

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

/// 極性の宣言 site と `Guard` の網羅 match を両方向で突き合わせ、`polarity-sites=<site>/<arm>/<免除>`
/// を出す（`s2-07l.177`）。
///
/// 入力は `enum-slices` と同じ収集済みの file 列で、core の `polarity.rs` は [`Layout`] の core dir
/// から特定する（disk を 2 度読まない）。
pub(crate) fn measure_sites(layout: &Layout, files: &[SourceFile]) -> Measured {
    let mut violations = Vec::new();
    let mut sites = Vec::new();
    for file in files {
        for found in sites_in(&file.path, &file.text) {
            match found {
                Ok(site) => sites.push(site),
                Err(reason) => {
                    violations.push(format!("{SITES_TAG}: {}: {reason}", file.path.display()))
                }
            }
        }
    }
    let list = layout.core_dir.join("src").join(SITES_REL);
    let Some(source) = files.iter().find(|file| file.path == list) else {
        return absent(&sites, violations, &list);
    };
    let (arms, exempt) = match read_list(&source.text) {
        Ok(read) => read,
        Err(reason) => {
            violations.push(format!("{SITES_TAG}: {}: {reason}", list.display()));
            return Measured { fact: format!("{SITES_TAG}=?"), violations };
        }
    };
    violations.extend(cross(&sites, &arms, &exempt));
    Measured {
        fact: format!("{SITES_TAG}={}/{}/{}", sites.len(), arms.len(), exempt.len()),
        violations,
    }
}

/// 極性一覧の module が無い木。site も違反も 0 なら**母集団 0** として通し（擬似 workspace の形・
/// enum-slices の 0 対と同じ極性）、site が 1 つでも在れば突き合わせる相手が無い＝違反に倒す。
fn absent(sites: &[String], mut violations: Vec<String>, list: &Path) -> Measured {
    if sites.is_empty() && violations.is_empty() {
        return Measured { fact: format!("{SITES_TAG}=0/0/0"), violations };
    }
    violations.push(format!(
        "{SITES_TAG}: {} が無い（site {} 件を突き合わせられない）",
        list.display(),
        sites.len()
    ));
    Measured { fact: format!("{SITES_TAG}=?"), violations }
}

/// 極性一覧から網羅 match の参照 path と免除 slice の要素を読む（どちらか一方でも読めなければ `Err`）。
fn read_list(text: &str) -> Result<(Vec<String>, Vec<String>), String> {
    Ok((arms_of(text)?, exempt_of(text)?))
}

/// file 1 本の `const <NAME>: Polarity` の宣言 site を path の形で集める。
///
/// module path は file path から組む（`crates/<crate>/src/` より下・`mod.rs` / `lib.rs` / `main.rs`
/// は module 名に現れない）。行頭の `impl <Type> {` の中の関連 const は `crate::<mod>::<Type>::<NAME>`、
/// module 直下は `crate::<mod>::<NAME>`。読めない形（`crates/*/src` の下でない・`impl` の頭が
/// 裸の型名でない）は `Err` に倒す（黙って母集団から落とさない）。
fn sites_in(path: &Path, text: &str) -> Vec<Result<String, String>> {
    let mut found = Vec::new();
    let mut impl_head: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        if line == "}" {
            impl_head = None;
        } else if line.starts_with("impl") {
            impl_head = Some(line.strip_prefix("impl ").unwrap_or(line).trim_end_matches(" {").trim().to_owned());
        }
        let Some(name) = polarity_const(line) else {
            continue;
        };
        let line_no = index.saturating_add(1);
        found.push(site_path(path, impl_head.as_deref(), name).ok_or_else(|| {
            format!("{name}（{line_no} 行）の宣言 site の path を組めない（module か `impl` の形を読めない）")
        }));
    }
    found
}

/// `const <NAME>: Polarity = …` の行なら `<NAME>` を返す（型が `Polarity` ちょうどの行だけ）。
fn polarity_const(line: &str) -> Option<&str> {
    let head = const_head(line)?;
    let (name, after) = head.split_once(':')?;
    let declared = after.split('=').next().unwrap_or_default().trim();
    (declared == POLARITY_TYPE).then(|| name.trim())
}

/// 宣言 site の crate 相対 path。
fn site_path(path: &Path, impl_head: Option<&str>, name: &str) -> Option<String> {
    let module = module_of(path)?;
    let owner = match impl_head {
        None => None,
        Some(head) if is_type_ident(head) => Some(head),
        Some(_) => return None,
    };
    let mut out = String::from(CRATE_PREFIX);
    for part in [Some(module.as_str()), owner].into_iter().flatten() {
        if part.is_empty() {
            continue;
        }
        out.push_str(part);
        out.push_str("::");
    }
    out.push_str(name);
    Some(out)
}

/// file path から module path（`::` 区切り・crate root は空文字）を組む。
///
/// `crates/<crate>/src` の並びは**末尾から**探す: root は正規化されずに渡ることが在り
/// （歯は `CARGO_MANIFEST_DIR/../..` ＝ `…/crates/xtask/../..` を root に撃つ）、最初の `crates`
/// を見ると 2 つ先が `src` でない＝全 site が「読めない形」に化ける。
fn module_of(path: &Path) -> Option<String> {
    let parts: Vec<String> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let mut found = None;
    for (index, part) in parts.iter().enumerate() {
        let at = index.saturating_add(2);
        if part == "crates" && parts.get(at).map(String::as_str) == Some("src") {
            found = Some(at);
        }
    }
    let src = found?;
    let mut mods = Vec::new();
    for part in parts.get(src.checked_add(1)?..).unwrap_or_default() {
        let name = part.strip_suffix(".rs").unwrap_or(part);
        // crate root と module の file 名は module path に現れない。
        if ["mod", "lib", "main"].contains(&name) {
            continue;
        }
        mods.push(name.to_owned());
    }
    Some(mods.join("::"))
}

/// `Guard::polarity` の網羅 match の arm が参照する path。arm でない行が混じれば `Err`（fail-closed）。
fn arms_of(text: &str) -> Result<Vec<String>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| *line == POLARITY_FN_HEAD) else {
        return Err(format!("`{}` の宣言行が無い（読めない形）", POLARITY_FN_HEAD.trim()));
    };
    let mut arms = Vec::new();
    for line in lines.iter().skip(start.saturating_add(1)) {
        if *line == POLARITY_FN_END {
            return Ok(arms);
        }
        let item = line.trim();
        if item.is_empty() || item.starts_with("//") || item == "match self {" || item == "}" {
            continue;
        }
        let Some((_, referenced)) = item.split_once("=>") else {
            return Err(format!("`polarity` の網羅 match の行を読めない: `{item}`"));
        };
        let target = referenced.trim().trim_end_matches(',').trim();
        if !target.starts_with(CRATE_PREFIX) {
            return Err(format!("`polarity` の arm が `{CRATE_PREFIX}` で始まらない: `{target}`"));
        }
        arms.push(target.to_owned());
    }
    Err("`polarity` の閉じを見つけられない".to_owned())
}

/// 免除 slice `NOT_A_GUARD` の要素の path。宣言が無い・`crate::` で始まらない要素は `Err`。
fn exempt_of(text: &str) -> Result<Vec<String>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|line| *line == NOT_A_GUARD_HEAD) else {
        return Err(format!("`{NOT_A_GUARD_HEAD}` の宣言行が無い（免除は closed slice 1 本で持つ）"));
    };
    let mut exempt = Vec::new();
    for line in lines.iter().skip(start.saturating_add(1)) {
        if *line == NOT_A_GUARD_END {
            return Ok(exempt);
        }
        let item = line.trim().trim_end_matches(',').trim();
        if item.is_empty() {
            continue;
        }
        if !item.starts_with(CRATE_PREFIX) {
            return Err(format!("NOT_A_GUARD の要素が `{CRATE_PREFIX}` で始まらない: `{item}`"));
        }
        exempt.push(item.to_owned());
    }
    Err(format!("NOT_A_GUARD の閉じ `{NOT_A_GUARD_END}` を見つけられない"))
}

/// site と arm を**両方向**で突き合わせる（免除は site → arm の側からだけ外す）。
fn cross(sites: &[String], arms: &[String], exempt: &[String]) -> Vec<String> {
    let mut reasons = Vec::new();
    for site in sites {
        if !arms.contains(site) && !exempt.contains(site) {
            reasons.push(format!(
                "{SITES_TAG}: {site} が `Guard` の網羅 match に無い（guard でないなら NOT_A_GUARD に載せる）"
            ));
        }
    }
    for arm in arms {
        if !sites.contains(arm) {
            reasons.push(format!("{SITES_TAG}: `Guard` の arm が参照する {arm} の宣言 site が無い"));
        }
    }
    for site in exempt {
        if !sites.contains(site) {
            reasons.push(format!("{SITES_TAG}: NOT_A_GUARD の {site} の宣言 site が無い"));
        }
        if arms.contains(site) {
            reasons.push(format!("{SITES_TAG}: NOT_A_GUARD の {site} は `Guard` の arm にも在る"));
        }
    }
    reasons
}

#[cfg(test)]
mod tests {
    // 歯は module と同居させる（`.101` の裁定 A′）: 新 module の別 file `*_tests.rs` は base に
    // 宣言元が無く compile されず flip-check が測れない。fixture は tmp dir に置いた snapshot 1 本。
    use super::{
        measure, measure_sites, NOT_A_GUARD_END, NOT_A_GUARD_HEAD, POLARITY_FN_END,
        POLARITY_FN_HEAD, SITES_REL, SITES_TAG, SNAPSHOT_REL,
    };
    use crate::check::{Layout, SourceFile};
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

    // ---- polarity-sites（宣言 site と `Guard` の網羅 match の両方向の突合・`s2-07l.177`）----
    //
    // fixture は合成した [`SourceFile`] の列で disk を使わない（measure は収集済みの file だけを
    // 見る純関数・先例は `env_reads::tests`）。fixture の本文は**必ず 1 行の字面**で持つ: 物理行の
    // 行頭に `pub const …: Polarity` が来ると、この file 自身が実 repo の走査で site に数えられる。

    /// 極性の site 1 本を持つ file の本文（module 直下の宣言）。
    fn site_rs() -> String {
        "use crate::polarity::{OnFailure, Polarity, Timing};\n\npub const POLARITY: Polarity = Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed };\n".to_owned()
    }

    /// 極性一覧 file の本文（`Guard::polarity` の arm の参照と `NOT_A_GUARD` の要素を与える）。
    fn list_rs(arms: &[&str], exempt: &[&str]) -> String {
        let body: String = arms
            .iter()
            .enumerate()
            .map(|(at, target)| format!("            Self::G{at} => {target},\n"))
            .collect();
        let exempted: String = exempt.iter().map(|target| format!("    {target},\n")).collect();
        format!(
            "impl Guard {{\n{POLARITY_FN_HEAD}\n        match self {{\n{body}        }}\n{POLARITY_FN_END}\n}}\n\n{NOT_A_GUARD_HEAD}\n{exempted}{NOT_A_GUARD_END}\n"
        )
    }

    /// core の `src/` からの相対 path と本文の対から measure の入力を組む。
    ///
    /// root は**正規化しない形**で持つ（`…/crates/xtask/../..`）: 現物の呼び手
    /// （`check::tests::check_passes_on_workspace`）が `CARGO_MANIFEST_DIR` からこの形で撃つので、
    /// 最初の `crates` を crate root と見る実装はここで落ちる。
    fn workspace(files: &[(&str, String)]) -> (Layout, Vec<SourceFile>) {
        let root = PathBuf::from("/fixture-root/crates/xtask").join("..").join("..");
        let core_dir = root.join("crates").join("demo");
        let sources = files
            .iter()
            .map(|(rel, text)| SourceFile {
                path: core_dir.join("src").join(rel),
                text: text.clone(),
            })
            .collect();
        let layout = Layout { root, core_dir, member_dirs: Vec::new(), name: "demo".to_owned() };
        (layout, sources)
    }

    /// 違反が `polarity-sites` の 1 件で `needle` を含むことを表明する。
    fn assert_sites(violations: &[String], needle: &str) {
        assert_eq!(violations.len(), 1, "違反は 1 件のはず: {violations:?}");
        let head = violations.first().map(String::as_str).unwrap_or_default();
        assert!(head.starts_with(&format!("{SITES_TAG}: ")), "tag は {SITES_TAG} のはず: {head}");
        assert!(head.contains(needle), "{needle} を名指すはず: {head}");
    }

    /// `Polarity` を宣言しているのに `Guard` の網羅 match に無い site を名指す（site → arm の欠け）。
    #[test]
    fn polarity_sites_names_a_site_missing_from_guard() {
        let (layout, files) = workspace(&[
            ("hook/guard.rs", site_rs()),
            ("seat/rebrief.rs", site_rs()),
            (SITES_REL, list_rs(&["crate::hook::guard::POLARITY"], &[])),
        ]);
        let got = measure_sites(&layout, &files);
        assert_sites(&got.violations, "crate::seat::rebrief::POLARITY");
        assert_eq!(got.fact, "polarity-sites=2/1/0", "site 2 / arm 1 / 免除 0");
        // site も arm も 0 の木は母集団 0 として通す（enum-slices の 0 対と同じ極性）。
        let (empty_layout, empty_files) = workspace(&[("hook/guard.rs", "fn deny() {}\n".to_owned())]);
        let empty = measure_sites(&empty_layout, &empty_files);
        assert!(empty.violations.is_empty(), "母集団 0 は通る: {:?}", empty.violations);
        assert_eq!(empty.fact, "polarity-sites=0/0/0", "0 を 0 と出す");
    }

    /// `Guard` の arm が参照するのに宣言 site の無い path を名指す（arm → site の欠け）。
    /// 一覧そのものが読めない周も違反に倒す（fail-closed）。
    #[test]
    fn polarity_sites_names_a_guard_arm_without_a_site() {
        let (layout, files) =
            workspace(&[(SITES_REL, list_rs(&["crate::hook::guard::POLARITY"], &[]))]);
        let got = measure_sites(&layout, &files);
        assert_sites(&got.violations, "crate::hook::guard::POLARITY");
        assert_eq!(got.fact, "polarity-sites=0/1/0", "site 0 / arm 1 / 免除 0");
        // site は在るのに一覧の file が無い木（突き合わせる相手が無い）。
        let (no_list_layout, no_list_files) = workspace(&[("hook/guard.rs", site_rs())]);
        let no_list = measure_sites(&no_list_layout, &no_list_files);
        assert_sites(&no_list.violations, "site 1 件を突き合わせられない");
        assert_eq!(no_list.fact, "polarity-sites=?", "読めない周の fact は ?");
        // 免除 slice を持たない一覧・`crate::` で始まらない arm も読めない形。
        let (bare_layout, bare_files) = workspace(&[
            ("hook/guard.rs", site_rs()),
            (SITES_REL, "impl Guard {\n".to_owned() + POLARITY_FN_HEAD + "\n        match self {\n            Self::G0 => crate::hook::guard::POLARITY,\n        }\n" + POLARITY_FN_END + "\n}\n"),
        ]);
        assert_sites(&measure_sites(&bare_layout, &bare_files).violations, "NOT_A_GUARD");
        let (alias_layout, alias_files) =
            workspace(&[(SITES_REL, list_rs(&["self::hook::guard::POLARITY"], &[]))]);
        assert_sites(&measure_sites(&alias_layout, &alias_files).violations, "`crate::` で始まらない");
    }

    /// `NOT_A_GUARD` に載る site は site → arm の欠けから外す（免除は散文でなく closed slice）。
    /// 宣言 site の無い要素と、arm にも在る要素は逆に違反である（腐った免除を残さない）。
    #[test]
    fn polarity_sites_skips_sites_listed_in_not_a_guard() {
        let exempted = ["crate::fleet::json_tree::POLARITY"];
        let (layout, files) = workspace(&[
            ("hook/guard.rs", site_rs()),
            ("fleet/json_tree.rs", site_rs()),
            (SITES_REL, list_rs(&["crate::hook::guard::POLARITY"], &exempted)),
        ]);
        let got = measure_sites(&layout, &files);
        assert!(got.violations.is_empty(), "免除された site は落ちない: {:?}", got.violations);
        assert_eq!(got.fact, "polarity-sites=2/1/1", "site 2 / arm 1 / 免除 1");
        // 宣言 site の消えた免除は残骸として落ちる。
        let (stale_layout, stale_files) = workspace(&[
            ("hook/guard.rs", site_rs()),
            (SITES_REL, list_rs(&["crate::hook::guard::POLARITY"], &exempted)),
        ]);
        assert_sites(&measure_sites(&stale_layout, &stale_files).violations, "の宣言 site が無い");
        // guard でもあり免除でもある形（2 面）は落ちる。
        let (both_layout, both_files) = workspace(&[
            ("hook/guard.rs", site_rs()),
            (SITES_REL, list_rs(&["crate::hook::guard::POLARITY"], &["crate::hook::guard::POLARITY"])),
        ]);
        assert_sites(&measure_sites(&both_layout, &both_files).violations, "arm にも在る");
    }

    /// `impl <Type> {` の中の関連 const は `crate::<mod>::<Type>::<NAME>` で集める（現物の 3 件の形）。
    /// `impl` の頭が裸の型名でない周は path を組めない＝違反に倒す（黙って母集団から落とさない）。
    #[test]
    fn polarity_sites_reads_associated_consts_inside_impl() {
        let associated = "pub enum UsageError {\n    Http,\n}\n\nimpl UsageError {\n    pub const POLARITY: Polarity = Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailOpen };\n}\n\npub const POLARITY: Polarity = Polarity { timing: Timing::PostHoc, on_failure: OnFailure::FailClosed };\n";
        let (layout, files) = workspace(&[
            ("fleet/usage.rs", associated.to_owned()),
            (
                SITES_REL,
                list_rs(
                    &["crate::fleet::usage::POLARITY"],
                    &["crate::fleet::usage::UsageError::POLARITY"],
                ),
            ),
        ]);
        let got = measure_sites(&layout, &files);
        assert!(got.violations.is_empty(), "impl の中と直下を別の path で数える: {:?}", got.violations);
        assert_eq!(got.fact, "polarity-sites=2/1/1", "impl の中の 1 本も site に数える");
        // `impl <Trait> for <Type> {` の中の宣言は path を組めない（fail-closed）。
        let unreadable = "impl std::fmt::Display for UsageError {\n    pub const POLARITY: Polarity = Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailOpen };\n}\n";
        let (bad_layout, bad_files) = workspace(&[
            ("fleet/usage.rs", unreadable.to_owned()),
            (SITES_REL, list_rs(&[], &[])),
        ]);
        assert_sites(&measure_sites(&bad_layout, &bad_files).violations, "path を組めない");
    }
}
