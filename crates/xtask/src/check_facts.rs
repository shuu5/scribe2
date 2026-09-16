//! `cargo xtask check` の**宣言 file を読む measure**（manifest-name / manifest-version /
//! lints-set / lints-optin / deps-empty / toolchain-pin / contracts-schema）。母集団は `Cargo.toml` /
//! `rust-toolchain.toml` / 生成物 manifest / 契約表の欄の生成物である。
//!
//! `check.rs` から分けたのは憲法 C4（1 file の上限）のためで、**測る内容は 1 つも変えていない**
//! （`s2-07l.84`・純粋な移動）。判定行の名前・順序・値の書式は不変である。

use crate::check::{failed, json_string_field, read_text, Layout, Measured};
use crate::genmanifest::MANIFEST_REL;
use crate::limits::{Limits, ALLOWED_DEPS, REQUIRED_LINTS};
use crate::toml_lite::{entries_in, key_value, lint_level, quoted, sections};
use crate::workspace::read_dir_sorted;
use std::collections::BTreeSet;
use std::path::Path;

/// plugin manifest と core crate の突き合わせ（manifest-name / manifest-version）。
pub(crate) fn measure_manifests(layout: &Layout) -> Vec<Measured> {
    let plugin = match read_text(&layout.root.join(MANIFEST_REL)) {
        Ok(text) => text,
        Err(reason) => {
            return vec![
                failed("manifest-name", &reason),
                failed("manifest-version", &reason),
            ]
        }
    };
    vec![
        agreement(
            "manifest-name",
            &[
                ("plugin.json", json_string_field(&plugin, "name")),
                ("Cargo.toml", layout.core_package_field("name").ok()),
                ("name.rs", Some(layout.name.clone())),
            ],
        ),
        agreement(
            "manifest-version",
            &[
                ("plugin.json", json_string_field(&plugin, "version")),
                ("Cargo.toml", layout.core_version().ok()),
            ],
        ),
    ]
}

/// 与えた出所の値がすべて同一であることを測る。
fn agreement(tag: &str, sources: &[(&str, Option<String>)]) -> Measured {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut missing = Vec::new();
    for (label, value) in sources {
        match value {
            Some(found) => {
                seen.insert(found.clone());
            }
            None => missing.push(*label),
        }
    }
    let listed: Vec<String> = seen.iter().cloned().collect();
    let fact = format!("{tag}={}", listed.join("|"));
    if !missing.is_empty() {
        let reason = format!("{} から値を読めない", missing.join(", "));
        return Measured {
            fact,
            violations: vec![format!("{tag}: {reason}")],
        };
    }
    if listed.len() > 1 {
        return Measured {
            fact,
            violations: vec![format!("{tag}: {}", listed.join(" != "))],
        };
    }
    Measured {
        fact,
        violations: Vec::new(),
    }
}

/// root manifest の lint 集合（lints-set）と member 側 opt-in（lints-optin）。
pub(crate) fn measure_lints(layout: &Layout) -> Vec<Measured> {
    let root_manifest = match read_text(&layout.root.join("Cargo.toml")) {
        Ok(text) => text,
        Err(reason) => {
            return vec![failed("lints-set", &reason), failed("lints-optin", &reason)]
        }
    };
    vec![measure_lints_set(&root_manifest), measure_lints_optin(layout)]
}

/// root manifest が宣言している `(section, lint, level)` の 3 つ組集合。
fn declared_lints(manifest: &str) -> BTreeSet<(String, String, String)> {
    let mut found = BTreeSet::new();
    for section in ["rust", "clippy"] {
        for (key, value) in entries_in(manifest, &format!("workspace.lints.{section}")) {
            if let Some(level) = lint_level(value) {
                found.insert((section.to_owned(), key.to_owned(), level));
            }
        }
    }
    found
}

/// root manifest の lint 集合が [`REQUIRED_LINTS`] と一致すること（lints-set）。
fn measure_lints_set(manifest: &str) -> Measured {
    let declared = declared_lints(manifest);
    let required: BTreeSet<(String, String, String)> = REQUIRED_LINTS
        .iter()
        .map(|(section, lint, level)| {
            ((*section).to_owned(), (*lint).to_owned(), (*level).to_owned())
        })
        .collect();
    let mut violations = Vec::new();
    for (section, lint, level) in required.difference(&declared) {
        violations.push(format!(
            "lints-set: {section}.{lint} = \"{level}\" が root Cargo.toml に無い"
        ));
    }
    for (section, lint, level) in declared.difference(&required) {
        violations.push(format!(
            "lints-set: {section}.{lint} = \"{level}\" は REQUIRED_LINTS に無い"
        ));
    }
    Measured {
        fact: format!("lints-set={}", declared.len()),
        violations,
    }
}

/// 各 member が `[lints] workspace = true` を持つこと（lints-optin）。
///
/// これが無いと `[workspace.lints]` は member に一切適用されず、clippy も
/// nextest も全部緑のまま歯が死ぬ。
fn measure_lints_optin(layout: &Layout) -> Measured {
    let mut violations = Vec::new();
    let mut opted = 0;
    for dir in &layout.member_dirs {
        let path = dir.join("Cargo.toml");
        match read_text(&path) {
            Ok(text) if has_workspace_lints(&text) => opted += 1,
            Ok(_) => violations.push(format!(
                "lints-optin: {} に [lints] workspace = true が無い（workspace.lints が不活性になる）",
                path.display()
            )),
            Err(reason) => violations.push(format!("lints-optin: {reason}")),
        }
    }
    Measured {
        fact: format!("lints-optin={opted}/{}", layout.member_dirs.len()),
        violations,
    }
}

/// member manifest が `[lints] workspace = true` を持つか。
fn has_workspace_lints(manifest: &str) -> bool {
    entries_in(manifest, "lints")
        .iter()
        .any(|(key, value)| *key == "workspace" && *value == "true")
}

/// root と全 member の直接依存が [`ALLOWED_DEPS`] の内側であること（deps-empty）。
///
/// 中身は allowlist だが measure tag の名は ADR-0002 §2.4 が凍結しているので
/// `deps-empty` に据え置く。
pub(crate) fn measure_deps_empty(layout: &Layout) -> Measured {
    let mut manifests = vec![layout.root.join("Cargo.toml")];
    manifests.extend(layout.member_dirs.iter().map(|dir| dir.join("Cargo.toml")));
    let mut violations = Vec::new();
    for path in &manifests {
        match read_text(path) {
            Ok(text) => violations.extend(declared_deps(&text, path)),
            Err(reason) => violations.push(format!("deps-empty: {reason}")),
        }
    }
    Measured {
        fact: format!("deps-empty={}", manifests.len()),
        violations,
    }
}

/// 1 つの manifest が宣言している allowlist 外の直接依存を違反行に写す。
///
/// section 名の完全一致では足りない。`[dependencies.<name>]` の入れ子形、
/// `[build-dependencies]`、`[target.'cfg(unix)'.dependencies]`、
/// `[workspace.dependencies]` のいずれも直接依存を 1 本増やすからである。
fn declared_deps(manifest: &str, path: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for (header, pairs) in sections(manifest) {
        let Some((section, nested)) = dep_section(header) else {
            continue;
        };
        match nested {
            Some(dep) => {
                let renamed = pairs.iter().any(|(key, _)| *key == "package");
                found.extend(dep_violation(path, header, section, dep, renamed));
            }
            None => {
                for (key, value) in pairs {
                    found.extend(dep_violation(path, header, section, key, renames_package(value)));
                }
            }
        }
    }
    found
}

/// 直接依存 1 本を測り、allowlist の外なら違反行を返す。
///
/// `package =` で別 crate へ改名した entry は key 名が allowlist に在っても違反である
/// （さもないと `insta = { package = "other" }` が allowlist を素通りする）。
fn dep_violation(
    path: &Path,
    header: &str,
    section: &str,
    dep: &str,
    renamed: bool,
) -> Option<String> {
    if !renamed && is_allowed_dep(section, dep) {
        return None;
    }
    let reason = if renamed {
        "package = で別 crate へ改名している（allowlist は key 名では通さない）"
    } else {
        "allowlist 外の直接依存"
    };
    Some(format!(
        "deps-empty: {} の [{header}] に {dep} が在る（{reason}）",
        path.display()
    ))
}

/// 直接依存を宣言しうる section の base 名。`workspace.` / `target.<spec>.` の前置は
/// 剥がしてから照合する。
const DEP_SECTION_BASES: &[&str] = &["dependencies", "dev-dependencies", "build-dependencies"];

/// section header が依存 section なら `(base 名, `[<base>.<name>]` 形の dep 名)`。
fn dep_section(header: &str) -> Option<(&'static str, Option<&str>)> {
    let scoped = strip_target_scope(header.strip_prefix("workspace.").unwrap_or(header));
    for base in DEP_SECTION_BASES {
        if scoped == *base {
            return Some((base, None));
        }
        let nested = scoped
            .strip_prefix(*base)
            .and_then(|rest| rest.strip_prefix('.'))
            .filter(|dep| !dep.is_empty() && !dep.contains('.'));
        if let Some(dep) = nested {
            return Some((base, Some(dep)));
        }
    }
    None
}

/// `target.<spec>.` の前置を剥がす。`<spec>` は quote 内に `.` を含みうるので、
/// dot 分割ではなく base 名の直前の `.` を探して切る。
fn strip_target_scope(header: &str) -> &str {
    let Some(after) = header.strip_prefix("target.") else {
        return header;
    };
    for base in DEP_SECTION_BASES {
        if let Some(at) = after.find(&format!(".{base}")) {
            return after.get(at + 1..).unwrap_or(after);
        }
    }
    after
}

/// inline table 形の dep 値が `package = ` による改名を持つか。
fn renames_package(value: &str) -> bool {
    let Some(inner) = value
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return false;
    };
    inner
        .split(',')
        .filter_map(|part| part.split_once('='))
        .any(|(key, _)| key.trim() == "package")
}

/// `(section, dep 名)` が [`ALLOWED_DEPS`] に在るか。
fn is_allowed_dep(section: &str, dep: &str) -> bool {
    ALLOWED_DEPS
        .iter()
        .any(|(allowed_section, allowed_dep)| *allowed_section == section && *allowed_dep == dep)
}

/// clippy-thresholds の tag。
const CLIPPY_TAG: &str = "clippy-thresholds";

/// clippy の設定 file（workspace root からの相対）。
const CLIPPY_REL: &str = "clippy.toml";

/// `clippy.toml` の key ↔ manifest の行 id（R-C4-4.*）。実効値は clippy が持つので、
/// manifest の値との一致をここで測る（写しを第 2 正本にしない・憲法 C1 / C14.2）。
const CLIPPY_KEYS: [(&str, &str); 3] = [
    ("too-many-lines-threshold", "R-C4-4.fn-lines"),
    ("cognitive-complexity-threshold", "R-C4-4.complexity"),
    ("too-many-arguments-threshold", "R-C4-4.args"),
];

/// `clippy.toml` の 3 閾値が manifest の R-C4-4.* と同じ値であること（clippy-thresholds）。
///
/// file が無い / key が無い周は違反（読めなかったを一致に化けさせない）。
pub(crate) fn measure_clippy_thresholds(layout: &Layout, limits: &Limits) -> Measured {
    let text = match read_text(&layout.root.join(CLIPPY_REL)) {
        Ok(text) => text,
        Err(reason) => return failed(CLIPPY_TAG, &reason),
    };
    let violations = clippy_drift(&text, limits);
    let fact = if violations.is_empty() { format!("{CLIPPY_TAG}=ok") } else { format!("{CLIPPY_TAG}=drift") };
    Measured { fact, violations }
}

/// `clippy.toml` と [`Limits`] の差（違反行の列・一致なら空）。key 1 本につき多くとも 1 件。
fn clippy_drift(clippy: &str, limits: &Limits) -> Vec<String> {
    let expected = [limits.fn_lines, limits.fn_complexity, limits.fn_args];
    let mut violations = Vec::new();
    for ((key, id), want) in CLIPPY_KEYS.iter().zip(expected) {
        // `clippy.toml` は section を持たないので行ごとに `key = value` で読む。
        let found = clippy.lines().filter_map(key_value).find(|(found, _)| found == key).map(|(_, value)| value);
        match found.map(|value| (value, value.trim().parse::<u64>())) {
            Some((_, Ok(actual))) if actual == want => {}
            Some((raw, _)) => violations.push(format!("{CLIPPY_TAG}: {key} = {raw} ≠ {id} = {want}")),
            None => violations.push(format!("{CLIPPY_TAG}: {CLIPPY_REL} に {key} が無い（{id} = {want} の実効値を持たない）")),
        }
    }
    violations
}

/// nextest-tmux-group の tag。
const TMUX_TAG: &str = "nextest-tmux-group";

/// nextest の設定 file（workspace root からの相対・tmux を立てる歯の test-group の写し・設計 gate-cost.md §3.1）。
pub(crate) const NEXTEST_REL: &str = ".config/nextest.toml";

/// test-group の名。
const TMUX_GROUP: &str = "tmux";

/// 正本の行 id（manifest の `gate.tmux_test_threads`・[`Limits::tmux_test_threads`]）。
const TMUX_ROW: &str = "gate.tmux_test_threads";

/// `[[profile.default.overrides]]` の header を [`sections`] が返す字面（`[` を 1 つだけ剥がす）。
const OVERRIDE_HEADER: &str = "[profile.default.overrides";

/// tmux を立てる歯の**種**: 本文がこの字面を名指す fn（席の fixture の道具・`tests/e2e/seat.rs`）。
const TMUX_SEEDS: [&str; 2] = ["start_seat(", "IsolatedSeat"];

/// filter の固定形 `test(/^(名|名|…)$/)` の頭と尾。名は `cargo nextest list` が出す module 付きの形。
const FILTER_HEAD: &str = "test(/^(";
const FILTER_TAIL: &str = ")$/)";

/// tmux を立てる歯が nextest の test-group `tmux` に居ること（nextest-tmux-group・clippy-thresholds と同型・C10.3）:
/// (a) `.config/nextest.toml` の `test-groups.tmux.max-threads` が manifest の `gate.tmux_test_threads` と同値、
/// (b) group の filter が列挙する名の集合と、e2e の木を**関数名の固定点**で閉じた `#[test]` の集合が両向きに一致する
/// （filter に無い歯 = group の外・歯に無い名 = 幽霊・固定形でない filter は typed に断る）。
///
/// file が無い / key が無い / e2e の木を読めない周は違反（読めなかったを一致に化けさせない）。fact は母集団
/// （歯の本数と file 数）を出す。
pub(crate) fn measure_nextest_tmux_group(layout: &Layout, limits: &Limits) -> Measured {
    let text = match read_text(&layout.root.join(NEXTEST_REL)) {
        Ok(text) => text,
        Err(reason) => return failed(TMUX_TAG, &reason),
    };
    let e2e = layout.core_dir.join("tests").join("e2e");
    let files = match e2e_files(&e2e) {
        Ok(files) => files,
        Err(reason) => return failed(TMUX_TAG, &reason),
    };
    let tests = tmux_tests(&files);
    let modules: BTreeSet<&str> = tests.iter().map(|name| name.rsplit_once("::").map_or("", |(module, _)| module)).collect();
    let mut violations = threads_drift(&text, limits.tmux_test_threads);
    violations.extend(group_drift(&text, &tests));
    let state = if violations.is_empty() { "ok" } else { "drift" };
    Measured { fact: format!("{TMUX_TAG}={state} tests={} files={}", tests.len(), modules.len()), violations }
}

/// `test-groups.tmux.max-threads` と正本の値の差（多くとも 1 件・key が無い / 整数でない周も 1 件）。
fn threads_drift(config: &str, want: u64) -> Vec<String> {
    let found = entries_in(config, &format!("test-groups.{TMUX_GROUP}"))
        .into_iter()
        .find(|(key, _)| *key == "max-threads")
        .map(|(_, value)| value);
    match found.map(|value| (value, value.trim().parse::<u64>())) {
        Some((_, Ok(actual))) if actual == want => Vec::new(),
        Some((raw, _)) => vec![format!("{TMUX_TAG}: test-groups.{TMUX_GROUP}.max-threads = {raw} ≠ {TMUX_ROW} = {want}")],
        None => vec![format!(
            "{TMUX_TAG}: {NEXTEST_REL} に test-groups.{TMUX_GROUP}.max-threads が無い（{TMUX_ROW} = {want} の実効値を持たない）"
        )],
    }
}

/// filter の列挙と歯の集合の**両向き**の差（group の外の歯・幽霊の名を 1 件 1 行）。固定形でない filter はその 1 件。
fn group_drift(config: &str, tests: &BTreeSet<String>) -> Vec<String> {
    let listed = match filter_names(config) {
        Ok(names) => names,
        Err(reason) => return vec![format!("{TMUX_TAG}: {reason}")],
    };
    let mut violations: Vec<String> = tests
        .difference(&listed)
        .map(|name| format!("{TMUX_TAG}: {name} は tmux を立てる歯だが group の外（{NEXTEST_REL} の filter に無い）"))
        .collect();
    violations.extend(
        listed.difference(tests).map(|name| format!("{TMUX_TAG}: {name} は filter に在るが tmux を立てる歯に無い（幽霊）")),
    );
    violations
}

/// `test-group = "tmux"` の `[[profile.default.overrides]]` の `filter` を固定形として読む（名の集合）。
///
/// override が無い / `filter` が無い / 固定形でない（頭尾が違う・名が空・名に識別子と `::` 以外の字が在る）は `Err`
/// で、0 件の集合には化けない。
fn filter_names(config: &str) -> Result<BTreeSet<String>, String> {
    let pairs = sections(config)
        .into_iter()
        .filter(|(header, _)| *header == OVERRIDE_HEADER)
        .map(|(_, pairs)| pairs)
        .find(|pairs| pairs.iter().any(|(key, value)| *key == "test-group" && unquote(value) == Some(TMUX_GROUP)))
        .ok_or_else(|| format!("{NEXTEST_REL} に test-group = \"{TMUX_GROUP}\" の [[profile.default.overrides]] が無い"))?;
    let raw = pairs
        .iter()
        .find(|(key, _)| *key == "filter")
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("{NEXTEST_REL} の {TMUX_GROUP} の override に filter が無い"))?;
    let refused = || format!("{NEXTEST_REL} の filter が固定形 {FILTER_HEAD}名|名|…{FILTER_TAIL} でない: {raw}");
    let inner = unquote(raw)
        .and_then(|text| text.strip_prefix(FILTER_HEAD))
        .and_then(|text| text.strip_suffix(FILTER_TAIL))
        .ok_or_else(refused)?;
    let names: BTreeSet<String> = inner.split('|').map(str::to_owned).collect();
    let well_formed = |name: &String| !name.is_empty() && name.chars().all(|ch| is_ident(ch) || ch == ':');
    if names.iter().all(well_formed) {
        Ok(names)
    } else {
        Err(refused())
    }
}

/// `'…'` / `"…"` の中身（TOML の literal / basic string の 1 行形）。
fn unquote(value: &str) -> Option<&str> {
    let text = value.trim();
    ['\'', '"']
        .into_iter()
        .find_map(|quote| text.strip_prefix(quote).and_then(|rest| rest.strip_suffix(quote)))
}

/// Rust の識別子を成す文字。
fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// `crates/<core>/tests/e2e/` 配下の `.rs`（`(module の接頭辞, 本文)`・path 順）。接頭辞は path から `e2e/` と `.rs` を
/// 落とし `::` で繋いだ形（`seat/cycle.rs` → `seat::cycle`・`main.rs` → 空・`main.rs` の宣言順は使わない）。
fn e2e_files(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for entry in read_dir_sorted(root)? {
        if entry.is_dir() {
            let dir = entry.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            for (module, text) in e2e_files(&entry)? {
                out.push((if module.is_empty() { dir.clone() } else { format!("{dir}::{module}") }, text));
            }
        } else if entry.extension().is_some_and(|ext| ext == "rs") {
            let stem = entry.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            out.push((if stem == "main" { String::new() } else { stem }, read_text(&entry)?));
        }
    }
    Ok(out)
}

/// e2e の木の fn 1 本（名・module の接頭辞・`#[test]` か・本文が種を名指すか・本文が `(` を直後に持つ識別子の集合）。
struct Item {
    name: String,
    module: String,
    is_test: bool,
    seed: bool,
    calls: BTreeSet<String>,
}

/// tmux を立てる `#[test]` を**関数名の固定点**で閉じ、module 付きの名（`seat::cycle::<fn>`）で返す。
///
/// 種 = 本文が [`TMUX_SEEDS`] を名指す fn。集合の fn 名を本文で呼ぶ fn を、増えなくなるまで足す（helper 越しの歯を
/// 接頭辞や直接名指しでは拾えない・`.360` run 2 の実測）。
fn tmux_tests(files: &[(String, String)]) -> BTreeSet<String> {
    let items: Vec<Item> = files.iter().flat_map(|(module, text)| items_of(module, text)).collect();
    let mut names: BTreeSet<String> = items.iter().filter(|item| item.seed).map(|item| item.name.clone()).collect();
    loop {
        let grown: Vec<String> = items
            .iter()
            .filter(|item| !names.contains(&item.name) && !item.calls.is_disjoint(&names))
            .map(|item| item.name.clone())
            .collect();
        if grown.is_empty() {
            break;
        }
        names.extend(grown);
    }
    items
        .iter()
        .filter(|item| item.is_test && names.contains(&item.name))
        .map(|item| if item.module.is_empty() { item.name.clone() } else { format!("{}::{}", item.module, item.name) })
        .collect()
}

/// file の fn を rustfmt の形で切る: `fn <名>` の行から同じ字下げの `}` だけの行まで（1 行の fn はその行）。
/// 行頭コメント行は本文から除く（doc の字面を種に数えない）。直前の `#[test]` がその fn の印。
fn items_of(module: &str, text: &str) -> Vec<Item> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut is_test = false;
    let mut at = 0_usize;
    while let Some(line) = lines.get(at) {
        let trimmed = line.trim_start();
        if trimmed == "#[test]" {
            is_test = true;
        } else if let Some(name) = fn_name(trimmed) {
            let close = format!("{}}}", " ".repeat(line.len().saturating_sub(trimmed.len())));
            let end = if trimmed.ends_with('}') || trimmed.ends_with(';') {
                at
            } else {
                let mut after = lines.iter().enumerate().skip(at.saturating_add(1));
                after.find(|(_, rest)| **rest == close.as_str()).map_or(lines.len(), |(end, _)| end)
            };
            let span = lines.iter().skip(at).take(end.saturating_sub(at).saturating_add(1));
            let body = span.filter(|rest| !rest.trim_start().starts_with("//")).copied().collect::<Vec<&str>>().join("\n");
            let seed = TMUX_SEEDS.iter().any(|needle| body.contains(needle));
            out.push(Item { name, module: module.to_owned(), is_test, seed, calls: called_names(&body) });
            is_test = false;
            at = end;
        }
        at = at.saturating_add(1);
    }
    out
}

/// 行が fn の宣言ならその名（`pub` / `pub(crate)` / `async` / `const` / `unsafe` の前置きは任意・コメント行は見ない）。
fn fn_name(trimmed: &str) -> Option<String> {
    if trimmed.starts_with("//") {
        return None;
    }
    let (head, rest) = trimmed.split_once("fn ")?;
    if !head.split_whitespace().all(|word| matches!(word, "pub" | "async" | "const" | "unsafe") || word.starts_with("pub(")) {
        return None;
    }
    let name: String = rest.chars().take_while(|ch| is_ident(*ch)).collect();
    let follows = rest.get(name.len()..).is_some_and(|tail| tail.starts_with('(') || tail.starts_with('<'));
    (!name.is_empty() && follows).then_some(name)
}

/// 本文で `(` を直後に持つ識別子の集合（呼ぶ関数の名の候補・`.name(` の method 呼びも含む）。
fn called_names(body: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut ident = String::new();
    for ch in body.chars() {
        if is_ident(ch) {
            ident.push(ch);
            continue;
        }
        if ch == '(' && !ident.is_empty() {
            out.insert(ident.clone());
        }
        ident.clear();
    }
    out
}

/// dep-budget の tag。
const DEP_BUDGET_TAG: &str = "dep-budget";

/// [`ALLOWED_DEPS`] の本数が manifest の R-C13-1 の内側であること（dep-budget）。
pub(crate) fn measure_dep_budget(limits: &Limits) -> Measured {
    dep_budget(ALLOWED_DEPS.len(), limits.dep_budget)
}

/// allowlist の本数 `count` と上限 `max` の突合（fact は `dep-budget=<n>/<max>`）。
fn dep_budget(count: usize, max: u64) -> Measured {
    let over = u64::try_from(count).unwrap_or(u64::MAX) > max;
    let violations = if over {
        vec![format!("{DEP_BUDGET_TAG}: ALLOWED_DEPS が {count} 本で R-C13-1 = {max} を超える")]
    } else {
        Vec::new()
    };
    Measured { fact: format!("{DEP_BUDGET_TAG}={count}/{max}"), violations }
}

/// channel が浮動 channel の語を含むならその語を返す。
fn floating_word(channel: &str) -> Option<&'static str> {
    ["stable", "beta", "nightly"]
        .into_iter()
        .find(|word| channel.contains(word))
}

/// channel が版番号の字面（`major.minor.patch`・任意で `-<target-triple>` 付き）か。
///
/// 3 要素を要求するのは、`1.98` のような短い形が rustup では 1.98.x の最新へ
/// **浮動解決**され、■H1 が要求する「host に既に導入済みの toolchain 名」に
/// ならないためである。`my-custom` のような custom toolchain 名もここで落ちる。
fn is_version_literal(channel: &str) -> bool {
    let core = channel.split_once('-').map_or(channel, |(head, _)| head);
    let mut parts = 0;
    for part in core.split('.') {
        if part.is_empty() || !part.chars().all(|digit| digit.is_ascii_digit()) {
            return false;
        }
        parts += 1;
    }
    parts == 3
}

/// `rust-toolchain.toml` の channel が版番号の字面であり、かつ
/// stable / beta / nightly の語を含まないこと（toolchain-pin）。
///
/// 負の語検査だけだと `1.98` や `my-custom` が素通りするので、正の字面検査
/// （[`is_version_literal`]）と合接で測る。違反は多くとも 1 件に畳む。
pub(crate) fn measure_toolchain_pin(layout: &Layout) -> Measured {
    let path = layout.root.join("rust-toolchain.toml");
    let channel = read_text(&path).ok().and_then(|text| {
        entries_in(&text, "toolchain")
            .into_iter()
            .find(|(key, _)| *key == "channel")
            .and_then(|(_, value)| quoted(value))
    });
    let Some(channel) = channel else {
        return failed(
            "toolchain-pin",
            &format!("{} の channel を読めない", path.display()),
        );
    };
    let fact = format!("toolchain-pin={channel}");
    let violation = match floating_word(&channel) {
        Some(word) => Some(format!(
            "toolchain-pin: channel \"{channel}\" が {word} を含む（版番号で固定する）"
        )),
        None if !is_version_literal(&channel) => Some(format!(
            "toolchain-pin: channel \"{channel}\" が版番号の字面でない（major.minor.patch で固定する）"
        )),
        None => None,
    };
    Measured {
        fact,
        violations: violation.into_iter().collect(),
    }
}

/// contracts-schema の tag。
const SCHEMA_TAG: &str = "contracts-schema";

/// 契約表の欄の生成物（workspace root からの相対・core の `contracts schema` の出力）。
pub(crate) const SCHEMA_REL: &str = "contracts/schema.toml";

/// 欄の正本を持つ core の file（core crate の dir からの相対）。
pub(crate) const TABLE_SRC: &str = "src/pipe/table.rs";

/// 正本の const slice の宣言行（この行から `];` までの 1 項目 1 行を読む）。
const FIELDS_HEAD: &str = "pub const FIELDS: &[Field] = &[";

/// 欄 1 つ（名・必須 / 任意・値の形）。
type Column = (String, String, String);

/// contracts-schema（設計 contract-source.md §2・hooks.json / 極性一覧と同型）: tracked な生成物の欄の列（名・必須 /
/// 任意・値の形・順序）が core の `pipe/table.rs` の `FIELDS` と同じ列であること。
///
/// xtask は core に依存しない（ADR-0006 / ADR-0013）ので binary を撃たず、2 つの tracked file を字面で読んで比べる。
/// render と tracked の byte の一致は core の e2e（`contract_schema_`）が測る＝2 つの面を別の歯が受ける。
/// 読めない・正本の欄を 1 本も読めない周は違反に倒す（fail-closed）。
pub(crate) fn measure_contracts_schema(layout: &Layout) -> Measured {
    let faces = (read_text(&layout.root.join(SCHEMA_REL)), read_text(&layout.core_dir.join(TABLE_SRC)));
    let (schema, table) = match faces {
        (Ok(schema), Ok(table)) => (schema, table),
        (Err(reason), _) | (_, Err(reason)) => return failed(SCHEMA_TAG, &reason),
    };
    let violations = schema_drift(&schema, &table);
    let fact = if violations.is_empty() { format!("{SCHEMA_TAG}=ok") } else { format!("{SCHEMA_TAG}=drift") };
    Measured { fact, violations }
}

/// 2 面の欄の列の差（違反行の列・一致なら空）。
fn schema_drift(schema: &str, table: &str) -> Vec<String> {
    let declared = table_columns(table);
    if declared.is_empty() {
        return vec![format!(
            "{SCHEMA_TAG}: core の {TABLE_SRC} から FIELDS の欄を 1 本も読めない（読めない形を一致に化けさせない）"
        )];
    }
    let tracked = schema_columns(schema);
    if tracked == declared {
        return Vec::new();
    }
    vec![format!(
        "{SCHEMA_TAG}: {SCHEMA_REL} の欄の列が core の FIELDS と違う（core の contracts schema で描き直す）: tracked=[{}] FIELDS=[{}]",
        show(&tracked),
        show(&declared)
    )]
}

/// 生成物の `[[field]]` の列（`name` / `need` / `shape` の 3 key）。
fn schema_columns(schema: &str) -> Vec<Column> {
    let mut found: Vec<Column> = Vec::new();
    for line in schema.lines().map(str::trim) {
        if line == "[[field]]" {
            found.push(Default::default());
            continue;
        }
        let (Some(last), Some((key, value))) = (found.last_mut(), line.split_once('=')) else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        match key.trim() {
            "name" => last.0 = value,
            "need" => last.1 = value,
            "shape" => last.2 = value,
            _ => {}
        }
    }
    found
}

/// core の `FIELDS` の 1 項目 1 行（`Field { name: "id", need: Need::Required, shape: Shape::Text },`）の列。
fn table_columns(table: &str) -> Vec<Column> {
    table
        .lines()
        .skip_while(|line| line.trim() != FIELDS_HEAD)
        .skip(1)
        .take_while(|line| line.trim() != "];")
        .filter_map(|line| {
            let name = line.split_once("name: \"")?.1.split_once('"')?.0.to_owned();
            Some((name, variant(line, "Need::")?, variant(line, "Shape::")?))
        })
        .collect()
}

/// `Need::Required` の `Required` を小文字にした語（生成物の値の形）。
fn variant(line: &str, head: &str) -> Option<String> {
    let word: String = line.split_once(head)?.1.chars().take_while(char::is_ascii_alphanumeric).collect();
    (!word.is_empty()).then(|| word.to_ascii_lowercase())
}

/// 欄の列の短い表示（`name:need:shape` の `,` 区切り）。
fn show(columns: &[Column]) -> String {
    columns.iter().map(|(name, need, shape)| format!("{name}:{need}:{shape}")).collect::<Vec<String>>().join(",")
}

/// 健全な擬似 workspace の contracts-schema の 2 面（`(workspace 相対 path, 本文)`・`core` は core crate の dir 名）。
#[cfg(test)]
pub(crate) fn contracts_fixture(core: &str) -> Vec<(String, String)> {
    let table = "pub struct Field;\n\npub const FIELDS: &[Field] = &[\n    Field { name: \"id\", need: Need::Required, shape: Shape::Text },\n];\n";
    let schema = "schema = 1\n\n[[field]]\nname = \"id\"\nneed = \"required\"\nshape = \"text\"\n";
    vec![(SCHEMA_REL.to_owned(), schema.to_owned()), (format!("crates/{core}/{TABLE_SRC}"), table.to_owned())]
}

/// nextest の設定の fixture（`threads` と固定形の filter に載せる名の列）。
#[cfg(test)]
pub(crate) fn nextest_fixture(threads: u64, names: &[&str]) -> String {
    format!(
        "[test-groups.{TMUX_GROUP}]\nmax-threads = {threads}\n\n[[profile.default.overrides]]\nfilter = '{FILTER_HEAD}{}{FILTER_TAIL}'\ntest-group = '{TMUX_GROUP}'\n",
        names.join("|")
    )
}

/// 健全な擬似 workspace の e2e の木（`(core crate の dir からの相対 path, 本文)`）: 種を持つ helper と、それを呼ぶ歯 1 本
/// （`seat::seat_x`・[`nextest_fixture`] の名と対）。
#[cfg(test)]
pub(crate) fn e2e_fixture() -> Vec<(&'static str, &'static str)> {
    vec![
        ("tests/e2e/main.rs", "mod seat;\n\n#[test]\nfn plain_x() {\n    assert!(true);\n}\n"),
        (
            "tests/e2e/seat.rs",
            "struct IsolatedSeat;\n\nfn start_seat(name: &str) -> IsolatedSeat {\n    let _ = name;\n    IsolatedSeat\n}\n\n#[test]\nfn seat_x() {\n    let _guard = start_seat(\"x\");\n}\n",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::{clippy_drift, contracts_fixture, dep_budget, schema_drift};
    use crate::limits::{Limits, ALLOWED_DEPS};

    /// 現物の manifest から読んだ [`Limits`]。
    fn real_limits() -> Limits {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join(crate::check::RULES_REL);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()));
        Limits::read(&text).unwrap_or_else(|reason| panic!("{reason}"))
    }

    /// `clippy.toml` の fixture（3 key・値は与えた順）。
    fn clippy_toml(lines: u64, complexity: u64, args: u64) -> String {
        format!(
            "too-many-arguments-threshold = {args}\ntoo-many-lines-threshold = {lines}\ncognitive-complexity-threshold = {complexity}\nallow-unwrap-in-tests = true\n"
        )
    }

    /// manifest と同じ値の fixture は違反 0（現物の `clippy.toml` も同じ）。
    #[test]
    fn clippy_thresholds_pass_when_the_three_keys_match_the_manifest() {
        let limits = real_limits();
        let same = clippy_toml(limits.fn_lines, limits.fn_complexity, limits.fn_args);
        assert_eq!(clippy_drift(&same, &limits), Vec::<String>::new());
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let real = std::fs::read_to_string(root.join(super::CLIPPY_REL)).unwrap_or_else(|err| panic!("clippy.toml を読めない: {err}"));
        assert_eq!(clippy_drift(&real, &limits), Vec::<String>::new(), "現物の clippy.toml は manifest と一致");
    }

    /// `too-many-lines-threshold = 600` は違反 1 件が key と両値を名指し、key 欠落も違反。
    #[test]
    fn clippy_thresholds_name_key_and_both_values_on_drift_and_fail_on_missing_key() {
        let limits = real_limits();
        let loosened = clippy_toml(600, limits.fn_complexity, limits.fn_args);
        let found = clippy_drift(&loosened, &limits);
        assert_eq!(found.len(), 1, "{found:?}");
        let line = found.first().map(String::as_str).unwrap_or_default();
        assert!(line.starts_with("clippy-thresholds: too-many-lines-threshold = 600 ≠ R-C4-4.fn-lines = "), "{line}");
        assert!(line.ends_with(&format!(" = {}", limits.fn_lines)), "manifest の値を名指す: {line}");
        let without = clippy_toml(limits.fn_lines, limits.fn_complexity, limits.fn_args)
            .replace("cognitive-complexity-threshold", "cognitive-complexity-thresh0ld");
        let missing = clippy_drift(&without, &limits);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(
            missing.first().is_some_and(|line| line.contains("cognitive-complexity-threshold が無い")),
            "{missing:?}"
        );
        // 整数でない値も一致には化けない。
        let quoted_args = format!(
            "too-many-arguments-threshold = \"{}\"\ntoo-many-lines-threshold = {}\ncognitive-complexity-threshold = {}\n",
            limits.fn_args, limits.fn_lines, limits.fn_complexity
        );
        assert_eq!(clippy_drift(&quoted_args, &limits).len(), 1, "整数でない値は一致に化けない");
    }

    /// `ALLOWED_DEPS.len()` ≤ R-C13-1 が現物で成り立ち、fixture で超えると違反。
    #[test]
    fn dep_budget_holds_on_workspace_and_fails_when_exceeded() {
        let limits = real_limits();
        let real = super::measure_dep_budget(&limits);
        assert_eq!(real.violations, Vec::<String>::new(), "{}", real.fact);
        assert_eq!(real.fact, format!("dep-budget={}/{}", ALLOWED_DEPS.len(), limits.dep_budget));
        let over = dep_budget(13, 12);
        assert_eq!(over.violations.len(), 1, "{:?}", over.violations);
        assert!(over.violations.first().is_some_and(|line| line.starts_with("dep-budget: ") && line.contains("13")));
        assert_eq!(over.fact, "dep-budget=13/12");
        assert_eq!(dep_budget(12, 12).violations, Vec::<String>::new(), "等しいは通る");
    }

    /// 生成物と正本の 2 面（欄は `(名, Need の variant, Shape の variant)`）。
    fn faces(columns: &[(&str, &str, &str)]) -> (String, String) {
        let mut schema = "schema = 1\n".to_owned();
        let mut table = "pub const FIELDS: &[Field] = &[\n".to_owned();
        for (name, need, shape) in columns {
            let (low_need, low_shape) = (need.to_lowercase(), shape.to_lowercase());
            schema.push_str(&format!("\n[[field]]\nname = \"{name}\"\nneed = \"{low_need}\"\nshape = \"{low_shape}\"\n"));
            table.push_str(&format!("    Field {{ name: \"{name}\", need: Need::{need}, shape: Shape::{shape} }},\n"));
        }
        table.push_str("];\n");
        (schema, table)
    }

    /// 生成物と正本の欄の列（名・必須 / 任意・値の形・順序）が一致すれば違反 0（健全な擬似 workspace の 2 面も同じ）。
    #[test]
    fn contracts_schema_passes_when_the_tracked_columns_match_the_field_slice() {
        let (schema, table) = faces(&[("id", "Required", "Text"), ("touches", "Optional", "List")]);
        assert_eq!(schema_drift(&schema, &table), Vec::<String>::new());
        let fixture = contracts_fixture("demo");
        let body = |at: usize| fixture.get(at).map(|(_, text)| text.clone()).unwrap_or_default();
        assert_eq!(schema_drift(&body(0), &body(1)), Vec::<String>::new(), "健全な擬似 workspace の 2 面");
    }

    /// 並べ替え・必須 / 任意の違い・値の形の違い・欄の欠けはどれも 1 件で落ち、正本を読めない形は一致に化けない。
    #[test]
    fn contracts_schema_names_order_need_shape_and_missing_columns() {
        let (_, table) = faces(&[("id", "Required", "Text"), ("touches", "Optional", "List")]);
        for columns in [
            vec![("touches", "Optional", "List"), ("id", "Required", "Text")],
            vec![("id", "Optional", "Text"), ("touches", "Optional", "List")],
            vec![("id", "Required", "List"), ("touches", "Optional", "List")],
            vec![("id", "Required", "Text")],
        ] {
            let (schema, _) = faces(&columns);
            let found = schema_drift(&schema, &table);
            assert_eq!(found.len(), 1, "{columns:?}: {found:?}");
            assert!(found.iter().all(|line| line.starts_with("contracts-schema: ")), "{found:?}");
        }
        let (schema, _) = faces(&[("id", "Required", "Text")]);
        let unreadable = schema_drift(&schema, "fn nothing() {}\n");
        assert!(unreadable.first().is_some_and(|line| line.contains("FIELDS の欄を 1 本も読めない")), "{unreadable:?}");
    }
}

