//! host の破壊防止の見張り（subcommand `host-guard`・設計 docs/design/vessel-hook.md §11 行 b・ADR-0056・
//! SRS FR56 / FR20 / FR24 / AC7 / AC27 / NFR4 / NFR5）。
//!
//! 口座の設定から PreToolUse で呼ばれる器の 1 つの口で、**marker と anchor に依らず**判定する（hook の入口の沈黙
//! 〔FR24〕は持ち込まない＝`.vessel` の無い repo・他の name の marker・git repo でない cwd でも同じ判定）。種類は閉じた
//! 5 値 [`Kind`] で、判定は宣言順に 1 種類 1 関数、**先に当たった 1 つだけ**を断る（1 周に deny 1 行）。何を止めるかは
//! 種類ごとの rules 行（[`WORD_ROWS`] と [`RM_ROW`]・裁定 id つき）が持ち、見張り自身の設定の種類は行を持たない。
//!
//! 行 b が判定を持つのは語列の 3 種類（git / tmux / 台帳）で、command 行は起票の門の分割（[`segments`]・引用符と `\` を
//! 解く）で切り、`NAME=value` の前置きを読み飛ばしてから command guard と同じ照合（[`matched`]）に掛ける。rm・台帳の形・
//! 見張り自身の設定は行 c / f / e が同じ関数の中身を埋める（行 b では当たらない）。
//!
//! payload が JSON でない・`tool_name` が無い・`Bash` なのに command が無い・`--state-dir` が無い・rules が読めない周は
//! **deny**（FailClosed・[`POLARITY`]）。Bash / 編集系でない tool は 1 byte も書かず rc 0。断る周だけ `inject.jsonl` に
//! 1 行を残す（tmux は撃たない・席は null）。env も HOME も読まない（C2.2）。

use super::command::{matched, BASH};
use super::ledger_guard::{is_assignment, segments};
use super::{append, command_of, InjectionRecord, SCHEMA};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_tree::{self, Tree};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::Path;
use std::time::Instant;

/// この境界の極性: 行為の時点で止め、payload・引数・rules を読めない周は通さない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// git の履歴破壊の語列の行。
pub const GIT_ROW: &str = "host_guard.git";
/// tmux server の破壊の語列の行。
pub const TMUX_ROW: &str = "host_guard.tmux";
/// 台帳の破壊の語列の行。
pub const LEDGER_ROW: &str = "host_guard.ledger";
/// rm の守る集合の行（判定は行 c）。
pub const RM_ROW: &str = "host_guard.rm";

/// 語列の行の閉じた列（宣言順・command guard も enabled を見ずに読む）。
pub const WORD_ROWS: [&str; 3] = [GIT_ROW, TMUX_ROW, LEDGER_ROW];

/// 判定に載る tool（Bash と編集系）。これ以外は 1 byte も書かず通す。
const TOOLS: [&str; 5] = [BASH, "Edit", "Write", "MultiEdit", "NotebookEdit"];

/// 記録の置き場の flag。
const FLAG_STATE_DIR: &str = "--state-dir";
/// rules manifest を差し替える flag（歯の seam・hook と同じ形）。
const FLAG_RULES: &str = "--rules";
/// payload から拾う key（tool 名）。
const KEY_TOOL: &str = "tool_name";

/// 止める種類（閉じた 5 値・宣言順が判定の順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// git の履歴破壊。
    Git,
    /// rm の不可逆削除（行 c）。
    Rm,
    /// tmux server の破壊。
    Tmux,
    /// 台帳の破壊と素 write（形の判定は行 f）。
    Ledger,
    /// 見張り自身の設定の編集（行 e・行を持たない）。
    Settings,
}

/// [`Kind`] の全 variant（宣言順）。
pub const KINDS: &[Kind] = &[Kind::Git, Kind::Rm, Kind::Tmux, Kind::Ledger, Kind::Settings];

impl Kind {
    /// 断りの行と記録に出す種類の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::Rm => "rm",
            Self::Tmux => "tmux",
            Self::Ledger => "ledger",
            Self::Settings => "settings",
        }
    }

    /// 種類の rules 行の id（見張り自身の設定は行を持たない＝切れない唯一の種類）。
    pub fn row(self) -> Option<&'static str> {
        match self {
            Self::Git => Some(GIT_ROW),
            Self::Rm => Some(RM_ROW),
            Self::Tmux => Some(TMUX_ROW),
            Self::Ledger => Some(LEDGER_ROW),
            Self::Settings => None,
        }
    }

    /// 代わりの経路（1 種類 1 文・断りの行の末尾）。
    pub fn route(self) -> &'static str {
        match self {
            Self::Git => "履歴を残す形で書く（--force-with-lease か新しい branch）",
            Self::Rm => "tracked なら git rm・それ以外は退役の mv で脇へ移す",
            Self::Tmux => "自席の window だけを操作する（server と他の session は壊さない）",
            Self::Ledger => "台帳は bdw と --append-notes で書く",
            Self::Settings => "見張り自身の設定は user が編集する",
        }
    }
}

/// host-guard の判定。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostGuardDecision {
    /// 通す（1 byte も書かない・記録も残さない）。
    Allow,
    /// 止める。`what` は記録の `what`・`line` は stderr へ出す 1 行。
    Deny {
        /// 記録の種別（`host-guard-deny <種類の語>`）。
        what: String,
        /// stderr の 1 行。
        line: String,
    },
}

/// 読めない周の閉じた理由（FailClosed）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// 引数が `--state-dir` / `--rules` の対でない。
    BadArgs,
    /// `--state-dir` が無い。
    NoStateDir,
    /// payload が JSON として読めない。
    PayloadUnreadable,
    /// payload に `tool_name` が無い。
    NoToolName,
    /// `Bash` なのに command が無い。
    NoCommand,
    /// rules を読めない。
    RulesUnreadable,
}

impl Unreadable {
    /// 断りの行と記録に出す理由の 1 語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BadArgs => "bad-args",
            Self::NoStateDir => "no-state-dir",
            Self::PayloadUnreadable => "payload-unreadable",
            Self::NoToolName => "no-tool-name",
            Self::NoCommand => "no-command",
            Self::RulesUnreadable => "rules-unreadable",
        }
    }

    /// 断りの 1 行（種類は解けていない＝`kind=-`）。
    fn line(self) -> String {
        format!(
            "{NAME}: host-guard deny kind=- hit={} row=- ruling=- — 読めない周は通さない（fail-closed）: 配線の引数・payload・rules を直す",
            self.as_str()
        )
    }
}

/// 断る 1 件（種類・当たったもの・行 id・裁定 id）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Refusal {
    /// 種類。
    kind: Kind,
    /// 当たった語列か、解けない理由の 1 語。
    hit: String,
    /// 行 id。
    row: &'static str,
    /// 裁定 id（行が読めない周は `-`）。
    ruling: String,
}

impl Refusal {
    /// 種類の行が無い・列でない周（FailClosed）。
    fn no_row(kind: Kind, row: &'static str) -> Self {
        Self { kind, hit: "no-row".to_owned(), row, ruling: "-".to_owned() }
    }

    /// 判定へ写す。
    fn decision(self) -> HostGuardDecision {
        let line = format!(
            "{NAME}: host-guard deny kind={} hit={} row={} ruling={} — {}",
            self.kind.as_str(),
            self.hit,
            self.row,
            self.ruling,
            self.kind.route()
        );
        HostGuardDecision::Deny { what: format!("host-guard-deny {}", self.kind.as_str()), line }
    }
}

/// 判定の材料（tool と、Bash なら前置きを剥いだ segment の列）。
struct Subject<'a> {
    /// tool 名。
    tool: &'a str,
    /// segment の語（起票の門の分割・`NAME=value` の前置きを剥いだ後・空は捨てる）。
    segments: Vec<Vec<String>>,
}

/// `host-guard` に続く引数と stdin の payload を捌く。通す周は 0 byte・rc 0、断る周は rc 2・stderr 1 行・stdout 0 byte
/// （記録の置き場が解けた周は `inject.jsonl` に 1 行）。
pub fn dispatch(args: &[String], payload: &str) -> Outcome {
    let started = Instant::now();
    let Some((state_dir, rules)) = args_of(args) else {
        return Outcome::failed_line(RC_BROKEN, Unreadable::BadArgs.line());
    };
    let Some(state_dir) = state_dir else {
        return Outcome::failed_line(RC_BROKEN, Unreadable::NoStateDir.line());
    };
    let (what, line) = match decide(payload, rules) {
        Ok(HostGuardDecision::Allow) => return Outcome::ok(Vec::new()),
        Ok(HostGuardDecision::Deny { what, line }) => (what, line),
        Err(reason) => (format!("host-guard-deny reason={}", reason.as_str()), reason.line()),
    };
    let record = InjectionRecord {
        schema: SCHEMA,
        who: "host-guard".to_owned(),
        what,
        when: "PreToolUse".to_owned(),
        bytes: u64::try_from(line.len()).unwrap_or(u64::MAX).saturating_add(1),
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: None,
        ts: crate::seat::state::now_secs(),
    };
    // 記録の失敗で判定を濁さない（hook の deny と同じ・stderr は判定文 1 行だけ）。
    let _ = append(Path::new(state_dir), &record);
    Outcome::failed_line(RC_BROKEN, line)
}

/// `--state-dir D` と `--rules F` だけを読む（値欠け・空文字・重複・未知の引数は `None`）。
fn args_of(args: &[String]) -> Option<(Option<&str>, Option<&str>)> {
    let (mut state_dir, mut rules) = (None, None);
    for pair in args.chunks(2) {
        let value = pair.get(1).map(String::as_str).filter(|found| !found.trim().is_empty() && !found.starts_with("--"));
        match (pair.first().map(String::as_str), value) {
            (Some(FLAG_STATE_DIR), Some(found)) if state_dir.is_none() => state_dir = Some(found),
            (Some(FLAG_RULES), Some(found)) if rules.is_none() => rules = Some(found),
            _ => return None,
        }
    }
    Some((state_dir, rules))
}

/// payload と rules（`--rules` の差し替えか埋め込み）から判定する。読めない周は理由を `Err` で返す（FailClosed）。
pub fn decide(payload: &str, rules: Option<&str>) -> Result<HostGuardDecision, Unreadable> {
    let tree = json_tree::parse(payload).map_err(|_| Unreadable::PayloadUnreadable)?;
    let tool = tree.get(KEY_TOOL).and_then(Tree::as_str).ok_or(Unreadable::NoToolName)?;
    if !TOOLS.contains(&tool) {
        return Ok(HostGuardDecision::Allow);
    }
    let command = match tool {
        BASH => command_of(payload).ok_or(Unreadable::NoCommand)?,
        _ => String::new(),
    };
    let manifest = rules
        .map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path)))
        .map_err(|_| Unreadable::RulesUnreadable)?;
    Ok(judge(tool, &command, &manifest))
}

/// manifest の行だけから判定する（pure）。種類を宣言順に回し、先に当たった 1 つだけを断る。
pub fn judge(tool: &str, command: &str, manifest: &Manifest) -> HostGuardDecision {
    let segments = match tool {
        BASH => segments(command)
            .into_iter()
            .map(|words| words.into_iter().skip_while(|word| is_assignment(word)).collect::<Vec<String>>())
            .filter(|words| !words.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    let subject = Subject { tool, segments };
    match KINDS.iter().find_map(|kind| judge_kind(*kind, &subject, manifest)) {
        Some(found) => found.decision(),
        None => HostGuardDecision::Allow,
    }
}

/// 1 種類の判定（1 種類 1 arm）。当たらなければ `None`。
fn judge_kind(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    match kind {
        Kind::Git | Kind::Tmux | Kind::Ledger => sequences(kind, subject, manifest),
        // rm は行 c・見張り自身の設定は行 e が中身を埋める（行 b の時点では当たらない）。
        Kind::Rm | Kind::Settings => None,
    }
}

/// 語列の種類の判定: Bash の segment を種類の行の語列に掛ける（command guard と同じ照合）。行が無い・列でない周は
/// 種類の語で断り（FailClosed）、`enabled = false` の行はこの口だけを切る（通す）。
fn sequences(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    let id = kind.row().filter(|_| subject.tool == BASH)?;
    let Some(row) = manifest.get(id) else {
        return Some(Refusal::no_row(kind, id));
    };
    if !row.enabled {
        return None;
    }
    let RuleValue::List(ref denied) = row.value else {
        return Some(Refusal::no_row(kind, id));
    };
    matched(&subject.segments, denied).map(|hit| Refusal { kind, hit: hit.sequence, row: id, ruling: row.ruling.clone() })
}

#[cfg(test)]
mod tests {
    use super::{decide, judge, HostGuardDecision, Kind, Unreadable, KINDS, LEDGER_ROW, RM_ROW, TMUX_ROW, WORD_ROWS};
    use crate::hook::command::{self, denied_in, CommandDecision};
    use crate::name::NAME;
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;

    /// fixture の裁定 id。
    const RULING: &str = "user 2026-09-19T15:28Z";

    /// `runner.denied_commands` の fixture の語列（git の語列は host_guard.git と重複・tmux の語列は持たない）。
    const RUNNER: &[&str] = &["git push --force", "cargo mutants"];

    /// 1 行の本文。
    fn row(id: &str, kind: &str, value: &[&str], enabled: bool) -> String {
        let quoted: Vec<String> = value.iter().map(|item| format!("\"{item}\"")).collect();
        format!(
            "\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = [{}]\nenabled = {enabled}\nruling = \"{RULING}\"\nruled_at = \"2026-09-19\"\n",
            quoted.join(", ")
        )
    }

    /// `runner.denied_commands` と host_guard の語列の 3 行を持つ manifest（host_guard.tmux の enabled は引数・`drop` の行は
    /// 持たない）。
    fn manifest(tmux_enabled: bool, drop: Option<&str>) -> Manifest {
        let mut text = format!("schema = 1\n{}", row(command::ROW, "RunnerDeniedCommands", RUNNER, true));
        for (id, value, enabled) in [
            (super::GIT_ROW, &["git push --force", "git reset --hard"][..], true),
            (TMUX_ROW, &["tmux kill-server"][..], tmux_enabled),
            (LEDGER_ROW, &["bd delete"][..], true),
        ] {
            if drop != Some(id) {
                text.push_str(&row(id, "HostGuardDeniedCommands", value, enabled));
            }
        }
        Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// Bash の判定の (what, line)。Allow なら `None`。
    fn denied(command: &str, manifest: &Manifest) -> Option<(String, String)> {
        match judge("Bash", command, manifest) {
            HostGuardDecision::Deny { what, line } => Some((what, line)),
            HostGuardDecision::Allow => None,
        }
    }

    /// Bash の payload（command は JSON の escape を通す）。
    fn bash(command: &str) -> String {
        format!("{{\"cwd\":\"/tmp\",\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":{}}}}}", crate::fleet::json_lite::quote(command))
    }

    /// 種類は閉じた 5 値で、const slice は宣言順に 5 本（git → rm → tmux → 台帳 → 自身の設定）。行を持たないのは自身の設定だけ。
    #[test]
    fn host_guard_kind_slice_is_the_five_kinds_in_declaration_order() {
        assert!(is_declaration_order(KINDS, |kind| kind as usize), "KINDS は宣言順: {KINDS:?}");
        let words: Vec<&str> = KINDS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(words, ["git", "rm", "tmux", "ledger", "settings"], "5 値の語");
        let rows: Vec<Option<&str>> = KINDS.iter().map(|kind| kind.row()).collect();
        assert_eq!(rows, [Some(super::GIT_ROW), Some(RM_ROW), Some(TMUX_ROW), Some(LEDGER_ROW), None], "行 id");
        assert!(!is_declaration_order(&[Kind::Rm, Kind::Git], |kind| kind as usize), "述語は並べ替えを落とす");
    }

    /// 引用符の中に区切りを含む command で 2 つの口の判定が割れる（分割だけが 2 本・照合は同じ 1 関数）: command guard の
    /// 従来の分割は引用符の中の `;` で切って当て、host-guard は引用符を解くので当てない。逆に引用符で包んだ flag は
    /// host-guard だけが解いて当てる。
    #[test]
    fn host_guard_kind_quoted_separator_splits_the_two_mouths() {
        let manifest = manifest(true, None);
        let runner: Vec<String> = RUNNER.iter().map(|item| (*item).to_owned()).collect();
        let quoted = "echo \"x; git push --force origin\"";
        assert!(denied_in(quoted, &runner).is_some(), "command guard の分割は引用符の中の ; で切る");
        assert_eq!(denied(quoted, &manifest), None, "host-guard は引用符の中を 1 語に読む");
        let wrapped = "git push \"--force\" origin main";
        assert!(denied_in(wrapped, &runner).is_none(), "command guard は引用符を解かない");
        let (what, _) = denied(wrapped, &manifest).unwrap_or_else(|| panic!("host-guard は引用符を解いて当てる"));
        assert_eq!(what, "host-guard-deny git");
    }

    /// `NAME=value` の前置きを読み飛ばして当てる（前置きが 2 つでも）。
    #[test]
    fn host_guard_kind_skips_env_assignment_prefix() {
        let manifest = manifest(true, None);
        for (command, what) in [
            ("NAME=1 git push --force origin main", "host-guard-deny git"),
            ("A=1 B=x tmux kill-server", "host-guard-deny tmux"),
            ("cd /tmp && GIT_DIR=x git reset --hard", "host-guard-deny git"),
        ] {
            let found = denied(command, &manifest).map(|(found, _)| found);
            assert_eq!(found.as_deref(), Some(what), "{command}");
        }
    }

    /// git・tmux・台帳の語列がそれぞれ自分の行の id と裁定 id を名指し、断りの 1 行は 5 欄（kind / hit / row / ruling / 代わりの
    /// 経路）を持つ。
    #[test]
    fn host_guard_kind_each_word_kind_names_its_own_row() {
        let manifest = manifest(true, None);
        let (what, line) = denied("git push origin main --force", &manifest).unwrap_or_else(|| panic!("git は断る"));
        assert_eq!(what, "host-guard-deny git");
        assert_eq!(
            line,
            format!("{NAME}: host-guard deny kind=git hit=git push --force row=host_guard.git ruling={RULING} — {}", Kind::Git.route())
        );
        for (command, kind, id) in [("tmux -L x kill-server", Kind::Tmux, TMUX_ROW), ("bd delete s2-1", Kind::Ledger, LEDGER_ROW)] {
            let (what, line) = denied(command, &manifest).unwrap_or_else(|| panic!("{command} は断る"));
            assert_eq!(what, format!("host-guard-deny {}", kind.as_str()), "{command}");
            assert!(line.contains(&format!(" kind={} ", kind.as_str())) && line.contains(&format!(" row={id} ")), "{line}");
            assert!(line.contains(&format!(" ruling={RULING} — ")) && line.ends_with(kind.route()), "{line}");
            assert_eq!(line.lines().count(), 1, "1 行: {line}");
        }
        assert_eq!(denied("git push --force-with-lease origin x", &manifest), None, "語が違う flag は通す");
    }

    /// host_guard.tmux の `enabled = false` で、その行にだけ在る語列を host-guard は通す（他の種類は動く・同じ manifest で
    /// 行が発効なら断る＝切ったことだけが効く）。git の語列は runner.denied_commands と重複するので enabled の性質は tmux の
    /// 行で測る。
    #[test]
    fn host_guard_kind_disabled_tmux_row_passes_host_guard() {
        let off = manifest(false, None);
        assert_eq!(denied("tmux kill-server", &off), None, "切った行の語列は host-guard が通す");
        assert!(denied("git push --force", &off).is_some(), "他の種類は動く");
        let on = denied("tmux kill-server", &manifest(true, None)).map(|(what, _)| what);
        assert_eq!(on.as_deref(), Some("host-guard-deny tmux"), "発効の行なら断る");
    }

    /// 同じ `enabled = false` の fixture で、command guard の `denied_of` は host_guard.tmux の語列を行 id つきで返し、`judge`
    /// は断って行 id host_guard.tmux を名指す（enabled を見ない・disabled の行を捨てる変異で赤）。
    #[test]
    fn host_guard_kind_disabled_tmux_row_is_still_read_by_command_guard() {
        let manifest = manifest(false, None);
        let sources = command::denied_of(&manifest).unwrap_or_else(|| panic!("行が揃う"));
        assert!(sources.contains(&(TMUX_ROW, vec!["tmux kill-server".to_owned()])), "{sources:?}");
        let CommandDecision::Deny { what, line } = command::judge("tmux kill-server", &manifest) else {
            panic!("command guard は enabled を見ずに断る");
        };
        assert_eq!(what, "tmux kill-server");
        assert!(line.contains(&format!("rules 行 {TMUX_ROW} が禁じる")), "{line}");
    }

    /// 語列の行が無い manifest は、その行の種類の語で断る（FailClosed・当たらない command でも）。空の manifest は宣言順で
    /// 先の git の種類で断る。
    #[test]
    fn host_guard_kind_missing_row_fails_closed_with_the_kind_word() {
        for (id, kind) in WORD_ROWS.iter().zip(["git", "tmux", "ledger"]) {
            let (what, line) = denied("ls", &manifest(true, Some(*id))).unwrap_or_else(|| panic!("{id} が無い周は断る"));
            assert_eq!(what, format!("host-guard-deny {kind}"), "{id}");
            assert!(line.contains(&format!(" hit=no-row row={id} ruling=- — ")), "{line}");
        }
        let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(denied("ls", &empty).map(|(what, _)| what).as_deref(), Some("host-guard-deny git"));
        assert_eq!(judge("Edit", "", &empty), HostGuardDecision::Allow, "編集系には語列の種類が掛からない");
    }

    /// host_guard.tmux にだけ在る語列を command guard が断り、deny 文が行 id host_guard.tmux を名指す（runner の id を固定で
    /// 書かない）。runner と重複する語列は行の並びで先の runner.denied_commands を名指す。
    #[test]
    fn host_guard_kind_command_guard_names_the_tmux_row() {
        let manifest = manifest(true, None);
        let CommandDecision::Deny { what, line } = command::judge("tmux kill-server", &manifest) else {
            panic!("tmux の行の語列を command guard が断る");
        };
        assert_eq!(what, "tmux kill-server", "記録の what は当たった語列");
        assert!(line.starts_with(&format!("{NAME}: deny tmux kill-server は rules 行 {TMUX_ROW} が禁じる")), "{line}");
        assert!(!line.contains(command::ROW), "runner の id を名乗らない: {line}");
        let CommandDecision::Deny { line, .. } = command::judge("bd delete x", &manifest) else {
            panic!("台帳の行の語列も断る");
        };
        assert!(line.contains(&format!("rules 行 {LEDGER_ROW} が禁じる")), "{line}");
        let CommandDecision::Deny { line, .. } = command::judge("git push --force", &manifest) else {
            panic!("runner の語列は断る");
        };
        assert!(line.contains(&format!("rules 行 {} が禁じる", command::ROW)), "{line}");
    }

    /// runner.denied_commands は在るが host_guard の 1 行を欠く manifest では、command guard は当たらない command も
    /// `no-row <欠けた行の id>` で断る（`denied_of` は `None`・欠けた行を読み飛ばす変異で赤）。
    #[test]
    fn host_guard_kind_command_guard_fails_closed_without_a_host_row() {
        for id in WORD_ROWS {
            let manifest = manifest(true, Some(id));
            assert_eq!(command::denied_of(&manifest), None, "{id} を欠く周は揃わない");
            let CommandDecision::Deny { what, line } = command::judge("ls", &manifest) else {
                panic!("{id} が無い周は当たらない command も断る");
            };
            assert_eq!(what, format!("reason=no-row {id}"));
            assert!(line.contains(&format!("rules 行 {id} を読めない")), "{line}");
        }
        assert_eq!(command::judge("ls", &manifest(true, None)), CommandDecision::Allow, "揃えば当たらない command は通す");
    }

    /// git と tmux の語列を両方含む 1 command は宣言順で先の git だけを断り、deny は 1 行。
    #[test]
    fn host_guard_kind_first_kind_in_declaration_order_wins() {
        let manifest = manifest(true, None);
        for command in ["tmux kill-server; git push --force origin main", "git push --force && tmux kill-server"] {
            let (what, line) = denied(command, &manifest).unwrap_or_else(|| panic!("{command} は断る"));
            assert_eq!(what, "host-guard-deny git", "{command}");
            assert!(!line.contains("kind=tmux") && line.lines().count() == 1, "{line}");
        }
    }

    /// rm・台帳の形・見張り自身の設定の 3 arm は行 b では Allow: state dir の rm・bdw を経ない台帳の write・settings.json への
    /// Write が通る。
    #[test]
    fn host_guard_kind_rm_ledger_arm_and_settings_arms_allow_in_row_b() {
        for command in ["rm -rf /tmp/state-dir", "bd close s2-1", "bd update s2-1 --notes x"] {
            assert_eq!(decide(&bash(command), None), Ok(HostGuardDecision::Allow), "{command}");
        }
        let write = "{\"cwd\":\"/tmp\",\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"/tmp/acct/settings.json\"}}";
        assert_eq!(decide(write, None), Ok(HostGuardDecision::Allow), "settings.json への Write");
        assert!(matches!(decide(&bash("git push --force"), None), Ok(HostGuardDecision::Deny { .. })), "埋め込みの行で git は断る");
    }

    /// payload が JSON でない・tool_name が無い・Bash の command が無い・rules が読めない周は理由つきで断り、判定に載らない
    /// tool は rules を読まずに通す。
    #[test]
    fn host_guard_kind_unreadable_payload_fails_closed() {
        assert_eq!(decide("not json", None), Err(Unreadable::PayloadUnreadable));
        assert_eq!(decide("{\"cwd\":\"/tmp\"}", None), Err(Unreadable::NoToolName));
        assert_eq!(decide("{\"tool_name\":\"Bash\",\"tool_input\":{}}", None), Err(Unreadable::NoCommand));
        assert_eq!(decide(&bash("ls"), Some("/nonexistent/rules.toml")), Err(Unreadable::RulesUnreadable));
        assert_eq!(decide("{\"tool_name\":\"Read\"}", Some("/nonexistent/rules.toml")), Ok(HostGuardDecision::Allow));
    }
}
