//! `cargo xtask check` が測る閾値の**読み手**と lint 集合の置き場。
//!
//! 閾値の正本は `rules/manifest.toml` の 1 面だけである（憲法 C1・SRS FR17）。この file は
//! 値を持たず、manifest の `[[rule]]` から [`Limits`] を組み立てる（`s2-07l.163`）。lint の
//! 集合・依存の allowlist・private path の印は値でなく列なのでここに残る。test 側の期待値も
//! [`Limits::read`] で現物の manifest から機械的に作る（fixture に magic number を書かない）。

use crate::toml_lite::{quoted, sections};

/// `[[rule]]` の section header を [`sections`] が返す字面。
///
/// `sections` は `[` を 1 つだけ剥がすので、array-of-tables は `[rule` になる。
const RULE_HEADER: &str = "[rule";

/// manifest の行 id ↔ [`Limits`] の field と境界 crate の上限。`read` が要求する 12 本（欠けは Err）。
const CORE_LINES: &str = "R-C4-1";
const BOUNDARY_LINES: &str = "R-C4-5";
const FILE_LINES: &str = "R-C4-2";
const TEST_SRC_RATIO_PCT: &str = "R-C4-3";
const FN_LINES: &str = "R-C4-4.fn-lines";
const FN_COMPLEXITY: &str = "R-C4-4.complexity";
const FN_ARGS: &str = "R-C4-4.args";
const LINE_WIDTH: &str = "R-C4.line-width";
const DEP_BUDGET: &str = "R-C13-1";
const DEP_PER_PR: &str = "R-C13-1.per-pr";
const CHECK_DELTA_MS: &str = "R-C13-1.check-delta-ms";
const TMUX_TEST_THREADS: &str = "gate.tmux_test_threads";

/// manifest の行 id ↔ [`FlipLimits`] の field。`FlipLimits::read` が要求する 2 本（欠けは Err・`s2-07l.170`）。
const DOCS_ONLY_FACES: &str = "flip.docs_only_faces";
const MARKS_PER_PR: &str = "flip.marks_per_pr";

/// `cargo xtask check` / `xtask deps-delta` が比べる閾値（manifest の R-C4 / R-C13 / gate.tmux_test_threads
/// 行の読み出し）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Limits {
    /// core crate の `src` 配下 `.rs` の総行数の上限（R-C4-1）。
    pub(crate) core_lines: u64,
    /// `crates/*/src` 配下 `.rs` 1 file あたりの物理行数の上限（R-C4-2）。
    pub(crate) file_lines: u64,
    /// test 行 / src 行 の比の上限（百分率・R-C4-3）。
    pub(crate) test_src_ratio_pct: u64,
    /// 関数 1 本の行数の上限（R-C4-4.fn-lines・clippy `too-many-lines-threshold`）。
    pub(crate) fn_lines: u64,
    /// 関数 1 本の認知的複雑度の上限（R-C4-4.complexity・clippy `cognitive-complexity-threshold`）。
    pub(crate) fn_complexity: u64,
    /// 関数 1 本の引数の上限（R-C4-4.args・clippy `too-many-arguments-threshold`）。
    pub(crate) fn_args: u64,
    /// 行の数え方の幅（文字・R-C4.line-width）。R-C4-1〜3 の行数はこの幅で正規化して数える。
    pub(crate) line_width: u64,
    /// 直接依存の本数の上限（R-C13-1）。
    pub(crate) dep_budget: u64,
    /// 1 便で足してよい直接依存の本数（R-C13-1.per-pr・`xtask deps-delta` の deny の面）。
    pub(crate) dep_per_pr: u64,
    /// 依存を足した便の増分 compile 秒の検出線（R-C13-1.check-delta-ms・ms・rc に触れない = C13.5）。
    pub(crate) check_delta_ms: u64,
    /// tmux を立てる歯の同時本数（gate.tmux_test_threads・nextest の test-group `tmux` の `max-threads` の正本）。
    pub(crate) tmux_test_threads: u64,
}

impl Limits {
    /// manifest の本文から 12 値を読み、[`Limits`] の 11 値を返す（R-C4-5 も必須の行として読む＝[`Self::read_with_boundary_lines`]）。
    pub(crate) fn read(manifest_text: &str) -> Result<Self, String> {
        Self::read_with_boundary_lines(manifest_text).map(|(limits, _)| limits)
    }

    /// manifest の本文から 12 値を読み、[`Limits`] と境界 crate の src の本体の上限（R-C4-5・boundary-lines が読む）を返す。
    ///
    /// R-C4-5 を field に持たないのは、[`Limits`] を literal で組む歯（deps-delta）を動かさないためである（読み手が要求する
    /// 行は 12 本で同じ）。12 本のどれかが無い / `value` が整数でない / `enabled = true` でない周は `Err`
    /// （測れないを緑にしない・SRS FR18）。不備は**全件**を集めて 1 つの reason に畳み、
    /// 各件が行 id と（本文に在る行なら）行番号を名指す。`enabled` の省略も `false` と
    /// 同じく拒む——省略を true に埋めると書き忘れた行が黙って効く側へ倒れる。
    pub(crate) fn read_with_boundary_lines(manifest_text: &str) -> Result<(Self, u64), String> {
        let mut problems = Vec::new();
        let mut value_of = |id: &str| -> u64 {
            match int_rule(manifest_text, id) {
                Ok(value) => value,
                Err(problem) => {
                    problems.push(problem);
                    0
                }
            }
        };
        let limits = Self {
            core_lines: value_of(CORE_LINES),
            file_lines: value_of(FILE_LINES),
            test_src_ratio_pct: value_of(TEST_SRC_RATIO_PCT),
            fn_lines: value_of(FN_LINES),
            fn_complexity: value_of(FN_COMPLEXITY),
            fn_args: value_of(FN_ARGS),
            line_width: value_of(LINE_WIDTH),
            dep_budget: value_of(DEP_BUDGET),
            dep_per_pr: value_of(DEP_PER_PR),
            check_delta_ms: value_of(CHECK_DELTA_MS),
            tmux_test_threads: value_of(TMUX_TEST_THREADS),
        };
        let boundary_lines = value_of(BOUNDARY_LINES);
        if problems.is_empty() {
            Ok((limits, boundary_lines))
        } else {
            Err(format!("rules manifest の閾値を読めない: {}", problems.join("・")))
        }
    }
}

/// `cargo xtask flip-check` が読む免除経路の上限（manifest の `flip.*` 行・設計 pipeline.md §7・`s2-07l.170`）。
///
/// [`Limits`] と別の型に置くのは、[`Limits::read`] の 12 本を要求する読み手（check / deps-delta と、その歯の
/// fixture）を動かさずに、flip-check だけが要る 2 本を同じ極性（欠け・不発効・形違いは `Err`）で読むためである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlipLimits {
    /// docs-only と読む path の面（`flip.docs_only_faces`・`/` で終わる要素は接頭辞・他は path の完全一致）。
    pub(crate) docs_only_faces: Vec<String>,
    /// 1 便が足してよい札（`retroactive` / `moved`）の本数の上限（`flip.marks_per_pr`）。
    pub(crate) marks_per_pr: u64,
}

impl FlipLimits {
    /// manifest の本文から 2 値を読む。不備は**全件**を 1 つの reason に畳む（[`Limits::read`] と同じ極性）。
    pub(crate) fn read(manifest_text: &str) -> Result<Self, String> {
        let faces = list_rule(manifest_text, DOCS_ONLY_FACES);
        let marks = int_rule(manifest_text, MARKS_PER_PR);
        match (faces, marks) {
            (Ok(docs_only_faces), Ok(marks_per_pr)) => Ok(Self { docs_only_faces, marks_per_pr }),
            (faces, marks) => {
                let problems: Vec<String> = [faces.err(), marks.err()].into_iter().flatten().collect();
                Err(format!("rules manifest の flip の行を読めない: {}", problems.join("・")))
            }
        }
    }
}

/// 行 id を持つ有効な `[[rule]]` の整数 `value`。不備は行 id（と行番号）を名指す。
fn int_rule(text: &str, id: &str) -> Result<u64, String> {
    let at = line_of(text, id).map_or(String::new(), |line| format!("（{line} 行目）"));
    let Some(raw) = raw_field(text, id, "value") else {
        return Err(format!("{id} の行か value が無い{at}"));
    };
    let Ok(value) = raw.trim().parse::<u64>() else {
        return Err(format!("{id} の value が整数でない{at}: {raw}"));
    };
    enabled_rule(text, id, &at).map(|()| value)
}

/// 行 id を持つ有効な `[[rule]]` の文字列の列の `value`（空の列・列でない値は Err）。
fn list_rule(text: &str, id: &str) -> Result<Vec<String>, String> {
    let at = line_of(text, id).map_or(String::new(), |line| format!("（{line} 行目）"));
    let Some(raw) = raw_field(text, id, "value") else {
        return Err(format!("{id} の行か value が無い{at}"));
    };
    let items = crate::toml_lite::string_array(&raw);
    if !raw.trim().starts_with('[') || items.is_empty() || items.iter().any(String::is_empty) {
        return Err(format!("{id} の value が空でない文字列の列でない{at}: {raw}"));
    }
    enabled_rule(text, id, &at).map(|()| items)
}

/// 行が `enabled = true` か（省略も `false` と同じく拒む）。
fn enabled_rule(text: &str, id: &str, at: &str) -> Result<(), String> {
    match raw_field(text, id, "enabled") {
        Some(enabled) if enabled.trim() == "true" => Ok(()),
        Some(enabled) => Err(format!("{id} が enabled = true でない{at}: enabled = {enabled}")),
        None => Err(format!("{id} に enabled が無い{at}")),
    }
}

/// `id = "<id>"` の行の 1 始まりの行番号。
fn line_of(text: &str, id: &str) -> Option<usize> {
    text.lines().position(|line| {
        crate::toml_lite::key_value(line)
            .filter(|(key, _)| *key == "id")
            .and_then(|(_, value)| quoted(value))
            .as_deref()
            == Some(id)
    })
    .map(|zero_based| zero_based.saturating_add(1))
}

/// 行 id を持つ `[[rule]]` の `field` の生の値を引く（最初の 1 件）。
pub(crate) fn raw_field(text: &str, id: &str, field: &str) -> Option<String> {
    for (header, pairs) in sections(text) {
        if header != RULE_HEADER {
            continue;
        }
        let found = pairs
            .iter()
            .find(|(key, _)| *key == "id")
            .and_then(|(_, value)| quoted(value));
        if found.as_deref() != Some(id) {
            continue;
        }
        return pairs
            .iter()
            .find(|(key, _)| *key == field)
            .map(|(_, value)| (*value).to_owned());
    }
    None
}

/// 行 id を持つ `[[rule]]` の `value` を整数で引く（`enabled` は見ない・§3 突合の歯の材料）。
///
/// 呼ぶのは test 区間だけだが、[`raw_field`] と対で非 test 側に置く（`s2-07l.163`）。非 test
/// build では未使用になるので、憲法 C11 の口（理由付き expect）で dead_code だけを除く。
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "憲法 §3 の写しと突合する歯だけが呼ぶ（bd s2-07l.163・raw_field と対で src 配置）")
)]
pub(crate) fn int_value(text: &str, id: &str) -> Option<u64> {
    raw_field(text, id, "value").and_then(|value| value.trim().parse::<u64>().ok())
}

/// workspace root の `Cargo.toml` が持つべき lint の 3 つ組
/// `(section, lint 名, level)`。section は `workspace.lints.<section>` の後半。
///
/// 個数は契約に書かない（この配列が唯一の SSOT である）。`print_stdout` /
/// `print_stderr` を deny に据え置くのは、出力層に `#[expect]` を使えなくなる
/// forbid を避けるためである。逆に粒度 lint 3 本は `#[allow]` / `#[expect]`
/// による黙殺を compile error にしたいので forbid とする。
pub const REQUIRED_LINTS: &[(&str, &str, &str)] = &[
    ("rust", "unsafe_code", "forbid"),
    ("rust", "unused_must_use", "deny"),
    ("clippy", "unwrap_used", "deny"),
    ("clippy", "expect_used", "deny"),
    ("clippy", "panic", "deny"),
    ("clippy", "todo", "deny"),
    ("clippy", "unimplemented", "deny"),
    ("clippy", "unreachable", "deny"),
    ("clippy", "exit", "deny"),
    ("clippy", "indexing_slicing", "deny"),
    ("clippy", "dbg_macro", "deny"),
    ("clippy", "print_stdout", "deny"),
    ("clippy", "print_stderr", "deny"),
    ("clippy", "too_many_arguments", "forbid"),
    ("clippy", "too_many_lines", "forbid"),
    ("clippy", "cognitive_complexity", "forbid"),
    ("clippy", "allow_attributes", "deny"),
];

/// tracked file の本文に残してはならない private path 形の needle 集合。
///
/// 集合は 2 形ちょうどである（絶対 home dir の接頭形と、home dir の短縮展開記号 +
/// 区切りの 2 byte 形）。字面をこの file に置くと paths-clean が自分自身を撃つので
/// **実行時に組み立てる**（`concat!` は compile 時に連結するため source の byte 列に
/// needle が現れない）。行頭錨ではなく行中のどこに現れても違反である。
pub const PRIVATE_PATH_MARKS: &[&str] = &[concat!("/", "home", "/"), concat!("~", "/")];

/// `[dependencies]` / `[dev-dependencies]` に在ってよい依存の `(section, dep 名)`。
///
/// crate 名の字面を持たない 2 つ組であるため、同名の dep をどの crate が宣言しても
/// 区別できない（xtask に同名 dev-dep を足しても検出できないのは既知の限界であり、
/// crate 粒度の回復は leg 2 の所管である）。
pub const ALLOWED_DEPS: &[(&str, &str)] = &[
    ("dev-dependencies", "insta"),
    ("dev-dependencies", "proptest"),
];

#[cfg(test)]
mod tests {
    use super::{int_value, raw_field, FlipLimits, Limits};
    use crate::toml_lite::quoted;
    use std::path::PathBuf;

    /// manifest の本文（workspace root は この crate の 2 つ上）。
    fn manifest_text() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("manifest.toml");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()))
    }

    /// 行 id を持つ `[[rule]]` の `ruling` を引く。
    fn ruling(text: &str, id: &str) -> Option<String> {
        raw_field(text, id, "ruling").and_then(|value| quoted(&value))
    }

    /// 現物の manifest で [`Limits::read_with_boundary_lines`] が 12 値を返し、`core_lines` / `file_lines` が
    /// R-C4-1 / R-C4-2 の行の値と等しい（憲法 C14.2・const を消して読み手 1 本にした形）。
    /// flip-check の 2 行（`flip.docs_only_faces` / `flip.marks_per_pr`・`s2-07l.170`）も [`FlipLimits::read`] で
    /// 欠け無く読め、値は行の現物と等しい（読み手が要求する本数は行の本数に追随する）。
    #[test]
    fn limits_match_rules_manifest() {
        let text = manifest_text();
        let (limits, boundary_lines) = Limits::read_with_boundary_lines(&text).unwrap_or_else(|reason| panic!("{reason}"));
        // 値 ↔ 行 id の 12 対（`read` の const と同じ対応・field 名の取り違えを行 id で名指す）。
        let pairs = [
            (boundary_lines, "R-C4-5"),
            (limits.core_lines, "R-C4-1"),
            (limits.file_lines, "R-C4-2"),
            (limits.test_src_ratio_pct, "R-C4-3"),
            (limits.fn_lines, "R-C4-4.fn-lines"),
            (limits.fn_complexity, "R-C4-4.complexity"),
            (limits.fn_args, "R-C4-4.args"),
            (limits.line_width, "R-C4.line-width"),
            (limits.dep_budget, "R-C13-1"),
            (limits.dep_per_pr, "R-C13-1.per-pr"),
            (limits.check_delta_ms, "R-C13-1.check-delta-ms"),
            (limits.tmux_test_threads, "gate.tmux_test_threads"),
        ];
        assert_eq!(pairs.len(), 12, "読む値の個数（`.600` で +1〔R-C4-5〕）");
        for (field, id) in pairs {
            assert_eq!(Some(field), int_value(&text, id), "{id} の行と field");
        }
        // 12 値はどれも 0 ではない（`read` が不備を 0 で埋めて Ok に化けていない）。
        assert!(boundary_lines > 0, "上限 0 は境界 crate の本体を 1 行も許さない");
        assert!(limits.core_lines > 0 && limits.file_lines > 0 && limits.dep_budget > 0, "{limits:?}");
        assert!(limits.dep_per_pr > 0 && limits.check_delta_ms > 0, "0 は依存を 1 本も足せない: {limits:?}");
        assert!(limits.line_width > 0, "幅 0 は数え方を縮退させる: {limits:?}");
        assert!(limits.tmux_test_threads > 0, "同時本数 0 は tmux の歯を 1 本も走らせない: {limits:?}");
        let flip = FlipLimits::read(&text).unwrap_or_else(|reason| panic!("{reason}"));
        assert_eq!(Some(flip.marks_per_pr), int_value(&text, "flip.marks_per_pr"), "flip.marks_per_pr の行と field");
        let raw = raw_field(&text, "flip.docs_only_faces", "value").unwrap_or_default();
        assert_eq!(flip.docs_only_faces, crate::toml_lite::string_array(&raw), "flip.docs_only_faces の行と field");
        assert!(flip.marks_per_pr > 0 && !flip.docs_only_faces.is_empty(), "{flip:?}");
        // 2 行のどちらを欠いても・不発効にしても Err が行 id を名指す（0 や空で埋めて Ok に化けない）。
        for id in ["flip.docs_only_faces", "flip.marks_per_pr"] {
            let dropped = text.replace(&format!("id = \"{id}\""), "id = \"flip.renamed\"");
            assert!(FlipLimits::read(&dropped).err().is_some_and(|reason| reason.contains(id)), "{id} の欠け");
        }
        let empty = text.replace(&raw, "[]");
        assert!(FlipLimits::read(&empty).err().is_some_and(|reason| reason.contains("flip.docs_only_faces")), "空の列");
    }

    /// 12 行の読み手用 fixture。`drop` に与えた行だけ `value` を落とし、`disabled` の行は
    /// `enabled = false` にする。
    fn limits_fixture(drop: Option<&str>, disabled: Option<&str>) -> String {
        let rows = [
            ("R-C4-1", 40_000),
            ("R-C4-5", 316),
            ("R-C4-2", 1_500),
            ("R-C4-3", 100),
            ("R-C4-4.fn-lines", 60),
            ("R-C4-4.complexity", 15),
            ("R-C4-4.args", 5),
            ("R-C4.line-width", 120),
            ("R-C13-1", 12),
            ("R-C13-1.per-pr", 1),
            ("R-C13-1.check-delta-ms", 300),
            ("gate.tmux_test_threads", 1),
        ];
        let mut text = "schema = 1\n".to_owned();
        for (id, value) in rows {
            text.push_str(&format!("\n[[rule]]\nid = \"{id}\"\nkind = \"X\"\n"));
            if drop != Some(id) {
                text.push_str(&format!("value = {value}\n"));
            }
            let enabled = if disabled == Some(id) { "false" } else { "true" };
            text.push_str(&format!("enabled = {enabled}\nruling = \"fixture\"\nruled_at = \"2026-09-14\"\n"));
        }
        text
    }

    /// 現物の manifest の R-C4-5（境界 crate の src の本体の上限・kind `BoundaryLines`・設計 core-boundary.md §9 行 i）を
    /// [`Limits::read_with_boundary_lines`] が行の値のまま返し、行を欠いた・不発効にした manifest は [`Limits::read`] も
    /// `Err` で R-C4-5 を名指す（12 本目の必須の行・他の 11 本と同じ極性）。
    #[test]
    fn limits_read_carries_the_boundary_lines_row() {
        let text = manifest_text();
        let (_, boundary_lines) = Limits::read_with_boundary_lines(&text).unwrap_or_else(|reason| panic!("{reason}"));
        assert_eq!(Some(boundary_lines), int_value(&text, "R-C4-5"), "R-C4-5 の行の値");
        assert_eq!(raw_field(&text, "R-C4-5", "kind").and_then(|kind| quoted(&kind)).as_deref(), Some("BoundaryLines"), "kind");
        let dropped = text.replace("id = \"R-C4-5\"", "id = \"R-C4-9\"");
        assert!(Limits::read(&dropped).err().is_some_and(|reason| reason.contains("R-C4-5")), "行の欠けは Err");
        let fixture = limits_fixture(None, Some("R-C4-5"));
        assert!(
            Limits::read(&fixture).err().is_some_and(|reason| reason.contains("R-C4-5") && reason.contains("enabled")),
            "不発効は Err"
        );
        let whole = Limits::read_with_boundary_lines(&limits_fixture(None, None)).map(|(_, value)| value);
        assert_eq!(whole, Ok(316), "fixture の値");
    }

    /// 12 行そろった fixture は読め、`value = 60` を欠いた fixture は `Err` が行 id を名指す。
    #[test]
    fn limits_read_names_the_row_missing_its_value() {
        let whole = Limits::read(&limits_fixture(None, None)).unwrap_or_else(|reason| panic!("{reason}"));
        assert_eq!(whole.fn_lines, 60);
        assert_eq!(whole.dep_budget, 12);
        assert_eq!((whole.dep_per_pr, whole.check_delta_ms), (1, 300));
        assert_eq!(whole.tmux_test_threads, 1);
        let missing = Limits::read(&limits_fixture(Some("R-C4-4.fn-lines"), None));
        let reason = missing.err().unwrap_or_default();
        assert!(reason.contains("R-C4-4.fn-lines"), "欠いた行 id を名指す: {reason}");
        assert!(!reason.contains("R-C4-1"), "他の行は名指さない: {reason}");
        // 行そのものが無い形も同じ（12 本のどれかが無いは Err）。
        let dropped = limits_fixture(None, None).replace("id = \"R-C13-1\"", "id = \"R-C13-9\"");
        assert!(Limits::read(&dropped).err().is_some_and(|reason| reason.contains("R-C13-1")));
    }

    /// `enabled = false` の行は `Err`（発効は書かれた事実・省略も同じ側へ倒す）。
    #[test]
    fn limits_read_refuses_a_disabled_row() {
        let disabled = Limits::read(&limits_fixture(None, Some("R-C4-3")));
        let reason = disabled.err().unwrap_or_default();
        assert!(reason.contains("R-C4-3") && reason.contains("enabled"), "{reason}");
        let omitted = limits_fixture(None, None).replacen("enabled = true\n", "", 1);
        assert!(Limits::read(&omitted).err().is_some_and(|reason| reason.contains("R-C4-1")), "省略も Err");
        let text = limits_fixture(None, None).replace("value = 1500", "value = \"1500\"");
        assert!(Limits::read(&text).err().is_some_and(|reason| reason.contains("R-C4-2")), "整数でない value");
    }

    /// 憲法 §3 の閾値セル（写し）を持つ file。
    fn constitution_text() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("design-intent")
            .join("spec")
            .join("constitution.html");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()))
    }

    /// `<tr id="…">` の 4 列目（初期値）を tag を剥がして返す。
    ///
    /// **無い行を黙って飛ばさない**——行が消えた周に「一致した」と言わせないためで、
    /// 見つからない / 列が足りないはいずれも panic（測れなかったを緑にしない）。
    fn initial_cell(html: &str, id: &str) -> String {
        let head = format!("<tr id=\"{id}\">");
        // **最初の 1 件で引かない**。元の行をコメントで殺して生きた行を書き換える形と、
        // 同じ id の行を 2 本目として足す形は、どちらも「先頭の 1 件」だけを見る歯を
        // 素通りする（実測: どちらも rc 0 で、描画される §3 の値だけが変わる）。
        let hits = html.matches(&head).count();
        assert_eq!(hits, 1, "§3 の {id} の行が 1 本でない");
        let at = html
            .find(&head)
            .unwrap_or_else(|| panic!("§3 に {id} の行が無い"));
        // 行の終わりは**次の `<tr` の手前**までで測る。`</tr>` を全文から探すと、
        // 当該行の閉じ tag が消えた周に**次の行の `</tr>`** で止まって緑になる
        // （表が壊れているのに「測れた」と言う形＝N2）。
        let rest = &html[at..];
        let limit = rest[head.len()..]
            .find("<tr")
            .map_or(rest.len(), |at_next| at_next + head.len());
        let row = &rest[..limit];
        let end = row
            .find("</tr>")
            .unwrap_or_else(|| panic!("{id} の行が閉じていない"));
        let cells: Vec<&str> = row[..end].split("<td>").skip(1).collect();
        let cell = cells
            .get(3)
            .unwrap_or_else(|| panic!("{id} に 4 列目（初期値）が無い"));
        strip_tags(&drop_deleted(cell))
    }

    /// 改訂 marker の**削除側**（`<del …>…</del>`）を本文ごと落とす。
    ///
    /// 憲法の改訂形は `<del class="delta">旧</del><ins class="delta">新</ins>` の対である
    /// （N4.2）。tag を剥がすだけだと旧と新の数字が両方残り、**manifest と整合した合法な
    /// 改訂が RED になる**（実測: `left: 7 right: 6`）。生きている値は `<ins>` 側なので、
    /// 削除側は本文ごと落として読む。
    fn drop_deleted(cell: &str) -> String {
        let mut out = String::new();
        let mut rest = cell;
        while let Some(at) = rest.find("<del") {
            out.push_str(&rest[..at]);
            let tail = &rest[at..];
            match tail.find("</del>") {
                Some(end) => rest = &tail[end.saturating_add("</del>".len())..],
                None => return out,
            }
        }
        out.push_str(rest);
        out
    }

    /// tag を剥がし、桁区切りの `,` を落とした本文（parser は書かない・std だけ）。
    fn strip_tags(cell: &str) -> String {
        let mut out = String::new();
        let mut inside = false;
        for ch in cell.chars() {
            match ch {
                '<' => inside = true,
                '>' => inside = false,
                ',' => {}
                _ if !inside => out.push(ch),
                _ => {}
            }
        }
        out
    }

    /// 本文の整数を**出現順**に拾う。
    fn ints(body: &str) -> Vec<u64> {
        let mut found = Vec::new();
        let mut digits = String::new();
        for ch in body.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            if !digits.is_empty() {
                let parsed = digits
                    .parse::<u64>()
                    .unwrap_or_else(|err| panic!("{digits} を整数にできない: {err}"));
                found.push(parsed);
                digits.clear();
            }
        }
        found
    }

    /// 数値 token（`0-9` と `.` の連なり）を出現順に拾う。
    fn number_tokens(body: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut token = String::new();
        for ch in body.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_digit() || (ch == '.' && !token.is_empty()) {
                token.push(ch);
                continue;
            }
            if !token.is_empty() {
                found.push(std::mem::take(&mut token));
            }
        }
        found
    }

    /// 「1.0」形の比を pct（×100）で読む。小数点以下は 2 桁までを許す。
    ///
    /// **個数も測る**——最初の token だけを読む形だと、セルに数値を 1 つ足す手編集
    /// （`1.0 以下 (2 面で測る)`）が緑で通る（他の 3 行は個数違いを FAIL にしている）。
    fn pct(body: &str) -> u64 {
        let tokens = number_tokens(body);
        assert_eq!(tokens.len(), 1, "比のセルの数値が 1 個でない: {tokens:?}");
        let token = tokens.first().cloned().unwrap_or_default();
        let (whole, fraction) = match token.split_once('.') {
            Some((left, right)) => (left.to_owned(), right.to_owned()),
            None => (token.clone(), String::new()),
        };
        assert!(fraction.len() <= 2, "小数点以下が 2 桁を超える: {token}");
        let padded = format!("{fraction:0<2}");
        let hundreds = ints(&whole).first().copied().unwrap_or_else(|| panic!("比を読めない: {body}"));
        let rest = ints(&padded).first().copied().unwrap_or(0);
        hundreds.saturating_mul(100).saturating_add(rest)
    }

    /// 憲法 §3 の初期値の裁定 id の接頭辞。§3 の閾値セルはこの裁定の値を写した列である。
    const INITIAL_RULING: &str = "RULING-v2-p1-exit-bundle";

    /// §3 の閾値セルと突合する manifest の行（セルから拾う数値の出現順）。
    const THRESHOLD_ROWS: [&str; 6] = [
        "R-C4-1",
        "R-C4-2",
        "R-C4-3",
        "R-C4-4.fn-lines",
        "R-C4-4.complexity",
        "R-C4-4.args",
    ];

    /// §3 の閾値セル（初期値）と manifest の行を突合し、**裁定の無い差**を返す。
    ///
    /// 現在値の正本は manifest の行で、§3 は初期値の列である。初期値と違う値の行は、
    /// ruling が初期の裁定（[`INITIAL_RULING`]）でない＝新しい裁定で改めた行だけを通す。
    /// 値を変えて ruling を初期のまま残した行（手編集）は差として返す。
    fn unruled_drift(html: &str, text: &str) -> Vec<String> {
        let mut cells: Vec<u64> = Vec::new();
        cells.extend(ints(&initial_cell(html, "r-c4-1")));
        cells.extend(ints(&initial_cell(html, "r-c4-2")));
        cells.push(pct(&initial_cell(html, "r-c4-3")));
        cells.extend(ints(&initial_cell(html, "r-c4-4")));
        assert_eq!(cells.len(), THRESHOLD_ROWS.len(), "§3 の閾値セルから拾えた数値: {cells:?}");
        let mut drift = Vec::new();
        for (cell, id) in cells.iter().zip(THRESHOLD_ROWS) {
            let value = int_value(text, id).unwrap_or_else(|| panic!("manifest に {id} が無い"));
            if value == *cell {
                continue;
            }
            let ruled = ruling(text, id).unwrap_or_else(|| panic!("manifest の {id} に ruling が無い"));
            if ruled.starts_with(INITIAL_RULING) {
                drift.push(format!("{id}: §3 = {cell}・manifest = {value}・ruling = {ruled}"));
            }
        }
        drift
    }

    /// 憲法 §3 の閾値セルは manifest の初期値の写しである（裁定なしの手編集は RED）。
    ///
    /// 値の正本は manifest 側で、憲法は読む人のための初期値の列である。3 面目のこの写しを
    /// 手で書き換えても self-test も folio も緑のままだったので、ここで突合する。
    // flip-check: retroactive s2-07l.8
    #[test]
    fn constitution_thresholds_match_rules_manifest() {
        let drift = unruled_drift(&constitution_text(), &manifest_text());
        assert!(drift.is_empty(), "憲法 §3 の閾値セルと rules manifest の値: {drift:?}");
    }

    /// §3 の 4 行の fixture（初期値 = 20,000 / 1,500 / 1.0 / 60・15・5）。
    const HTML_FIXTURE: &str = concat!(
        "<tr id=\"r-c4-1\"><td>R-C4-1</td><td>C4</td><td>core</td><td>20,000 行 以下</td></tr>\n",
        "<tr id=\"r-c4-2\"><td>R-C4-2</td><td>C4</td><td>module</td><td>1,500 行 以下</td></tr>\n",
        "<tr id=\"r-c4-3\"><td>R-C4-3</td><td>C4</td><td>ratio</td><td>1.0 以下</td></tr>\n",
        "<tr id=\"r-c4-4\"><td>R-C4-4</td><td>C4</td><td>fn</td><td>関数 60 行・複雑度 15・引数 5</td></tr>\n",
    );

    /// 6 行の manifest fixture。R-C4-1 だけ値と ruling を差し替える。
    fn manifest_fixture(core_lines: u64, core_ruling: &str) -> String {
        let rows = [
            ("R-C4-1", core_lines, core_ruling),
            ("R-C4-2", 1500, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-3", 100, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-4.fn-lines", 60, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-4.complexity", 15, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-4.args", 5, "RULING-v2-p1-exit-bundle 論点 2"),
        ];
        let mut text = "schema = 1\n".to_owned();
        for (id, value, ruled) in rows {
            text.push_str(&format!(
                "\n[[rule]]\nid = \"{id}\"\nvalue = {value}\nruling = \"{ruled}\"\n"
            ));
        }
        text
    }

    /// 値を変え ruling を新しい裁定にした行は通る（初期値のままの行も通る）。
    // flip-check: retroactive s2-07l.191
    #[test]
    fn constitution_drift_passes_rows_changed_under_a_new_ruling() {
        let initial = manifest_fixture(20_000, "RULING-v2-p1-exit-bundle 論点 2");
        assert_eq!(unruled_drift(HTML_FIXTURE, &initial), Vec::<String>::new(), "初期値のまま");
        let ruled = manifest_fixture(26_000, "user 2026-09-13T07:08Z");
        assert_eq!(unruled_drift(HTML_FIXTURE, &ruled), Vec::<String>::new(), "新しい裁定で改めた行");
    }

    /// 値を変え ruling を初期の裁定のまま残した行（裁定なしの手編集）は差として返る。
    #[test]
    fn constitution_drift_flags_rows_changed_under_the_initial_ruling() {
        let unruled = manifest_fixture(26_000, "RULING-v2-p1-exit-bundle 論点 2");
        assert_eq!(
            unruled_drift(HTML_FIXTURE, &unruled),
            vec!["R-C4-1: §3 = 20000・manifest = 26000・ruling = RULING-v2-p1-exit-bundle 論点 2".to_owned()]
        );
    }
}
