//! 席の指示文（設計 docs/design/seat-roles.md §5・ADR-0022 §2.4・SRS FR42 / FR44）: 役割ごとの tracked な雛形 1 枚
//! （`planner.txt` / `admin.txt`・`include_str!` で埋め込む・`headless/runner.txt` と同じ形）の穴を登録 row と rules 行
//! の値で埋めるだけの生成（[`render`]・行の追加も削除もしない）。読み手は SessionStart の hook（`hook/mod.rs`）。
//!
//! 雛形の行は「穴」か「出所 pointer を持つ行」だけである（憲法 C1.2・規範文の定義 = pointer を持たない行・typed）。
//! pointer の形は退避物の命令行と同じ [`PointerKind`]（行末の `→ SSOT:` の後ろを [`wm::references`] が切り
//! [`wm::classify`] が分類する・字面の語彙で判定しない）。行の分類（[`classify_line`]）は in-file の歯が読み、
//! xtask の drift 検査（C14.2・AC17）は同じ規律を雛形 file に対して測る。**env を読まない**（C2.2）: 穴の値は
//! 登録 row と rules 行 `role.<役割>` から来る。

use super::role::{Capability, Role};
use super::wm::{self, PointerKind};
use crate::fleet::Registration;
use crate::headless::fill;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;

/// planner の雛形（tracked・絶対 path も口座名も含まない）。
const PLANNER: &str = include_str!("planner.txt");
/// 管理席の雛形。
const ADMIN: &str = include_str!("admin.txt");

/// 雛形の穴（**閉じた列**・宣言順・設計 §5）。列に無い `{…}` は雛形の違反である。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hole {
    /// rules 行 `role.<役割>` の値の列。
    Capabilities,
    /// 登録 row の target（`session:window`）。
    Target,
    /// 登録 row の anchor（repo root）。
    Anchor,
    /// 役割の名。
    Role,
}

/// [`Hole`] の全 variant（宣言順）。
pub const HOLES: &[Hole] = &[Hole::Capabilities, Hole::Target, Hole::Anchor, Hole::Role];

impl Hole {
    /// 雛形の中の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capabilities => "{capabilities}",
            Self::Target => "{target}",
            Self::Anchor => "{anchor}",
            Self::Role => "{role}",
        }
    }
}

/// 権能の名の列の区切り（生成文の `{capabilities}` の中）。
const CAPABILITY_SEPARATOR: &str = "・";

/// 役割の雛形（役割ごとに 1 枚・variant を足すときは雛形も足す）。
pub fn template(role: Role) -> &'static str {
    match role {
        Role::Planner => PLANNER,
        Role::Admin => ADMIN,
    }
}

/// 雛形の 1 行の分類（**融合しない**・設計 §5「規範文 = pointer を持たない行」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineKind {
    /// 空行（空白だけ）。
    Blank,
    /// 定義済みの穴だけの行（穴を抜くと空白だけ）。
    Holes,
    /// 出所 pointer を持つ行（最も強い kind）。
    Pointed(PointerKind),
    /// 定義に無い穴を持つ行（字面）。
    UnknownHole(String),
    /// 穴でも pointer 行でもない＝規範文（違反）。
    Bare,
}

/// 行の中の `{…}` の字面（出現順・閉じの無い `{` は数えない）。
pub fn braces(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        let Some(tail) = rest.get(open..) else {
            break;
        };
        let Some(close) = tail.find('}') else {
            break;
        };
        found.extend(tail.get(..=close));
        rest = tail.get(close.saturating_add(1)..).unwrap_or_default();
    }
    found
}

/// 行を分類する（pure）。未知の穴は pointer の有無より先に見る（穴の列は閉じている）。台帳 prefix は渡さない
/// （雛形は台帳の id を pointer にしない＝憲法・ADR・設計 doc・rules 行の形だけ）。
pub fn classify_line(line: &str) -> LineKind {
    if line.trim().is_empty() {
        return LineKind::Blank;
    }
    let known: Vec<&str> = HOLES.iter().map(|hole| hole.as_str()).collect();
    if let Some(unknown) = braces(line).into_iter().find(|found| !known.contains(found)) {
        return LineKind::UnknownHole(unknown.to_owned());
    }
    let without_holes = known.iter().fold(line.to_owned(), |text, hole| text.replace(hole, ""));
    if without_holes.trim().is_empty() {
        return LineKind::Holes;
    }
    wm::references(line)
        .iter()
        .filter_map(|reference| wm::classify(reference, &[]))
        .min()
        .map_or(LineKind::Bare, LineKind::Pointed)
}

/// 雛形の違反行（1 始まりの行番号と分類・空 = 通る）。
pub fn violations(template: &str) -> Vec<(usize, LineKind)> {
    template
        .lines()
        .enumerate()
        .filter_map(|(at, line)| match classify_line(line) {
            LineKind::Blank | LineKind::Holes | LineKind::Pointed(_) => None,
            found @ (LineKind::UnknownHole(_) | LineKind::Bare) => Some((at.saturating_add(1), found)),
        })
        .collect()
}

/// 役割の rules 行 `role.<役割>` が持つ権能（行が無い・不発効・値が列でない周は `None`＝注入しない側）。
/// guard（`hook/role_guard.rs`）と同じ行を読む（ADR-0022 §2.2「読み手 4 つ・行は 1 つ」）。
pub fn capabilities_of(manifest: &Manifest, role: Role) -> Option<Vec<Capability>> {
    let row = manifest.get(&crate::hook::role_guard::row_id(role)).filter(|row| row.enabled)?;
    let RuleValue::List(names) = &row.value else {
        return None;
    };
    Some(names.iter().filter_map(|name| Capability::parse(name)).collect())
}

/// 生成文を組む（pure・穴を埋めるだけ・行の追加も削除もしない）。
///
/// 穴は **1 走査**で埋める（[`fill`]・runner / lens と同じ）: 重ねて replace すると、先に埋めた target や anchor の中の
/// `{role}` が次の走査で展開される。`role` は雛形の選択と `{role}` の値で、`registration` からは target と anchor だけを読む。
pub fn render(role: Role, registration: &Registration, capabilities: &[Capability]) -> String {
    let names: Vec<&str> = capabilities.iter().map(|cap| cap.as_str()).collect();
    let listed = names.join(CAPABILITY_SEPARATOR);
    fill(
        template(role),
        &[
            (Hole::Capabilities.as_str(), &listed),
            (Hole::Target.as_str(), &registration.target),
            (Hole::Anchor.as_str(), &registration.anchor),
            (Hole::Role.as_str(), role.as_str()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{braces, capabilities_of, classify_line, render, template, violations, Hole, LineKind, HOLES};
    use crate::fleet::Registration;
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;
    use crate::seat::role::{Capability, Role, ALL, CAPABILITIES};
    use crate::seat::wm::{self, Anchor, PointerKind, Resolution};
    use std::path::PathBuf;

    /// 歯の登録 row（穴の値は固定）。
    fn registration(role: Role) -> Registration {
        Registration {
            role,
            anchor: "/srv/anchor".to_owned(),
            target: "fixture:seat".to_owned(),
            sid: "sid".to_owned(),
            account: "a1".to_owned(),
            launch: "claude\n".to_owned(),
            model: None,
        }
    }

    /// `HOLES` は宣言順・字面は `{…}` の形で重複なし。
    #[test]
    fn seat_brief_holes_are_declared_in_order_with_distinct_braced_names() {
        assert!(is_declaration_order(HOLES, |hole| hole as usize), "HOLES は宣言順");
        assert_eq!(HOLES.len(), 4, "設計 §5 の穴は 4 つ");
        for hole in HOLES.iter().copied() {
            let text = hole.as_str();
            assert!(text.starts_with('{') && text.ends_with('}'), "{text}");
            assert_eq!(HOLES.iter().filter(|other| other.as_str() == text).count(), 1, "字面 {text} が重複する");
            assert_eq!(braces(text), vec![text], "穴 1 つの行から穴 1 つを読む");
        }
        assert_eq!(braces("a {x} b {y"), vec!["{x}"], "閉じの無い `{{` は数えない");
    }

    /// 行の分類は 5 値で融合しない: 空行 / 穴だけ / pointer 行（最も強い kind）/ 未知の穴 / 規範文。
    #[test]
    fn seat_brief_classify_line_separates_holes_pointers_and_bare_prose() {
        assert_eq!(classify_line("   "), LineKind::Blank);
        assert_eq!(classify_line("{capabilities}"), LineKind::Holes);
        assert_eq!(classify_line("{role} {target} {anchor}"), LineKind::Holes, "穴が複数でも穴だけ");
        assert_eq!(classify_line("x → SSOT: ADR-0022 §2.4"), LineKind::Pointed(PointerKind::Adr));
        assert_eq!(
            classify_line("x → SSOT: ADR-0022 §2.4 / 憲法 C1.2"),
            LineKind::Pointed(PointerKind::Constitution),
            "最も強い kind（宣言順で最小）"
        );
        assert_eq!(classify_line("{role} の権能 → SSOT: rules 行 role.planner"), LineKind::Pointed(PointerKind::Manifest));
        assert_eq!(classify_line("{unknown} → SSOT: N2"), LineKind::UnknownHole("{unknown}".to_owned()), "未知の穴は pointer より先");
        assert_eq!(classify_line("席は lock を確保する"), LineKind::Bare, "pointer 無し");
        assert_eq!(classify_line("席は lock を確保する → SSOT: user 裁定 2026-09-14"), LineKind::Bare, "分類できない参照だけ");
        assert_eq!(classify_line("C1 を読む"), LineKind::Bare, "区切りの無い字面は pointer にしない");
        assert_eq!(classify_line("x → SSOT: s2-07l.248"), LineKind::Bare, "台帳の id は雛形の pointer にしない");
        assert_eq!(violations("a → SSOT: N2\n\nb\n{bad}\n"), vec![(3, LineKind::Bare), (4, LineKind::UnknownHole("{bad}".to_owned()))]);
    }

    /// 現物の雛形 2 枚は違反 0（穴か pointer 行だけ）で、pointer はすべて repo の現物に実在する（憲法 / ADR / 設計 doc /
    /// rules 行）。役割の名と `{capabilities}` の穴を必ず持つ（生成文に権能の名がすべて現れる前提）。
    #[test]
    fn seat_brief_templates_hold_only_holes_and_resolvable_pointers() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let anchor = Anchor::with_prefixes(&root, Vec::new());
        for role in ALL.iter().copied() {
            let text = template(role);
            assert_eq!(violations(text), Vec::new(), "{}: 穴か pointer 行だけ", role.as_str());
            assert!(text.lines().count() >= 5, "{}: 設計 §5 の項目を持つ", role.as_str());
            assert!(text.ends_with('\n') && !text.contains('\r'), "{}: LF 終端", role.as_str());
            assert!(text.contains(Hole::Capabilities.as_str()), "{}: 権能の穴", role.as_str());
            assert!(text.contains(&crate::hook::role_guard::row_id(role)), "{}: 自分の rules 行 id を名指す", role.as_str());
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                for reference in wm::references(line) {
                    let Some(kind) = wm::classify(&reference, &[]) else {
                        continue;
                    };
                    assert_eq!(anchor.resolve(kind, &reference), Resolution::Resolved, "{}: {reference}", role.as_str());
                }
            }
        }
    }

    /// `render` は穴を埋めるだけ（行数は雛形と同じ・穴の字面が 0 個残る・値は 1 走査で埋める）。
    #[test]
    fn seat_brief_render_fills_holes_in_one_pass_without_adding_lines() {
        let caps = [Capability::Launch, Capability::Relay];
        let text = render(Role::Admin, &registration(Role::Admin), &caps);
        assert_eq!(text.lines().count(), template(Role::Admin).lines().count(), "行の追加も削除もしない");
        assert!(HOLES.iter().all(|hole| !text.contains(hole.as_str())), "穴が残らない: {text}");
        assert!(text.contains("役割 = admin・target = fixture:seat・anchor = /srv/anchor"), "穴の値: {text}");
        assert!(text.contains("権能 = launch・relay（"), "権能の名の列: {text}");
        // 値の中の穴の字面は展開しない（1 走査）。
        let mut braced = registration(Role::Admin);
        braced.target = "sess:{role}".to_owned();
        let text = render(Role::Admin, &braced, &caps);
        assert!(text.contains("役割 = admin・target = sess:{role}・anchor = /srv/anchor"), "target の中の穴は展開しない: {text}");
        let planner = render(Role::Planner, &registration(Role::Planner), CAPABILITIES);
        assert!(CAPABILITIES.iter().all(|cap| planner.contains(cap.as_str())), "権能の名がすべて現れる: {planner}");
        assert!(planner.contains("役割 = planner"), "{planner}");
    }

    /// 権能は rules 行 `role.<役割>` から読む: 行が無い・不発効・列でない周は `None`。
    #[test]
    fn seat_brief_capabilities_come_from_the_role_row() {
        let parse = |text: &str| match Manifest::parse(text) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        };
        let row = |value: &str, enabled: bool| {
            format!("schema = 1\n\n[[rule]]\nid = \"role.admin\"\nkind = \"RoleCapabilities\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n")
        };
        assert_eq!(
            capabilities_of(&parse(&row("[\"merge\", \"launch\"]", true)), Role::Admin),
            Some(vec![Capability::Merge, Capability::Launch]),
            "行の並びのまま"
        );
        assert_eq!(capabilities_of(&parse(&row("[\"merge\"]", false)), Role::Admin), None, "不発効");
        assert_eq!(capabilities_of(&parse(&row("[\"merge\"]", true)), Role::Planner), None, "行が無い役割");
        assert_eq!(capabilities_of(&parse("schema = 1\n"), Role::Admin), None, "空の manifest");
        let embedded = match Manifest::embedded() {
            Ok(found) => found,
            Err(errors) => panic!("埋め込み manifest を読める: {errors:?}"),
        };
        for role in ALL.iter().copied() {
            assert!(capabilities_of(&embedded, role).is_some_and(|caps| !caps.is_empty()), "{}: 埋め込みに行が在る", role.as_str());
        }
    }
}
