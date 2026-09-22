//! `cargo xtask main-provenance [--rev <rev>]` の本体（設計 docs/design/pipeline.md §7 約束 3・`s2-07l.170`）。
//!
//! push(main) の CI が撃つ門で、main の HEAD が**入口を通った commit か**を message の字面で測る。入口は 2 つ:
//! PR の squash（件名の末尾が `(#N)`）と `pipe land` の trailer（本文の 1 行 `run: <run id>`）。どちらも持たない
//! commit は PR の flip-check も便の gate も通っていない＝入口の flip の免除経路の最後の 1 本（直接の push）である。
//!
//! 判定行は stdout へ 1 行: `main-provenance: ok via=pr number=<N> sha=<sha>` / `main-provenance: ok via=land
//! run=<run id> sha=<sha>`（rc 0）・`main-provenance: FAIL reason=no-provenance sha=<sha>`（rc 1）。git を撃てない・
//! rev が解けない周は判定行を出さず stderr へ理由を出して rc 2（測れなかったを通過にも赤にも化けさせない）。

use crate::flipcheck::is_bead_id;
use crate::{emit, emit_err};
use std::path::Path;
use std::process::{Command, ExitCode};

/// `--rev` の既定（CI は checkout した HEAD を測る）。
const DEFAULT_REV: &str = "HEAD";

/// land の trailer の頭（`pipe land` が squash の本文へ書く 1 行 `run: <run id>`）。
const RUN_TRAILER: &str = "run: ";

/// commit の出所（入口の 2 形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Provenance {
    /// PR の squash（件名の末尾 `(#N)`）。
    Pr(u64),
    /// `pipe land` の trailer（`run: <run id>`）。
    Land(String),
}

/// commit message から出所を読む。件名（1 行目）の `(#N)` を先に見て、無ければ本文（2 行目以降）の trailer を見る。
pub(crate) fn provenance_of(message: &str) -> Option<Provenance> {
    let mut lines = message.lines();
    let subject = lines.next().unwrap_or_default();
    if let Some(number) = pr_number(subject) {
        return Some(Provenance::Pr(number));
    }
    lines
        .filter_map(|line| line.trim_end().strip_prefix(RUN_TRAILER))
        .find(|run| is_run_id(run))
        .map(|run| Provenance::Land(run.to_owned()))
}

/// 件名の末尾 ` (#N)` の N（N は 1 桁以上の数字だけ・前に空白が要る）。
fn pr_number(subject: &str) -> Option<u64> {
    let inner = subject.trim_end().strip_suffix(')')?;
    let (head, digits) = inner.rsplit_once("(#")?;
    if !head.ends_with(' ') || digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// run id の閉じた形 `<bead id>-<YYYYMMDD>T<HHMMSS>Z`（`s2-07l.351-20260922T061245Z`）。
fn is_run_id(run: &str) -> bool {
    let Some((bead, stamp)) = run.rsplit_once('-') else {
        return false;
    };
    let digits = |text: &str, len: usize| text.len() == len && text.chars().all(|ch| ch.is_ascii_digit());
    let shaped = stamp
        .strip_suffix('Z')
        .and_then(|rest| rest.split_once('T'))
        .is_some_and(|(date, time)| digits(date, 8) && digits(time, 6));
    shaped && is_bead_id(bead)
}

/// 判定 1 行と rc。
pub(crate) fn verdict(sha: &str, message: &str) -> (String, ExitCode) {
    match provenance_of(message) {
        Some(Provenance::Pr(number)) => (format!("main-provenance: ok via=pr number={number} sha={sha}"), ExitCode::SUCCESS),
        Some(Provenance::Land(run)) => (format!("main-provenance: ok via=land run={run} sha={sha}"), ExitCode::SUCCESS),
        None => (format!("main-provenance: FAIL reason=no-provenance sha={sha}"), ExitCode::FAILURE),
    }
}

/// `--rev <rev>` を読む（無ければ [`DEFAULT_REV`]・値の無い `--rev` と空文字は Err）。
fn parse_rev(args: &[String]) -> Result<String, String> {
    let Some(at) = args.iter().position(|arg| arg == "--rev") else {
        return Ok(DEFAULT_REV.to_owned());
    };
    match args.get(at.saturating_add(1)) {
        Some(value) if !value.is_empty() => Ok(value.clone()),
        _ => Err("main-provenance: --rev の直後に値が無い".to_owned()),
    }
}

/// `git log -1 --format=%H%n%B <rev>` の sha と message（spawn 失敗・rc≠0 は Err）。
fn commit_of(workdir: &Path, rev: &str) -> Result<(String, String), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["log", "-1", "--format=%H%n%B", rev, "--"])
        .output()
        .map_err(|err| format!("main-provenance: git log を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "main-provenance: git log {rev} が rc≠0: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let (sha, message) = text.split_once('\n').unwrap_or((text.as_str(), ""));
    Ok((sha.trim().to_owned(), message.to_owned()))
}

/// CLI 面。判定行を stdout へ 1 行だけ出し rc を返す（測れなかった周だけが rc 2）。
pub fn run(args: &[String]) -> ExitCode {
    let measured = parse_rev(args).and_then(|rev| {
        let workdir = std::env::current_dir().map_err(|err| format!("main-provenance: cwd を解決できない: {err}"))?;
        commit_of(&workdir, &rev)
    });
    match measured {
        Ok((sha, message)) => {
            let (line, code) = verdict(&sha, &message);
            emit(&line);
            code
        }
        Err(reason) => {
            emit_err(&reason);
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_rev, provenance_of, verdict, Provenance};
    use std::process::ExitCode;

    /// PR の squash の件名（末尾 `(#N)`）は `via=pr` で通り、N を読む。
    #[test]
    fn main_provenance_accepts_squash_subject_with_pr_number() {
        let message = "docs(design): pipeline §7 の改訂 (#542)\n\n本文。\n\nCo-authored-by: someone\n";
        assert_eq!(provenance_of(message), Some(Provenance::Pr(542)));
        let (line, code) = verdict("abc123", message);
        assert_eq!(line, "main-provenance: ok via=pr number=542 sha=abc123");
        assert_eq!(code, ExitCode::SUCCESS);
    }

    /// `pipe land` の trailer（本文の `run: <run id>`）は `via=land` で通り、run id を名指す。
    #[test]
    fn main_provenance_accepts_land_trailer_run_id() {
        let message = "s2-07l.351: 受付の e2e の hub を割る\n\n要旨\n\nrun: s2-07l.351-20260922T061245Z\nScribe2-Contract: docs/design/pipeline.md#b\n";
        assert_eq!(provenance_of(message), Some(Provenance::Land("s2-07l.351-20260922T061245Z".to_owned())));
        let (line, code) = verdict("def456", message);
        assert_eq!(line, "main-provenance: ok via=land run=s2-07l.351-20260922T061245Z sha=def456");
        assert_eq!(code, ExitCode::SUCCESS);
    }

    /// どちらも持たない HEAD は rc 1 で落ちる。**字面の近い偽物**も入口と読まない: 件名の途中の `(#N)`・数字でない
    /// N・空白の無い `(#N)`・件名に置いた trailer・形の崩れた run id・字下げした trailer。
    #[test]
    fn main_provenance_refuses_head_without_either_entrance() {
        for message in [
            "fix: direct push\n",
            "fix: see (#12) later\n",
            "fix: wip (#12a)\n",
            "fix: wip(#12)\n",
            "fix: wip (#)\n",
            "run: s2-07l.351-20260922T061245Z\n",
            "fix: x\n\nrun: s2-07l.351\n",
            "fix: x\n\nrun: s2-07l.351-2026-09-22\n",
            "fix: x\n\nrun: TODO-20260922T061245Z\n",
            "fix: x\n\n  run: s2-07l.351-20260922T061245Z\n",
            "",
        ] {
            assert_eq!(provenance_of(message), None, "{message:?}");
            let (line, code) = verdict("0ff1ce", message);
            assert_eq!(line, "main-provenance: FAIL reason=no-provenance sha=0ff1ce", "{message:?}");
            assert_eq!(code, ExitCode::FAILURE, "{message:?}");
        }
    }

    /// `--rev` は省略で HEAD・値の無い形は Err（CLI 面が rc 2 を返す経路）。
    #[test]
    fn main_provenance_rev_argument_forms() {
        assert_eq!(parse_rev(&[]).ok().as_deref(), Some("HEAD"));
        assert_eq!(parse_rev(&["--rev".to_owned(), "main".to_owned()]).ok().as_deref(), Some("main"));
        assert!(parse_rev(&["--rev".to_owned()]).is_err());
        assert!(parse_rev(&["--rev".to_owned(), String::new()]).is_err());
        assert!(crate::USAGE.contains("main-provenance"), "usage が subcommand を名指す");
    }
}
