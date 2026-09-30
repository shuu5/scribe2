//! publish の走査の段（設計 docs/design/vessel-hook.md §22 行 n5 / n6・ADR-0078 / ADR-0093・SRS FR80 / AC50 / AC63）。
//!
//! 隣が在る segment ごとに、隣（名札 = anchor の basename・名の列 = basename と導けた name）・公開先（対象の name と PUBLIC の anchor の
//! name）・除外（host の面の字句）・行の要素・出ていく字面を照合の核（[`scan`]）へ 1 回渡し、越えの印の在る segment は照合せず
//! oversize、当たりは identifier で断る。object id・tracked path・台帳 id の材料（行 n6）は、候補の在る形だけを隣の repo と押す repo に
//! 問い、読む上限の外で締め切りだけが縛る（子の失敗と読めない台帳は unresolved:neighbor）。

use super::outgoing::{denial, pairs, server_tips, Denial, Found, Gap, Halt};
use super::probe::{run, Bound, Stop, DEADLINE_ROW, READ_ROW};
use super::scan::{candidates, scan, Candidates, Neighbor, Phrases, Public};
use super::visibility::{repo_of, Anchor, Sighted};
use super::{elements, Reason};
use crate::fleet::json_tree;
use crate::hook::host_guard::{Scene, PUBLISH_ROW};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;

/// 隣の材料が読めない止まり（子の失敗・読めない台帳）。
const UNREAD: Halt = Halt::Gap(Gap::Neighbor);

/// 重複を除いて足す（出現の順を保つ）。
fn add(names: &mut Vec<String>, name: &str) {
    if !names.iter().any(|found| found == name) {
        names.push(name.to_owned());
    }
}

/// owner/name の name。
fn name_of(repo: &str) -> Option<&str> {
    repo.split_once('/').map(|(_, name)| name)
}

/// 隣ごとの [`Neighbor`]（名札は dir の basename・名の列は basename と導けた name〔重複なし〕・object / path / 台帳の欄は空）。
fn neighbors(sighted: &Sighted) -> Vec<Neighbor> {
    let one = |anchor: &Anchor| {
        let mut names = vec![anchor.label.clone()];
        if let Some(name) = anchor.repo.as_deref().and_then(name_of) {
            add(&mut names, name);
        }
        Neighbor { tag: anchor.label.clone(), names, ..Neighbor::default() }
    };
    sighted.neighbors.iter().map(one).collect()
}

/// 公開先の [`Public`]（名の列は対象の name と PUBLIC の anchor の name〔重複なし〕・object / path の欄は空）。
fn public(found: &Found) -> Public {
    let mut names = Vec::new();
    for name in found.own.iter().flatten().filter_map(|repo| name_of(repo)) {
        add(&mut names, name);
    }
    for name in &found.sighted.public {
        add(&mut names, name);
    }
    Public { names, ..Public::default() }
}

/// 材料の子を撃つ場（program と dir と締め切りだけの縛り・子は `git -C <dir>`）。
struct Ask<'a> {
    /// git の program。
    program: &'a Path,
    /// 撃つ dir。
    dir: &'a Path,
    /// 締め切り（読む上限は外す）。
    bound: Bound,
}

impl Ask<'_> {
    /// git を 1 回撃つ。rc 0 は標準出力・rc 非 0 と起こせない周は `None`・締め切りの越えは `Err`。
    fn git(&self, args: &[&str], input: &str) -> Result<Option<String>, Halt> {
        let dir = self.dir.to_string_lossy();
        let all: Vec<&str> = ["-C", dir.as_ref()].into_iter().chain(args.iter().copied()).collect();
        match run(self.program, &all, self.dir, input.as_bytes(), self.bound) {
            Ok(ran) if ran.code == 0 => Ok(Some(String::from_utf8_lossy(&ran.bytes).into_owned())),
            Ok(_) | Err(Stop::Spawn) => Ok(None),
            Err(Stop::Deadline) => Err(Halt::Deadline),
        }
    }

    /// `cat-file --batch-check` 1 回（候補を stdin）: 候補ごとの（第 1 語・第 2 語）。子の失敗と行の数の違いは `None`。
    fn kinds(&self, objects: &[String]) -> Result<Option<Vec<(String, String)>>, Halt> {
        let input: String = objects.iter().map(|word| format!("{word}\n")).collect();
        let Some(out) = self.git(&["cat-file", "--batch-check"], &input)? else {
            return Ok(None);
        };
        let rows: Vec<(String, String)> = out.lines().map(|line| line.split_once(' ').unwrap_or((line, ""))).map(|(sha, rest)| (sha.to_owned(), rest.split(' ').next().unwrap_or_default().to_owned())).collect();
        Ok((rows.len() == objects.len()).then_some(rows))
    }
}

/// `-z` の出力の path の列。
fn paths_of(text: &str) -> Vec<String> {
    text.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect()
}

/// `<anchor>/.beads/issues.jsonl` の各行の id（file が無い隣は空・普通の file でない path と読めない行は `None`）。
fn ledger(dir: &Path) -> Option<HashSet<String>> {
    let path = dir.join(".beads").join("issues.jsonl");
    match std::fs::metadata(&path) {
        Err(why) if why.kind() == ErrorKind::NotFound => return Some(HashSet::new()),
        Ok(meta) if meta.is_file() => {}
        _ => return None,
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let id = |line: &str| json_tree::parse(line).ok()?.get("id")?.as_str().map(str::to_owned);
    text.lines().filter(|line| !line.trim().is_empty()).map(id).collect()
}

/// 隣の材料を埋める（候補の在る形だけ・子の失敗と読めない台帳は解けない）。
fn fill_neighbor(one: &mut Neighbor, anchor: &Anchor, cand: &Candidates, ask: &Ask) -> Result<(), Halt> {
    if !cand.objects.is_empty() {
        let rows = ask.kinds(&cand.objects)?.ok_or(UNREAD)?;
        one.objects = cand.objects.iter().zip(rows).filter(|(_, (_, kind))| kind != "missing").map(|(word, _)| word.clone()).collect();
    }
    if !cand.paths.is_empty() {
        one.paths = paths_of(&ask.git(&["ls-files", "-z"], "")?.ok_or(UNREAD)?);
    }
    if !cand.ledger.is_empty() {
        one.ledger = ledger(&anchor.dir).ok_or(UNREAD)?;
    }
    Ok(())
}

/// gh の segment の server の先端: dir の remote のうち導いた owner/name が対象と同じ 1 つの URL の `ls-remote`（無ければ空）。
fn remote_tips(ask: &Ask, own: Option<&String>) -> Result<Vec<(String, String)>, Halt> {
    let (Some(own), Some(config)) = (own, ask.git(&["config", "--list", "-z"], "")?) else {
        return Ok(Vec::new());
    };
    let urls = pairs(&config).into_iter().filter(|(key, _)| key.starts_with("remote.") && (key.ends_with(".url") || key.ends_with(".pushurl")));
    let same = |url: &String| !url.starts_with('-') && repo_of(url).is_some_and(|repo| repo.eq_ignore_ascii_case(own));
    let Some((_, url)) = urls.into_iter().find(|(_, url)| same(url)) else {
        return Ok(Vec::new());
    };
    Ok(ask.git(&["ls-remote", &url, "HEAD", "refs/heads/*", "refs/tags/*"], "")?.map(|out| server_tips(&out)).unwrap_or_default())
}

/// 候補のうち commit で server の先端から辿れるもの（`rev-list --stdin <候補> ^<先端>` の出力に無いもの）。commit でない object と手元に無い
/// 候補と、rev-list が落ちる周の commit の候補は辿れない側。
fn reached(ask: &Ask, objects: &[String], tips: &[(String, String)]) -> Result<HashSet<String>, Halt> {
    let Some(rows) = ask.kinds(objects)? else {
        return Ok(HashSet::new());
    };
    let commits: Vec<(&String, String)> = objects.iter().zip(rows).filter(|(_, (_, kind))| kind == "commit").map(|(word, (sha, _))| (word, sha)).collect();
    let lines = commits.iter().map(|(_, sha)| sha.clone()).chain(tips.iter().map(|(_, sha)| format!("^{sha}")));
    let input: String = lines.map(|line| format!("{line}\n")).collect();
    let listed = if commits.is_empty() { None } else { ask.git(&["rev-list", "--ignore-missing", "--stdin"], &input)? };
    let Some(listed) = listed else {
        return Ok(HashSet::new());
    };
    let out: HashSet<&str> = listed.lines().collect();
    Ok(commits.into_iter().filter(|(_, sha)| !out.contains(sha.as_str())).map(|(word, _)| word.clone()).collect())
}

/// 公開先の材料を埋める（押す repo で・候補の在る形だけ・読めない先端は空）: object id は [`reached`]、tracked path は出ていく ref の変更前の
/// sha と server の HEAD の sha の `ls-tree -r --name-only -z`。
fn fill_public(open: &mut Public, seg: &Found, cand: &Candidates, ask: &Ask) -> Result<(), Halt> {
    let tips = match &seg.push {
        Some(push) => push.tips.clone(),
        None => remote_tips(ask, seg.own.first().and_then(Option::as_ref))?,
    };
    if !cand.objects.is_empty() {
        open.objects = reached(ask, &cand.objects, &tips)?;
    }
    if !cand.paths.is_empty() {
        let olds = seg.push.iter().flat_map(|push| push.refs.iter()).filter_map(|found| found.old.as_deref());
        let heads = tips.iter().filter(|(name, _)| name == "HEAD").map(|(_, sha)| sha.as_str());
        let mut shas: Vec<&str> = Vec::new();
        for sha in olds.chain(heads).filter(|sha| !sha.starts_with('-')) {
            if !shas.contains(&sha) {
                shas.push(sha);
            }
        }
        for sha in shas {
            open.paths.extend(ask.git(&["ls-tree", "-r", "--name-only", "-z", sha], "")?.map(|out| paths_of(&out)).unwrap_or_default());
        }
    }
    Ok(())
}

/// 隣と公開先の材料を埋める（子は読む上限の外で、締め切りだけが縛る）。
fn gather(seg: &Found, cand: &Candidates, (near, open): (&mut [Neighbor], &mut Public), (scene, bound): (&Scene, Bound)) -> Result<(), Halt> {
    let bound = Bound { limit: u64::MAX, ..bound };
    for (one, anchor) in near.iter_mut().zip(&seg.sighted.neighbors) {
        fill_neighbor(one, anchor, cand, &Ask { program: scene.git, dir: &anchor.dir, bound })?;
    }
    if cand.objects.is_empty() && cand.paths.is_empty() {
        return Ok(());
    }
    fill_public(open, seg, cand, &Ask { program: scene.git, dir: &seg.dir, bound })
}

/// 走査の段（**1 関数**・段の入口が可視性の後に呼ぶ）: 隣が在る segment を順に、越えの印が立っていれば照合せず oversize、そうでなければ
/// 候補の在る形の材料を集めて（解けない隣は unresolved:neighbor・締め切りの越えは deadline）[`scan`] を 1 回呼び、当たりを identifier で断る。
/// 最初の断りで止まる。`ruling` は publish の行の裁定 id。
pub(super) fn check(found: &[Found], manifest: &Manifest, (scene, bound): (&Scene, Bound), ruling: &str) -> Option<Denial> {
    let forms = match manifest.get(PUBLISH_ROW).map(|row| &row.value) {
        Some(RuleValue::List(values)) => elements(values).unwrap_or_default(),
        _ => Default::default(),
    };
    let phrases = Phrases { phrases: scene.host.publish_exclusions().iter().map(|found| found.phrase().to_owned()).collect() };
    let ruled = |id: &str| manifest.get(id).map_or_else(|| "-".to_owned(), |row| row.ruling.clone());
    for seg in found.iter().filter(|seg| !seg.sighted.neighbors.is_empty()) {
        if seg.texts.over {
            return Some(Denial { reason: Reason::Oversize, word: READ_ROW.to_owned(), row: READ_ROW, ruling: ruled(READ_ROW), route: None });
        }
        let texts = seg.texts.texts();
        let (mut near, mut open) = (neighbors(&seg.sighted), public(seg));
        if let Err(halt) = gather(seg, &candidates(&forms, &texts), (&mut near, &mut open), (scene, bound)) {
            return Some(denial(halt, ruling, &ruled(DEADLINE_ROW)));
        }
        if let Some(word) = scan(&forms, &texts, &near, &open, &phrases) {
            return Some(Denial { reason: Reason::Identifier, word, row: PUBLISH_ROW, ruling: ruling.to_owned(), route: None });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{check, neighbors, public};
    use crate::hook::host_guard::publish::outgoing::{Denial, Found, Push};
    use crate::hook::host_guard::publish::probe::Bound;
    use crate::hook::host_guard::publish::scan::Source;
    use crate::hook::host_guard::publish::texts::{Body, Texts};
    use crate::hook::host_guard::publish::visibility::{Anchor, Sighted};
    use crate::hook::host_guard::publish::Reason;
    use crate::hook::host_guard::Scene;
    use crate::invocation::Invocation;
    use crate::rules::manifest::Manifest;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// 公開の行（form は引数）と上限の 2 行を持つ manifest（裁定 id は `r-<行 id>`）。
    fn manifest_with(forms: &[&str]) -> Manifest {
        let row = |id: &str, kind: &str, value: &str| format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r-{id}\"\nruled_at = \"d\"\n");
        let list = forms.iter().map(|form| format!("\"form {form}\"")).collect::<Vec<_>>().join(", ");
        let text = format!(
            "schema = 1\n{}{}{}",
            row("host_guard.publish", "HostGuardPublish", &format!("[{list}]")),
            row("host_guard.publish_deadline_ms", "HostGuardPublishDeadlineMs", "6000"),
            row("host_guard.publish_read_bytes", "HostGuardPublishReadBytes", "8388608"),
        );
        Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// 公開の行が form repo-name だけの manifest。
    fn manifest() -> Manifest {
        manifest_with(&["repo-name"])
    }

    /// 走査の段を撃つ（host の面は空・git は本物・読む上限は 8 byte・締め切りは 20 秒後）。
    fn run(found: &[Found], rules: &Manifest, ruling: &str) -> Option<Denial> {
        checked(found, rules, (Path::new("git"), 20_000), ruling)
    }

    /// 走査の段を `git` の program と締め切りのミリ秒で撃つ（読む上限は 8 byte）。
    fn checked(found: &[Found], rules: &Manifest, (git, ms): (&Path, u64), ruling: &str) -> Option<Denial> {
        let face = Manifest::default();
        let scene = Scene { cwd: Path::new("/"), state_dir: Path::new("/"), git, gh: Path::new("gh"), host: &face };
        check(found, rules, (&scene, Bound { deadline: Instant::now() + Duration::from_millis(ms), limit: 8 }), ruling)
    }

    /// 隣 2 つ（導けた anchor と導けなかった anchor）・PUBLIC の anchor の name `open` を持つ segment（対象は acme/pub・本文は 1 つ）。
    fn found(body: &str, over: bool) -> Found {
        let neighbor = |label: &str, repo: Option<&str>| Anchor { label: label.to_owned(), repo: repo.map(str::to_owned), dir: PathBuf::new() };
        let sighted = Sighted { private: false, neighbors: vec![neighbor("inner", Some("acme/secret")), neighbor("lost", None)], public: vec!["open".to_owned()] };
        let texts = Texts { bodies: vec![Body { source: Source::CommitMessage, body: body.to_owned() }], over };
        Found { texts, sighted, own: vec![Some("acme/pub".to_owned())], dir: PathBuf::new(), push: None }
    }

    /// 行 n5 (a) 隣が PUBLIC でない anchor で名札は basename・名の列は basename と導けた name（重複なし）、既に公開の名は対象の name と PUBLIC の
    /// anchor の name: 隣の名は identifier（件数と先頭・row は publish の行・裁定 id は渡した id）で断り、既に公開の名は通し、隣が 0 の segment は通す。
    #[test]
    fn publish_scan_names_the_neighbors_by_basename_and_spares_the_public_names() {
        let seg = found("", false);
        let got: Vec<(String, Vec<String>)> = neighbors(&seg.sighted).into_iter().map(|one| (one.tag, one.names)).collect();
        let want = [("inner".to_owned(), vec!["inner".to_owned(), "secret".to_owned()]), ("lost".to_owned(), vec!["lost".to_owned()])];
        assert_eq!(got, want);
        let same = Sighted { neighbors: vec![Anchor { label: "same".to_owned(), repo: Some("acme/same".to_owned()), dir: PathBuf::new() }], ..Sighted::default() };
        assert_eq!(neighbors(&same).first().map(|one| one.names.clone()), Some(vec!["same".to_owned()]), "重複なし");
        assert_eq!(public(&seg).names, ["pub", "open"]);
        assert!(neighbors(&seg.sighted).iter().all(|one| one.objects.is_empty() && one.paths.is_empty() && one.ledger.is_empty()));
        let rules = manifest();
        let denied = run(&[found("fix secret and pub and open", false)], &rules, "ruling-x").unwrap_or_else(|| panic!("隣の名は断る"));
        assert_eq!((denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str()), (Reason::Identifier, "1:repo-name=secret@inner", "host_guard.publish", "ruling-x"));
        let two = run(&[found("inner lost", false)], &rules, "r").map(|one| one.word);
        assert_eq!(two.as_deref(), Some("2:repo-name=inner@inner,repo-name=lost@lost"));
        assert!(run(&[found("pub open", false)], &rules, "r").is_none(), "既に公開の名は通す");
        let alone = Found { sighted: Sighted::default(), ..found("secret", false) };
        assert!(run(&[alone], &rules, "r").is_none(), "隣が 0 の segment は通す");
    }

    /// 行 n5 (b) 越えの印の在る segment は照合せず oversize（row と裁定 id は読む上限の行）。隣が 0 の segment の越えは通し、断りは segment の順の最初で止まる。
    #[test]
    fn publish_scan_oversize_is_denied_without_matching() {
        let rules = manifest();
        let denied = run(&[found("nothing", true)], &rules, "r").unwrap_or_else(|| panic!("越えは断る"));
        let got = (denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str());
        assert_eq!(got, (Reason::Oversize, "host_guard.publish_read_bytes", "host_guard.publish_read_bytes", "r-host_guard.publish_read_bytes"));
        let quiet = Found { sighted: Sighted::default(), ..found("nothing", true) };
        assert!(run(std::slice::from_ref(&quiet), &rules, "r").is_none(), "隣が 0 の周の越えは通す");
        let first = run(&[quiet, found("secret", true), found("secret", false)], &rules, "r").map(|one| one.reason);
        assert_eq!(first, Some(Reason::Oversize), "segment の順に最初の断り");
    }

    /// git を 1 回撃ち rc 0 を要求して標準出力を返す。
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Invocation::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@e.invalid"]).args(args).output();
        let out = out.unwrap_or_else(|why| panic!("git を撃てる: {why}"));
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// 歯ごとの置き場: 公開先 `open`（commit 1 つ・shared/lib.rs）と、それを clone した隣 `nbr`（隣だけの commit と secret/plan.md）。
    struct Place {
        root: PathBuf,
        open: PathBuf,
        nbr: PathBuf,
        /// 公開先の先端の sha（隣にも在る）。
        shared: String,
        /// 隣だけの commit の sha。
        secret: String,
    }

    fn place(name: &str) -> Place {
        let root = std::env::temp_dir().join(format!("scribe2-publish-material-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (open, nbr) = (root.join("open"), root.join("nbr"));
        std::fs::create_dir_all(open.join("shared")).unwrap_or_else(|why| panic!("dir を作れる: {why}"));
        git(&root, &["init", "-q", "-b", "main", &open.display().to_string()]);
        std::fs::write(open.join("shared/lib.rs"), "x\n").unwrap_or_else(|why| panic!("file を書ける: {why}"));
        git(&open, &["add", "-A"]);
        git(&open, &["commit", "-q", "-m", "base"]);
        git(&root, &["clone", "-q", &open.display().to_string(), &nbr.display().to_string()]);
        std::fs::create_dir_all(nbr.join("secret")).unwrap_or_else(|why| panic!("dir を作れる: {why}"));
        std::fs::write(nbr.join("secret/plan.md"), "y\n").unwrap_or_else(|why| panic!("file を書ける: {why}"));
        git(&nbr, &["add", "-A"]);
        git(&nbr, &["commit", "-q", "-m", "secret"]);
        let (shared, secret) = (git(&open, &["rev-parse", "HEAD"]), git(&nbr, &["rev-parse", "HEAD"]));
        Place { root, open, nbr, shared, secret }
    }

    /// 本文 1 つを持つ segment（隣は nbr・公開先は open の先端が `main` と `HEAD` の push）。
    fn seen(place: &Place, body: &str) -> Found {
        let anchor = Anchor { label: "nbr".to_owned(), repo: Some("acme/nbr".to_owned()), dir: place.nbr.clone() };
        let tips = vec![("HEAD".to_owned(), place.shared.clone()), ("refs/heads/main".to_owned(), place.shared.clone())];
        let texts = Texts { bodies: vec![Body { source: Source::CommitMessage, body: body.to_owned() }], over: false };
        let sighted = Sighted { neighbors: vec![anchor], ..Sighted::default() };
        Found { texts, sighted, own: vec![Some("acme/pub".to_owned())], dir: place.open.clone(), push: Some(Push { tips, ..Push::default() }) }
    }

    /// 呼出しを `calls.log` に 1 行ずつ残し、`tail` を走らせる偽の git（実行可能な script）と、その記録の path。
    fn fake(place: &Place, tail: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let (path, calls) = (place.root.join("fake-git"), place.root.join("calls.log"));
        let _ = std::fs::remove_file(&calls);
        std::fs::write(&path, format!("#!/bin/sh\necho \"$@\" >> {}\n{tail}\n", calls.display())).unwrap_or_else(|why| panic!("偽の git を書ける: {why}"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap_or_else(|why| panic!("偽の git を実行可能にできる: {why}"));
        (path, calls)
    }

    /// 本物の git へ渡す偽の git で `body` を撃った断りの hit の語。
    fn word(place: &Place, forms: &[&str], body: &str) -> Option<String> {
        let (wrap, _) = fake(place, "exec git \"$@\"");
        checked(&[seen(place, body)], &manifest_with(forms), (&wrap, 20_000), "r").map(|one| one.word)
    }

    /// 行 n6 (a) 形ごとに候補の在る周だけ 1 回問う: object id だけの本文は隣と公開先の `cat-file --batch-check` が 1 回ずつで `ls-files` は 0 回、path だけの
    /// 本文は `ls-files` が 1 回で `cat-file` は 0 回。`ambiguous` は在る・台帳の file の無い隣は空・読めない行は unresolved:neighbor。
    #[test]
    fn publish_material_asks_only_the_forms_with_candidates_and_reads_ambiguous_and_ledgers() {
        let place = place("asks");
        let forms = ["object-id", "tracked-path", "ledger-id"];
        let (wrap, calls) = fake(&place, "exec git \"$@\"");
        let lines = |needle: &str| std::fs::read_to_string(&calls).unwrap_or_default().lines().filter(|line| line.contains(needle)).count();
        let hit = checked(&[seen(&place, &format!("see {}", &place.secret[..7]))], &manifest_with(&forms), (&wrap, 20_000), "r").map(|one| one.word);
        assert_eq!(hit, Some(format!("1:object-id={}@nbr", &place.secret[..7])));
        let asked = |dir: &Path| lines(&format!("-C {} cat-file --batch-check", dir.display()));
        assert_eq!((asked(&place.nbr), asked(&place.open), lines("ls-files")), (1, 1, 0), "object id だけ");
        let (wrap, calls) = fake(&place, "exec git \"$@\"");
        let hit = checked(&[seen(&place, "see secret/plan.md")], &manifest_with(&forms), (&wrap, 20_000), "r").map(|one| one.word);
        assert_eq!(hit.as_deref(), Some("1:tracked-path=secret/plan.md@nbr"));
        let text = std::fs::read_to_string(&calls).unwrap_or_default();
        assert_eq!((text.matches("ls-files -z").count(), text.matches("cat-file").count()), (1, 0), "path だけ");
        let (guess, _) = fake(&place, "case \"$*\" in *cat-file*) cat > /dev/null; echo 'abcdef1 ambiguous';; *) exec git \"$@\";; esac");
        let hit = checked(&[seen(&place, "see abcdef1")], &manifest_with(&forms), (&guess, 20_000), "r").map(|one| one.word);
        assert_eq!(hit.as_deref(), Some("1:object-id=abcdef1@nbr"), "ambiguous は在る");
        assert_eq!(word(&place, &forms, "see 1234567"), None, "missing は無い");
        assert_eq!(word(&place, &forms, "see s2-x.1"), None, "台帳の file の無い隣は空");
        std::fs::create_dir_all(place.nbr.join(".beads")).unwrap_or_else(|why| panic!("dir を作れる: {why}"));
        let issues = place.nbr.join(".beads/issues.jsonl");
        std::fs::write(&issues, "{\"id\":\"s2-x.1\"}\n\n{\"id\":\"s2-x.2\"}\n").unwrap_or_else(|why| panic!("台帳を書ける: {why}"));
        assert_eq!(word(&place, &forms, "see s2-x.1").as_deref(), Some("1:ledger-id=s2-x.1@nbr"));
        assert_eq!(word(&place, &forms, "see s2-x.3"), None);
        std::fs::write(&issues, "{\"id\":\"s2-x.1\"}\nnot json\n").unwrap_or_else(|why| panic!("台帳を書ける: {why}"));
        let (wrap, _) = fake(&place, "exec git \"$@\"");
        let denied = checked(&[seen(&place, "see s2-x.1")], &manifest_with(&forms), (&wrap, 20_000), "r").unwrap_or_else(|| panic!("読めない行は断る"));
        assert_eq!((denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str()), (Reason::Unresolved, "neighbor", "host_guard.publish", "r"));
        let _ = std::fs::remove_dir_all(&place.root);
    }

    /// 行 n6 (b) 公開先の辿れる object は commit だけ: 公開先の先端の commit は通し、公開先にも在る blob（commit でない）と公開先に無い隣だけの
    /// commit は辿れない側で identifier。
    #[test]
    fn publish_material_only_commits_the_public_tips_reach_are_spared() {
        let place = place("reach");
        let forms = ["object-id"];
        let blob = git(&place.nbr, &["rev-parse", "HEAD:shared/lib.rs"]);
        assert_eq!(word(&place, &forms, &format!("see {}", &place.shared[..7])), None, "先端の commit");
        assert_eq!(word(&place, &forms, &format!("see {}", &blob[..7])).as_deref(), Some(&*format!("1:object-id={}@nbr", &blob[..7])), "blob");
        assert_eq!(word(&place, &forms, &format!("see {}", &place.secret[..7])).as_deref(), Some(&*format!("1:object-id={}@nbr", &place.secret[..7])), "手元に無い commit");
        let _ = std::fs::remove_dir_all(&place.root);
    }

    /// 行 n6 (c) 読む上限を 8 byte にした `Bound` でも、隣の tracked path の列（上限を越える長さの path）を読み切り、その path を足す字面は path の当たり
    /// （identifier）で断る（unresolved でも oversize でもない）。公開先の先端にも在る path は通す。
    #[test]
    fn publish_material_the_read_limit_does_not_bound_the_neighbor_paths() {
        let place = place("limit");
        let (wrap, _) = fake(&place, "exec git \"$@\"");
        let rules = manifest_with(&["tracked-path"]);
        let denied = checked(&[seen(&place, "see secret/plan.md")], &rules, (&wrap, 20_000), "r").unwrap_or_else(|| panic!("path の当たりは断る"));
        assert_eq!((denied.reason, denied.word.as_str()), (Reason::Identifier, "1:tracked-path=secret/plan.md@nbr"));
        assert!(checked(&[seen(&place, "see shared/lib.rs")], &rules, (&wrap, 20_000), "r").is_none(), "公開先にも在る path");
        let _ = std::fs::remove_dir_all(&place.root);
    }

    /// 行 n6 (d) 呼ばれると 30 秒眠る偽の git を `Scene` の git に置き、締め切り 1500 ms で隣の在る segment を撃つと、2.5 秒の内に deadline（row は締め切りの行）で断る。
    #[test]
    fn publish_material_a_sleeping_child_is_stopped_by_the_deadline() {
        let place = place("sleep");
        let (sleeper, _) = fake(&place, "sleep 30");
        let started = Instant::now();
        let denied = checked(&[seen(&place, "see secret/plan.md")], &manifest_with(&["tracked-path"]), (&sleeper, 1500), "r").unwrap_or_else(|| panic!("締め切りで断る"));
        assert_eq!((denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str()), (Reason::Deadline, "host_guard.publish_deadline_ms", "host_guard.publish_deadline_ms", "r-host_guard.publish_deadline_ms"));
        assert!(started.elapsed() < Duration::from_millis(2500), "締め切り 1500 ms の内: {:?}", started.elapsed());
        let _ = std::fs::remove_dir_all(&place.root);
    }

    /// 行 n6 (e) rev-list にだけ rc 1 を返し他の子は本物の git へ渡す偽の git では、本物の git なら公開先の先端から辿れて通る隣の commit id が
    /// 辿れない側に残り identifier で断る（空の出力と同じに読まない）。
    #[test]
    fn publish_material_a_failing_rev_list_leaves_the_commit_unreachable() {
        let place = place("revlist");
        let (forms, body) = (["object-id"], format!("see {}", &place.shared[..7]));
        assert_eq!(word(&place, &forms, &body), None, "本物の git なら通る");
        let (broken, _) = fake(&place, "case \"$*\" in *rev-list*) exit 1;; *) exec git \"$@\";; esac");
        let denied = checked(&[seen(&place, &body)], &manifest_with(&forms), (&broken, 20_000), "r").unwrap_or_else(|| panic!("辿れない側に置く"));
        assert_eq!((denied.reason, denied.word), (Reason::Identifier, format!("1:object-id={}@nbr", &place.shared[..7])));
        let _ = std::fs::remove_dir_all(&place.root);
    }
}
