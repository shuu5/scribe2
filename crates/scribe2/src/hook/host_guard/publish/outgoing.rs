//! publish の配線の段の入口と git push の行き先の解き（設計 docs/design/vessel-hook.md §22 行 n2・ADR-0078 (3)・SRS FR80 / NFR5）。
//!
//! git push の行き先・server の先端・出ていく commit は自前の refspec の読みでなく、git に同じ語の `push --dry-run --porcelain`
//! と `ls-remote` と `cat-file` と `rev-list` で答えさせる（[`solve`]）。段の入口（[`stage`]）は公開の segment を command 行の順に
//! 解き、解けない周を unresolved:target・締め切りの越えを deadline で断る部品を [`Denial`] で返す。子は全て
//! [`probe::run`] で撃ち、締め切りの時刻は [`stage`] が 1 度だけ決める。字面の読みは行 n3、gh の対象と可視性の 1 回の問い（行 n4・
//! [`visibility`]）の後に、走査の段（行 n5・[`check`]）が名の当たりと読む上限の越えを断る。

use super::material::check;
use super::probe::{budget, run, Bound, Ran, Stop, DEADLINE_ROW};
use super::texts::{of_gh, of_push, Budget, Texts};
use super::visibility::{gh_target, repo_of, sight, Sighted};
use super::{Marked, Published, Reason, Sort};
use crate::hook::host_guard::{Scene, PUBLISH_ROW};
use crate::rules::manifest::Manifest;
use std::path::Path;
use std::time::{Duration, Instant};

/// 上限の行が無い周の経路（行 id ごと・行を置く文）。
const NO_DEADLINE: &str = "その行を裁定を添えて置く（器の manifest の host_guard.publish_deadline_ms）";
/// 読む上限の行が無い周の経路。
const NO_READ: &str = "その行を裁定を添えて置く（器の manifest の host_guard.publish_read_bytes）";
/// remote の設定の key の頭。
const REMOTE: &str = "remote.";
/// 値を次の語に取る push の flag。
const VALUED: [&str; 4] = ["-o", "--push-option", "--receive-pack", "--exec"];

/// 解けない語（閉じた 3 値・宣言順・hit の `unresolved:` の後ろに付く・§17 の印と形の語と重ならない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    /// push の行き先・server の先端・出ていく commit が git に解けない。
    Target,
    /// 出ていく字面が読めない（行 n3）。
    Text,
    /// 隣の材料が読めない（行 n6）。
    Neighbor,
}

/// [`Gap`] の全 variant（宣言順）。
pub const GAPS: &[Gap] = &[Gap::Target, Gap::Text, Gap::Neighbor];

impl Gap {
    /// hit の `unresolved:` の後ろの語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Text => "text",
            Self::Neighbor => "neighbor",
        }
    }
}

/// 解きの止まり（解けない語か締め切りの越え）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    /// 解けない。
    Gap(Gap),
    /// 締め切りを越えた。
    Deadline,
}

impl From<Stop> for Halt {
    fn from(stop: Stop) -> Self {
        match stop {
            Stop::Spawn => Self::Gap(Gap::Target),
            Stop::Deadline => Self::Deadline,
        }
    }
}

/// 出ていく ref 1 つ（削除は `from` と `kind` が空・`to` の名だけが出ていく）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// from の sha（解く前は from の名）。
    pub from: String,
    /// from の型（commit / tag / …）。
    pub kind: String,
    /// 押す先の ref の名。
    pub to: String,
    /// 変更前の server の sha（server に無い ref は `None`）。
    pub old: Option<String>,
}

/// git push の segment 1 つの解き（行 n3〜n6 が読む）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Push {
    /// push の URL の列（git が pushurl と書き換えを当てた後）。
    pub urls: Vec<String>,
    /// server の先端（ref の名・sha・URL の順に足す）。
    pub tips: Vec<(String, String)>,
    /// 出ていく ref。
    pub refs: Vec<Ref>,
    /// 出ていく commit（sha）。
    pub commits: Vec<String>,
    /// 出ていく commit が読む上限を越えたか。
    pub over: bool,
}

/// porcelain の `To` の塊 1 つ（URL と ref の行の flag・from・to）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    /// `To` の URL。
    url: String,
    /// ref の行。
    refs: Vec<(char, String, String)>,
}

/// git を 1 回撃つ場（大域の語は動詞の前に置く）。
struct Git<'a> {
    /// program。
    program: &'a Path,
    /// 大域の語。
    globals: &'a [String],
    /// 撃つ dir。
    dir: &'a Path,
    /// 締め切りと読む上限。
    bound: Bound,
}

impl Git<'_> {
    /// 撃つ（rc と越えは呼び手が見る）。
    fn call(&self, args: &[&str], input: &str) -> Result<Ran, Halt> {
        let all: Vec<&str> = self.globals.iter().map(String::as_str).chain(args.iter().copied()).collect();
        Ok(run(self.program, &all, self.dir, input.as_bytes(), self.bound)?)
    }

    /// 撃って標準出力の字を返す（rc 非 0 と越えは解けない）。
    fn ok(&self, args: &[&str], input: &str) -> Result<String, Halt> {
        let ran = self.call(args, input)?;
        if ran.code != 0 || ran.over {
            return Err(Halt::Gap(Gap::Target));
        }
        Ok(text(&ran))
    }
}

/// 標準出力の字。
fn text(ran: &Ran) -> String {
    String::from_utf8_lossy(&ran.bytes).into_owned()
}

/// git の segment の語（先頭は git）の動詞より前の語から `-C <dir>` の対を落とした列（`-c k=v` など・[`Published::globals`]）。
pub fn globals(words: &[String]) -> Vec<String> {
    let (mut kept, mut rest) = (Vec::new(), words.iter().skip(1));
    while let Some(word) = rest.next() {
        match word.as_str() {
            "-C" => {
                rest.next();
            }
            "-c" | "--namespace" | "--git-dir" | "--work-tree" => {
                kept.push(word.clone());
                kept.extend(rest.next().cloned());
            }
            flag if flag.starts_with('-') => kept.push(word.clone()),
            _ => break,
        }
    }
    kept
}

/// push の後ろの語から `-q` / `--quiet` と shell の redirect（`2>` と、演算子だけの語ならその先）を落とした列。
fn cleaned(rest: &[String]) -> Vec<String> {
    let (mut kept, mut words) = (Vec::new(), rest.iter());
    while let Some(word) = words.next() {
        let operator = word.trim_start_matches(|found: char| found.is_ascii_digit());
        if operator.starts_with(['<', '>']) {
            if operator.trim_start_matches(['<', '>']).is_empty() {
                words.next();
            }
        } else if word != "-q" && word != "--quiet" {
            kept.push(word.clone());
        }
    }
    kept
}

/// 相手の語（最初の flag でない語か `--repo` の値・値を取る flag の値は読み飛ばす）。
fn target_word(words: &[String]) -> Option<&str> {
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        if let Some(value) = word.strip_prefix("--repo=") {
            return Some(value);
        }
        if word == "--repo" {
            return rest.next().map(String::as_str);
        }
        if VALUED.contains(&word.as_str()) {
            rest.next();
        } else if !(word.starts_with('-') && word.len() > 1) {
            return Some(word);
        }
    }
    None
}

/// `config --list -z` の出力を（key・値）の列にする。
pub(super) fn pairs(text: &str) -> Vec<(String, String)> {
    text.split('\0').filter(|entry| !entry.is_empty()).map(|entry| entry.split_once('\n').unwrap_or((entry, ""))).map(|(key, value)| (key.to_owned(), value.to_owned())).collect()
}

/// 設定に在る remote の名（`remote.<名>.<変数>` の名・続く重複は畳む）。
fn remotes(config: &[(String, String)]) -> Vec<&str> {
    let mut names: Vec<&str> = config.iter().filter_map(|(key, _)| key.strip_prefix(REMOTE)?.rsplit_once('.').map(|(name, _)| name)).collect();
    names.dedup();
    names
}

/// 語の無い push の remote の名（**純関数**）: branch の pushRemote → remote.pushDefault → branch の remote → origin。
fn default_remote(config: &[(String, String)], branch: Option<&str>) -> String {
    let get = |key: String| config.iter().rev().find(|(found, _)| *found == key).map(|(_, value)| value.clone());
    let of = |suffix: &str| branch.and_then(|name| get(format!("branch.{name}.{suffix}")));
    of("pushremote").or_else(|| get("remote.pushdefault".to_owned())).or_else(|| of("remote")).unwrap_or_else(|| "origin".to_owned())
}

/// git が `To` に出す URL（利用者の部分を落とした形・git の transport_anonymize_url と同じ規則の 1 関数）。
pub fn anonymized(url: &str) -> String {
    let Some((before, anon)) = url.split_once('@') else {
        return url.to_owned();
    };
    let colon = url.find(':');
    let local = colon.is_none() || url.find('/').zip(colon).is_some_and(|(slash, colon)| slash < colon);
    if local {
        return url.to_owned();
    }
    let Some(scheme) = url.find("://") else {
        return if anon.contains(':') { anon.to_owned() } else { url.to_owned() };
    };
    let valid = url.get(..scheme).is_some_and(|head| head.chars().all(|found| found.is_ascii_alphanumeric() || "+.-".contains(found)));
    let past = url.get(scheme.saturating_add(3)..).and_then(|tail| tail.find('/')).is_some_and(|slash| scheme.saturating_add(3).saturating_add(slash) < before.len());
    match (valid && !past, url.get(..scheme.saturating_add(3))) {
        (true, Some(prefix)) => format!("{prefix}{anon}"),
        _ => url.to_owned(),
    }
}

/// push の URL の列と porcelain の `To` の塊が順に同じ URL を指すか（利用者の部分を落として比べる・数が違うか 0 は違う）。
fn same_target(urls: &[String], blocks: &[Block]) -> bool {
    !blocks.is_empty() && urls.len() == blocks.len() && urls.iter().zip(blocks).all(|(url, block)| anonymized(url) == block.url)
}

/// porcelain の出力を `To` の塊に読む（ref の行は `<flag>\t<from>:<to>\t<要約>`・`To` の前の行と `Done` は捨てる）。
fn porcelain(output: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for line in output.lines() {
        if let Some(url) = line.strip_prefix("To ") {
            blocks.push(Block { url: url.to_owned(), refs: Vec::new() });
            continue;
        }
        let mut parts = line.split('\t');
        let (flag, names) = (parts.next().unwrap_or_default(), parts.next().and_then(|names| names.split_once(':')));
        let mut chars = flag.chars();
        if let (Some(flag), None, Some((from, to)), Some(block)) = (chars.next(), chars.next(), names, blocks.last_mut()) {
            block.refs.push((flag, from.to_owned(), to.to_owned()));
        }
    }
    blocks
}

/// 出ていく ref（flag が空白 / `+` / `*` の行は from も出る・`-` は to の名だけ・`=` と `!` は出ない）。変更前は server の先端の名から引く。
fn outgoing(blocks: &[Block], tips: &[(String, String)]) -> Vec<Ref> {
    let old = |to: &str| tips.iter().find(|(name, _)| name == to).map(|(_, sha)| sha.clone());
    let refs = blocks.iter().flat_map(|block| block.refs.iter());
    refs.filter_map(|(flag, from, to)| match flag {
        ' ' | '+' | '*' => Some(Ref { from: from.clone(), kind: String::new(), to: to.clone(), old: old(to) }),
        '-' => Some(Ref { from: String::new(), kind: String::new(), to: to.clone(), old: old(to) }),
        _ => None,
    })
    .collect()
}

/// server の先端（`ls-remote` の `<sha>\t<名>` の行）。
fn server_tips(output: &str) -> Vec<(String, String)> {
    output.lines().filter_map(|line| line.split_once('\t')).map(|(sha, name)| (name.to_owned(), sha.to_owned())).collect()
}

/// push の URL の列: 相手が設定に在る remote（語が無ければ [`default_remote`]）なら `remote get-url --push --all`、URL の語なら `ls-remote --get-url`。
fn urls_of(git: &Git, config: &[(String, String)], word: Option<&str>) -> Result<Vec<String>, Halt> {
    let name = match word {
        Some(word) => word.to_owned(),
        None => {
            let ran = git.call(&["symbolic-ref", "-q", "--short", "HEAD"], "")?;
            let branch = (ran.code == 0).then(|| text(&ran).trim().to_owned()).filter(|name| !name.is_empty());
            default_remote(config, branch.as_deref())
        }
    };
    let args: Vec<&str> = if remotes(config).contains(&name.as_str()) {
        vec!["remote", "get-url", "--push", "--all", &name]
    } else {
        vec!["ls-remote", "--get-url", &name]
    };
    Ok(git.ok(&args, "")?.lines().map(str::to_owned).collect())
}

/// from の名を sha と型に解く（`cat-file --batch-check` 1 回・missing と ambiguous は解けない）。
fn resolve(git: &Git, refs: &mut [Ref]) -> Result<(), Halt> {
    let names: Vec<&str> = refs.iter().filter(|found| !found.from.is_empty()).map(|found| found.from.as_str()).collect();
    if names.is_empty() {
        return Ok(());
    }
    let output = git.ok(&["cat-file", "--batch-check"], &format!("{}\n", names.join("\n")))?;
    let mut lines = output.lines().map(|line| line.split_whitespace().collect::<Vec<&str>>());
    for found in refs.iter_mut().filter(|found| !found.from.is_empty()) {
        let line = lines.next().unwrap_or_default();
        let [sha, kind, _size] = line.as_slice() else {
            return Err(Halt::Gap(Gap::Target));
        };
        (found.from, found.kind) = ((*sha).to_owned(), (*kind).to_owned());
    }
    Ok(())
}

/// 出ていく commit（`rev-list --ignore-missing --stdin` 1 回・出ていく from の sha と `^<server の先端の sha>`）と越えの印。
fn commits_of(git: &Git, refs: &[Ref], tips: &[(String, String)]) -> Result<(Vec<String>, bool), Halt> {
    if !refs.iter().any(|found| !found.from.is_empty()) {
        return Ok((Vec::new(), false));
    }
    let tops = refs.iter().filter(|found| !found.from.is_empty()).map(|found| found.from.clone());
    let input: String = tops.chain(tips.iter().map(|(_, sha)| format!("^{sha}"))).map(|line| format!("{line}\n")).collect();
    let ran = git.call(&["rev-list", "--ignore-missing", "--stdin"], &input)?;
    if ran.code != 0 && !ran.over {
        return Err(Halt::Gap(Gap::Target));
    }
    let output = text(&ran);
    let whole = if ran.over { output.rsplit_once('\n').map_or("", |(kept, _)| kept) } else { output.as_str() };
    Ok((whole.lines().map(str::to_owned).collect(), ran.over))
}

/// git push の segment 1 つの行き先と server の先端と出ていく commit を git に解かせる（**入口**・着地の push の行も `git push <remote>
/// main:main` の語の [`Published`] と dir で呼ぶ）。解けない周は [`Gap::Target`]・締め切りの越えは [`Halt::Deadline`]。
pub fn solve(found: &Published, dir: &Path, program: &Path, bound: Bound) -> Result<Push, Halt> {
    let git = Git { program, globals: &found.globals, dir, bound };
    let words = cleaned(&found.rest);
    let config = pairs(&git.ok(&["config", "--list", "-z"], "")?);
    let urls = urls_of(&git, &config, target_word(&words))?;
    let mut args = vec!["push", "--dry-run", "--porcelain", "--no-verify"];
    args.extend(words.iter().map(String::as_str));
    let blocks = porcelain(&git.ok(&args, "")?);
    if !same_target(&urls, &blocks) {
        return Err(Halt::Gap(Gap::Target));
    }
    let mut tips = Vec::new();
    for url in &urls {
        tips.extend(server_tips(&git.ok(&["ls-remote", url, "HEAD", "refs/heads/*", "refs/tags/*"], "")?));
    }
    let mut refs = outgoing(&blocks, &tips);
    resolve(&git, &mut refs)?;
    let (commits, over) = commits_of(&git, &refs, &tips)?;
    Ok(Push { urls, tips, refs, commits, over })
}

/// 段の断り 1 件（publish.rs の断りへ渡す部品）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Denial {
    /// 理由。
    pub reason: Reason,
    /// hit の理由の後ろの語（identifier は `<件数>:<先頭 5 件>`）。
    pub word: String,
    /// 行 id。
    pub row: &'static str,
    /// 裁定 id。
    pub ruling: String,
    /// 経路の差し替え（上限の行が無い周だけ）。
    pub route: Option<&'static str>,
}

/// 段の入口（**1 関数**・設計 §22 行 n2 形 3）: 上限の 2 行を読み（読めない周は no-row:<行 id>）、締め切りの時刻を 1 度だけ決めて公開の
/// segment を command 行の順に解き、字面を読む（[`walk`]）。解けない git push で unresolved:target、読めない字面で unresolved:text、
/// 締め切りの越えで deadline。解けた周は `None`。
pub(super) fn stage(found: &[Marked], manifest: &Manifest, scene: &Scene, ruling: &str) -> Option<Denial> {
    let (deadline, limit) = match budget(manifest) {
        Ok(read) => read,
        Err(id) => {
            let route = if id == DEADLINE_ROW { NO_DEADLINE } else { NO_READ };
            return Some(Denial { reason: Reason::NoRow, word: id.to_owned(), row: id, ruling: "-".to_owned(), route: Some(route) });
        }
    };
    let deadline_ruling = manifest.get(DEADLINE_ROW).map_or_else(|| "-".to_owned(), |row| row.ruling.clone());
    let bound = Bound { deadline: Instant::now() + Duration::from_millis(deadline), limit };
    match walk(found, &Walk { scene, bound, ruling, deadline_ruling }) {
        Ok(read) => check(&read, manifest, scene.host, ruling),
        Err(stop) => Some(stop),
    }
}

/// 段が 1 度だけ決めたもの（判定の場〔git と gh の program・host の面〕・締め切りと読む上限・断りの裁定 id）。
struct Walk<'a> {
    /// 判定の場。
    scene: &'a Scene<'a>,
    /// 締め切りと読む上限。
    bound: Bound,
    /// publish の行の裁定 id。
    ruling: &'a str,
    /// 締め切りの行の裁定 id。
    deadline_ruling: String,
}

/// segment 1 つの読み（出ていく字面と可視性の読み・走査の段が読む）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// 出ていく字面。
    pub texts: Texts,
    /// 可視性の読み。
    pub sighted: Sighted,
    /// 対象の owner/name（git は push の URL ごと・gh は 1 つ・導けないものは `None`）。
    pub own: Vec<Option<String>>,
}

/// 解きの止まりを段の断りにする。
fn denial(halt: Halt, plan: &Walk) -> Denial {
    match halt {
        Halt::Gap(gap) => Denial { reason: Reason::Unresolved, word: gap.as_str().to_owned(), row: PUBLISH_ROW, ruling: plan.ruling.to_owned(), route: None },
        Halt::Deadline => Denial { reason: Reason::Deadline, word: DEADLINE_ROW.to_owned(), row: DEADLINE_ROW, ruling: plan.deadline_ruling.clone(), route: None },
    }
}

/// 公開の segment 1 つを解いて字面を読み、対象の owner/name（git は push の URL ごと・gh は 1 つ）を導く（設計 §22 行 n3 形 6・行 n4 形 3〜4）:
/// git push は行き先の解きの後に字面を読み（解けない行き先の周は log を撃たない）、gh は本文と本文の file を読んだ後に対象を導く。
fn segment(seg: &Published, plan: &Walk, budget: &mut Budget) -> Result<(Texts, Vec<Option<String>>), Halt> {
    let (program, bound) = (plan.scene.git, plan.bound);
    match seg.sort {
        Sort::Git => {
            let push = solve(seg, &seg.dir, program, bound)?;
            Ok((of_push(seg, &push, program, bound, budget)?, push.urls.iter().map(|url| repo_of(url)).collect()))
        }
        Sort::Gh | Sort::Api => Ok((of_gh(seg, budget)?, vec![gh_target(seg, plan.scene.gh, bound)?])),
    }
}

/// 公開の segment を command 行の順に解いて字面を読み、最後に可視性を 1 回問う（設計 §22 行 n4）。字面は出ていく字面の合計を 1 つの計数で数える。
fn walk(found: &[Marked], plan: &Walk) -> Result<Vec<Found>, Denial> {
    let mut budget = Budget { left: plan.bound.limit };
    let (mut texts, mut targets) = (Vec::new(), Vec::new());
    for seg in found.iter().filter_map(|seg| seg.read.as_ref()) {
        let (read, target) = segment(seg, plan, &mut budget).map_err(|halt| denial(halt, plan))?;
        texts.push(read);
        targets.push(target);
    }
    let sighted = sight(&targets, plan.scene, plan.bound).map_err(|halt| denial(halt, plan))?;
    Ok(texts.into_iter().zip(sighted).zip(targets).map(|((texts, sighted), own)| Found { texts, sighted, own }).collect())
}

#[cfg(test)]
mod tests {
    use super::{anonymized, default_remote, outgoing, porcelain, same_target, solve, Gap, Halt, Ref, Walk, GAPS};
    use crate::hook::host_guard::Scene;
    use crate::rules::manifest::Manifest;
    use crate::hook::host_guard::publish::probe::Bound;
    use crate::hook::host_guard::publish::{Hole, Mark, Published};
    use crate::invocation::Invocation;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// git を 1 回撃ち rc 0 を要求して標準出力を返す。
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Invocation::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@e.invalid"]).args(args).output();
        let out = out.unwrap_or_else(|why| panic!("git を撃てる: {why}"));
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// 歯ごとの置き場（origin・other・work の 3 つ・work の main は origin に 1 commit を出した後で 2 commit 進む）。
    fn place(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("scribe2-publish-push-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for bare in ["origin.git", "other.git"] {
            git(&std::env::temp_dir(), &["init", "-q", "--bare", "-b", "main", &root.join(bare).display().to_string()]);
        }
        git(&std::env::temp_dir(), &["init", "-q", "-b", "main", &root.join("work").display().to_string()]);
        let work = root.join("work");
        git(&work, &["commit", "-q", "--allow-empty", "-m", "base"]);
        git(&work, &["remote", "add", "origin", &root.join("origin.git").display().to_string()]);
        git(&work, &["push", "-q", "origin", "main"]);
        work
    }

    /// 相手の語を持つ git push の読み。
    fn push(words: &[&str]) -> Published {
        Published { rest: words.iter().map(|word| (*word).to_owned()).collect(), ..Published::default() }
    }

    /// 解く（締め切りは 20 秒後・読む上限は 1 MB）。
    fn solved(found: &Published, work: &Path) -> Result<super::Push, Halt> {
        solve(found, work, Path::new("git"), Bound { deadline: Instant::now() + Duration::from_secs(20), limit: 1 << 20 })
    }

    /// 行 n2 (a) porcelain の 2 つの `To` の塊と flag ごとの出ていく ref: 空白・`+`・`*` は from も出て、`-` は to の名だけ、`=` と `!`
    /// は出ず、server の先端から変更前の sha を引く。`To` が無い出力は塊が 0 で解けない側。
    #[test]
    fn publish_push_porcelain_blocks_and_flags_name_the_outgoing_refs() {
        let text = "To /o.git\n \trefs/heads/a:refs/heads/a\t1..2\n+\trefs/heads/b:refs/heads/b\t1...2 (forced update)\n*\trefs/heads/c:refs/heads/c\t[new branch]\n\
                    -\t:refs/heads/d\t[deleted]\n=\trefs/heads/e:refs/heads/e\t[up to date]\n!\trefs/heads/f:refs/heads/f\t[rejected] (non-fast-forward)\n\
                    To h:o/p.git\n*\trefs/tags/t:refs/tags/t\t[new tag]\nDone\n";
        let blocks = porcelain(text);
        assert_eq!(blocks.iter().map(|block| block.url.as_str()).collect::<Vec<_>>(), ["/o.git", "h:o/p.git"]);
        let tips = [("refs/heads/b".to_owned(), "b1".to_owned()), ("refs/heads/d".to_owned(), "d1".to_owned())];
        let refs = outgoing(&blocks, &tips);
        let pairs: Vec<(&str, &str, Option<&str>)> = refs.iter().map(|found| (found.from.as_str(), found.to.as_str(), found.old.as_deref())).collect();
        let want = [
            ("refs/heads/a", "refs/heads/a", None), ("refs/heads/b", "refs/heads/b", Some("b1")), ("refs/heads/c", "refs/heads/c", None),
            ("", "refs/heads/d", Some("d1")), ("refs/tags/t", "refs/tags/t", None),
        ];
        assert_eq!(pairs, want);
        assert!(porcelain("fatal: no remote\n").is_empty() && !same_target(&["/o.git".to_owned()], &porcelain("Done\n")), "`To` の無い出力は解けない");
        assert_eq!(GAPS.iter().map(|gap| gap.as_str()).collect::<Vec<_>>(), ["target", "text", "neighbor"]);
        let words: Vec<&str> = super::super::MARKS.iter().map(|mark: &Mark| mark.as_str()).chain(super::super::HOLES.iter().map(|hole: &Hole| hole.as_str())).collect();
        assert!(GAPS.iter().all(|gap| !words.contains(&gap.as_str())), "§17 の語と重ならない");
    }

    /// 行 n2 (b) 利用者の部分を落とした照合: scp 形と `ssh://` の形と `user:pass@` は git が `To` に出す形に落ち、local の path・`@` が
    /// 最初の `/` の後ろの URL・`:` の無い `@` は落とさず、違う URL は解けない。
    #[test]
    fn publish_push_target_match_drops_only_the_user_part() {
        for (url, want) in [
            ("git@github.com:o/p.git", "github.com:o/p.git"), ("ssh://git@host/o/p.git", "ssh://host/o/p.git"), ("https://u:pw@host/o/p.git", "https://host/o/p.git"),
            ("/tmp/a@b/x.git", "/tmp/a@b/x.git"), ("https://host/a@b", "https://host/a@b"), ("me@there/path", "me@there/path"), ("host:path", "host:path"),
        ] {
            assert_eq!(anonymized(url), want, "{url}");
        }
        let blocks = porcelain("To github.com:o/p.git\n*\trefs/heads/a:refs/heads/a\t[new branch]\n");
        assert!(same_target(&["git@github.com:o/p.git".to_owned()], &blocks));
        assert!(!same_target(&["git@github.com:o/q.git".to_owned()], &blocks), "違う URL は解けない");
        assert!(!same_target(&["git@github.com:o/p.git".to_owned(), "x".to_owned()], &blocks), "数が違う");
    }

    /// 行 n2 (c) 実の git: bare な origin に 1 commit・手元に 2 commit で `git push origin main` の出ていく commit は 2 つ（変更前は
    /// origin の先端）、server に在る commit を指す新しい branch は 0。
    #[test]
    fn publish_push_outgoing_commits_are_the_ones_the_server_lacks() {
        let work = place("commits");
        let base = git(&work, &["rev-parse", "HEAD"]);
        git(&work, &["branch", "feat", &base]);
        for name in ["one", "two"] {
            git(&work, &["commit", "-q", "--allow-empty", "-m", name]);
        }
        let head = git(&work, &["rev-parse", "HEAD"]);
        let pushed = solved(&push(&["origin", "main"]), &work).unwrap_or_else(|why| panic!("解ける: {why:?}"));
        assert_eq!(pushed.commits.len(), 2, "{:?}", pushed.commits);
        assert!(pushed.commits.contains(&head) && !pushed.commits.contains(&base) && !pushed.over);
        let want = Ref { from: head, kind: "commit".to_owned(), to: "refs/heads/main".to_owned(), old: Some(base.clone()) };
        assert_eq!(pushed.refs, [want]);
        assert!(pushed.tips.contains(&("refs/heads/main".to_owned(), base)), "server の先端: {:?}", pushed.tips);
        let feat = solved(&push(&["-q", "origin", "feat"]), &work).unwrap_or_else(|why| panic!("解ける: {why:?}"));
        assert_eq!((feat.commits.len(), feat.refs.len(), feat.refs.first().and_then(|found| found.old.clone())), (0, 1, None), "新しい branch は server に在る commit だけ");
        assert_eq!(solved(&push(&["nowhere", "main"]), &work), Err(Halt::Gap(Gap::Target)), "無い remote は解けない");
        let _ = std::fs::remove_dir_all(work.parent().unwrap_or(&work));
    }

    /// 行 n3 (e) 段の入口は行き先の解きの後に字面を読む: 呼出しを file に数える偽の git で、解けない行き先の周は `log` を撃たず（unresolved:target）、
    /// 解けた周は `log` を 1 回撃って commit の message を本文に持つ。
    #[test]
    fn publish_texts_stage_reads_the_text_only_after_the_target_resolves() {
        use super::{walk, Denial};
        use crate::hook::host_guard::publish::scan::Source;
        use crate::hook::host_guard::publish::{Marked, Reason};
        let work = place("texts-stage");
        git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let root = work.parent().unwrap_or(&work).to_path_buf();
        let calls = root.join("calls.log");
        let (deny, wrap) = (script(&root, "deny", &calls, "exit 1"), script(&root, "wrap", &calls, "exec git \"$@\""));
        let found = [Marked { read: Some(Published { dir: work.clone(), resolved: true, ..push(&["origin", "main"]) }), marks: Vec::new(), holes: Vec::new() }];
        let seen = || std::fs::read_to_string(&calls).unwrap_or_default();
        let denied = walk(&found, &plan(&scene(&work, &deny, Path::new("gh"), &Manifest::default())));
        let want = Denial { reason: Reason::Unresolved, word: "target".to_owned(), row: "host_guard.publish", ruling: "r".to_owned(), route: None };
        assert_eq!(denied.err(), Some(want), "解けない行き先");
        assert!(!seen().is_empty() && !seen().contains("log --no-walk"), "log を撃たない: {}", seen());
        let _ = std::fs::remove_file(&calls);
        let texts = walk(&found, &plan(&scene(&work, &wrap, Path::new("gh"), &Manifest::default()))).unwrap_or_else(|why| panic!("解ける: {why:?}"));
        assert_eq!(seen().lines().filter(|line| line.contains("log --no-walk=unsorted")).count(), 1, "log は 1 回: {}", seen());
        let messages: Vec<&str> = texts.iter().flat_map(|found| found.texts.bodies.iter()).filter(|body| body.source == Source::CommitMessage).map(|body| body.body.trim()).collect();
        assert_eq!(messages, ["one"], "解けた周は字面を読む");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 呼出しを file に 1 行ずつ数え、`tail` を走らせる偽の program（実行可能な script）。
    fn script(root: &Path, name: &str, calls: &Path, tail: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\necho \"$@\" >> {}\n{tail}\n", calls.display())).unwrap_or_else(|why| panic!("偽の program を書ける: {why}"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap_or_else(|why| panic!("偽の program を実行可能にできる: {why}"));
        path
    }

    /// 判定の場（cwd と記録の置き場は work）。
    fn scene<'a>(work: &'a Path, git: &'a Path, gh: &'a Path, host: &'a Manifest) -> Scene<'a> {
        Scene { cwd: work, state_dir: work, git, gh, host }
    }

    /// 段の計画（締め切りは 20 秒後・読む上限は 1 MB）。
    fn plan<'a>(scene: &'a Scene<'a>) -> Walk<'a> {
        Walk { scene, bound: Bound { deadline: Instant::now() + Duration::from_secs(20), limit: 1 << 20 }, ruling: "r", deadline_ruling: "d".to_owned() }
    }

    /// 行 n4 (d) Scene の gh に呼出しを数える偽の gh で、github.com の origin への push（偽の ssh が tmp の bare へ届ける）の gh の呼出し: 群の無い面は
    /// 0 回・対象の外の anchor が在る面は 1 回（隣は private と答えた anchor）・anchor が対象だけの面は 0 回・対象が private の push は通す。
    #[test]
    fn publish_visibility_asks_once_only_when_an_anchor_lies_outside_the_target() {
        use super::{walk, Sighted};
        use crate::hook::host_guard::publish::visibility::Anchor;
        use crate::hook::host_guard::publish::Marked;
        use crate::rules::manifest::HostManifest;
        let work = place("visibility");
        git(&work, &["commit", "-q", "--allow-empty", "-m", "one"]);
        let root = work.parent().unwrap_or(&work).to_path_buf();
        let serve = format!("for last; do :; done\nservice=${{last%% *}}\nexec git \"${{service#git-}}\" {}", root.join("origin.git").display());
        let ssh = script(&root, "ssh", &root.join("ssh.log"), &serve);
        git(&work, &["config", "remote.origin.url", "git@github.com:acme/pub.git"]);
        git(&work, &["config", "core.sshCommand", &ssh.display().to_string()]);
        let anchor = |name: &str, url: &str| {
            let dir = root.join(name);
            git(&root, &["init", "-q", &dir.display().to_string()]);
            git(&dir, &["config", "remote.origin.url", url]);
            dir
        };
        let (mine, other) = (anchor("a-pub", "git@github.com:acme/pub.git"), anchor("a-other", "git@github.com:acme/other.git"));
        let face = |dirs: &[&Path]| {
            let list = dirs.iter().map(|dir| format!("\"{}\"", dir.display())).collect::<Vec<_>>().join(", ");
            std::fs::write(root.join("host.toml"), format!("schema = 1\n\n[[account]]\nlabel = \"x\"\n\n[[account-group]]\nname = \"Tier1\"\nanchors = [{list}]\naccounts = [\"x\"]\n")).unwrap_or_else(|why| panic!("面を書ける: {why}"));
            match HostManifest::read(&root.join("host.toml")) {
                HostManifest::Present(face) => face,
                other => panic!("面を読める: {other:?}"),
            }
        };
        let found = [Marked { read: Some(Published { dir: work.clone(), resolved: true, ..push(&["origin", "main"]) }), marks: Vec::new(), holes: Vec::new() }];
        let calls = root.join("gh.log");
        let count = || std::fs::read_to_string(&calls).unwrap_or_default().lines().count();
        let sighted = |host: &Manifest, answer: &str| {
            let _ = std::fs::remove_file(&calls);
            let gh = script(&root, "gh", &calls, &format!("echo '{answer}'"));
            let read = walk(&found, &plan(&scene(&work, Path::new("git"), &gh, host))).unwrap_or_else(|why| panic!("通す: {why:?}"));
            read.into_iter().map(|found| found.sighted).collect::<Vec<Sighted>>()
        };
        let answer = r#"{"data":{"r0":{"visibility":"PUBLIC"},"r1":{"visibility":"PRIVATE"}}}"#;
        assert_eq!((sighted(&Manifest::default(), answer), count()), (vec![Sighted::default()], 0), "群の無い面");
        let neighbor = Anchor { label: "a-other".to_owned(), repo: Some("acme/other".to_owned()) };
        assert_eq!((sighted(&face(&[&other]), answer), count()), (vec![Sighted { neighbors: vec![neighbor], ..Sighted::default() }], 1), "対象の外の anchor");
        assert_eq!((sighted(&face(&[&mine]), answer), count()), (vec![Sighted::default()], 0), "anchor が対象だけ");
        let private = r#"{"data":{"r0":{"visibility":"PRIVATE"},"r1":{"visibility":"PRIVATE"}}}"#;
        assert_eq!((sighted(&face(&[&mine, &other]), private), count()), (vec![Sighted { private: true, ..Sighted::default() }], 1), "対象が private");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 行 n5 (c) 段の入口は可視性の後に走査を呼ぶ: github.com の origin への push（偽の ssh が tmp の bare へ届ける）で、隣の anchor（basename
    /// `proj-alpha`・acme/alpha）の name を message に持つ commit は identifier の断り（`<件数>:<先頭 5 件>`・row は publish の行・裁定 id は渡した id・
    /// 経路の差し替えなし）、名を持たない commit は通す。
    #[test]
    fn publish_scan_stage_denies_the_neighbor_name_after_the_visibility_step() {
        use super::stage;
        use crate::hook::host_guard::publish::{Marked, Reason};
        use crate::rules::manifest::HostManifest;
        let work = place("scan-stage");
        git(&work, &["commit", "-q", "--allow-empty", "-m", "see alpha"]);
        let root = work.parent().unwrap_or(&work).to_path_buf();
        let serve = format!("for last; do :; done\nservice=${{last%% *}}\nexec git \"${{service#git-}}\" {}", root.join("origin.git").display());
        let ssh = script(&root, "ssh", &root.join("ssh.log"), &serve);
        git(&work, &["config", "remote.origin.url", "git@github.com:acme/pub.git"]);
        git(&work, &["config", "core.sshCommand", &ssh.display().to_string()]);
        let anchor = root.join("proj-alpha");
        git(&root, &["init", "-q", &anchor.display().to_string()]);
        git(&anchor, &["config", "remote.origin.url", "git@github.com:acme/alpha.git"]);
        std::fs::write(root.join("host.toml"), format!("schema = 1\n\n[[account]]\nlabel = \"x\"\n\n[[account-group]]\nname = \"Tier1\"\nanchors = [\"{}\"]\naccounts = [\"x\"]\n", anchor.display()))
            .unwrap_or_else(|why| panic!("面を書ける: {why}"));
        let HostManifest::Present(face) = HostManifest::read(&root.join("host.toml")) else { panic!("面を読める") };
        let row = |id: &str, kind: &str, value: &str| format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n");
        let text = row("host_guard.publish", "HostGuardPublish", "[\"form repo-name\"]") + &row("host_guard.publish_deadline_ms", "HostGuardPublishDeadlineMs", "6000");
        let rules = Manifest::parse(&format!("schema = 1\n{text}{}", row("host_guard.publish_read_bytes", "HostGuardPublishReadBytes", "1048576")))
            .unwrap_or_else(|errors| panic!("manifest を読める: {errors:?}"));
        let gh = script(&root, "gh", &root.join("gh.log"), "echo '{\"data\":{\"r0\":{\"visibility\":\"PUBLIC\"},\"r1\":{\"visibility\":\"PRIVATE\"}}}'");
        let found = [Marked { read: Some(Published { dir: work.clone(), resolved: true, ..push(&["origin", "main"]) }), marks: Vec::new(), holes: Vec::new() }];
        let denied = stage(&found, &rules, &scene(&work, Path::new("git"), &gh, &face), "rr").unwrap_or_else(|| panic!("隣の名は断る"));
        let got = (denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str(), denied.route);
        assert_eq!(got, (Reason::Identifier, "1:repo-name=alpha@proj-alpha", "host_guard.publish", "rr", None));
        git(&work, &["commit", "-q", "--amend", "--allow-empty", "-m", "see nothing"]);
        assert_eq!(stage(&found, &rules, &scene(&work, Path::new("git"), &gh, &face), "rr"), None, "名を持たない commit は通す");
        let _ = std::fs::remove_dir_all(root);
    }

    /// 行 n2 (d) 行き先は git が決める: pushurl を持つ remote・pushInsteadOf・語の無い push の remote（branch の pushRemote → remote.pushDefault
    /// → branch の remote → origin の順）を git から読み、`-c` の大域の語は動詞の前に置かれる。
    #[test]
    fn publish_push_target_follows_pushurl_pushinsteadof_and_the_default_remote() {
        let work = place("target");
        let root = work.parent().unwrap_or(&work).to_path_buf();
        let (origin, other) = (root.join("origin.git").display().to_string(), root.join("other.git").display().to_string());
        let urls = |found: &Published| solved(found, &work).map(|pushed| pushed.urls.join(" ")).unwrap_or_else(|why| panic!("解ける: {why:?}"));
        assert_eq!(urls(&push(&["origin", "main"])), origin);
        git(&work, &["config", "remote.origin.pushurl", &other]);
        assert_eq!(urls(&push(&["origin", "main"])), other, "pushurl");
        git(&work, &["config", "--unset", "remote.origin.pushurl"]);
        git(&work, &["config", &format!("url.{other}.pushInsteadOf"), &origin]);
        assert_eq!(urls(&push(&["origin", "main"])), other, "pushInsteadOf");
        git(&work, &["config", "--unset", &format!("url.{other}.pushInsteadOf")]);
        let with_c = Published { globals: vec!["-c".to_owned(), format!("remote.origin.pushurl={other}")], ..push(&["origin", "main"]) };
        assert_eq!(urls(&with_c), other, "-c の pushurl");
        git(&work, &["config", "push.default", "current"]);
        git(&work, &["remote", "add", "second", &other]);
        assert_eq!(urls(&push(&[])), origin, "語の無い push は origin");
        git(&work, &["config", "remote.pushDefault", "second"]);
        assert_eq!(urls(&push(&[])), other, "remote.pushDefault");
        let config = |pairs: &[(&str, &str)]| pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect::<Vec<_>>();
        let all = config(&[("branch.main.remote", "up"), ("remote.pushdefault", "def"), ("branch.main.pushremote", "push")]);
        assert_eq!(default_remote(&all, Some("main")), "push");
        assert_eq!(default_remote(all.get(..2).unwrap_or_default(), Some("main")), "def");
        assert_eq!(default_remote(all.get(..1).unwrap_or_default(), Some("main")), "up");
        assert_eq!(default_remote(all.get(..1).unwrap_or_default(), Some("dev")), "origin");
        assert_eq!(default_remote(&[], None), "origin");
        let _ = std::fs::remove_dir_all(root);
    }
}
