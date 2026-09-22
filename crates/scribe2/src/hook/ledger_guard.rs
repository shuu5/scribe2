//! 起票の門（`pre-tool-use` の Bash の門・設計 docs/design/ledger-form.md §3 の 9・§6 行 d・SRS FR20 / FR51・
//! ADR-0014 §2.1）。
//!
//! `bd` / `bdw` の `create` の command 行を読み、2 つを要求する: (1) memo の create（title が `[memo]` で始まるか
//! label `intake:memo` を持つ）は `--body-file` の本文に memo の 4 節の見出し（[`MEMO_SECTIONS`]）が全部在る・
//! (2) 契約の create（acceptance に設計 pointer 行）は label `intake:memo` を持たない（4 象限の違反 2 形の 1 つを起票の
//! 時点で塞ぐ）。どちらも散文の免除を持たず、欠ければ閉じた理由（[`Refusal`]）1 つで止める。
//!
//! 読めない command（memo の create で body-file が無い・開けない）は **deny**（FailClosed・[`POLARITY`]）。memo でも
//! 契約でもない create（epic・裁定）は memo の判定に載らない。command 行は引用符
//! （`'…'` / `"…"`）と `\` を解いて語に分け、`;` / `&&` / `||` / `|` / 改行で segment に分ける。変数展開・command 置換は
//! 解かない（字面のまま読む＝body-file の path が解けなければ開けない側＝deny に倒れる）。
//!
//! memo の判定で止まらなかった `bd` / `bdw` の segment は、台帳 write の 4 形（[`FORMS`]・設計 vessel-hook.md §10）に
//! 掛ける。断る形の閉じた列は rules 行 [`ROW`] の値が持ち（裁定 id つき）、判定は [`judge_write`] 1 本が持つ。rules が
//! 読めない・行が無い・不発効・値が列でない周は `bd` / `bdw` を通さない（FailClosed・`bd` / `bdw` の無い command は
//! rules を読まずに通す）。

use crate::ledger::form::{pointer_text, MEMO_LABEL, MEMO_SECTIONS};
use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::Path;

/// この境界の極性: 起票の時点で止め、memo の本文を読めない周は create を通さない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 台帳の client の名（path の末尾の字面で照合する＝`scripts/bdw` も当たる）。
const CLIENTS: [&str; 2] = ["bd", "bdw"];

/// memo の判定に載る subcommand。
const CREATE: &str = "create";

/// 断る形の閉じた列を持つ rules 行の id。
pub const ROW: &str = "ledger.denied_writes";

/// 台帳の script を経ない client の名（path の末尾・`bdw` でない側）。
const RAW_CLIENT: &str = "bd";

/// 記憶の subcommand（`memory-subcommand` の閉じた列）。
const MEMORY: [&str; 3] = ["remember", "recall", "memories"];

/// bd の書き込みの subcommand（道具の語彙・`bd-outside-bdw` の閉じた列＝`bd` に subcommand が増えた周は手が入る）。
const WRITES: &[&str] = &[
    "create", "update", "close", "reopen", "delete", "dep", "label", "comment", "comments", "edit", "remember", "forget",
    "set-state", "rename", "rename-prefix", "move", "promote", "import", "merge", "duplicate", "supersede", "sync",
];

/// notes を丸ごと置き換える flag（`--append-notes` は別の語）。
const NOTES: &str = "--notes";

/// 親の flag。
const PARENT: &str = "--parent";

/// 台帳 write の 4 形（判定の順＝1 segment で 2 形に当たる周は先の形の理由）。
pub const FORMS: [Refusal; 4] =
    [Refusal::NotesReplace, Refusal::MemorySubcommand, Refusal::CreateWithoutParent, Refusal::BdOutsideBdw];

/// memo を名乗る title の頭。
const MEMO_TITLE: &str = "[memo]";

/// 値を取る flag のうち、門が読むもの以外（値を title の候補に数えない）。
const VALUED: &[&str] = &[
    "--type", "-t", "--priority", "-p", "--parent", "--deps", "--assignee", "-a", "--description", "-d", "--design",
    "--design-file", "--notes", "--external-ref", "--spec-id", "--metadata", "--estimate", "-e", "--defer", "--due",
    "--id", "--graph", "--file", "-f",
];

/// title の flag。
const TITLE: &str = "--title";
/// label の flag（綴り 3 つ）。
const LABELS: [&str; 3] = ["--labels", "--label", "-l"];
/// 本文の file の flag。
const BODY_FILE: &str = "--body-file";
/// acceptance の flag。
const ACCEPTANCE: &str = "--acceptance";

/// 止める閉じた理由（1 周に 1 つ・先に当たったもの）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 契約の create（acceptance に設計 pointer 行）が label `intake:memo` を持つ。
    MemoOnContract,
    /// memo の create が `--body-file` を持たない。
    NoBodyFile,
    /// memo の create の `--body-file` を開けない。
    BodyUnreadable,
    /// memo の本文に 4 節の見出しの 1 つが無い（[`MEMO_SECTIONS`] の添字・宣言順で最初の欠け）。
    MissingSection(usize),
    /// `bd` / `bdw` の語に `--notes`（`--notes=<値>` を含む）が在る。
    NotesReplace,
    /// subcommand が記憶の 3 語（[`MEMORY`]）のどれか。
    MemorySubcommand,
    /// `create` が `--parent` を持たない。
    CreateWithoutParent,
    /// 先頭語の末尾が `bd` で、subcommand が書き込みの列（[`WRITES`]）に在る。
    BdOutsideBdw,
    /// rules を読めない（断る形を解けない＝FailClosed）。
    RulesUnreadable,
    /// rules 行 [`ROW`] が無い・不発効・値が列でない（FailClosed）。
    NoRow,
}

impl Refusal {
    /// 記録と deny 文に出す理由の 1 語（4 形は rules 行 [`ROW`] の値の語と同じ字面）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotesReplace => "notes-replace",
            Self::MemorySubcommand => "memory-subcommand",
            Self::CreateWithoutParent => "create-without-parent",
            Self::BdOutsideBdw => "bd-outside-bdw",
            Self::RulesUnreadable => "rules-unreadable",
            Self::NoRow => "no-row",
            Self::MemoOnContract => "memo-on-contract",
            Self::NoBodyFile => "no-body-file",
            Self::BodyUnreadable => "body-unreadable",
            Self::MissingSection(0) => "no-source",
            Self::MissingSection(1) => "no-observation",
            Self::MissingSection(2) => "no-candidate",
            Self::MissingSection(_) => "no-promotion",
        }
    }

    /// 理由の説明と次の一手（deny 文の後半）。
    fn guidance(self) -> String {
        match self {
            Self::MemoOnContract => format!(
                "acceptance に設計 pointer 行を持つ契約は label {MEMO_LABEL} を持てない — 契約なら label を外し、memo なら pointer 行を外す"
            ),
            Self::NoBodyFile => "memo の本文は --body-file で渡す — 4 節の本文を file に書いて --body-file で名指す".to_owned(),
            Self::BodyUnreadable => "--body-file を開けない — 在る file の path を渡す".to_owned(),
            Self::MissingSection(at) => format!(
                "本文に見出し {} が無い — memo の 4 節（{}）を全部置く",
                MEMO_SECTIONS.get(at).copied().unwrap_or_default(),
                MEMO_SECTIONS.join(" / ")
            ),
            Self::NotesReplace => "--notes は notes を丸ごと置き換える — 足すなら --append-notes で書く".to_owned(),
            Self::MemorySubcommand => {
                "bd の記憶（remember / recall / memories）は使わない — 残す事実は memo か notes に書く".to_owned()
            }
            Self::CreateWithoutParent => format!(
                "親を持たない create は台帳に孤児を作る — {PARENT} で親の id を名指す（通す周は rules 行 {ROW} の値から語を外す）"
            ),
            Self::BdOutsideBdw => "bd の書き込みは台帳の script を経る — 同じ引数で bdw を撃つ".to_owned(),
            Self::RulesUnreadable => format!("rules 行 {ROW} を読めない — 断る形を解けない周は bd / bdw を通さない"),
            Self::NoRow => format!("rules 行 {ROW} が無い・不発効 — 断る形を解けない周は bd / bdw を通さない"),
        }
    }
}

/// 起票の門の判定。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerDecision {
    /// 通す（1 byte も書かない・記録も残さない）。
    Allow,
    /// 止める。`what` は記録の `what`（`ledger-deny ` の後ろ）・`line` は stderr へ出す 1 行。
    Deny {
        /// 記録の種別（理由の 1 語）。
        what: String,
        /// stderr の 1 行。
        line: String,
    },
}

/// create の command 行から門が読む分。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Create {
    /// title の候補（`--title` の値と、値を取る flag に食われない裸の語）。
    pub titles: Vec<String>,
    /// label（`,` で割った字面）。
    pub labels: Vec<String>,
    /// `--body-file` の値（最後の 1 つ）。
    pub body_file: Option<String>,
    /// `--acceptance` の値（連ねた字面）。
    pub acceptance: String,
}

impl Create {
    /// memo の create か（title が `[memo]` で始まるか label `intake:memo` を持つ）。
    pub fn is_memo(&self) -> bool {
        self.has_memo_label() || self.titles.iter().any(|title| title.trim_start().starts_with(MEMO_TITLE))
    }

    /// label `intake:memo` を持つか。
    pub fn has_memo_label(&self) -> bool {
        self.labels.iter().any(|label| label == MEMO_LABEL)
    }

    /// 契約の create か（acceptance に設計 pointer 行）。
    pub fn is_contract(&self) -> bool {
        pointer_text(&self.acceptance).is_some()
    }
}

/// command 行を捌く（cwd は body-file の相対 path を解く起点・rules は `--rules` の差し替えか埋め込み）。memo の判定が
/// 先で、止まらなければ `bd` / `bdw` の segment を台帳 write の 4 形に掛ける。当たる segment が無ければ Allow。
pub fn decide(command: &str, cwd: &Path, rules: Option<&Path>) -> LedgerDecision {
    let all = segments(command);
    let memo = all
        .iter()
        .filter_map(|words| create_of(words))
        .find_map(|create| judge(&create, |path| std::fs::read_to_string(cwd.join(path)).ok()));
    let refusal = memo.or_else(|| {
        let writes: Vec<Write> = all.iter().filter_map(|words| write_of(words)).collect();
        if writes.is_empty() {
            return None;
        }
        let manifest = rules.map_or_else(Manifest::embedded, Manifest::load);
        match manifest.map_or(Err(Refusal::RulesUnreadable), |found| forms_of(&found)) {
            Ok(forms) => writes.iter().find_map(|write| judge_write(write, &forms)),
            Err(found) => Some(found),
        }
    });
    match refusal {
        Some(found) => LedgerDecision::Deny { what: found.as_str().to_owned(), line: denied_line(found) },
        None => LedgerDecision::Allow,
    }
}

/// create 1 つの判定（pure・本文は `read` が返す＝開けない周は `None`）。通すなら `None`。
pub fn judge(create: &Create, read: impl Fn(&str) -> Option<String>) -> Option<Refusal> {
    if create.has_memo_label() && create.is_contract() {
        return Some(Refusal::MemoOnContract);
    }
    if !create.is_memo() {
        return None;
    }
    let Some(path) = create.body_file.as_deref().filter(|found| !found.trim().is_empty()) else {
        return Some(Refusal::NoBodyFile);
    };
    let Some(body) = read(path) else {
        return Some(Refusal::BodyUnreadable);
    };
    MEMO_SECTIONS
        .iter()
        .position(|heading| !body.lines().any(|line| line.trim() == *heading))
        .map(Refusal::MissingSection)
}

/// segment の語が `bd` / `bdw` の `create` なら、門が読む分を返す（それ以外は `None`）。
pub fn create_of(words: &[String]) -> Option<Create> {
    let mut rest = words.iter().skip_while(|word| is_assignment(word));
    let client = rest.next()?;
    let name = client.rsplit('/').next().unwrap_or_default();
    if !CLIENTS.contains(&name) {
        return None;
    }
    let rest: Vec<&String> = rest.collect();
    let at = rest.iter().position(|word| !word.starts_with('-'))?;
    (rest.get(at).map(|word| word.as_str()) == Some(CREATE)).then(|| flags_of(rest.get(at.saturating_add(1)..).unwrap_or_default()))
}

/// `bd` / `bdw` の segment 1 つから 4 形の判定が読む分。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Write {
    /// 先頭語の path の末尾（`bd` か `bdw`）。
    pub client: String,
    /// subcommand（flag でない最初の語・無ければ空）。
    pub subcommand: String,
    /// flag の名（`--flag=value` は `=` の前・flag でない語は含めない）。
    pub flags: Vec<String>,
}

/// segment の語が `bd` / `bdw` の command なら、4 形の判定が読む分を返す（それ以外は `None`）。
pub fn write_of(words: &[String]) -> Option<Write> {
    let mut rest = words.iter().skip_while(|word| is_assignment(word));
    let client = rest.next()?.rsplit('/').next().unwrap_or_default();
    if !CLIENTS.contains(&client) {
        return None;
    }
    let rest: Vec<&String> = rest.collect();
    let subcommand = rest.iter().find(|word| !word.starts_with('-')).map(|word| (*word).clone()).unwrap_or_default();
    let flags = rest
        .iter()
        .filter(|word| word.starts_with('-'))
        .map(|word| word.split_once('=').map_or(word.as_str(), |(flag, _)| flag).to_owned())
        .collect();
    Some(Write { client: client.to_owned(), subcommand, flags })
}

/// rules 行 [`ROW`] の値から、断る形を判定の順（[`FORMS`]）で返す。行が無い・不発効・値が列でない周は
/// [`Refusal::NoRow`]（FailClosed）。値の列に無い形は断らない。
pub fn forms_of(manifest: &Manifest) -> Result<Vec<Refusal>, Refusal> {
    let row = manifest.get(ROW).filter(|row| row.enabled).ok_or(Refusal::NoRow)?;
    let RuleValue::List(ref words) = row.value else {
        return Err(Refusal::NoRow);
    };
    Ok(FORMS.iter().copied().filter(|form| words.iter().any(|word| word == form.as_str())).collect())
}

/// segment 1 つを台帳 write の形に掛ける（**4 形の唯一の判定**・pure）。`forms` の順で最初に当たった形を返す。
pub fn judge_write(write: &Write, forms: &[Refusal]) -> Option<Refusal> {
    let has = |flag: &str| write.flags.iter().any(|found| found == flag);
    let sub = write.subcommand.as_str();
    forms.iter().copied().find(|form| match form {
        Refusal::NotesReplace => has(NOTES),
        Refusal::MemorySubcommand => MEMORY.contains(&sub),
        Refusal::CreateWithoutParent => sub == CREATE && !has(PARENT),
        Refusal::BdOutsideBdw => write.client == RAW_CLIENT && WRITES.contains(&sub),
        _ => false,
    })
}

/// `NAME=value` の env の前置きか。
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.chars().all(|found| found.is_ascii_alphanumeric() || found == '_')
    })
}

/// create の後ろの語から flag を読む（`--flag=value` と `--flag value` の両形）。
fn flags_of(words: &[&String]) -> Create {
    let mut create = Create::default();
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at = at.saturating_add(1);
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) if word.starts_with('-') => (flag, Some(value.to_owned())),
            _ => (word.as_str(), None),
        };
        let takes = flag == TITLE || LABELS.contains(&flag) || flag == BODY_FILE || flag == ACCEPTANCE || VALUED.contains(&flag);
        if !flag.starts_with('-') {
            create.titles.push(flag.to_owned());
            continue;
        }
        if !takes {
            continue;
        }
        let value = inline.or_else(|| {
            let next = words.get(at).map(|found| (*found).clone());
            at = at.saturating_add(usize::from(next.is_some()));
            next
        });
        apply(&mut create, flag, value.unwrap_or_default());
    }
    create
}

/// 値を取る flag 1 つを反映する（門が読まない flag は捨てる）。
fn apply(create: &mut Create, flag: &str, value: String) {
    if flag == TITLE {
        create.titles.push(value);
    } else if LABELS.contains(&flag) {
        create.labels.extend(value.split(',').map(|label| label.trim().to_owned()).filter(|label| !label.is_empty()));
    } else if flag == BODY_FILE {
        create.body_file = Some(value);
    } else if flag == ACCEPTANCE {
        create.acceptance.push_str(&value);
        create.acceptance.push('\n');
    }
}

/// 字句の状態（引用符の中か）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    /// 引用符の外。
    Bare,
    /// `'…'` の中（`\` を解かない）。
    Single,
    /// `"…"` の中。
    Double,
}

/// command 行を segment（語の列）に分ける。区切りは引用符の外の `;` / `&` / `|` / 改行（空 segment は捨てる）。
pub fn segments(command: &str) -> Vec<Vec<String>> {
    let mut all: Vec<Vec<String>> = Vec::new();
    let (mut words, mut word, mut quoted) = (Vec::new(), String::new(), false);
    let mut quote = Quote::Bare;
    let mut chars = command.chars();
    while let Some(found) = chars.next() {
        match (quote, found) {
            (Quote::Bare, '\'') => (quote, quoted) = (Quote::Single, true),
            (Quote::Bare, '"') => (quote, quoted) = (Quote::Double, true),
            (Quote::Single, '\'') | (Quote::Double, '"') => quote = Quote::Bare,
            (Quote::Bare | Quote::Double, '\\') => word.extend(chars.next()),
            (Quote::Bare, ';' | '&' | '|' | '\n') => {
                flush(&mut words, &mut word, &mut quoted);
                all.push(std::mem::take(&mut words));
            }
            (Quote::Bare, blank) if blank.is_whitespace() => flush(&mut words, &mut word, &mut quoted),
            (_, other) => word.push(other),
        }
    }
    flush(&mut words, &mut word, &mut quoted);
    all.push(words);
    all.retain(|segment| !segment.is_empty());
    all
}

/// 溜めた語を 1 つ確定する（引用符だけの空語も 1 語に数える）。
fn flush(words: &mut Vec<String>, word: &mut String, quoted: &mut bool) {
    if !word.is_empty() || *quoted {
        words.push(std::mem::take(word));
    }
    *quoted = false;
}

/// deny 文: 理由の 1 語と説明・次の一手を 1 行で（memo の理由と台帳 write の形で頭と出所が分かれる）。
fn denied_line(refusal: Refusal) -> String {
    match refusal {
        Refusal::MemoOnContract | Refusal::NoBodyFile | Refusal::BodyUnreadable | Refusal::MissingSection(_) => format!(
            "{NAME}: deny bd create は起票の門が止める reason={}（{}・ledger-form.md §3 の 9）",
            refusal.as_str(),
            refusal.guidance()
        ),
        _ => format!(
            "{NAME}: deny 台帳の write は起票の門が止める reason={}（{}・vessel-hook.md §10）",
            refusal.as_str(),
            refusal.guidance()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{create_of, decide, forms_of, judge, judge_write, segments, write_of, LedgerDecision, Refusal, FORMS, ROW};
    use crate::rules::manifest::Manifest;

    /// 本文の 4 節。
    const FULL: &str = "## memo\n### 出所\n- x\n### 観測\n### 候補\n### 昇格条件\n";

    /// 引用符・`\`・区切りを解いて語に分ける（引用符の中の `;` は区切りでない）。
    #[test]
    fn hook_memo_guard_segments_respect_quotes() {
        let found = segments("bd create \"[memo] a; b\" --labels='intake:memo' && echo x\\ y");
        assert_eq!(
            found,
            vec![
                vec!["bd".to_owned(), "create".to_owned(), "[memo] a; b".to_owned(), "--labels=intake:memo".to_owned()],
                vec!["echo".to_owned(), "x y".to_owned()],
            ]
        );
    }

    /// title・label・body-file・acceptance を両形で読み、値を取る flag の値を title に数えない。
    #[test]
    fn hook_memo_guard_reads_create_flags() {
        let words = |line: &str| segments(line).into_iter().next().unwrap_or_default();
        let found = create_of(&words("scripts/bdw create --type task --title=x -l a,intake:memo --body-file f --acceptance 'design = d#a'"));
        let Some(create) = found else {
            panic!("bdw create を読む");
        };
        assert_eq!(create.titles, ["x"]);
        assert_eq!(create.labels, ["a", "intake:memo"]);
        assert_eq!(create.body_file.as_deref(), Some("f"));
        assert!(create.is_memo() && create.is_contract());
        assert_eq!(create_of(&words("bd update x --title create")), None, "create 以外の subcommand");
        assert_eq!(create_of(&words("echo bd create")), None, "client でない先頭語");
    }

    /// 判定の順: 契約 × label → body-file の不在 → 開けない → 4 節の最初の欠け。
    #[test]
    fn hook_memo_guard_judge_orders_reasons() {
        let words = |line: &str| segments(line).into_iter().next().unwrap_or_default();
        let judged = |line: &str, body: Option<&str>| {
            let create = create_of(&words(line)).unwrap_or_default();
            judge(&create, |_| body.map(str::to_owned))
        };
        assert_eq!(judged("bd create x --labels intake:memo --acceptance 'design = d#a'", Some(FULL)), Some(Refusal::MemoOnContract));
        assert_eq!(judged("bd create '[memo] x'", Some(FULL)), Some(Refusal::NoBodyFile));
        assert_eq!(judged("bd create '[memo] x' --body-file f", None), Some(Refusal::BodyUnreadable));
        assert_eq!(judged("bd create '[memo] x' --body-file f", Some("### 出所\n### 候補\n")), Some(Refusal::MissingSection(1)));
        assert_eq!(judged("bd create '[memo] x' --body-file f", Some(FULL)), None);
        assert_eq!(judged("bd create x --type epic", None), None, "memo でも契約でもない");
        assert_eq!(judged("bd create x --acceptance 'design = d#a'", None), None, "label の無い契約");
    }

    /// segment 1 つを 4 形の全部に掛ける（`None` は bd / bdw でない）。
    fn formed(line: &str) -> Option<Option<Refusal>> {
        let words = segments(line).into_iter().next().unwrap_or_default();
        write_of(&words).map(|write| judge_write(&write, &FORMS))
    }

    /// 4 形 × 当たる例: bd と bdw のどちらでも `--notes` の両形が notes-replace・記憶の 3 語が memory-subcommand・
    /// `--parent` の無い create が create-without-parent・`bd` の書き込みが bd-outside-bdw。
    #[test]
    fn hook_ledger_write_judge_hits_each_form() {
        for line in ["bd update s2-1 --notes x", "bdw update s2-1 --notes=x", "scripts/bdw update s2-1 --notes x", "bd update s2-1 --notes=x"] {
            assert_eq!(formed(line), Some(Some(Refusal::NotesReplace)), "{line}");
        }
        for line in ["bdw remember x", "bdw recall x", "bdw memories", "bd remember x", "bd recall x", "bd memories"] {
            assert_eq!(formed(line), Some(Some(Refusal::MemorySubcommand)), "{line}");
        }
        for line in ["bdw create x --type task", "X=1 bdw create --title=x", "bd create x"] {
            assert_eq!(formed(line), Some(Some(Refusal::CreateWithoutParent)), "{line}");
        }
        for line in ["bd update s2-1 --status open", "bd close s2-1", "/usr/bin/bd dep add a b", "bd create x --parent=s2-1"] {
            assert_eq!(formed(line), Some(Some(Refusal::BdOutsideBdw)), "{line}");
        }
    }

    /// 4 形 × 当たらない例（空虚さの柵）: `--append-notes`・`bdw` の書き込み・`--parent` を持つ create・読みの subcommand・
    /// bd / bdw でない先頭語。
    #[test]
    fn hook_ledger_write_judge_passes_the_near_misses() {
        for line in [
            "bdw update s2-1 --append-notes x",
            "bdw update s2-1 --append-notes=--notes",
            "bdw close s2-1 --reason x",
            "bdw create x --parent s2-1",
            "bdw create --parent=s2-1 --title=x",
            "bd list --json",
            "bd show s2-1",
            "bd ready",
            "bdw show s2-1",
        ] {
            assert_eq!(formed(line), Some(None), "{line}");
        }
        for line in ["echo bd update --notes x", "bdwx update --notes x", "mybd close x"] {
            assert_eq!(formed(line), None, "{line}");
        }
    }

    /// 値の列に載る形だけを断り、行が無い・不発効・値が列でない周は no-row（FailClosed）。
    #[test]
    fn hook_ledger_write_forms_read_the_row_and_fail_closed() {
        let manifest = |value: &str, enabled: bool| {
            Manifest::parse(&format!(
                "schema = 1\n\n[[rule]]\nid = \"{ROW}\"\nkind = \"LedgerDeniedWrites\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n"
            ))
            .unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
        };
        let full = "[\"bd-outside-bdw\", \"notes-replace\", \"memory-subcommand\", \"create-without-parent\"]";
        assert_eq!(forms_of(&manifest(full, true)), Ok(FORMS.to_vec()), "判定の順は FORMS（値の並びに依らない）");
        let one = forms_of(&manifest("[\"notes-replace\", \"unknown\"]", true));
        assert_eq!(one, Ok(vec![Refusal::NotesReplace]), "列に載る形だけ");
        let write = write_of(&segments("bd create x").into_iter().next().unwrap_or_default()).unwrap_or_default();
        assert_eq!(judge_write(&write, &[Refusal::NotesReplace]), None, "列に無い形は断らない");
        assert_eq!(forms_of(&manifest(full, false)), Err(Refusal::NoRow), "不発効");
        let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(forms_of(&empty), Err(Refusal::NoRow), "行が無い");
        let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(forms_of(&embedded), Ok(FORMS.to_vec()), "埋め込みの行は 4 形を全部断る");
    }

    /// decide: memo の理由が先・rules が読めない / 行が無い周は bd / bdw だけを断り、bd / bdw の無い command は通す。
    #[test]
    fn hook_ledger_write_decide_orders_memo_first_and_fails_closed() {
        let cwd = std::path::Path::new("/nonexistent-ledger-guard-cwd");
        let what = |line: &str, rules: Option<&std::path::Path>| match decide(line, cwd, rules) {
            LedgerDecision::Deny { what, line } => {
                assert_eq!(line.lines().count(), 1, "1 行: {line}");
                assert!(line.contains(&format!("reason={what}（")), "理由を名指す: {line}");
                what
            }
            LedgerDecision::Allow => String::new(),
        };
        assert_eq!(what("bd create '[memo] x'", None), "no-body-file", "memo の理由は 4 形より先");
        assert_eq!(what("bdw update s2-1 --notes x", None), "notes-replace");
        assert_eq!(what("ls && bd close s2-1", None), "bd-outside-bdw", "後ろの segment も読む");
        assert_eq!(what("bdw create x --parent s2-1", None), "");
        let missing = std::path::Path::new("/nonexistent-ledger-guard-rules.toml");
        assert_eq!(what("bdw show s2-1", Some(missing)), "rules-unreadable", "rules が読めない周");
        assert_eq!(what("cargo build", Some(missing)), "", "bd / bdw の無い command は rules を読まない");
    }
}
