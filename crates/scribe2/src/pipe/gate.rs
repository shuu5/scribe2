//! gate（設計 docs/design/pipeline.md §5.3・FR8 / FR9 / NFR1）。
//!
//! 契約の `verify` 各行の逐条 rc（機械検証）と lens 1 本の判定を合わせて 3 値を出す。
//! **判定は wildcard 無しの順序で決める**: verify に rc≠0 → FAIL ／ diff byte が cap 超
//! → INCONCLUSIVE（lens を呼ばない）／ lens 側の不備 → INCONCLUSIVE ／ それ以外は
//! lens の verdict。
//!
//! **偽の PASS を作らない**（AC3）。判定に届かなかった周はすべて INCONCLUSIVE へ倒す
//! ——「測れなかった」を「通った」に化けさせないためで、極性は fail-closed（C11.2）。
//!
//! **同じ便を 2 度以上通ることが在る**（INCONCLUSIVE からの測り直し）。`verdict.json` は
//! 最後の判定で上書きし、`RunStage stage=Gated detail=verdict:<V>` は追記する
//! （append-only＝「1 度目は測れなかった」という事実を消さない）。**測り直してよい便か**
//! の判定はここではなく段の入口（[`super::cli`]）が持つ。

use super::contract::Contract;
use super::{
    emit, git_bytes, git_line, verdict_path, verify_log_path, worktree_path, Emit,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{append_line, LockPolicy};
use crate::fleet::{cli::now_utc, EventKind, Stage, SCHEMA};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// 判定できなかったときの rc（設計 §5.3）。
///
/// `cli_outcome` の 0 / 1 / 2 は器の全 subcommand が共有する語彙で、3 を要るのは
/// gate の 3 値判定だけである。共有語彙へ足すと「断り」でも「壊れ」でもない値が
/// 全 subcommand の面に生えるので、**要る側の module に置く**。
pub const RC_INCONCLUSIVE: u8 = 3;

/// lens の出力から拾う JSON 行の始まり。
const JSON_HEAD: char = '{';

/// gate の 3 値。**bool で持たない**（「PASS でない」に 2 つの意味があるため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 通した。
    Pass,
    /// 落ちた。
    Fail,
    /// 判定できなかった。
    Inconclusive,
}

/// [`Verdict`] の全 variant。
pub const VERDICTS: &[Verdict] = &[Verdict::Pass, Verdict::Fail, Verdict::Inconclusive];

impl Verdict {
    /// JSON と stdout に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Inconclusive => "INCONCLUSIVE",
        }
    }

    /// 字面から引く。3 値の外は `None`（＝呼び手が INCONCLUSIVE へ倒す）。
    pub fn parse(text: &str) -> Option<Self> {
        VERDICTS.iter().copied().find(|found| found.as_str() == text)
    }

    /// process の rc。
    pub fn rc(self) -> u8 {
        match self {
            Self::Pass => RC_OK,
            Self::Fail => RC_REFUSED,
            Self::Inconclusive => RC_INCONCLUSIVE,
        }
    }
}

/// 規則から読んだ 2 つの線。**数値をこの file に焼かない**（憲法 C1 / C5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// 要る lens の本数（rules 行 `gate.lens_count`）。
    pub lens_count: u64,
    /// diff の byte 数の上限（rules 行 `gate.token_cap`）。
    pub token_cap: u64,
}

/// gate 1 回の材料。
pub struct Gate<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// lens のコマンド（無ければ `None`）。
    pub lens: Option<&'a str>,
    /// 規則から読んだ線。
    pub limits: Limits,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// 実測した 2 つの量。
struct Measured {
    /// rc≠0 だった verify 行の本数。
    red: u64,
    /// `git diff <base>..HEAD` の生 byte。
    diff: Vec<u8>,
}

/// 書き留める判定 1 件。
struct Decision {
    /// 3 値。
    verdict: Verdict,
    /// 理由（lens の evidence か、lens を呼ばなかった理由）。
    evidence: String,
    /// rc≠0 だった verify 行の本数。
    red: u64,
    /// diff の byte 数。
    diff_bytes: u64,
}

/// gate を 1 回通す。
pub fn gate(entry: &Gate<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    let Some(base) = super::base_of_run(entry.state_dir, entry.run) else {
        return refused(format!("run {} に base が無い（spawn を通っていない）", entry.run));
    };
    if let Some(reason) = precheck(&worktree, &base) {
        return precheck_failed(entry, &reason);
    }
    let measured = match measure(entry, &worktree, &base) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let (verdict, evidence) = decide(entry, &worktree, &measured);
    let decision = Decision {
        verdict,
        evidence,
        red: measured.red,
        diff_bytes: byte_count(&measured.diff),
    };
    match settle(entry, &decision) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome {
            out: vec![format!("run={} verdict={}", entry.run, verdict.as_str())],
            err: Vec::new(),
            rc: verdict.rc(),
        },
    }
}

/// 前提を見る。満たしていれば `None`、違反なら理由 1 行。
///
/// **段（Implemented）の検査はここに置かない**。段違いは他の subcommand と同じく
/// 「何もしない rc 1」で、`Failed` を書いて便を終端させる筋合いが無いためである
/// （早く叩いただけの便が resume 不能になる）。ここが見るのは worktree の事実だけ。
fn precheck(worktree: &Path, base: &str) -> Option<String> {
    if !worktree.exists() {
        return Some(format!("worktree {} が無い", worktree.display()));
    }
    match git_bytes(worktree, &["status", "--porcelain"]) {
        None => return Some(format!("{} の状態を読めない", worktree.display())),
        Some(bytes) if !bytes.is_empty() => return Some("worktree が clean でない".to_owned()),
        Some(_) => {}
    }
    let range = format!("{base}..HEAD");
    let commits: u64 = git_line(worktree, &["rev-list", "--count", &range])
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    (commits < 1).then(|| "commit が 1 本も無い".to_owned())
}

/// verify を逐条で撃ち、diff を測る。
fn measure(entry: &Gate<'_>, worktree: &Path, base: &str) -> Result<Measured, String> {
    let red = record_verify(entry, worktree)?;
    let range = format!("{base}..HEAD");
    let diff = git_bytes(worktree, &["diff", &range])
        .ok_or_else(|| format!("{} の diff を測れない", worktree.display()))?;
    Ok(Measured { red, diff })
}

/// verify 各行を撃ち、行ごとの rc を `verify.jsonl` へ逐条で残す。
fn record_verify(entry: &Gate<'_>, worktree: &Path) -> Result<u64, String> {
    let path = verify_log_path(entry.state_dir, entry.run);
    let mut red = 0;
    for (index, line) in entry.contract.verify.iter().enumerate() {
        let rc = run_line(worktree, line);
        if rc != 0 {
            red += 1;
        }
        let record = json_lite::write_object(&[
            ("schema", Value::Num(SCHEMA)),
            ("n", Value::Num(index as u64 + 1)),
            ("rc", Value::Num(recorded_rc(rc))),
            ("cmd", Value::Str(line.clone())),
        ]);
        append_line(&path, &record, entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(red)
}

/// verify 1 行を与えられた worktree で撃って rc を得る。起動できない周も RED 側へ倒す。
///
/// land の「main 実測」も同じ関数を通す（**verify 行を撃つ実装は器の中で 1 本**）。
/// 2 本になると gate が通した行と main で撃った行の意味が静かにずれる。
pub fn run_line(worktree: &Path, line: &str) -> i32 {
    Command::new("sh")
        .arg("-c")
        .arg(line)
        .current_dir(worktree)
        .output()
        .map_or(-1, |out| out.status.code().unwrap_or(-1))
}

/// rc を JSON の非負整数へ写す。`sh` が signal で落ちた周（負）は 255 に畳む。
fn recorded_rc(rc: i32) -> u64 {
    u64::try_from(rc).unwrap_or(u64::from(u8::MAX))
}

/// byte 数を数える。
fn byte_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// 判定順を 1 か所に閉じる（**wildcard 無し・上から順に効く**）。
fn decide(entry: &Gate<'_>, worktree: &Path, measured: &Measured) -> (Verdict, String) {
    if measured.red > 0 {
        return (
            Verdict::Fail,
            format!("verify の {} 行が rc≠0", measured.red),
        );
    }
    let size = byte_count(&measured.diff);
    if size > entry.limits.token_cap {
        // **換算係数を持たない**（NFR1）。byte ≥ token の保守的な読みで直接比べる。
        return (
            Verdict::Inconclusive,
            format!("diff {size} byte が cap {} を超えた", entry.limits.token_cap),
        );
    }
    // **本数は照合する**。0 本（lens を呼ばずに通す）も 2 本以上（1 本で足りたことに
    // する）も「lens の verdict」を得ていないので、判定順の 4 番目は成立しない。
    // どちらも判定できていない周ゆえ INCONCLUSIVE へ倒す（AC3・C11.2）。
    if entry.limits.lens_count != 1 {
        return (
            Verdict::Inconclusive,
            format!(
                "規則は lens {} 本を定める（通せるのは 1 本だけ）",
                entry.limits.lens_count
            ),
        );
    }
    let Some(cmd) = entry.lens else {
        return (Verdict::Inconclusive, "lens が要るのに --lens が無い".to_owned());
    };
    ask_lens(cmd, worktree, &measured.diff)
}

/// lens へ diff を stdin で渡し、stdout の JSON 1 行を読む。
fn ask_lens(cmd: &str, worktree: &Path, diff: &[u8]) -> (Verdict, String) {
    let spawned = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return (Verdict::Inconclusive, format!("lens を起動できない: {err}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // 読まずに終える lens への write は EPIPE になる。**判定は出力で決める**ので
        // ここの失敗は理由にしない（take で drop され、lens は EOF を見る）。
        let _ = stdin.write_all(diff);
    }
    let out = match child.wait_with_output() {
        Ok(found) => found,
        Err(err) => return (Verdict::Inconclusive, format!("lens の出力を読めない: {err}")),
    };
    if !out.status.success() {
        let rc = out.status.code().unwrap_or(-1);
        return (Verdict::Inconclusive, format!("lens が rc {rc} で終わった"));
    }
    parse_lens(&String::from_utf8_lossy(&out.stdout))
}

/// lens の stdout から最後の JSON 行を読む。読めない周は INCONCLUSIVE。
fn parse_lens(text: &str) -> (Verdict, String) {
    let found = text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with(JSON_HEAD));
    let Some(line) = found else {
        return (Verdict::Inconclusive, "lens の出力に JSON 行が無い".to_owned());
    };
    let pairs = match json_lite::parse_object(line.trim()) {
        Ok(parsed) => parsed,
        Err(reason) => {
            return (
                Verdict::Inconclusive,
                format!("lens の出力を読めない: {reason}"),
            )
        }
    };
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(found_key, _)| found_key == key)
            .and_then(|(_, value)| value.as_str())
    };
    let evidence = get("evidence").unwrap_or_default().to_owned();
    match get("verdict").and_then(Verdict::parse) {
        Some(verdict) => (verdict, evidence),
        None => (
            Verdict::Inconclusive,
            "lens の verdict が 3 値でない".to_owned(),
        ),
    }
}

/// 判定を `verdict.json` へ書き、`Gated` を 1 件追記する。
///
/// **測り直しの周も同じ経路を通る**: file は最後の判定で上書きし、event は追記する。
fn settle(entry: &Gate<'_>, decision: &Decision) -> Result<(), String> {
    let body = json_lite::write_object(&[
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("verdict", Value::Str(decision.verdict.as_str().to_owned())),
        ("evidence", Value::Str(decision.evidence.clone())),
        ("verify_red", Value::Num(decision.red)),
        ("diff_bytes", Value::Num(decision.diff_bytes)),
        ("ts", Value::Str(now_utc())),
    ]);
    let path = verdict_path(entry.state_dir, entry.run);
    std::fs::write(&path, format!("{body}\n"))
        .map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Gated),
            seat: None,
            pid: None,
            detail: Some(format!("verdict:{}", decision.verdict.as_str())),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// 前提違反を `Failed detail=precheck:<理由>` で残して断る（lens は起動しない）。
fn precheck_failed(entry: &Gate<'_>, reason: &str) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some(format!("precheck:{reason}")),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => refused(format!("gate の前提を満たさない（{reason}）")),
    }
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
