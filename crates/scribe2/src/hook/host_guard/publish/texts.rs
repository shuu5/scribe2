//! publish の出ていく字面の読み（設計 docs/design/vessel-hook.md §22 行 n3・ADR-0078・SRS FR80 / NFR5）。
//!
//! git push の出ていく commit を 1 回の `git log`（欄の区切りを commit 自身の sha で閉じる書式）で message・author の名・committer の名・
//! patch の追加行・patch の path に割り、押す tag の本文と押す ref の名と gh の本文・本文の file の中身を足す。字面の合計は 1 つの
//! 計数（[`Budget`]）で数え、越えた周は読みを止めて越えの印を立てる（断るのは後続の行）。解けない字面は [`Gap::Text`]。

use super::outgoing::{Gap, Halt, Push, Ref};
use super::probe::{run, Bound, Stop};
use super::scan::{gh_texts, Source, Text};
use super::Published;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::ops::Range;
use std::path::Path;

/// 欄の頭の印の頭の byte（RS）。
const RS: u8 = 0x1e;
/// 欄の区切りの byte（US）。
const US: u8 = 0x1f;
/// 出ていく commit を 1 回で読む `git log` の引数（欄の区切りは commit 自身の sha で閉じる）。
const LOG: [&str; 15] = [
    "-c", "core.quotePath=false", "log", "--no-walk=unsorted", "--stdin", "--format=%x1e%H%x1f%an%x1f%cn%x1f%H%x1f%B%x1f%H%x1f",
    "--patch-with-raw", "-M", "--no-color", "--no-textconv", "--no-ext-diff", "--src-prefix=a/", "--dst-prefix=b/", "--no-relative",
    "--diff-merges=first-parent",
];

/// 出ていく字面 1 つ（出所の種別と本文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    /// 出所の種別。
    pub source: Source,
    /// 本文。
    pub body: String,
}

/// segment 1 つの出ていく字面（本文の列と越えの印）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Texts {
    /// 本文の列（commit ごとに message・author・committer・追加行・path、続いて tag の本文と ref の名）。
    pub bodies: Vec<Body>,
    /// 出ていく字面が読む上限を越えたか（越えた周は読みを止めた・断るのは行 n5）。
    pub over: bool,
}

impl Texts {
    /// 照合の核（`scan`）へ渡す借用の本文。
    pub fn texts(&self) -> Vec<Text<'_>> {
        self.bodies.iter().map(|found| Text { source: found.source, body: &found.body }).collect()
    }

    /// 本文を足す（[`Source`] と本文）。
    fn push(&mut self, source: Source, body: String) {
        self.bodies.push(Body { source, body });
    }
}

/// 出ていく字面の byte の合計を数える 1 つの計数（残りの byte）。
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// 残りの byte。
    pub left: u64,
}

impl Budget {
    /// `bytes` を数える（残りに収まれば減らして `true`・越えれば残りを 0 にして `false`）。
    fn take(&mut self, bytes: usize) -> bool {
        let need = u64::try_from(bytes).unwrap_or(u64::MAX);
        let rest = self.left.checked_sub(need);
        self.left = rest.unwrap_or(0);
        rest.is_some()
    }
}

/// git を 1 回撃つ場（大域の語は引数の前に置く）。
struct Child<'a> {
    /// program。
    program: &'a Path,
    /// 大域の語。
    globals: &'a [String],
    /// 撃つ dir。
    dir: &'a Path,
    /// 締め切りと読む上限。
    bound: Bound,
}

impl Child<'_> {
    /// 撃って標準出力の byte を返す（残りの上限で読む・越えは `None`・rc 非 0 と起こせない周は解けない）。
    fn shoot(&self, args: &[&str], input: &str, budget: &mut Budget) -> Result<Option<Vec<u8>>, Halt> {
        let all: Vec<&str> = self.globals.iter().map(String::as_str).chain(args.iter().copied()).collect();
        let bound = Bound { limit: budget.left, ..self.bound };
        let ran = match run(self.program, &all, self.dir, input.as_bytes(), bound) {
            Ok(ran) => ran,
            Err(Stop::Deadline) => return Err(Halt::Deadline),
            Err(Stop::Spawn) => return Err(Halt::Gap(Gap::Text)),
        };
        if ran.over {
            budget.left = 0;
            return Ok(None);
        }
        if ran.code != 0 || !budget.take(ran.bytes.len()) {
            return Err(Halt::Gap(Gap::Text));
        }
        Ok(Some(ran.bytes))
    }
}

/// 印 1 つ（頭の印か閉じの印・commit の番号・出力の中の位置）。
#[derive(Debug, Clone, Copy)]
struct Mark {
    /// 頭の印（RS・sha・US）か。閉じの印は（US・sha・US）。
    head: bool,
    /// 出ていく commit の順の番号。
    index: usize,
    /// 印の先頭の位置。
    at: usize,
}

/// 出力の中の全ての印（出ていく commit の sha を持つもの・重なりも数える）。
fn marks(output: &[u8], shas: &[String]) -> Vec<Mark> {
    let known: HashMap<&[u8], usize> = shas.iter().enumerate().map(|(index, sha)| (sha.as_bytes(), index)).collect();
    let width = shas.first().map_or(0, String::len);
    let mut found = Vec::new();
    for (at, byte) in output.iter().enumerate() {
        if *byte != RS && *byte != US {
            continue;
        }
        let from = at.saturating_add(1);
        let (sha, close) = (output.get(from..from.saturating_add(width)), output.get(from.saturating_add(width)));
        if let (Some(sha), Some(&US)) = (sha, close) {
            if let Some(&index) = known.get(sha) {
                found.push(Mark { head: *byte == RS, index, at });
            }
        }
    }
    found
}

/// commit 1 つの欄の範囲（名の欄・message・patch）。
struct Parts {
    /// 名の欄（author の名・US・committer の名）。
    names: Range<usize>,
    /// message。
    message: Range<usize>,
    /// patch。
    patch: Range<usize>,
}

/// 印の並びを検める（頭の印はちょうど 1 回ずつ出ていく commit の順・閉じの印はその後ろにちょうど 2 回・先頭は最初の commit の頭の印）
/// 並びが合えば commit ごとの欄の範囲。合わない周と欄の範囲が逆転する周は `None`。
fn parts(output: &[u8], shas: &[String]) -> Option<Vec<Parts>> {
    let found = marks(output, shas);
    let width = shas.first().map_or(0, String::len);
    let shape = |at: usize| at.saturating_add(width).saturating_add(2);
    if found.len() != shas.len().checked_mul(3)? || found.first().is_some_and(|mark| mark.at != 0) {
        return None;
    }
    let mut all = Vec::new();
    for (index, chunk) in found.chunks(3).enumerate() {
        let [head, first, second] = chunk else {
            return None;
        };
        if !(head.head && !first.head && !second.head && [head, first, second].iter().all(|mark| mark.index == index)) {
            return None;
        }
        let next = found.get(index.saturating_add(1).saturating_mul(3)).map_or(output.len(), |mark| mark.at);
        let (names, message, patch) = (shape(head.at)..first.at, shape(first.at)..second.at, shape(second.at)..next);
        if names.start > names.end || message.start > message.end || patch.start > patch.end {
            return None;
        }
        all.push(Parts { names, message, patch });
    }
    Some(all)
}

/// C の引用（`"…"`・`\n` `\t` `\"` `\\` などと 3 桁までの 8 進）を解く（引用でない字はそのまま）。
fn unquote(field: &str) -> String {
    let Some(inner) = field.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) else {
        return field.to_owned();
    };
    let (bytes, mut out, mut at) = (inner.as_bytes(), Vec::new(), 0_usize);
    while let Some(&byte) = bytes.get(at) {
        at = at.saturating_add(1);
        let Some(&escaped) = bytes.get(at).filter(|_| byte == b'\\') else {
            out.push(byte);
            continue;
        };
        at = at.saturating_add(1);
        out.push(match escaped {
            b'a' => 7,
            b'b' => 8,
            b'f' => 12,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 11,
            b'0'..=b'7' => {
                let digits = bytes.get(at..).unwrap_or_default().iter().take(2).take_while(|found| (b'0'..=b'7').contains(found)).count();
                let span = bytes.get(at.saturating_sub(1)..at.saturating_add(digits)).unwrap_or_default();
                at = at.saturating_add(digits);
                let value = span.iter().fold(0_u32, |value, digit| value.saturating_mul(8).saturating_add(u32::from(digit.saturating_sub(b'0'))));
                u8::try_from(value & 0xff).unwrap_or_default()
            }
            other => other,
        });
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// patch の（追加行・path）: raw の行（`:` で始まる・hunk の外）の tab で割った path（rename / copy の両側・C の引用を解く）と、
/// `@@` の後の hunk の中の `+` の行（先頭の `+` を落とす）。binary の patch は hunk が無いので path だけ。
fn patch_of(patch: &str) -> (Vec<String>, Vec<String>) {
    let (mut added, mut paths, mut hunk) = (Vec::new(), Vec::new(), false);
    for line in patch.split('\n') {
        if line.starts_with("diff --git ") {
            hunk = false;
        } else if hunk {
            added.extend(line.strip_prefix('+').map(str::to_owned));
        } else if line.starts_with("@@") {
            hunk = true;
        } else if line.starts_with(':') {
            paths.extend(line.split('\t').skip(1).map(unquote));
        }
    }
    (added, paths)
}

/// 出力の byte の範囲を UTF-8 の置き換えで読んだ字。
fn lossy(output: &[u8], range: Range<usize>) -> String {
    String::from_utf8_lossy(output.get(range).unwrap_or_default()).into_owned()
}

/// 名の欄（US をちょうど 1 つ持つ）を（author・committer）に割る（どちらかが UTF-8 でない周も `None`）。
fn names_of(field: &[u8]) -> Option<(String, String)> {
    let mut halves = field.split(|byte| *byte == US);
    let (Some(author), Some(committer), None) = (halves.next(), halves.next(), halves.next()) else {
        return None;
    };
    Some((String::from_utf8(author.to_vec()).ok()?, String::from_utf8(committer.to_vec()).ok()?))
}

/// `git log` の出力を commit ごとの本文に割る（**1 関数**・区切りを探して欄を推す読みは持たない）。
fn commit_bodies(output: &[u8], shas: &[String]) -> Option<Vec<Body>> {
    let mut found = Texts::default();
    for one in parts(output, shas)? {
        let (author, committer) = names_of(output.get(one.names)?)?;
        found.push(Source::CommitMessage, lossy(output, one.message));
        found.push(Source::AuthorName, author);
        found.push(Source::CommitterName, committer);
        let (added, paths) = patch_of(&lossy(output, one.patch));
        added.into_iter().for_each(|line| found.push(Source::PatchAdded, line));
        paths.into_iter().for_each(|path| found.push(Source::PatchPath, path));
    }
    Some(found.bodies)
}

/// 押す tag の本文（`cat-file --batch` 1 回・header の後の空行から）。越えは `None`。
fn tag_bodies(child: &Child, refs: &[Ref], budget: &mut Budget) -> Result<Option<Vec<Body>>, Halt> {
    let tags: Vec<&str> = refs.iter().filter(|found| found.kind == "tag").map(|found| found.from.as_str()).collect();
    if tags.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let input: String = tags.iter().map(|sha| format!("{sha}\n")).collect();
    let Some(output) = child.shoot(&["cat-file", "--batch"], &input, budget)? else {
        return Ok(None);
    };
    let (mut at, mut found) = (0_usize, Vec::new());
    for _ in &tags {
        let rest = output.get(at..).unwrap_or_default();
        let line = rest.split(|byte| *byte == b'\n').next().unwrap_or_default();
        let size = std::str::from_utf8(line).ok().and_then(|head| match head.split(' ').collect::<Vec<&str>>().as_slice() {
            [_, "tag", size] => size.parse::<usize>().ok(),
            _ => None,
        });
        let start = at.saturating_add(line.len()).saturating_add(1);
        let end = start.saturating_add(size.ok_or(Halt::Gap(Gap::Text))?);
        let content = output.get(start..end).ok_or(Halt::Gap(Gap::Text))?;
        let body = content.windows(2).position(|pair| pair == b"\n\n").and_then(|blank| content.get(blank.saturating_add(2)..)).unwrap_or_default();
        found.push(Body { source: Source::TagBody, body: String::from_utf8_lossy(body).into_owned() });
        at = end.saturating_add(1);
    }
    Ok(Some(found))
}

/// git push の segment 1 つの出ていく字面（**入口**・`push` は行き先の解きの結果）: commit の字面を `git log` 1 回で、押す tag の本文を
/// `cat-file --batch` 1 回で、押す ref の名（削除を含む）を足す。子は残りの上限で読み、越えた周は読みを止めて越えの印を立てる。
/// UTF-8 でない名・印の並びの違い・rc 非 0 は [`Gap::Text`]、締め切りの越えは [`Halt::Deadline`]。
pub fn of_push(found: &Published, push: &Push, program: &Path, bound: Bound, budget: &mut Budget) -> Result<Texts, Halt> {
    let mut texts = Texts { over: push.over, ..Texts::default() };
    if push.over {
        return Ok(texts);
    }
    let child = Child { program, globals: &found.globals, dir: &found.dir, bound };
    let input: String = push.commits.iter().map(|sha| format!("{sha}\n")).collect();
    let log = if push.commits.is_empty() {
        Some(Vec::new())
    } else {
        child.shoot(&LOG, &input, budget)?.map(|out| commit_bodies(&out, &push.commits).ok_or(Halt::Gap(Gap::Text))).transpose()?
    };
    let tags = if log.is_some() { tag_bodies(&child, &push.refs, budget)? } else { None };
    let (Some(log), Some(tags)) = (log, tags) else {
        texts.over = true;
        return Ok(texts);
    };
    texts.bodies.extend(log.into_iter().chain(tags));
    for pushed in &push.refs {
        if !budget.take(pushed.to.len()) {
            texts.over = true;
            break;
        }
        texts.push(Source::RefName, pushed.to.clone());
    }
    Ok(texts)
}

/// gh の本文の file の中身（開く前に metadata〔symlink を辿る〕で普通の file かを判じ、FIFO・dir・device・socket と metadata を読めない
/// path は開かずに解けない・残りの上限まで読み、越えは `None`）。
fn read_file(path: &Path, budget: &mut Budget) -> Result<Option<String>, Halt> {
    let unresolved = || Halt::Gap(Gap::Text);
    if !std::fs::metadata(path).map_err(|_| unresolved())?.is_file() {
        return Err(unresolved());
    }
    let mut bytes = Vec::new();
    File::open(path).and_then(|file| file.take(budget.left.saturating_add(1)).read_to_end(&mut bytes)).map_err(|_| unresolved())?;
    Ok(budget.take(bytes.len()).then(|| String::from_utf8_lossy(&bytes).into_owned()))
}

/// gh の segment 1 つの出ていく字面: [`gh_texts`] の本文と、それが返す path の file の中身（[`Published::dir`] から解く）。読めない file
/// と普通の file でない path は [`Gap::Text`]、越えは越えの印。
pub fn of_gh(found: &Published, budget: &mut Budget) -> Result<Texts, Halt> {
    let read = gh_texts(found);
    let mut texts = Texts::default();
    for value in read.bodies {
        if !budget.take(value.len()) {
            texts.over = true;
            return Ok(texts);
        }
        texts.push(Source::GhFlagValue, value);
    }
    for file in &read.files {
        match read_file(&found.dir.join(file), budget)? {
            Some(body) => texts.push(Source::GhFileBody, body),
            None => {
                texts.over = true;
                break;
            }
        }
    }
    Ok(texts)
}

#[cfg(test)]
mod tests {
    use super::{commit_bodies, of_gh, of_push, unquote, Body, Budget, Halt, Texts};
    use crate::hook::host_guard::publish::outgoing::{Gap, Push, Ref};
    use crate::hook::host_guard::publish::probe::Bound;
    use crate::hook::host_guard::publish::scan::Source;
    use crate::hook::host_guard::publish::{Published, Sort};
    use crate::invocation::Invocation;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    /// git を 1 回撃ち rc 0 を要求して標準出力を返す。
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Invocation::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@e.invalid"]).args(args).output();
        let out = out.unwrap_or_else(|why| panic!("git を撃てる: {why}"));
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// 歯ごとの置き場（commit を 1 つ持つ repo）。
    fn place(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scribe2-publish-texts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|why| panic!("dir を作れる: {why}"));
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "base"]);
        dir
    }

    /// 解く場（締め切りは 20 秒後・読む上限は `limit`）。
    fn bound(limit: u64) -> Bound {
        Bound { deadline: Instant::now() + Duration::from_secs(20), limit }
    }

    /// dir で撃つ git push の読み。
    fn pushing(dir: &Path) -> Published {
        Published { dir: dir.to_path_buf(), resolved: true, ..Published::default() }
    }

    /// 本文の出所と字の対。
    fn pairs(texts: &Texts) -> Vec<(Source, &str)> {
        texts.bodies.iter().map(|found| (found.source, found.body.as_str())).collect()
    }

    /// 出ていく commit の列と ref を持つ push の解き。
    fn push_of(commits: Vec<String>, refs: Vec<Ref>) -> Push {
        Push { commits, refs, ..Push::default() }
    }

    /// 行 n3 (a) 1 本の log の出力から 7 つの出所を順に取る: message・author・committer・追加行・path（rename の両側と binary の path）・
    /// 押す tag の本文・押す ref の名（削除を含む）。
    #[test]
    fn publish_texts_one_log_output_gives_the_seven_sources_in_order() {
        let dir = place("seven");
        std::fs::write(dir.join("old.txt"), "keep one\nkeep two\nkeep three\nkeep four\n").unwrap_or_else(|why| panic!("書ける: {why}"));
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        let base = git(&dir, &["rev-parse", "HEAD"]);
        git(&dir, &["mv", "old.txt", "new name.txt"]);
        std::fs::write(dir.join("new name.txt"), "keep one\nkeep two\nkeep three\nkeep four\nadded line\n").unwrap_or_else(|why| panic!("書ける: {why}"));
        std::fs::write(dir.join("blob.bin"), [0_u8, 1, 2, 0, 255]).unwrap_or_else(|why| panic!("書ける: {why}"));
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "second message"]);
        let head = git(&dir, &["rev-parse", "HEAD"]);
        git(&dir, &["tag", "-a", "-m", "tag body text", "v1"]);
        let tag = git(&dir, &["rev-parse", "v1"]);
        let refs = vec![
            Ref { from: tag, kind: "tag".to_owned(), to: "refs/tags/v1".to_owned(), old: None },
            Ref { from: String::new(), kind: String::new(), to: "refs/heads/gone".to_owned(), old: Some(base.clone()) },
        ];
        let mut budget = Budget { left: 1 << 20 };
        let texts = of_push(&pushing(&dir), &push_of(vec![head, base], refs), Path::new("git"), bound(1 << 20), &mut budget);
        let texts = texts.unwrap_or_else(|why| panic!("読める: {why:?}"));
        let body = |source: Source| -> Vec<&str> { texts.bodies.iter().filter(|found| found.source == source).map(|found| found.body.as_str()).collect() };
        assert!(!texts.over);
        assert_eq!(body(Source::CommitMessage).first().map(|found| found.trim()), Some("second message"));
        assert_eq!((body(Source::AuthorName), body(Source::CommitterName)), (vec!["t", "t"], vec!["t", "t"]));
        assert_eq!(body(Source::PatchAdded), ["added line", "keep one", "keep two", "keep three", "keep four"], "追加行");
        assert_eq!(body(Source::PatchPath), ["blob.bin", "old.txt", "new name.txt", "old.txt"], "rename の両側と binary の path");
        assert!(body(Source::TagBody).first().is_some_and(|found| found.contains("tag body text")), "{:?}", body(Source::TagBody));
        assert_eq!(body(Source::RefName), ["refs/tags/v1", "refs/heads/gone"]);
        let order: Vec<Source> = texts.bodies.iter().map(|found| found.source).collect();
        let at = |source: Source| order.iter().position(|found| *found == source).unwrap_or(usize::MAX);
        assert!(at(Source::CommitMessage) < at(Source::AuthorName) && at(Source::CommitterName) < at(Source::PatchAdded));
        assert!(at(Source::PatchPath) < at(Source::TagBody) && at(Source::TagBody) < at(Source::RefName), "出所の順: {order:?}");
        assert_eq!(unquote("\"a\\tb\\303\\251\\\"\""), "a\tbé\"", "C の引用");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 印つきの出力 1 commit 分（sha `s`）。
    fn frame(sha: &str, names: &[u8], message: &[u8], patch: &[u8]) -> Vec<u8> {
        let mut out = [&[0x1e_u8][..], sha.as_bytes(), &[0x1f], names, &[0x1f], sha.as_bytes(), &[0x1f]].concat();
        out.extend_from_slice(message);
        out.extend_from_slice(&[[0x1f].as_slice(), sha.as_bytes(), &[0x1f], patch].concat());
        out
    }

    /// 行 n3 (b) UTF-8 でない author の名は解けず（unresolved:text）、UTF-8 でない message は置き換えて読む。
    #[test]
    fn publish_texts_non_utf8_author_is_unresolved_and_message_is_replaced() {
        let dir = place("utf8");
        let empty = git(&dir, &["rev-parse", "HEAD^{tree}"]);
        let commit = |author: &[u8], message: &[u8]| {
            let object = [format!("tree {empty}\nauthor ").as_bytes(), author, b" <a@e.invalid> 0 +0000\ncommitter c <c@e.invalid> 0 +0000\n\n", message].concat();
            let mut call = Invocation::new("git");
            call.arg("-C").arg(&dir).args(["hash-object", "-t", "commit", "-w", "--literally", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped());
            let mut child = call.spawn().unwrap_or_else(|why| panic!("git を撃てる: {why}"));
            child.stdin.take().unwrap_or_else(|| panic!("stdin を開ける")).write_all(&object).unwrap_or_else(|why| panic!("書ける: {why}"));
            let out = child.wait_with_output().unwrap_or_else(|why| panic!("待てる: {why}"));
            assert!(out.status.success(), "hash-object が rc 0");
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        let run = |sha: String| {
            let mut budget = Budget { left: 1 << 20 };
            of_push(&pushing(&dir), &push_of(vec![sha], Vec::new()), Path::new("git"), bound(1 << 20), &mut budget)
        };
        assert_eq!(run(commit(b"bad\xffname", b"m")), Err(Halt::Gap(Gap::Text)), "UTF-8 でない author の名");
        let texts = run(commit(b"good", b"m\xfe\xfdx\n")).unwrap_or_else(|why| panic!("読める: {why:?}"));
        let replaced = |found: &(Source, &str)| found.0 == Source::CommitMessage && found.1.starts_with("m\u{fffd}\u{fffd}x");
        assert!(pairs(&texts).iter().any(replaced), "{:?}", pairs(&texts));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 行 n3 (c) gh の本文の file を dir から読み（相対 path は Published の dir から解く）、無い file と dir の path は解けない。
    #[test]
    fn publish_texts_gh_body_file_is_read_from_the_dir_and_unreadable_is_unresolved() {
        let dir = place("body");
        std::fs::write(dir.join("body.md"), "file words").unwrap_or_else(|why| panic!("書ける: {why}"));
        let gh = |file: &str| Published {
            sort: Sort::Gh,
            group: Some("pr".to_owned()),
            verb: Some("create".to_owned()),
            rest: vec!["--title".to_owned(), "t".to_owned(), "--body-file".to_owned(), file.to_owned()],
            ..pushing(&dir)
        };
        let mut budget = Budget { left: 1 << 20 };
        let texts = of_gh(&gh("body.md"), &mut budget).unwrap_or_else(|why| panic!("読める: {why:?}"));
        assert_eq!(pairs(&texts), [(Source::GhFlagValue, "t"), (Source::GhFileBody, "file words")]);
        for file in ["missing.md", ".", ""] {
            assert_eq!(of_gh(&gh(file), &mut Budget { left: 1 << 20 }), Err(Halt::Gap(Gap::Text)), "{file:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 行 n3 (d) 上限を越える log は読みを止めて越えの印を立て、合計の計数を使い切る。
    #[test]
    fn publish_texts_over_the_limit_marks_over() {
        let dir = place("over");
        std::fs::write(dir.join("big.txt"), "x".repeat(20_000)).unwrap_or_else(|why| panic!("書ける: {why}"));
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "big"]);
        let head = git(&dir, &["rev-parse", "HEAD"]);
        let mut budget = Budget { left: 4096 };
        let texts = of_push(&pushing(&dir), &push_of(vec![head], Vec::new()), Path::new("git"), bound(4096), &mut budget);
        let texts = texts.unwrap_or_else(|why| panic!("読める: {why:?}"));
        assert!(texts.over && texts.bodies.is_empty(), "越えの印: {texts:?}");
        assert_eq!(budget.left, 0);
        let mut small = Budget { left: 3 };
        let gh = Published { sort: Sort::Gh, group: Some("pr".to_owned()), verb: Some("create".to_owned()), rest: vec!["--title".to_owned(), "long title".to_owned()], ..pushing(&dir) };
        assert!(of_gh(&gh, &mut small).is_ok_and(|texts| texts.over), "gh の本文も同じ計数");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 行 n3 (g) 印の並びの違いは解けない: 古い commit の頭の印を持つ新しい commit の message・名の欄の US が 2 つ・author の名が US を持ち
    /// committer の名が UTF-8 でない commit。どの印も偽らず US と RS を持つだけの message は解けて、その byte を含む。
    #[test]
    fn publish_texts_forged_marks_are_unresolved_and_plain_separator_bytes_are_kept() {
        let (old, new) = ("a".repeat(40), "b".repeat(40));
        let shas = [new.clone(), old.clone()];
        let forged = [&[0x1e_u8][..], old.as_bytes(), &[0x1f]].concat();
        let pair = [frame(&new, b"n\x1fc", &[b"m ".as_slice(), &forged].concat(), b"\n"), frame(&old, b"n\x1fc", b"old", b"\n")].concat();
        assert_eq!(commit_bodies(&pair, &shas), None, "新しい commit の message が古い commit の頭の印を持つ");
        let twice = [frame(&new, b"n\x1fx\x1fc", b"m", b"\n"), frame(&old, b"n\x1fc", b"old", b"\n")].concat();
        assert_eq!(commit_bodies(&twice, &shas), None, "名の欄の US が 2 つ");
        let mixed = [frame(&new, b"n\x1fbad\xff", b"m", b"\n"), frame(&old, b"n\x1fc", b"old", b"\n")].concat();
        assert_eq!(commit_bodies(&mixed, &shas), None, "UTF-8 でない committer の名");
        let plain = [frame(&new, b"n\x1fc", b"m \x1f \x1e end", b"\n"), frame(&old, b"n\x1fc", b"old", b"\n")].concat();
        let bodies = commit_bodies(&plain, &shas).unwrap_or_default();
        let first = bodies.first().map(|found: &Body| found.body.as_str());
        assert_eq!(first, Some("m \u{1f} \u{1e} end"), "どの印も偽らない message は解けてその byte を含む");
        assert_eq!(bodies.len(), 6);
        assert_eq!(commit_bodies(&[plain.as_slice(), b"x"].concat(), &shas).map(|found| found.len()), Some(6), "後ろの byte は最後の patch");
        assert_eq!(commit_bodies(&plain[1..], &shas), None, "先頭が頭の印でない");
    }

    /// 行 n3 (h) `mkfifo` で作った FIFO を `--body-file` に名指す gh の segment は開かれず（書き手の無い FIFO は開くだけで止まる）、
    /// 数秒の内に unresolved:text で返る。
    #[test]
    fn publish_texts_fifo_body_file_returns_unresolved_without_hanging() {
        let dir = place("fifo");
        let fifo = dir.join("body.fifo");
        let made = Invocation::new("mkfifo").arg(&fifo).status().unwrap_or_else(|why| panic!("mkfifo を撃てる: {why}"));
        assert!(made.success(), "FIFO を作れる");
        let gh = Published {
            sort: Sort::Gh,
            group: Some("pr".to_owned()),
            verb: Some("create".to_owned()),
            rest: vec!["--body-file".to_owned(), fifo.display().to_string()],
            ..pushing(&dir)
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(std::thread::spawn(move || {
            let _ = sender.send(of_gh(&gh, &mut Budget { left: 1 << 20 }));
        }));
        let got = receiver.recv_timeout(Duration::from_secs(5));
        assert_eq!(got, Ok(Err(Halt::Gap(Gap::Text))), "FIFO は開かずに解けない");
        let _ = std::fs::remove_dir_all(dir);
    }
}
