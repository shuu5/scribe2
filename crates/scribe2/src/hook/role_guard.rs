//! 席の権能の執行（`pre-tool-use` の role guard・設計 docs/design/seat-roles.md §3 / §4 / §6・
//! ADR-0022 §2.2 / §2.3 / §2.5・SRS FR41 / FR45 / AC15 / AC16・憲法 C1 / C5 / C2 / C2.2 / C11.2 / C16）。
//!
//! 役割の規律（planner は実装しない・管理席は記帳しない）を**役割ごとの rules 行 1 つ**（`role.<役割名>`・
//! 値は権能の名の列・裁定 id 付き）に置き、この guard がその行を読んで **2 面**で止める:
//! (1) **Bash** — command 行が権能付き subcommand（[`CAPABILITY_COMMANDS`]）を含む周に、席の役割の行が
//! その権能を持たなければ deny。(2) **Edit 系** — 編集先の path 種別（[`PathKind`]・repo root からの相対
//! path の prefix だけで分類し、字面の語彙では判定しない）ごとの権能を照合し、持たなければ deny。契約が
//! 印（`opens`）で開いた便の write-set の内側だけは、その種別の権能が無くても通す（AC16）。
//!
//! **identity は `--pane` だけ**（C2.2・env を読まない）。pane が無い・空（tmux の外の runner / lens）は席では
//! なく本 guard の対象外＝[`RoleDecision::Inactive`]（ADR-0009 の write-set guard がそのまま担う）。**解く順**は
//! anchor（repo root・state dir・hook 側）→ pane → target → 登録 row（`seat register`・s2-07l.192）→ role →
//! 行 → 権能で、pane が在るのに途中で解けない周（target が解けない・登録 row が無い・event log や rules が
//! 読めない・行が無い）は**権能なし＝権能付きの操作を deny**（FailClosed・[`POLARITY`]）。止めるのは権能付きの
//! 操作だけで、それ以外の Bash / Edit は通す（[`subject`] が `None`＝tmux も event log も撃たない・NFR5）。
//!
//! subcommand は役割を検査しない（引数の identity は偽装できる）。発話は監視しない。

use super::guard::GUARDED;
use crate::fleet::{replay, store};
use crate::name::NAME;
use crate::pipe::contract::Contract;
use crate::pipe::{contract_path, worktrees_dir};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use crate::seat::role::{role_of_target, Capability, Role, ALL as ROLES};
use std::path::{Component, Path, PathBuf};

/// この境界の極性: 操作の時点で止め、権能を解けない周は権能付きの操作を通さない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// Bash 面が見る tool の名。
const BASH: &str = "Bash";

/// `design-intent/` の段（[`PathKind::DesignIntent`]）。
const DESIGN_INTENT_DIR: &str = "design-intent";

/// `docs/design/` の 2 段（[`PathKind::DesignDoc`]）。
const DESIGN_DOC_DIRS: [&str; 2] = ["docs", "design"];

/// subcommand の名の並びを閉じる token の末尾（`;` `&&` `|` `)` の直付け・`pipe answer;` の形）。
const SEPARATORS: &[char] = &[';', '&', '|', ')'];

/// 編集先の path 種別（closed enum・宣言順）。分類は repo root からの相対 path の **prefix だけ**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathKind {
    /// `design-intent/` 配下。
    DesignIntent,
    /// `docs/design/` 配下。
    DesignDoc,
    /// 上記以外の repo 内。
    Code,
    /// repo root の外（root からの相対 path が `..` で始まる・root を解けない周も同じ）。
    Outside,
}

/// [`PathKind`] の全 variant（宣言順）。
pub const PATH_KINDS: &[PathKind] = &[
    PathKind::DesignIntent,
    PathKind::DesignDoc,
    PathKind::Code,
    PathKind::Outside,
];

impl PathKind {
    /// 契約の印 `opens` と記録に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesignIntent => "design-intent",
            Self::DesignDoc => "design-doc",
            Self::Code => "code",
            Self::Outside => "outside",
        }
    }

    /// 字面から引く。未知なら `None`（variant 名の字面も受けない）。
    pub fn parse(text: &str) -> Option<Self> {
        PATH_KINDS.iter().copied().find(|found| found.as_str() == text)
    }

    /// この種別の編集に要る権能（種別と 1:1）。
    pub fn capability(self) -> Capability {
        match self {
            Self::DesignIntent => Capability::EditDesignIntent,
            Self::DesignDoc => Capability::EditDesignDoc,
            Self::Code => Capability::EditCode,
            Self::Outside => Capability::EditOutside,
        }
    }

    /// repo 相対 path（`..` を畳んだ後の形）から種別を引く。root ちょうど（空）は repo 内＝`Code`。
    pub fn of_relative(rel: &Path) -> Self {
        let parts: Vec<&str> = rel.components().filter_map(|part| part.as_os_str().to_str()).collect();
        match parts.as_slice() {
            [first, ..] if *first == ".." => Self::Outside,
            [first, ..] if *first == DESIGN_INTENT_DIR => Self::DesignIntent,
            [first, second, ..] if [*first, *second] == DESIGN_DOC_DIRS => Self::DesignDoc,
            _ => Self::Code,
        }
    }
}

/// 権能付き subcommand の名（`<NAME>` の直後の 2 語）→ 権能。**器の口だけ**を見る（`gh pr merge` 等の他 tool は
/// 見ない）。`Go` / `Relay` / `EditContract` は対応する subcommand が無い（宣言だけ・module doc）。
pub const CAPABILITY_COMMANDS: &[(&str, Capability)] = &[
    ("pipe answer", Capability::Answer),
    ("pipe approve", Capability::Approve),
    ("pipe intake", Capability::Launch),
    ("pipe run", Capability::Launch),
    ("pipe resume", Capability::Launch),
    ("pipe stop", Capability::Launch),
    ("pipe retire", Capability::Launch),
    ("pipe land", Capability::Merge),
];

/// 権能付きの操作の種別（記録の `what` と deny 文に書く）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Bash の command 行が含む権能付き subcommand の権能（宣言順・重複なし・1 行に複数在れば全部）。
    Capabilities(Vec<Capability>),
    /// Edit 系の編集先の種別。`opened` = 契約が印で開いた便の write-set の内側（AC16）。
    Path {
        /// 編集先の種別。
        kind: PathKind,
        /// 印で開いた便の write-set の内側か。
        opened: bool,
    },
}

impl Subject {
    /// 記録の `what` に書く種別（`capability=<名>+<名>` / `path=<名>[ opened]`）。
    pub fn render(&self) -> String {
        match self {
            Self::Capabilities(found) => {
                let names: Vec<&str> = found.iter().map(|cap| cap.as_str()).collect();
                format!("capability={}", names.join("+"))
            }
            Self::Path { kind, opened: true } => format!("path={} opened", kind.as_str()),
            Self::Path { kind, opened: false } => format!("path={}", kind.as_str()),
        }
    }
}

/// role guard の判定。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleDecision {
    /// 通す。
    Allow,
    /// 止める。中身は stderr へ出す 1 行。
    Deny(String),
    /// `--pane` が無い・空＝席ではない（tmux の外の runner / lens）。guard は働かない。
    Inactive,
}

/// 1 回の操作の入力（hook が payload と引数から組む）。
pub struct Operation<'a> {
    /// tool 名。
    pub tool: &'a str,
    /// Bash の command 行（Bash 以外は `None`）。
    pub command: Option<&'a str>,
    /// Edit 系の編集先（payload の `file_path` / `notebook_path`）。
    pub path: Option<&'a str>,
    /// repo root（anchor・解けない周は `None`＝編集先は root の外として扱う）。
    pub root: Option<&'a Path>,
    /// 相対 path の基準（payload の `cwd`）。
    pub cwd: &'a Path,
}

/// 席の出所（`--pane` / `--tmux-socket`）と、登録 row と rules の置き場。
pub struct Seat<'a> {
    /// 自席の pane id（無い・空は席ではない）。
    pub pane: Option<&'a str>,
    /// tmux の socket（歯の seam・既定の server なら `None`）。
    pub socket: Option<&'a str>,
    /// 登録 row（event log）の置き場。
    pub state_dir: &'a Path,
    /// rules manifest の override（`--rules`・無ければ埋め込み）。
    pub rules: Option<&'a Path>,
}

/// 操作が権能付きか。権能付きでない Bash / Edit は `None`（通す・記録なし・tmux も event log も撃たない）。
///
/// Edit 系で編集先を読めない周は root の内側と確かめられないので `Outside` として扱う（fail-closed）。
pub fn subject(op: &Operation, state_dir: Option<&Path>) -> Option<Subject> {
    if op.tool == BASH {
        let found = capabilities_of(op.command.unwrap_or_default());
        return (!found.is_empty()).then_some(Subject::Capabilities(found));
    }
    if !GUARDED.contains(&op.tool) {
        return None;
    }
    let Some(target) = op.path else {
        return Some(Subject::Path { kind: PathKind::Outside, opened: false });
    };
    let located = locate(op.root, op.cwd, target);
    let opened = match (&located.run, state_dir) {
        (Some((run, rel)), Some(dir)) => opened_by_contract(dir, run, rel, located.kind),
        _ => false,
    };
    Some(Subject::Path { kind: located.kind, opened })
}

/// command 行が含む権能付き subcommand の権能（宣言順・重複なし）。
///
/// 照合は空白区切りの token の並び `<NAME> <sub> <sub2>` で、binary の名は `NAME` そのものか path の末尾
/// （`target/debug/<NAME>`）。`${..._BIN}` の展開後の字面は見ない（shell の展開を器は解かない）。
pub fn capabilities_of(command: &str) -> Vec<Capability> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let mut found: Vec<Capability> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if !is_self(token) {
            continue;
        }
        let sub = tokens.get(at.saturating_add(1)).map(|t| t.trim_end_matches(SEPARATORS));
        let sub2 = tokens.get(at.saturating_add(2)).map(|t| t.trim_end_matches(SEPARATORS));
        let (Some(sub), Some(sub2)) = (sub, sub2) else {
            continue;
        };
        let named = format!("{sub} {sub2}");
        found.extend(CAPABILITY_COMMANDS.iter().filter(|(name, _)| *name == named).map(|(_, cap)| *cap));
    }
    crate::seat::role::CAPABILITIES.iter().copied().filter(|cap| found.contains(cap)).collect()
}

/// token が器の binary を名指すか（`NAME` か `…/NAME`）。
fn is_self(token: &str) -> bool {
    token == NAME || token.rsplit('/').next() == Some(NAME)
}

/// 編集先の所在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// 編集先の種別。
    pub kind: PathKind,
    /// 便の worktree（`<root>/.worktrees/<NAME>/<run>/`）の中なら (run id, worktree 相対 path)。
    pub run: Option<(String, PathBuf)>,
}

/// 編集先を repo root からの相対 path へ解いて分類する。
///
/// 相対 path の基準は payload の `cwd`。root の外（字句で `..` へ抜ける・実体が symlink で外を指す）と root を
/// 解けない周は `Outside`。便の worktree の中は worktree 相対で分類する（便の木は repo の写しである）。
pub fn locate(root: Option<&Path>, cwd: &Path, target: &str) -> Located {
    let outside = Located { kind: PathKind::Outside, run: None };
    let Some(root) = root else {
        return outside;
    };
    let raw = Path::new(target);
    let absolute = if raw.is_absolute() { raw.to_path_buf() } else { cwd.join(raw) };
    let Some(lexical) = relative_to(root, &absolute) else {
        return outside;
    };
    // 実体の段。symlink を経由して root の外へ出る・repo 内の別の種別へ抜ける形は実体で分類する。
    let Some(rel) = resolved_relative(root, &absolute).or(Some(lexical)) else {
        return outside;
    };
    let Ok(inside) = root.join(&rel).strip_prefix(worktrees_dir(root)).map(Path::to_path_buf) else {
        return Located { kind: PathKind::of_relative(&rel), run: None };
    };
    let mut parts = inside.components();
    let Some(Component::Normal(run)) = parts.next() else {
        return Located { kind: PathKind::Code, run: None };
    };
    let within: PathBuf = parts.collect();
    Located {
        kind: PathKind::of_relative(&within),
        run: Some((run.to_string_lossy().into_owned(), within)),
    }
}

/// 絶対 path を repo 相対へ**字句で**畳む。root の外・`..` で外れるものは `None`（write-set guard と同じ形）。
fn relative_to(root: &Path, absolute: &Path) -> Option<PathBuf> {
    let rel = absolute.strip_prefix(root).ok()?;
    let mut out = PathBuf::new();
    for part in rel.components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// 実体で解いた repo 相対 path（在る段だけ symlink を解く・まだ無い段は字句のまま）。実体が root の外なら `None`。
fn resolved_relative(root: &Path, absolute: &Path) -> Option<PathBuf> {
    let real_root = root.canonicalize().ok()?;
    let mut real = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::RootDir | Component::Prefix(_) => real.push(part.as_os_str()),
            Component::CurDir => {}
            Component::Normal(name) => {
                real.push(name);
                if let Ok(found) = real.canonicalize() {
                    real = found;
                }
            }
            Component::ParentDir => {
                if !real.pop() {
                    return None;
                }
            }
        }
    }
    real.strip_prefix(&real_root).ok().map(Path::to_path_buf)
}

/// 便の契約の印が `kind` を開き、かつ worktree 相対 path が便の write-set の内側か（AC16）。
///
/// 便の写し `contract.toml`（run dir）から読む。写しが無い・読めない・印の無い便は開かない（fail-closed）。
fn opened_by_contract(state_dir: &Path, run: &str, rel: &Path, kind: PathKind) -> bool {
    let Ok(contract) = Contract::load(&contract_path(state_dir, run)) else {
        return false;
    };
    contract.opened_kinds().contains(&kind) && within_write_set(&contract.write_set, rel)
}

/// worktree 相対 path が write-set の内側か。末尾 `/` の項目は配下全部（write-set guard の allowlist と同じ形）。
fn within_write_set(write_set: &[String], rel: &Path) -> bool {
    let text = rel.to_string_lossy();
    write_set.iter().any(|entry| match entry.strip_suffix('/') {
        Some(dir) => text.starts_with(&format!("{dir}/")),
        None => *entry == text,
    })
}

/// 権能付きの操作を判定する（解く順 = pane → target → 登録 row → role → 行 → 権能）。
pub fn decide(subject: &Subject, seat: &Seat) -> RoleDecision {
    let Some(pane) = seat.pane.filter(|found| !found.trim().is_empty()) else {
        return RoleDecision::Inactive;
    };
    let socket = seat.socket.filter(|found| !found.trim().is_empty());
    let Some(target) = crate::seat::target_of_pane(socket, pane) else {
        return RoleDecision::Deny(refused(subject, "target-unresolved"));
    };
    let Ok(events) = store::read_all(seat.state_dir) else {
        return RoleDecision::Deny(refused(subject, "registry-unreadable"));
    };
    let Some(role) = role_of_target(&replay(&events), &target) else {
        return RoleDecision::Deny(refused(subject, "unregistered"));
    };
    let manifest = seat.rules.map_or_else(Manifest::embedded, Manifest::load);
    let Ok(manifest) = manifest else {
        return RoleDecision::Deny(refused(subject, "rules-unreadable"));
    };
    judge(subject, role, &manifest)
}

/// 役割と rules 行だけから判定する（pure・tmux も file も撃たない）。行が無い・不発効の役割は権能なし。
pub fn judge(subject: &Subject, role: Role, manifest: &Manifest) -> RoleDecision {
    let Some(held) = held_by(manifest, role) else {
        return RoleDecision::Deny(refused(subject, &format!("no-row {}", row_id(role))));
    };
    let missing: Vec<Capability> = match subject {
        Subject::Capabilities(needed) => needed.iter().copied().filter(|cap| !held.contains(cap)).collect(),
        Subject::Path { opened: true, .. } => Vec::new(),
        Subject::Path { kind, opened: false } => {
            let cap = kind.capability();
            if held.contains(&cap) { Vec::new() } else { vec![cap] }
        }
    };
    if missing.is_empty() {
        RoleDecision::Allow
    } else {
        RoleDecision::Deny(denied(role, &missing, manifest))
    }
}

/// 役割の rules 行の id（`role.<役割名>`）。
pub fn row_id(role: Role) -> String {
    format!("role.{}", role.as_str())
}

/// 役割の行が持つ権能（行が無い・不発効・値が列でない周は `None`）。列に無い名は loader が拒むので落ちない。
fn held_by(manifest: &Manifest, role: Role) -> Option<Vec<Capability>> {
    let row = manifest.get(&row_id(role)).filter(|row| row.enabled)?;
    let RuleValue::List(names) = &row.value else {
        return None;
    };
    Some(names.iter().filter_map(|name| Capability::parse(name)).collect())
}

/// deny 文: **権能を持つ役割の名と rules 行 id** を名指す（設計 §4・字面は現物が正本）。
fn denied(role: Role, missing: &[Capability], manifest: &Manifest) -> String {
    let names: Vec<&str> = missing.iter().map(|cap| cap.as_str()).collect();
    let holders: Vec<String> = ROLES
        .iter()
        .copied()
        .filter(|other| held_by(manifest, *other).is_some_and(|held| missing.iter().all(|cap| held.contains(cap))))
        .map(|other| format!("{} 席の権能（rules 行 {}）", other.as_str(), row_id(other)))
        .collect();
    let rows: Vec<String> = ROLES.iter().copied().map(row_id).collect();
    if holders.is_empty() {
        format!(
            "{NAME}: この操作（{}）はどの役割の席の権能でもない（rules 行 {}）＝{} 席では止める",
            names.join("+"),
            rows.join(" / "),
            role.as_str()
        )
    } else {
        format!("{NAME}: この操作（{}）は {}＝{} 席は持たない", names.join("+"), holders.join(" / "), role.as_str())
    }
}

/// 権能を解けない周の 1 行（FailClosed・理由の 1 語つき）。
fn refused(subject: &Subject, reason: &str) -> String {
    format!("{NAME}: この操作（{}）は権能なし reason={reason}（席の登録 row と rules 行から権能を解けない）", subject.render())
}

/// anchor（repo root・state dir）を解けない周の 1 行（pane は在る＝席なのに仕える repo が無い）。
pub fn unanchored_line(subject: &Subject) -> String {
    refused(subject, "no-anchor")
}

#[cfg(test)]
mod tests {
    use super::{capabilities_of, judge, locate, PathKind, RoleDecision, Subject, CAPABILITY_COMMANDS, PATH_KINDS};
    use crate::name::NAME;
    use crate::rules::manifest::Manifest;
    use crate::seat::role::{Capability, Role, CAPABILITIES};
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::path::{Path, PathBuf};

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config { cases: 256, failure_persistence: None, ..Config::default() }
    }

    /// `role.planner` が `held` を持つ manifest（`role.admin` は無い）。空の列は行を置けない（loader が空の
    /// 配列を拒む）ので、行の無い manifest にする（＝権能なし）。
    fn manifest_with(held: &[Capability]) -> Manifest {
        let names: Vec<String> = held.iter().map(|cap| format!("\"{}\"", cap.as_str())).collect();
        let row = if names.is_empty() {
            String::new()
        } else {
            format!(
                "\n[[rule]]\nid = \"role.planner\"\nkind = \"RoleCapabilities\"\nvalue = [{}]\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n",
                names.join(", ")
            )
        };
        Manifest::parse(&format!("schema = 1\n{row}"))
            .unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// 分類は **prefix だけ**（`..`・root ちょうど・似た名の兄弟 dir・段の深さ）。
    #[test]
    fn role_guard_path_kind_classifies_by_prefix_only() {
        let root = Path::new("/repo");
        for (target, want) in [
            ("design-intent/spec/srs.html", PathKind::DesignIntent),
            ("design-intent", PathKind::DesignIntent),
            ("design-intents/x.html", PathKind::Code),
            ("docs/design/seat-roles.md", PathKind::DesignDoc),
            ("docs/design", PathKind::DesignDoc),
            ("docs/designs/x.md", PathKind::Code),
            ("docs/x.md", PathKind::Code),
            ("src/design-intent/x.rs", PathKind::Code),
            ("README.md", PathKind::Code),
            ("", PathKind::Code),
            ("src/../docs/design/x.md", PathKind::DesignDoc),
            ("../outside.rs", PathKind::Outside),
            ("src/../../outside.rs", PathKind::Outside),
            ("/elsewhere/x.rs", PathKind::Outside),
        ] {
            let found = locate(Some(root), root, target);
            assert_eq!(found.kind, want, "{target}");
            assert_eq!(found.run, None, "{target} は便の worktree の外");
        }
        // 相対 path の基準は cwd（subdir から見た `design-intent/` は repo の `sub/design-intent/`＝Code）。
        assert_eq!(locate(Some(root), &root.join("sub"), "design-intent/x.html").kind, PathKind::Code);
        // root を解けない周は root の外として扱う（fail-closed）。
        assert_eq!(locate(None, root, "src/lib.rs").kind, PathKind::Outside);
    }

    /// 便の worktree の中は worktree 相対で分類し、run id と相対 path を返す。
    #[test]
    fn role_guard_locates_bead_worktree_paths_by_run_id() {
        let root = PathBuf::from("/repo");
        let inside = format!(".worktrees/{NAME}/run-1/docs/design/x.md");
        let found = locate(Some(&root), &root, &inside);
        assert_eq!(found.kind, PathKind::DesignDoc, "worktree 相対で分類する");
        assert_eq!(found.run, Some(("run-1".to_owned(), PathBuf::from("docs/design/x.md"))));
        let code = locate(Some(&root), &root, &format!(".worktrees/{NAME}/run-2/src/lib.rs"));
        assert_eq!(code.kind, PathKind::Code);
        assert_eq!(code.run.map(|(run, _)| run), Some("run-2".to_owned()));
        // worktree の集合 dir そのもの・別の器の worktree は便の中ではない。
        assert_eq!(locate(Some(&root), &root, &format!(".worktrees/{NAME}")).run, None);
        assert_eq!(locate(Some(&root), &root, ".worktrees/other/run-1/src/lib.rs").run, None);
    }

    /// 照合は `<NAME> <sub> <sub2>` の並び: binary の名は末尾でもよく、1 行に複数在れば全部・重複は畳む・
    /// 器の口でない command（`gh pr merge`・`pipe show`）と `${..._BIN}` の形は見ない。
    #[test]
    fn role_guard_capability_commands_match_the_three_word_sequence() {
        let answer = format!("{NAME} pipe answer --run r --words x");
        assert_eq!(capabilities_of(&answer), vec![Capability::Answer]);
        let by_path = format!("target/debug/{NAME} pipe land --run r");
        assert_eq!(capabilities_of(&by_path), vec![Capability::Merge]);
        let two = format!("{NAME} pipe answer --run r; {NAME} pipe run --run r && {NAME} pipe stop --run r");
        assert_eq!(capabilities_of(&two), vec![Capability::Answer, Capability::Launch], "宣言順・重複なし");
        let tight = format!("{NAME} pipe approve; ls");
        assert_eq!(capabilities_of(&tight), vec![Capability::Approve], "`;` の直付けでも 2 語目が読める");
        for silent in [
            "ls -la".to_owned(),
            format!("{NAME} pipe show --run r"),
            format!("{NAME} pipe"),
            format!("gh pr merge 1 && echo {NAME}"),
            format!("\"${{{}_BIN:-{NAME}}}\" pipe answer", NAME.to_uppercase()),
            format!("not{NAME} pipe answer"),
            format!("{NAME}x/pipe answer"),
        ] {
            assert!(capabilities_of(&silent).is_empty(), "権能付きでない: {silent}");
        }
        // 表は subcommand の名を 2 語で持ち、`pipe` の口だけである。
        for (name, _) in CAPABILITY_COMMANDS {
            assert_eq!(name.split(' ').count(), 2, "{name}");
            assert!(name.starts_with("pipe "), "{name}");
        }
    }

    /// 判定は行の値だけを読む: 持てば Allow・欠けば Deny（deny 文は権能を持つ役割の名と行 id）・
    /// 行の無い役割は権能なし・印で開いた path は種別の権能が無くても Allow。
    #[test]
    fn role_guard_judge_reads_the_role_row() {
        let manifest = manifest_with(&[Capability::Answer, Capability::EditDesignDoc]);
        let answer = Subject::Capabilities(vec![Capability::Answer]);
        assert_eq!(judge(&answer, Role::Planner, &manifest), RoleDecision::Allow);
        let launch = Subject::Capabilities(vec![Capability::Answer, Capability::Launch]);
        let RoleDecision::Deny(line) = judge(&launch, Role::Planner, &manifest) else {
            panic!("欠けた権能は deny");
        };
        assert!(line.starts_with(&format!("{NAME}: ")), "器が名乗る: {line}");
        assert!(line.contains("launch") && !line.contains("answer"), "欠けた権能だけを名指す: {line}");
        assert!(line.contains("role.planner") && line.contains("role.admin"), "行 id を名指す: {line}");
        let RoleDecision::Deny(line) = judge(&answer, Role::Admin, &manifest) else {
            panic!("行の無い役割は権能なし");
        };
        assert!(line.contains("reason=no-row role.admin"), "{line}");
        let doc = Subject::Path { kind: PathKind::DesignDoc, opened: false };
        assert_eq!(judge(&doc, Role::Planner, &manifest), RoleDecision::Allow);
        let code = Subject::Path { kind: PathKind::Code, opened: false };
        let RoleDecision::Deny(line) = judge(&code, Role::Planner, &manifest) else {
            panic!("種別の権能が無ければ deny");
        };
        assert!(line.contains("edit-code"), "{line}");
        let opened = Subject::Path { kind: PathKind::Code, opened: true };
        assert_eq!(judge(&opened, Role::Planner, &manifest), RoleDecision::Allow, "印で開いた path は通る");
        // 権能を持つ役割の名を deny 文に含める（admin の行が在る manifest）。
        let both = Manifest::parse(
            "schema = 1\n\n[[rule]]\nid = \"role.planner\"\nkind = \"RoleCapabilities\"\nvalue = [\"answer\"]\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n\n[[rule]]\nid = \"role.admin\"\nkind = \"RoleCapabilities\"\nvalue = [\"launch\"]\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n",
        )
        .unwrap_or_else(|errors| panic!("{errors:?}"));
        let RoleDecision::Deny(line) = judge(&answer, Role::Admin, &both) else {
            panic!("admin は answer を持たない");
        };
        assert!(line.contains("planner 席の権能（rules 行 role.planner）"), "権能を持つ役割と行 id: {line}");
        assert!(line.contains("admin 席は持たない"), "{line}");
    }

    /// 記録の種別の字面。
    #[test]
    fn role_guard_subject_renders_capability_or_path_kind() {
        let caps = Subject::Capabilities(vec![Capability::Answer, Capability::Launch]);
        assert_eq!(caps.render(), "capability=answer+launch");
        assert_eq!(Subject::Path { kind: PathKind::Code, opened: false }.render(), "path=code");
        assert_eq!(Subject::Path { kind: PathKind::Code, opened: true }.render(), "path=code opened");
        assert_eq!(PATH_KINDS.len(), 4, "path 種別は 4 つ");
    }

    /// 権能の名か表に無い語。
    fn word() -> impl Strategy<Value = String> {
        prop::sample::select(vec![
            NAME.to_owned(),
            format!("bin/{NAME}"),
            "pipe".to_owned(),
            "answer".to_owned(),
            "approve".to_owned(),
            "run".to_owned(),
            "land".to_owned(),
            "show".to_owned(),
            "ls".to_owned(),
            "&&".to_owned(),
            "--run".to_owned(),
        ])
    }

    /// 任意の command 行（表の語の並び）。
    fn command() -> impl Strategy<Value = String> {
        prop::collection::vec(word(), 0..12).prop_map(|words| words.join(" "))
    }

    /// 権能付き subcommand を少なくとも 1 つ含む command 行（前後は任意の語）。
    fn armed_command() -> impl Strategy<Value = String> {
        (command(), 0..CAPABILITY_COMMANDS.len(), command()).prop_map(|(head, at, tail)| {
            let (name, _) = CAPABILITY_COMMANDS.get(at).copied().unwrap_or(("pipe answer", Capability::Answer));
            format!("{head} {NAME} {name} {tail}")
        })
    }

    /// 権能の任意の部分集合。
    fn capability_set() -> impl Strategy<Value = Vec<Capability>> {
        prop::collection::vec(any::<bool>(), CAPABILITIES.len()).prop_map(|mask| {
            CAPABILITIES.iter().copied().zip(mask).filter(|(_, keep)| *keep).map(|(cap, _)| cap).collect()
        })
    }

    proptest! {
        #![proptest_config(config())]

        /// `Capability` / `PathKind` の `as_str` ↔ `parse` は往復し、列に無い名は必ず `None`。
        #[test]
        fn prop_role_names_round_trip_and_reject_unknown(name in "[a-z-]{0,20}") {
            for cap in CAPABILITIES {
                prop_assert_eq!(Capability::parse(cap.as_str()), Some(*cap));
            }
            for kind in PATH_KINDS {
                prop_assert_eq!(PathKind::parse(kind.as_str()), Some(*kind));
            }
            let known_cap = CAPABILITIES.iter().any(|cap| cap.as_str() == name);
            prop_assert_eq!(Capability::parse(&name).is_some(), known_cap);
            let known_kind = PATH_KINDS.iter().any(|kind| kind.as_str() == name);
            prop_assert_eq!(PathKind::parse(&name).is_some(), known_kind);
        }

        /// 任意の command 行で「照合された権能 ⊆ 行の値」なら Allow、1 つでも欠ければ Deny。
        #[test]
        fn prop_role_bash_face_allows_iff_matched_capabilities_are_held(line in armed_command(), extra in capability_set()) {
            let matched = capabilities_of(&line);
            prop_assert!(!matched.is_empty(), "{line}");
            let subject = Subject::Capabilities(matched.clone());
            let mut held = matched.clone();
            held.extend(extra.iter().copied());
            prop_assert_eq!(judge(&subject, Role::Planner, &manifest_with(&held)), RoleDecision::Allow);
            let short: Vec<Capability> = held.iter().copied().filter(|cap| Some(cap) != matched.first()).collect();
            prop_assert!(matches!(judge(&subject, Role::Planner, &manifest_with(&short)), RoleDecision::Deny(_)));
        }

        /// 照合は行の語の並びだけを見る＝行の中の `<NAME> <sub> <sub2>` の 3 語窓が表に無ければ空。
        #[test]
        fn prop_role_capabilities_are_a_subset_of_the_table(line in command()) {
            let found = capabilities_of(&line);
            for cap in &found {
                prop_assert!(CAPABILITY_COMMANDS.iter().any(|(_, listed)| listed == cap));
            }
            let mut unique = found.clone();
            unique.dedup();
            prop_assert_eq!(unique.len(), found.len());
        }
    }
}
