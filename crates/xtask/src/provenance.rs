//! `cargo xtask main-provenance [--rev <rev>]` の本体（設計 docs/design/pipeline.md §7 約束 3・§60・`s2-07l.170`）。
//!
//! push(main) の CI が撃つ門で、main の HEAD が**入口を通った commit か**を message の字面で測る。本文（2 行目から
//! 後ろ）を `run` / `source` / `none` の 3 語に分け、件名の末尾の ` (#N)` と組む。入口は 3 形:
//! - 器の便の land（本文の 1 行 `run: <run id>`）→ `main-provenance: ok via=land run=<run id> sha=<sha>`（件名に依らない）。
//! - PR の squash（件名の末尾 `(#N)`）で本文が発端の trailer を持つ → `main-provenance: ok via=pr number=<N>
//!   source=<id>[,<id>…] sha=<sha>`。発端の行を持たない (#N) の commit は `FAIL reason=no-source number=<N>`（rc 1）。
//! - どちらでもない commit（直接の push）は発端の行を持っていても `FAIL reason=no-provenance sha=<sha>`（rc 1）。
//!
//! 発端の行の読み（正本は docs/design/vessel-hook.md §21 形 2・見本は `source_trailer_cases.txt`）: key は NAME の
//! 先頭を大文字にして `-Source:` を足した字。本文の各行から末尾の CR・半角空白・tab を落とし、**行頭から key で
//! 始まる行**を発端の行と読む（字下げと大小の違う行は発端の行でない）。形に合うのは「key・半角空白 1 つ・bead id
//! を半角空白 1 つで区切って 1 本以上」だけ。形の外の発端の行を 1 本でも持つ本文は `none`、そうでなく `run:` の
//! 行を持てば `run`、そうでなく発端の行を持てば `source`（id は出てきた順）、どれでもなければ `none`。
//!
//! git を撃てない・rev が解けない・workspace の NAME を読めない周は判定行を出さず stderr へ理由を出して rc 2
//! （測れなかったを通過にも赤にも化けさせない）。

use crate::flipcheck::is_bead_id;
use crate::workspace::Layout;
use crate::{emit, emit_err};
use std::path::Path;
use std::process::{Command, ExitCode};

/// `--rev` の既定（CI は checkout した HEAD を測る）。
const DEFAULT_REV: &str = "HEAD";

/// land の trailer の頭（`pipe land` が squash の本文へ書く 1 行 `run: <run id>`）。
const RUN_TRAILER: &str = "run: ";

/// 発端の trailer の key の語幹（key は NAME から導く・C2.2）。
const SOURCE_STEM: &str = "Source";

/// commit の出所（入口の 2 形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Provenance {
    /// PR の squash（件名の末尾 `(#N)`）と発端の行の id の列。
    Pr(u64, Vec<String>),
    /// `pipe land` の trailer（`run: <run id>`）。
    Land(String),
}

/// 本文の分け方の 3 語。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Body {
    /// `is_run_id` の形の `run:` の行を持つ。
    Run(String),
    /// 形に合う発端の行を 1 本以上持つ（id は出てきた順）。
    Source(Vec<String>),
    /// どちらでもない・形の外の発端の行を持つ。
    Unsourced,
}

/// 発端の trailer の key（NAME の先頭を大文字にして `-Source:` を足す・core の `trailer_key` と同じ導き）。
pub(crate) fn source_key(name: &str) -> String {
    let mut chars = name.chars();
    let head: String = chars.next().map(|first| first.to_uppercase().to_string()).unwrap_or_default();
    format!("{head}{}-{SOURCE_STEM}:", chars.as_str())
}

/// 本文（2 行目から後ろ）を `run` / `source` / `none` に分ける。
pub(crate) fn classify_body(body: &str, key: &str) -> Body {
    let mut run = None;
    let mut ids = Vec::new();
    for raw in body.lines() {
        let line = raw.trim_end_matches(['\r', ' ', '\t']);
        if let Some(rest) = line.strip_prefix(key) {
            match rest.strip_prefix(' ').filter(|list| list.split(' ').all(is_bead_id)) {
                Some(list) => ids.extend(list.split(' ').map(str::to_owned)),
                None => return Body::Unsourced,
            }
        } else if run.is_none() {
            run = raw.trim_end().strip_prefix(RUN_TRAILER).filter(|id| is_run_id(id));
        }
    }
    match run {
        Some(run) => Body::Run(run.to_owned()),
        None if !ids.is_empty() => Body::Source(ids),
        None => Body::Unsourced,
    }
}

/// 件名（1 行目）と本文（2 行目から後ろ）。
fn split_message(message: &str) -> (&str, &str) {
    message.split_once('\n').unwrap_or((message, ""))
}

/// commit message から出所を読む。本文が `run` なら land、`source` で件名が (#N) で終わるなら PR。
pub(crate) fn provenance_of(message: &str, key: &str) -> Option<Provenance> {
    let (subject, body) = split_message(message);
    match classify_body(body, key) {
        Body::Run(run) => Some(Provenance::Land(run)),
        Body::Source(ids) => pr_number(subject).map(|number| Provenance::Pr(number, ids)),
        Body::Unsourced => None,
    }
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
pub(crate) fn verdict(sha: &str, message: &str, key: &str) -> (String, ExitCode) {
    match provenance_of(message, key) {
        Some(Provenance::Pr(number, ids)) => (
            format!("main-provenance: ok via=pr number={number} source={} sha={sha}", ids.join(",")),
            ExitCode::SUCCESS,
        ),
        Some(Provenance::Land(run)) => (format!("main-provenance: ok via=land run={run} sha={sha}"), ExitCode::SUCCESS),
        None => match pr_number(split_message(message).0) {
            Some(number) => (format!("main-provenance: FAIL reason=no-source number={number} sha={sha}"), ExitCode::FAILURE),
            None => (format!("main-provenance: FAIL reason=no-provenance sha={sha}"), ExitCode::FAILURE),
        },
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

/// CLI 面。判定行を stdout へ 1 行だけ出し rc を返す（測れなかった周だけが rc 2）。NAME を読むのはここだけ。
pub fn run(args: &[String]) -> ExitCode {
    let measured = parse_rev(args).and_then(|rev| {
        let workdir = std::env::current_dir().map_err(|err| format!("main-provenance: cwd を解決できない: {err}"))?;
        let layout = Layout::discover(&workdir).map_err(|err| format!("main-provenance: NAME を読めない: {err}"))?;
        commit_of(&workdir, &rev).map(|(sha, message)| (sha, message, source_key(&layout.name)))
    });
    match measured {
        Ok((sha, message, key)) => {
            let (line, code) = verdict(&sha, &message, &key);
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
    use super::{classify_body, parse_rev, provenance_of, source_key, verdict, Body, Provenance};
    use crate::workspace::Layout;
    use std::path::PathBuf;
    use std::process::ExitCode;

    /// 見本 file（xtask と core の 2 つの読み手が読む 1 本）。
    const CASES: &str = include_str!("source_trailer_cases.txt");

    /// 本 repo の workspace の NAME から組んだ key（歯の source に NAME の字面を置かない・name-literal）。
    fn repo_key() -> String {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        source_key(&Layout::discover(&root).expect("自 workspace は読める").name)
    }

    /// 見本 file の期待の語。
    fn word(body: &Body) -> &'static str {
        match body {
            Body::Run(_) => "run",
            Body::Source(_) => "source",
            Body::Unsourced => "none",
        }
    }

    /// 見本 file の塊（期待の語・本文）の列。行頭 `#` は注、行 `===` が区切り、空の塊（先頭の区切りの前）は数えない。
    fn sample_cases() -> Vec<(String, String)> {
        let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
        for line in CASES.lines().filter(|line| !line.starts_with('#')) {
            match line {
                "===" => blocks.push(Vec::new()),
                _ => blocks.last_mut().expect("列は空でない").push(line),
            }
        }
        blocks
            .into_iter()
            .filter_map(|block| block.split_first().map(|(word, body)| ((*word).to_owned(), body.join("\n"))))
            .collect()
    }

    /// (a) 見本の全塊を分けた語が期待と一致する。塊は 24 以上で 3 語が全部出る。行末の CR・半角空白・tab を持つ本文は
    /// file に置かず、ここで組んで `source` と読む。
    #[test]
    fn main_provenance_source_sample_cases_match_expected_words() {
        let key = repo_key();
        let cases = sample_cases();
        for (expected, body) in &cases {
            assert_eq!(word(&classify_body(body, &key)), expected, "{body:?}");
        }
        assert!(cases.len() >= 24, "塊が 24 以上: {}", cases.len());
        for word in ["source", "run", "none"] {
            assert!(cases.iter().any(|(expected, _)| expected == word), "{word} の塊が在る");
        }
        for tail in ["\r", " ", "\t", " \r"] {
            let body = format!("要旨\n{key} s2-a.1{tail}\nCo-Authored-By: someone\r\n");
            assert_eq!(classify_body(&body, &key), Body::Source(vec!["s2-a.1".to_owned()]), "{body:?}");
        }
    }

    /// (b) 件名が (#N) で終わり発端の行の無い commit は `no-source` で N を名指して rc 1。
    #[test]
    fn main_provenance_source_refuses_pr_number_without_source() {
        let message = "docs(design): pipeline §7 の改訂 (#542)\n\n本文。\n\nCo-authored-by: someone\n";
        assert_eq!(provenance_of(message, &repo_key()), None);
        let (line, code) = verdict("abc123", message, &repo_key());
        assert_eq!(line, "main-provenance: FAIL reason=no-source number=542 sha=abc123");
        assert_eq!(code, ExitCode::FAILURE);
    }

    /// (c) (#N) と発端の行を持つ commit は `via=pr` で通り、id を出てきた順に `,` で並べる。
    #[test]
    fn main_provenance_source_accepts_pr_number_with_source_ids() {
        let key = repo_key();
        for (ids, listed) in [("s2-07l.739", "s2-07l.739"), ("s2-07l.739 s2-07l.722", "s2-07l.739,s2-07l.722")] {
            let message = format!("docs(design): 行 bc (#810)\n\n要旨\n\n{key} {ids}\n");
            let (line, code) = verdict("0ff1ce", &message, &key);
            assert_eq!(line, format!("main-provenance: ok via=pr number=810 source={listed} sha=0ff1ce"));
            assert_eq!(code, ExitCode::SUCCESS);
        }
    }

    /// (d) 件名に (#N) が無ければ、形に合う発端の行を持っていても `no-provenance`（直接の push）。
    #[test]
    fn main_provenance_source_refuses_source_without_pr_number() {
        let key = repo_key();
        let message = format!("fix: direct push\n\n{key} s2-07l.739\n");
        let (line, code) = verdict("0ff1ce", &message, &key);
        assert_eq!(line, "main-provenance: FAIL reason=no-provenance sha=0ff1ce");
        assert_eq!(code, ExitCode::FAILURE);
    }

    /// (e) key は名から導く: 別の名で組んだ key では、その key の行だけが発端の行で、本 repo の key の行は読まない。
    #[test]
    fn main_provenance_source_key_derives_from_name() {
        let key = source_key("fixturename");
        assert_eq!(key, "Fixturename-Source:");
        assert_eq!(classify_body("Fixturename-Source: s2-a.1", &key), Body::Source(vec!["s2-a.1".to_owned()]));
        assert_eq!(classify_body(&format!("{} s2-a.1", repo_key()), &key), Body::Unsourced);
    }

    /// `pipe land` の trailer（本文の `run: <run id>`）は `via=land` で通り、run id を名指す。
    #[test]
    fn main_provenance_accepts_land_trailer_run_id() {
        let message = "s2-07l.351: 受付の e2e の hub を割る\n\n要旨\n\nrun: s2-07l.351-20260922T061245Z\nScribe2-Contract: docs/design/pipeline.md#b\n";
        assert_eq!(provenance_of(message, &repo_key()), Some(Provenance::Land("s2-07l.351-20260922T061245Z".to_owned())));
        let (line, code) = verdict("def456", message, &repo_key());
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
            assert_eq!(provenance_of(message, &repo_key()), None, "{message:?}");
            let (line, code) = verdict("0ff1ce", message, &repo_key());
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
