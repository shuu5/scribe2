//! `cargo xtask check` の**宣言 file を読む measure**（manifest-name / manifest-version /
//! lints-set / lints-optin / deps-empty / toolchain-pin / contracts-schema）。母集団は `Cargo.toml` /
//! `rust-toolchain.toml` / 生成物 manifest / 契約表の欄の生成物である。
//!
//! `check.rs` から分けたのは憲法 C4（1 file の上限）のためで、**測る内容は 1 つも変えていない**
//! （`s2-07l.84`・純粋な移動）。判定行の名前・順序・値の書式は不変である。

use crate::check::{failed, json_string_field, read_text, Layout, Measured};
use crate::genmanifest::MANIFEST_REL;
use crate::limits::{ALLOWED_DEPS, REQUIRED_LINTS};
use crate::toml_lite::{entries_in, lint_level, quoted, sections};
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

#[cfg(test)]
mod tests {
    use super::{contracts_fixture, schema_drift};

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

