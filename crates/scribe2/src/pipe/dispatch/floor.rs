//! 床の検査を撃つ側（設計 docs/design/dispatcher.md §34・契約表の行 ai・FR85 / AC55・ADR-0084・rules 行 floor.timeout_s）。
//!
//! 起こす側の 1 周の頭（[`super::fire`]）だけが、main の先端の sha の tree の宣言の任意 key `floor-check` の 1 行を、sha の木で
//! 封じ込めの内側に 1 回撃つ。観測の口（[`super::turn`]）は撃たない。結果は sha ごとに置き場（`<state>/pipe/floor`）へ残り、
//! 撃てない周と越えた周は結果を残さず次の周に撃ち直す。待つ側（行 aj）は [`judgement`] を読むだけで撃たない。

use super::Input;
use crate::fleet::json_tree::{parse, Tree};
use crate::fleet::store::{lock_owner, started_ms, Owner};
use crate::hook::command::{denied_in, denied_of};
use crate::invocation::Invocation;
use crate::pipe::confine::{release_scope, unit_name, wrap_command, Caps, Limit, Wrap};
use crate::pipe::declaration::{floor_check_at, METACHARS};
use crate::pipe::land::MAIN_REF;
use crate::pipe::{git_line, git_ok};
use crate::rules::manifest::Manifest;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 待つ上限の rules 行の id。
const ROW: &str = "floor.timeout_s";

/// 置き場の下の dir（`<state>/pipe/floor`）。
const DIR: [&str; 2] = ["pipe", "floor"];

/// 今の判定の file の名（sha を持たない）。
const CURRENT: &str = "current";

/// 子の終了を見に行く刻み。
const POLL: Duration = Duration::from_millis(10);

/// 要約の字数の上限。
const SUMMARY_CHARS: usize = 200;

/// 出力の読む末尾の byte 数（多く出す子でも pipe を詰まらせず末尾だけ残す）。
const TAIL_BYTES: usize = 65536;

/// PATH の絶対 path の dir を順に引き、実行できる file の path を 1 行出す（env は読まず sh に引かせる）。
const LOOK: &str = r#"set -f; IFS=:; for d in $PATH; do case $d in /*) [ -f "$d/$1" ] && [ -x "$d/$1" ] && { printf '%s\n' "$d/$1"; exit 0; };; esac; done; exit 1"#;

/// 判定の語（閉じた 4 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// rc 0。
    Pass,
    /// rc 0 でない。
    Fail,
    /// 撃てない（結果を残さない）。
    Unfireable,
    /// 上限を越えた（結果を残さない）。
    Timeout,
}

impl Word {
    /// file と doctor に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unfireable => "unfireable",
            Self::Timeout => "timeout",
        }
    }

    /// 字面から引く。
    fn parse(text: &str) -> Option<Self> {
        [Self::Pass, Self::Fail, Self::Unfireable, Self::Timeout].into_iter().find(|word| word.as_str() == text)
    }
}

/// 1 周の判定（今の判定の file の中身）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judged {
    /// 撃った sha（40 字）。
    pub sha: String,
    /// 語。
    pub word: Word,
    /// 子の rc（撃てない周と越えた周は `None`）。
    pub rc: Option<i32>,
    /// 撃てない理由の語（`form` / `denied` / `metachar` / `path` / `tree` / `confine` / `row`・他は `None`）。
    pub why: Option<String>,
    /// 出力の最後の空でない行（制御文字を除き 200 字・無ければ空）。
    pub summary: String,
}

impl Judged {
    /// doctor の 1 行。
    pub fn line(&self) -> String {
        let rc = self.rc.map_or("-".to_owned(), |found| found.to_string());
        let summary = if self.summary.is_empty() { "-" } else { self.summary.as_str() };
        let sha: String = self.sha.chars().take(7).collect();
        format!("floor={} rc={rc} sha={sha} why={} summary={summary}", self.word.as_str(), self.why.as_deref().unwrap_or("-"))
    }

    /// 今の判定の file の本文（1 行の JSON）。
    pub(super) fn body(&self) -> String {
        let rc = self.rc.map_or("null".to_owned(), |found| found.to_string());
        let why = self.why.as_deref().map_or("null".to_owned(), quoted);
        let at = quoted(&crate::fleet::cli::now_utc());
        let (sha, word, summary) = (quoted(&self.sha), quoted(self.word.as_str()), quoted(&self.summary));
        format!("{{\"schema\":1,\"sha\":{sha},\"word\":{word},\"rc\":{rc},\"why\":{why},\"summary\":{summary},\"at\":{at}}}\n")
    }

    /// 撃てなかった周の判定。
    fn unfireable(sha: &str, why: &str) -> Self {
        Self { sha: sha.to_owned(), word: Word::Unfireable, rc: None, why: Some(why.to_owned()), summary: String::new() }
    }
}

/// JSON の文字列（制御文字は要約が既に除いている）。
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 置き場の floor の dir。
fn dir_of(state_dir: &Path) -> PathBuf {
    DIR.iter().fold(state_dir.to_path_buf(), |path, part| path.join(part))
}

/// sha ごとの結果の file の名（名に sha の 40 字を持つ）。
fn result_of(sha: &str) -> String {
    format!("{sha}.result")
}

/// file を temp へ書いてから rename する（読み手が書きかけを見ない）。
fn put(path: &Path, body: &str) {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temporary, body).is_err() || fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

/// 今の判定の file を読む（無ければ `None`・読めなければ `Some(None)`）。
fn current_of(state_dir: &Path) -> Option<Option<Judged>> {
    match fs::read_to_string(dir_of(state_dir).join(CURRENT)) {
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(_) => Some(None),
        Ok(text) => Some(judged_of(&text)),
    }
}

/// 今の判定の file の本文から読む。
pub(super) fn judged_of(text: &str) -> Option<Judged> {
    let tree = parse(text).ok()?;
    let rc = match tree.get("rc")? {
        Tree::Num(digits) => Some(digits.parse().ok()?),
        Tree::Null => None,
        _ => return None,
    };
    let why = match tree.get("why")? {
        Tree::Str(word) => Some(word.clone()),
        _ => None,
    };
    let text_of = |key: &str| tree.get(key).and_then(Tree::as_str).map(str::to_owned);
    Some(Judged { sha: text_of("sha")?, word: Word::parse(&text_of("word")?)?, rc, why, summary: text_of("summary")? })
}

/// sha の判定を file だけから読む（撃たない・待つ側が呼ぶ）: sha の結果の file → 同じ sha の今の判定の順。
pub fn judgement(state_dir: &Path, sha: &str) -> Option<Judged> {
    if let Ok(text) = fs::read_to_string(dir_of(state_dir).join(result_of(sha))) {
        let tree = parse(&text).ok()?;
        let Tree::Num(digits) = tree.get("rc")? else {
            return None;
        };
        let rc: i32 = digits.parse().ok()?;
        let word = if rc == 0 { Word::Pass } else { Word::Fail };
        return Some(Judged { sha: sha.to_owned(), word, rc: Some(rc), why: None, summary: tree.get("summary")?.as_str()?.to_owned() });
    }
    current_of(state_dir).flatten().filter(|found| found.sha == sha)
}

/// doctor の 1 行（今の判定の file が在る周だけ・読めない file は `floor=unreadable`）。
pub fn doctor_line(state_dir: &Path) -> Option<String> {
    current_of(state_dir).map(|read| read.map_or_else(|| "floor=unreadable".to_owned(), |found| found.line()))
}

/// sha ごとの lock（排他の作成・中身は pid と起動時刻・外れるのは撃ち終えた周の Drop）。
struct Lock(PathBuf);

impl Lock {
    /// 取る。持ち主の死んだ lock は 1 回だけ外して取り直す。生きている・読めない持ち主の lock は取らない。
    fn take(dir: &Path, sha: &str) -> Option<Self> {
        fs::create_dir_all(dir).ok()?;
        let path = dir.join(format!("{sha}.lock"));
        for _ in 0..2 {
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let pid = std::process::id();
                    let body = started_ms(pid).started().map_or_else(|| format!("{pid}\n"), |at| format!("{pid} {at}\n"));
                    let _ = file.write_all(body.as_bytes());
                    return Some(Self(path));
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    let dead = fs::read_to_string(&path).is_ok_and(|body| lock_owner(&body, started_ms) == Owner::Dead);
                    if !dead || fs::remove_file(&path).is_err() {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// sha の一時の木（器の作った木だけを片付ける・Drop で worktree の登録ごと畳む）。
struct Worktree {
    /// 親 repo。
    repo: PathBuf,
    /// 木の path（名に sha の 40 字を持つ）。
    path: PathBuf,
}

impl Worktree {
    /// 作る。同じ sha の死んだ周が残した木は先に片付ける（木でない dir は消さない＝作れない周になる）。
    fn make(repo: &Path, dir: &Path, sha: &str) -> Option<Self> {
        let path = dir.join(format!("{sha}.tree"));
        let text = path.display().to_string();
        if path.join(".git").is_file() {
            let _ = git_ok(repo, &["worktree", "remove", "--force", &text]);
        }
        let _ = git_ok(repo, &["worktree", "prune"]);
        git_ok(repo, &["worktree", "add", "--detach", &text, sha]).then(|| Self { repo: repo.to_path_buf(), path })
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let text = self.path.display().to_string();
        if !git_ok(&self.repo, &["worktree", "remove", "--force", &text]) {
            let _ = fs::remove_dir_all(&self.path);
            let _ = git_ok(&self.repo, &["worktree", "prune"]);
        }
    }
}

/// 起こす側の 1 周の頭の段（宣言に key の無い sha は置き場も作らない・結果の在る sha と lock を取れない周は撃たない）。
pub(super) fn round(input: &Input<'_>) {
    let Some(sha) = git_line(input.repo, &["rev-parse", MAIN_REF]) else {
        return;
    };
    let Ok(Some(row)) = floor_check_at(input.repo, &sha) else {
        return;
    };
    let dir = dir_of(input.state_dir);
    if dir.join(result_of(&sha)).exists() {
        return;
    }
    let Some(_lock) = Lock::take(&dir, &sha) else {
        return;
    };
    // lock を取る間に別の周が撃ち終えた sha は撃ち直さない。
    if dir.join(result_of(&sha)).exists() {
        return;
    }
    let judged = judge(input, &dir, &sha, &row);
    if let Some(rc) = judged.rc {
        let summary = quoted(&judged.summary);
        put(&dir.join(result_of(&sha)), &format!("{{\"schema\":1,\"rc\":{rc},\"summary\":{summary}}}\n"));
    }
    put(&dir.join(CURRENT), &judged.body());
}

/// 行の 3 つの検査（撃つ前・純関数）。最初に当たった語（form → denied → metachar）を返す。禁じる語列の行を読めない周は `row`。
pub(super) fn fault(manifest: &Manifest, row: &str) -> Option<&'static str> {
    if row.trim().is_empty() || row.chars().any(char::is_control) || row.contains(['{', '}']) {
        return Some("form");
    }
    let Some(sources) = denied_of(manifest) else {
        return Some("row");
    };
    if sources.iter().any(|(_, sequences)| denied_in(row, sequences).is_some()) {
        return Some("denied");
    }
    row.chars().any(|found| METACHARS.contains(&found)).then_some("metachar")
}

/// 頭の語を PATH の絶対 path の dir から実行できる file に解く（包みの中で解かせない・解けなければ `None`）。
fn resolve(head: &str) -> Option<String> {
    let out = Invocation::new("sh").args(["-c", LOOK, "sh", head]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !path.is_empty()).then_some(path)
}

/// 1 周を判じる（撃てない 4 形は 0 回・撃った周は木を畳み scope を片付けてから返す）。
fn judge(input: &Input<'_>, dir: &Path, sha: &str, row: &str) -> Judged {
    if let Some(why) = fault(input.manifest, row) {
        return Judged::unfireable(sha, why);
    }
    let mut words = row.split_whitespace();
    let Some(program) = words.next().and_then(resolve) else {
        return Judged::unfireable(sha, "path");
    };
    let Ok(secs) = crate::rules::int_row(input.manifest, ROW) else {
        return Judged::unfireable(sha, "row");
    };
    let mut cmd = Invocation::new(program);
    cmd.args(words);
    let unit = unit_name(&format!("{sha}.tree"), "floor", 1);
    let wrap = Wrap { unit: &unit, limit: Limit::HostReserve, caps: Caps::embedded(), width: None };
    let (mut cmd, confinement) = wrap_command(cmd, &wrap);
    if !confinement.confined() {
        return Judged::unfireable(sha, "confine");
    }
    let Some(tree) = Worktree::make(input.repo, dir, sha) else {
        return Judged::unfireable(sha, "tree");
    };
    cmd.current_dir(&tree.path).process_group(0).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let ran = run(&mut cmd, Duration::from_secs(secs));
    let _ = release_scope(&confinement);
    match ran {
        None => Judged::unfireable(sha, "path"),
        Some(Ran::Timeout) => Judged { sha: sha.to_owned(), word: Word::Timeout, rc: None, why: None, summary: String::new() },
        Some(Ran::Done { rc, summary }) => {
            Judged { sha: sha.to_owned(), word: if rc == 0 { Word::Pass } else { Word::Fail }, rc: Some(rc), why: None, summary }
        }
    }
}

/// 子の終わり方。
enum Ran {
    /// 上限までに終わった（rc は signal で死んだ周が -1）。
    Done { rc: i32, summary: String },
    /// 上限を越えた（group ごと止めた）。
    Timeout,
}

/// 子を撃って上限まで待つ（起こせない周は `None`）。stdout と stderr は別 thread で末尾だけ読む。
fn run(cmd: &mut Invocation, limit: Duration) -> Option<Ran> {
    let mut child = cmd.spawn().ok()?;
    let readers = (child.stdout.take().map(tail), child.stderr.take().map(tail));
    let deadline = Instant::now().checked_add(limit);
    let mut status: Option<ExitStatus> = None;
    loop {
        status = status.or_else(|| child.try_wait().ok().flatten());
        let drained = [&readers.0, &readers.1].iter().all(|reader| reader.as_ref().is_none_or(JoinHandle::is_finished));
        if let (Some(found), true) = (status, drained) {
            let (out, err) = (bytes_of(readers.0), bytes_of(readers.1));
            let summary = summary_of(&out).or_else(|| summary_of(&err)).unwrap_or_default();
            return Some(Ran::Done { rc: found.code().unwrap_or(-1), summary });
        }
        if deadline.is_some_and(|at| Instant::now() >= at) {
            stop(&mut child);
            return Some(Ran::Timeout);
        }
        std::thread::sleep(POLL);
    }
}

/// pipe の末尾 [`TAIL_BYTES`] だけを読み切る thread。
fn tail<R: Read + Send + 'static>(mut pipe: R) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let (mut kept, mut chunk) = (Vec::new(), [0_u8; 8192]);
        while let Ok(count) = pipe.read(&mut chunk) {
            if count == 0 {
                break;
            }
            kept.extend_from_slice(chunk.get(..count).unwrap_or_default());
            let over = kept.len().saturating_sub(TAIL_BYTES);
            kept.drain(..over);
        }
        kept
    })
}

/// 読み終えた thread の byte（読めなかった周は空）。
fn bytes_of(reader: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader.and_then(|handle| handle.join().ok()).unwrap_or_default()
}

/// 子の group ごと止めて待つ（`pipe stop` と同じ `kill -KILL -- -<pgid>`・group が失敗した周は子だけでも止める）。
fn stop(child: &mut Child) {
    let group = format!("-{}", child.id());
    let _ = Invocation::new("kill").args(["-KILL", "--"]).arg(group).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = child.kill();
    let _ = child.wait();
}

/// 出力の最後の空でない行から制御文字を除いて 200 字で切る（空なら `None`）。
pub(super) fn summary_of(output: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(output);
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let kept: String = line.chars().filter(|found| !found.is_control()).take(SUMMARY_CHARS).collect();
    (!kept.is_empty()).then_some(kept)
}
