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
//! label `intake:question` を持つ create は memo の判定の代わりに問いの段（[`crate::ledger::question::judge`]・
//! ledger-form.md §14）に掛かる。本文（body-file と `-d`）と metadata（`@<path>` は cwd から）の字は門が読んで渡す。
//!
//! memo の判定で止まらなかった `bd` / `bdw` の segment は、台帳 write の 6 形（[`FORMS`]・設計 vessel-hook.md §10・
//! ledger-form.md §11）に
//! 掛ける。断る形の閉じた列は rules 行 [`ROW`] の値が持ち（裁定 id つき）、判定は [`judge_write`] 1 本が持つ。rules が
//! 読めない・行が無い・不発効・値が列でない周は `bd` / `bdw` を通さない（FailClosed・`bd` / `bdw` の無い command は
//! rules を読まずに通す）。
//!
//! 引き金の段（ledger-form.md §15）: memo の判定で止まらない memo の create と 6 形で止まらない本文を書く update は、昇格条件
//! の節に読める引き金の行（[`trigger::read`]）が無ければ止める。台帳は読まず、接頭辞は cwd から上の `.beads` の設定から解く。

use crate::ledger::close_reason::{self, Defect, Form, Head};
use crate::ledger::form::{pointer_text, MEMO_LABEL, MEMO_SECTIONS, QUESTION_LABEL};
use crate::ledger::question::{self, Gap, Metadata};
use crate::ledger::trigger;
use crate::name::NAME;
use crate::pipe::declaration::{close_check, CloseCheck};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use crate::seat::brief::pointer::Anchor;
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

/// 引き金の段に載る本文を書く subcommand。
const UPDATE: &str = "update";

/// 台帳の dir（接頭辞を解く root の印）。
const BEADS: &str = ".beads";

/// close の理由の flag（綴り 2 つ・`=` の形も読む）。
const REASON: [&str; 2] = ["-r", "--reason"];

/// close の理由を file から読む flag。
const REASON_FILE: &str = "--reason-file";

/// `gate resolve`（理由を持つ close の口）の 2 語。
const GATE: &str = "gate";
const RESOLVE: &str = "resolve";

/// 宣言を読めない repo で close の段に当たった周の記録の語。
const CLOSE_UNREADABLE: &str = "close-declaration-unreadable";

/// 席が書ける理由の 8 つの頭の字面（landed は器だけが書く）。
const CLOSE_FORMS: &str = "重複 <bead id>・後継 <bead id>・取り下げ <理由>・裁定 <裁定 id>・昇格済み <契約 id …>・まとめた <memo id>・見送り <裁定 id>・完了";

/// 断る形の閉じた列を持つ rules 行の id。
pub const ROW: &str = "ledger.denied_writes";

/// 台帳の script を経ない client の名（path の末尾・`bdw` でない側）。
const RAW_CLIENT: &str = "bd";

/// 記憶の subcommand（`memory-subcommand` の閉じた列）。
const MEMORY: [&str; 3] = ["remember", "recall", "memories"];

/// bd の書き込みの subcommand（道具の語彙・`bd-outside-bdw` の閉じた列＝`bd` に subcommand が増えた周は手が入る・host-guard
/// は `ledger.denied_writes` が不発効の周にこの列を全部断る＝設計 vessel-hook.md §11 の形 f 2）。
pub(crate) const WRITES: &[&str] = &[
    "create", "update", "close", "reopen", "delete", "dep", "label", "comment", "comments", "edit", "remember", "forget",
    "set-state", "rename", "rename-prefix", "move", "promote", "import", "merge", "duplicate", "supersede", "sync",
];

/// notes を丸ごと置き換える flag（`--append-notes` は別の語）。
const NOTES: &str = "--notes";

/// 親の flag。
const PARENT: &str = "--parent";

/// 親を運べない起票の subcommand（`create-bypass`・道具の語彙・設計 ledger-form.md §11 の 2）。
const BYPASS: [&str; 3] = ["q", "create-form", "batch"];

/// 次の語が [`ADD`] のとき親を運べない起票になる subcommand。
const TODO: &str = "todo";

/// 辺を足す語（`todo add` / `dep add`）。
const ADD: &str = "add";

/// 辺の subcommand（次の語が [`ADD`] のとき辺を張る）。
const DEP: &str = "dep";

/// 辺を張る subcommand（既定の型は blocks）。
const LINK: &str = "link";

/// 辺の型の flag（綴り 2 つ・create の型の flag と同じ綴り）。
const EDGE_TYPE: [&str; 2] = ["--type", "-t"];

/// 親子の辺の型。
const PARENT_CHILD: &str = "parent-child";

/// `dep add` の辺を file から読む flag（中身は読まない＝fail-closed）。
const EDGE_FILE: &str = "--file";

/// create の辺の flag（値は `<型>:<id>` の `,` 区切り）。
const DEPS: &str = "--deps";

/// 台帳 write の 6 形（判定の順＝1 segment で 2 形に当たる周は先の形の理由）。
pub const FORMS: [Refusal; 6] = [
    Refusal::NotesReplace,
    Refusal::MemorySubcommand,
    Refusal::CreateWithoutParent,
    Refusal::BdOutsideBdw,
    Refusal::CreateBypass,
    Refusal::ParentEdge,
];

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
/// plan の file の flag（`create --graph`）。
const GRAPH: &str = "--graph";
/// 本文の字の flag（綴り 2 つ）。
const DESCRIPTION: [&str; 2] = ["--description", "-d"];
/// metadata の flag（JSON の字か `@<path>`）。
const METADATA: &str = "--metadata";
/// 本文を標準入力から読む flag（`--body-file -` の別名）。
const STDIN: &str = "--stdin";
/// 親の label を継がない flag。
const NO_INHERIT: &str = "--no-inherit-labels";

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
    /// 親を運べない起票の口（subcommand が [`BYPASS`] のどれかか、`todo` の次の語が `add`）。
    CreateBypass,
    /// parent-child の辺を横から張る書き（`dep add` / `link` の型が parent-child・`dep add --file`・create の `--deps` の
    /// 値に `parent-child:`）。
    ParentEdge,
    /// rules を読めない（断る形を解けない＝FailClosed）。
    RulesUnreadable,
    /// rules 行 [`ROW`] が無い・不発効・値が列でない（FailClosed）。
    NoRow,
}

impl Refusal {
    /// 記録と deny 文に出す理由の 1 語（6 形は rules 行 [`ROW`] の値の語と同じ字面）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotesReplace => "notes-replace",
            Self::MemorySubcommand => "memory-subcommand",
            Self::CreateWithoutParent => "create-without-parent",
            Self::BdOutsideBdw => "bd-outside-bdw",
            Self::CreateBypass => "create-bypass",
            Self::ParentEdge => "parent-edge",
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
            Self::CreateBypass => format!(
                "q / todo add / batch / create-form は親を運べない起票の口 — bdw create <題> {PARENT} <epic> で撃つ"
            ),
            Self::ParentEdge => format!(
                "parent-child の辺を横から張ると親が 2 つや親の輪を作る — 親は bdw update <子> {PARENT} <親> で付け替える（1 本に置き換わる）"
            ),
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
    /// `--parent` の値（最後の 1 つ・空の値も運ぶ・台帳の形の門 [`super::graph_guard`] が読む）。
    pub parent: Option<String>,
    /// `--type` / `-t` の値（最後の 1 つ）。
    pub kind: Option<String>,
    /// `--graph` の plan の file（最後の 1 つ）。
    pub graph: Option<String>,
    /// `-d` / `--description` の値（最後の 1 つ）。
    pub description: Option<String>,
    /// `--metadata` の値（最後の 1 つ）。
    pub metadata: Option<String>,
    /// `--stdin` を持つか（`=false` の周だけ持たない）。
    pub stdin: bool,
    /// `--no-inherit-labels` を持つか（裸か `=true` の周だけ持つ）。
    pub no_inherit_labels: bool,
}

impl Create {
    /// 台帳の問いの create か（label `intake:question` を持つ）。
    pub fn has_question_label(&self) -> bool {
        self.labels.iter().any(|label| label == QUESTION_LABEL)
    }

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
/// 先で（label `intake:question` の create は代わりに問いの段）、止まらなければ `bd` / `bdw` の segment を台帳 write の
/// 6 形に掛ける。当たる segment が無ければ Allow。
pub fn decide(command: &str, cwd: &Path, rules: Option<&Path>) -> LedgerDecision {
    let all = segments(command);
    let read = |path: &str| std::fs::read_to_string(cwd.join(path)).ok();
    let created = all.iter().filter_map(|words| create_of(words)).find_map(|create| {
        if create.has_question_label() {
            return question_gap(&create, read).map(|gap| deny(gap.as_str(), question_line(&gap)));
        }
        let found = judge(&create, read).map(|found| deny(found.as_str(), denied_line(found)));
        found.or_else(|| memo_untriggered(&create, read, cwd).map(Untriggered::decision))
    });
    if let Some(found) = created {
        return found;
    }
    let writes: Vec<Write> = all.iter().filter_map(|words| write_of(words)).collect();
    if writes.is_empty() {
        return LedgerDecision::Allow;
    }
    let manifest = rules.map_or_else(Manifest::embedded, Manifest::load);
    let refusal = match manifest.map_or(Err(Refusal::RulesUnreadable), |found| forms_of(&found)) {
        Ok(forms) => writes.iter().find_map(|write| judge_write(write, &forms)),
        Err(found) => Some(found),
    };
    if let Some(found) = refusal {
        return deny(found.as_str(), denied_line(found));
    }
    let mut updates = all.iter().filter_map(|words| command_of(words, UPDATE));
    if let Some(found) = updates.find_map(|update| update_untriggered(&update, read, cwd)) {
        return found.decision();
    }
    closes_denied(&writes, cwd, &read).unwrap_or(LedgerDecision::Allow)
}

/// close の段（ledger-form.md §16）: close の segment が在る周だけ HEAD の宣言を 1 回読み、加わる周と読めない周に掛ける。
fn closes_denied(writes: &[Write], cwd: &Path, read: &impl Fn(&str) -> Option<String>) -> Option<LedgerDecision> {
    if !writes.iter().any(is_close) {
        return None;
    }
    let root = ledger_root(cwd);
    let check = root.map_or(CloseCheck::Exempt, close_check);
    let prefix = root.filter(|_| check != CloseCheck::Exempt).and_then(prefix_at);
    close_stage(check, writes, prefix.as_deref(), read)
}

/// close の段の判定（pure・3 値ごとの掛け方）: 加わらない周は撃たず、加わる周は最初に外れた理由の語で、読めない周は同じ判定で
/// 当たった語を名指す close-declaration-unreadable で断る。
fn close_stage(
    check: CloseCheck,
    writes: &[Write],
    prefix: Option<&str>,
    read: &impl Fn(&str) -> Option<String>,
) -> Option<LedgerDecision> {
    if check == CloseCheck::Exempt {
        return None;
    }
    let found = writes.iter().filter(|write| is_close(write)).find_map(|write| judge_close(write, prefix, read))?;
    let (what, why) = if check == CloseCheck::Unreadable {
        let fix = format!(
            "当たった形は {}（{}）。この repo の宣言 .vessel.toml を読めない — 不備を直す（close-check の値は true か false・contracts check --repo で key と行番号を読める）",
            found.as_str(),
            found.guidance()
        );
        (CLOSE_UNREADABLE, fix)
    } else {
        (found.as_str(), found.guidance())
    };
    Some(deny(what, format!("{NAME}: deny bd close は起票の門が止める reason={what}（{why}・ledger-form.md §16）")))
}

/// 理由を持つ close の口（`close`・別名 `done`・`gate resolve`）か。
fn is_close(write: &Write) -> bool {
    matches!(write.subcommand.as_str(), "close" | "done") || (write.subcommand == GATE && write.second() == RESOLVE)
}

/// close の断りの語（行 l1 の閉じた 4 語・宣言を読めない周は [`CLOSE_UNREADABLE`] に包む）。値は説明の材料。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Close {
    /// 理由の値を 1 つも持たないか空。
    NoReason,
    /// 頭が landed（値の形を問わない）。
    Landed,
    /// 頭の外・値の形の外・id を読めない（理由の先頭 40 字と欠陥）。
    Outside(String),
    /// 読めない形（`--reason-file` の `-`・値なし・開けない file・`$` か backtick の値）。
    Unreadable(String),
}

impl Close {
    /// 記録と deny 文の理由の 1 語。
    fn as_str(&self) -> &'static str {
        match self {
            Self::NoReason => "close-no-reason",
            Self::Landed => "close-landed",
            Self::Outside(_) => "close-outside-forms",
            Self::Unreadable(_) => "close-reason-unreadable",
        }
    }

    /// 説明と次の一手（断り文の後半）。
    fn guidance(&self) -> String {
        match self {
            Self::NoReason => format!("理由の無い close は席に許さない — --reason '<形>' を渡す（形: {CLOSE_FORMS}）"),
            Self::Landed => "着地の形は land の終端と pipe retire だけが書く — 止まった終端は原因を直して pipe land --run <run> --terminal-only で撃ち直し、PR の便は pipe retire --run <run>（どちらも名指しの 1 行なら席の決着の権能 settle で撃てる・seat-roles.md §32）".to_owned(),
            Self::Outside(what) => format!("理由が和の外 {what} — 次のどれかで書く: {CLOSE_FORMS}"),
            Self::Unreadable(what) => format!("理由を読めない: {what} — 字のままの --reason '<形>' で渡す"),
        }
    }
}

/// close 1 つの理由の全部（`-r`・`--reason`・`--reason-file` の値を出てきた順に）を読み手に掛け、最初に外れた語を返す。
/// 理由の値を 1 つも持たない close は [`Close::NoReason`]（`-` で始まる値は flag と読まれ持たない側に倒れる）。
fn judge_close(write: &Write, prefix: Option<&str>, read: &impl Fn(&str) -> Option<String>) -> Option<Close> {
    let mut seen = false;
    for (flag, value) in &write.values {
        let text = if REASON.contains(&flag.as_str()) {
            if value.contains(['$', '`']) {
                return Some(Close::Unreadable(format!("{flag} の値に $ か backtick（展開の後の字を門は知らない）")));
            }
            value.clone()
        } else if flag == REASON_FILE {
            match value.trim() {
                "" => return Some(Close::Unreadable(format!("{REASON_FILE} の値が空"))),
                path => match read(path) {
                    Some(text) => text,
                    None => return Some(Close::Unreadable(format!("{REASON_FILE} の {path} を開けない"))),
                },
            }
        } else {
            continue;
        };
        seen = true;
        if let Some(found) = judge_reason(&text, prefix) {
            return Some(found);
        }
    }
    let files = write.flags.iter().filter(|flag| *flag == REASON_FILE).count();
    let valued = write.values.iter().filter(|(flag, _)| flag == REASON_FILE).count();
    if files > valued {
        return Some(Close::Unreadable(format!("{REASON_FILE} が - か値なし")));
    }
    (!seen).then_some(Close::NoReason)
}

/// 理由の字 1 つを読み手（[`close_reason::read`]）に掛ける。頭が landed なら値の形に依らず [`Close::Landed`]。
fn judge_reason(text: &str, prefix: Option<&str>) -> Option<Close> {
    match close_reason::read(text, prefix) {
        Ok(Form::Landed { .. }) | Err(Defect::Value(Head::Landed)) => Some(Close::Landed),
        Ok(_) => None,
        Err(Defect::Empty) => Some(Close::NoReason),
        Err(defect) => {
            let head: String = text.trim().chars().take(40).map(|found| if found.is_control() { ' ' } else { found }).collect();
            let why = match defect {
                Defect::Value(found) => format!("{} の値の形の外", found.as_str()),
                Defect::NoPrefix => "台帳の接頭辞が解けず bead id を読めない".to_owned(),
                _ => "頭が 8 つの外".to_owned(),
            };
            Some(Close::Outside(format!("「{head}」（{why}）")))
        }
    }
}

/// 引き金の段の断り（ledger-form.md §15・[`Refusal`] に変種を足さない）。値は最初の読めない行（先頭 40 字と理由）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Untriggered {
    /// memo の create の昇格条件に読める引き金の行が無い。
    Create(Option<String>),
    /// update が書く本文の昇格条件に読める引き金の行が無い。
    Update(Option<String>),
    /// 本文を書く update の本文を読めない。
    UpdateUnreadable,
}

impl Untriggered {
    /// 記録の語と `deny bd <create|update>` の頭の 1 行。
    fn decision(self) -> LedgerDecision {
        let (verb, what) = match self {
            Self::Create(_) => (CREATE, "no-trigger"),
            Self::Update(_) => (UPDATE, "update-no-trigger"),
            Self::UpdateUnreadable => (UPDATE, "update-body-unreadable"),
        };
        let why = match self {
            Self::Create(first) | Self::Update(first) => format!(
                "昇格条件の節に読める引き金の行が無い{} — {} のどれかを 1 行置く",
                first.map(|text| format!("（最初の読めない行: {text}）")).unwrap_or_default(),
                trigger::shapes()
            ),
            Self::UpdateUnreadable => "本文を書く update の本文を読めない（--stdin・--body-file の - か値なし・開けない file・$ か backtick を含む -d） — 本文を file に書いて --body-file で名指す".to_owned(),
        };
        deny(what, format!("{NAME}: deny bd {verb} は起票の門が止める reason={what}（{why}・ledger-form.md §15）"))
    }
}

/// memo の判定で止まらなかった memo の create の引き金の段（非 memo と memo の判定が断った create は掛からない）。
fn memo_untriggered(create: &Create, read: impl Fn(&str) -> Option<String>, cwd: &Path) -> Option<Untriggered> {
    let body = create.is_memo().then(|| create.body_file.as_deref().and_then(read)).flatten()?;
    untriggered(&body, cwd).map(Untriggered::Create)
}

/// update の引き金の段（本文を書かない update は掛からない・読めない本文は bead の種類に依らず断る＝fail-closed）。
fn update_untriggered(update: &Create, read: impl Fn(&str) -> Option<String>, cwd: &Path) -> Option<Untriggered> {
    if !update.stdin && update.body_file.is_none() && update.description.is_none() {
        return None;
    }
    let Some(body) = body_text(update, &read) else {
        return Some(Untriggered::UpdateUnreadable);
    };
    untriggered(&body, cwd).map(Untriggered::Update)
}

/// 本文が昇格条件の節を持ち読める引き金の行が無ければ `Some`（中身は最初の読めない行）。節の無い本文は `None`。
fn untriggered(body: &str, cwd: &Path) -> Option<Option<String>> {
    let reading = trigger::read(body, "", ledger_prefix(cwd).as_deref());
    let first = || reading.first_unreadable().map(|(text, why)| format!("{}・{}", text.chars().take(40).collect::<String>(), why.describe()));
    (reading.section && reading.readable().next().is_none()).then(first)
}

/// cwd から上へ辿った最初の `.beads` の dir を持つ dir の台帳の接頭辞（bd が台帳を探す向き・解けない周は `None`）。
fn ledger_prefix(cwd: &Path) -> Option<String> {
    prefix_at(ledger_root(cwd)?)
}

/// cwd から上へ辿った最初の `.beads` の dir を持つ dir（台帳の根・無ければ `None`）。
fn ledger_root(cwd: &Path) -> Option<&Path> {
    cwd.ancestors().find(|dir| dir.join(BEADS).is_dir())
}

/// 台帳の根の接頭辞。
fn prefix_at(root: &Path) -> Option<String> {
    Anchor::open(root)?.prefixes().first().cloned()
}

/// 止める判定 1 つ。
fn deny(what: &str, line: String) -> LedgerDecision {
    LedgerDecision::Deny { what: what.to_owned(), line }
}

/// 問いの create 1 つを問いの段に掛ける（pure の判定へ渡す字を読む・`read` は cwd から解いた file の字）。通すなら `None`。
fn question_gap(create: &Create, read: impl Fn(&str) -> Option<String>) -> Option<Gap> {
    let inherits = create.parent.as_deref().filter(|parent| !parent.trim().is_empty() && !create.no_inherit_labels);
    let body = body_text(create, &read);
    let file: Option<String>;
    let metadata = match create.metadata.as_deref() {
        None => Metadata::Absent,
        Some(value) => match value.strip_prefix('@') {
            Some(path) => {
                file = read(path);
                file.as_deref().map_or(Metadata::Unreadable, Metadata::Text)
            }
            None => Metadata::Text(value),
        },
    };
    question::judge(&create.labels, inherits, body.as_deref(), metadata)
}

/// 問いと update の本文（body-file の字と `-d` の最後の値を行の列として合わせる）。`--stdin`・値が `-` か無い `--body-file`・
/// 開けない file・`$` か backtick を含む `-d` の値は読めない（`None`）。
fn body_text(create: &Create, read: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let file = match create.body_file.as_deref().map(str::trim) {
        _ if create.stdin => return None,
        None => String::new(),
        Some("" | "-") => return None,
        Some(path) => read(path)?,
    };
    let text = create.description.as_deref().unwrap_or_default();
    (!text.contains(['$', '`'])).then(|| format!("{file}\n{text}"))
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
    command_of(words, CREATE)
}

/// segment の語が `bd` / `bdw` の `sub` なら flag を読んだ分を返す（create と update が同じ読み [`flags_of`] を引く）。
fn command_of(words: &[String], sub: &str) -> Option<Create> {
    let mut rest = words.iter().skip_while(|word| is_assignment(word));
    let client = rest.next()?;
    let name = client.rsplit('/').next().unwrap_or_default();
    if !CLIENTS.contains(&name) {
        return None;
    }
    let rest: Vec<&String> = rest.collect();
    let at = rest.iter().position(|word| !word.starts_with('-'))?;
    (rest.get(at).map(|word| word.as_str()) == Some(sub)).then(|| flags_of(rest.get(at.saturating_add(1)..).unwrap_or_default()))
}

/// `bd` / `bdw` の segment 1 つから 6 形の判定が読む分。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Write {
    /// 先頭語の path の末尾（`bd` か `bdw`）。
    pub client: String,
    /// subcommand（flag でない最初の語・無ければ空）。
    pub subcommand: String,
    /// flag の名（`--flag=value` は `=` の前・flag でない語は含めない）。
    pub flags: Vec<String>,
    /// flag でない語（出てきた順・subcommand と、flag の後ろに離れて置かれた値も含む）。
    pub words: Vec<String>,
    /// flag の名と値（`--flag=value` は `=` の後ろ・`--flag value` は次の語が flag でなければその語）。
    pub values: Vec<(String, String)>,
}

impl Write {
    /// subcommand の次の flag でない語（無ければ空）。
    fn second(&self) -> &str {
        self.words.get(1).map_or("", String::as_str)
    }

    /// flag を持つか（名で照合する）。
    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|found| found == flag)
    }

    /// `flags` のどれかの値の列。
    fn values_of<'a>(&'a self, flags: &'a [&str]) -> impl Iterator<Item = &'a str> + 'a {
        self.values.iter().filter(|(flag, _)| flags.contains(&flag.as_str())).map(|(_, value)| value.as_str())
    }
}

/// segment の語が `bd` / `bdw` の command なら、6 形の判定が読む分を返す（それ以外は `None`・[`Write`] の唯一の構築点）。
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
    let bare = rest.iter().filter(|word| !word.starts_with('-')).map(|word| (*word).clone()).collect();
    let values = rest
        .iter()
        .enumerate()
        .filter(|(_, word)| word.starts_with('-'))
        .filter_map(|(at, word)| match word.split_once('=') {
            Some((flag, value)) => Some((flag.to_owned(), value.to_owned())),
            None => rest
                .get(at.saturating_add(1))
                .filter(|next| !next.starts_with('-'))
                .map(|next| ((*word).clone(), (*next).clone())),
        })
        .collect();
    Some(Write { client: client.to_owned(), subcommand, flags, words: bare, values })
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

/// segment 1 つを台帳 write の形に掛ける（**6 形の唯一の判定**・pure）。`forms` の順で最初に当たった形を返す。
pub fn judge_write(write: &Write, forms: &[Refusal]) -> Option<Refusal> {
    let has = |flag: &str| write.has(flag);
    let sub = write.subcommand.as_str();
    forms.iter().copied().find(|form| match form {
        Refusal::NotesReplace => has(NOTES),
        Refusal::MemorySubcommand => MEMORY.contains(&sub),
        Refusal::CreateWithoutParent => sub == CREATE && !has(PARENT),
        Refusal::BdOutsideBdw => write.client == RAW_CLIENT && WRITES.contains(&sub),
        Refusal::CreateBypass => BYPASS.contains(&sub) || (sub == TODO && write.second() == ADD),
        Refusal::ParentEdge => parent_edge(write),
        _ => false,
    })
}

/// parent-child の辺を横から張る書きか: `dep add` / `link` の型が parent-child・`dep add` が `--file` を持つ・create の
/// `--deps` の値に `parent-child:` が在る。
fn parent_edge(write: &Write) -> bool {
    let sub = write.subcommand.as_str();
    let dep_add = sub == DEP && write.second() == ADD;
    let typed = write.values_of(&EDGE_TYPE).any(|value| value == PARENT_CHILD);
    let prefix = format!("{PARENT_CHILD}:");
    ((dep_add || sub == LINK) && typed)
        || (dep_add && write.has(EDGE_FILE))
        || (sub == CREATE && write.values_of(&[DEPS]).any(|value| value.contains(&prefix)))
}

/// `NAME=value` の env の前置きか（host-guard も語列の照合の前に読み飛ばす・設計 vessel-hook.md §11 の形 b 4）。
pub(crate) fn is_assignment(word: &str) -> bool {
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
        if flag == STDIN {
            create.stdin = inline.as_deref() != Some("false");
        } else if flag == NO_INHERIT {
            create.no_inherit_labels = inline.as_deref().is_none_or(|value| value == "true");
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
    } else if flag == PARENT {
        create.parent = Some(value);
    } else if EDGE_TYPE.contains(&flag) {
        create.kind = Some(value);
    } else if flag == GRAPH {
        create.graph = Some(value);
    } else if DESCRIPTION.contains(&flag) {
        create.description = Some(value);
    } else if flag == METADATA {
        create.metadata = Some(value);
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

/// 問いの段の deny 文（memo の理由と同じ頭・出所は ledger-form.md §14）。
fn question_line(gap: &Gap) -> String {
    format!("{NAME}: deny bd create は起票の門が止める reason={}（{}・ledger-form.md §14）", gap.as_str(), gap.guidance())
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
        let full = "[\"parent-edge\", \"bd-outside-bdw\", \"create-bypass\", \"notes-replace\", \"memory-subcommand\", \"create-without-parent\"]";
        assert_eq!(forms_of(&manifest(full, true)), Ok(FORMS.to_vec()), "判定の順は FORMS（値の並びに依らない）");
        let one = forms_of(&manifest("[\"notes-replace\", \"unknown\"]", true));
        assert_eq!(one, Ok(vec![Refusal::NotesReplace]), "列に載る形だけ");
        let write = write_of(&segments("bd create x").into_iter().next().unwrap_or_default()).unwrap_or_default();
        assert_eq!(judge_write(&write, &[Refusal::NotesReplace]), None, "列に無い形は断らない");
        assert_eq!(forms_of(&manifest(full, false)), Err(Refusal::NoRow), "不発効");
        let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(forms_of(&empty), Err(Refusal::NoRow), "行が無い");
        let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(forms_of(&embedded), Ok(FORMS.to_vec()), "埋め込みの行は 6 形を全部断る");
    }

    /// 6 形は既存の 4 形の後ろに create-bypass → parent-edge の順で並び、埋め込みの行はその 6 語を全部断る。
    #[test]
    fn hook_ledger_edge_forms_are_six_in_order() {
        let want = [
            Refusal::NotesReplace,
            Refusal::MemorySubcommand,
            Refusal::CreateWithoutParent,
            Refusal::BdOutsideBdw,
            Refusal::CreateBypass,
            Refusal::ParentEdge,
        ];
        assert_eq!(FORMS, want, "判定の順");
        let words: Vec<&str> = FORMS.iter().map(|form| form.as_str()).collect();
        let names = ["notes-replace", "memory-subcommand", "create-without-parent", "bd-outside-bdw", "create-bypass", "parent-edge"];
        assert_eq!(words, names, "記録と rules の語");
        let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("{errors:?}"));
        assert_eq!(forms_of(&embedded), Ok(FORMS.to_vec()), "埋め込みの行は 6 形を全部断る");
        assert_eq!(formed("bdw create x --deps parent-child:y"), Some(Some(Refusal::CreateWithoutParent)), "既存の形が先");
        assert_eq!(formed("bd dep add a b --type parent-child"), Some(Some(Refusal::BdOutsideBdw)), "既存の形が先");
    }

    /// Write は flag でない語（subcommand を含む・出てきた順）と flag の値（`=` の形と離れた形）を運ぶ。
    #[test]
    fn hook_ledger_edge_write_carries_words_and_values() {
        let words = segments("X=1 scripts/bdw dep add a b --type parent-child --json -t=blocks").into_iter().next().unwrap_or_default();
        let Some(write) = write_of(&words) else {
            panic!("bdw を読む");
        };
        assert_eq!(write.client, "bdw");
        assert_eq!(write.subcommand, "dep");
        assert_eq!(write.flags, ["--type", "--json", "-t"]);
        assert_eq!(write.words, ["dep", "add", "a", "b", "parent-child"]);
        let values = vec![("--type".to_owned(), "parent-child".to_owned()), ("-t".to_owned(), "blocks".to_owned())];
        assert_eq!(write.values, values, "値を持たない flag（--json の次は flag）は載らない");
    }

    /// create-bypass: bd と bdw のどちらでも q・create-form・batch と、次の語が add の todo が当たり、todo list と todo done
    /// と todo だけは当たらない。
    #[test]
    fn hook_ledger_edge_create_bypass_hits_the_intake_mouths() {
        for client in ["bd", "bdw", "scripts/bdw"] {
            for rest in ["q x", "q --type task x", "create-form", "batch", "batch --file f", "todo add x", "todo add"] {
                let line = format!("{client} {rest}");
                assert_eq!(formed(&line), Some(Some(Refusal::CreateBypass)), "{line}");
            }
            for rest in ["todo list", "todo done x", "todo", "show q", "list --json"] {
                let line = format!("{client} {rest}");
                assert_eq!(formed(&line), Some(None), "{line}");
            }
        }
    }

    /// parent-edge: dep add と link の --type・-t・--type= の値 parent-child・dep add の --file・create の --deps の値の
    /// parent-child: が当たり、dep add の blocks・link の既定・dep remove は当たらない。
    #[test]
    fn hook_ledger_edge_parent_edge_hits_the_side_edges() {
        for line in [
            "bdw dep add a b --type parent-child",
            "bdw dep add a b -t parent-child",
            "bdw dep add a b --type=parent-child",
            "bdw dep add --type parent-child a b",
            "bdw link a b --type parent-child",
            "bdw link a b -t=parent-child",
            "bd link a b --type=parent-child",
            "bdw dep add --file edges.jsonl",
            "bdw dep add --file=edges.jsonl",
            "bdw create x --parent e --deps parent-child:y",
            "bdw create x --parent=e --deps=blocks:a,parent-child:b",
        ] {
            assert_eq!(formed(line), Some(Some(Refusal::ParentEdge)), "{line}");
        }
        for line in [
            "bdw dep add a b",
            "bdw dep add a b --type blocks",
            "bdw dep add a b -t=related",
            "bdw link a b",
            "bd link a b",
            "bdw link a b --type blocks",
            "bdw dep remove a b",
            "bdw dep remove a b --type parent-child",
            "bdw dep list a --type parent-child",
            "bdw create x --parent e --deps blocks:y",
            "bdw update a --parent b",
            "bdw show a --type parent-child",
        ] {
            assert_eq!(formed(line), Some(None), "{line}");
        }
    }

    /// 断り文は既存の形で、create-bypass は create の --parent を、parent-edge は update の --parent を次の一手に持つ。
    #[test]
    fn hook_ledger_edge_deny_lines_name_the_next_move() {
        let cwd = std::path::Path::new("/nonexistent-ledger-guard-cwd");
        for (line, what, next) in [
            ("bdw q x", "create-bypass", "bdw create <題> --parent <epic>"),
            ("bdw dep add a b --type parent-child", "parent-edge", "bdw update <子> --parent <親>"),
        ] {
            let LedgerDecision::Deny { what: found, line: text } = decide(line, cwd, None) else {
                panic!("{line}: 断る");
            };
            assert_eq!(found, what, "{line}");
            assert_eq!(text.lines().count(), 1, "{line}: 1 行");
            assert!(text.contains(&format!("deny 台帳の write は起票の門が止める reason={what}（")), "{text}");
            let after = text.split_once(" — ").map_or("", |(_, after)| after);
            assert!(after.contains(next), "{line}: 次の一手: {text}");
        }
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

    /// create_of は `-d` と `--description=`（最後の値）・`--metadata`・`--stdin`・`--no-inherit-labels` と `=false` を読む。
    #[test]
    fn hook_question_form_create_reads_body_metadata_and_flags() {
        let create = |line: &str| create_of(&segments(line).into_iter().next().unwrap_or_default()).unwrap_or_default();
        let found = create("bdw create x -d a --description='b c' --metadata '{\"effect\":1}' --stdin --no-inherit-labels -l intake:question");
        assert_eq!(found.description.as_deref(), Some("b c"), "最後の値");
        assert_eq!(found.metadata.as_deref(), Some("{\"effect\":1}"));
        assert!(found.stdin && found.no_inherit_labels && found.has_question_label());
        assert_eq!(found.titles, ["x"], "値を title に数えない");
        let found = create("bd create x --stdin=false --no-inherit-labels=false");
        assert!(!found.stdin && !found.no_inherit_labels, "=false は持たない");
        assert!(create("bd create x --no-inherit-labels=true").no_inherit_labels);
        assert!(!create("bd create x --no-inherit-labels=yes").no_inherit_labels, "true でない値は継ぐ側");
        assert!(!create("bd create x -d y").has_question_label());
    }

    /// 段の順: 併せ持ちが継ぐ指定と 4 行より先・継ぐ指定が本文より先・本文を読めない形（--stdin・-・$・backtick・開けない）。
    #[test]
    fn hook_question_form_stages_run_in_order() {
        let meta = "--metadata '{\"effect\":\"document\",\"asked\":\"seat\"}'";
        let full = "-d '概要 = a\n技術 = b\n理由 = c\n推奨 = d'";
        let what = |line: &str| match decide(line, std::path::Path::new("/nonexistent-question-cwd"), None) {
            LedgerDecision::Deny { what, line } => {
                assert_eq!(line.lines().count(), 1, "1 行: {line}");
                assert!(line.contains("・ledger-form.md §14）"), "{line}");
                what
            }
            LedgerDecision::Allow => String::new(),
        };
        let base = "bdw create x -l intake:question";
        assert_eq!(what(&format!("{base},intake:memo --parent s2-1 -d y")), "question-memo-label", "併せ持ちが先");
        assert_eq!(what(&format!("{base} --parent s2-1 -d y")), "question-inherits-labels", "継ぐ指定が 4 行より先");
        let titled = "bdw create '[memo] x' -l intake:question --parent s2-1 --no-inherit-labels -d y";
        assert_eq!(what(titled), "question-no-summary", "memo の判定を掛けない");
        for body in ["--stdin", "--body-file -", "--body-file", "-d '$X'", "-d '`x`'", "--body-file nope.md"] {
            assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {meta} {body}")), "question-body-unreadable", "{body}");
        }
        assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {full} --metadata @nope.json")), "question-metadata-unreadable");
        assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {full}")), "question-no-effect");
        assert_eq!(what(&format!("{base} --parent s2-1 --no-inherit-labels {full} {meta}")), "");
        assert_eq!(what(&format!("{base} --parent '' {full} {meta}")), "", "空の親は継がない");
    }

    /// cwd で撃ち、断れば 1 行と §15 の出所を確かめて語を返す（通れば空）。
    fn triggered(line: &str, cwd: &std::path::Path) -> String {
        match decide(line, cwd, None) {
            LedgerDecision::Deny { what, line } => {
                assert!(line.lines().count() == 1 && line.ends_with("・ledger-form.md §15）"), "{line}");
                what
            }
            LedgerDecision::Allow => String::new(),
        }
    }

    /// update の本文の出どころ: `--body-file`・`--body-file=`・`-d` を読み、`--stdin`・値の無い `--body-file`・`-`・開けない
    /// file・`$` の `-d` は読めない。本文を書かない update と見出しの無い本文と読める引き金の本文は通る。
    #[test]
    fn hook_memo_trigger_update_reads_each_body_source() {
        let dir = crate::pipe::fixture::scratch("memo-trigger-update");
        for (name, body) in [("bare.md", "### 昇格条件\n- 散文\n"), ("ok.md", "### 昇格条件\n- 引き金: 再発 1\n"), ("plain.md", "本文\n")] {
            std::fs::write(dir.join(name), body).unwrap_or_else(|error| panic!("{name}: {error}"));
        }
        for body in ["--body-file bare.md", "--body-file=bare.md", "-d '### 昇格条件\n引き金: 再発 3（x）'", "-d '### 昇格条件'"] {
            assert_eq!(triggered(&format!("bdw update s2-1 {body}"), &dir), "update-no-trigger", "{body}");
        }
        for body in ["--stdin", "--body-file", "--body-file -", "--body-file gone.md", "-d '$X'", "--body-file ok.md --stdin"] {
            assert_eq!(triggered(&format!("bdw update s2-1 {body}"), &dir), "update-body-unreadable", "{body}");
        }
        for body in ["--body-file ok.md", "--body-file plain.md", "-d 本文", "--status open", "--stdin=false", "--append-notes '### 昇格条件'"] {
            assert_eq!(triggered(&format!("bdw update s2-1 {body}"), &dir), "", "{body}");
        }
        let _ =std::fs::remove_dir_all(&dir);
    }

    /// 接頭辞は cwd から上へ辿った最初の `.beads` の dir を持つ dir の設定から解く（外側の台帳より内側が先）。
    #[test]
    fn hook_memo_trigger_resolves_the_prefix_upward_from_cwd() {
        let dir = crate::pipe::fixture::scratch("memo-trigger-prefix");
        let (outer, inner) = (dir.join("outer"), dir.join("outer").join("repo"));
        for (root, config) in [(&outer, "issue-prefix: far\n"), (&inner, "issue-prefix: \"toy\"\n")] {
            std::fs::create_dir_all(root.join(".beads").join("x")).unwrap_or_else(|error| panic!("{error}"));
            std::fs::write(root.join(".beads").join("config.yaml"), config).unwrap_or_else(|error| panic!("{error}"));
        }
        let deep = inner.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((super::ledger_prefix(&deep), super::ledger_prefix(&outer)), (Some("toy".to_owned()), Some("far".to_owned())));
        assert_eq!(super::ledger_prefix(&dir.join("gone")), None, ".beads の無い木");
        let body = "### 出所\n### 観測\n### 候補\n### 昇格条件\n";
        for (dep, want) in [("toy-1.2", ""), ("far-1", "no-trigger")] {
            std::fs::write(deep.join("m.md"), format!("{body}- 引き金: 依存 {dep}\n")).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(triggered("bdw create '[memo] x' --parent s2-1 --body-file m.md", &deep), want, "{dep}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 40 桁の 16 進。
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    /// git を 1 回撃つ（toy repo の組み立て用）。
    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = std::process::Command::new("git").arg("-C").arg(dir).args(args).status();
        assert!(status.is_ok_and(|found| found.success()), "git {args:?}");
    }

    /// toy repo（`declaration` を HEAD に commit・`beads` が真なら `.beads/config.yaml`＝接頭辞 toy を置く）。宣言 `None` は無い。
    fn toy(name: &str, declaration: Option<&str>, beads: bool) -> std::path::PathBuf {
        let dir = crate::pipe::fixture::scratch(name);
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.name", "t"]);
        git(&dir, &["config", "user.email", "t@example.invalid"]);
        std::fs::write(dir.join("seed"), "seed\n").unwrap_or_else(|error| panic!("{error}"));
        if let Some(extra) = declaration {
            let text = format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n{extra}");
            std::fs::write(dir.join(crate::pipe::declaration::DECL_FILE), text).unwrap_or_else(|error| panic!("{error}"));
        }
        if beads {
            std::fs::create_dir_all(dir.join(".beads")).unwrap_or_else(|error| panic!("{error}"));
            std::fs::write(dir.join(".beads").join("config.yaml"), "issue-prefix: \"toy\"\n").unwrap_or_else(|error| panic!("{error}"));
        }
        git(&dir, &["add", "-A", "."]);
        git(&dir, &["commit", "-q", "-m", "seed"]);
        dir
    }

    /// close の段で撃ち、断れば 1 行と §16 の出所と `deny bd close` の頭を確かめて語を返す（通れば空）。
    fn closed(line: &str, cwd: &std::path::Path) -> (String, String) {
        match decide(line, cwd, None) {
            LedgerDecision::Deny { what, line: text } => {
                assert!(text.lines().count() == 1 && text.ends_with("・ledger-form.md §16）"), "{line}: {text}");
                assert!(text.contains(&format!("deny bd close は起票の門が止める reason={what}（")), "{line}: {text}");
                (what, text)
            }
            LedgerDecision::Allow => (String::new(), String::new()),
        }
    }

    /// 理由の集め方: `-r`・`-r=`・`--reason=`・`--reason`・`--reason-file`（前後の空白を除く）・`done`・`gate resolve` の理由が読まれ、
    /// 値の無い `--reason`・空・`-r<字>` は理由なし、2 つの理由は出てきた順に最初の外れで断る。
    #[test]
    fn hook_close_reason_collects_every_reason_flag_and_reads_in_order() {
        let dir = toy("close-reason-collect", Some("close-check = true\n"), true);
        std::fs::write(dir.join("ok.txt"), "  完了\n").unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(dir.join("bad.txt"), "done it\n").unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(dir.join("landed.txt"), format!("landed {SHA} ci=success\n")).unwrap_or_else(|error| panic!("{error}"));
        let word = |line: &str| closed(line, &dir).0;
        for line in [
            "bdw close toy-1 -r 完了",
            "bdw close toy-1 -r=完了",
            "bdw close toy-1 --reason=完了",
            "bdw close toy-1 --reason 完了",
            "bdw done toy-1 --reason '取り下げ 要らない'",
            "bdw gate resolve toy-9 -r 完了",
            "scripts/bdw close toy-1 --reason '重複 toy-2'",
            "bdw close toy-1 --reason-file ok.txt",
            "bdw close toy-1 toy-2 --reason 完了 --reason '後継 toy-3'",
            "bdw update toy-1 --status open",
            "bdw show toy-1",
        ] {
            assert_eq!(word(line), "", "{line}");
        }
        for line in ["bdw close", "bdw close toy-1", "bdw close toy-1 --reason", "bdw close toy-1 --reason ''", "bdw close toy-1 -r=", "bdw close toy-1 --reason=", "bdw done toy-1", "bdw gate resolve toy-9", "bdw close toy-1 -r完了"] {
            assert_eq!(word(line), "close-no-reason", "{line}");
        }
        for line in ["bdw close toy-1 --reason 'done it'", "bdw done toy-1 -r 'done it'", "bdw gate resolve toy-9 -r 'done it'", "bdw close toy-1 --reason-file bad.txt", "bdw close toy-1 toy-2 --reason 完了 --reason 'x y'", "bdw close toy-1 --reason nope --reason 'landed x'", "bdw close toy-1 --reason '重複 other-1'", "bdw close toy-1 --reason '昇格済み toy-1,toy-2'", "bdw close toy-1 --reason 'Landed x'"] {
            assert_eq!(word(line), "close-outside-forms", "{line}");
        }
        for line in [format!("bdw close toy-1 --reason 'landed {SHA} ci=success'"), "bdw close toy-1 --reason 'landed x'".to_owned(), "bdw close toy-1 --reason-file landed.txt".to_owned(), "bdw close toy-1 --reason 完了 --reason 'landed'".to_owned()] {
            assert_eq!(word(&line), "close-landed", "{line}");
        }
        for line in ["bdw close toy-1 --reason-file -", "bdw close toy-1 --reason-file", "bdw close toy-1 --reason-file=", "bdw close toy-1 --reason-file gone.txt", "bdw close toy-1 --reason '$X'", "bdw close toy-1 --reason '`x`'", "bdw close toy-1 -r \"$(cat r.txt)\"", "bdw close toy-1 --reason=完了$"] {
            assert_eq!(word(line), "close-reason-unreadable", "{line}");
        }
        let (_, text) = closed("bdw close toy-1 --reason 'landed x'", &dir);
        for named in ["pipe land --run <run> --terminal-only", "pipe retire --run <run>", "settle"] {
            assert!(text.contains(named), "{named}: {text}");
        }
        let (_, text) = closed("bdw close toy-1 --reason 'done it'", &dir);
        assert!(text.contains("「done it」") && text.contains("重複 <bead id>") && text.contains("完了") && !text.contains("landed <"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 3 値ごとの掛け方（pure の判定）: 加わる周は断り、読めない周は同じ判定で当たった語を名指す close-declaration-unreadable、
    /// 加わらない周は撃たない。形に合う close はどの値でも通る。
    #[test]
    fn hook_close_reason_stage_applies_by_the_three_values() {
        use crate::pipe::declaration::CloseCheck;
        let writes = |line: &str| -> Vec<super::Write> { segments(line).iter().filter_map(|words| write_of(words)).collect() };
        let stage = |check: CloseCheck, line: &str| super::close_stage(check, &writes(line), Some("toy"), &|_: &str| None::<String>);
        let what = |found: Option<LedgerDecision>| match found {
            Some(LedgerDecision::Deny { what, line }) => (what, line),
            _ => (String::new(), String::new()),
        };
        assert_eq!(what(stage(CloseCheck::Joins, "bdw close toy-1")).0, "close-no-reason");
        let (word, line) = what(stage(CloseCheck::Unreadable, "bdw close toy-1"));
        assert_eq!(word, "close-declaration-unreadable");
        assert!(line.contains("close-no-reason") && line.contains(".vessel.toml") && line.contains("true か false"), "{line}");
        let (_, line) = what(stage(CloseCheck::Unreadable, "bdw close toy-1 --reason 'landed x'"));
        assert!(line.contains("close-landed"), "{line}");
        assert_eq!(stage(CloseCheck::Exempt, "bdw close toy-1"), None, "加わらない周は撃たない");
        for check in [CloseCheck::Joins, CloseCheck::Unreadable] {
            assert_eq!(stage(check, "bdw close toy-1 --reason '取り下げ x'"), None, "形に合う close は通る");
            assert_eq!(stage(check, "bdw show toy-1"), None, "close の segment が無い");
        }
        assert_eq!(what(stage(CloseCheck::Joins, "bdw show x && bdw done toy-2")).0, "close-no-reason", "後ろの segment も読む");
    }

    /// 宣言の読みは HEAD だけ・close の segment が在る周だけ: false・key 無し・宣言 file 無し・`.beads` 無し・作業ツリーだけの true は
    /// 通し、文字列 yes の宣言は close-declaration-unreadable、加わる repo は語で断る。close の無い command は読めない repo でも通る。
    #[test]
    fn hook_close_reason_reads_the_head_declaration_once_per_close_and_exempts_the_rest() {
        let bare = "bdw close toy-1";
        let joined = toy("close-reason-joins", Some("close-check = true\n"), true);
        assert_eq!(closed(bare, &joined).0, "close-no-reason");
        assert_eq!(closed(bare, &joined.join(".beads")).0, "close-no-reason", "根は cwd から上へ辿る");
        for (name, declaration, beads) in [
            ("close-reason-false", Some("close-check = false\n"), true),
            ("close-reason-absent", Some(""), true),
            ("close-reason-nodecl", None, true),
            ("close-reason-nobeads", Some("close-check = true\n"), false),
        ] {
            let dir = toy(name, declaration, beads);
            assert_eq!(closed(bare, &dir).0, "", "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        let worktree = toy("close-reason-worktree", Some(""), true);
        std::fs::write(worktree.join(crate::pipe::declaration::DECL_FILE), "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\nclose-check = true\n").unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(closed(bare, &worktree).0, "", "作業ツリーの宣言は読まない");
        let broken = toy("close-reason-unreadable", Some("close-check = \"yes\"\n"), true);
        let (word, text) = closed(bare, &broken);
        assert_eq!((word.as_str(), text.contains("close-no-reason")), ("close-declaration-unreadable", true), "{text}");
        assert_eq!(closed("bdw close toy-1 --reason '取り下げ x'", &broken).0, "", "形に合う close は読めない repo でも通る");
        assert_eq!(closed("bdw show toy-1 && sh close.sh", &broken).0, "", "close の segment が無い周は読まない");
        for dir in [&joined, &worktree, &broken] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
