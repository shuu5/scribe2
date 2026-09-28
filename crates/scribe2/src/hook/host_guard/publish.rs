//! host-guard の 6 つ目の種類 publish の土台（設計 docs/design/vessel-hook.md §16 行 j・ADR-0078・SRS FR80 / AC50 / NFR4）。
//!
//! 公開の segment の読み（[`read`]・pure な 1 関数）と、rules 行 host_guard.publish の要素の読み手（[`elements`]）と、本行の
//! 範囲の判定（[`judge`]: 公開の segment が在る周に行が無い・列でない周だけ断る）を持つ。読むのは頭の語が git か gh の
//! segment だけで、字面で読めない形の印・全履歴・照合・配線は後続の行が足す。本 module は子 process を撃たない。

use super::{root_of, Kind, Refusal, Subject, PUBLISH_ROW};
use crate::hook::live_row::{git_segment, walked, Walked};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::{Path, PathBuf};

/// 識別子の形の要素の札。
const FORM: &str = "form";
/// 除外の digest の要素の札。
const EXCLUDE: &str = "exclude";
/// 除外の digest の字数（sha256 の 16 進）。
const DIGEST_LEN: usize = 64;
/// 比べる語の末尾から落とす字（`(cd sub && git push)` の `push)` も push）。
const TAIL: [char; 4] = [')', '}', '`', ';'];
/// git の公開の動詞。
const PUSH: &str = "push";
/// gh の api の群（動詞を持たない）。
const API: &str = "api";
/// gh api の graphql の対象。
const GRAPHQL: &str = "graphql";
/// `-R` / `--repo` を群の flag に持つ群。
const REPO_GROUPS: [&str; 4] = ["pr", "issue", "release", "label"];
/// gh の公開の動詞の閉じた表（群と動詞・`new` は組み込みの別名・`reopen` は `-c` で本文を出す）。
const TABLE: [(&str, &[&str]); 6] = [
    ("pr", &["create", "new", "edit", "comment", "review", "merge", "close", "reopen"]),
    ("issue", &["create", "new", "edit", "comment", "close", "reopen"]),
    ("release", &["create", "new", "edit", "upload"]),
    ("gist", &["create", "new", "edit"]),
    ("label", &["create", "edit"]),
    ("repo", &["create", "new", "edit"]),
];
/// gh api の値を取る flag（短い形〔無ければ空〕・長い形）。
const VALUED: [(&str, &str); 10] = [
    ("-X", "--method"), ("-H", "--header"), ("-f", "--raw-field"), ("-F", "--field"), ("", "--input"),
    ("-q", "--jq"), ("-t", "--template"), ("-p", "--preview"), ("", "--hostname"), ("", "--cache"),
];
/// gh api の値を取らない flag。
const BARE: [(&str, &str); 4] = [("-i", "--include"), ("", "--paginate"), ("", "--silent"), ("", "--verbose")];
/// 書きの method。
const WRITE_METHODS: [&str; 4] = ["POST", "PUT", "PATCH", "DELETE"];

/// 公開の前に走査する識別子の形の記号（閉じた 4 値・宣言順・rules 行 host_guard.publish の `form` の値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// 隣の repo の名。
    RepoName,
    /// git の object id。
    ObjectId,
    /// 隣の repo の tracked な path。
    TrackedPath,
    /// 台帳の id。
    LedgerId,
}

/// [`Form`] の全 variant（宣言順）。
pub const FORMS: &[Form] = &[Form::RepoName, Form::ObjectId, Form::TrackedPath, Form::LedgerId];

impl Form {
    /// rules 行の値の記号。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepoName => "repo-name",
            Self::ObjectId => "object-id",
            Self::TrackedPath => "tracked-path",
            Self::LedgerId => "ledger-id",
        }
    }

    /// 記号の字面から引く（綴り違いは `None`＝読み込みで拒む）。
    pub fn parse(text: &str) -> Option<Self> {
        FORMS.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 行の値を読んだもの（形の記号の列と除外の digest の列・どちらも manifest の順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Elements {
    /// 走査する識別子の形。
    pub forms: Vec<Form>,
    /// 除外の digest（16 進 64 字の小文字）。
    pub excludes: Vec<String>,
}

/// 行の要素の読み手（**1 本**・`names_are_known` の arm が呼ぶ）: 各要素は `form <記号>` か `exclude <digest>`。綴り違いの
/// 記号・同じ記号 2 回・16 進 64 字の小文字でない digest・同じ digest 2 回・札の外の要素を、要素を名指す理由で拒む（NFR4）。
pub fn elements(values: &[String]) -> Result<Elements, String> {
    let mut found = Elements::default();
    for value in values {
        let words: Vec<&str> = value.split_whitespace().collect();
        let reason = match words.as_slice() {
            [FORM, symbol] => match Form::parse(symbol) {
                Some(form) if !found.forms.contains(&form) => {
                    found.forms.push(form);
                    continue;
                }
                Some(_) => "記号が 2 回目".to_owned(),
                None => {
                    let taken: Vec<&str> = FORMS.iter().map(|form| form.as_str()).collect();
                    format!("記号が未知（取るのは {}）", taken.join(" / "))
                }
            },
            [EXCLUDE, digest] if !is_digest(digest) => format!("digest が 16 進 {DIGEST_LEN} 字の小文字でない"),
            [EXCLUDE, digest] if found.excludes.iter().any(|seen| seen == digest) => "digest が 2 回目".to_owned(),
            [EXCLUDE, digest] => {
                found.excludes.push((*digest).to_owned());
                continue;
            }
            _ => format!("札が {FORM} <記号> / {EXCLUDE} <digest> の形でない"),
        };
        return Err(format!("要素 {value:?} の{reason}"));
    }
    Ok(found)
}

/// 16 進 64 字の小文字か。
fn is_digest(text: &str) -> bool {
    text.len() == DIGEST_LEN && text.chars().all(|found| matches!(found, '0'..='9' | 'a'..='f'))
}

/// 公開の segment の種別。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    /// `git push`。
    #[default]
    Git,
    /// gh の公開の群と動詞。
    Gh,
    /// `gh api` の書き（か、知らない flag で書きかを読めない api）。
    Api,
}

/// gh api の読み（対象・method・欄・`--input` の値）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Api {
    /// 対象（host と `api/v3/`・先頭と末尾の `/`・`?` と `#` から後ろを落とした字面・知らない flag より後ろは読まない）。
    pub target: Option<String>,
    /// 最後の `-X` / `--method` の値（大文字）。
    pub method: Option<String>,
    /// 欄（flag〔`-f` か `-F`〕・key・value）の列。
    pub fields: Vec<(String, String, String)>,
    /// `--input` の値。
    pub input: Option<String>,
}

impl Api {
    /// 値を取る flag（長い形）の値を読みに足す。
    fn take(&mut self, (short, long): (&str, &str), value: String) {
        match long {
            "--method" => self.method = Some(value.to_ascii_uppercase()),
            "--raw-field" | "--field" => {
                let (key, rest) = value.split_once('=').unwrap_or((&value, ""));
                self.fields.push((short.to_owned(), key.to_owned(), rest.to_owned()));
            }
            "--input" => self.input = Some(value),
            _ => {}
        }
    }

    /// 書きか: graphql は `query` の欄が語 mutation を持つか、`-F` の `query` の値が `@` で始まるか、`--input` を持つ周だけ。
    /// 他の対象は method が書きの 4 つか、method が無く欄か `--input` を持つ周（gh が POST にする形）。
    fn writes(&self) -> bool {
        if self.target.as_deref() == Some(GRAPHQL) {
            let query = |(flag, key, value): &(String, String, String)| {
                key == "query" && (value.split(|found: char| !found.is_ascii_alphanumeric()).any(|word| word == "mutation")
                    || (flag == "-F" && value.starts_with('@')))
            };
            return self.input.is_some() || self.fields.iter().any(query);
        }
        match self.method.as_deref() {
            Some(method) => WRITE_METHODS.contains(&method),
            None => self.input.is_some() || !self.fields.is_empty(),
        }
    }
}

/// 公開の segment 1 つの読み（設計 §16 形 3）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Published {
    /// 種別。
    pub sort: Sort,
    /// gh の群（git は `None`・api は `api`）。
    pub group: Option<String>,
    /// 動詞（git は `push`・api は `None`）。
    pub verb: Option<String>,
    /// 動詞の後ろの語（api は `api` の後ろの語）。
    pub rest: Vec<String>,
    /// 対象の dir（解けない周は repo の root）。
    pub dir: PathBuf,
    /// 対象の dir を字面で解けたか。
    pub resolved: bool,
    /// `-R` / `--repo` の値。
    pub repo: Option<String>,
    /// 前置きの `GH_REPO=` の値。
    pub gh_repo: Option<String>,
    /// 前置きの `GH_HOST=` の値。
    pub gh_host: Option<String>,
    /// api の読み（api の segment だけ）。
    pub api: Option<Api>,
    /// 知らない flag を持つか。
    pub unknown_flag: bool,
}

/// 公開の segment の読み（**pure な 1 関数**・設計 §16 形 3）: command 行を segment の読み手（[`walked`]）で辿り、頭の語が
/// git の segment は動詞が push のもの、gh の segment は群と動詞が公開の表に在るもの・api の書き・知らない flag を持つものを
/// 返す。`cwd` は payload の cwd、`root` は解けない dir を倒す先。他の頭の語の segment は読まない（印は行 k）。
pub fn read(command: &str, cwd: &Path, root: &Path) -> Vec<Published> {
    walked(command, cwd)
        .iter()
        .filter_map(|seg| match trimmed(seg.words.first()?) {
            "git" => pushed(seg, root),
            "gh" => gh(seg, root),
            _ => None,
        })
        .collect()
}

/// 比べる語（末尾の [`TAIL`] を落とす）。
fn trimmed(word: &str) -> &str {
    word.trim_end_matches(TAIL)
}

/// 辿った segment の既定の読み（dir と解けたか・前置きの `GH_REPO=` / `GH_HOST=` の値・後の値が勝つ）。
fn base(sort: Sort, seg: &Walked, root: &Path) -> Published {
    let env = |name: &str| seg.lead.iter().rev().find_map(|word| word.strip_prefix(name)).map(str::to_owned);
    let dir = if seg.resolved { seg.dir.clone() } else { root.to_path_buf() };
    Published { sort, dir, resolved: seg.resolved, gh_repo: env("GH_REPO="), gh_host: env("GH_HOST="), ..Published::default() }
}

/// git の segment のうち動詞が push のもの（dir と解けたかは git の segment の読み手が `-C` と `--git-dir` まで解いた値）。
fn pushed(seg: &Walked, root: &Path) -> Option<Published> {
    let git = git_segment(seg, root).filter(|git| trimmed(&git.verb) == PUSH)?;
    let found = base(Sort::Git, seg, root);
    Some(Published { verb: Some(PUSH.to_owned()), rest: git.rest, dir: git.dir, resolved: git.resolved, ..found })
}

/// gh の segment の読み: 群と動詞が公開の表に在るか、知らない flag を持ち群を読めた周、api は書きか知らない flag を持つ周。
fn gh(seg: &Walked, root: &Path) -> Option<Published> {
    let rest = seg.words.get(1..).unwrap_or_default();
    let (picked, at, repo, unknown) = leading(rest);
    let group = picked.first().cloned();
    let unknown = unknown || (repo.is_some() && !group.as_deref().is_some_and(|found| REPO_GROUPS.contains(&found)));
    let after = rest.get(at..).unwrap_or_default().to_vec();
    let found = Published { group: group.clone(), repo, rest: after.clone(), unknown_flag: unknown, ..base(Sort::Gh, seg, root) };
    if group.as_deref() == Some(API) {
        let (api, strange) = api(&after);
        let unknown_flag = unknown || strange;
        return (unknown_flag || api.writes()).then_some(Published { sort: Sort::Api, api: Some(api), unknown_flag, ..found });
    }
    let verb = picked.get(1).cloned();
    let listed = TABLE.iter().any(|(name, verbs)| group.as_deref() == Some(*name) && verb.as_deref().is_some_and(|v| verbs.contains(&v)));
    (listed || (unknown && group.is_some())).then_some(Published { verb, ..found })
}

/// gh の後ろの語から群と動詞（flag でも flag の値でもない最初の 2 語・api は群だけ）と、その前の `-R` / `--repo` の値
/// （`--repo=<値>` と `-R<値>` の続け書きも）と、それ以外の flag を持つかを読む。位置は読んだ最後の語の次。
fn leading(rest: &[String]) -> (Vec<String>, usize, Option<String>, bool) {
    let (mut picked, mut at, mut repo, mut unknown) = (Vec::<String>::new(), 0_usize, None, false);
    while let Some(word) = rest.get(at) {
        at = at.saturating_add(1);
        if word == "-R" || word == "--repo" {
            repo = rest.get(at).cloned();
            at = at.saturating_add(1);
        } else if let Some(value) = word.strip_prefix("--repo=").or_else(|| word.strip_prefix("-R")) {
            repo = Some(value.to_owned());
        } else if word.starts_with('-') && word.len() > 1 {
            unknown = true;
        } else {
            picked.push(trimmed(word).to_owned());
            if picked.len() == 2 || picked.first().is_some_and(|group| group == API) {
                break;
            }
        }
    }
    (picked, at, repo, unknown)
}

/// gh api の後ろの語の読み（flag は閉じた 2 列・`--flag=<値>` と 1 字の flag の続け書きも同じ）と、知らない flag を持つか
/// （持てば値を取るかが判らないので、それより後ろは読まない）。対象は flag にも flag の値にも取られない最初の語。
fn api(rest: &[String]) -> (Api, bool) {
    let mut found = Api::default();
    let mut at = 0_usize;
    while let Some(word) = rest.get(at) {
        at = at.saturating_add(1);
        let Some((flag, inline)) = flag_of(word) else {
            if found.target.is_none() {
                found.target = Some(target_of(word));
            }
            continue;
        };
        let known = |list: &[(&'static str, &'static str)]| list.iter().copied().find(|(short, long)| flag == *short || flag == *long);
        if known(&BARE).is_some() {
            continue;
        }
        let Some(pair) = known(&VALUED) else {
            return (found, true);
        };
        let value = match inline {
            Some(value) => value.to_owned(),
            None => {
                let value = rest.get(at).cloned().unwrap_or_default();
                at = at.saturating_add(1);
                value
            }
        };
        found.take(pair, value);
    }
    (found, false)
}

/// flag の語を（flag の名・続け書きの値）に分ける。flag でない語は `None`。
fn flag_of(word: &str) -> Option<(&str, Option<&str>)> {
    if word.starts_with("--") && word.len() > 2 {
        return Some(word.split_once('=').map_or((word, None), |(name, value)| (name, Some(value))));
    }
    if !(word.starts_with('-') && word.len() > 1) {
        return None;
    }
    match (word.get(..2), word.get(2..)) {
        (Some(name), Some(value)) => Some((name, Some(value).filter(|found| !found.is_empty()))),
        _ => Some((word, None)),
    }
}

/// api の対象の正規化: `http://` か `https://` と host の成分・続く `api/v3/`・先頭の `/`・`?` か `#` から後ろ・末尾の `/`
/// を落とす（gh が埋める `{owner}` / `{repo}` / `{branch}` の成分は字面のまま）。
fn target_of(word: &str) -> String {
    let path = match word.strip_prefix("https://").or_else(|| word.strip_prefix("http://")) {
        Some(rest) => rest.find('/').and_then(|at| rest.get(at..)).unwrap_or_default(),
        None => word,
    };
    let path = path.trim_start_matches('/');
    let path = path.strip_prefix("api/v3/").unwrap_or(path);
    path.split(['?', '#']).next().unwrap_or_default().trim_matches('/').to_owned()
}

/// 本行の範囲の判定（設計 §16 形 5）: 公開の segment が 0 の周は行を読まずに通し、1 つ以上の周に行が無い・列でない周は
/// `no-row` で断り、行が在る周は `enabled` を問わず通す（読めない形・全履歴・走査の断りは後続の行）。子 process は撃たない。
pub(super) fn judge(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    let cwd = subject.scene.cwd;
    let root = root_of(cwd).unwrap_or_else(|| cwd.to_path_buf());
    if read(subject.command, cwd, &root).is_empty() {
        return None;
    }
    match manifest.get(PUBLISH_ROW).map(|row| &row.value) {
        Some(RuleValue::List(_)) => None,
        _ => Some(Refusal::no_row(kind, PUBLISH_ROW)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{judge, HostGuardDecision, Kind, Scene};
    use super::{elements, read, Form, Published, FORMS};
    use crate::name::NAME;
    use crate::rules::manifest::Manifest;
    use std::path::{Path, PathBuf};

    /// 1 行の本文。
    fn row(id: &str, kind: &str, value: &str, enabled: bool) -> String {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = [{value}]\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n")
    }

    /// 語列の 3 行と `extra` の本文を持つ manifest（publish の行は `extra` だけが持つ）。
    fn manifest(extra: &str) -> Manifest {
        let mut text = "schema = 1\n".to_owned();
        for (id, value) in [("host_guard.git", "\"git push --force\""), ("host_guard.tmux", "\"tmux kill-server\""), ("host_guard.ledger", "\"bd delete\"")] {
            text.push_str(&row(id, "HostGuardDeniedCommands", value, true));
        }
        text.push_str(extra);
        Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// Bash の判定の (what, line)。Allow なら `None`。
    fn denied(command: &str, manifest: &Manifest) -> Option<(String, String)> {
        let scene = Scene { cwd: Path::new("/nonexistent"), state_dir: Path::new("/nonexistent/s"), git: Path::new("git"), accounts: &[] };
        match judge("Bash", command, manifest, &scene) {
            HostGuardDecision::Deny { what, line } => Some((what, line)),
            HostGuardDecision::Allow => None,
        }
    }

    /// 読んだ公開の segment の列（cwd `/w`・root `/root`）。
    fn published(line: &str) -> Vec<Published> {
        read(line, Path::new("/w"), Path::new("/root"))
    }

    /// 種別・群・動詞・`-R` の値を 1 語列にする（無い欄は `-`）。
    fn shape(line: &str) -> Vec<String> {
        let or = |found: &Option<String>| found.clone().unwrap_or_else(|| "-".to_owned());
        published(line).iter().map(|seg| format!("{:?} {} {} {}", seg.sort, or(&seg.group), or(&seg.verb), or(&seg.repo))).collect()
    }

    /// (a) 読みの表: 公開の segment は種別・群と動詞（api は群だけ）・`-R` の値つきで 1 つ、公開でない segment は列に無い。
    #[test]
    fn host_guard_publish_reads_the_closed_table() {
        for (line, want) in [
            ("git push origin main", "Git - push -"),
            ("git -C sub push", "Git - push -"),
            ("sudo git push", "Git - push -"),
            ("env A=1 git push", "Git - push -"),
            ("git push)", "Git - push -"),
            ("(cd sub && git push)", "Git - push -"),
            ("gh pr create", "Gh pr create -"),
            ("gh pr new", "Gh pr new -"),
            ("gh pr -R o/n create", "Gh pr create o/n"),
            ("gh -R o/n pr create", "Gh pr create o/n"),
            ("gh --repo=o/n issue comment 1", "Gh issue comment o/n"),
            ("gh issue reopen 1 -c x", "Gh issue reopen -"),
            ("gh release create v1", "Gh release create -"),
            ("gh gist new f", "Gh gist new -"),
            ("gh label create x", "Gh label create -"),
            ("gh repo new x", "Gh repo new -"),
            ("gh repo edit", "Gh repo edit -"),
            ("gh api -X PATCH repos/o/n -f a=b", "Api api - -"),
            ("gh api --method=patch repos/o/n", "Api api - -"),
            ("gh api repos/o/n/issues -f title=x", "Api api - -"),
            ("gh api repos/{owner}/{repo}/issues/1/comments -f body=x", "Api api - -"),
            ("gh api graphql -f query=mutation{addStar}", "Api api - -"),
            ("gh api graphql --input q.json", "Api api - -"),
        ] {
            assert_eq!(shape(line), [want], "{line}");
        }
        for line in [
            "git status", "git fetch", "gh pr list", "gh -R o/n pr list", "gh pr view 1", "gh api repos/o/n",
            "gh api -X GET repos/o/n -f a=b", "gh api graphql -f query={viewer{login}}", "gh run list", "echo git push",
        ] {
            assert!(published(line).is_empty(), "{line}: {:?}", shape(line));
        }
    }

    /// (b) 読んだ値: api の対象・method・欄、placeholder の字面、知らない flag、前置きの値、解けない dir。
    #[test]
    fn host_guard_publish_reads_the_values() {
        let line = "gh api -H 'Accept: x' -XPATCH https://api.github.com/repos/o/n/?a=1 -fprivate=false";
        let api = published(line).first().and_then(|seg| seg.api.clone()).unwrap_or_default();
        assert_eq!(api.target.as_deref(), Some("repos/o/n"), "{line}");
        assert_eq!(api.method.as_deref(), Some("PATCH"), "{line}");
        assert_eq!(api.fields, [("-f".to_owned(), "private".to_owned(), "false".to_owned())], "{line}");
        let placeholder = published("gh api repos/{owner}/{repo}/issues/1/comments -f body=x");
        let target = placeholder.first().and_then(|seg| seg.api.as_ref()).and_then(|api| api.target.clone());
        assert_eq!(target.as_deref(), Some("repos/{owner}/{repo}/issues/1/comments"), "placeholder は字面のまま");
        let slurp = published("gh api --slurp repos/o/n -f a=b");
        assert_eq!(slurp.iter().map(|seg| (seg.unknown_flag, seg.api.as_ref().and_then(|api| api.target.clone()))).collect::<Vec<_>>(), [(true, None)]);
        let prefixed = published("GH_REPO=o/n GH_HOST=h gh pr create");
        assert_eq!(prefixed.iter().map(|seg| (seg.gh_repo.as_deref(), seg.gh_host.as_deref())).collect::<Vec<_>>(), [(Some("o/n"), Some("h"))]);
        let moved = published("cd \"$D\" && git push");
        assert_eq!(moved.iter().map(|seg| (seg.dir.clone(), seg.resolved)).collect::<Vec<_>>(), [(PathBuf::from("/root"), false)]);
        let literal = published("cd /d && git -C sub push origin main");
        assert_eq!(literal.iter().map(|seg| (seg.dir.clone(), seg.resolved, seg.rest.clone())).collect::<Vec<_>>(), [(PathBuf::from("/d/sub"), true, vec!["origin".to_owned(), "main".to_owned()])]);
    }

    /// (c) 行: 行の無い manifest で公開の segment は hit no-row（経路は publish の経路）、公開の segment の無い command は通す。
    /// 行が在れば enabled を問わず通す。
    #[test]
    fn host_guard_publish_without_the_row_denies_only_publish_segments() {
        let bare = manifest("");
        for line in ["git push origin main", "gh pr create", "gh repo new x"] {
            let (what, text) = denied(line, &bare).unwrap_or_else(|| panic!("{line} は断る"));
            assert_eq!(what, "host-guard-deny publish", "{line}");
            let want = format!("{NAME}: host-guard deny kind=publish hit=no-row row=host_guard.publish ruling=- — {}", Kind::Publish.route());
            assert_eq!(text, want, "{line}");
        }
        for line in ["ls", "git status", "gh pr list", "cd \"$D\" && ls"] {
            assert_eq!(denied(line, &bare), None, "{line}");
        }
        for enabled in [true, false] {
            let with = manifest(&row("host_guard.publish", "HostGuardPublish", "\"form repo-name\"", enabled));
            assert_eq!(denied("git push origin main", &with), None, "enabled={enabled}");
        }
    }

    /// (d) 判定の順: publish の行を持たず語列の行を持つ manifest で `git push --force origin main` は kind git。
    #[test]
    fn host_guard_publish_comes_after_the_git_kind() {
        let found = denied("git push --force origin main", &manifest("")).map(|(what, _)| what);
        assert_eq!(found.as_deref(), Some("host-guard-deny git"), "publish が先に回れば no-row");
    }

    /// (e) 要素の読み手: 4 記号の form と 64 字小文字の exclude は受理し、綴り違い・同じ記号 2 回・63 字と大文字の digest・
    /// 同じ digest 2 回・札の外の要素は拒む。
    #[test]
    fn host_guard_publish_elements_accept_the_four_forms_and_digests() {
        let digest = "0123456789abcdef".repeat(4);
        let mut good: Vec<String> = FORMS.iter().map(|form| format!("form {}", form.as_str())).collect();
        good.push(format!("exclude {digest}"));
        let read = elements(&good).unwrap_or_else(|why| panic!("受理される: {why}"));
        assert_eq!(read.forms, [Form::RepoName, Form::ObjectId, Form::TrackedPath, Form::LedgerId], "宣言順の 4 記号");
        assert_eq!(read.excludes, std::slice::from_ref(&digest), "digest");
        let short = digest.get(1..).unwrap_or_default().to_owned();
        let twice = |one: String| vec![one.clone(), one];
        for bad in [
            vec!["form repo_name".to_owned()], twice("form repo-name".to_owned()), vec![format!("exclude {short}")],
            vec![format!("exclude {}", digest.to_ascii_uppercase())], twice(format!("exclude {digest}")),
            vec!["repo-name".to_owned()], vec!["scan repo-name".to_owned()],
        ] {
            let why = elements(&bad).err().unwrap_or_else(|| panic!("{bad:?} は拒む"));
            assert!(why.starts_with("要素 "), "{bad:?}: {why}");
        }
    }
}
