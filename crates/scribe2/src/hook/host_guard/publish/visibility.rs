//! publish の配線の可視性（設計 docs/design/vessel-hook.md §22 行 n4・ADR-0078 (3)(4)・SRS FR80 / AC50）。
//!
//! git の push URL と gh の対象（[`gh_target`]）と host の面の群と park の区画の anchor（[`anchors`]）から github.com の owner/name を
//! 導き（[`repo_of`]）、公開先の外の anchor が在る周だけ `gh api graphql` の 1 回で可視性を問う（[`sight`]）。読めない値は対象なら
//! public・anchor なら private に倒す。隣の在る segment は走査の段（行 n5）が読む（この行では通す）。

use super::outgoing::{pairs, Halt};
use super::probe::{run, Bound, Stop};
use super::Published;
use crate::fleet::json_tree;
use crate::hook::host_guard::Scene;
use std::path::Path;

/// 導く host（大小を畳む）。
const GITHUB: &str = "github.com";
/// URL の scheme の閉じた列。
const SCHEMES: [&str; 3] = ["https", "ssh", "git"];

/// URL の（host, path）。scheme つきは authority から利用者の部分と port を落とし、scp 形は `:` の前から利用者の部分を落とす。
fn split(url: &str) -> Option<(&str, &str)> {
    let (authority, path) = match url.split_once("://") {
        Some((scheme, rest)) if SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) => rest.split_once('/').unwrap_or((rest, "")),
        Some(_) => return None,
        None => url.split_once(':').filter(|(head, _)| !head.contains('/'))?,
    };
    let host = authority.rsplit('@').next()?;
    Some((host.split_once(':').map_or(host, |(name, _)| name), path))
}

/// owner は英数字と `-`・name は英数字と `.` `_` `-` だけの周か。
fn valid(owner: &str, name: &str) -> bool {
    let (word, tail) = (|c: char| c.is_ascii_alphanumeric(), |c: char| c.is_ascii_alphanumeric() || "._-".contains(c));
    !owner.is_empty() && !name.is_empty() && owner.chars().all(|c| word(c) || c == '-') && name.chars().all(tail)
}

/// URL から github.com の owner/name（**pure**）: host が github.com で、末尾の `/` と `.git` を落とした末尾 2 成分の字が [`valid`]。
pub fn repo_of(url: &str) -> Option<String> {
    let (host, path) = split(url)?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut tail = path.rsplit('/');
    let (name, owner) = (tail.next()?, tail.next()?);
    (host.eq_ignore_ascii_case(GITHUB) && valid(owner, name)).then(|| format!("{owner}/{name}"))
}

/// gh の `[HOST/]OWNER/REPO`（HOST は github.com のときだけ）から owner/name。
fn spec(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('/').collect();
    match parts.as_slice() {
        [owner, name] => valid(owner, name).then(|| format!("{owner}/{name}")),
        [host, owner, name] if host.eq_ignore_ascii_case(GITHUB) => valid(owner, name).then(|| format!("{owner}/{name}")),
        _ => None,
    }
}

/// 名指しの対象: `-R` → 前置きの `GH_REPO=`（あれば導けるかに依らずそれが対象）、無ければ repo の群の位置引数 → api の `repos/<o>/<n>`
/// （placeholder は字の外で外れる）。名指しが無い周は `None`。
fn named(found: &Published) -> Option<Option<String>> {
    if let Some(flag) = found.repo.as_deref().or(found.gh_repo.as_deref()) {
        return Some(spec(flag));
    }
    let word = found.rest.first().filter(|word| found.group.as_deref() == Some("repo") && !word.starts_with('-'));
    let target = found.api.as_ref().and_then(|api| api.target.as_deref()).unwrap_or_default();
    let mut parts = target.split('/');
    let from_api = (parts.next() == Some("repos")).then(|| parts.next().zip(parts.next())).flatten().filter(|(owner, name)| valid(owner, name));
    word.and_then(|word| spec(word)).or_else(|| from_api.map(|(owner, name)| format!("{owner}/{name}"))).map(Some)
}

/// `gh repo view --json nameWithOwner,url` を dir で 1 回撃ち、url の host が github.com のときの nameWithOwner。失敗と読めない出力は導けない。
fn view(found: &Published, gh: &Path, bound: Bound) -> Result<Option<String>, Halt> {
    let ran = match run(gh, &["repo", "view", "--json", "nameWithOwner,url"], &found.dir, b"", bound) {
        Ok(ran) if ran.code == 0 && !ran.over => ran,
        Ok(_) | Err(Stop::Spawn) => return Ok(None),
        Err(Stop::Deadline) => return Err(Halt::Deadline),
    };
    let tree = json_tree::parse(&String::from_utf8_lossy(&ran.bytes)).ok();
    let field = |key: &str| tree.as_ref().and_then(|tree| tree.get(key)?.as_str().map(str::to_owned));
    Ok(field("url").and_then(|url| repo_of(&url)).and(field("nameWithOwner").and_then(|name| spec(&name))))
}

/// gh の segment の対象の owner/name（設計 §22 行 n4 形 3）: gist と github.com でない `GH_HOST=` / `--hostname` は gh を撃たずに導けず、
/// 名指し（[`named`]）が無い周は dir で [`view`] を 1 回撃つ。導けない周は `Ok(None)`（解けないにしない）、締め切りの越えは `Err`。
pub fn gh_target(found: &Published, gh: &Path, bound: Bound) -> Result<Option<String>, Halt> {
    let other = |host: Option<&str>| host.is_some_and(|host| !host.eq_ignore_ascii_case(GITHUB));
    let hostname = found.api.as_ref().and_then(|api| api.hostname.as_deref());
    if found.group.as_deref() == Some("gist") || other(found.gh_host.as_deref()) || other(hostname) {
        return Ok(None);
    }
    match named(found) {
        Some(target) => Ok(target),
        None => view(found, gh, bound),
    }
}

/// host の面の anchor 1 つ（名札は dir の basename・repo は push 先から導けた owner/name）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// dir の basename。
    pub label: String,
    /// 導けた owner/name（導けない anchor は `None`＝private の隣）。
    pub repo: Option<String>,
}

/// anchor の設定から push の URL: `remote.pushDefault`（無ければ origin）の pushurl（無ければ url）に insteadOf の最長一致を当てる。
fn push_url(config: &[(String, String)]) -> Option<String> {
    let get = |key: &str| config.iter().rev().find(|(found, _)| found == key).map(|(_, value)| value.as_str());
    let remote = get("remote.pushdefault").unwrap_or("origin");
    let url = get(&format!("remote.{remote}.pushurl")).or_else(|| get(&format!("remote.{remote}.url")))?;
    let rewrites = config.iter().filter(|(_, prefix)| !prefix.is_empty() && url.starts_with(prefix.as_str()));
    let best = rewrites.filter_map(|(key, prefix)| Some((key.strip_prefix("url.")?.strip_suffix(".insteadof")?, prefix))).max_by_key(|(_, prefix)| prefix.len());
    Some(best.map_or_else(|| url.to_owned(), |(base, prefix)| format!("{base}{}", url.get(prefix.len()..).unwrap_or_default())))
}

/// host の面の groups と park の anchor を宣言順に、実体の在る dir だけ（重複は畳む）。anchor ごとに `git -C <anchor> config --list -z` を
/// 1 回撃ち、push の URL から [`repo_of`] で導く（導けない anchor は `repo` が `None`）。
pub fn anchors(scene: &Scene, bound: Bound) -> Result<Vec<Anchor>, Halt> {
    let mut groups: Vec<_> = scene.host.groups().iter().chain(scene.host.park()).collect();
    groups.sort_by_key(|group| group.line());
    let mut dirs: Vec<&str> = Vec::new();
    for dir in groups.iter().flat_map(|group| group.anchors()).map(String::as_str) {
        if Path::new(dir).is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    let mut found = Vec::new();
    for dir in dirs {
        let config = match run(scene.git, &["-C", dir, "config", "--list", "-z"], Path::new(dir), b"", bound) {
            Ok(ran) if ran.code == 0 && !ran.over => pairs(&String::from_utf8_lossy(&ran.bytes)),
            Ok(_) | Err(Stop::Spawn) => Vec::new(),
            Err(Stop::Deadline) => return Err(Halt::Deadline),
        };
        let label = Path::new(dir).file_name().map_or_else(|| dir.to_owned(), |name| name.to_string_lossy().into_owned());
        found.push(Anchor { label, repo: push_url(&config).and_then(|url| repo_of(&url)) });
    }
    Ok(found)
}

/// GitHub が答えた可視性 1 つ（`PRIVATE` / `PUBLIC` / `INTERNAL` の外・null・読めない値は `Unread`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// 非公開。
    Private,
    /// 公開。
    Public,
    /// 組織内。
    Internal,
    /// 読めない（対象なら public・anchor なら private と読む）。
    Unread,
}

/// 可視性の問い（引数の repo の識別は owner/name だけ・alias は並びの順に `r0` `r1` …）。
pub fn query(repos: &[String]) -> String {
    let one = |(at, repo): (usize, &String)| repo.split_once('/').map(|(owner, name)| format!("r{at}:repository(owner:\"{owner}\",name:\"{name}\"){{visibility}}"));
    format!("query{{{}}}", repos.iter().enumerate().filter_map(one).collect::<Vec<_>>().join(" "))
}

/// 答えの stdout（rc に依らず）から alias `r0`〜の可視性を `count` 個読む（読めないものは `Unread`）。
pub fn answers(stdout: &str, count: usize) -> Vec<Seen> {
    let tree = json_tree::parse(stdout).ok();
    let seen = |at: usize| tree.as_ref().and_then(|tree| tree.get("data")?.get(&format!("r{at}"))?.get("visibility")?.as_str().map(str::to_owned));
    (0..count)
        .map(|at| match seen(at).as_deref() {
            Some("PRIVATE") => Seen::Private,
            Some("PUBLIC") => Seen::Public,
            Some("INTERNAL") => Seen::Internal,
            _ => Seen::Unread,
        })
        .collect()
}

/// owner/name の同一（GitHub は大小を畳む）。
fn same(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

/// segment ごとの可視性の読み（走査の段が読む）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sighted {
    /// 対象が全て PRIVATE と答えられたか（通す）。
    pub private: bool,
    /// 対象と別の PUBLIC でない anchor（隣）。
    pub neighbors: Vec<Anchor>,
    /// PUBLIC の anchor の name（既に公開の名）。
    pub public: Vec<String>,
}

/// anchor が segment の対象の外か（owner/name が対象と一致しない anchor・導けない anchor を含む）。
fn outside(own: &[Option<String>], anchor: &Anchor) -> bool {
    !anchor.repo.as_ref().is_some_and(|repo| own.iter().flatten().any(|target| same(target, repo)))
}

/// 可視性を問い segment ごとの読みを返す（設計 §22 行 n4 形 6〜7）: 全ての segment の対象の外の anchor が無い周は問わない。問うのは導けた
/// 対象（segment の順）と anchor（宣言順）の owner/name を重複なく並べた 1 回で、導けた owner/name が 0 の周は撃たない。
pub fn sight(targets: &[Vec<Option<String>>], scene: &Scene, bound: Bound) -> Result<Vec<Sighted>, Halt> {
    let anchors = anchors(scene, bound)?;
    if !targets.iter().any(|own| anchors.iter().any(|anchor| outside(own, anchor))) {
        return Ok(targets.iter().map(|_| Sighted::default()).collect());
    }
    let mut names: Vec<String> = Vec::new();
    for repo in targets.iter().flatten().flatten().chain(anchors.iter().filter_map(|anchor| anchor.repo.as_ref())) {
        if !names.iter().any(|name| same(name, repo)) {
            names.push(repo.clone());
        }
    }
    let replies = if names.is_empty() { Vec::new() } else { ask(&names, scene.gh, bound)? };
    Ok(settle(targets, &anchors, &(names, replies)))
}

/// 答え（owner/name の並びと同じ順の可視性）から segment ごとの読みを決める（**pure**・設計 §22 行 n4 形 8）: 対象が全て PRIVATE の segment は
/// 通し、それ以外は対象と別の PUBLIC でない anchor を隣・PUBLIC の anchor の name を既に公開の名にする。読めない値は対象なら public・anchor なら private。
fn settle(targets: &[Vec<Option<String>>], anchors: &[Anchor], (names, replies): &(Vec<String>, Vec<Seen>)) -> Vec<Sighted> {
    let seen = |repo: Option<&String>| repo.and_then(|repo| names.iter().position(|name| same(name, repo))).and_then(|at| replies.get(at)).copied().unwrap_or(Seen::Unread);
    targets
        .iter()
        .map(|own| {
            let private = !own.is_empty() && own.iter().all(|target| seen(target.as_ref()) == Seen::Private);
            if private {
                return Sighted { private, ..Sighted::default() };
            }
            let others = anchors.iter().filter(|anchor| outside(own, anchor));
            let neighbors = others.filter(|anchor| seen(anchor.repo.as_ref()) != Seen::Public).cloned().collect();
            let public = anchors.iter().filter(|anchor| seen(anchor.repo.as_ref()) == Seen::Public);
            Sighted { private, neighbors, public: public.filter_map(|anchor| anchor.repo.as_deref()?.split_once('/').map(|(_, name)| name.to_owned())).collect() }
        })
        .collect()
}

/// `gh api graphql --hostname github.com -f query=…` を 1 回撃ち、rc に依らず stdout の JSON を読む（起こせない周は全て `Unread`）。
fn ask(names: &[String], gh: &Path, bound: Bound) -> Result<Vec<Seen>, Halt> {
    let args = ["api".to_owned(), "graphql".to_owned(), "--hostname".to_owned(), GITHUB.to_owned(), "-f".to_owned(), format!("query={}", query(names))];
    match run(gh, &args, Path::new("/"), b"", bound) {
        Ok(ran) => Ok(answers(&String::from_utf8_lossy(&ran.bytes), names.len())),
        Err(Stop::Spawn) => Ok(answers("", names.len())),
        Err(Stop::Deadline) => Err(Halt::Deadline),
    }
}

#[cfg(test)]
mod tests {
    use super::{answers, ask, query, repo_of, settle, Anchor, Seen, Sighted};
    use crate::hook::host_guard::publish::probe::Bound;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// 行 n4 (a) owner/name の表: https・ssh・git・scp 形・利用者の部分・port・`.git`・末尾の `/`・大小は導け、GitHub の外・local の path・字の外・
    /// 成分が足りない形は導けない。
    #[test]
    fn publish_visibility_owner_name_table() {
        let table = [
            ("https://github.com/o/n", Some("o/n")), ("https://github.com/o/n.git/", Some("o/n")), ("https://GitHub.com/O-x/N_1.y.git", Some("O-x/N_1.y")),
            (concat!("https://user:pw", "@github.com/o/n"), Some("o/n")), (concat!("ssh://git", "@github.com/o/n.git"), Some("o/n")),
            (concat!("ssh://git", "@github.com:22/o/n.git"), Some("o/n")), ("git://github.com/o/n.git", Some("o/n")), ("git@github.com:o/n.git", Some("o/n")),
            ("github.com:/o/n", Some("o/n")), ("https://github.com/x/o/n", Some("o/n")), ("https://gitlab.com/o/n", None), ("https://github.com.evil.io/o/n", None),
            (concat!("https://github.com", "@evil.io/o/n"), None), ("http://github.com/o/n", None), ("/tmp/github.com/o/n.git", None), ("./o/n", None),
            ("https://github.com/o", None), ("https://github.com/o/n?x", None), ("https://github.com/o_x/n", None), ("https://github.com/o/n m", None),
            ("https://github.com/o//n", None), ("", None),
        ];
        for (url, want) in table {
            assert_eq!(repo_of(url).as_deref(), want, "{url}");
        }
    }

    /// 呼出しの引数を 1 行ずつ file に残し、`tail` を走らせる偽の gh（tmp の script）と、その呼出しの記録。
    fn fake(name: &str, tail: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("scribe2-publish-visibility-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|why| panic!("dir を作れる: {why}"));
        let (gh, calls) = (dir.join("gh"), dir.join("calls.log"));
        std::fs::write(&gh, format!("#!/bin/sh\necho \"$@\" >> {}\n{tail}\n", calls.display())).unwrap_or_else(|why| panic!("偽の gh を書ける: {why}"));
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap_or_else(|why| panic!("偽の gh を実行可能にできる: {why}"));
        (gh, calls)
    }

    /// 締め切りは 20 秒後・読む上限は 1 MB。
    fn bound() -> Bound {
        Bound { deadline: Instant::now() + Duration::from_secs(20), limit: 1 << 20 }
    }

    /// 行 n4 (b) 問いの引数は owner/name と固定の字だけで、alias は並びの順に `r0` `r1`: 偽の gh が受けた引数は `api graphql --hostname github.com -f
    /// query=…` の 1 行だけで、repo の識別は `owner:"…",name:"…"` の中の owner/name だけ。
    #[test]
    fn publish_visibility_query_carries_only_owner_name_in_alias_order() {
        let names = ["acme/pub".to_owned(), "Acme-2/x.y_z".to_owned()];
        let want = "query{r0:repository(owner:\"acme\",name:\"pub\"){visibility} r1:repository(owner:\"Acme-2\",name:\"x.y_z\"){visibility}}";
        assert_eq!(query(&names), want);
        let (gh, calls) = fake("query", "echo '{\"data\":{\"r0\":{\"visibility\":\"PUBLIC\"},\"r1\":{\"visibility\":\"PRIVATE\"}}}'\nexit 1");
        let got = ask(&names, &gh, bound()).unwrap_or_else(|why| panic!("問える: {why:?}"));
        assert_eq!(got, [Seen::Public, Seen::Private], "rc 1 でも stdout の JSON を読む");
        let seen = std::fs::read_to_string(&calls).unwrap_or_default();
        assert_eq!(seen.trim_end(), format!("api graphql --hostname github.com -f query={want}"));
        let _ = std::fs::remove_dir_all(gh.parent().unwrap_or(Path::new("/")));
    }

    /// 行 n4 (c) 読めない答え（null・知らない値・JSON でない・空・起こせない gh）は対象で public・anchor で private: 対象は PRIVATE でなく、対象と別の
    /// anchor は隣に入る。PUBLIC の anchor は隣でなく既に公開の名になり、INTERNAL の anchor は隣。
    #[test]
    fn publish_visibility_unreadable_answers_read_public_for_targets_and_private_for_anchors() {
        let text = "{\"data\":{\"r0\":null,\"r1\":{\"visibility\":\"SECRET\"},\"r2\":{\"visibility\":\"INTERNAL\"},\"r3\":{\"visibility\":\"PUBLIC\"}}}";
        assert_eq!(answers(text, 5), [Seen::Unread, Seen::Unread, Seen::Internal, Seen::Public, Seen::Unread]);
        for broken in ["", "not json", "[]", "{\"data\":null}", "{\"errors\":[{}]}"] {
            assert_eq!(answers(broken, 2), [Seen::Unread, Seen::Unread], "{broken}");
        }
        let missing = ask(&["o/n".to_owned()], Path::new("/nonexistent/scribe2-no-such-gh"), bound()).unwrap_or_else(|why| panic!("問える: {why:?}"));
        assert_eq!(missing, [Seen::Unread], "起こせない gh");
        let anchor = |label: &str, repo: Option<&str>| Anchor { label: label.to_owned(), repo: repo.map(str::to_owned) };
        let anchors = [anchor("open", Some("o/open")), anchor("lost", None), anchor("intra", Some("o/intra"))];
        let names = vec!["o/pub".to_owned(), "o/open".to_owned(), "o/intra".to_owned()];
        let unread = settle(&[vec![Some("o/pub".to_owned())]], &anchors, &(names.clone(), vec![Seen::Unread; 3]));
        let labels = |one: Option<&Sighted>| one.map(|one| one.neighbors.iter().map(|found| found.label.clone()).collect::<Vec<String>>());
        let want = |list: &[&str]| Some(list.iter().map(|label| (*label).to_owned()).collect::<Vec<String>>());
        assert_eq!((unread.first().map(|one| one.private), labels(unread.first())), (Some(false), want(&["open", "lost", "intra"])));
        let replies = vec![Seen::Private, Seen::Public, Seen::Internal];
        let read = settle(&[vec![Some("o/pub".to_owned())], vec![None]], &anchors, &(names, replies));
        assert_eq!((read.first().map(|one| one.private), labels(read.first())), (Some(true), want(&[])), "対象が PRIVATE の segment は隣を持たない");
        assert_eq!((read.get(1).map(|one| one.private), labels(read.get(1))), (Some(false), want(&["lost", "intra"])), "導けない対象は public");
        assert_eq!(read.get(1).map(|one| one.public.clone()), Some(vec!["open".to_owned()]), "PUBLIC の anchor の name");
    }
}
