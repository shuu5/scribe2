//! memo の入口（設計 docs/design/ledger-form.md §3 の 8・§6 行 c・FR51 / FR31）。
//!
//! 器の **read-only の口**: 便の終端（`--run`）か user の要望（`--from user`）から memo の plan（bd の create の
//! 引数と 4 節の本文）を組んで標準出力に出すだけで、**台帳は読まず書かない**（起票は席が bdw で撃つ・ADR-0045 §2・
//! C15）。client の binary は 1 度も起こさない。
//!
//! 便の終端の原本は 4 形で閉じる（人が写さない・C10）: `Gated` = run dir の `verdict.json` の evidence と at／
//! `Reviewed` = `review.json` の evidence と at（どちらも verdict は FAIL か INCONCLUSIVE）／`Questioned` = event log の
//! 質問の逐語と about（既存の読み手 [`crate::pipe::question_of_run`]）／`Failed` = event log の `RunStage(Failed)` の
//! detail と ts。どの形にも当たらない終端（file が無い・evidence や detail が空・log を読めない）は写さず、閉じた
//! 理由 1 つで断る（fail-closed）。

use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::json_lite;
use crate::fleet::{replay, store, Event, EventKind, Stage};
use crate::pipe::gate::Verdict;
use crate::pipe::review::review_path;
use crate::pipe::{question_of_run, run_dir, verdict_path};
use std::path::Path;

/// memo を名乗る label（識別は label の 1 つ・ledger-form.md §3 の 1）。
pub const LABEL: &str = "intake:memo";

/// 関連 bead へ張る edge の種類。
pub const RELATES: &str = "relates-to";

/// `--from user` の出所の 1 行（逐語は席が notes へ写す・§3 の 8 (b)）。
pub const USER_SOURCE: &str = "user 逐語は本 bead の notes";

/// 値の無い欄の字面（空に潰さない・C10）。
const ABSENT: &str = "-";

/// `ledger` の使い方（1 枚）。
pub fn usage() -> String {
    "usage: ledger memo <--run ID --state-dir S|--from user> --parent EPIC [--relates ID]...".to_owned()
}

/// plan を出せない閉じた理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// run dir が無い id。
    NoRunDir,
    /// event log を読めない。
    LogUnreadable,
    /// 終端でない（段が終端の 4 形の外・verdict が PASS・log に便が無い〔`None`〕）。
    NotTerminal(Option<Stage>),
    /// 終端だが原本が無い（file が無い・壊れている・evidence / 質問 / detail が空）。
    NoMaterial(Stage),
}

impl Refusal {
    /// 断りの 1 行（stderr）。
    pub fn render(&self, run: &str) -> String {
        let stage = |found: Option<Stage>| found.map_or(ABSENT, Stage::as_str);
        match self {
            Self::NoRunDir => format!("memo: refused reason=no-run-dir run={run}"),
            Self::LogUnreadable => format!("memo: refused reason=log-unreadable run={run}"),
            Self::NotTerminal(found) => format!("memo: refused reason=not-terminal run={run} stage={}", stage(*found)),
            Self::NoMaterial(found) => format!("memo: refused reason=no-material run={run} stage={}", found.as_str()),
        }
    }
}

/// 終端の種類ごとの原本（4 形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Material {
    /// gate（`verdict.json`）か審査（`review.json`）の FAIL・INCONCLUSIVE。
    Judged {
        /// 3 値のうち PASS でない 2 つ。
        verdict: Verdict,
        /// file の `kind`（審査の理由の型・無ければ `None`）。
        finding: Option<String>,
        /// file の `evidence`（空でない）。
        evidence: String,
        /// file の `at`（無ければ `None`）。
        at: Option<String>,
    },
    /// runner の質問（逐語と about）。
    Question {
        /// 質問の逐語（空でない）。
        question: String,
        /// 契約のどの key か（無ければ `None`）。
        about: Option<String>,
    },
    /// `RunStage(Failed)` の detail と ts。
    Failed {
        /// 閉じた理由の字面（空でない）。
        detail: String,
        /// event の ts。
        ts: String,
    },
}

/// 便の終端（段と原本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    /// 便 id。
    pub run: String,
    /// 終端の段。
    pub stage: Stage,
    /// 原本。
    pub material: Material,
}

impl Terminal {
    /// 出所の `kind`（Gated / Reviewed は verdict〔と審査の理由の型〕・Questioned は `question`・Failed は `failed`）。
    pub fn kind(&self) -> String {
        match &self.material {
            Material::Judged { verdict, finding: Some(found), .. } => format!("{} {found}", verdict.as_str()),
            Material::Judged { verdict, finding: None, .. } => verdict.as_str().to_owned(),
            Material::Question { .. } => "question".to_owned(),
            Material::Failed { .. } => "failed".to_owned(),
        }
    }
}

/// 便の終端を run dir と event log から読む（読むだけ・書かない）。
pub fn terminal_of(state_dir: &Path, run: &str) -> Result<Terminal, Refusal> {
    if !run_dir(state_dir, run).is_dir() {
        return Err(Refusal::NoRunDir);
    }
    let events = store::read_all(state_dir).map_err(|_| Refusal::LogUnreadable)?;
    let stage = replay(&events).runs.get(run).map(|found| found.stage).ok_or(Refusal::NotTerminal(None))?;
    let material = match stage {
        Stage::Gated => judged(&verdict_path(state_dir, run), stage)?,
        Stage::Reviewed => judged(&review_path(state_dir, run), stage)?,
        Stage::Questioned => question_of_run(state_dir, run)
            .filter(|found| !found.question.trim().is_empty())
            .map(|found| Material::Question { question: found.question, about: found.about })
            .ok_or(Refusal::NoMaterial(stage))?,
        Stage::Failed => failed(&events, run).ok_or(Refusal::NoMaterial(stage))?,
        Stage::Intake
        | Stage::Blocked
        | Stage::Spawned
        | Stage::RateLimited
        | Stage::Implemented
        | Stage::Landed
        | Stage::Stopped => return Err(Refusal::NotTerminal(Some(stage))),
    };
    Ok(Terminal { run: run.to_owned(), stage, material })
}

/// `verdict.json` / `review.json` から FAIL・INCONCLUSIVE の原本を読む。PASS は終端でない。
fn judged(path: &Path, stage: Stage) -> Result<Material, Refusal> {
    let text = std::fs::read_to_string(path).map_err(|_| Refusal::NoMaterial(stage))?;
    let pairs = json_lite::parse_object(text.trim()).map_err(|_| Refusal::NoMaterial(stage))?;
    let get = |key: &str| {
        pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str()).map(str::to_owned)
    };
    let verdict = get("verdict").as_deref().and_then(Verdict::parse).ok_or(Refusal::NoMaterial(stage))?;
    if verdict == Verdict::Pass {
        return Err(Refusal::NotTerminal(Some(stage)));
    }
    let evidence = get("evidence").filter(|found| !found.trim().is_empty()).ok_or(Refusal::NoMaterial(stage))?;
    Ok(Material::Judged { verdict, finding: get("kind"), evidence, at: get("at") })
}

/// 便の最後の `RunStage(Failed)` の detail と ts（detail が空なら `None`）。
fn failed(events: &[Event], run: &str) -> Option<Material> {
    let found = events
        .iter()
        .rev()
        .find(|event| event.run == run && event.kind == EventKind::RunStage && event.stage == Some(Stage::Failed))?;
    let detail = found.detail.clone().filter(|text| !text.trim().is_empty())?;
    Some(Material::Failed { detail, ts: found.ts.clone() })
}

/// plan の出所（2 形だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// 便の終端。
    Run(Terminal),
    /// user の要望（日付 `YYYY-MM-DD`）。
    User(String),
}

/// memo の plan（bd の create の引数と 4 節の本文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// 出所。
    pub origin: Origin,
    /// parent の epic id（引数）。
    pub parent: String,
    /// `relates-to` を張る関連 bead の id（引数の順）。
    pub relates: Vec<String>,
}

impl Plan {
    /// title（`[memo]` の接頭辞・行 d の門が読む印）。
    fn title(&self) -> String {
        match &self.origin {
            Origin::Run(found) => format!("[memo] {} {} {}", found.run, found.stage.as_str(), found.kind()),
            Origin::User(date) => format!("[memo] user {date}"),
        }
    }

    /// 本文（`## memo` と 4 節・候補と昇格条件は空の見出し＝席が埋める）。
    pub fn body(&self) -> Vec<String> {
        let (source, observed) = match &self.origin {
            Origin::Run(found) => (
                vec![
                    format!("- run: {}", found.run),
                    format!("- stage: {}", found.stage.as_str()),
                    format!("- kind: {}", found.kind()),
                ],
                observation(&found.material),
            ),
            Origin::User(date) => (vec![format!("- {USER_SOURCE}"), format!("- date: {date}")], Vec::new()),
        };
        let mut lines = vec!["## memo".to_owned(), "### 出所".to_owned()];
        lines.extend(source);
        lines.push("### 観測".to_owned());
        lines.extend(observed);
        lines.extend(["### 候補".to_owned(), "### 昇格条件".to_owned()]);
        lines
    }

    /// 標準出力の行: `arg:` の行（bd の create の引数）→ `body:` の 1 行 → 本文（以後は全部本文）。
    pub fn render(&self) -> Vec<String> {
        let mut lines = vec![
            format!("arg: --title={}", self.title()),
            "arg: --type=task".to_owned(),
            format!("arg: --labels={LABEL}"),
            format!("arg: --parent={}", self.parent),
        ];
        lines.extend(self.relates.iter().map(|id| format!("arg: --deps={RELATES}:{id}")));
        lines.push("body:".to_owned());
        lines.extend(self.body());
        lines
    }
}

/// 観測の節（終端の種類ごとの原本の逐語）。
fn observation(material: &Material) -> Vec<String> {
    match material {
        Material::Judged { evidence, at, .. } => {
            vec![format!("- evidence: {evidence}"), format!("- at: {}", at.as_deref().unwrap_or(ABSENT))]
        }
        Material::Question { question, about } => {
            vec![format!("- question: {question}"), format!("- about: {}", about.as_deref().unwrap_or(ABSENT))]
        }
        Material::Failed { detail, ts } => vec![format!("- detail: {detail}"), format!("- ts: {ts}")],
    }
}

/// 引数の読み取り結果。
struct Args<'a> {
    run: Option<&'a str>,
    state_dir: Option<&'a str>,
    user: bool,
    parent: &'a str,
    relates: Vec<&'a str>,
}

/// id の形（空でなく空白を含まず `--` で始まらない＝行の形の出力を割らない）。
fn is_id(value: &str) -> bool {
    !value.is_empty() && !value.starts_with("--") && !value.chars().any(char::is_whitespace)
}

/// `memo` に続く引数を読む。値欠け・重複・未知の flag・形の違う id・2 形の混在は `None`（使い方の誤り）。
fn parse(args: &[String]) -> Option<Args<'_>> {
    let (mut run, mut state_dir, mut user, mut parent, mut relates) = (None, None, false, None, Vec::new());
    let mut iter = args.iter().map(String::as_str);
    while let Some(flag) = iter.next() {
        let value = iter.next().filter(|found| !found.trim().is_empty() && !found.starts_with("--"))?;
        match flag {
            "--run" if run.is_none() && is_id(value) => run = Some(value),
            "--state-dir" if state_dir.is_none() => state_dir = Some(value),
            "--from" if !user && value == "user" => user = true,
            "--parent" if parent.is_none() && is_id(value) => parent = Some(value),
            "--relates" if is_id(value) => relates.push(value),
            _ => return None,
        }
    }
    match (run.is_some(), state_dir.is_some(), user) {
        (true, true, false) | (false, false, true) => Some(Args { run, state_dir, user, parent: parent?, relates }),
        _ => None,
    }
}

/// `ledger memo` の口。plan は stdout・断りは stderr の 1 行（rc 1）。
pub fn dispatch(args: &[String]) -> Outcome {
    let Some(found) = parse(args) else {
        return Outcome::failed(RC_REFUSED, vec![usage()]);
    };
    let origin = match (found.run, found.state_dir, found.user) {
        (Some(run), Some(dir), false) => match terminal_of(Path::new(dir), run) {
            Ok(terminal) => Origin::Run(terminal),
            Err(refusal) => return Outcome::failed_line(RC_REFUSED, refusal.render(run)),
        },
        _ => Origin::User(today()),
    };
    let plan = Plan {
        origin,
        parent: found.parent.to_owned(),
        relates: found.relates.iter().map(|id| (*id).to_owned()).collect(),
    };
    Outcome::ok(plan.render())
}

/// 今日の日付（UTC の `YYYY-MM-DD`・event の ts と同じ時計）。
fn today() -> String {
    let now = crate::fleet::cli::now_utc();
    match now.split_once('T') {
        Some((date, _)) => date.to_owned(),
        None => now,
    }
}
