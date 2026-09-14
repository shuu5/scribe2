//! `cargo xtask rules-diff --base <sha>`: rules manifest の **差分の門**
//! （設計 rules-manifest.md §4.3・憲法 C5 の「変える」側・監査 2026-09-12 塊 4 = `s2-07l.162`）。
//!
//! loader の拒否 5 形（§4.2）は行の**静的な形**しか見ない。`value = 150000 → 900000` を既存の
//! `ruling` のまま出す便は loader も CI も通り、AI が単独で規則の値を動かせる。C5 は「行を足す /
//! 変える」という **diff の事実**に裁定を結びつける条なので、その面は base との比較でしか測れない。
//!
//! base 側は `git show <base>:rules/manifest.toml`、HEAD 側は tracked の現物を読み、両方を
//! [`toml_lite`] で `[[rule]]` の行に分ける。判定は純関数 [`judge`] が行い（歯はこれを撃つ）、
//! I/O は [`run`] に閉じる。**数値の閾値を持たない**（母集団と件数を出すだけ）。
//!
//! **測れないを緑に化けさせない**（mutants-diff の rc 2 と同じ慣例）: base が読めない・
//! scanner が行に分けられない・`id` 重複（突合の鍵が壊れる）の周は [`Unmeasurable`] で rc 2。

use crate::toml_lite;
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, ExitCode};

/// manifest の中の path（base 側は `git show <base>:<この path>`・HEAD 側は cwd 相対の現物）。
const MANIFEST_REL: &str = "rules/manifest.toml";

/// array-of-tables の section header の字面（[`toml_lite::section_header`] は `[` を 1 つだけ剥がす）。
const RULE_HEADER: &str = "[rule";

/// 使い方（rc 2 の 1 行）。
const USAGE: &str = "usage: cargo xtask rules-diff --base <ref>";

/// `[[rule]]` 1 行分。**突合に使う 4 key と行番号だけ**を持つ（§4.3 (iv): 他の key の差は見ない）。
///
/// `value` / `enabled` は字面（trim 済み）で比べる——型は loader の門（§4.2）が別に守る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 突合の鍵。
    pub id: String,
    /// `value` の字面。
    pub value: String,
    /// `enabled` の字面。
    pub enabled: String,
    /// 裁定 id の字面（`"…"` の中身）。
    pub ruling: String,
    /// `[[rule]]` header の行番号（1 始まり・違反行の `line=` に写す）。
    pub line: usize,
}

/// 違反の形（§4.3 の (i) / (ii)・閉じた enum）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    /// (i) 両方に在る行の `value` か `enabled` が変わったのに、`ruling` が base の裁定のまま
    /// （同じ行の字面と同じ・または base の**別の行**の裁定を貼った相乗り）。
    ValueChangedRulingKept,
    /// (ii) 新設行の `ruling` が base の別の行の裁定と同じ字面（過去の裁定への相乗り）。
    NewRowReusesRuling,
}

impl Violation {
    /// 違反行に写す名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ValueChangedRulingKept => "value-changed-ruling-kept",
            Self::NewRowReusesRuling => "new-row-reuses-ruling",
        }
    }
}

/// 違反 1 件（HEAD の行で名指す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// HEAD 側の `[[rule]]` header の行番号。
    pub line: usize,
    /// 行 id。
    pub id: String,
    /// 違反の形。
    pub violation: Violation,
    /// 据え置かれた / 相乗りした裁定 id の字面。
    pub ruling: String,
}

impl Finding {
    /// stdout へ出す 1 行 `rules/manifest.toml:<line> <id> <violation> ruling=<字面>`。
    pub fn line(&self) -> String {
        format!(
            "{MANIFEST_REL}:{} {} {} ruling={}",
            self.line,
            self.id,
            self.violation.as_str(),
            self.ruling
        )
    }
}

/// 測れなかった理由（閉じた enum・rc 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasurable {
    /// base 側の manifest を git が答えない・行に分けられない・`id` が重複する。
    BaseUnreadable,
    /// HEAD 側の manifest を読めない・行に分けられない・`id` が重複する。
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

/// manifest の本文を `[[rule]]` の行に分ける。
///
/// 4 key（`id` / `value` / `enabled` / `ruling`）のどれかを欠く行と `id` の重複は `Err`（突合の
/// 鍵が壊れている＝測れない）。`ruled_at` や `kind` は読まない（§4.3 (iv)）。
pub fn rows(manifest: &str) -> Result<Vec<Row>, String> {
    let mut found: Vec<Row> = Vec::new();
    let mut open: Option<Draft> = None;
    for (at, text) in manifest.lines().enumerate() {
        if let Some(header) = toml_lite::section_header(text) {
            if let Some(draft) = open.take() {
                found.push(draft.close()?);
            }
            if header == RULE_HEADER {
                open = Some(Draft::at(at.saturating_add(1)));
            }
            continue;
        }
        if let (Some((key, value)), Some(draft)) = (toml_lite::key_value(text), open.as_mut()) {
            draft.take(key, value);
        }
    }
    if let Some(draft) = open.take() {
        found.push(draft.close()?);
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for row in &found {
        if !seen.insert(&row.id) {
            return Err(format!("id が重複する: {} (line={})", row.id, row.line));
        }
    }
    Ok(found)
}

/// 読み取り途中の 1 行（4 key が揃って [`Row`] になる）。
struct Draft {
    line: usize,
    id: Option<String>,
    value: Option<String>,
    enabled: Option<String>,
    ruling: Option<String>,
}

impl Draft {
    fn at(line: usize) -> Self {
        Self { line, id: None, value: None, enabled: None, ruling: None }
    }

    /// `key = value` を 1 つ受ける（見ない key は捨てる）。
    fn take(&mut self, key: &str, value: &str) {
        match key {
            "id" => self.id = Some(toml_lite::quoted(value).unwrap_or_else(|| value.to_owned())),
            "value" => self.value = Some(value.to_owned()),
            "enabled" => self.enabled = Some(value.to_owned()),
            "ruling" => self.ruling = Some(toml_lite::quoted(value).unwrap_or_else(|| value.to_owned())),
            _ => {}
        }
    }

    fn close(self) -> Result<Row, String> {
        let missing = |name: &str| format!("line={} の行に {name} が無い", self.line);
        Ok(Row {
            id: self.id.ok_or_else(|| missing("id"))?,
            value: self.value.ok_or_else(|| missing("value"))?,
            enabled: self.enabled.ok_or_else(|| missing("enabled"))?,
            ruling: self.ruling.ok_or_else(|| missing("ruling"))?,
            line: self.line,
        })
    }
}

/// 同じ id の base 行。
fn base_row<'a>(base: &'a [Row], id: &str) -> Option<&'a Row> {
    base.iter().find(|row| row.id == id)
}

/// base の**別の行**（id が違う行）が同じ裁定 id を持つか（相乗りの検査）。
fn other_base_row_rules(base: &[Row], head: &Row) -> bool {
    base.iter().any(|row| row.id != head.id && row.ruling == head.ruling)
}

/// HEAD の行が「変わった / 新設」の母集団（`changed=`）に入るか。
///
/// 両方に在る行は `value` か `enabled` の字面差（`ruled_at` だけの打ち直しは変化に数えない）、
/// base に無い行は新設として数える。
fn is_changed(base: &[Row], head: &Row) -> bool {
    match base_row(base, &head.id) {
        Some(before) => before.value != head.value || before.enabled != head.enabled,
        None => true,
    }
}

/// `changed=` の母集団＝(i) の変わった行 + (ii) の新設行。
pub fn changed(base: &[Row], head: &[Row]) -> usize {
    head.iter().filter(|row| is_changed(base, row)).count()
}

/// 判定（純関数・I/O を持たない）。列挙順は HEAD の行順。
///
/// (i) 変わった行: `ruling` が同じ行の base の字面と同じ・または base の別の行の裁定と同じなら
/// [`Violation::ValueChangedRulingKept`]。(ii) 新設行: `ruling` が base のどれかの行の裁定と同じなら
/// [`Violation::NewRowReusesRuling`]（同じ便で新設した複数の行が 1 つの**新しい**裁定を共有する形は
/// base に無いので違反にならない）。(iii) base にだけ在る行は見ない。
pub fn judge(base: &[Row], head: &[Row]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for row in head {
        let violation = match base_row(base, &row.id) {
            Some(before) => {
                let changed = before.value != row.value || before.enabled != row.enabled;
                let kept = before.ruling == row.ruling || other_base_row_rules(base, row);
                (changed && kept).then_some(Violation::ValueChangedRulingKept)
            }
            None => other_base_row_rules(base, row).then_some(Violation::NewRowReusesRuling),
        };
        if let Some(violation) = violation {
            findings.push(Finding {
                line: row.line,
                id: row.id.clone(),
                violation,
                ruling: row.ruling.clone(),
            });
        }
    }
    findings
}

/// 判定行（stdout の最終行）。
pub fn summary_line(base: &str, rows: usize, changed: usize, violations: usize) -> String {
    format!("rules-diff: base={base} rows={rows} changed={changed} violations={violations}")
}

/// 「測れなかった」を表す rc（0 にも 1 にも化けさせない第 3 の値）。
fn unmeasurable(reason: Unmeasurable) -> ExitCode {
    crate::emit_err(&format!("rules-diff: unmeasurable reason={}", reason.as_str()));
    ExitCode::from(2)
}

/// flag の値を取る（値が無ければ `None`）。
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at.saturating_add(1))
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
}

/// base 側の manifest の本文（`git show <base>:rules/manifest.toml`）。
fn base_manifest(root: &Path, base: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("show")
        .arg(format!("{base}:{MANIFEST_REL}"))
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// `rules-diff` の入口。
pub fn run(args: &[String]) -> ExitCode {
    let Some(base) = flag(args, "--base") else {
        crate::emit_err(USAGE);
        return ExitCode::from(2);
    };
    let Ok(root) = std::env::current_dir() else {
        return unmeasurable(Unmeasurable::HeadUnreadable);
    };
    let base_rows = match base_manifest(&root, base).map(|text| rows(&text)) {
        Some(Ok(found)) => found,
        Some(Err(reason)) => {
            crate::emit_err(&format!("rules-diff: base の manifest: {reason}"));
            return unmeasurable(Unmeasurable::BaseUnreadable);
        }
        None => return unmeasurable(Unmeasurable::BaseUnreadable),
    };
    let head_rows = match std::fs::read_to_string(root.join(MANIFEST_REL)).map(|text| rows(&text)) {
        Ok(Ok(found)) => found,
        Ok(Err(reason)) => {
            crate::emit_err(&format!("rules-diff: HEAD の manifest: {reason}"));
            return unmeasurable(Unmeasurable::HeadUnreadable);
        }
        Err(_) => return unmeasurable(Unmeasurable::HeadUnreadable),
    };
    let findings = judge(&base_rows, &head_rows);
    for finding in &findings {
        crate::emit(&finding.line());
    }
    crate::emit(&summary_line(base, head_rows.len(), changed(&base_rows, &head_rows), findings.len()));
    if findings.is_empty() {
        return ExitCode::SUCCESS;
    }
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{changed, judge, rows, summary_line, Finding, Violation};

    /// base 側の manifest（**実在しない** id と裁定 id＝fixture の字面が入力と衝突しない）。
    const BASE: &str = "schema = 1\n\
\n\
[[rule]]\n\
id = \"probe.alpha\"\n\
kind = \"ProbeAlpha\"\n\
value = 150000\n\
enabled = true\n\
ruling = \"probe ruling A-7f3\"\n\
ruled_at = \"2026-09-07\"\n\
\n\
[[rule]]\n\
id = \"probe.beta\"\n\
kind = \"ProbeBeta\"\n\
value = \"planner\"\n\
enabled = true\n\
ruling = \"probe ruling B-9c1\"\n\
ruled_at = \"2026-09-09\"\n\
\n\
[[rule]]\n\
id = \"probe.gone\"\n\
kind = \"ProbeGone\"\n\
value = 3\n\
enabled = false\n\
ruling = \"probe ruling G-2b8\"\n\
ruled_at = \"2026-09-09\"\n";

    /// 新設行（`ruling` は呼び手が差し込む）。
    fn new_row(id: &str, ruling: &str) -> String {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"ProbeNew\"\nvalue = 9\nenabled = true\nruling = \"{ruling}\"\nruled_at = \"2026-09-14\"\n")
    }

    fn findings_of(head: &str) -> Vec<Finding> {
        judge(&rows(BASE).expect("base fixture は読める"), &rows(head).expect("head fixture は読める"))
    }

    #[test]
    fn rules_diff_flags_value_change_that_keeps_the_ruling() {
        // (a) 同じ行で `value` が変わり `ruling` が同じ → 1 件・行番号は HEAD の `[[rule]]` header。
        let head = BASE.replace("value = 150000", "value = 900000");
        let found = findings_of(&head);
        assert_eq!(
            found,
            vec![Finding {
                line: 3,
                id: "probe.alpha".to_owned(),
                violation: Violation::ValueChangedRulingKept,
                ruling: "probe ruling A-7f3".to_owned(),
            }]
        );
        assert_eq!(
            found.first().map(Finding::line),
            Some("rules/manifest.toml:3 probe.alpha value-changed-ruling-kept ruling=probe ruling A-7f3".to_owned())
        );
    }

    #[test]
    fn rules_diff_accepts_value_change_with_a_new_ruling() {
        // (b) `value` が変わり `ruling` も変わる → 0 件（新しい裁定で値を動かす通常形）。
        let head = BASE
            .replace("value = 150000", "value = 900000")
            .replace("probe ruling A-7f3", "probe ruling A2-1e5");
        assert_eq!(findings_of(&head), Vec::new());
        // 変えた `ruling` が base の**別の行**の裁定と同じ字面なら相乗り＝違反（§4.3 (i)）。
        let borrowed = BASE
            .replace("value = 150000", "value = 900000")
            .replace("probe ruling A-7f3", "probe ruling B-9c1");
        let found = findings_of(&borrowed);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found.first().map(|f| f.violation), Some(Violation::ValueChangedRulingKept));
    }

    #[test]
    fn rules_diff_flags_enabled_flip_that_keeps_the_ruling() {
        // (c) `enabled` だけ変わり `ruling` 同じ → 1 件（発効の切替も「変える」）。
        let head = BASE.replace("value = \"planner\"\nenabled = true", "value = \"planner\"\nenabled = false");
        let found = findings_of(&head);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found.first().map(|f| f.id.as_str()), Some("probe.beta"));
        assert_eq!(found.first().map(|f| f.violation), Some(Violation::ValueChangedRulingKept));
    }

    #[test]
    fn rules_diff_ignores_ruled_at_only_edits() {
        // (d) `ruled_at` だけ変わる → 0 件・母集団にも数えない（裁定の証拠は id の側）。
        let head = BASE.replace("ruled_at = \"2026-09-07\"", "ruled_at = \"2026-09-14\"");
        let base_rows = rows(BASE).expect("base fixture は読める");
        let head_rows = rows(&head).expect("head fixture は読める");
        assert_eq!(judge(&base_rows, &head_rows), Vec::new());
        assert_eq!(changed(&base_rows, &head_rows), 0);
    }

    #[test]
    fn rules_diff_flags_new_row_that_reuses_a_base_ruling() {
        // (e) 新設行が base の別の行の `ruling` と同じ字面 → 相乗り 1 件。
        let head = format!("{BASE}{}", new_row("probe.fresh", "probe ruling B-9c1"));
        let found = findings_of(&head);
        assert_eq!(
            found,
            vec![Finding {
                line: 27,
                id: "probe.fresh".to_owned(),
                violation: Violation::NewRowReusesRuling,
                ruling: "probe ruling B-9c1".to_owned(),
            }]
        );
        assert_eq!(
            found.first().map(Finding::line),
            Some("rules/manifest.toml:27 probe.fresh new-row-reuses-ruling ruling=probe ruling B-9c1".to_owned())
        );
    }

    #[test]
    fn rules_diff_accepts_new_rows_that_share_one_new_ruling() {
        // (f) 同じ便で新設した 2 行が 1 つの新しい `ruling` を共有 → 0 件（1 裁定で複数の値）。
        let head = format!(
            "{BASE}{}{}",
            new_row("probe.fresh", "probe ruling N-4d2"),
            new_row("probe.fresh2", "probe ruling N-4d2")
        );
        assert_eq!(findings_of(&head), Vec::new());
    }

    #[test]
    fn rules_diff_ignores_rows_that_only_base_has() {
        // (g) base にだけ在る行（除去）→ 0 件・母集団にも数えない。
        let head = BASE.replace(
            "\n[[rule]]\nid = \"probe.gone\"\nkind = \"ProbeGone\"\nvalue = 3\nenabled = false\nruling = \"probe ruling G-2b8\"\nruled_at = \"2026-09-09\"\n",
            "",
        );
        let base_rows = rows(BASE).expect("base fixture は読める");
        let head_rows = rows(&head).expect("head fixture は読める");
        assert_eq!(head_rows.len(), 2, "除去が fixture に効いている");
        assert_eq!(judge(&base_rows, &head_rows), Vec::new());
        assert_eq!(changed(&base_rows, &head_rows), 0);
    }

    #[test]
    fn rules_diff_summary_counts_match_the_population() {
        // (h) rows / changed / violations が母集団と一致（changed = (i) の変わった行 + (ii) の新設行）。
        let head = format!(
            "{}{}{}",
            BASE.replace("value = 150000", "value = 900000"),
            new_row("probe.fresh", "probe ruling B-9c1"),
            new_row("probe.fresh2", "probe ruling N-4d2")
        );
        let base_rows = rows(BASE).expect("base fixture は読める");
        let head_rows = rows(&head).expect("head fixture は読める");
        let findings = judge(&base_rows, &head_rows);
        let line = summary_line("probe-sha-5a1", head_rows.len(), changed(&base_rows, &head_rows), findings.len());
        assert_eq!(line, "rules-diff: base=probe-sha-5a1 rows=5 changed=3 violations=2");
    }

    #[test]
    fn rules_diff_cannot_measure_without_the_join_key() {
        // `id` 重複は突合の鍵を壊す・4 key を欠く行は行に分けられない → `Err`（rc 2 の側）。
        let duplicated = format!("{BASE}{}", new_row("probe.alpha", "probe ruling N-4d2"));
        assert!(rows(&duplicated).is_err(), "id 重複は測れない");
        let missing = BASE.replace("ruling = \"probe ruling A-7f3\"\n", "");
        assert!(rows(&missing).is_err(), "ruling を欠く行は測れない");
        // 正常な fixture は 3 行に分かれ、行番号は `[[rule]]` header のもの。
        let lines: Vec<usize> = rows(BASE).expect("base fixture は読める").iter().map(|r| r.line).collect();
        assert_eq!(lines, [3, 11, 19]);
    }
}
