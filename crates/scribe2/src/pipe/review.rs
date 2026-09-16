//! 契約の審査の段（`Stage::Reviewed`・設計 docs/design/contract-source.md §4・SRS FR49 / FR9 / AC22）。
//!
//! `pipe intake` の直後に器が lens を **1 回**撃ち、契約とそれが実装する設計の節と要件を読ませて verdict を
//! 採る。verdict は run dir の [`REVIEW_FILE`]（gate の `verdict.json` と同型・tmp → rename の atomic 書き）と
//! `RunStage stage=Reviewed detail=verdict:<PASS|FAIL|INCONCLUSIVE>` に残る。**PASS だけが spawn へ進む**
//! （FAIL / INCONCLUSIVE は終端・`live` は偽）。審査を飛ばす口は無い（`--no-review` を作らない・C16）。
//!
//! lens の口は gate と同じ `--lens <cmd>`（穴は `{contract}` / `{worktree}` の 2 つ・語彙は共有し関数は共有しない）。
//! 審査の材料は run dir の [`REVIEW_DIR`] に**契約の写し**と `{design}` / `{requirements}` の本文（[`DESIGN_FILE`] /
//! [`REQUIREMENTS_FILE`]）として置き、lens には**その写しの path**を `{contract}` で渡す。lens は契約の隣に材料の
//! 2 file が在る周だけ契約審査の雛形（`headless/lens-contract.txt`）を使う（裁定の写し `rulings.txt` と同じ「隣の
//! file」の形・`lens.cmd` の穴は不変・`s2-07l.309` と同型）。gate の周は `<run_dir>/contract.toml` を渡すので
//! 材料は隣に無く、diff の雛形のまま＝1 つの `--lens` が 2 つの段に効く。
//!
//! **偽の PASS を作らない**（AC3 / FR9）。lens が無い・起動できない・出力を読めない・3 値の外はすべて
//! INCONCLUSIVE（終端）。材料の欠け（設計 pointer でない `design`・読めない要件面・要件面に無い id）は
//! 材料の本文に**明示の 1 行**として載せ（C10・空を黙らせない）、判定は lens が持つ。

use super::contract::Contract;
use super::gate::{last_json_object, Verdict};
use super::lens_record::LensSource;
use super::{confine, contract_path, emit, run_dir, table, Emit};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::LockPolicy;
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// 審査の判定を書く file（gate の `verdict.json` と同型）。
pub const REVIEW_FILE: &str = "review.json";

/// 審査の材料を置く run dir 配下の dir 名（契約の写し + 設計の節 + 要件本文）。
pub const REVIEW_DIR: &str = "review";

/// `{design}` の穴の本文（契約の写しの隣・lens が読む）。
pub const DESIGN_FILE: &str = "design.txt";

/// `{requirements}` の穴の本文（契約の写しの隣・lens が読む）。
pub const REQUIREMENTS_FILE: &str = "requirements.txt";

/// lens の scope の unit 名に載せる段の名。
const REVIEW_STAGE: &str = "review";

/// 書きかけの `review.json` の拡張子（同じ dir に置いて rename する）。
const PARTIAL_EXT: &str = "json.partial";

/// 審査の段の極性（[`ReviewCheck`]・設計 contract-source.md §8）: spawn の**前**に測り、verdict が PASS でない
/// 便と判定を読めない便はどちらも起こさない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// spawn の入口が読む審査の判定。**bool で持たない**（「通らない」に「PASS でない」と「読めない」の 2 つの意味が
/// あるため・C11.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCheck {
    /// verdict が PASS＝起こしてよい。
    Passed,
    /// verdict が FAIL / INCONCLUSIVE＝終端（起こさない）。
    Stopped(Verdict),
    /// `review.json` が無い・壊れている・3 値の外＝測れなかった（起こさない）。
    Unreadable,
}

impl ReviewCheck {
    /// run dir の [`REVIEW_FILE`] から判定する（**判定の読み手はこの 1 本**）。
    pub fn judge(state_dir: &Path, id: &str) -> Self {
        match verdict_of(state_dir, id) {
            Some(Verdict::Pass) => Self::Passed,
            Some(found) => Self::Stopped(found),
            None => Self::Unreadable,
        }
    }

    /// 起こしてよいか。**bool はここ 1 本で enum から導く**（読めない周は偽）。
    pub fn is_clear(self) -> bool {
        matches!(self, Self::Passed)
    }

    /// 便が live（終端でない）か。PASS は spawn へ進める側・FAIL / INCONCLUSIVE は終端・読めない周は `None`
    /// （呼び手が断る側へ倒す・`pipe::cli::live` と同じ極性）。
    pub fn live(self) -> Option<bool> {
        match self {
            Self::Passed => Some(true),
            Self::Stopped(_) => Some(false),
            Self::Unreadable => None,
        }
    }

    /// 断りの理由に載せる字面（verdict の 3 値か「読めない」）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => Verdict::Pass.as_str(),
            Self::Stopped(found) => found.as_str(),
            Self::Unreadable => "読めない",
        }
    }
}

/// 便の `review.json`。
pub fn review_path(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(REVIEW_FILE)
}

/// 審査の材料の dir（`<run_dir>/review/`）。
pub fn review_dir(state_dir: &Path, id: &str) -> PathBuf {
    run_dir(state_dir, id).join(REVIEW_DIR)
}

/// `review.json` から 3 値を読む。読めない周は `None`（＝PASS ではない）。
pub fn verdict_of(state_dir: &Path, id: &str) -> Option<Verdict> {
    let text = std::fs::read_to_string(review_path(state_dir, id)).ok()?;
    let pairs = json_lite::parse_object(text.trim()).ok()?;
    pairs
        .iter()
        .find(|(found, _)| found == "verdict")
        .and_then(|(_, value)| value.as_str())
        .and_then(Verdict::parse)
}

/// 審査 1 回の材料。
pub struct Review<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo（設計 doc と要件面を読む base・lens の cwd）。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// 要件面の repo 相対 path（宣言 `requirements`・無ければ既定）。
    pub requirements: &'a str,
    /// lens のコマンドの出所（`--lens`・無い / 読めないは別の値＝どちらも INCONCLUSIVE・[`super::lens_record`]）。
    pub lens: &'a LensSource,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// `{design}` / `{requirements}` の穴の本文。
struct Material {
    /// 契約が実装する設計の節の本文（pointer でない周は明示の 1 行）。
    design: String,
    /// `req` の各 id の要件本文（読めない周は明示の 1 行）。
    requirements: String,
}

/// 審査を 1 回通す。
pub fn review(entry: &Review<'_>) -> Outcome {
    let material = materials(entry);
    let contract = match keep(entry, &material) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let (verdict, evidence, scope) = decide(entry, &contract);
    match settle(entry, verdict, &evidence, scope) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome {
            out: vec![format!("run={} stage={} verdict={}", entry.run, Stage::Reviewed.as_str(), verdict.as_str())],
            err: Vec::new(),
            rc: verdict.rc(),
        },
    }
}

/// 材料を base から読む（読めなさは本文の明示の 1 行にする・C10）。
fn materials(entry: &Review<'_>) -> Material {
    Material {
        design: design_text(entry.repo, &entry.contract.design),
        requirements: requirements_text(entry.repo, entry.requirements, &entry.contract.req),
    }
}

/// 契約の `design` が設計 pointer（`<doc>#<id>`）なら、base の設計 doc からその行の `section` の節の本文を読む
/// （§4「順序」: 生成 (b) の前後で穴の出所は変わらない）。pointer でない周・解けない周は理由の 1 行。
fn design_text(repo: &Path, design: &str) -> String {
    let pointer = match table::parse_pointer(design) {
        Ok(found) => found,
        Err(error) => return format!("（設計の節なし: design={design} は設計 pointer でない・{}）", error.reason()),
    };
    let text = match table::read(repo, &pointer.path) {
        Ok(found) => found,
        Err(reason) => return format!("（設計の節を読めない: {reason}）"),
    };
    let row = match table::find_row(&pointer.path, &text, &pointer.id) {
        Ok(found) => found,
        Err(errors) => {
            let reasons: Vec<String> = errors.iter().map(|error| error.reason()).collect();
            return format!("（契約表の行 {} を読めない: {}）", pointer.id, reasons.join(" / "));
        }
    };
    let body = section_text(&text, &row.section);
    if body.trim().is_empty() {
        return format!("（設計 doc {} の節 {} が無いか空）", pointer.path, row.section);
    }
    format!("{}#{} §{}\n{body}", pointer.path, pointer.id, row.section)
}

/// 節 `number` の本文（`## N.` の見出しの次の行から次の `## ` 見出しの前まで・契約表の区間と fence の中の
/// `## ` は見出しに数えない・`pipe::table` の節の読みと同じ形）。無ければ空。
fn section_text(doc: &str, number: &str) -> String {
    let mut found: Vec<&str> = Vec::new();
    let (mut fenced, mut inside, mut open) = (false, false, false);
    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed == table::BEGIN {
            inside = true;
            continue;
        }
        if trimmed == table::END {
            inside = false;
            continue;
        }
        if inside {
            continue;
        }
        if trimmed.starts_with("```") {
            fenced = !fenced;
        }
        match line.strip_prefix("## ").filter(|_| !fenced) {
            Some(title) => open = section_number(title).as_deref() == Some(number),
            None if open => found.push(line),
            None => {}
        }
    }
    found.join("\n")
}

/// `## N. …` の N（数字の列だけ・それ以外は `None`）。
fn section_number(title: &str) -> Option<String> {
    let (head, _) = title.split_once('.')?;
    (!head.is_empty() && head.chars().all(|found| found.is_ascii_digit())).then(|| head.to_owned())
}

/// `req` の各 id の要件本文を要件面（`.html` の `id="<id>"` を持つ行・tag を剥がした字面）から抜く。
/// 要件面を読めない周は理由の 1 行・id が無い周はその id の行に明示する（黙って落とさない）。
fn requirements_text(repo: &Path, path: &str, req: &[String]) -> String {
    let text = match table::read(repo, path) {
        Ok(found) => found,
        Err(reason) => return format!("（要件面を読めない: {reason}）"),
    };
    req.iter()
        .map(|id| match requirement_row(&text, id) {
            Some(body) => format!("{id}: {body}"),
            None => format!("{id}: （要件面 {path} に無い）"),
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// 要件面の中で `id="<id>"` / `id='<id>'` を持つ最初の行の本文（tag を剥がし空白を畳む）。無ければ `None`。
fn requirement_row(text: &str, id: &str) -> Option<String> {
    let marks = [format!("id=\"{id}\""), format!("id='{id}'")];
    text.lines()
        .find(|line| marks.iter().any(|mark| line.contains(mark.as_str())))
        .map(strip_tags)
}

/// HTML の tag（`<…>`）を剥がし、連続する空白を 1 つに畳む。
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut inside = false;
    for found in line.chars() {
        match found {
            '<' => inside = true,
            '>' if inside => {
                inside = false;
                out.push(' ');
            }
            _ if inside => {}
            _ => out.push(found),
        }
    }
    out.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// 材料を run dir の [`REVIEW_DIR`] へ置き、lens に渡す契約の写しの path を返す。書けない周は `Err`（判定に届かない）。
fn keep(entry: &Review<'_>, material: &Material) -> Result<PathBuf, String> {
    let dir = review_dir(entry.state_dir, entry.run);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let contract = dir.join(super::CONTRACT_FILE);
    std::fs::copy(contract_path(entry.state_dir, entry.run), &contract)
        .map_err(|err| format!("{} を写せない: {err}", contract.display()))?;
    for (name, body) in [(DESIGN_FILE, &material.design), (REQUIREMENTS_FILE, &material.requirements)] {
        let path = dir.join(name);
        std::fs::write(&path, format!("{body}\n")).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    }
    Ok(contract)
}

/// lens を 1 回撃って判定を得る（**wildcard 無し・判定に届かない周は INCONCLUSIVE**）。
///
/// 3 つ目は lens の scope を片付けた結果（record に書く周だけ `Some`）。
fn decide(entry: &Review<'_>, contract: &Path) -> (Verdict, String, Option<confine::Released>) {
    // **無いと読めないは別の理由**（設計 pipeline.md §26・C10・gate の判定順と同じ 3 値の match）。
    let cmd = match entry.lens {
        LensSource::Cmd(cmd) => cmd.as_str(),
        LensSource::Absent => return (Verdict::Inconclusive, "lens が要るのに --lens が無い".to_owned(), None),
        LensSource::Unreadable { path, reason } => {
            return (Verdict::Inconclusive, format!("lens の写し {} を読めない（{reason}）", path.display()), None);
        }
    };
    // **渡すのは path であって本文ではない**（cmd は `sh -c` の 1 行）。穴は gate と同じ 2 つで、`{worktree}` は
    // 便の worktree がまだ無いので base の repo（lens が憲法を読む cwd）を置く。**1 走査で埋める**。
    let line = crate::headless::fill(
        cmd,
        &[("{contract}", &contract.display().to_string()), ("{worktree}", &entry.repo.display().to_string())],
    );
    let unit = confine::unit_name(entry.run, REVIEW_STAGE, 1);
    let wrap = confine::Wrap { unit: &unit, limit: confine::Limit::HostReserve, caps: confine::Caps::embedded() };
    let (mut command, confinement) = confine::wrap_line(&line, &wrap);
    // diff は無い（stdin は piped のまま閉じる＝lens は EOF を見る）。stderr は捨てる（gate と同じ）。
    let spawned = command
        .current_dir(entry.repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return (Verdict::Inconclusive, format!("lens を起動できない: {err}"), None),
    };
    drop(child.stdin.take());
    let waited = child.wait_with_output();
    let scope = confine::release_scope(&confinement);
    let (verdict, evidence) = lens_outcome(waited, &confinement);
    (verdict, evidence, scope)
}

/// 終わった lens の出力から判定を読む（箱の中の死 → rc → 最後の JSON 行の順・gate の lens と同じ極性）。
fn lens_outcome(waited: std::io::Result<std::process::Output>, confinement: &confine::Confinement) -> (Verdict, String) {
    let out = match waited {
        Ok(found) => found,
        Err(err) => return (Verdict::Inconclusive, format!("lens の出力を読めない: {err}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    if confinement.confined() {
        let killed = (confine::read_usage(&text).oom_kill >= 1).then_some(confine::Reason::OomKill);
        let killed = killed.or_else(|| out.status.code().is_none().then_some(confine::Reason::Signal));
        if let Some(reason) = killed {
            return (Verdict::Inconclusive, format!("lens が scope の中で死んだ（reason={}）", reason.as_str()));
        }
    }
    if !out.status.success() {
        let rc = out.status.code().unwrap_or(-1);
        return (Verdict::Inconclusive, format!("lens が rc {rc} で終わった"));
    }
    parse_lens(&text)
}

/// lens の stdout の最後の JSON 行から 3 値を読む。読めない周・3 値の外は INCONCLUSIVE（FR9）。
fn parse_lens(text: &str) -> (Verdict, String) {
    let pairs = match last_json_object(text) {
        Ok(parsed) => parsed,
        Err(reason) => return (Verdict::Inconclusive, format!("lens の{reason}")),
    };
    let get = |key: &str| pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str());
    let evidence = get("evidence").unwrap_or_default().to_owned();
    match get("verdict").and_then(Verdict::parse) {
        Some(verdict) => (verdict, evidence),
        None => (Verdict::Inconclusive, "lens の verdict が 3 値でない".to_owned()),
    }
}

/// 判定を `review.json` へ atomic に書き、`Reviewed` を 1 件追記する。
fn settle(entry: &Review<'_>, verdict: Verdict, evidence: &str, scope: Option<confine::Released>) -> Result<(), String> {
    let mut fields = vec![
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("verdict", Value::Str(verdict.as_str().to_owned())),
        ("evidence", Value::Str(evidence.to_owned())),
    ];
    if let Some(released) = scope {
        fields.push(("scope", Value::Str(released.as_str().to_owned())));
    }
    fields.push(("ts", Value::Str(now_utc())));
    let body = json_lite::write_object(&fields);
    write_review(&review_path(entry.state_dir, entry.run), &format!("{body}\n"))?;
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Reviewed),
            seat: None,
            pid: None,
            detail: Some(format!("verdict:{}", verdict.as_str())),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// `review.json` を **atomic に**書く（同じ dir の書きかけへ書いて rename・gate の `verdict.json` と同じ形）。
/// 書けなかった周は書きかけを残さない。
fn write_review(path: &Path, text: &str) -> Result<(), String> {
    let partial = path.with_extension(PARTIAL_EXT);
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|err| {
            let _ = std::fs::remove_file(&partial);
            format!("{} を書けない: {err}", path.display())
        })
}

/// 対象そのものが壊れている（rc 2・判定を書かない）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{requirement_row, requirements_text, section_text, strip_tags, verdict_of, write_review, ReviewCheck};
    use crate::pipe::gate::Verdict;
    use crate::pipe::run_dir;
    use std::path::{Path, PathBuf};

    /// 歯ごとの空の tmp dir。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pipe-review-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 節の本文は `## N.` の見出しの次の行から次の `## ` の前まで。fence の中の `## ` と契約表の区間は見出しに
    /// 数えず、無い節は空。
    #[test]
    fn pipe_review_section_text_reads_the_numbered_section_only() {
        let doc = "# t\n\n## 1. one\na\n```\n## 9. fenced\n```\nb\n\n## 2. two\nc\n<!-- contracts:begin -->\n## 3. region\n<!-- contracts:end -->\n";
        assert_eq!(section_text(doc, "1"), "a\n```\n## 9. fenced\n```\nb\n", "節 1 は fence 込みで次の見出しの前まで");
        assert_eq!(section_text(doc, "2"), "c", "節 2 は区間の前まで");
        assert_eq!(section_text(doc, "3"), "", "区間の中の見出しは節ではない");
        assert_eq!(section_text(doc, "9"), "", "fence の中の見出しは節ではない");
    }

    /// 要件本文は `id="<id>"` の行の tag を剥がした字面で、無い id はその旨を行に明示し、読めない要件面は
    /// 理由の 1 行になる。
    #[test]
    fn pipe_review_requirements_text_names_missing_ids_and_unreadable_faces() {
        let html = "<tr id=\"FR1\"><td>one</td><td class=\"x\">two  words</td></tr>\n<tr id='FR2'><td>deux</td></tr>\n";
        assert_eq!(strip_tags("<a href=\"x\">p</a>q<br/>  r"), "p q r");
        assert_eq!(requirement_row(html, "FR1").as_deref(), Some("one two words"));
        assert_eq!(requirement_row(html, "FR2").as_deref(), Some("deux"), "単引用符の id も読む");
        assert_eq!(requirement_row(html, "FR10"), None, "FR1 の行は FR10 に当たらない");
        let repo = scratch("req");
        let _ = std::fs::write(repo.join("srs.html"), html);
        let listed = requirements_text(&repo, "srs.html", &["FR1".to_owned(), "FR3".to_owned()]);
        assert_eq!(listed, "FR1: one two words\nFR3: （要件面 srs.html に無い）");
        assert!(requirements_text(&repo, "absent.html", &["FR1".to_owned()]).starts_with("（要件面を読めない: "));
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// `review.json` の 3 値は 1 本の読み手で読み、無い / 壊れ / 3 値の外は `Unreadable`（起こさない側）。
    /// `ReviewCheck` の bool は PASS だけが真で、`live` は PASS = 真・他の 2 値 = 偽・読めない = `None`。
    #[test]
    fn pipe_review_check_reads_review_json_fail_closed() {
        let state = scratch("check");
        let dir = run_dir(&state, "r");
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(ReviewCheck::judge(&state, "r"), ReviewCheck::Unreadable, "file が無い");
        for (body, want) in [
            ("{\"verdict\":\"PASS\"}\n", ReviewCheck::Passed),
            ("{\"verdict\":\"FAIL\"}\n", ReviewCheck::Stopped(Verdict::Fail)),
            ("{\"verdict\":\"INCONCLUSIVE\"}\n", ReviewCheck::Stopped(Verdict::Inconclusive)),
            ("{\"verdict\":\"pass\"}\n", ReviewCheck::Unreadable),
            ("not json\n", ReviewCheck::Unreadable),
        ] {
            assert_eq!(write_review(&dir.join("review.json"), body), Ok(()));
            assert_eq!(ReviewCheck::judge(&state, "r"), want, "{body}");
        }
        assert_eq!(verdict_of(&state, "r"), None, "3 値の外は読めない");
        assert!(ReviewCheck::Passed.is_clear() && !ReviewCheck::Stopped(Verdict::Fail).is_clear());
        assert!(!ReviewCheck::Unreadable.is_clear());
        assert_eq!(ReviewCheck::Passed.live(), Some(true));
        assert_eq!(ReviewCheck::Stopped(Verdict::Inconclusive).live(), Some(false));
        assert_eq!(ReviewCheck::Unreadable.live(), None);
        assert_eq!(ReviewCheck::Unreadable.as_str(), "読めない");
        assert_eq!(ReviewCheck::Stopped(Verdict::Fail).as_str(), "FAIL");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 書けない周は本 file が生まれず書きかけも残さない（親 dir が無い）。
    #[test]
    fn pipe_review_write_failure_leaves_no_partial() {
        let dir = scratch("partial");
        let absent = dir.join("absent").join("review.json");
        assert!(write_review(&absent, "{}\n").is_err());
        assert!(!absent.exists() && !Path::new(&absent.with_extension("json.partial")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
