//! host-guard の 6 つ目の種類 publish の土台（設計 docs/design/vessel-hook.md §16 行 j・ADR-0078・SRS FR80 / AC50 / NFR4）。
//!
//! 公開の segment の読み（[`read`]・pure な 1 関数）と、rules 行 host_guard.publish の要素の読み手（[`elements`]）と、字面で
//! 読めない segment の印の読み（[`marked`]・§17 行 k）と、読めた segment の解けない形（[`Hole`]・§17 行 k2・行 k3 が全ての
//! 語へ広げる）と、判定
//! （[`judge`]: 行が無い・列でない周は no-row、enabled の行で印か形を持つ segment は unresolved）を持つ。全履歴・照合・配線は
//! 後続の行が足す。本 module は子 process を撃たない。

use super::{root_of, verb_of, Kind, Refusal, Subject, PUBLISH_ROW, UNRESOLVED};
use crate::hook::ledger_guard::{is_assignment, segments};
use crate::hook::live_row::{git_segment, walked, Walked};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::{Path, PathBuf};

pub mod history;
pub mod probe;
pub mod scan;

/// 識別子の形の要素の札。
const FORM: &str = "form";
/// 除外の要素の札（置けない要素の頭の語・除外は host の面の表が持つ）。
const EXCLUDE: &str = "exclude";
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
/// 細かい語を割る shell の区切りの字（空白と `$` の前と `:-` `:=` `:+` `:?` でも割る）。
const BREAKS: [char; 10] = [';', '&', '|', '(', ')', '{', '}', '<', '>', '`'];
/// 付け替えの env の名（launcher が剥いだ語と前の segment の代入で数える・前置きの値は行 j が読む）。
const REDIRECT_ENV: [&str; 4] = ["GIT_DIR", "GIT_WORK_TREE", "GH_REPO", "GH_HOST"];
/// 行き先を付け替える git の設定の接頭辞（小文字で比べる）。
const REDIRECT_CONFIG: [&str; 5] = ["remote.", "url.", "include.", "includeif.", "branch."];
/// 後ろの segment へ env を渡す頭の語。
const EXPORTS: [&str; 3] = ["export", "declare", "typeset"];
/// 移動の語（頭の語でない周だけ後ろの公開の segment を dir にする・popd は頭の語でも）。
const MOVES: [&str; 2] = ["cd", "pushd"];
/// 戻る移動の語。
const POPD: &str = "popd";
/// 本文・題・説明・comment の flag（api でない gh・値の字を読む）。
const BODY: [&str; 12] = ["--body", "-b", "--title", "-t", "--subject", "--notes", "-n", "--desc", "--description", "-d", "--comment", "-c"];
/// 本文の file の flag（api でない gh）。
const BODY_FILE: [&str; 3] = ["--body-file", "--notes-file", "-F"];
/// 動詞によって値を取らない flag（値の読みは次の 1 語だけを検査する・gist の create の `-d` は `--desc`）。
const EITHER: [&str; 3] = ["-d", "-c", "--comment"];
/// gist の create の値を取る flag（値は file の語に数えない）。
const GIST_VALUED: [&str; 4] = ["-d", "--desc", "-f", "--filename"];
/// api でない gh の editor の flag（editor が本文を書く・行 k3）。
const EDITOR: [&str; 2] = ["-e", "--editor"];
/// `--mirror`（`--m` 以上の長さの接頭辞も・git は long option の一意な接頭辞を受ける・行 k3）。
const MIRROR: &str = "--mirror";
/// gh が埋める api の対象の placeholder の成分。
const PLACEHOLDERS: [&str; 3] = ["{owner}", "{repo}", "{branch}"];
/// 読める heredoc の語を細かい語の割りから外す頭の語（どれも引数を command として撃たない）。
const HEREDOC_HEADS: [&str; 4] = ["git", "gh", "bd", "bdw"];
/// 解けない segment の経路。
const REWRITE: &str = "解ける形で書き直す（git / gh を包まずに頭の語に置く・ref と remote と dir と -R と可視性の欄は literal・本文は file〔--body-file か api の -F k=@file〕か区切りを引用した heredoc で渡す）";

/// 締め切りを越えた断りの経路。
const RETRY: &str = "撃ち直す（公開の前の問いが締め切りを越えたので断る・同じ command をもう 1 度撃つ）";
/// 読む上限を越えた断りの経路。
const SPLIT: &str = "分けて出す（出ていく字面が読む上限を越えたので断る・push を小さな単位に分けて出す）";
/// 隣の識別子を持つ断りの経路。
const REDACT: &str = "識別子を消し「隣の project」と件数に言い換えて出し直す（出ていく字面に隣の private repo の識別子が在るので断る・本文と message と path から消す）";

/// 断りの理由（閉じた 6 値・宣言順・設計 §17 形 1・§18 形 5・§22 行 n 形 4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 行が無い・列でない。
    NoRow,
    /// 全履歴を走査せずに出す command（hit は `full-history:<種別>`・[`history`]）。
    FullHistory,
    /// 字面で解けない（hit は `unresolved:<印の語>`）。
    Unresolved,
    /// 締め切りを越えた（hit は `deadline`・[`probe`]）。
    Deadline,
    /// 読む上限を越えた（hit は `oversize`・[`probe`]）。
    Oversize,
    /// 隣の識別子が出ていく（hit は `identifier`）。
    Identifier,
}

/// [`Reason`] の全 variant（宣言順）。
pub const REASONS: &[Reason] = &[Reason::NoRow, Reason::FullHistory, Reason::Unresolved, Reason::Deadline, Reason::Oversize, Reason::Identifier];

impl Reason {
    /// hit の頭の語と経路。
    pub fn parts(self) -> (&'static str, &'static str) {
        match self {
            Self::NoRow => ("no-row", Kind::Publish.route()),
            Self::FullHistory => ("full-history", history::ROUTE),
            Self::Unresolved => ("unresolved", REWRITE),
            Self::Deadline => ("deadline", RETRY),
            Self::Oversize => ("oversize", SPLIT),
            Self::Identifier => ("identifier", REDACT),
        }
    }
}

/// 字面で読めない segment の印（閉じた 5 値・宣言順が断りの順・設計 §17 形 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// 頭の語が git / gh でない segment の中の git push と gh の公開の群。
    Wrapped,
    /// 変数の頭・git の動詞・gh の群と動詞・api の method が解けない。
    Verb,
    /// 知らない flag を持つ公開の segment。
    Shape,
    /// 解けない dir か、前の segment の読み手が辿らない移動の後ろ。
    Dir,
    /// env・前の segment の代入・git の設定による行き先の付け替え。
    Redirect,
}

/// [`Mark`] の全 variant（宣言順）。
pub const MARKS: &[Mark] = &[Mark::Wrapped, Mark::Verb, Mark::Shape, Mark::Dir, Mark::Redirect];

impl Mark {
    /// hit の `unresolved:` の後ろの語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wrapped => "wrapped",
            Self::Verb => "verb",
            Self::Shape => "shape",
            Self::Dir => "dir",
            Self::Redirect => "redirect",
        }
    }
}

/// 読めた公開の segment の解けない形（閉じた 5 値・宣言順が断りの順・印の後ろ・設計 §17 形 1 行 k2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hole {
    /// push の相手・`-R`・gh の動詞の後ろの語・前置きの値・api の対象か host が解けない字を持つ。
    VariableRef,
    /// `git push --mirror`。
    Mirror,
    /// 本文を標準入力から読む。
    StdinBody,
    /// repo・repo を作る口・graphql の書きが可視性の欄を file で渡すか、欄の key が解けない字を持つ。
    ApiVisibilityFile,
    /// 本文・題の値か本文の file の path が字面で読めない。
    UnreadableBody,
}

/// [`Hole`] の全 variant（宣言順）。
pub const HOLES: &[Hole] = &[Hole::VariableRef, Hole::Mirror, Hole::StdinBody, Hole::ApiVisibilityFile, Hole::UnreadableBody];

impl Hole {
    /// hit の `unresolved:` の後ろの語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VariableRef => "variable-ref",
            Self::Mirror => "mirror",
            Self::StdinBody => "stdin-body",
            Self::ApiVisibilityFile => "api-visibility-file",
            Self::UnreadableBody => "unreadable-body",
        }
    }
}

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

/// 行の値を読んだもの（形の記号の列・manifest の順）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Elements {
    /// 走査する識別子の形。
    pub forms: Vec<Form>,
}

/// 行の要素の読み手（**1 本**・`names_are_known` の arm が呼ぶ）: 各要素は `form <記号>` だけ。綴り違いの記号・同じ記号 2 回・
/// 札の外の要素を、要素を名指す理由で拒む。頭の語が `exclude` の要素は、除外が host の面の表 `[[publish-exclusion]]` に在ると
/// 名指して拒む（ADR-0093・NFR4）。
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
            [EXCLUDE, ..] => "除外の要素は置けない（除外は host の面の [[publish-exclusion]]・ADR-0093）".to_owned(),
            _ => format!("札が {FORM} <記号> の形でない"),
        };
        return Err(format!("要素 {value:?} の{reason}"));
    }
    Ok(found)
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
    /// `--hostname` の値。
    pub hostname: Option<String>,
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
            "--hostname" => self.hostname = Some(value),
            _ => {}
        }
    }

    /// `-F` の欄の値の `@` の後ろの path。
    fn files(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().filter(|(flag, _, _)| flag == "-F").filter_map(|(_, _, value)| value.strip_prefix('@'))
    }

    /// api-visibility-file: 対象が repo そのもの（`repos/<o>/<n>`）か repo を作る口（`user/repos`・`orgs/<org>/repos`）の書きが
    /// `visibility` か `private` の欄を `-F` の `@` で読むか `--input` を持つ、graphql の書きが `query` の欄を `-F` の `@` で読むか
    /// `--input` を持つ、またはこの 3 つの対象の書きの欄の key が解けない字を持つ。
    fn visibility_file(&self) -> bool {
        let parts: Vec<&str> = self.target.as_deref().unwrap_or_default().split('/').collect();
        let repo = matches!(parts.as_slice(), ["repos", _, _] | ["user", "repos"] | ["orgs", _, "repos"]);
        let keys: &[&str] = if parts == [GRAPHQL] { &["query"] } else { &["visibility", "private"] };
        let filed = |(flag, key, value): &(String, String, String)| flag == "-F" && keys.contains(&key.as_str()) && value.starts_with('@');
        let loose = |(_, key, _): &(String, String, String)| key.contains(UNRESOLVED);
        (repo || parts == [GRAPHQL]) && self.writes() && (self.input.is_some() || self.fields.iter().any(|field| filed(field) || loose(field)))
    }

    /// 書きか: graphql は `query` の欄が語 mutation を持つか、`-F` の `query` の値が `@` で始まるか、`--input` を持つ周と、欄の
    /// key が解けない字を持つか `query` の値が `$` か `` ` `` を持つ（読める heredoc を除く）周（行 k3）。他の対象は method が
    /// 書きの 4 つか、method が無く欄か `--input` を持つ周（gh が POST にする形）。
    fn writes(&self) -> bool {
        if self.target.as_deref() == Some(GRAPHQL) {
            let query = |(flag, key, value): &(String, String, String)| {
                key == "query" && (value.split(|found: char| !found.is_ascii_alphanumeric()).any(|word| word == "mutation")
                    || (flag == "-F" && value.starts_with('@'))
                    || (value.contains(['$', '`']) && !readable(value)))
            };
            let loose = |(_, key, _): &(String, String, String)| key.contains(UNRESOLVED);
            return self.input.is_some() || self.fields.iter().any(|field| query(field) || loose(field));
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
    walked(command, cwd).iter().filter_map(|seg| one(seg, root)).collect()
}

/// 辿った segment 1 つの読み（[`read`] と [`marked`] が呼ぶ 1 本）。
fn one(seg: &Walked, root: &Path) -> Option<Published> {
    match trimmed(seg.words.first()?) {
        "git" => pushed(seg, root),
        "gh" => gh(seg, root),
        _ => None,
    }
}

/// 公開の segment 1 つ（行 j の読みか印を持つ segment・設計 §17 形 2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    /// 行 j の読み（頭の語が git / gh の公開の segment だけ）。
    pub read: Option<Published>,
    /// 印（宣言順）。
    pub marks: Vec<Mark>,
    /// 解けない形（宣言順・行 j の読みの在る segment だけ）。
    pub holes: Vec<Hole>,
}

/// 印を読む segment 1 つの材料。
struct Piece<'a> {
    /// segment の語（前置きと launcher を含む）。
    words: &'a [String],
    /// 細かい語。
    fine: Vec<String>,
    /// 辿り（前置きだけの segment は `None`）。
    walk: Option<&'a Walked>,
    /// 頭の語（比べる語・前置きだけの segment は空）。
    head: &'a str,
    /// 行 j の読み。
    read: Option<Published>,
}

/// 同じ command 行の前の segment が残したもの（読み手が辿らない移動・後ろへ渡る付け替えの代入）。
#[derive(Debug, Clone, Copy, Default)]
struct Before {
    /// 頭の語でない `cd` / `pushd` か `popd` が在った。
    moved: bool,
    /// export の類か前置きだけの segment が付け替えの env に代入した。
    exported: bool,
}

impl Before {
    /// segment 1 つを足した後。
    fn after(self, piece: &Piece) -> Self {
        let moved = piece.fine.iter().any(|word| word == POPD || (!MOVES.contains(&piece.head) && MOVES.contains(&word.as_str())));
        let exporting = piece.walk.is_none() || EXPORTS.contains(&piece.head);
        let exported = exporting && piece.words.iter().any(|word| assigns(word, true));
        Self { moved: self.moved || moved, exported: self.exported || exported }
    }
}

/// 印の読み（**pure な 1 関数**・設計 §17 形 2）: [`segments`] の各 segment を [`walked`] の辿りと対にし（前置きだけの segment は
/// 辿りを持たない）、行 j の読みか印を持つ segment だけを command 行の順に返す。頭の語が [`HEREDOC_HEADS`] の segment は
/// 読める heredoc の語（`--flag=` の頭を落として測る）を細かい語に割らない（行 k2 形 2）。子 process は撃たない。
pub fn marked(command: &str, cwd: &Path, root: &Path) -> Vec<Marked> {
    let walks = walked(command, cwd);
    let mut walks = walks.iter();
    let (mut before, mut found) = (Before::default(), Vec::new());
    for words in segments(command) {
        let lead = words.iter().take_while(|word| is_assignment(word)).count();
        let walk = verb_of(words.get(lead..).unwrap_or_default()).and_then(|_| walks.next());
        let head = walk.and_then(|seg| seg.words.first()).map_or("", |word| trimmed(word));
        let kept = |word: &&String| !(HEREDOC_HEADS.contains(&head) && readable(unflagged(word)));
        let fine = words.iter().filter(kept).flat_map(|word| fine(word)).collect();
        let piece = Piece { words: &words, fine, walk, head, read: walk.and_then(|seg| one(seg, root)) };
        let marks: Vec<Mark> = MARKS.iter().copied().filter(|mark| has(*mark, &piece, before, root)).collect();
        before = before.after(&piece);
        let holes = piece.read.as_ref().map(holes_of).unwrap_or_default();
        if piece.read.is_some() || !marks.is_empty() {
            found.push(Marked { read: piece.read, marks, holes });
        }
    }
    found
}

/// 読める heredoc の形か（**1 関数**・設計 §17 形 2 行 k2）: 値が `$(cat <<'D'` か `$(cat <<"D"`（D は空白と引用符を持たない
/// 1 語）と改行で始まり、`D` だけの行がちょうど 1 つで、その後ろが `)` だけ（間の改行は許す）。
fn readable(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("$(cat <<") else {
        return false;
    };
    let Some(quote) = rest.chars().next().filter(|found| matches!(found, '\'' | '"')) else {
        return false;
    };
    let Some((delimiter, body)) = rest.get(1..).and_then(|after| after.split_once(quote)) else {
        return false;
    };
    let bad = |found: char| found.is_whitespace() || matches!(found, '\'' | '"');
    let lines: Vec<&str> = body.strip_prefix('\n').map(|body| body.split('\n').collect()).unwrap_or_default();
    let ends: Vec<usize> = lines.iter().enumerate().filter(|(_, line)| **line == delimiter).map(|(at, _)| at).collect();
    let closed = |at: usize| lines.get(at.saturating_add(1)..).unwrap_or_default().concat() == ")";
    !delimiter.is_empty() && !delimiter.contains(bad) && matches!(ends.as_slice(), [at] if closed(*at))
}

/// `--flag=` の頭を落とした字（flag でない語はそのまま）。
fn unflagged(word: &str) -> &str {
    word.strip_prefix('-').and_then(|_| word.split_once('=')).map_or(word, |(_, value)| value)
}

/// api でない gh の動詞の後ろの語の読み（設計 §17 形 1 行 k2・動詞ごとの表は持たない）。
#[derive(Debug, Default)]
struct Words {
    /// variable-ref が数える語（本文と題と本文の file の flag とその値と gist の create の file の語を除く全て）。
    counted: Vec<String>,
    /// 本文・題の値（3 つの flag は値としての読みの次の 1 語）。
    bodies: Vec<String>,
    /// 本文の file の値と gist の create の file の語。
    files: Vec<String>,
    /// gist の create の file の語の数（gist の create でなければ `None`）。
    gist: Option<usize>,
}

impl Words {
    /// 位置の語 1 つ（3 つの flag の直後の読める heredoc は variable-ref が数えない）。
    fn positional(&mut self, word: &str, after_either: bool) {
        if let Some(count) = self.gist.as_mut() {
            *count = count.saturating_add(1);
            self.files.push(word.to_owned());
        } else if !(after_either && readable(word)) {
            self.counted.push(word.to_owned());
        }
    }

    /// 本文と題の値を除く動詞の後ろの全ての語の字（数える語とその `--flag=` と続け書きの値・本文の file の値・行 k3）。
    fn spread(&self) -> impl Iterator<Item = &str> {
        let inline = self.counted.iter().filter_map(|word| flag_of(word).and_then(|(_, value)| value));
        self.counted.iter().map(String::as_str).chain(inline).chain(self.files.iter().map(String::as_str))
    }

    /// editor の flag を持つか。
    fn edited(&self) -> bool {
        self.counted.iter().any(|word| flag_of(word).is_some_and(|(name, _)| EDITOR.contains(&name)))
    }
}

/// 標準入力の形（**1 つの読み**・行 k3）: `-` と、`//` と `/./` を `/` に畳んだ path が `/dev/` か `/proc/` で始まるもの。
pub(crate) fn is_stdin(path: &str) -> bool {
    let mut folded = path.to_owned();
    loop {
        let next = folded.replace("//", "/").replace("/./", "/");
        if next == folded {
            break;
        }
        folded = next;
    }
    path == "-" || folded.starts_with("/dev/") || folded.starts_with("/proc/")
}

/// 字面で読めない path か: `$` か `` ` `` を持つか、`<(` / `>(` を含む（bash は語の途中も展開する・行 k3）。
pub(crate) fn is_loose_path(path: &str) -> bool {
    path.contains(['$', '`']) || path.contains("<(") || path.contains(">(")
}

/// git push の後ろの語が `--mirror` か、`--m` 以上の長さのその接頭辞か。
fn is_mirror(word: &str) -> bool {
    let word = trimmed(word);
    word.len() >= "--m".len() && MIRROR.starts_with(word)
}

/// api でない gh の segment の動詞の後ろの語を読む（値は `--flag 値`・`--flag=値`・続け書きを [`flag_of`] で同じに読む）。
fn words_of(found: &Published) -> Words {
    let gist = found.group.as_deref() == Some("gist") && matches!(found.verb.as_deref(), Some("create" | "new"));
    let mut read = Words { gist: gist.then_some(0), ..Words::default() };
    let (rest, mut at, mut either) = (&found.rest, 0_usize, false);
    while let Some(word) = rest.get(at) {
        at = at.saturating_add(1);
        let after_either = std::mem::take(&mut either);
        let Some((flag, inline)) = flag_of(word) else {
            read.positional(word, after_either);
            continue;
        };
        if EITHER.contains(&flag) && !gist && inline.is_none() {
            read.bodies.extend(rest.get(at).cloned());
            either = true;
            continue;
        }
        if !(BODY.contains(&flag) || BODY_FILE.contains(&flag) || (gist && GIST_VALUED.contains(&flag))) {
            read.counted.push(word.clone());
            continue;
        }
        let value = inline.map(str::to_owned).or_else(|| {
            let next = rest.get(at).cloned();
            at = at.saturating_add(usize::from(next.is_some()));
            next
        });
        let into = if BODY_FILE.contains(&flag) { &mut read.files } else if BODY.contains(&flag) { &mut read.bodies } else { &mut read.counted };
        into.extend(value);
    }
    read
}

/// 読めた segment 1 つの解けない形（宣言順）。
fn holes_of(found: &Published) -> Vec<Hole> {
    let words = if found.sort == Sort::Gh { words_of(found) } else { Words::default() };
    HOLES.iter().copied().filter(|hole| has_hole(*hole, found, &words)).collect()
}

/// segment 1 つが解けない形を持つか（1 形 1 arm）。
fn has_hole(hole: Hole, found: &Published, words: &Words) -> bool {
    match (hole, found.api.as_ref()) {
        (Hole::VariableRef, api) => variable_ref(found, api, words),
        (Hole::Mirror, _) => found.sort == Sort::Git && found.rest.iter().any(|word| is_mirror(word)),
        (Hole::StdinBody, Some(api)) => api.input.as_deref().is_some_and(is_stdin) || api.files().any(is_stdin),
        (Hole::StdinBody, None) => words.spread().any(is_stdin) || words.gist == Some(0),
        (Hole::ApiVisibilityFile, api) => api.is_some_and(Api::visibility_file),
        (Hole::UnreadableBody, api) => unreadable_body(api, words),
    }
}

/// variable-ref: push の後ろの全ての語（flag の語を含む・行 k3）・動詞の前の `-R`・前置きの `GH_REPO=` / `GH_HOST=`・api でない
/// gh の数える語・api の対象の成分（placeholder を除く）と `--hostname` が解けない字を持つ。
fn variable_ref(found: &Published, api: Option<&Api>, words: &Words) -> bool {
    let loose = |word: &String| word.contains(UNRESOLVED);
    let part = |part: &str| !PLACEHOLDERS.contains(&part) && part.contains(UNRESOLVED);
    let target = api.and_then(|api| api.target.as_deref()).is_some_and(|target| target.split('/').any(part));
    let pushed = found.sort == Sort::Git && found.rest.iter().any(loose);
    [&found.repo, &found.gh_repo, &found.gh_host].into_iter().flatten().any(loose)
        || api.is_some_and(|api| api.hostname.iter().any(loose))
        || target
        || pushed
        || words.counted.iter().any(loose)
}

/// unreadable-body: 本文・題の値と api の欄の語の全体（key と値）が `$` か `` ` `` を持つ（読める heredoc の値を除く）か、本文の
/// file の path が字面で読めない（[`is_loose_path`]）か、api でない gh の動詞の後ろの本文と題の値を除く語が `<(` / `>(` を含むか
/// editor の flag を持つ（行 k3）。
fn unreadable_body(api: Option<&Api>, words: &Words) -> bool {
    let text = |value: &String| value.contains(['$', '`']) && !readable(value);
    let field = |(_, key, value): &(String, String, String)| key.contains(['$', '`']) || text(value);
    match api {
        Some(api) => api.fields.iter().any(field) || api.input.as_deref().is_some_and(is_loose_path) || api.files().any(is_loose_path),
        None => {
            let process = |word: &str| word.contains("<(") || word.contains(">(");
            words.bodies.iter().any(text) || words.files.iter().any(|file| is_loose_path(file)) || words.spread().any(process) || words.edited()
        }
    }
}

/// segment 1 つが印を持つか（1 印 1 arm）。
fn has(mark: Mark, piece: &Piece, before: Before, root: &Path) -> bool {
    let read = piece.read.as_ref();
    match mark {
        Mark::Wrapped => !matches!(piece.head, "git" | "gh") && follows(&piece.fine),
        Mark::Verb => piece.walk.is_some_and(|seg| variable_verb(seg, piece.head, root)),
        Mark::Shape => read.is_some_and(|found| found.unknown_flag),
        Mark::Dir => read.is_some_and(|found| !found.resolved || before.moved),
        Mark::Redirect => read.is_some() && (before.exported || redirected(piece)),
    }
}

/// gh の公開の群の語か。
fn is_group(word: &str) -> bool {
    word == API || TABLE.iter().any(|(group, _)| *group == word)
}

/// 細かい語に `git` とその後ろの `push`、か `gh` とその後ろの公開の群の語が在るか（隣でなくてよい）。
fn follows(fine: &[String]) -> bool {
    fine.iter().enumerate().any(|(at, word)| {
        let after = fine.get(at.saturating_add(1)..).unwrap_or_default();
        match word.as_str() {
            "git" => after.iter().any(|next| next == PUSH),
            "gh" => after.iter().any(|next| is_group(next)),
            _ => false,
        }
    })
}

/// verb の印: 頭の語が `$` / `` ` `` を持ち頭の語の 2 つ目以後の片か後ろの語の細かい語に push か公開の群の語が在る・git の
/// 動詞・gh の群か動詞・api の method が解けない字を持つ（api の対象の placeholder は印でない）。
fn variable_verb(seg: &Walked, head: &str, root: &Path) -> bool {
    let rest = seg.words.get(1..).unwrap_or_default();
    if head.contains(['$', '`']) {
        let first = seg.words.first().map(|word| fine(word)).unwrap_or_default();
        let mut later = first.into_iter().skip(1).chain(rest.iter().flat_map(|word| fine(word)));
        return later.any(|word| word == PUSH || is_group(&word));
    }
    let unresolved = |word: &str| word.contains(UNRESOLVED);
    match head {
        "git" => git_segment(seg, root).is_some_and(|git| unresolved(trimmed(&git.verb))),
        "gh" => {
            let (picked, at, _, _) = leading(rest);
            let api_group = picked.first().is_some_and(|group| group == API);
            let method = api_group.then(|| api(rest.get(at..).unwrap_or_default()).0.method).flatten();
            picked.iter().chain(method.iter()).any(|word| unresolved(word))
        }
        _ => false,
    }
}

/// redirect の印（前の segment の代入を除く）: launcher が剥いだ語の付け替えの env・前置きか launcher が剥いだ語の git の設定の
/// file の env・git の動詞より前の `-c` / `--config-env=` の値。
fn redirected(piece: &Piece) -> bool {
    let Some(seg) = piece.walk else {
        return false;
    };
    let end = piece.words.len().saturating_sub(seg.words.len());
    let stripped = piece.words.get(seg.lead.len()..end).unwrap_or_default();
    stripped.iter().any(|word| assigns(word, true))
        || seg.lead.iter().any(|word| assigns(word, false))
        || (piece.head == "git" && configured(&seg.words))
}

/// 付け替えの代入か: 名が `GIT_CONFIG` で始まるか `HOME` か `XDG_CONFIG_HOME`、`env` なら [`REDIRECT_ENV`] も。
fn assigns(word: &str, env: bool) -> bool {
    word.split_once('=').filter(|_| is_assignment(word)).is_some_and(|(name, _)| {
        name.starts_with("GIT_CONFIG") || name == "HOME" || name == "XDG_CONFIG_HOME" || (env && REDIRECT_ENV.contains(&name))
    })
}

/// git の動詞より前の `-c` か `--config-env=` の値が行き先の設定の接頭辞で始まる（大小を問わない）か解けない字を持つか。
fn configured(words: &[String]) -> bool {
    let mut rest = words.iter().skip(1);
    while let Some(word) = rest.next() {
        let value = match word.as_str() {
            "-c" => rest.next().map(String::as_str),
            "-C" | "--namespace" | "--git-dir" | "--work-tree" => {
                rest.next();
                continue;
            }
            flag if flag.starts_with('-') => flag.strip_prefix("--config-env="),
            _ => return false,
        };
        let value = value.unwrap_or_default();
        let lower = value.to_ascii_lowercase();
        if value.contains(UNRESOLVED) || REDIRECT_CONFIG.iter().any(|prefix| lower.starts_with(prefix)) {
            return true;
        }
    }
    false
}

/// 語の細かい語（設計 §17 形 2）: `$(` か `` ` `` を持つ語と、空白を持たずに `<(` か `>(` を持つ語は [`pieces`] で割り、他の語は
/// 割らずに先頭の `(` `{` と末尾の `)` `}` `;` だけを落とす。どちらも片の先頭の `NAME=` を落とした basename（空の片は捨てる）。
fn fine(word: &str) -> Vec<String> {
    let substituted = word.contains("$(") || word.contains('`');
    let process = !word.contains(char::is_whitespace) && (word.contains("<(") || word.contains(">("));
    let parts = if substituted || process {
        pieces(word)
    } else {
        vec![word.trim_start_matches(['(', '{']).trim_end_matches([')', '}', ';'])]
    };
    parts
        .into_iter()
        .filter_map(|part| {
            let bare = if is_assignment(part) { part.split_once('=').map_or(part, |(_, value)| value) } else { part };
            let base = bare.rsplit('/').next().unwrap_or(bare);
            (!base.is_empty()).then(|| base.to_owned())
        })
        .collect()
}

/// 割る語を片に分ける: 空白と [`BREAKS`] と `:-` `:=` `:+` `:?` で割り、`$` の前でも割る。
fn pieces(word: &str) -> Vec<&str> {
    let (mut found, mut start) = (Vec::new(), 0_usize);
    for (at, found_char) in word.char_indices() {
        let next = at.saturating_add(found_char.len_utf8());
        let resume = if found_char.is_whitespace() || BREAKS.contains(&found_char) {
            Some(next)
        } else if found_char == '$' {
            Some(at)
        } else if found_char == ':' && word.get(next..).is_some_and(|rest| rest.starts_with(['-', '=', '+', '?'])) {
            Some(next.saturating_add(1))
        } else {
            None
        };
        if let Some(resume) = resume {
            found.extend(word.get(start..at));
            start = resume;
        }
    }
    found.extend(word.get(start..));
    found
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
/// （`--repo=<値>` と `-R<値>` の続け書きも・[`flag_of`] の読み）と、それ以外の flag を持つかを読む。位置は読んだ最後の語の次。
fn leading(rest: &[String]) -> (Vec<String>, usize, Option<String>, bool) {
    let (mut picked, mut at, mut repo, mut unknown) = (Vec::<String>::new(), 0_usize, None, false);
    while let Some(word) = rest.get(at) {
        at = at.saturating_add(1);
        if let Some((_, inline)) = flag_of(word).filter(|(name, _)| matches!(*name, "-R" | "--repo")) {
            repo = inline.map(str::to_owned).or_else(|| rest.get(at).cloned());
            at = at.saturating_add(usize::from(inline.is_none()));
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

/// flag の語を（flag の名・続け書きの値）に分ける（**1 つの読み手**・1 字の flag の続け書きの値の頭の `=` は 1 つ落とす＝
/// pflag の読み）。flag でない語は `None`。
pub(crate) fn flag_of(word: &str) -> Option<(&str, Option<&str>)> {
    if word.starts_with("--") && word.len() > 2 {
        return Some(word.split_once('=').map_or((word, None), |(name, value)| (name, Some(value))));
    }
    if !(word.starts_with('-') && word.len() > 1) {
        return None;
    }
    match (word.get(..2), word.get(2..)) {
        (Some(name), Some(value)) => Some((name, Some(value.strip_prefix('=').unwrap_or(value)).filter(|found| !found.is_empty()))),
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

/// 判定（設計 §17 形 3）: 公開の segment（[`marked`]）が 0 の周は行を読まずに通し、行が無い・列でない周は no-row、
/// `enabled = false` の周は通し、全履歴の段は segment の順に [`history::kind_of`] の先に当たった 1 つで断り（前の segment の印や形より
/// 先・§18）、解けない段は segment の順に各 segment の印 → 形（どちらも宣言順）の先に当たった 1 つで断る。印も形も無ければ通す
/// （走査の断りは後続の行）。子 process は撃たない。
pub(super) fn judge(kind: Kind, subject: &Subject, manifest: &Manifest) -> Option<Refusal> {
    let cwd = subject.scene.cwd;
    let root = root_of(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let found = marked(subject.command, cwd, &root);
    if found.is_empty() {
        return None;
    }
    let Some(row) = manifest.get(PUBLISH_ROW).filter(|row| matches!(row.value, RuleValue::List(_))) else {
        return Some(refused(kind, Reason::NoRow, None, "-".to_owned()));
    };
    if !row.enabled {
        return None;
    }
    if let Some(found) = found.iter().filter_map(|seg| seg.read.as_ref()).find_map(history::kind_of) {
        return Some(refused(kind, Reason::FullHistory, Some(found.as_str()), row.ruling.clone()));
    }
    let first = |seg: &Marked| seg.marks.first().map(|mark| mark.as_str()).or_else(|| seg.holes.first().map(|hole| hole.as_str()));
    let word = found.iter().find_map(first)?;
    Some(refused(kind, Reason::Unresolved, Some(word), row.ruling.clone()))
}

/// 理由の断り（hit は理由の頭の語か `<頭の語>:<印か形の語>`・経路は理由ごと）。
fn refused(kind: Kind, reason: Reason, word: Option<&str>, ruling: String) -> Refusal {
    let (head, route) = reason.parts();
    let hit = word.map_or_else(|| head.to_owned(), |word| format!("{head}:{word}"));
    Refusal { kind, hit, row: PUBLISH_ROW, ruling, route }
}

#[cfg(test)]
mod tests {
    // flip-check: retroactive s2-07l.738.16
    use super::super::{judge, HostGuardDecision, Kind, Scene};
    use super::probe::{budget, DEADLINE_ROW, READ_ROW};
    use super::{elements, marked, read, Form, Published, Reason, FORMS, MARKS, REASONS};
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

    /// (e) 要素の読み手: 4 記号の form だけを受け、頭の語が exclude の要素（digest の形でも長さに依らず）は
    /// `[[publish-exclusion]]` と ADR-0093 を名指す理由で拒み、綴り違い・同じ記号 2 回・札の外の要素も拒む（理由は form の形だけを名指す）。
    #[test]
    fn publish_exclusion_elements_take_only_the_form_symbols() {
        let good: Vec<String> = FORMS.iter().map(|form| format!("form {}", form.as_str())).collect();
        let read = elements(&good).unwrap_or_else(|why| panic!("受理される: {why}"));
        assert_eq!(read.forms, [Form::RepoName, Form::ObjectId, Form::TrackedPath, Form::LedgerId], "宣言順の 4 記号");
        let digest = "0123456789abcdef".repeat(4);
        let short = digest.get(1..).unwrap_or_default().to_owned();
        for bad in [
            format!("exclude {digest}"), format!("exclude {short}"), format!("exclude {}", digest.to_ascii_uppercase()),
            "exclude proj-accent".to_owned(), "exclude".to_owned(),
        ] {
            let why = elements(std::slice::from_ref(&bad)).err().unwrap_or_else(|| panic!("{bad:?} は拒む"));
            assert!(why.starts_with("要素 ") && why.contains("[[publish-exclusion]]") && why.contains("ADR-0093"), "{bad:?}: {why}");
        }
        let twice = |one: &str| vec![one.to_owned(), one.to_owned()];
        for bad in [vec!["form repo_name".to_owned()], twice("form repo-name"), vec!["repo-name".to_owned()], vec!["scan repo-name".to_owned()]] {
            let why = elements(&bad).err().unwrap_or_else(|| panic!("{bad:?} は拒む"));
            assert!(why.starts_with("要素 ") && !why.contains("[[publish-exclusion]]"), "{bad:?}: {why}");
        }
        let odd = elements(&["scan repo-name".to_owned()]).err().unwrap_or_default();
        assert!(odd.contains("form <記号>") && !odd.contains("exclude <digest>"), "札の外の理由は form だけを名指す: {odd}");
    }

    /// 公開の segment ごとの印の語（cwd `/w`・root `/root`）。
    fn marks(line: &str) -> Vec<Vec<&'static str>> {
        marked(line, Path::new("/w"), Path::new("/root")).iter().map(|seg| seg.marks.iter().map(|mark| mark.as_str()).collect()).collect()
    }

    /// 行 k (a) 印の表: 5 値がそれぞれ先頭の印として当たり、印の無い公開の segment と公開の segment に加わらない command を分ける。
    #[test]
    fn publish_marks_are_the_closed_five() {
        let table: [(&str, &[&str]); 5] = [
            ("wrapped", &[
                "if git push origin main; then echo ok; fi", "PR=$(gh pr create --fill)", "(git push)", "echo \"$(git push)\"",
                "time git -C sub push", "flock /tmp/l git push", "nohup gh repo edit o/n --visibility public", "echo git push",
                "cat <(git push origin main)", "tee >(gh pr create --fill) < /dev/null", "echo \"$(cd sub&&git push origin main)\"",
                "echo \"$(true;git push origin main)\"", ": ${X:-$(git push origin main)}", "x=$(true)$(git push origin main)",
                "\"$(git push origin main)\"", "\"`git push origin main`\"",
            ]),
            ("verb", &["\"$GIT\" push origin main", "git \"$V\" origin main", "gh \"$G\" create", "gh api -X \"$M\" repos/o/n -f a=b"]),
            ("shape", &["gh api --slurp repos/o/n -f a=b"]),
            ("dir", &["cd \"$D\" && git push", "(cd sub && git push)", "pushd a; popd; git push", "(cd sub; ls; git push origin main)"]),
            ("redirect", &[
                "env GIT_DIR=../p/.git git push origin main", "export GH_REPO=o/n && gh pr create", "GH_REPO=o/n; gh pr create",
                "git -c remote.origin.pushurl=u push origin main", "GIT_CONFIG_GLOBAL=/tmp/c git push", "HOME=/tmp/h git push origin main",
                "export GH_REPO=o/n; ls; gh pr create", "git -c branch.main.pushRemote=u push", "git -c \"$CFG\" push origin main",
                "git --config-env=\"$E\" push origin main",
            ]),
        ];
        for (want, lines) in table {
            for line in lines {
                let first = marks(line).into_iter().find_map(|seg| seg.first().copied());
                assert_eq!(first, Some(want), "{line}: {:?}", marks(line));
            }
        }
        for line in ["git push origin main", "gh api repos/{owner}/{repo}/issues/1/comments -f body=x", "GH_REPO=o/n gh pr view 1; git push origin main"] {
            let found = marks(line);
            assert!(found.len() == 1 && found.iter().all(Vec::is_empty), "印の無い公開の segment 1 つ: {line}: {found:?}");
        }
        for line in [
            "cd \"$D\" && ls", "for f in a; do echo \"$f\"; done", "git log --grep push", "grep -n \"git push\" x.md",
            "scripts/bdw update s2-x --append-notes \"gh pr merge 1 で着地\"", "grep -E \"git|push\" x.md", "git status",
        ] {
            assert!(marks(line).is_empty(), "公開の segment に加わらない: {line}: {:?}", marks(line));
        }
    }

    /// 行 k (b) 判定の順: 行の無い manifest は no-row、enabled の行は segment の順に先頭の印で unresolved、`enabled = false` は通す。
    #[test]
    fn publish_marks_deny_after_the_row_only_when_enabled() {
        let line = |hit: &str, ruling: &str, route: &str| format!("{NAME}: host-guard deny kind=publish hit={hit} row=host_guard.publish ruling={ruling} — {route}");
        let bare = denied("(git push)", &manifest("")).map(|(_, text)| text);
        assert_eq!(bare, Some(line("no-row", "-", Kind::Publish.route())), "行の無い manifest");
        let cases = [("(git push)", "unresolved:wrapped"), ("env GIT_DIR=../p/.git git push origin main; (git push)", "unresolved:redirect")];
        for enabled in [true, false] {
            let with = manifest(&row("host_guard.publish", "HostGuardPublish", "\"form repo-name\"", enabled));
            for (command, hit) in cases {
                let want = enabled.then(|| ("host-guard-deny publish".to_owned(), line(hit, "r", Reason::Unresolved.parts().1)));
                assert_eq!(denied(command, &with), want, "{command} enabled={enabled}");
            }
        }
        let with = manifest(&row("host_guard.publish", "HostGuardPublish", "\"form repo-name\"", true));
        assert_eq!(denied("git push origin main", &with), None, "解ける push は通す");
        let force = denied("git push --force origin main", &with).map(|(_, text)| text);
        let want = format!("{NAME}: host-guard deny kind=git hit=git push --force row=host_guard.git ruling=r — {}", Kind::Git.route());
        assert_eq!(force, Some(want), "git の種類の経路は不変");
    }

    /// 行 k (c) 理由と印の閉じた列と、理由ごとの経路（no-row は publish の種類の経路・unresolved は書き直しの経路）。
    #[test]
    fn publish_marks_routes_are_one_per_reason() {
        let heads = ["no-row", "full-history", "unresolved", "deadline", "oversize", "identifier"];
        assert_eq!(REASONS.iter().map(|reason| reason.parts().0).collect::<Vec<_>>(), heads, "理由の宣言順");
        let routes: Vec<&str> = REASONS.iter().map(|reason| reason.parts().1).collect();
        assert!(routes.iter().enumerate().all(|(at, route)| routes.iter().skip(at.saturating_add(1)).all(|other| other != route)), "経路は互いに違う");
        for (reason, lead) in [(Reason::Deadline, "撃ち直す（"), (Reason::Oversize, "分けて出す（"), (Reason::Identifier, "識別子を消し「隣の project」と件数に言い換えて出し直す（")] {
            assert!(reason.parts().1.starts_with(lead), "{reason:?}: {}", reason.parts().1);
        }
        assert_eq!(MARKS.iter().map(|mark| mark.as_str()).collect::<Vec<_>>(), ["wrapped", "verb", "shape", "dir", "redirect"], "印の宣言順");
        let (no_row, unresolved) = (Reason::NoRow.parts().1, Reason::Unresolved.parts().1);
        assert_eq!(no_row, Kind::Publish.route(), "no-row は種類の経路");
        assert_ne!(no_row, unresolved, "経路は理由ごと");
        assert!(unresolved.starts_with("解ける形で書き直す（"), "{unresolved}");
    }

    /// 行 n (e) 予算の読み: 埋め込みの 2 行で 6000 ms と 8388608 byte・行が無い / 不発効 / 整数でない / 0 / 10000 以上の締め切りは
    /// その行の id の `Err`（読む上限の行の不良は上限の行の id）。
    #[test]
    fn publish_limits_budget_reads_the_two_rows() {
        let embedded = Manifest::embedded().unwrap_or_else(|errors| panic!("埋め込み manifest を読める: {errors:?}"));
        assert_eq!(budget(&embedded), Ok((6000, 8_388_608)), "埋め込みの 2 行");
        let int = |id: &str, kind: &str, value: &str, enabled: bool| {
            format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n")
        };
        let deadline = |value: &str, enabled: bool| int(DEADLINE_ROW, "HostGuardPublishDeadlineMs", value, enabled);
        let bytes = |value: &str, enabled: bool| int(READ_ROW, "HostGuardPublishReadBytes", value, enabled);
        let (deadline_row, bytes_row) = (DEADLINE_ROW, READ_ROW);
        for (rows, want) in [
            (bytes("4096", true), Err(deadline_row)),
            (deadline("6000", true), Err(bytes_row)),
            (deadline("6000", false) + &bytes("4096", true), Err(deadline_row)),
            (deadline("6000", true) + &bytes("4096", false), Err(bytes_row)),
            (deadline("0", true) + &bytes("4096", true), Err(deadline_row)),
            (deadline("10000", true) + &bytes("4096", true), Err(deadline_row)),
            (deadline("9999", true) + &bytes("0", true), Err(bytes_row)),
            (deadline("9999", true) + &bytes("4096", true), Ok((9999, 4096))),
        ] {
            assert_eq!(budget(&manifest(&rows)), want, "{rows}");
        }
        let listed = row(DEADLINE_ROW, "HostGuardPublishDeadlineMs", "\"6000\"", true);
        assert!(Manifest::parse(&format!("schema = 1\n{listed}")).is_err(), "kind の形は Int（列の値は読み込みで拒む）");
    }

    /// 公開の segment ごとの解けない形の語（cwd `/w`・root `/root`）。
    fn holes(line: &str) -> Vec<Vec<&'static str>> {
        marked(line, Path::new("/w"), Path::new("/root")).iter().map(|seg| seg.holes.iter().map(|hole| hole.as_str()).collect()).collect()
    }

    /// 区切りを `open` で開く heredoc の値（外の二重引用つき・`tail` は `)` の後ろ）。
    fn heredoc(open: &str, body: &str, tail: &str) -> String {
        format!("\"$({open}\n{body}\nEOF\n){tail}\"")
    }

    /// enabled の行を持つ manifest の判定の hit（通す周は `None`）。
    fn hit(command: &str, enabled: bool) -> Option<String> {
        let with = manifest(&row("host_guard.publish", "HostGuardPublish", "\"form repo-name\"", enabled));
        let route = format!(" row=host_guard.publish ruling=r — {}", Reason::Unresolved.parts().1);
        denied(command, &with).map(|(_, text)| text.split_once(" hit=").map_or(text.clone(), |(_, tail)| tail.replace(&route, "")))
    }

    /// 行 k2 (a) 形の表: 5 値がそれぞれ先頭の形として当たり（3 つの flag の読みと `=` の読みを含む）、当たらない形は読めた
    /// segment 1 つで形を持たない。形の宣言順の語。
    #[test]
    fn publish_unresolved_forms_are_the_closed_five() {
        let table: [(&str, &[&str]); 5] = [
            ("variable-ref", &[
                "git push origin $B", "git push --repo=$R main", "gh pr create -R \"$R\"", "gh pr comment \"$N\" -b x", "GH_REPO=$X gh pr create",
                "gh repo edit o/n --visibility \"$V\"", "gh repo create x --public=$P", "gh api -X PATCH repos/$O/n -f a=b", "gh pr merge --squash \"$N\"",
                "gh pr comment -b x \"$N\"", "gh api --hostname \"$H\" repos/o/n -f a=b", "gh pr close -d \"$N\"", "git push origin `x`", "git push origin @{u}",
            ]),
            ("mirror", &["git push --mirror"]),
            ("stdin-body", &[
                "gh pr create --body-file -", "gh issue comment 1 -F/dev/stdin", "gh release create v1 --notes-file=/proc/self/fd/0",
                "gh api -X POST repos/o/n/issues -F body=@-", "gh release create v1 -d -F -", "gh pr create -d --body-file -", "gh pr review 1 --comment -F -",
                "gh pr merge 1 -d --body-file /dev/stdin", "gh api -X POST repos/o/n/issues -F body=@/dev/stdin", "gh api graphql --input -",
                "gh api -X POST repos/o/n/issues --field=body=@/proc/self/fd/0", "gh gist create", "gh gist create -d x -", "gh gist create /dev/fd/0",
                "gh gist create -d x", "gh gist create -d \"$X\"", "gh api -X POST repos/o/n/issues -F=body=@-", "gh issue comment 1 -F=-",
            ]),
            ("api-visibility-file", &[
                "gh api -X PATCH repos/o/n -F visibility=@v", "gh api -X PATCH repos/o/n --input v.json", "gh api orgs/x/repos -F private=@p",
                "gh api graphql -F query=@q.graphql", "gh api graphql --input q.json", "gh api user/repos -F private=@p",
                "gh api -X PATCH repos/o/n -f \"$K=public\"", "gh api -X PATCH repos/o/n -f \"$KV\"", "gh api -X=PATCH repos/o/n -F=visibility=@v",
            ]),
            ("unreadable-body", &[
                "gh pr create --body \"$X\"", "gh pr create -t 'costs $5'", "gh pr create --body-file <(cmd)", "gh api repos/o/n/issues -f body=\"$(cat x)\"",
                "gh gist create a.txt \"$F\"", "gh gist create -d \"$X\" a.txt",
            ]),
        ];
        for (want, lines) in table {
            for line in lines {
                assert_eq!(holes(line).into_iter().find_map(|seg| seg.first().copied()), Some(want), "{line}: {:?}", holes(line));
            }
        }
        let body = heredoc("cat <<'EOF'", "本文", "");
        for line in [
            "git push origin main".to_owned(), "git push -u origin feat/x".to_owned(), "gh pr create --body-file b.md --title x".to_owned(),
            "gh api repos/{owner}/{repo}/issues/1/comments -f body=x".to_owned(), "gh api -X PATCH repos/o/n -f visibility=private".to_owned(),
            "gh api -X POST repos/o/n/issues -F body=@b.md".to_owned(), "gh gist create a.txt".to_owned(), "gh gist create -d x a.txt".to_owned(),
            "gh pr close 1 -c done".to_owned(), format!("gh pr create -d --body {body}"), format!("gh pr close 1 -d -c {body}"),
            "gh api -X POST repos/o/n/issues --input i.json".to_owned(),
        ] {
            assert_eq!(holes(&line), [Vec::<&str>::new()], "読めた segment 1 つで形を持たない: {line}");
        }
        assert_eq!(super::HOLES.iter().map(|hole| hole.as_str()).collect::<Vec<_>>(), ["variable-ref", "mirror", "stdin-body", "api-visibility-file", "unreadable-body"]);
    }

    /// 行 k2 (b) 判定の順: enabled の行で segment の順に印 → 形の先に当たった 1 つ（hit と unresolved の経路）、`enabled = false` は通す。
    #[test]
    fn publish_unresolved_forms_follow_the_marks_only_when_enabled() {
        for (command, want) in [("git push --mirror", "unresolved:mirror"), ("git push --mirror; (git push)", "unresolved:mirror"), ("cd \"$D\" && git push --mirror", "unresolved:dir")] {
            assert_eq!(hit(command, true).as_deref(), Some(want), "{command}");
            assert_eq!(hit(command, false), None, "{command} enabled=false");
        }
    }

    /// 行 k2 (c) 区切りを引用した heredoc の本文だけが読める字面: 本文と api の欄は通り、台帳の notes と commit の message は印を
    /// 持たず、引用しない区切りと cat でない command と閉じの崩れと `$X` は unreadable-body、`bash -c` と `eval` の後ろは wrapped。
    #[test]
    fn publish_unresolved_heredoc_body_is_readable_only_with_a_quoted_delimiter() {
        let quoted = |body: &str| heredoc("cat <<'EOF'", body, "");
        assert_eq!(hit(&format!("gh pr create --title x --body {}", quoted("## 要約\n$HOME も字")), true), None, "本文");
        assert_eq!(hit(&format!("gh api -X PATCH repos/o/n/pulls/1 -f body={}", quoted("`x` と $Y")), true), None, "api の欄");
        let notes = quoted("gh pr merge 1 で着地");
        for line in [format!("scripts/bdw update s2-x --append-notes {notes}"), format!("scripts/bdw update s2-x --append-notes={notes}")] {
            assert!(marks(&line).is_empty(), "台帳の notes は印を持たない: {line}: {:?}", marks(&line));
        }
        let commit = format!("git commit -m {} && git push origin main", quoted("fix: cd sub"));
        assert!(marks(&commit).iter().all(Vec::is_empty) && hit(&commit, true).is_none(), "{commit}: {:?}", marks(&commit));
        for body in [
            heredoc("cat <<EOF", "x", ""), heredoc("cat <<-EOF", "x", ""), heredoc("cat <<'EOF'", "x", "x"), heredoc("cat <<'EOF'", "x\nEOF\nls", ""),
            heredoc("cat <<\"EOF\"", "x", ""), heredoc("sh <<'EOF'", "x", ""), heredoc("bash <<'EOF'", "x", ""), "\"$X\"".to_owned(),
        ] {
            let line = format!("gh pr create --body {body}");
            assert_eq!(hit(&line, true).as_deref(), Some("unresolved:unreadable-body"), "{line}");
        }
        for head in ["bash -c", "eval"] {
            let line = format!("{head} {}", quoted("git push --mirror origin"));
            assert_eq!(hit(&line, true).as_deref(), Some("unresolved:wrapped"), "{line}");
        }
    }

    /// 行 k3 (a) 広げた読み: 5 値の形が push と gh の動詞の後ろの全ての語に当たり、行 k2 の当たらない形の隣は読めた segment 1 つで
    /// 形を持たない。
    #[test]
    fn publish_widened_forms_reach_every_word_after_the_verb() {
        let table: [(&str, &[&str]); 5] = [
            ("variable-ref", &["git push --rep=$R main", "git push --force-with-lease=main:$S origin main"]),
            ("mirror", &["git push --m origin", "git push --mirr origin"]),
            ("stdin-body", &[
                "gh gist edit abc -", "gh gist edit abc /dev/stdin", "gh gist edit abc -a /dev/stdin", "gh release upload v1 /dev/stdin",
                "gh pr create --body-file /dev/./stdin", "gh issue comment 1 -F //dev/stdin", "gh api -X POST repos/o/n/issues -F body=@/proc/thread-self/fd/0",
            ]),
            ("api-visibility-file", &["gh api graphql -f \"$K=mutation{x}\""]),
            ("unreadable-body", &[
                "gh api graphql -f query=\"$(cat q.graphql)\"", "gh api graphql -f query=\"$Q\"", "gh release upload v1 <(cmd)",
                "gh pr create --body-file /<(cmd)", "gh issue comment 1 -e", "gh pr comment 1 --editor", "gh api repos/o/n/issues -f \"$KV\"",
                "gh api repos/o/n/issues -F \"$KV\"",
            ]),
        ];
        for (want, lines) in table {
            for line in lines {
                assert_eq!(holes(line).into_iter().find_map(|seg| seg.first().copied()), Some(want), "{line}: {:?}", holes(line));
            }
        }
        for line in [
            "git push --force-with-lease origin main", "git push -u origin feat/x", "gh pr create --body - --title x", "gh release upload v1 dist/a.tgz",
            "gh gist edit abc a.txt", "gh api graphql -f query=mutation{x}",
        ] {
            assert_eq!(holes(line), [Vec::<&str>::new()], "読めた segment 1 つで形を持たない: {line}");
        }
    }

    /// 行 k3 (b) graphql の書き: 変数の query と解けない key は `read` の列に api の書きとして加わり、読みの query と区切りを引用した
    /// heredoc の query は加わらず、行の無い manifest で変数の query は hit no-row。
    #[test]
    fn publish_widened_graphql_writes_with_unreadable_query_join_the_read() {
        use super::Sort;
        for line in ["gh api graphql -f query=\"$Q\"", "gh api graphql -f \"$K=x\""] {
            assert_eq!(published(line).iter().map(|seg| seg.sort).collect::<Vec<_>>(), [Sort::Api], "{line}");
        }
        let quoted = format!("gh api graphql -f query={}", heredoc("cat <<'EOF'", "{viewer{login}}", ""));
        for line in ["gh api graphql -f query={viewer{login}}", quoted.as_str()] {
            assert!(published(line).is_empty(), "書きでない: {line}: {:?}", shape(line));
        }
        let bare = denied("gh api graphql -f query=\"$Q\"", &manifest("")).map(|(_, text)| text);
        let want = format!("{NAME}: host-guard deny kind=publish hit=no-row row=host_guard.publish ruling=- — {}", Kind::Publish.route());
        assert_eq!(bare, Some(want), "行の無い manifest");
    }

    /// 行 l (d) 判定の順: 全履歴の段は解けない段より先（前の segment の解けない形より先）・`enabled = false` は通す・行の無い manifest は
    /// no-row・全履歴の断りの経路は「持ち主が手で行う（」で始まる。
    #[test]
    fn publish_history_is_judged_before_unresolved() {
        let line = |hit: &str, ruling: &str, route: &str| format!("{NAME}: host-guard deny kind=publish hit={hit} row=host_guard.publish ruling={ruling} — {route}");
        let route = Reason::FullHistory.parts().1;
        assert!(route.starts_with("持ち主が手で行う（"), "{route}");
        assert_eq!(Reason::FullHistory.parts().0, "full-history");
        let history = "git push origin $B; gh repo edit o/n --visibility public";
        let with = manifest(&row("host_guard.publish", "HostGuardPublish", "\"form repo-name\"", true));
        let want = ("host-guard-deny publish".to_owned(), line("full-history:repo-edit-public", "r", route));
        assert_eq!(denied(history, &with), Some(want), "段の順が segment の順より先");
        assert_eq!(hit("gh repo edit o/n --visibility private; git push origin $B", true).as_deref(), Some("unresolved:variable-ref"));
        assert_eq!(hit(history, false), None, "enabled = false は通す");
        let bare = denied(history, &manifest("")).map(|(_, text)| text);
        assert_eq!(bare, Some(line("no-row", "-", Kind::Publish.route())), "行の無い manifest");
    }

    /// 行 m2 (f) `elements` で読んだ要素の記号が名の当たりを決める（`form repo-name` で効き、`form object-id` だけでは当たらない）。
    #[test]
    fn publish_names_follow_the_row_elements() {
        use super::scan::{scan, Neighbor, Phrases, Public, Source, Text};
        let texts = [Text { source: Source::CommitMessage, body: "see proj-x" }];
        let near = [Neighbor { tag: "t".to_owned(), names: vec!["proj".to_owned()], ..Neighbor::default() }];
        let read = |value: &str| elements(&[value.to_owned()]).unwrap_or_default();
        let hit = |value: &str| scan(&read(value), &texts, &near, &Public::default(), &Phrases::default());
        assert_eq!(hit("form repo-name").as_deref(), Some("1:repo-name=proj@t"));
        assert_eq!(hit("form object-id"), None);
    }
}
