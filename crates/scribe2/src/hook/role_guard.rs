//! 席の権能の執行（`pre-tool-use` の role guard・設計 docs/design/seat-roles.md §3 / §4 / §6・
//! ADR-0022 §2.2 / §2.3 / §2.5・SRS FR41 / FR45 / AC15 / AC16・憲法 C1 / C5 / C2 / C2.2 / C11.2 / C16）。
//!
//! 役割の規律（orchestrator は実装を自分で行わない・ADR-0045 §2 (1)）を**役割ごとの rules 行 1 つ**（`role.<役割名>`・
//! 値は権能の名の列・裁定 id 付き）に置き、この guard がその行を読んで **2 面**で止める:
//! (1) **Bash** — command 行が権能付き subcommand（[`CAPABILITY_COMMANDS`]）を含む周に、席の役割の行が
//! その権能を持たなければ deny。(2) **Edit 系** — 編集先の path 種別（[`PathKind`]・repo root からの相対
//! path の prefix だけで分類し、字面の語彙では判定しない）ごとの権能を照合し、持たなければ deny。契約が
//! 印（`opens`）で開いた便の write-set の内側だけは、その種別の権能が無くても通す（AC16）。
//!
//! 種別に属する path の集合は**対象 repo の vessel 宣言が名乗る**（設計 §24・ADR-0047・[`PathKinds`]）: guard は
//! 分類の直前に anchor の HEAD の tree の宣言を読み（Edit 系の周にだけ git の子 process 1 回）、書かれた key の
//! 種別は宣言の prefix で、書かれていない種別は固定の判定（本 repo の配置）で分類する。宣言 file 自身は常に
//! `Code`（席は自分の柵を広げられない）で、不正な宣言は repo 内の全 file を `Code` に倒し deny の行が理由を名乗る。
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
use crate::pipe::declaration::path_kinds::{self, Invalid, PathKinds};
use crate::pipe::declaration::DECL_FILE;
use crate::pipe::{contract_path, worktrees_dir};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use crate::seat::role::{role_of_target, Capability, Role};
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

/// 歯の段（[`PathKind::Tests`]・`crates/<crate>/tests/…` の 1 段目と 3 段目）。
const TESTS_DIRS: [&str; 2] = ["crates", "tests"];

/// subcommand の名の並びを閉じる token の末尾（`;` `&&` `|` `)` の直付け・`pipe answer;` の形）。
const SEPARATORS: &[char] = &[';', '&', '|', ')'];

/// 編集先の path 種別（closed enum・宣言順）。分類は repo root からの相対 path の **prefix だけ**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathKind {
    /// `design-intent/` 配下。
    DesignIntent,
    /// `docs/design/` 配下。
    DesignDoc,
    /// 歯（`crates/<crate>/tests/` 配下）。
    Tests,
    /// 上記以外の repo 内。
    Code,
    /// repo root の外（root からの相対 path が `..` で始まる・root を解けない周も同じ）。
    Outside,
}

/// [`PathKind`] の全 variant（宣言順）。
pub const PATH_KINDS: &[PathKind] = &[
    PathKind::DesignIntent,
    PathKind::DesignDoc,
    PathKind::Tests,
    PathKind::Code,
    PathKind::Outside,
];

impl PathKind {
    /// 契約の印 `opens` と記録に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesignIntent => "design-intent",
            Self::DesignDoc => "design-doc",
            Self::Tests => "tests",
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
            Self::Tests => Capability::EditTests,
            Self::Code => Capability::EditCode,
            Self::Outside => Capability::EditOutside,
        }
    }

    /// repo 相対 path（`..` を畳んだ後の形）から**固定の判定**（本 repo の配置）で種別を引く。root ちょうど（空）は
    /// repo 内＝`Code`。
    pub fn of_relative(rel: &Path) -> Self {
        let parts: Vec<&str> = rel.components().filter_map(|part| part.as_os_str().to_str()).collect();
        Self::fixed(&parts)
    }

    /// 固定の判定の本体（段の並びだけを見る）。
    fn fixed(parts: &[&str]) -> Self {
        match parts {
            [first, ..] if *first == ".." => Self::Outside,
            [first, ..] if *first == DESIGN_INTENT_DIR => Self::DesignIntent,
            [first, second, ..] if [*first, *second] == DESIGN_DOC_DIRS => Self::DesignDoc,
            [first, _, third, ..] if [*first, *third] == TESTS_DIRS => Self::Tests,
            _ => Self::Code,
        }
    }

    /// repo 相対 path から、anchor の宣言（[`PathKinds`]・設計 §24）で種別を引く。`..` で外れる形は宣言に依らず
    /// `Outside`。宣言 file 自身（[`DECL_FILE`]）は宣言に何が書いてあっても `Code`。不正な宣言は repo 内の全 file が
    /// `Code`。書かれた key の種別は宣言の prefix（[`path_kinds::matches`]）で、書かれていない種別は固定の判定
    /// （[`Self::fixed`]）で、宣言順に最初に当たった種別を返す。
    pub fn classify(rel: &Path, kinds: &PathKinds) -> Self {
        let parts: Vec<&str> = rel.components().filter_map(|part| part.as_os_str().to_str()).collect();
        if parts.first() == Some(&"..") {
            return Self::Outside;
        }
        let text = parts.join("/");
        if text == DECL_FILE {
            return Self::Code;
        }
        let declared = match kinds {
            PathKinds::Invalid(_) => return Self::Code,
            PathKinds::Default => return Self::fixed(&parts),
            PathKinds::Declared(declared) => declared,
        };
        let items = [&declared.design_intent, &declared.design_doc, &declared.tests];
        let hit = |(kind, declared): (&Self, &Option<Vec<String>>)| match declared {
            Some(items) => path_kinds::under_any(items, &text),
            None => Self::fixed(&parts) == *kind,
        };
        let kinds = [Self::DesignIntent, Self::DesignDoc, Self::Tests];
        kinds.iter().zip(items).find(|pair| hit(*pair)).map_or(Self::Code, |(kind, _)| *kind)
    }
}

/// 権能付き subcommand の名（`<NAME>` の直後の 2 語）→ 権能。**器の口だけ**を見る（`gh pr merge` 等の他 tool は
/// 見ない）。`Go` / `EditContract` は対応する subcommand が無い（宣言だけ・module doc）。`pipe stop` は便 1 本を
/// 名指す形（[`named_stop`]）だけが `Stop` で、それ以外の停止は [`capabilities_of`] が `Launch` へ降ろす
/// （ADR-0048 §2・設計 seat-roles.md §25 約束 3 / 4）。
pub const CAPABILITY_COMMANDS: &[(&str, Capability)] = &[
    ("pipe answer", Capability::Answer),
    ("pipe approve", Capability::Approve),
    ("pipe intake", Capability::Launch),
    ("pipe run", Capability::Launch),
    ("pipe resume", Capability::Launch),
    ("pipe stop", Capability::Stop),
    ("pipe retire", Capability::Launch),
    ("pipe land", Capability::Merge),
];

/// 停止の 2 語（[`CAPABILITY_COMMANDS`] の `Stop` の行の名）。
const STOP_COMMAND: &str = "pipe stop";

/// 名指しの停止の窓に許す flag（値つき・完全一致＝`--run=<id>` の 1 語は当たらない・§25 約束 3）。
const STOP_FLAGS: [&str; 4] = ["--run", "--state-dir", "--repo", "--rules"];

/// 名指しの停止の窓の値に許さない字（shell が意味を変える字: 区切り・pipe・括弧・`$`・backtick・引用符・redirect）。
const SHELL_CHARS: &[char] = &[';', '&', '|', '(', ')', '$', '`', '\'', '"', '<', '>'];

/// 停止の窓が降りた周の deny 文の末尾の 1 句（通る名指しの形・設計 seat-roles.md §29）。
const STOP_HINT: &str =
    "hint=名指しの停止は --run <id> と置き場・repo・rules の値の対だけの 1 行（前にも後ろにも何も付けない）で stop の権能で通る";

/// 権能付きの操作の種別（記録の `what` と deny 文に書く）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Bash の command 行が含む権能付き subcommand の権能（宣言順・重複なし・1 行に複数在れば全部）と、その行に
    /// 停止の窓が名指しでなく降りた occurrence が 1 つ以上在ったか（[`stop_demoted`]・§29）。
    Capabilities(Vec<Capability>, bool),
    /// Edit 系の編集先の種別。`opened` = 契約が印で開いた便の write-set の内側（AC16）。
    Path {
        /// 編集先の種別。
        kind: PathKind,
        /// 印で開いた便の write-set の内側か。
        opened: bool,
        /// anchor の宣言が不正な周の理由（§24・deny の行に載せる・repo の外の判定は宣言に依らないので `None`）。
        invalid: Option<Invalid>,
    },
}

impl Subject {
    /// 記録の `what` に書く種別（`capability=<名>+<名>` / `path=<名>[ opened]`）。
    pub fn render(&self) -> String {
        match self {
            Self::Capabilities(found, _) => {
                let names: Vec<&str> = found.iter().map(|cap| cap.as_str()).collect();
                format!("capability={}", names.join("+"))
            }
            Self::Path { kind, opened: true, .. } => format!("path={} opened", kind.as_str()),
            Self::Path { kind, opened: false, .. } => format!("path={}", kind.as_str()),
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
/// Edit 系で編集先を読めない周は root の内側と確かめられないので `Outside` として扱う（fail-closed）。分類の直前に
/// anchor（`op.root`）の HEAD の宣言を読む（[`PathKinds::read_at_head`]・Edit 系で編集先と root が在る周にだけ git を
/// 撃つ・§24）。repo の外の判定は宣言に依らないので、`Outside` の周は不正の理由を載せない。
pub fn subject(op: &Operation, state_dir: Option<&Path>) -> Option<Subject> {
    if op.tool == BASH {
        let command = op.command.unwrap_or_default();
        let found = capabilities_of(command);
        return (!found.is_empty()).then(|| Subject::Capabilities(found, stop_demoted(command)));
    }
    if !GUARDED.contains(&op.tool) {
        return None;
    }
    let Some(target) = op.path else {
        return Some(Subject::Path { kind: PathKind::Outside, opened: false, invalid: None });
    };
    let kinds = op.root.map_or(PathKinds::Default, PathKinds::read_at_head);
    let located = locate(op.root, op.cwd, target, &kinds);
    let opened = match (&located.run, state_dir) {
        (Some((run, rel)), Some(dir)) => opened_by_contract(dir, run, rel, located.kind),
        _ => false,
    };
    let invalid = kinds.invalid().filter(|_| located.kind != PathKind::Outside);
    Some(Subject::Path { kind: located.kind, opened, invalid })
}

/// command 行が含む権能付き subcommand の権能（宣言順・重複なし）。
///
/// 照合は空白区切りの token の並び `<NAME> <sub> <sub2>` で、binary の名は `NAME` そのものか path の末尾
/// （`target/debug/<NAME>`）・写しの形（`<NAME>.bin` / `…/<NAME>-pipe.bin`・[`is_self`]）。`${..._BIN}` の展開後の字面は見ない（shell の展開を器は解かない）。
///
/// 停止（[`STOP_COMMAND`]）だけは 2 語の後ろの**窓**も読む: 便 1 本を名指す形（[`named_stop`]）は `Stop`・それ以外は
/// `Launch` へ降ろす（席の行に `launch` は無いのでどの席でも止まる・fail-closed・§25 約束 4）。
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
        let demoted = named == STOP_COMMAND && !named_stop(&tokens, at);
        let listed = CAPABILITY_COMMANDS.iter().filter(|(name, _)| *name == named).map(|(_, cap)| *cap);
        found.extend(listed.map(|cap| if demoted { Capability::Launch } else { cap }));
    }
    crate::seat::role::CAPABILITIES.iter().copied().filter(|cap| found.contains(cap)).collect()
}

/// command 行に停止の 2 語の窓が名指しでなく降りた occurrence が 1 つ以上在ったか（§29・deny 文の 1 句の条件）。
///
/// [`capabilities_of`] と同じ token の並びを読み、判定は [`is_self`] と [`named_stop`] に委ねる（規則を増やさない）。
fn stop_demoted(command: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let word = |at: usize| tokens.get(at).map(|t| t.trim_end_matches(SEPARATORS));
    tokens.iter().enumerate().any(|(at, token)| {
        let (Some(sub), Some(sub2)) = (word(at.saturating_add(1)), word(at.saturating_add(2))) else {
            return false;
        };
        is_self(token) && format!("{sub} {sub2}") == STOP_COMMAND && !named_stop(&tokens, at)
    })
}

/// `tokens[at]` が器の名で続く 2 語が停止の周に、その呼び出しが便 1 本を名指す停止か（§25 約束 3・allowlist）。
///
/// 停止の 2 語目に区切りが直付けの形（`stop;`）は窓が別の command なので名指しでない。窓（2 語の直後から行の
/// 末尾まで）は [`stop_window_is_named`] が読む＝後ろに別の呼び出しが続く行は名指しでない（約束 5）。
fn named_stop(tokens: &[&str], at: usize) -> bool {
    let plain = tokens.get(at.saturating_add(2)).is_some_and(|raw| !raw.ends_with(SEPARATORS));
    plain && stop_window_is_named(tokens.get(at.saturating_add(3)..).unwrap_or_default())
}

/// 停止の窓が名指しの形か: token が [`STOP_FLAGS`] とその値の対だけで出来ていて、`--run` がちょうど 1 回在り、
/// どの値も `-` で始まらず [`SHELL_CHARS`] を 1 つも含まない。`--all`・`--run` 無し・値無し・`--run=<id>` の 1 語・
/// 他の flag・区切りや pipe や `$(` を含む形はすべて `false`（起動の権能へ降りる側・§25 約束 4）。
fn stop_window_is_named(window: &[&str]) -> bool {
    let mut runs = 0usize;
    for pair in window.chunks(2) {
        let [flag, value] = pair else {
            return false;
        };
        if !STOP_FLAGS.contains(flag) || value.starts_with('-') || value.contains(SHELL_CHARS) {
            return false;
        }
        if *flag == "--run" {
            runs = runs.saturating_add(1);
        }
    }
    runs == 1
}

/// token が器の binary を名指すか: basename（最後の `/` の後ろ）が `NAME` に等しいか、`NAME` で始まりその直後の
/// 1 文字が `.` か `-`（写しの `NAME.bin` / `NAME-pipe.bin`・`NAMEctl` は違う・設計 seat-roles.md §27）。字面だけを
/// 見る（file の中身を読まない・実行しない・PATH を引かない）。
fn is_self(token: &str) -> bool {
    let base = token.rsplit('/').next().unwrap_or(token);
    base.strip_prefix(NAME).is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '-']))
}

/// 編集先の所在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// 編集先の種別。
    pub kind: PathKind,
    /// 便の worktree（`<root>/.worktrees/<NAME>/<run>/`）の中なら (run id, worktree 相対 path)。
    pub run: Option<(String, PathBuf)>,
}

/// 編集先を repo root からの相対 path へ解いて分類する（分類は anchor の宣言 `kinds`・[`PathKind::classify`]）。
///
/// 相対 path の基準は payload の `cwd`。root の外（字句で `..` へ抜ける・実体が symlink で外を指す）と root を
/// 解けない周は `Outside`。便の worktree の中は worktree 相対で分類する（便の木は repo の写しである＝anchor の
/// 宣言で分類する・§24）。`.worktrees/` 直下の便の器でない worktree（`<root>/.worktrees/<name>/<rel>`・席が docs
/// PR 用に切る木）も repo の写しとして `<rel>` で分類する（便の印は開かない＝`run: None`）。`.worktrees/<name>`
/// そのものは `Code`。
pub fn locate(root: Option<&Path>, cwd: &Path, target: &str, kinds: &PathKinds) -> Located {
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
    let bead_trees = worktrees_dir(root);
    let Ok(inside) = root.join(&rel).strip_prefix(&bead_trees).map(Path::to_path_buf) else {
        return Located { kind: PathKind::classify(repo_copy_relative(root, &bead_trees, &rel), kinds), run: None };
    };
    let mut parts = inside.components();
    let Some(Component::Normal(run)) = parts.next() else {
        return Located { kind: PathKind::Code, run: None };
    };
    let within: PathBuf = parts.collect();
    Located {
        kind: PathKind::classify(&within, kinds),
        run: Some((run.to_string_lossy().into_owned(), within)),
    }
}

/// 分類に渡す相対 path: `.worktrees/<name>/<rel>`（便の器でない worktree＝repo の写し）なら `<rel>`・
/// `.worktrees/<name>` そのものは `.worktrees/<name>` のまま（＝`Code`）・それ以外は `rel` のまま。
fn repo_copy_relative<'a>(root: &Path, bead_trees: &Path, rel: &'a Path) -> &'a Path {
    let Some(copies) = bead_trees.parent().and_then(|dir| dir.strip_prefix(root).ok()) else {
        return rel;
    };
    let Ok(inside) = rel.strip_prefix(copies) else {
        return rel;
    };
    let mut parts = inside.components();
    match (parts.next(), parts.as_path()) {
        (Some(Component::Normal(_)), within) if !within.as_os_str().is_empty() => within,
        _ => rel,
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

/// 権能を解けない周の断りの理由（closed enum・宣言順 = 解く順・設計 seat-roles.md §13・憲法 C2 / C11）。
///
/// 理由の字面（[`RefuseReason::as_str`]）は deny 文の `reason=` に載る 1 語で、variant ごとに**代替ルートの 1 行**
/// （[`RefuseReason::route`]・器の subcommand の形）を持つ＝止められた席が source を読まずに次の手を取れる（FR45）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefuseReason {
    /// pane → target（`session:window`）が解けない（tmux が撃てない・名が空）。
    TargetUnresolved,
    /// 登録 row（event log）が読めない。
    RegistryUnreadable,
    /// target の登録 row が無い（FR40・席は役割の記録が無ければどの権能も持たない）。
    Unregistered,
    /// rules manifest が読めない。
    RulesUnreadable,
    /// 役割の rules 行（`role.<役割名>`）が無い・不発効。
    NoRow(Role),
    /// anchor（repo root・state dir）が解けない（pane は在る＝席なのに仕える repo が無い）。
    NoAnchor,
}

impl RefuseReason {
    /// 全 variant（宣言順）。`NoRow` は先頭の役割で代表する（字面と route は役割に依らない）。
    pub const ALL: [Self; 6] = [
        Self::TargetUnresolved,
        Self::RegistryUnreadable,
        Self::Unregistered,
        Self::RulesUnreadable,
        Self::NoRow(Role::Orchestrator),
        Self::NoAnchor,
    ];

    /// deny 文の `reason=` の 1 語（`NoRow` は行 id を [`RefuseReason::render`] が続ける）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TargetUnresolved => "target-unresolved",
            Self::RegistryUnreadable => "registry-unreadable",
            Self::Unregistered => "unregistered",
            Self::RulesUnreadable => "rules-unreadable",
            Self::NoRow(_) => "no-row",
            Self::NoAnchor => "no-anchor",
        }
    }

    /// deny 文に載る理由の字面（`no-row <row>` だけが行 id を伴う）。
    pub fn render(self) -> String {
        match self {
            Self::NoRow(role) => format!("{} {}", self.as_str(), row_id(role)),
            _ => self.as_str().to_owned(),
        }
    }

    /// 代替ルートの 1 行（器の subcommand の形・deny 文が `<NAME>` を前置する）。理由ごとに 1 形で、散文の手順は
    /// 持たない（N2）。
    pub fn route(self) -> &'static str {
        match self {
            Self::TargetUnresolved => "seat launch --state-dir <S> --role <orchestrator> --target <session:window>",
            Self::RegistryUnreadable => "doctor --state-dir <S>",
            Self::Unregistered => {
                "seat register --state-dir <S> --target <session:window> --role <orchestrator> --account <L> --launch <FILE>"
            }
            Self::RulesUnreadable => "doctor --state-dir <S> --rules <PATH>",
            Self::NoRow(_) => "rules get <row> --rules <PATH>",
            Self::NoAnchor => "vessel init --state-dir <S> <ROOT>",
        }
    }
}

/// 権能付きの操作を判定する（解く順 = pane → target → 登録 row → role → 行 → 権能）。
pub fn decide(subject: &Subject, seat: &Seat) -> RoleDecision {
    let Some(pane) = seat.pane.filter(|found| !found.trim().is_empty()) else {
        return RoleDecision::Inactive;
    };
    let socket = seat.socket.filter(|found| !found.trim().is_empty());
    let Some(target) = crate::seat::target_of_pane(socket, pane) else {
        return RoleDecision::Deny(refused(subject, RefuseReason::TargetUnresolved));
    };
    let Ok(events) = store::read_all(seat.state_dir) else {
        return RoleDecision::Deny(refused(subject, RefuseReason::RegistryUnreadable));
    };
    let Some(role) = role_of_target(&replay(&events), &target) else {
        return RoleDecision::Deny(refused(subject, RefuseReason::Unregistered));
    };
    let manifest = seat.rules.map_or_else(Manifest::embedded, Manifest::load);
    let Ok(manifest) = manifest else {
        return RoleDecision::Deny(refused(subject, RefuseReason::RulesUnreadable));
    };
    judge(subject, role, &manifest)
}

/// 役割と rules 行だけから判定する（pure・tmux も file も撃たない）。行が無い・不発効の役割は権能なし。
pub fn judge(subject: &Subject, role: Role, manifest: &Manifest) -> RoleDecision {
    let Some(held) = held_by(manifest, role) else {
        return RoleDecision::Deny(refused(subject, RefuseReason::NoRow(role)));
    };
    let (missing, invalid, demoted): (Vec<Capability>, Option<Invalid>, bool) = match subject {
        Subject::Capabilities(needed, demoted) => {
            (needed.iter().copied().filter(|cap| !held.contains(cap)).collect(), None, *demoted)
        }
        Subject::Path { opened: true, .. } => (Vec::new(), None, false),
        Subject::Path { kind, opened: false, invalid } => {
            let cap = kind.capability();
            (if held.contains(&cap) { Vec::new() } else { vec![cap] }, *invalid, false)
        }
    };
    if missing.is_empty() {
        RoleDecision::Allow
    } else {
        RoleDecision::Deny(denied(role, &missing, invalid, demoted))
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

/// deny 文: **欠けた権能と rules 行 id** を名指す（設計 §4・字面は現物が正本）。anchor の宣言が不正な周は末尾に
/// `paths=invalid:<理由>`（doctor の欄と同じ字面・§24）を持つ＝止められた席が「全 file が code に倒れている」
/// ことと直す先（宣言）を読める。
///
/// 役割は orchestrator 1 つなので「他の役割が持つ」形は持たない（ADR-0045 §2 (1)）——欠けた権能は
/// 行に無いということで、行 id を 1 本名指せば直す先が決まる。
///
/// 欠けた権能に `launch` を含み、かつ停止の窓が降りた行（`demoted`）の周だけ、末尾に通る名指しの形の 1 句
/// （[`STOP_HINT`]）を足す（§29）。条件が揃わない周の行は変わらない。
fn denied(role: Role, missing: &[Capability], invalid: Option<Invalid>, demoted: bool) -> String {
    let names: Vec<&str> = missing.iter().map(|cap| cap.as_str()).collect();
    let paths = invalid.map(|reason| format!(" paths={}", PathKinds::Invalid(reason).render())).unwrap_or_default();
    let hint = if demoted && missing.contains(&Capability::Launch) { format!(" {STOP_HINT}") } else { String::new() };
    format!(
        "{NAME}: この操作（{}）は席の権能でない（rules 行 {}）＝{} 席では止める{paths}{hint}",
        names.join("+"),
        row_id(role),
        role.as_str()
    )
}

/// 権能を解けない周の 1 行（FailClosed・理由の 1 語つき・末尾に代替ルート `route=<NAME> <1 行>`・§13）。
fn refused(subject: &Subject, reason: RefuseReason) -> String {
    format!(
        "{NAME}: この操作（{}）は権能なし reason={}（席の登録 row と rules 行から権能を解けない） route={NAME} {}",
        subject.render(),
        reason.render(),
        reason.route()
    )
}

/// anchor（repo root・state dir）を解けない周の 1 行（pane は在る＝席なのに仕える repo が無い）。
pub fn unanchored_line(subject: &Subject) -> String {
    refused(subject, RefuseReason::NoAnchor)
}

#[cfg(test)]
#[path = "role_guard_tests.rs"]
mod tests;
// flip-check: moved s2-07l.737.22
